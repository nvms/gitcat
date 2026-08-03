use maud::{Markup, html};

use super::{encode_path, encode_segment, page};
use crate::git::{
    Blob, ChangeStatus, Commit, EntryKind, FileDiff, LineKind, MAX_RENDER_BYTES, RefInfo, TreeItem,
};
use crate::time;

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

    fn nav(&self) -> Markup {
        html! {
            a href="/" { "gitcat" } " / "
            a href=(self.base()) { (self.repo) }
            " "
            a href=(self.base()) { "summary" }
            " "
            a href={ (self.base()) "/log/" (encode_segment(self.rev)) } { "log" }
            " "
            a href=(self.at("tree", "")) { "tree" }
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
}

pub fn summary(ctx: &Context, data: &Summary) -> Markup {
    page(
        ctx.repo,
        ctx.nav(),
        html! {
            p { code { (data.clone_url) } }

            @if data.commits.is_empty() {
                p class="muted" { "This repository is empty. Push a commit to see it here." }
            } @else {
                (commit_table(ctx, data.commits))
                p { a href={ (ctx.base()) "/log/" (encode_segment(ctx.rev)) } { "more commits" } }
            }

            @if !data.branches.is_empty() {
                h2 { "branches" }
                ul {
                    @for branch in data.branches {
                        li {
                            a href={ (ctx.base()) "/tree/" (encode_segment(&branch.name)) } { (branch.name) }
                        }
                    }
                }
            }

            @if !data.tags.is_empty() {
                h2 { "tags" }
                ul {
                    @for tag in data.tags {
                        li {
                            a href={ (ctx.base()) "/tree/" (encode_segment(&tag.name)) } { (tag.name) }
                        }
                    }
                }
            }
        },
    )
}

pub fn log(ctx: &Context, commits: &[Commit], next: Option<&str>) -> Markup {
    page(
        &ctx.title("log"),
        ctx.nav(),
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
                        td { a href=(ctx.commit_url(&commit.id)) { (commit.short_id) } }
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
        ctx.nav(),
        html! {
            h2 { (commit.summary) }
            @if let Some(body) = &commit.body {
                pre { (body) }
            }
            p class="muted" {
                (commit.author) " <" (commit.email) "> " (time::relative(commit.seconds))
                br;
                "commit " (commit.id)
                @for parent in &commit.parents {
                    br;
                    "parent " a href=(ctx.commit_url(parent)) { (parent) }
                }
            }

            @if diffs.is_empty() {
                p class="muted" { "No changes." }
            }
            @for file in diffs {
                h3 {
                    (status_label(file.status)) " "
                    a href=(ctx.at("blob", &file.path)) { (file.path) }
                    @if let Some(old) = &file.old_path {
                        span class="muted" { " (from " (old) ")" }
                    }
                }
                @match &file.skipped {
                    Some(reason) => p class="muted" { (reason) },
                    None => (hunks(file)),
                }
            }
        },
    )
}

fn status_label(status: ChangeStatus) -> &'static str {
    match status {
        ChangeStatus::Added => "added",
        ChangeStatus::Deleted => "deleted",
        ChangeStatus::Modified => "modified",
        ChangeStatus::Renamed => "renamed",
    }
}

fn hunks(file: &FileDiff) -> Markup {
    html! {
        table class="diff" {
            @for hunk in &file.hunks {
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

pub fn tree(ctx: &Context, path: &str, items: &[TreeItem]) -> Markup {
    page(
        &ctx.title("tree"),
        ctx.nav(),
        html! {
            (breadcrumbs(ctx, path, "tree"))

            table {
                tbody {
                    @if !path.is_empty() {
                        tr { td { a href=(ctx.at("tree", parent_of(path))) { ".." } } }
                    }
                    @for item in items {
                        tr {
                            td {
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
        },
    )
}

pub fn blob(ctx: &Context, path: &str, blob: &Blob) -> Markup {
    page(
        &ctx.title(path),
        ctx.nav(),
        html! {
            (breadcrumbs(ctx, path, "blob"))
            p { a href=(ctx.at("raw", path)) { "raw" } " " (blob.bytes.len()) " bytes" }

            @if blob.binary {
                p class="muted" { "Binary file." }
            } @else if blob.bytes.len() > MAX_RENDER_BYTES {
                p class="muted" { "File is too large to display." }
            } @else {
                pre { (String::from_utf8_lossy(&blob.bytes)) }
            }
        },
    )
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
        p {
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
