use crate::dir::{read_dir_iter, write_file, write_to_directory};
use crate::tar::TarFileIter;
use crate::write_to_tar_gz;
use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;

use anyhow::Result;
use assert_cmd::Command;
use flate2::read::GzDecoder;

use crate::template::{SyntaxMode, TemplateConfig, TemplateFile, TemplatedFileIter};

/// Create an in-memory file iterator from a HashMap of path -> content
pub fn files_from_map(files: HashMap<&str, &str>) -> impl Iterator<Item = Result<TemplateFile>> {
    files.into_iter().map(|(path, content)| {
        Ok(TemplateFile {
            path: PathBuf::from(path),
            content: content.as_bytes().to_vec(),
        })
    })
}

/// Collect templated files into a HashMap for easy assertion
pub fn collect_to_map(
    iter: impl Iterator<Item = Result<TemplateFile>>,
) -> Result<HashMap<PathBuf, String>> {
    let mut result = HashMap::new();
    for file in iter {
        let file = file?;
        let content = String::from_utf8(file.content)
            .map_err(|e| anyhow::anyhow!("non-utf8 content: {}", e))?;
        result.insert(file.path, content);
    }
    Ok(result)
}

/// Returns (template, expected) HashMaps for testing
pub fn test_template() -> (
    HashMap<&'static str, &'static str>,
    HashMap<&'static str, &'static str>,
) {
    let template = HashMap::from([
        (
            "README.md",
            "# {{ values.project_name }}\n\nA project by {{ values.author }}.",
        ),
        (
            "src/main.rs",
            "fn main() {\n    println!(\"Hello from {{ values.project_name }}\");\n}",
        ),
        (
            "src/{{ values.project_name }}.rs",
            "// prepared file for {{ values.project_name }}",
        ),
    ]);
    let expected = HashMap::from([
        ("README.md", "# my-app\n\nA project by Alice."),
        (
            "src/main.rs",
            "fn main() {\n    println!(\"Hello from my-app\");\n}",
        ),
        ("src/my-app.rs", "// prepared file for my-app"),
    ]);
    (template, expected)
}

/// Convert expected HashMap to PathBuf keys for comparison
pub fn to_pathbuf_map(map: HashMap<&str, &str>) -> HashMap<PathBuf, String> {
    map.into_iter()
        .map(|(k, v)| (PathBuf::from(k), v.to_string()))
        .collect()
}

#[test]
fn test_cli_tar_to_dir() {
    let (template, expected) = test_template();
    let temp_dir = tempfile::tempdir().unwrap();

    // Write template.tar.gz using library functions
    let template_path = temp_dir.path().join("template.tar.gz");
    let source = files_from_map(template);
    write_to_tar_gz(&template_path, source).unwrap();

    // Write params.yaml (CLI wraps under 'values' automatically)
    let params_path = temp_dir.path().join("params.yaml");
    std::fs::write(&params_path, "project_name: my-app\nauthor: Alice\n").unwrap();

    // Run rte CLI
    let output_dir = temp_dir.path().join("output");
    Command::cargo_bin("rte")
        .unwrap()
        .args([
            "-p",
            params_path.to_str().unwrap(),
            template_path.to_str().unwrap(),
            output_dir.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Read output using library functions and compare
    let result = collect_to_map(read_dir_iter(&output_dir)).unwrap();
    assert_eq!(result, to_pathbuf_map(expected));
}

#[test]
fn test_cli_dir_to_tar() {
    let (template, expected) = test_template();
    let temp_dir = tempfile::tempdir().unwrap();

    // Write template directory
    let template_dir = temp_dir.path().join("template");
    for (path, content) in &template {
        let file_path = template_dir.join(path);
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, content).unwrap();
    }

    // Write params.yaml
    let params_path = temp_dir.path().join("params.yaml");
    std::fs::write(&params_path, "project_name: my-app\nauthor: Alice\n").unwrap();

    // Run rte CLI
    let output_path = temp_dir.path().join("output.tar.gz");
    Command::cargo_bin("rte")
        .unwrap()
        .args([
            "-p",
            params_path.to_str().unwrap(),
            template_dir.to_str().unwrap(),
            output_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Read output using library functions and compare
    let file = File::open(&output_path).unwrap();
    let decoder = GzDecoder::new(file);
    let tar_iter = TarFileIter::new(decoder).unwrap();
    let result = collect_to_map(tar_iter).unwrap();

    assert_eq!(result, to_pathbuf_map(expected));
}

#[test]
fn test_template_rendering() {
    let (template, expected) = test_template();
    let params = serde_json::json!({
            "project_name": "my-app",
            "author": "Alice"
    });

    let source = files_from_map(template);
    let templated = TemplatedFileIter::with_config(source, params, TemplateConfig::default());
    let result = collect_to_map(templated).unwrap();

    assert_eq!(result, to_pathbuf_map(expected));
}

#[test]
fn test_undefined_parameter_fails() {
    let files = HashMap::from([("file.txt", "Hello {{ missing_param }}")]);

    let params = serde_json::json!({});

    let source = files_from_map(files);
    let mut templated = TemplatedFileIter::with_config(source, params, TemplateConfig::default());

    let result = templated.next().unwrap();
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("missing_param") || err.contains("undefined"));
}

#[test]
fn test_write_file_rejects_parent_dir() {
    let temp_dir = tempfile::tempdir().unwrap();
    let file = TemplateFile {
        path: PathBuf::from("../escape.txt"),
        content: b"evil content".to_vec(),
    };

    let result = write_file(temp_dir.path(), &file);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains(".."));
}

#[test]
fn test_write_to_tar_read_from_tar() {
    let (template, expected) = test_template();
    let params = serde_json::json!({
            "project_name": "my-app",
            "author": "Alice"
    });

    let temp_dir = tempfile::tempdir().unwrap();
    let tar_path = temp_dir.path().join("output.tar.gz");

    // Write templated files to tar
    let source = files_from_map(template);
    let templated = TemplatedFileIter::with_config(source, params, TemplateConfig::default());
    write_to_tar_gz(&tar_path, templated).unwrap();

    // Read back from tar
    let file = File::open(&tar_path).unwrap();
    let decoder = GzDecoder::new(file);
    let tar_iter = TarFileIter::new(decoder).unwrap();
    let result = collect_to_map(tar_iter).unwrap();

    assert_eq!(result, to_pathbuf_map(expected));
}

#[test]
fn test_write_to_dir_read_from_dir() {
    let (template, expected) = test_template();
    let params = serde_json::json!({
            "project_name": "my-app",
            "author": "Alice",
    });

    let temp_dir = tempfile::tempdir().unwrap();
    let output_dir = temp_dir.path().join("output");

    // Write templated files to directory
    let source = files_from_map(template);
    let templated = TemplatedFileIter::with_config(source, params, TemplateConfig::default());
    write_to_directory(&output_dir, templated, false).unwrap();

    // Read back from directory
    let dir_iter = read_dir_iter(&output_dir);
    let result = collect_to_map(dir_iter).unwrap();

    assert_eq!(result, to_pathbuf_map(expected));
}

/// Returns (template, expected) HashMaps for backstage syntax testing
pub fn backstage_test_template() -> (
    HashMap<&'static str, &'static str>,
    HashMap<&'static str, &'static str>,
) {
    let template = HashMap::from([
        (
            "README.md",
            "# ${{ values.project_name }}\n\nA project by ${{ values.author }}.",
        ),
        (
            "src/main.rs",
            "fn main() {\n    println!(\"Hello from ${{ values.project_name }}\");\n}",
        ),
        (
            "src/${{ values.project_name }}.rs",
            "// prepared file for ${{ values.project_name }}",
        ),
    ]);
    let expected = HashMap::from([
        ("README.md", "# my-app\n\nA project by Alice."),
        (
            "src/main.rs",
            "fn main() {\n    println!(\"Hello from my-app\");\n}",
        ),
        ("src/my-app.rs", "// prepared file for my-app"),
    ]);
    (template, expected)
}

#[test]
fn test_backstage_syntax() {
    let (template, expected) = backstage_test_template();
    let params = serde_json::json!({
            "project_name": "my-app",
            "author": "Alice"
    });

    let source = files_from_map(template);
    let templated = TemplatedFileIter::with_config(
        source,
        params,
        TemplateConfig {
            syntax: SyntaxMode::Backstage,
            root_value: Some("values".to_owned()),
        },
    );
    let result = collect_to_map(templated).unwrap();

    assert_eq!(result, to_pathbuf_map(expected));
}

#[test]
fn test_backstage_ignores_jinja_syntax() {
    // Backstage mode should NOT process standard {{ }} syntax
    let files = HashMap::from([(
        "file.txt",
        "Keep {{ this }} as-is, but render ${{ values.name }}",
    )]);

    let params = serde_json::json!({
        "name": "Bob"
    });

    let source = files_from_map(files);
    let templated = TemplatedFileIter::with_config(
        source,
        params,
        TemplateConfig {
            syntax: SyntaxMode::Backstage,
            root_value: Some("values".to_owned()),
        },
    );
    let result = collect_to_map(templated).unwrap();

    let expected: HashMap<PathBuf, String> = HashMap::from([(
        PathBuf::from("file.txt"),
        "Keep {{ this }} as-is, but render Bob".to_string(),
    )]);
    assert_eq!(result, expected);
}