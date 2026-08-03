use std::path::{Path, PathBuf};

/// Longest repository name gitcat will accept.
pub const MAX_NAME_LEN: usize = 100;

const DEFAULT_DESCRIPTION_PREFIX: &str = "Unnamed repository";

#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    #[error("invalid repository name")]
    InvalidName,
    #[error("repository not found")]
    NotFound,
    #[error("failed to read repository directory: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to open repository: {0}")]
    Open(#[from] Box<gix::open::Error>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitSummary {
    pub id: String,
    pub summary: String,
    pub author: String,
    pub seconds: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoEntry {
    pub name: String,
    pub description: Option<String>,
    pub head: Option<CommitSummary>,
}

/// Accepts a repository name with or without the `.git` suffix and returns the
/// bare name. Anything outside `[A-Za-z0-9._-]` is rejected, as is a leading
/// `.` or `-`, which keeps `..` and option-lookalikes out of every path we build.
pub fn validate_name(name: &str) -> Result<&str, RepoError> {
    let name = name.strip_suffix(".git").unwrap_or(name);

    if name.is_empty() || name.len() > MAX_NAME_LEN {
        return Err(RepoError::InvalidName);
    }
    if name.starts_with('.') || name.starts_with('-') {
        return Err(RepoError::InvalidName);
    }
    if !name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return Err(RepoError::InvalidName);
    }

    Ok(name)
}

/// Resolves a validated name to a directory inside `root`. `root` must already
/// be canonical; the resolved path is canonicalized and checked for containment
/// so a symlink cannot point the server at a directory outside the repo root.
pub fn resolve_path(root: &Path, name: &str) -> Result<PathBuf, RepoError> {
    let name = validate_name(name)?;
    let path = root
        .join(format!("{name}.git"))
        .canonicalize()
        .map_err(|_| RepoError::NotFound)?;

    if !path.starts_with(root) {
        return Err(RepoError::NotFound);
    }

    Ok(path)
}

pub fn open(root: &Path, name: &str) -> Result<gix::Repository, RepoError> {
    let path = resolve_path(root, name)?;
    gix::open(&path).map_err(|e| RepoError::Open(Box::new(e)))
}

/// Scans `root` for bare repositories. Entries that fail to open are skipped
/// rather than failing the whole listing - one broken repo should not take the
/// index page down.
pub fn discover(root: &Path) -> Result<Vec<RepoEntry>, RepoError> {
    let mut entries = Vec::new();

    for dir_entry in std::fs::read_dir(root)? {
        let dir_entry = match dir_entry {
            Ok(e) => e,
            Err(e) => {
                tracing::debug!(error = %e, "skipping unreadable directory entry");
                continue;
            }
        };

        let file_name = dir_entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        if !file_name.ends_with(".git") || !dir_entry.path().is_dir() {
            continue;
        }
        let Ok(name) = validate_name(file_name) else {
            tracing::debug!(name = file_name, "skipping repository with invalid name");
            continue;
        };

        match read_entry(&dir_entry.path(), name) {
            Ok(entry) => entries.push(entry),
            Err(e) => tracing::debug!(name, error = %e, "skipping unreadable repository"),
        }
    }

    entries.sort_by(|a, b| {
        let time = b.head.as_ref().map(|c| c.seconds);
        time.cmp(&a.head.as_ref().map(|c| c.seconds))
            .then_with(|| a.name.cmp(&b.name))
    });

    Ok(entries)
}

fn read_entry(path: &Path, name: &str) -> Result<RepoEntry, RepoError> {
    let repo = gix::open(path).map_err(|e| RepoError::Open(Box::new(e)))?;

    Ok(RepoEntry {
        name: name.to_owned(),
        description: read_description(path),
        head: head_summary(&repo),
    })
}

fn read_description(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path.join("description")).ok()?;
    let text = text.trim();

    if text.is_empty() || text.starts_with(DEFAULT_DESCRIPTION_PREFIX) {
        return None;
    }

    Some(text.to_owned())
}

/// `None` for a repository with no commits yet, which is the normal state of a
/// freshly created remote.
pub fn head_summary(repo: &gix::Repository) -> Option<CommitSummary> {
    let commit = repo.head_commit().ok()?;
    let author = commit.author().ok()?;

    Some(CommitSummary {
        id: commit
            .short_id()
            .map(|id| id.to_string())
            .unwrap_or_else(|_| commit.id().to_string()),
        summary: commit
            .message()
            .ok()
            .map(|m| m.summary().to_string())
            .unwrap_or_default(),
        author: author.name.to_string(),
        seconds: author.time().ok().map(|t| t.seconds).unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plain_names_with_or_without_suffix() {
        assert_eq!(validate_name("gitcat").unwrap(), "gitcat");
        assert_eq!(validate_name("gitcat.git").unwrap(), "gitcat");
        assert_eq!(validate_name("my_repo-2.0").unwrap(), "my_repo-2.0");
    }

    #[test]
    fn rejects_traversal_and_separators() {
        for name in [
            "..",
            "../etc",
            "..git",
            "a/b",
            "a\\b",
            "/etc/passwd",
            ".ssh",
            "-rf",
            "",
            ".git",
            "a b",
            "réf",
            "a\0b",
        ] {
            assert!(validate_name(name).is_err(), "should reject {name:?}");
        }
    }

    #[test]
    fn rejects_overlong_names() {
        let long = "a".repeat(MAX_NAME_LEN + 1);
        assert!(validate_name(&long).is_err());
        assert!(validate_name(&"a".repeat(MAX_NAME_LEN)).is_ok());
    }
}
