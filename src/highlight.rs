use std::sync::OnceLock;

use syntect::html::{ClassStyle, ClassedHTMLGenerator};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

use crate::git::MAX_RENDER_BYTES;

/// Emitting classes rather than inline styles keeps the generated HTML small
/// and lets one stylesheet carry both the light and the dark palette.
const CLASS_STYLE: ClassStyle = ClassStyle::SpacedPrefixed { prefix: "syn-" };

/// How the syntax is chosen: a fenced code block gives a language token, a blob
/// gives a file name.
pub enum Hint<'a> {
    Token(&'a str),
    FileName(&'a str),
}

/// Loaded once from syntect's binary dump. Doing it per request would cost tens
/// of milliseconds each time.
fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

/// Returns highlighted HTML, or `None` when no syntax matches or the input is
/// too large to be worth the pass. Callers fall back to plain escaped text.
pub fn html(source: &str, hint: Hint<'_>) -> Option<String> {
    if source.len() > MAX_RENDER_BYTES {
        return None;
    }

    let syntaxes = syntaxes();
    let syntax = find_syntax(syntaxes, source, hint)?;
    let mut generator = ClassedHTMLGenerator::new_with_class_style(syntax, syntaxes, CLASS_STYLE);

    for line in LinesWithEndings::from(source) {
        // a malformed line must degrade to plain text, never abort the response
        if generator
            .parse_html_for_line_which_includes_newline(line)
            .is_err()
        {
            return None;
        }
    }

    Some(generator.finalize())
}

/// syntect's bundled syntaxes predate TypeScript, JSX, and SCSS, so these fall
/// back to the closest language that is bundled. Only mappings that highlight
/// correctly in the common case are listed - a wrong syntax is worse than none,
/// which is why TOML, Zig, Swift, and friends are deliberately absent.
const ALIASES: &[(&str, &str)] = &[
    ("jsx", "js"),
    ("mjs", "js"),
    ("cjs", "js"),
    ("ts", "js"),
    ("tsx", "js"),
    ("mts", "js"),
    ("cts", "js"),
    ("scss", "css"),
    ("sass", "css"),
    ("less", "css"),
    ("vue", "html"),
    ("svelte", "html"),
    ("kt", "java"),
    ("kts", "java"),
    ("zsh", "sh"),
    ("bash", "sh"),
];

fn find_syntax<'a>(
    syntaxes: &'a SyntaxSet,
    source: &str,
    hint: Hint<'_>,
) -> Option<&'a SyntaxReference> {
    match hint {
        Hint::Token(token) => {
            let token = token.trim();
            if token.is_empty() {
                return None;
            }
            by_token(syntaxes, token)
        }
        Hint::FileName(name) => {
            let name = name.rsplit('/').next().unwrap_or(name);
            let by_extension = name.rsplit_once('.').and_then(|(_, ext)| {
                syntaxes
                    .find_syntax_by_extension(ext)
                    .or_else(|| aliased(syntaxes, ext))
            });

            by_extension
                .or_else(|| by_token(syntaxes, name))
                .or_else(|| syntaxes.find_syntax_by_first_line(source.lines().next()?))
        }
    }
}

fn by_token<'a>(syntaxes: &'a SyntaxSet, token: &str) -> Option<&'a SyntaxReference> {
    syntaxes
        .find_syntax_by_token(token)
        .or_else(|| aliased(syntaxes, token))
}

fn aliased<'a>(syntaxes: &'a SyntaxSet, token: &str) -> Option<&'a SyntaxReference> {
    let lower = token.to_ascii_lowercase();
    let (_, target) = ALIASES.iter().find(|(from, _)| *from == lower)?;

    syntaxes.find_syntax_by_extension(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_by_language_token() {
        let html = html("fn main() {}\n", Hint::Token("rust")).expect("highlighted");
        assert!(html.contains("syn-"));
        assert!(html.contains("main"));
    }

    #[test]
    fn highlights_by_file_extension() {
        let html =
            html("body { color: red; }\n", Hint::FileName("src/site.css")).expect("highlighted");
        assert!(html.contains("syn-"));
    }

    #[test]
    fn escapes_markup_in_the_source() {
        let html = html("<script>alert(1)</script>\n", Hint::Token("html")).expect("highlighted");
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;"));
    }

    #[test]
    fn declines_unknown_and_empty_tokens() {
        assert!(html("x\n", Hint::Token("")).is_none());
        assert!(html("x\n", Hint::Token("not-a-real-language")).is_none());
    }

    #[test]
    fn declines_oversized_input() {
        let big = "a\n".repeat(MAX_RENDER_BYTES);
        assert!(html(&big, Hint::Token("rust")).is_none());
    }
}
