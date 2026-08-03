use maud::{Markup, PreEscaped, html};

use super::{encode_path, encode_segment, page};
use crate::git::{
    Blob, ChangeStatus, Commit, DiffBody, EntryKind, FileDiff, Hunk, LineKind, MAX_RENDER_BYTES,
    Readme, RefInfo, TreeItem,
};
use crate::{git, highlight, markdown, time};

pub struct Context<'a> {
    pub repo: &'a str,
    pub rev: &'a str,
}

impl Context<'_> {
    fn base(&self) -> String {
        format!("/{}", encode_segment(self.repo))
    }

    fn at(&self, section: &str, path: &str) -> String {
        let base = format!("{}/{section}/{}", self.base(), encode_segment(self.rev));
        if path.is_empty() {
            base
        } else {
            format!("{base}/{}", encode_path(path))
        }
    }

    fn commit_url(&self, id: &str) -> String {
        format!("{}/commit/{id}", self.base())
    }

    fn nav(&self, active: &str) -> Markup {
        let section = |name: &str, href: String| -> Markup {
            html! {
                a href=(href) class=@if name == active { "active" } @else { "" } { (name) }
            }
        };

        html! {
            div class="crumbs" {
                a href="/" { "gitcat" } " / " a href=(self.base()) { (self.repo) }
            }
            nav {
                (section("summary", self.base()))
                (section("log", format!("{}/log/{}", self.base(), encode_segment(self.rev))))
                (section("tree", self.at("tree", "")))
            }
        }
    }

    fn title(&self, section: &str) -> String {
        format!("{} - {section}", self.repo)
    }
}

pub struct Summary<'a> {
    pub clone_url: &'a str,
    pub branches: &'a [RefInfo],
    pub tags: &'a [RefInfo],
    pub commits: &'a [Commit],
    pub entries: &'a [TreeItem],
    pub readme: Option<&'a Readme>,
}

/// Ordered the way a repository is actually read: what is in it, what it says
/// about itself, what changed lately, and only then the refs.
pub fn summary(ctx: &Context, data: &Summary) -> Markup {
    page(
        ctx.repo,
        ctx.nav("summary"),
        html! {
            p { code { (data.clone_url) } }

            @if data.commits.is_empty() {
                p class="muted" { "This repository is empty. Push a commit to see it here." }
            } @else {
                div class="panels" {
                    (panel("files", tree_table(ctx, "", data.entries), None))
                    (panel(
                        "recent commits",
                        commit_list(ctx, data.commits),
                        Some((
                            "full history",
                            format!("{}/log/{}", ctx.base(), encode_segment(ctx.rev)),
                        )),
                    ))
                    @if !data.branches.is_empty() || !data.tags.is_empty() {
                        (panel("refs", ref_lists(ctx, data), None))
                    }
                }

                @if let Some(readme) = data.readme {
                    (readme_section(ctx, readme))
                }
            }
        },
    )
}

/// Each panel scrolls inside a capped height - a repository with thousands of
/// commits or hundreds of tags must not push everything else off the page.
fn panel(title: &str, body: Markup, footer: Option<(&str, String)>) -> Markup {
    html! {
        section class="panel" {
            h2 { (title) }
            div class="scroll" { (body) }
            @if let Some((label, href)) = footer {
                p class="panel-footer" { a href=(href) { (label) } }
            }
        }
    }
}

fn ref_lists(ctx: &Context, data: &Summary) -> Markup {
    html! {
        @if !data.branches.is_empty() {
            h3 { "branches" }
            (ref_list(ctx, data.branches))
        }
        @if !data.tags.is_empty() {
            h3 { "tags" }
            (ref_list(ctx, data.tags))
        }
    }
}

fn ref_list(ctx: &Context, refs: &[RefInfo]) -> Markup {
    html! {
        ul class="mono plain" {
            @for entry in refs {
                li {
                    a href={ (ctx.base()) "/tree/" (encode_segment(&entry.name)) } { (entry.name) }
                }
            }
        }
    }
}

/// A column is too narrow for the four-column log table, so each commit becomes
/// a summary line with its metadata underneath.
fn commit_list(ctx: &Context, commits: &[Commit]) -> Markup {
    html! {
        ul class="plain commits" {
            @for commit in commits {
                li {
                    a href=(ctx.commit_url(&commit.id)) { (commit.summary) }
                    div class="muted" {
                        span class="mono" { (commit.short_id) }
                        " " (time::relative(commit.seconds))
                    }
                }
            }
        }
    }
}

/// The rendered markup is produced by `markdown::render`, which strips raw HTML
/// and unsafe link schemes before it ever reaches here.
fn readme_section(ctx: &Context, readme: &Readme) -> Markup {
    let markup = if is_markdown(&readme.path) {
        PreEscaped(markdown::render(
            &readme.text,
            &links_relative_to(ctx, &readme.path),
        ))
    } else {
        html! { pre { (readme.text) } }
    };

    html! { article class="readme" { (markup) } }
}

fn is_markdown(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".md") || lower.ends_with(".markdown")
}

pub fn log(ctx: &Context, commits: &[Commit], next: Option<&str>) -> Markup {
    page(
        &ctx.title("log"),
        ctx.nav("log"),
        html! {
            @if commits.is_empty() {
                p class="muted" { "No commits." }
            } @else {
                (commit_table(ctx, commits))
            }
            @if let Some(next) = next {
                p { a href={ (ctx.base()) "/log/" (encode_segment(next)) } { "older" } }
            }
        },
    )
}

fn commit_table(ctx: &Context, commits: &[Commit]) -> Markup {
    html! {
        table {
            thead {
                tr { th { "commit" } th { "summary" } th { "author" } th { "date" } }
            }
            tbody {
                @for commit in commits {
                    tr {
                        td class="mono" { a href=(ctx.commit_url(&commit.id)) { (commit.short_id) } }
                        td { (commit.summary) }
                        td class="muted" { (commit.author) }
                        td class="muted" { (time::relative(commit.seconds)) }
                    }
                }
            }
        }
    }
}

pub fn commit(ctx: &Context, commit: &Commit, diffs: &[FileDiff]) -> Markup {
    page(
        &ctx.title(&commit.short_id),
        ctx.nav("log"),
        html! {
            h2 { (commit.summary) }
            @if let Some(body) = &commit.body {
                pre { (body) }
            }
            p class="muted" {
                (commit.author) " <" (commit.email) "> " (time::relative(commit.seconds))
                br;
                "commit " span class="mono" { (commit.id) }
                @for parent in &commit.parents {
                    br;
                    "parent " a class="mono" href=(ctx.commit_url(parent)) { (parent) }
                }
            }

            @if diffs.is_empty() {
                p class="muted" { "No changes." }
            } @else {
                @let totals = git::stats(diffs);
                p {
                    (totals.files) " " (plural("file", totals.files)) " changed, "
                    (counts(totals.added, totals.removed))
                }
            }

            @for file in diffs {
                h3 {
                    (status_label(file.status)) " "
                    a class="mono" href=(ctx.at("blob", &file.path)) { (file.path) }
                    @if let Some(old) = &file.old_path {
                        span class="muted" { " (from " (old) ")" }
                    }
                    @let (added, removed) = file.line_counts();
                    " " (counts(added, removed))
                }
                (diff_body(&file.body))
            }
        },
    )
}

/// Nothing is shown when a change has no line counts, which is the case for
/// submodules and for content that was binary or too large to diff.
fn counts(added: usize, removed: usize) -> Markup {
    html! {
        @if added > 0 || removed > 0 {
            span class="counts" {
                @if added > 0 { span class="add" { "+" (added) } }
                @if added > 0 && removed > 0 { " " }
                @if removed > 0 { span class="del" { "-" (removed) } }
            }
        }
    }
}

fn plural(word: &str, count: usize) -> String {
    if count == 1 {
        word.to_owned()
    } else {
        format!("{word}s")
    }
}

fn status_label(status: ChangeStatus) -> &'static str {
    match status {
        ChangeStatus::Added => "added",
        ChangeStatus::Deleted => "deleted",
        ChangeStatus::Modified => "modified",
        ChangeStatus::Renamed => "renamed",
    }
}

fn diff_body(body: &DiffBody) -> Markup {
    html! {
        @match body {
            DiffBody::Text(hunks) => (hunk_table(hunks)),
            DiffBody::Submodule { old, new } => p class="muted" {
                "Submodule pointer changed"
                @if let Some(old) = old { br; "from " (old) }
                @if let Some(new) = new { br; "to " (new) }
            },
            DiffBody::Binary => p class="muted" { "Binary file, not shown." },
            DiffBody::TooLarge => p class="muted" { "File is too large to diff." },
            DiffBody::Unreadable => p class="muted" { "Contents are not available in this repository." },
        }
    }
}

fn hunk_table(hunks: &[Hunk]) -> Markup {
    html! {
        table class="diff" {
            @for hunk in hunks {
                tr class="hunk" { td colspan="3" { (hunk.header) } }
                @for line in &hunk.lines {
                    tr class=(line_class(line.kind)) {
                        td class="num" { @if let Some(n) = line.old_no { (n) } }
                        td class="num" { @if let Some(n) = line.new_no { (n) } }
                        td { (line_prefix(line.kind)) (line.text) }
                    }
                }
            }
        }
    }
}

fn line_class(kind: LineKind) -> &'static str {
    match kind {
        LineKind::Context => "ctx",
        LineKind::Add => "add",
        LineKind::Remove => "del",
    }
}

fn line_prefix(kind: LineKind) -> &'static str {
    match kind {
        LineKind::Context => " ",
        LineKind::Add => "+",
        LineKind::Remove => "-",
    }
}

pub struct TreeView<'a> {
    pub path: &'a str,
    pub items: &'a [TreeItem],
    pub readme: Option<&'a Readme>,
}

pub fn tree(ctx: &Context, view: &TreeView) -> Markup {
    page(
        &ctx.title("tree"),
        ctx.nav("tree"),
        html! {
            (breadcrumbs(ctx, view.path, "tree"))
            (tree_table(ctx, view.path, view.items))

            @if let Some(readme) = view.readme {
                (readme_section(ctx, readme))
            }
        },
    )
}

fn tree_table(ctx: &Context, path: &str, items: &[TreeItem]) -> Markup {
    html! {
        table class="tree" {
            tbody {
                @if !path.is_empty() {
                    tr { td class="mono" { a href=(ctx.at("tree", parent_of(path))) { ".." } } }
                }
                @for item in items {
                    tr {
                        td class="mono" {
                            @let child = join(path, &item.name);
                            @match item.kind {
                                EntryKind::Directory => {
                                    a href=(ctx.at("tree", &child)) { (item.name) "/" }
                                }
                                EntryKind::Submodule => {
                                    span class="muted" { (item.name) "@" }
                                }
                                _ => {
                                    a href=(ctx.at("blob", &child)) { (item.name) }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn blob(ctx: &Context, path: &str, blob: &Blob) -> Markup {
    page(
        &ctx.title(path),
        ctx.nav("tree"),
        html! {
            (breadcrumbs(ctx, path, "blob"))
            p { a href=(ctx.at("raw", path)) { "raw" } " " span class="muted" { (blob.bytes.len()) " bytes" } }

            @if blob.binary {
                p class="muted" { "Binary file." }
            } @else if blob.bytes.len() > MAX_RENDER_BYTES {
                p class="muted" { "File is too large to display." }
            } @else if is_markdown(path) {
                article class="readme" {
                    (PreEscaped(markdown::render(
                        &String::from_utf8_lossy(&blob.bytes),
                        &links_relative_to(ctx, path),
                    )))
                }
            } @else {
                (source(path, &String::from_utf8_lossy(&blob.bytes)))
            }
        },
    )
}

/// Relative links inside a markdown file resolve against the directory that
/// file lives in, not the repository root.
fn links_relative_to(ctx: &Context, path: &str) -> markdown::Links {
    let dir = parent_of(path);

    markdown::Links {
        blob_base: ctx.at("blob", dir),
        raw_base: ctx.at("raw", dir),
    }
}

/// Highlighted when syntect recognises the file, plain escaped text otherwise.
fn source(path: &str, text: &str) -> Markup {
    match highlight::html(text, highlight::Hint::FileName(path)) {
        Some(highlighted) => html! { pre { code { (PreEscaped(highlighted)) } } },
        None => html! { pre { code { (text) } } },
    }
}

/// Each segment links to the tree it sits in. The final segment of a blob path
/// is plain text - it is the page you are already on.
fn breadcrumbs(ctx: &Context, path: &str, section: &str) -> Markup {
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
    let mut walked = String::new();
    let crumbs: Vec<(String, String)> = parts
        .iter()
        .map(|part| {
            walked = join(&walked, part);
            ((*part).to_owned(), walked.clone())
        })
        .collect();
    let last = crumbs.len().saturating_sub(1);

    html! {
        p class="mono" {
            (ctx.rev) ": "
            a href=(ctx.at("tree", "")) { (ctx.repo) }
            @for (i, (name, full)) in crumbs.iter().enumerate() {
                "/"
                @if i == last && section == "blob" {
                    (name)
                } @else {
                    a href=(ctx.at("tree", full)) { (name) }
                }
            }
        }
    }
}

fn join(path: &str, name: &str) -> String {
    if path.is_empty() {
        name.to_owned()
    } else {
        format!("{path}/{name}")
    }
}

fn parent_of(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[..i],
        None => "",
    }
}
