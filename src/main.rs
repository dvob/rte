mod dir;
mod gitlab;
mod tar;
mod template;

use std::fs::{self, File};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use flate2::read::GzDecoder;
use url::Url;

use crate::dir::{read_dir_iter, write_to_directory};
use crate::tar::{TarFileIter, is_tar_gz, write_to_tar_gz};
use crate::template::{SyntaxMode, TemplateConfig, TemplateFile, TemplatedFileIter};

#[derive(Parser)]
#[command(
    version,
    about = "Rusty Template Executor - bootstrap code projects based on templates"
)]
struct Cli {
    /// Path to parameter file (can be used multiple times, later files override earlier)
    #[arg(short, long = "parameters")]
    parameters: Vec<PathBuf>,

    /// Set a template parameter (can be used multiple times, always overrides file parameters)
    #[arg(short, long = "set", value_name = "KEY=VALUE", value_parser = parse_key_value)]
    set: Vec<(String, String)>,

    /// Write into an already existing directory as destination. Otherwise execution
    /// aborts if directory already exists.
    #[arg(short, long = "force", default_value_t = false)]
    force: bool,

    /// Use Backstage software template syntax (${{ }} instead of {{ }})
    #[arg(long = "backstage", default_value_t = false)]
    backstage: bool,

    /// Pass parameters at root level instead of under 'values' key
    #[arg(long = "parameters-on-root", default_value_t = false)]
    parameters_on_root: bool,

    /// GitLab personal access token (can also use GITLAB_TOKEN env var)
    #[arg(long = "gitlab-token", env = "GITLAB_TOKEN", hide_env_values = true)]
    gitlab_token: Option<String>,

    /// Source template (directory, .tar.gz archive, or gitlab:// URL)
    source: String,

    /// Destination for rendered template (directory or .tar.gz archive)
    destination: PathBuf,
}

fn parse_key_value(s: &str) -> Result<(String, String), String> {
    let pos = s.find('=').ok_or("expected format: KEY=VALUE")?;
    Ok((s[..pos].to_string(), s[pos + 1..].to_string()))
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Read and merge parameters from files (later files override earlier)
    let mut params = serde_json::Map::new();
    for path in &cli.parameters {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read parameters file: {}", path.display()))?;
        let file_params: serde_json::Value = serde_yaml::from_str(&content)
            .with_context(|| format!("Failed to parse parameters file: {}", path.display()))?;
        if let serde_json::Value::Object(map) = file_params {
            params.extend(map);
        }
    }

    // Apply --set key=value overrides (always have precedence)
    for (key, value) in &cli.set {
        params.insert(key.clone(), serde_json::Value::String(value.clone()));
    }

    let params = serde_json::Value::Object(params);

    // Determine source type: URL scheme or local path
    let template_source: Box<dyn Iterator<Item = Result<TemplateFile>>> =
        match Url::parse(&cli.source) {
            Ok(url) => match url.scheme() {
                "gitlab" => Box::new(gitlab::fetch_archive(
                    &cli.source,
                    cli.gitlab_token.as_deref(),
                )?),
                scheme => {
                    anyhow::bail!("unknown url scheme '{}'", scheme)
                }
            },
            Err(_) => {
                // Not a valid URL, treat as local path
                let source_path = PathBuf::from(&cli.source);
                if source_path.is_dir() {
                    Box::new(read_dir_iter(&source_path))
                } else {
                    let file = File::open(&source_path).with_context(|| {
                        format!("Failed to open archive: {}", source_path.display())
                    })?;
                    let decoder = GzDecoder::new(file);
                    Box::new(TarFileIter::new(decoder)?)
                }
            }
        };

    //
    // Configure templating
    //
    let syntax = if cli.backstage {
        SyntaxMode::Backstage
    } else {
        SyntaxMode::Jinja
    };

    let root_value = if cli.parameters_on_root {
        None
    } else {
        Some("values".to_owned())
    };

    let templated_files = TemplatedFileIter::with_config(
        template_source,
        params,
        TemplateConfig { syntax, root_value },
    );

    if is_tar_gz(&cli.destination) {
        write_to_tar_gz(&cli.destination, templated_files)?;
    } else {
        write_to_directory(&cli.destination, templated_files, cli.force)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests;
