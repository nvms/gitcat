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

/// What the UI should show under a changed path. Anything that is not a plain
/// text diff gets its own variant so the view can explain it instead of
/// rendering an empty diff or an internal error string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffBody {
    Text(Vec<Hunk>),
    /// A gitlink. The pointed-at commits live in the submodule's own
    /// repository, so there is nothing here to diff.
    Submodule {
        old: Option<String>,
        new: Option<String>,
    },
    Binary,
    TooLarge,
    Unreadable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    pub path: String,
    pub old_path: Option<String>,
    pub status: ChangeStatus,
    pub body: DiffBody,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiffStats {
    pub files: usize,
    pub added: usize,
    pub removed: usize,
}

impl FileDiff {
    /// Lines added and removed. A submodule, binary, oversized, or unreadable
    /// change has no line counts and contributes nothing.
    pub fn line_counts(&self) -> (usize, usize) {
        let DiffBody::Text(hunks) = &self.body else {
            return (0, 0);
        };

        hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .fold((0, 0), |(added, removed), line| match line.kind {
                LineKind::Add => (added + 1, removed),
                LineKind::Remove => (added, removed + 1),
                LineKind::Context => (added, removed),
            })
    }
}

pub fn stats(diffs: &[FileDiff]) -> DiffStats {
    diffs.iter().fold(DiffStats::default(), |totals, file| {
        let (added, removed) = file.line_counts();
        DiffStats {
            files: totals.files + 1,
            added: totals.added + added,
            removed: totals.removed + removed,
        }
    })
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Readme {
    /// Path from the repository root, so relative links inside the file can be
    /// resolved against the directory it lives in.
    pub path: String,
    pub text: String,
}

/// Finds a README in the tree at `path`, which is how a directory listing gets
/// one rendered underneath it. `README.md` wins over a bare `README`, and the
/// comparison is case-insensitive because repositories are inconsistent.
pub fn readme(repo: &gix::Repository, id: gix::ObjectId, path: &str) -> Option<Readme> {
    const PREFERRED: [&str; 4] = ["readme.md", "readme.markdown", "readme.txt", "readme"];

    let entries = tree(repo, id, path).ok()?;
    let name = PREFERRED.iter().find_map(|candidate| {
        entries
            .iter()
            .find(|item| {
                item.kind != EntryKind::Directory && item.name.to_ascii_lowercase() == *candidate
            })
            .map(|item| item.name.clone())
    })?;

    let full_path = if path.is_empty() {
        name
    } else {
        format!("{path}/{name}")
    };

    let blob = blob(repo, id, &full_path).ok()?;
    if blob.binary || blob.bytes.len() > MAX_RENDER_BYTES {
        return None;
    }

    Some(Readme {
        text: String::from_utf8_lossy(&blob.bytes).into_owned(),
        path: full_path,
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
        let changed = describe(change);
        if changed.is_directory() {
            continue;
        }
        diffs.push(file_diff(repo, changed)?);
    }

    diffs.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(diffs)
}

struct ChangedPath {
    path: String,
    old_path: Option<String>,
    status: ChangeStatus,
    old: Option<(gix::ObjectId, gix::object::tree::EntryKind)>,
    new: Option<(gix::ObjectId, gix::object::tree::EntryKind)>,
}

impl ChangedPath {
    /// gix reports every changed directory alongside the changed files inside
    /// it, so a commit touching `a/b.txt` also yields a change for `a`. The
    /// directory has no diff of its own and its contents are already listed,
    /// so it is dropped - git's own output does the same.
    fn is_directory(&self) -> bool {
        use gix::object::tree::EntryKind::Tree;

        matches!(self.new, Some((_, Tree))) || matches!(self.old, Some((_, Tree)))
    }
}

fn describe(change: gix::object::tree::diff::ChangeDetached) -> ChangedPath {
    use gix::object::tree::diff::ChangeDetached as Change;

    match change {
        Change::Addition {
            location,
            id,
            entry_mode,
            ..
        } => ChangedPath {
            path: location.to_string(),
            old_path: None,
            status: ChangeStatus::Added,
            old: None,
            new: Some((id, entry_mode.kind())),
        },
        Change::Deletion {
            location,
            id,
            entry_mode,
            ..
        } => ChangedPath {
            path: location.to_string(),
            old_path: None,
            status: ChangeStatus::Deleted,
            old: Some((id, entry_mode.kind())),
            new: None,
        },
        Change::Modification {
            location,
            previous_id,
            previous_entry_mode,
            id,
            entry_mode,
            ..
        } => ChangedPath {
            path: location.to_string(),
            old_path: None,
            status: ChangeStatus::Modified,
            old: Some((previous_id, previous_entry_mode.kind())),
            new: Some((id, entry_mode.kind())),
        },
        Change::Rewrite {
            location,
            source_location,
            source_id,
            source_entry_mode,
            id,
            entry_mode,
            ..
        } => ChangedPath {
            path: location.to_string(),
            old_path: Some(source_location.to_string()),
            status: ChangeStatus::Renamed,
            old: Some((source_id, source_entry_mode.kind())),
            new: Some((id, entry_mode.kind())),
        },
    }
}

fn file_diff(repo: &gix::Repository, changed: ChangedPath) -> Result<FileDiff, GitError> {
    let body = diff_body(repo, &changed)?;

    Ok(FileDiff {
        path: changed.path,
        old_path: changed.old_path,
        status: changed.status,
        body,
    })
}

fn diff_body(repo: &gix::Repository, changed: &ChangedPath) -> Result<DiffBody, GitError> {
    use gix::object::tree::EntryKind::Commit;

    let is_submodule = |side: &Option<(gix::ObjectId, gix::object::tree::EntryKind)>| {
        matches!(side, Some((_, Commit)))
    };

    if is_submodule(&changed.old) || is_submodule(&changed.new) {
        return Ok(DiffBody::Submodule {
            old: changed.old.map(|(id, _)| id.to_string()),
            new: changed.new.map(|(id, _)| id.to_string()),
        });
    }

    let old = load_text(repo, changed.old.map(|(id, _)| id));
    let new = load_text(repo, changed.new.map(|(id, _)| id));

    match (old, new) {
        (Ok(old), Ok(new)) => Ok(DiffBody::Text(text_hunks(&old, &new)?)),
        (Err(reason), _) | (_, Err(reason)) => Ok(reason),
    }
}

fn load_text(repo: &gix::Repository, id: Option<gix::ObjectId>) -> Result<Vec<u8>, DiffBody> {
    let Some(id) = id else {
        return Ok(Vec::new());
    };

    let object = repo.find_object(id).map_err(|_| DiffBody::Unreadable)?;
    let bytes = object
        .try_into_blob()
        .map_err(|_| DiffBody::Unreadable)?
        .data
        .clone();

    if bytes.len() > MAX_RENDER_BYTES {
        return Err(DiffBody::TooLarge);
    }
    if is_binary(&bytes) {
        return Err(DiffBody::Binary);
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
