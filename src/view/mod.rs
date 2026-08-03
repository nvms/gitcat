pub mod index;
pub mod repository;

use maud::{DOCTYPE, Markup, html};
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};

pub const STYLE: &str = include_str!("../assets/style.css");

/// Path segments are encoded conservatively: `/` has to survive as `%2F` so a
/// branch called `feature/x` stays one URL segment.
const SEGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}')
    .add(b'/');

pub fn encode_segment(value: &str) -> String {
    utf8_percent_encode(value, SEGMENT).to_string()
}

/// A file path keeps its separators - only the individual segments are encoded.
pub fn encode_path(path: &str) -> String {
    path.split('/')
        .map(encode_segment)
        .collect::<Vec<_>>()
        .join("/")
}

pub fn page(title: &str, header: Markup, body: Markup) -> Markup {
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
                header { (header) }
                (body)
            }
        }
    }
}

pub fn error(status: axum::http::StatusCode, message: &str) -> Markup {
    page(
        message,
        html! { a href="/" { "gitcat" } },
        html! { p { (status.as_u16()) " " (message) } },
    )
}
