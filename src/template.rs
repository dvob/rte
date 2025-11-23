use std::path::PathBuf;

use anyhow::Result;
use minijinja::syntax::SyntaxConfig;
use minijinja::{Environment, UndefinedBehavior};

#[derive(Debug)]
pub struct TemplateFile {
    pub path: PathBuf,
    pub content: Vec<u8>,
}

/// Syntax mode for template delimiters
#[derive(Debug, Clone, Copy, Default)]
pub enum SyntaxMode {
    /// Standard Jinja2 syntax: {{ }} and {% %}
    #[default]
    Jinja,
    /// Backstage software templates syntax: ${{ }} and ${% %}
    Backstage,
}

pub struct TemplateConfig {
    pub syntax: SyntaxMode,
    pub root_value: Option<String>,
}

impl Default for TemplateConfig {
    fn default() -> Self {
        Self {
            syntax: SyntaxMode::Jinja,
            root_value: Some("values".to_owned()),
        }
    }
}

/// Iterator that applies templating to files
pub struct TemplatedFileIter<I> {
    inner: I,
    env: Environment<'static>,
    params: serde_json::Value,
}

impl<I> TemplatedFileIter<I> {
    pub fn with_config(inner: I, params: serde_json::Value, config: TemplateConfig) -> Self {
        let mut env = Environment::new();
        env.set_undefined_behavior(UndefinedBehavior::Strict);
        env.set_debug(true);

        if let SyntaxMode::Backstage = config.syntax {
            // https://github.com/backstage/backstage/blob/9e88165368eafc6744b8c41c9912260e853ec11b/plugins/scaffolder-backend/src/lib/templating/SecureTemplater.ts#L40
            let syntax_config = SyntaxConfig::builder()
                .variable_delimiters("${{", "}}")
                .build()
                .expect("valid backstage syntax config");
            env.set_syntax(syntax_config);
        }

        // Wrap params under root_value key if specified
        let params = match config.root_value {
            Some(key) => serde_json::json!({ key: params }),
            None => params,
        };

        Self { inner, env, params }
    }
}

impl<I: Iterator<Item = Result<TemplateFile>>> Iterator for TemplatedFileIter<I> {
    type Item = Result<TemplateFile>;

    fn next(&mut self) -> Option<Self::Item> {
        let file = match self.inner.next()? {
            Ok(f) => f,
            Err(e) => return Some(Err(e)),
        };

        // we are only able to run utf8 through the templating engine, but not all paths are valid utf8
        let path = match file.path.to_str() {
            Some(path) => path,
            None => {
                return Some(Err(anyhow::anyhow!(
                    "invalid path '{}' is not UTF8",
                    file.path.display(),
                )));
            }
        };

        // Render the path
        let rendered_path = match self
            .env
            .template_from_str(path)
            .and_then(|t| t.render(&self.params))
        {
            Ok(p) => p,
            Err(e) => {
                return Some(Err(anyhow::anyhow!(
                    "failed to render path '{}': {:#}",
                    file.path.display(),
                    e
                )));
            }
        };

        // Try to render content as UTF-8 template, otherwise keep as binary
        let rendered_content = match std::str::from_utf8(&file.content) {
            Err(e) => {
                return Some(Err(anyhow::anyhow!(
                    "file '{}' is not a UTF8 text file: {}",
                    file.path.display(),
                    e,
                )));
            }
            Ok(content) => match self
                .env
                .template_from_str(content)
                .and_then(|t| t.render(&self.params))
            {
                Ok(rendered_content) => rendered_content.into_bytes(),
                Err(e) => {
                    return Some(Err(anyhow::anyhow!(
                        "template execution for '{}' failed: {:#}",
                        file.path.display(),
                        e
                    )));
                }
            },
        };

        Some(Ok(TemplateFile {
            path: rendered_path.into(),
            content: rendered_content,
        }))
    }
}
