use std::path::{Path, PathBuf};

use anyhow::{Context, bail};

#[derive(Debug, Clone)]
pub struct Config {
    /// Canonical path to the directory holding bare repositories. Canonical so
    /// that per-request containment checks are a prefix comparison.
    pub repos: PathBuf,
    pub site_name: String,
}

impl Config {
    pub fn new(repos: &Path, site_name: String) -> anyhow::Result<Self> {
        let repos = repos
            .canonicalize()
            .with_context(|| format!("repository directory {} is not readable", repos.display()))?;

        if !repos.is_dir() {
            bail!("{} is not a directory", repos.display());
        }

        Ok(Self { repos, site_name })
    }
}
