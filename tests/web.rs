mod common;

use std::path::Path;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use gitcat::{Config, web};
use http_body_util::BodyExt;
use tower::ServiceExt;

async fn get(root: &Path, uri: &str) -> (StatusCode, String) {
    let config = Arc::new(Config::new(root, "test site".to_owned()).expect("config"));
    let response = web::router(config)
        .oneshot(
            Request::builder()
                .uri(uri)
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

#[tokio::test]
async fn index_lists_repositories() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();

    common::bare_repo_with_commit(root, "alpha", "a commit", 1_700_000_000);
    common::empty_bare_repo(root, "beta");

    let (status, body) = get(root, "/").await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("test site"));
    assert!(body.contains(r#"<a href="/alpha">alpha</a>"#));
    assert!(body.contains(r#"<a href="/beta">beta</a>"#));
    assert!(body.contains("empty"));
}

#[tokio::test]
async fn index_explains_an_empty_repository_directory() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (status, body) = get(tmp.path(), "/").await;
    let scanned = tmp.path().canonicalize().expect("canonicalize");

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("No bare repositories in"));
    assert!(body.contains(&scanned.display().to_string()));
}

#[tokio::test]
async fn descriptions_are_escaped() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();

    common::empty_bare_repo(root, "xss");
    std::fs::write(
        root.join("xss.git/description"),
        "<script>alert(1)</script>\n",
    )
    .expect("write description");

    let (_, body) = get(root, "/").await;

    assert!(!body.contains("<script>alert(1)</script>"));
    assert!(body.contains("&lt;script&gt;"));
}

#[tokio::test]
async fn unknown_paths_return_404() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (status, body) = get(tmp.path(), "/does/not/exist").await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains("Not Found"));
}

#[tokio::test]
async fn stylesheet_is_served_from_the_binary() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (status, body) = get(tmp.path(), "/static/style.css").await;

    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("--bg:"));
}
