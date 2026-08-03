use std::sync::Arc;

use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use maud::Markup;

use crate::config::Config;
use crate::view::repository::{Context, Summary};
use crate::{git, repo, view};

pub type AppState = Arc<Config>;

/// Commits per page in the log view.
const LOG_PAGE_SIZE: usize = 50;

/// Commits shown on a repository summary page.
const SUMMARY_COMMITS: usize = 10;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Not Found")]
    NotFound,
    #[error("Internal Server Error")]
    Internal(String),
}

impl From<repo::RepoError> for AppError {
    fn from(e: repo::RepoError) -> Self {
        match e {
            repo::RepoError::InvalidName | repo::RepoError::NotFound => AppError::NotFound,
            other => AppError::Internal(other.to_string()),
        }
    }
}

impl From<git::GitError> for AppError {
    fn from(e: git::GitError) -> Self {
        match e {
            git::GitError::UnknownRevision | git::GitError::UnknownPath => AppError::NotFound,
            git::GitError::Read(message) => AppError::Internal(message),
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match self {
            AppError::NotFound => StatusCode::NOT_FOUND,
            AppError::Internal(ref message) => {
                tracing::error!(error = %message, "request failed");
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };

        (status, view::error(status, &self.to_string())).into_response()
    }
}

pub fn router(config: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/static/style.css", get(stylesheet))
        .route("/{repo}", get(summary))
        .route("/{repo}/log/{rev}", get(log))
        .route("/{repo}/commit/{rev}", get(commit))
        .route("/{repo}/tree/{rev}", get(tree_root))
        .route("/{repo}/tree/{rev}/{*path}", get(tree))
        .route("/{repo}/blob/{rev}/{*path}", get(blob))
        .route("/{repo}/raw/{rev}/{*path}", get(raw))
        .fallback(not_found)
        .with_state(config)
}

async fn index(State(config): State<AppState>) -> Result<Markup, AppError> {
    let repos = repo::discover(&config.repos)?;
    Ok(view::index::render(&config, &repos))
}

async fn summary(
    State(config): State<AppState>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<Markup, AppError> {
    let git_repo = repo::open(&config.repos, &name)?;
    let name = repo::validate_name(&name)?.to_owned();
    let rev = git::default_branch(&git_repo);

    let commits = match git::resolve(&git_repo, &rev) {
        Ok(head) => git::history(&git_repo, head, SUMMARY_COMMITS)?.0,
        Err(_) => Vec::new(),
    };

    let host = headers.get(header::HOST).and_then(|h| h.to_str().ok());
    let clone_url = format!("git clone {}/{}.git", config.origin_for(host), name);
    let branches = git::branches(&git_repo);
    let tags = git::tags(&git_repo);

    let ctx = Context {
        repo: &name,
        rev: &rev,
    };

    Ok(view::repository::summary(
        &ctx,
        &Summary {
            clone_url: &clone_url,
            branches: &branches,
            tags: &tags,
            commits: &commits,
        },
    ))
}

async fn log(
    State(config): State<AppState>,
    Path((name, rev)): Path<(String, String)>,
) -> Result<Markup, AppError> {
    let git_repo = repo::open(&config.repos, &name)?;
    let name = repo::validate_name(&name)?.to_owned();
    let start = git::resolve(&git_repo, &rev)?;
    let (commits, next) = git::history(&git_repo, start, LOG_PAGE_SIZE)?;

    let ctx = Context {
        repo: &name,
        rev: &rev,
    };

    Ok(view::repository::log(&ctx, &commits, next.as_deref()))
}

async fn commit(
    State(config): State<AppState>,
    Path((name, rev)): Path<(String, String)>,
) -> Result<Markup, AppError> {
    let git_repo = repo::open(&config.repos, &name)?;
    let name = repo::validate_name(&name)?.to_owned();
    let id = git::resolve(&git_repo, &rev)?;
    let commit = git::commit(&git_repo, id)?;
    let diffs = git::commit_diff(&git_repo, id)?;

    let ctx = Context {
        repo: &name,
        rev: &rev,
    };

    Ok(view::repository::commit(&ctx, &commit, &diffs))
}

async fn tree_root(
    state: State<AppState>,
    Path((name, rev)): Path<(String, String)>,
) -> Result<Markup, AppError> {
    tree(state, Path((name, rev, String::new()))).await
}

async fn tree(
    State(config): State<AppState>,
    Path((name, rev, path)): Path<(String, String, String)>,
) -> Result<Markup, AppError> {
    let git_repo = repo::open(&config.repos, &name)?;
    let name = repo::validate_name(&name)?.to_owned();
    let id = git::resolve(&git_repo, &rev)?;
    let items = git::tree(&git_repo, id, &path)?;

    let ctx = Context {
        repo: &name,
        rev: &rev,
    };

    Ok(view::repository::tree(&ctx, &path, &items))
}

async fn blob(
    State(config): State<AppState>,
    Path((name, rev, path)): Path<(String, String, String)>,
) -> Result<Markup, AppError> {
    let git_repo = repo::open(&config.repos, &name)?;
    let name = repo::validate_name(&name)?.to_owned();
    let id = git::resolve(&git_repo, &rev)?;
    let blob = git::blob(&git_repo, id, &path)?;

    let ctx = Context {
        repo: &name,
        rev: &rev,
    };

    Ok(view::repository::blob(&ctx, &path, &blob))
}

/// Raw blobs are served as plain text with sniffing disabled. Serving them as
/// their guessed content type would let a pushed HTML file run script under
/// this origin.
async fn raw(
    State(config): State<AppState>,
    Path((name, rev, path)): Path<(String, String, String)>,
) -> Result<Response, AppError> {
    let git_repo = repo::open(&config.repos, &name)?;
    let id = git::resolve(&git_repo, &rev)?;
    let blob = git::blob(&git_repo, id, &path)?;

    Ok((
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (header::CONTENT_DISPOSITION, "inline"),
        ],
        blob.bytes,
    )
        .into_response())
}

async fn stylesheet() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        view::STYLE,
    )
}

async fn not_found() -> AppError {
    AppError::NotFound
}
