use std::io::Cursor;

use anyhow::{Context, Result};
use flate2::read::GzDecoder;
use url::Url;

use crate::tar::{StripComponents, TarFileIter};
use crate::template::TemplateFile;

/// Parsed GitHub URL from github:// scheme
/// Format: github://host/owner/repo[@ref]
#[derive(Debug)]
pub struct GitHubSource {
    pub host: String,
    pub owner: String,
    pub repo: String,
    pub git_ref: Option<String>,
}

impl GitHubSource {
    /// Parse a github:// URL
    /// Examples:
    ///   github://github.com/owner/repo
    ///   github://github.com/owner/repo@main
    ///   github://github.com/owner/repo@v1.0.0
    ///   github://github.example.com/owner/repo@develop
    pub fn parse(source: &str) -> Result<Self> {
        // Replace github:// with https:// for parsing
        let https_url = source
            .strip_prefix("github://")
            .context("URL must start with github://")?;
        let https_url = format!("https://{}", https_url);

        let url = Url::parse(&https_url).context("Invalid URL format")?;

        let host = url
            .host_str()
            .context("URL must contain a host")?
            .to_string();

        let path = url.path().trim_start_matches('/');
        if path.is_empty() {
            anyhow::bail!("Project path cannot be empty");
        }

        // Split off @ref from the end if present
        let (path, git_ref) = match path.rfind('@') {
            Some(pos) => (path[..pos].to_string(), Some(path[pos + 1..].to_string())),
            None => (path.to_string(), None),
        };

        // Parse owner/repo from path
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() != 2 {
            anyhow::bail!("GitHub path must be owner/repo, got: {}", path);
        }

        Ok(Self {
            host,
            owner: parts[0].to_string(),
            repo: parts[1].to_string(),
            git_ref,
        })
    }

    /// Build the archive API URL
    pub fn archive_url(&self) -> String {
        let git_ref = &self.git_ref;
        let mut url = format!(
            "https://api.{}/repos/{}/{}/tarball",
            self.host, self.owner, self.repo
        );

        if let Some(git_ref) = git_ref {
            url.push_str(format!("/{}", &git_ref).as_str());
        }
        url
    }
}

/// Fetch a GitHub repository archive and return an iterator over its files
pub fn fetch_archive(
    source: &str,
    token: Option<&str>,
) -> Result<impl Iterator<Item = Result<TemplateFile>> + use<>> {
    let source = GitHubSource::parse(source)?;
    let archive_url = source.archive_url();

    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()?;

    let mut request = client.get(&archive_url);

    if let Some(t) = token {
        request = request.header("Authorization", format!("Bearer {}", t));
    }

    // GitHub requires User-Agent header
    request = request.header("User-Agent", "rte");

    let response = request
        .send()
        .with_context(|| format!("Failed to fetch archive from {}", archive_url))?;

    if !response.status().is_success() {
        anyhow::bail!(
            "GitHub API {} returned error {}: {}",
            archive_url,
            response.status(),
            response.text().unwrap_or_default()
        );
    }

    let bytes = response.bytes().context("Failed to read response body")?;

    let decoder = GzDecoder::new(Cursor::new(bytes));
    let tar_iter = TarFileIter::new(decoder)?;

    // GitHub archives have a root folder like "owner-repo-sha/"
    Ok(StripComponents::new(tar_iter, 1))
}
