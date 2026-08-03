use maud::{Markup, html};

use super::{encode_segment, page};
use crate::config::Config;
use crate::repo::RepoEntry;
use crate::time;

pub fn render(config: &Config, repos: &[RepoEntry]) -> Markup {
    page(
        &config.site_name,
        html! { a href="/" { (config.site_name) } },
        html! {
            @if repos.is_empty() {
                p class="muted" {
                    "No bare repositories in " (config.repos.display()) ". "
                    "Create one with " code { "git init --bare <name>.git" } "."
                }
            } @else {
                table {
                    thead {
                        tr { th { "repository" } th { "description" } th { "updated" } }
                    }
                    tbody {
                        @for repo in repos {
                            tr {
                                td { a href={ "/" (encode_segment(&repo.name)) } { (repo.name) } }
                                td {
                                    @match &repo.description {
                                        Some(text) => (text),
                                        None => span class="muted" { "-" },
                                    }
                                }
                                td class="muted" {
                                    @match &repo.head {
                                        Some(head) => (time::relative(head.seconds)),
                                        None => "empty",
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
