use std::fs;
use std::path::{Component, Path};

use anyhow::{Context, Result};
use walkdir::WalkDir;

use crate::template::TemplateFile;

pub fn read_dir_iter(dir: &Path) -> impl Iterator<Item = Result<TemplateFile>> + use<> {
    let base = dir.to_path_buf();
    WalkDir::new(dir)
        .into_iter()
        .filter_entry(|e| e.file_name() != ".git")
        .filter(|entry| entry.as_ref().map_or(true, |e| !e.file_type().is_dir()))
        .map(move |entry| {
            let entry = entry?;
            let path = entry.path();
            let relative_path = path
                .strip_prefix(&base)
                .with_context(|| {
                    format!("path {} not under base {}", path.display(), base.display())
                })?
                .to_path_buf();
            let content =
                fs::read(path).with_context(|| format!("Failed to read {}", path.display()))?;
            Ok(TemplateFile {
                path: relative_path,
                content,
            })
        })
}

pub fn write_to_directory(
    dest: &Path,
    files: impl Iterator<Item = Result<TemplateFile>>,
    force: bool,
) -> Result<()> {
    if dest.exists() && !force {
        anyhow::bail!(
            "Destination '{}' already exists. Use --force to overwrite.",
            dest.display()
        );
    }

    fs::create_dir_all(dest)
        .with_context(|| format!("Failed to create destination directory: {}", dest.display()))?;

    for file in files {
        let file = file?;
        write_file(dest, &file)?;
    }
    Ok(())
}

pub fn write_file(dest: &Path, file: &TemplateFile) -> Result<()> {
    let mut file_dst = dest.to_path_buf();
    {
        for part in file.path.components() {
            // Code adapted from https://github.com/alexcrichton/tar-rs/blob/d0261f1f6cc959ba0758e7236b3fd81e90dd1dc6/src/entry.rs#L382
            match part {
                Component::Prefix(..) | Component::RootDir | Component::CurDir => continue,
                Component::ParentDir => {
                    return Err(anyhow::anyhow!(
                        "invalid path '{}' containing ..",
                        file.path.display()
                    ));
                }
                Component::Normal(part) => file_dst.push(part),
            }
        }
    }

    // Skip cases where only slashes or '.' parts were seen, because
    // this is effectively an empty filename.
    if *dest == *file_dst {
        return Ok(());
    }

    // Skip entries without a parent (i.e. outside of FS root)
    let parent = match file_dst.parent() {
        Some(p) => p,
        None => return Err(anyhow::anyhow!("invalid path '{}'", file.path.display())),
    };

    fs::create_dir_all(parent)
        .with_context(|| format!("Failed to create parent directory: {}", parent.display()))?;
    fs::write(&file_dst, &file.content)
        .with_context(|| format!("failed to write file: {}", file_dst.display()))?;

    Ok(())
}
