mod common;

use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use gitcat::{Config, web};
use http_body_util::BodyExt;
use tower::ServiceExt;

async fn get(root: &Path, uri: &str) -> (StatusCode, String) {
    let config = Arc::new(Config::new(root, "test".to_owned()).expect("config"));
    let response = web::router(config)
        .oneshot(
            Request::builder()
                .uri(uri)
                .header("host", "git.example.com")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();

    (status, String::from_utf8_lossy(&body).into_owned())
}

struct Fixture {
    root_commit: String,
    head: String,
}

/// A repository with two commits: the second edits a file, adds one, and
/// deletes another, so a single diff exercises every change status.
fn fixture(root: &Path) -> Fixture {
    let work = root.join("work");
    std::fs::create_dir_all(&work).expect("create work tree");

    common::git(&work, &["init", "--initial-branch", "main", "."]);
    std::fs::write(work.join("keep.txt"), "one\ntwo\nthree\n").expect("write");
    std::fs::write(work.join("gone.txt"), "delete me\n").expect("write");
    std::fs::create_dir(work.join("src")).expect("mkdir");
    std::fs::write(work.join("src/main.rs"), "fn main() {}\n").expect("write");
    common::git(&work, &["add", "."]);
    common::commit(&work, "initial commit", 1_700_000_000);
    let root_commit = common::capture(&work, &["rev-parse", "HEAD"]);

    std::fs::write(work.join("keep.txt"), "one\nTWO\nthree\n").expect("write");
    std::fs::remove_file(work.join("gone.txt")).expect("remove");
    std::fs::write(work.join("added.txt"), "brand new\n").expect("write");
    common::git(&work, &["add", "-A"]);
    common::commit(&work, "second commit", 1_700_000_100);
    let head = common::capture(&work, &["rev-parse", "HEAD"]);

    common::git(&work, &["tag", "v1.0.0"]);
    common::git(
        root,
        &["clone", "--bare", work.to_str().expect("utf-8"), "demo.git"],
    );
    std::fs::remove_dir_all(&work).expect("cleanup");

    Fixture { root_commit, head }
}

#[tokio::test]
async fn summary_shows_clone_url_commits_and_refs() {
    let tmp = tempfile::tempdir().expect("tempdir");
    fixture(tmp.path());

    let (status, body) = get(tmp.path(), "/demo").await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("git clone http://"));
    assert!(body.contains("demo.git"));
    assert!(body.contains("second commit"));
    assert!(body.contains("initial commit"));
    assert!(body.contains("v1.0.0"));
    assert!(body.contains("main"));
}

#[tokio::test]
async fn log_lists_history() {
    let tmp = tempfile::tempdir().expect("tempdir");
    fixture(tmp.path());

    let (status, body) = get(tmp.path(), "/demo/log/main").await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("second commit"));
    assert!(body.contains("initial commit"));
}

#[tokio::test]
async fn commit_diff_reports_every_change_status() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let fixture = fixture(tmp.path());

    let (status, body) = get(tmp.path(), &format!("/demo/commit/{}", fixture.head)).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("second commit"));
    assert!(body.contains("modified"));
    assert!(body.contains("added"));
    assert!(body.contains("deleted"));
    assert!(body.contains("keep.txt"));
    assert!(body.contains("+TWO"));
    assert!(body.contains("-two"));
}

#[tokio::test]
async fn root_commit_diffs_against_the_empty_tree() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let fixture = fixture(tmp.path());

    let (status, body) = get(tmp.path(), &format!("/demo/commit/{}", fixture.root_commit)).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("initial commit"));
    assert!(body.contains("added"));
    assert!(!body.contains("deleted"));
    assert!(body.contains("+one"));
}

#[tokio::test]
async fn tree_lists_directories_before_files() {
    let tmp = tempfile::tempdir().expect("tempdir");
    fixture(tmp.path());

    let (status, body) = get(tmp.path(), "/demo/tree/main").await;

    assert_eq!(status, StatusCode::OK);

    let src = body.find("src/").expect("directory entry");
    let keep = body.find("keep.txt").expect("file entry");
    assert!(src < keep, "directories should sort first");
}

#[tokio::test]
async fn tree_navigates_into_a_subdirectory() {
    let tmp = tempfile::tempdir().expect("tempdir");
    fixture(tmp.path());

    let (status, body) = get(tmp.path(), "/demo/tree/main/src").await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("main.rs"));
    assert!(body.contains(".."));
}

#[tokio::test]
async fn blob_shows_file_contents_and_raw_serves_bytes() {
    let tmp = tempfile::tempdir().expect("tempdir");
    fixture(tmp.path());

    let (status, body) = get(tmp.path(), "/demo/blob/main/src/main.rs").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("fn main() {}"));

    let (status, raw) = get(tmp.path(), "/demo/raw/main/src/main.rs").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(raw, "fn main() {}\n");
}

#[tokio::test]
async fn unknown_revisions_and_paths_are_404() {
    let tmp = tempfile::tempdir().expect("tempdir");
    fixture(tmp.path());

    for uri in [
        "/demo/tree/nope",
        "/demo/blob/main/does-not-exist.txt",
        "/demo/log/nope",
        "/demo/commit/0000000000000000000000000000000000000000",
        "/missing",
        "/demo/tree/main/keep.txt",
    ] {
        let (status, _) = get(tmp.path(), uri).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri} should 404");
    }
}

#[tokio::test]
async fn a_traversal_attempt_in_the_repo_name_is_404() {
    let tmp = tempfile::tempdir().expect("tempdir");
    fixture(tmp.path());
    std::fs::write(tmp.path().join("secret"), "top secret").expect("write");

    for uri in ["/..%2Fsecret", "/.%2E/secret", "/demo%2F..%2Fsecret"] {
        let (status, _) = get(tmp.path(), uri).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri} should 404");
    }
}

#[tokio::test]
async fn raw_output_cannot_be_sniffed_as_html() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let work = root.join("work");
    std::fs::create_dir_all(&work).expect("create work tree");

    common::git(&work, &["init", "--initial-branch", "main", "."]);
    std::fs::write(work.join("evil.html"), "<script>alert(1)</script>").expect("write");
    common::git(&work, &["add", "."]);
    common::commit(&work, "add page", 1_700_000_000);
    common::git(
        root,
        &["clone", "--bare", work.to_str().expect("utf-8"), "demo.git"],
    );
    std::fs::remove_dir_all(&work).expect("cleanup");

    let config = Arc::new(Config::new(root, "test".to_owned()).expect("config"));
    let response = web::router(config)
        .oneshot(
            Request::builder()
                .uri("/demo/raw/main/evil.html")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(
        response.headers()["content-type"],
        "text/plain; charset=utf-8"
    );
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
}

#[tokio::test]
async fn blob_view_escapes_file_contents() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let work = root.join("work");
    std::fs::create_dir_all(&work).expect("create work tree");

    common::git(&work, &["init", "--initial-branch", "main", "."]);
    std::fs::write(work.join("evil.html"), "<script>alert(1)</script>").expect("write");
    common::git(&work, &["add", "."]);
    common::commit(&work, "add page", 1_700_000_000);
    common::git(
        root,
        &["clone", "--bare", work.to_str().expect("utf-8"), "demo.git"],
    );
    std::fs::remove_dir_all(&work).expect("cleanup");

    let (_, body) = get(root, "/demo/blob/main/evil.html").await;

    assert!(!body.contains("<script>alert(1)</script>"));
    assert!(body.contains("&lt;script&gt;"));
}
