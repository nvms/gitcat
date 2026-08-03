use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use maud::Markup;

use crate::config::Config;
use crate::repo;
use crate::view;

pub type AppState = Arc<Config>;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Not Found")]
    NotFound,
    #[error("Internal Server Error")]
    Internal(#[from] repo::RepoError),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match self {
            AppError::NotFound => StatusCode::NOT_FOUND,
            AppError::Internal(ref e) => {
                tracing::error!(error = %e, "request failed");
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
        .fallback(not_found)
        .with_state(config)
}

async fn index(State(config): State<AppState>) -> Result<Markup, AppError> {
    let repos = repo::discover(&config.repos)?;
    Ok(view::index(&config.site_name, &repos))
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
