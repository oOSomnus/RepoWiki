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
