use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd, html};

use crate::highlight;

/// Where relative links in a README should point. Links resolve to the blob
/// view and images to the raw route, both at the revision being viewed.
pub struct Links {
    pub blob_base: String,
    pub raw_base: String,
}

/// Renders README markdown to HTML.
///
/// The source is a file out of a repository anyone with push access controls,
/// so it is treated as hostile: raw HTML is dropped rather than passed through,
/// and any link scheme other than http, https, or mailto is removed.
pub fn render(source: &str, links: &Links) -> String {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_FOOTNOTES);
    options.insert(Options::ENABLE_TASKLISTS);

    let mut events = Vec::new();
    let mut code: Option<CodeBlock> = None;

    for event in Parser::new_ext(source, options) {
        match event {
            Event::Html(_) | Event::InlineHtml(_) => {}

            Event::Start(Tag::CodeBlock(kind)) => {
                let language = match kind {
                    CodeBlockKind::Fenced(info) => info
                        .split_whitespace()
                        .next()
                        .unwrap_or_default()
                        .to_owned(),
                    CodeBlockKind::Indented => String::new(),
                };
                code = Some(CodeBlock {
                    language,
                    source: String::new(),
                });
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some(block) = code.take() {
                    events.push(Event::Html(block.into_html().into()));
                }
            }
            Event::Text(text) if code.is_some() => {
                if let Some(block) = code.as_mut() {
                    block.source.push_str(&text);
                }
            }

            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            }) => events.push(Event::Start(Tag::Link {
                link_type,
                dest_url: sanitize(&dest_url, &links.blob_base).into(),
                title,
                id,
            })),
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            }) => events.push(Event::Start(Tag::Image {
                link_type,
                dest_url: sanitize(&dest_url, &links.raw_base).into(),
                title,
                id,
            })),

            other => events.push(other),
        }
    }

    let mut out = String::new();
    html::push_html(&mut out, events.into_iter());
    out
}

struct CodeBlock {
    language: String,
    source: String,
}

impl CodeBlock {
    /// The only HTML this module emits verbatim. Highlighted output comes from
    /// syntect, which escapes as it goes; the fallback is escaped here.
    fn into_html(self) -> String {
        let body = highlight::html(&self.source, highlight::Hint::Token(&self.language))
            .unwrap_or_else(|| escape(&self.source));

        format!("<pre><code>{body}</code></pre>")
    }
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

fn sanitize(url: &str, base: &str) -> String {
    let trimmed = url.trim();

    if trimmed.is_empty() || trimmed.starts_with('#') {
        return trimmed.to_owned();
    }

    match scheme_of(trimmed) {
        Some(scheme) => {
            if matches!(scheme.as_str(), "http" | "https" | "mailto") {
                trimmed.to_owned()
            } else {
                String::new()
            }
        }
        None => resolve_relative(trimmed, base),
    }
}

/// A URL scheme per RFC 3986: a letter followed by letters, digits, `+`, `-`,
/// or `.`, terminated by a colon. Checked explicitly so `javascript:alert(1)`
/// cannot slip through as a relative path.
fn scheme_of(url: &str) -> Option<String> {
    let (candidate, _) = url.split_once(':')?;

    if candidate.is_empty() || !candidate.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return None;
    }
    if !candidate
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    {
        return None;
    }

    Some(candidate.to_ascii_lowercase())
}

/// Protocol-relative URLs (`//host/path`) are dropped - they are absolute in
/// practice and would leave the server.
fn resolve_relative(url: &str, base: &str) -> String {
    if url.starts_with("//") {
        return String::new();
    }

    let (path, suffix) = match url.find(['?', '#']) {
        Some(i) => (&url[..i], &url[i..]),
        None => (url, ""),
    };

    let mut segments: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => continue,
            ".." => {
                if segments.pop().is_none() {
                    return String::new();
                }
            }
            other => segments.push(other),
        }
    }

    if segments.is_empty() {
        return String::new();
    }

    format!("{base}/{}{suffix}", segments.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn links() -> Links {
        Links {
            blob_base: "/demo/blob/main".to_owned(),
            raw_base: "/demo/raw/main".to_owned(),
        }
    }

    #[test]
    fn renders_basic_markdown() {
        let html = render("# Title\n\nSome *text*.\n", &links());
        assert!(html.contains("<h1>Title</h1>"));
        assert!(html.contains("<em>text</em>"));
    }

    #[test]
    fn drops_raw_html() {
        let html = render("<script>alert(1)</script>\n\nafter\n", &links());
        assert!(!html.contains("<script>"));
        assert!(html.contains("after"));
    }

    #[test]
    fn drops_inline_html() {
        let html = render("text with <img src=x onerror=alert(1)> inline\n", &links());
        assert!(!html.contains("onerror"));
    }

    #[test]
    fn strips_dangerous_schemes() {
        for source in [
            "[click](javascript:alert(1))",
            "[click](JaVaScRiPt:alert(1))",
            "[click](data:text/html;base64,PHNjcmlwdD4=)",
            "[click](vbscript:msgbox)",
        ] {
            let html = render(source, &links());
            assert!(html.contains(r#"href="""#), "{source} -> {html}");
        }
    }

    #[test]
    fn keeps_safe_absolute_links() {
        let html = render("[x](https://example.com/a)", &links());
        assert!(html.contains(r#"href="https://example.com/a""#));
    }

    #[test]
    fn rewrites_relative_links_and_images() {
        let html = render(
            "[docs](./docs/guide.md)\n\n![logo](assets/logo.png)",
            &links(),
        );
        assert!(html.contains(r#"href="/demo/blob/main/docs/guide.md""#));
        assert!(html.contains(r#"src="/demo/raw/main/assets/logo.png""#));
    }

    #[test]
    fn refuses_relative_links_that_escape_the_repository() {
        let html = render("[up](../../etc/passwd)", &links());
        assert!(html.contains(r#"href="""#), "{html}");
    }

    #[test]
    fn keeps_fragment_links() {
        let html = render("[section](#usage)", &links());
        assert!(html.contains(r##"href="#usage""##));
    }
}
