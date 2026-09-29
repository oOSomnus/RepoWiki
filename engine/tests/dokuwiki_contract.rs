#[path = "common/mod.rs"]
mod common;

use repowiki::dokuwiki;
use repowiki::session;
use std::fs;

fn workspace_leftovers(context: &dokuwiki::WikiContext) -> Vec<std::path::PathBuf> {
    fs::read_dir(&context.workspace)
        .expect("read runtime workspace")
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("request-"))
        })
        .collect()
}

#[test]
fn one_context_serves_repeated_page_round_trips() {
    if !common::dokuwiki_runtime_available() {
        return;
    }
    let repo = tempfile::tempdir().expect("repository tempdir");
    let output = repo.path().join("docs");
    let state = session::create(repo.path(), &output).expect("create session");
    let context = dokuwiki::session_context(&state).expect("wiki context");
    let page_id = format!("{}:guide:start", state.wiki_id);

    for round in 0..2 {
        let content = format!("Round {round}\n\n===== Heading =====\n\nBody text.\n");
        context.write(&page_id, &content).expect("write page");
        assert_eq!(context.read(&page_id).expect("read page"), content);
        let parsed = context.parse(&page_id, &content).expect("parse page");
        assert!(
            parsed.links.is_empty(),
            "unexpected links: {:?}",
            parsed.links
        );
        assert_eq!(
            context.render(&page_id).expect("render page"),
            parsed.html,
            "parse and render must agree through one context"
        );
        assert!(
            workspace_leftovers(&context).is_empty(),
            "request files must be cleaned up per operation"
        );
    }
}

#[test]
fn structure_offsets_land_on_the_source_the_caller_holds() {
    if !common::dokuwiki_runtime_available() {
        return;
    }
    let repo = tempfile::tempdir().expect("repository tempdir");
    let output = repo.path().join("docs");
    let state = session::create(repo.path(), &output).expect("create session");
    let context = dokuwiki::session_context(&state).expect("wiki context");
    let page_id = format!("{}:start", state.wiki_id);

    let content = [
        "===== Alpha =====",
        "prose 中文 with %%raw 中文%% words here",
        "  ===== indented is verbatim =====",
        "<code java>",
        "===== fake heading =====",
        "int x = 1;",
        "</code>",
        "===== Beta 中文 =====",
        "",
    ]
    .join("\r\n");
    let source = content.replace("\r\n", "\n");

    let parsed = context.parse(&page_id, &content).expect("parse page");

    let headings = parsed
        .structure
        .headings
        .iter()
        .map(|heading| {
            (
                heading.level,
                heading.text.as_str(),
                &source[heading.start..heading.end],
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        headings,
        vec![
            (2, "Alpha", "===== Alpha ====="),
            (2, "Beta 中文", "===== Beta 中文 ====="),
        ],
        "offsets are byte offsets into the LF-folded source, and neither an indented \
         verbatim line nor a heading inside a code block is a heading"
    );

    let spans = parsed
        .structure
        .spans
        .iter()
        .map(|span| (span.kind, &source[span.start..span.end]))
        .collect::<Vec<_>>();
    assert_eq!(
        spans,
        vec![
            (dokuwiki::SpanKind::Unformatted, "%%raw 中文%%"),
            (
                dokuwiki::SpanKind::Code,
                "<code java>\n===== fake heading =====\nint x = 1;\n</code>"
            ),
        ],
        "spans cover the delimiters too, so a caller never counts them as prose"
    );
    assert!(parsed
        .structure
        .spans
        .iter()
        .flat_map(|span| [span.start, span.end])
        .all(|offset| source.is_char_boundary(offset)));
}
