use std::fs::File;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};
use flate2::write::GzEncoder;
use flate2::Compression;
use tar::{Archive, Builder, Entries};

use crate::template::TemplateFile;

pub fn is_tar_gz(path: &Path) -> bool {
    path.to_string_lossy().ends_with(".tar.gz")
}

/// An owning iterator over tar archive entries.
///
/// This struct holds both the Archive and its Entries iterator together,
/// solving the "lending iterator" problem by boxing the archive to give
/// it a stable address.
pub struct TarFileIter<R: Read + 'static> {
    // Archive is boxed so it has a stable memory address.
    // We keep it alive but access it through entries.
    #[allow(dead_code)]
    archive: Box<Archive<R>>,
    entries: Entries<'static, R>,
}

impl<R: Read + 'static> TarFileIter<R> {
    pub fn new(reader: R) -> Result<Self> {
        let archive = Box::new(Archive::new(reader));

        // SAFETY: We're creating a self-referential struct here.
        // This is sound because:
        // 1. Archive is boxed, so it has a stable address
        // 2. We keep archive alive for the lifetime of this struct
        // 3. entries is dropped before archive (Rust drops fields in declaration order)
        // 4. The 'static lifetime is a lie, but entries won't outlive archive
        let entries: Entries<'static, R> = unsafe {
            let archive_ptr: *mut Archive<R> = &*archive as *const _ as *mut _;
            (*archive_ptr).entries()?
        };

        Ok(Self { archive, entries })
    }
}

impl<R: Read + 'static> Iterator for TarFileIter<R> {
    type Item = Result<TemplateFile>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let entry = match self.entries.next()? {
                Ok(e) => e,
                Err(e) => return Some(Err(e.into())),
            };

            // Skip directories
            if entry.header().entry_type().is_dir() {
                continue;
            }

            let mut entry = entry;
            let path = match entry.path() {
                Ok(p) => p.to_path_buf(),
                Err(e) => return Some(Err(e.into())),
            };

            let mut content = Vec::new();
            if let Err(e) = entry.read_to_end(&mut content) {
                return Some(Err(e.into()));
            }

            return Some(Ok(TemplateFile { path, content }));
        }
    }
}

/// Iterator wrapper that strips leading path components from file paths.
/// Useful for archives that contain a root folder prefix (e.g., project-branch-sha/).
pub struct StripComponents<I> {
    inner: I,
    strip_count: usize,
}

impl<I> StripComponents<I> {
    pub fn new(inner: I, strip_count: usize) -> Self {
        Self { inner, strip_count }
    }
}

impl<I: Iterator<Item = Result<TemplateFile>>> Iterator for StripComponents<I> {
    type Item = Result<TemplateFile>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let file = match self.inner.next()? {
                Ok(f) => f,
                Err(e) => return Some(Err(e)),
            };

            // Strip the first N components from the path
            let components: Vec<_> = file.path.components().collect();
            if components.len() <= self.strip_count {
                // Skip entries that would become empty after stripping
                continue;
            }

            let new_path: std::path::PathBuf =
                components.into_iter().skip(self.strip_count).collect();

            return Some(Ok(TemplateFile {
                path: new_path,
                content: file.content,
            }));
        }
    }
}

pub fn write_to_tar_gz(dest: &Path, files: impl Iterator<Item = Result<TemplateFile>>) -> Result<()> {
    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("Failed to create parent directory: {}", parent.display())
            })?;
        }
    }

    let file = File::create(dest)
        .with_context(|| format!("Failed to create archive: {}", dest.display()))?;
    let encoder = GzEncoder::new(file, Compression::default());
    let mut tar = Builder::new(encoder);

    for file in files {
        let file = file?;
        let mut header = tar::Header::new_gnu();
        header.set_size(file.content.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, &file.path, file.content.as_slice())
            .with_context(|| format!("Failed to add file to archive: {}", file.path.display()))?;
    }

    tar.finish()
        .with_context(|| "Failed to finalize tar archive")?;
    Ok(())
}
