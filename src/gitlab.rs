use std::io::Cursor;

use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use url::Url;

use crate::tar::{StripComponents, TarFileIter};
use crate::template::TemplateFile;

/// Parsed GitLab URL from gitlab:// scheme
/// Format: gitlab://host/group/project[@ref]
#[derive(Debug)]
pub struct GitlabSource {
    pub host: String,
    pub project_path: String,
    pub git_ref: Option<String>,
}

impl GitlabSource {
    /// Parse a gitlab:// URL
    /// Examples:
    ///   gitlab://gitlab.com/group/project
    ///   gitlab://gitlab.com/group/subgroup/project@main
    ///   gitlab://gitlab.example.com/group/project@v1.0.0
    pub fn parse(source: &str) -> Result<Self> {
        let url = Url::parse(source).context("Invalid URL format")?;

        let host = url
            .host_str()
            .context("URL must contain a host")?
            .to_string();

        let path = url.path().trim_start_matches('/');
        if path.is_empty() {
            anyhow::bail!("Project path cannot be empty");
        }

        // Split off @ref from the end if present
        let (project_path, git_ref) = match path.rfind('@') {
            Some(pos) => (path[..pos].to_string(), Some(path[pos + 1..].to_string())),
            None => (path.to_string(), None),
        };

        Ok(Self {
            host,
            project_path,
            git_ref,
        })
    }

    /// Build the archive API URL
    pub fn archive_url(&self) -> String {
        // URL-encode the project path (e.g., "group/project" -> "group%2Fproject")
        let encoded_path = urlencoding::encode(&self.project_path);
        let base = format!(
            "https://{}/api/v4/projects/{}/repository/archive.tar.gz",
            self.host, encoded_path
        );
        match &self.git_ref {
            Some(r) => format!("{}?sha={}", base, urlencoding::encode(r)),
            None => base,
        }
    }
}

/// Fetch a GitLab repository archive and return an iterator over its files
pub fn fetch_archive(
    source: &str,
    token: Option<&str>,
) -> Result<impl Iterator<Item = Result<TemplateFile>> + use<>> {
    let source = GitlabSource::parse(source)?;

    let archive_url = source.archive_url();

    let client = reqwest::blocking::Client::new();
    let mut request = client.get(&archive_url);

    if let Some(t) = token {
        request = request.header("PRIVATE-TOKEN", t);
    }

    let response = request
        .send()
        .with_context(|| format!("Failed to fetch archive from {}", archive_url))?;

    if !response.status().is_success() {
        anyhow::bail!(
            "GitLab API '{}' returned error {}: {}",
            archive_url,
            response.status(),
            response.text().unwrap_or_default()
        );
    }

    let bytes = response.bytes().context("Failed to read response body")?;

    let decoder = GzDecoder::new(Cursor::new(bytes));
    let tar_iter = TarFileIter::new(decoder)?;

    // GitLab archives have a root folder like "project-branch-sha/"
    Ok(StripComponents::new(tar_iter, 1))
}
