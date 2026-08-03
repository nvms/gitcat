pub mod index;
pub mod repository;

use maud::{DOCTYPE, Markup, html};
use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};

pub const STYLE: &str = include_str!("../assets/style.css");

/// The stylesheet is served under a content-derived path so a changed build can
/// never be shadowed by a cached copy of the old one, while unchanged builds
/// still cache forever.
pub fn style_url() -> &'static str {
    static URL: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    URL.get_or_init(|| format!("/static/{}/style.css", fingerprint(STYLE.as_bytes())))
}

/// FNV-1a. This identifies content for caching, it is not a security boundary.
fn fingerprint(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    format!("{hash:016x}")
}

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
                link rel="stylesheet" href=(style_url());
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
