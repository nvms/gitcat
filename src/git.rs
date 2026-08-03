use gix::bstr::ByteSlice;
use gix::diff::blob::unified_diff::{ConsumeHunk, ContextSize, DiffLineKind, HunkHeader};
use gix::diff::blob::{Algorithm, InternedInput, UnifiedDiff, diff_with_slider_heuristics};

/// Files larger than this are neither highlighted nor diffed - they are offered
/// as a raw download instead. A generated 40MB blob must not cost the server a
/// pass over every line.
pub const MAX_RENDER_BYTES: usize = 1 << 20;

/// How many bytes of a blob to inspect when deciding whether it is binary. Same
/// heuristic git uses: a NUL byte near the start means binary.
const BINARY_SNIFF_BYTES: usize = 8000;

/// Longest revision spec accepted from a URL.
pub const MAX_SPEC_LEN: usize = 200;

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("revision not found")]
    UnknownRevision,
    #[error("path not found in tree")]
    UnknownPath,
    #[error("failed to read repository: {0}")]
    Read(String),
}

fn read<E: std::fmt::Display>(e: E) -> GitError {
    GitError::Read(e.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub id: String,
    pub short_id: String,
    pub summary: String,
    pub body: Option<String>,
    pub author: String,
    pub email: String,
    pub seconds: i64,
    pub parents: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Directory,
    File,
    Executable,
    Symlink,
    Submodule,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeItem {
    pub name: String,
    pub kind: EntryKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blob {
    pub bytes: Vec<u8>,
    pub binary: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeStatus {
    Added,
    Deleted,
    Modified,
    Renamed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Add,
    Remove,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    pub text: String,
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    pub path: String,
    pub old_path: Option<String>,
    pub status: ChangeStatus,
    pub hunks: Vec<Hunk>,
    /// Set when the content was binary or too large to diff, in which case
    /// `hunks` is empty and the UI says so rather than rendering nothing.
    pub skipped: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefInfo {
    pub name: String,
    pub target: String,
}

pub fn resolve(repo: &gix::Repository, spec: &str) -> Result<gix::ObjectId, GitError> {
    if spec.is_empty() || spec.len() > MAX_SPEC_LEN {
        return Err(GitError::UnknownRevision);
    }

    repo.rev_parse_single(spec)
        .map(|id| id.detach())
        .map_err(|_| GitError::UnknownRevision)
}

/// The ref a bare repository's HEAD points at, used as the default revision.
pub fn default_branch(repo: &gix::Repository) -> String {
    repo.head_name()
        .ok()
        .flatten()
        .map(|name| name.shorten().to_string())
        .unwrap_or_else(|| "HEAD".to_owned())
}

pub fn branches(repo: &gix::Repository) -> Vec<RefInfo> {
    collect_refs(repo, "refs/heads/")
}

pub fn tags(repo: &gix::Repository) -> Vec<RefInfo> {
    collect_refs(repo, "refs/tags/")
}

fn collect_refs(repo: &gix::Repository, prefix: &str) -> Vec<RefInfo> {
    let Ok(platform) = repo.references() else {
        return Vec::new();
    };
    let Ok(iter) = platform.prefixed(prefix.as_bytes()) else {
        return Vec::new();
    };

    let mut refs: Vec<_> = iter
        .filter_map(Result::ok)
        .map(|r| RefInfo {
            name: r.name().shorten().to_string(),
            target: r.id().to_string(),
        })
        .collect();

    refs.sort_by(|a, b| a.name.cmp(&b.name));
    refs
}

pub fn commit(repo: &gix::Repository, id: gix::ObjectId) -> Result<Commit, GitError> {
    let commit = repo
        .find_commit(id)
        .map_err(|_| GitError::UnknownRevision)?;
    let author = commit.author().map_err(read)?;
    let message = commit.message().map_err(read)?;
    let body = message.body().map(|b| b.to_str_lossy().into_owned());

    Ok(Commit {
        id: commit.id().to_string(),
        short_id: commit
            .short_id()
            .map(|id| id.to_string())
            .unwrap_or_else(|_| commit.id().to_string()),
        summary: message.summary().to_string(),
        body: body.filter(|b| !b.trim().is_empty()),
        author: author.name.to_str_lossy().into_owned(),
        email: author.email.to_str_lossy().into_owned(),
        seconds: author.time().map(|t| t.seconds).unwrap_or_default(),
        parents: commit.parent_ids().map(|id| id.to_string()).collect(),
    })
}

/// Walks history from `start`, returning up to `limit` commits plus the id of
/// the next one, which the log view uses as a pagination cursor.
pub fn history(
    repo: &gix::Repository,
    start: gix::ObjectId,
    limit: usize,
) -> Result<(Vec<Commit>, Option<String>), GitError> {
    let walk = repo
        .rev_walk([start])
        .all()
        .map_err(|_| GitError::UnknownRevision)?;

    let mut commits = Vec::with_capacity(limit);
    let mut next = None;

    for info in walk.take(limit + 1) {
        let info = info.map_err(read)?;
        if commits.len() == limit {
            next = Some(info.id.to_string());
            break;
        }
        commits.push(commit(repo, info.id)?);
    }

    Ok((commits, next))
}

fn tree_at<'repo>(
    repo: &'repo gix::Repository,
    id: gix::ObjectId,
    path: &str,
) -> Result<gix::Tree<'repo>, GitError> {
    let mut tree = repo
        .find_commit(id)
        .map_err(|_| GitError::UnknownRevision)?
        .tree()
        .map_err(read)?;

    if path.is_empty() {
        return Ok(tree);
    }

    let entry = tree
        .peel_to_entry_by_path(path)
        .map_err(read)?
        .ok_or(GitError::UnknownPath)?;

    entry
        .object()
        .map_err(read)?
        .try_into_tree()
        .map_err(|_| GitError::UnknownPath)
}

pub fn tree(
    repo: &gix::Repository,
    id: gix::ObjectId,
    path: &str,
) -> Result<Vec<TreeItem>, GitError> {
    let tree = tree_at(repo, id, path)?;
    let mut items: Vec<TreeItem> = tree
        .iter()
        .filter_map(Result::ok)
        .map(|entry| {
            let entry = entry.inner;
            TreeItem {
                name: entry.filename.to_str_lossy().into_owned(),
                kind: match entry.mode.kind() {
                    gix::object::tree::EntryKind::Tree => EntryKind::Directory,
                    gix::object::tree::EntryKind::Blob => EntryKind::File,
                    gix::object::tree::EntryKind::BlobExecutable => EntryKind::Executable,
                    gix::object::tree::EntryKind::Link => EntryKind::Symlink,
                    gix::object::tree::EntryKind::Commit => EntryKind::Submodule,
                },
            }
        })
        .collect();

    items.sort_by(|a, b| {
        let dir = |k: EntryKind| k != EntryKind::Directory;
        dir(a.kind)
            .cmp(&dir(b.kind))
            .then_with(|| a.name.cmp(&b.name))
    });

    Ok(items)
}

pub fn blob(repo: &gix::Repository, id: gix::ObjectId, path: &str) -> Result<Blob, GitError> {
    if path.is_empty() {
        return Err(GitError::UnknownPath);
    }

    let mut tree = repo
        .find_commit(id)
        .map_err(|_| GitError::UnknownRevision)?
        .tree()
        .map_err(read)?;
    let entry = tree
        .peel_to_entry_by_path(path)
        .map_err(read)?
        .ok_or(GitError::UnknownPath)?;
    let object = entry.object().map_err(read)?;
    let blob = object.try_into_blob().map_err(|_| GitError::UnknownPath)?;
    let bytes = blob.data.clone();

    Ok(Blob {
        binary: is_binary(&bytes),
        bytes,
    })
}

pub fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(BINARY_SNIFF_BYTES).any(|&byte| byte == 0)
}

/// Diffs a commit against its first parent. A root commit is diffed against the
/// empty tree, so its every file shows up as added.
pub fn commit_diff(repo: &gix::Repository, id: gix::ObjectId) -> Result<Vec<FileDiff>, GitError> {
    let commit = repo
        .find_commit(id)
        .map_err(|_| GitError::UnknownRevision)?;
    let new_tree = commit.tree().map_err(read)?;
    let old_tree = commit
        .parent_ids()
        .next()
        .and_then(|parent| repo.find_commit(parent).ok())
        .and_then(|parent| parent.tree().ok());

    let changes = repo
        .diff_tree_to_tree(old_tree.as_ref(), Some(&new_tree), None)
        .map_err(read)?;

    let mut diffs = Vec::with_capacity(changes.len());
    for change in changes {
        diffs.push(file_diff(repo, change)?);
    }

    diffs.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(diffs)
}

fn file_diff(
    repo: &gix::Repository,
    change: gix::object::tree::diff::ChangeDetached,
) -> Result<FileDiff, GitError> {
    use gix::object::tree::diff::ChangeDetached as Change;

    let (path, old_path, status, old_id, new_id) = match change {
        Change::Addition { location, id, .. } => (
            location.to_string(),
            None,
            ChangeStatus::Added,
            None,
            Some(id),
        ),
        Change::Deletion { location, id, .. } => (
            location.to_string(),
            None,
            ChangeStatus::Deleted,
            Some(id),
            None,
        ),
        Change::Modification {
            location,
            previous_id,
            id,
            ..
        } => (
            location.to_string(),
            None,
            ChangeStatus::Modified,
            Some(previous_id),
            Some(id),
        ),
        Change::Rewrite {
            location,
            source_location,
            source_id,
            id,
            ..
        } => (
            location.to_string(),
            Some(source_location.to_string()),
            ChangeStatus::Renamed,
            Some(source_id),
            Some(id),
        ),
    };

    let old = load_text(repo, old_id);
    let new = load_text(repo, new_id);

    let (hunks, skipped) = match (old, new) {
        (Ok(old), Ok(new)) => (text_hunks(&old, &new)?, None),
        (Err(reason), _) | (_, Err(reason)) => (Vec::new(), Some(reason)),
    };

    Ok(FileDiff {
        path,
        old_path,
        status,
        hunks,
        skipped,
    })
}

fn load_text(repo: &gix::Repository, id: Option<gix::ObjectId>) -> Result<Vec<u8>, &'static str> {
    let Some(id) = id else {
        return Ok(Vec::new());
    };

    let object = repo.find_object(id).map_err(|_| "unreadable")?;
    let bytes = object
        .try_into_blob()
        .map_err(|_| "not a file")?
        .data
        .clone();

    if bytes.len() > MAX_RENDER_BYTES {
        return Err("file too large to diff");
    }
    if is_binary(&bytes) {
        return Err("binary file");
    }

    Ok(bytes)
}

fn text_hunks(old: &[u8], new: &[u8]) -> Result<Vec<Hunk>, GitError> {
    let input = InternedInput::new(old, new);
    let diff = diff_with_slider_heuristics(Algorithm::Histogram, &input);

    UnifiedDiff::new(
        &diff,
        &input,
        HunkCollector::default(),
        ContextSize::default(),
    )
    .consume()
    .map_err(read)
}

#[derive(Default)]
struct HunkCollector {
    hunks: Vec<Hunk>,
}

impl ConsumeHunk for HunkCollector {
    type Out = Vec<Hunk>;

    fn consume_hunk(
        &mut self,
        header: HunkHeader,
        lines: &[(DiffLineKind, &[u8])],
    ) -> std::io::Result<()> {
        let mut old_no = header.before_hunk_start;
        let mut new_no = header.after_hunk_start;
        let mut collected = Vec::with_capacity(lines.len());

        for (kind, bytes) in lines {
            let text = bytes.to_str_lossy().trim_end_matches('\n').to_owned();
            let line = match kind {
                DiffLineKind::Context => {
                    let line = DiffLine {
                        kind: LineKind::Context,
                        text,
                        old_no: Some(old_no),
                        new_no: Some(new_no),
                    };
                    old_no += 1;
                    new_no += 1;
                    line
                }
                DiffLineKind::Remove => {
                    let line = DiffLine {
                        kind: LineKind::Remove,
                        text,
                        old_no: Some(old_no),
                        new_no: None,
                    };
                    old_no += 1;
                    line
                }
                DiffLineKind::Add => {
                    let line = DiffLine {
                        kind: LineKind::Add,
                        text,
                        old_no: None,
                        new_no: Some(new_no),
                    };
                    new_no += 1;
                    line
                }
            };
            collected.push(line);
        }

        self.hunks.push(Hunk {
            header: format!(
                "@@ -{},{} +{},{} @@",
                header.before_hunk_start,
                header.before_hunk_len,
                header.after_hunk_start,
                header.after_hunk_len
            ),
            lines: collected,
        });

        Ok(())
    }

    fn finish(self) -> Self::Out {
        self.hunks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_binary_content() {
        assert!(!is_binary(b"plain text\nwith lines\n"));
        assert!(is_binary(b"has a \0 nul"));
        assert!(!is_binary(&[]));
    }

    #[test]
    fn numbers_lines_across_a_hunk() {
        let hunks = text_hunks(b"a\nb\nc\n", b"a\nB\nc\n").expect("diff");
        let lines = &hunks[0].lines;

        let removed: Vec<_> = lines
            .iter()
            .filter(|l| l.kind == LineKind::Remove)
            .collect();
        let added: Vec<_> = lines.iter().filter(|l| l.kind == LineKind::Add).collect();

        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].text, "b");
        assert_eq!(removed[0].old_no, Some(2));
        assert_eq!(removed[0].new_no, None);

        assert_eq!(added.len(), 1);
        assert_eq!(added[0].text, "B");
        assert_eq!(added[0].new_no, Some(2));
        assert_eq!(added[0].old_no, None);
    }

    #[test]
    fn identical_content_produces_no_hunks() {
        assert!(text_hunks(b"same\n", b"same\n").expect("diff").is_empty());
    }
}
