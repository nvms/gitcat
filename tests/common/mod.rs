use std::path::Path;
use std::process::Command;

pub fn git(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_AUTHOR_NAME", "Test Author")
        .env("GIT_AUTHOR_EMAIL", "author@example.com")
        .env("GIT_COMMITTER_NAME", "Test Author")
        .env("GIT_COMMITTER_EMAIL", "author@example.com")
        .output()
        .expect("git is required to run these tests");

    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Builds a bare repository at `root/name.git` with a single commit whose
/// author and commit date are `timestamp` seconds past the epoch.
pub fn bare_repo_with_commit(root: &Path, name: &str, message: &str, timestamp: i64) {
    let work = root.join(format!("{name}-work"));
    std::fs::create_dir_all(&work).expect("create work tree");

    git(&work, &["init", "--initial-branch", "main", "."]);
    std::fs::write(work.join("README.md"), "hello\n").expect("write file");
    git(&work, &["add", "README.md"]);

    let date = format!("{timestamp} +0000");
    Command::new("git")
        .args(["commit", "-m", message])
        .current_dir(&work)
        .env("GIT_AUTHOR_NAME", "Test Author")
        .env("GIT_AUTHOR_EMAIL", "author@example.com")
        .env("GIT_COMMITTER_NAME", "Test Author")
        .env("GIT_COMMITTER_EMAIL", "author@example.com")
        .env("GIT_AUTHOR_DATE", &date)
        .env("GIT_COMMITTER_DATE", &date)
        .output()
        .map(|o| assert!(o.status.success(), "commit failed"))
        .expect("commit");

    git(
        root,
        &[
            "clone",
            "--bare",
            work.to_str().expect("utf-8 path"),
            &format!("{name}.git"),
        ],
    );
    std::fs::remove_dir_all(&work).expect("remove work tree");
}

pub fn empty_bare_repo(root: &Path, name: &str) {
    git(root, &["init", "--bare", &format!("{name}.git")]);
}
