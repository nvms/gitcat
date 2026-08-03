use std::path::{Path, PathBuf};

use anyhow::{Context, bail};

#[derive(Debug, Clone)]
pub struct Config {
    /// Canonical path to the directory holding bare repositories. Canonical so
    /// that per-request containment checks are a prefix comparison.
    pub repos: PathBuf,
    pub site_name: String,
    /// Origin used to build clone URLs. When unset the request's Host header is
    /// used, which is what makes the shown URL correct behind a proxy.
    pub base_url: Option<String>,
}

impl Config {
    pub fn new(repos: &Path, site_name: String) -> anyhow::Result<Self> {
        let repos = repos
            .canonicalize()
            .with_context(|| format!("repository directory {} is not readable", repos.display()))?;

        if !repos.is_dir() {
            bail!("{} is not a directory", repos.display());
        }

        Ok(Self {
            repos,
            site_name,
            base_url: None,
        })
    }

    pub fn with_base_url(mut self, base_url: Option<String>) -> Self {
        self.base_url = base_url;
        self
    }

    /// Falls back to the request's Host header so a clone URL is right whatever
    /// name the server is reached by.
    pub fn origin_for(&self, host: Option<&str>) -> String {
        match (&self.base_url, host) {
            (Some(base), _) => base.trim_end_matches('/').to_owned(),
            (None, Some(host)) => format!("http://{host}"),
            (None, None) => String::new(),
        }
    }
}
