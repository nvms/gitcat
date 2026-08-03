#![allow(dead_code)]

use std::path::Path;
use std::process::{Command, Output};

fn run(cwd: &Path, args: &[&str], date: Option<&str>) -> Output {
    let mut command = Command::new("git");
    command
        .args(args)
        .current_dir(cwd)
        .env("GIT_AUTHOR_NAME", "Test Author")
        .env("GIT_AUTHOR_EMAIL", "author@example.com")
        .env("GIT_COMMITTER_NAME", "Test Author")
        .env("GIT_COMMITTER_EMAIL", "author@example.com");

    if let Some(date) = date {
        command
            .env("GIT_AUTHOR_DATE", date)
            .env("GIT_COMMITTER_DATE", date);
    }

    let output = command
        .output()
        .expect("git is required to run these tests");

    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    output
}

pub fn git(cwd: &Path, args: &[&str]) {
    run(cwd, args, None);
}

pub fn capture(cwd: &Path, args: &[&str]) -> String {
    String::from_utf8_lossy(&run(cwd, args, None).stdout)
        .trim()
        .to_owned()
}

/// Commits with a fixed author and commit date so tests can assert on ordering
/// and relative times without depending on the wall clock.
pub fn commit(cwd: &Path, message: &str, timestamp: i64) {
    run(
        cwd,
        &["commit", "-m", message],
        Some(&format!("{timestamp} +0000")),
    );
}

/// Builds a bare repository at `root/name.git` with a single commit.
pub fn bare_repo_with_commit(root: &Path, name: &str, message: &str, timestamp: i64) {
    let work = root.join(format!("{name}-work"));
    std::fs::create_dir_all(&work).expect("create work tree");

    git(&work, &["init", "--initial-branch", "main", "."]);
    std::fs::write(work.join("README.md"), "hello\n").expect("write file");
    git(&work, &["add", "README.md"]);
    commit(&work, message, timestamp);

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
