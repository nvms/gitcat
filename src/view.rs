use maud::{DOCTYPE, Markup, html};

use crate::repo::RepoEntry;
use crate::time;

pub const STYLE: &str = include_str!("assets/style.css");

pub fn page(title: &str, body: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (title) }
                link rel="stylesheet" href="/static/style.css";
            }
            body {
                header {
                    a class="brand" href="/" { "gitcat" }
                }
                main { (body) }
            }
        }
    }
}

pub fn index(site_name: &str, repos: &[RepoEntry]) -> Markup {
    page(
        site_name,
        html! {
            h1 { (site_name) }

            @if repos.is_empty() {
                p class="empty" {
                    "No repositories yet. Create one with "
                    code { "git init --bare <name>.git" }
                    " in the repository directory."
                }
            } @else {
                table class="repos" {
                    thead {
                        tr { th { "Repository" } th { "Description" } th { "Updated" } }
                    }
                    tbody {
                        @for repo in repos {
                            tr {
                                td { a href={ "/" (repo.name) } { (repo.name) } }
                                td class="description" {
                                    @match &repo.description {
                                        Some(text) => (text),
                                        None => span class="muted" { "-" },
                                    }
                                }
                                td class="updated" {
                                    @match &repo.head {
                                        Some(head) => (time::relative(head.seconds)),
                                        None => span class="muted" { "empty" },
                                    }
                                }
                            }
                        }
                    }
                }
            }
        },
    )
}

pub fn error(status: axum::http::StatusCode, message: &str) -> Markup {
    page(
        message,
        html! {
            h1 { (status.as_u16()) " " (message) }
        },
    )
}
