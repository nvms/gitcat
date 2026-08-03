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

/// Syntax highlighting wraps tokens in spans, so assertions about what a page
/// says have to look at the text, not the markup.
fn text_of(html: &str) -> String {
    let mut out = String::new();
    let mut inside_tag = false;

    for c in html.chars() {
        match c {
            '<' => inside_tag = true,
            '>' => inside_tag = false,
            _ if !inside_tag => out.push(c),
            _ => {}
        }
    }

    out
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
    assert!(text_of(&body).contains("fn main() {}"));
    assert!(body.contains("syn-"), "source should be highlighted");

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

    assert!(
        !body.contains("<script"),
        "no live script tag may be emitted"
    );
    assert!(!body.contains("alert(1)</script>"));
    assert!(body.contains("&lt;"), "markup must arrive escaped");
    assert!(text_of(&body).contains("alert(1)"), "as visible text");
}

#[tokio::test]
async fn summary_renders_the_readme_and_lists_the_tree_first() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let work = root.join("work");
    std::fs::create_dir_all(&work).expect("create work tree");

    common::git(&work, &["init", "--initial-branch", "main", "."]);
    std::fs::write(
        work.join("README.md"),
        "# Demo\n\nSome *docs*.\n\n```rust\nfn main() {}\n```\n",
    )
    .expect("write");
    std::fs::write(work.join("code.rs"), "fn main() {}\n").expect("write");
    common::git(&work, &["add", "."]);
    common::commit(&work, "initial", 1_700_000_000);
    common::git(
        root,
        &["clone", "--bare", work.to_str().expect("utf-8"), "demo.git"],
    );
    std::fs::remove_dir_all(&work).expect("cleanup");

    let (status, body) = get(root, "/demo").await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains("<h1>Demo</h1>"),
        "readme is rendered markdown"
    );
    assert!(body.contains("<em>docs</em>"));
    assert!(body.contains("syn-"), "readme code block is highlighted");

    let tree = body.find("code.rs").expect("tree entry");
    let commits = body.find("recent commits").expect("commits section");
    let refs = body.find(">refs<").expect("refs section");
    let readme = body.find("<h1>Demo</h1>").expect("readme");

    assert!(tree < commits, "files panel comes first");
    assert!(commits < refs, "commits panel comes before refs");
    assert!(
        refs < readme,
        "the readme is last, so nothing below it reads as part of it"
    );
    assert!(
        !body.contains("<h2>README.md</h2>"),
        "the readme needs no heading of its own"
    );
}

#[tokio::test]
async fn any_markdown_blob_renders_as_markdown() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let work = root.join("work");
    std::fs::create_dir_all(&work).expect("create work tree");
    std::fs::create_dir(work.join("docs")).expect("mkdir");

    common::git(&work, &["init", "--initial-branch", "main", "."]);
    std::fs::write(
        work.join("docs/guide.md"),
        "## Guide\n\n[next](./other.md)\n\n![img](pic.png)\n",
    )
    .expect("write");
    common::git(&work, &["add", "."]);
    common::commit(&work, "initial", 1_700_000_000);
    common::git(
        root,
        &["clone", "--bare", work.to_str().expect("utf-8"), "demo.git"],
    );
    std::fs::remove_dir_all(&work).expect("cleanup");

    let (status, body) = get(root, "/demo/blob/main/docs/guide.md").await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("<h2>Guide</h2>"));
    assert!(
        body.contains(r#"href="/demo/blob/main/docs/other.md""#),
        "relative links resolve against the file's own directory"
    );
    assert!(body.contains(r#"src="/demo/raw/main/docs/pic.png""#));
}

#[tokio::test]
async fn a_markdown_blob_can_still_be_fetched_raw() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let work = root.join("work");
    std::fs::create_dir_all(&work).expect("create work tree");

    common::git(&work, &["init", "--initial-branch", "main", "."]);
    std::fs::write(work.join("doc.md"), "# Title\n").expect("write");
    common::git(&work, &["add", "."]);
    common::commit(&work, "initial", 1_700_000_000);
    common::git(
        root,
        &["clone", "--bare", work.to_str().expect("utf-8"), "demo.git"],
    );
    std::fs::remove_dir_all(&work).expect("cleanup");

    let (status, raw) = get(root, "/demo/raw/main/doc.md").await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(raw, "# Title\n");
}

#[tokio::test]
async fn a_readme_cannot_inject_script() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let work = root.join("work");
    std::fs::create_dir_all(&work).expect("create work tree");

    common::git(&work, &["init", "--initial-branch", "main", "."]);
    std::fs::write(
        work.join("README.md"),
        "<script>alert(1)</script>\n\n[x](javascript:alert(2))\n",
    )
    .expect("write");
    common::git(&work, &["add", "."]);
    common::commit(&work, "initial", 1_700_000_000);
    common::git(
        root,
        &["clone", "--bare", work.to_str().expect("utf-8"), "demo.git"],
    );
    std::fs::remove_dir_all(&work).expect("cleanup");

    let (_, body) = get(root, "/demo").await;

    assert!(!body.contains("<script"));
    assert!(!body.contains("javascript:"));
}

#[tokio::test]
async fn submodule_changes_are_described_not_diffed() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let inner = root.join("inner");
    let work = root.join("work");
    std::fs::create_dir_all(&inner).expect("create inner");
    std::fs::create_dir_all(&work).expect("create work tree");

    common::git(&inner, &["init", "--initial-branch", "main", "."]);
    std::fs::write(inner.join("a.txt"), "hello\n").expect("write");
    common::git(&inner, &["add", "."]);
    common::commit(&inner, "inner commit", 1_700_000_000);

    common::git(&work, &["init", "--initial-branch", "main", "."]);
    std::fs::write(work.join("top.txt"), "top\n").expect("write");
    common::git(&work, &["add", "."]);
    common::commit(&work, "initial", 1_700_000_000);
    common::git(
        &work,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            inner.to_str().expect("utf-8"),
            "vendor",
        ],
    );
    common::commit(&work, "add submodule", 1_700_000_100);
    let head = common::capture(&work, &["rev-parse", "HEAD"]);

    common::git(
        root,
        &["clone", "--bare", work.to_str().expect("utf-8"), "demo.git"],
    );

    let (status, body) = get(root, &format!("/demo/commit/{head}")).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("Submodule pointer changed"));
    assert!(!body.contains("not a file"));
    assert!(!body.contains("unreadable"));
}

#[tokio::test]
async fn the_stylesheet_url_changes_with_its_content() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (_, body) = get(tmp.path(), "/").await;

    let href = body
        .split(r#"<link rel="stylesheet" href=""#)
        .nth(1)
        .and_then(|s| s.split('"').next())
        .expect("stylesheet link");

    assert!(href.starts_with("/static/"), "{href}");
    assert!(href.ends_with("/style.css"), "{href}");

    let (status, css) = get(tmp.path(), href).await;
    assert_eq!(status, StatusCode::OK);
    assert!(css.contains("--bg:"));
}

#[tokio::test]
async fn a_subdirectory_readme_renders_below_its_listing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let work = root.join("work");
    std::fs::create_dir_all(work.join("benchmarks")).expect("create work tree");

    common::git(&work, &["init", "--initial-branch", "main", "."]);
    std::fs::write(work.join("README.md"), "# Root readme\n").expect("write");
    std::fs::write(
        work.join("benchmarks/README.md"),
        "# Benchmarks\n\n[result](./out.txt)\n",
    )
    .expect("write");
    std::fs::write(work.join("benchmarks/out.txt"), "numbers\n").expect("write");
    common::git(&work, &["add", "."]);
    common::commit(&work, "initial", 1_700_000_000);
    common::git(
        root,
        &["clone", "--bare", work.to_str().expect("utf-8"), "demo.git"],
    );
    std::fs::remove_dir_all(&work).expect("cleanup");

    let (status, body) = get(root, "/demo/tree/main/benchmarks").await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("<h1>Benchmarks</h1>"), "readme is rendered");
    assert!(
        !body.contains("Root readme"),
        "the directory's own readme is used, not the root one"
    );
    assert!(
        body.contains(r#"href="/demo/blob/main/benchmarks/out.txt""#),
        "relative links resolve inside the subdirectory"
    );

    let listing = body.find("out.txt").expect("tree entry");
    let readme = body.find("<h1>Benchmarks</h1>").expect("readme");
    assert!(listing < readme, "the readme sits below the folder list");
}

#[tokio::test]
async fn a_directory_without_a_readme_renders_only_the_listing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let work = root.join("work");
    std::fs::create_dir_all(work.join("src")).expect("create work tree");

    common::git(&work, &["init", "--initial-branch", "main", "."]);
    std::fs::write(work.join("README.md"), "# Root readme\n").expect("write");
    std::fs::write(work.join("src/main.rs"), "fn main() {}\n").expect("write");
    common::git(&work, &["add", "."]);
    common::commit(&work, "initial", 1_700_000_000);
    common::git(
        root,
        &["clone", "--bare", work.to_str().expect("utf-8"), "demo.git"],
    );
    std::fs::remove_dir_all(&work).expect("cleanup");

    let (status, body) = get(root, "/demo/tree/main/src").await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("main.rs"));
    assert!(
        !body.contains("Root readme"),
        "no readme leaks in from above"
    );
    assert!(!body.contains(r#"class="readme""#));
}

#[tokio::test]
async fn a_commit_reports_how_many_lines_changed() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let fixture = fixture(tmp.path());

    let (status, body) = get(tmp.path(), &format!("/demo/commit/{}", fixture.head)).await;
    let text = text_of(&body);

    assert_eq!(status, StatusCode::OK);

    // keep.txt: one line each way. gone.txt: one removed. added.txt: one added.
    assert!(
        text.contains("3 files changed"),
        "expected a file count in: {text}"
    );
    assert!(text.contains("+2"), "expected two added lines in: {text}");
    assert!(text.contains("-2"), "expected two removed lines in: {text}");
}

#[tokio::test]
async fn a_binary_change_contributes_no_line_counts() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let work = root.join("work");
    std::fs::create_dir_all(&work).expect("create work tree");

    common::git(&work, &["init", "--initial-branch", "main", "."]);
    std::fs::write(work.join("data.bin"), [0u8, 1, 2, 0, 3]).expect("write");
    std::fs::write(work.join("notes.txt"), "one\n").expect("write");
    common::git(&work, &["add", "."]);
    common::commit(&work, "add files", 1_700_000_000);
    let head = common::capture(&work, &["rev-parse", "HEAD"]);
    common::git(
        root,
        &["clone", "--bare", work.to_str().expect("utf-8"), "demo.git"],
    );
    std::fs::remove_dir_all(&work).expect("cleanup");

    let (status, body) = get(root, &format!("/demo/commit/{head}")).await;
    let text = text_of(&body);

    assert_eq!(status, StatusCode::OK);
    assert!(text.contains("2 files changed"));
    assert!(text.contains("+1"), "only the text file counts: {text}");
    assert!(body.contains("Binary file, not shown."));
}

#[tokio::test]
async fn a_commit_lists_changed_files_not_changed_directories() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let work = root.join("work");
    std::fs::create_dir_all(work.join("client/nested")).expect("create work tree");

    common::git(&work, &["init", "--initial-branch", "main", "."]);
    std::fs::write(work.join("client/package.json"), "{}\n").expect("write");
    std::fs::write(work.join("client/nested/deep.txt"), "one\n").expect("write");
    common::git(&work, &["add", "."]);
    common::commit(&work, "initial", 1_700_000_000);

    std::fs::write(work.join("client/package.json"), "{ }\n").expect("write");
    std::fs::write(work.join("client/nested/deep.txt"), "two\n").expect("write");
    common::git(&work, &["add", "-A"]);
    common::commit(&work, "edit nested files", 1_700_000_100);
    let head = common::capture(&work, &["rev-parse", "HEAD"]);

    common::git(
        root,
        &["clone", "--bare", work.to_str().expect("utf-8"), "demo.git"],
    );
    std::fs::remove_dir_all(&work).expect("cleanup");

    let (status, body) = get(root, &format!("/demo/commit/{head}")).await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("client/package.json"));
    assert!(body.contains("client/nested/deep.txt"));

    // the directories holding those files are not changes of their own
    assert!(
        !body.contains(r#"href="/demo/blob/main/client"><"#),
        "the client directory should not be listed"
    );
    assert!(
        !body.contains(r#"href="/demo/blob/main/client/nested"><"#),
        "the nested directory should not be listed"
    );
    assert!(
        text_of(&body).contains("2 files changed"),
        "only the two files count: {}",
        text_of(&body)
    );
    assert!(
        !body.contains("Contents are not available"),
        "no directory should fall through as unreadable"
    );
}
