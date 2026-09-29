#![allow(dead_code)]

use repowiki::dokuwiki;
use repowiki::model::{ArtifactIndex, Node, Summary};
use repowiki::session::{self, SessionState};
use std::collections::BTreeMap;
use tempfile::{tempdir, TempDir};

/// Guard for cases that cross into the pinned DokuWiki runtime.
///
/// Skips loudly rather than mocking: the adapter under test is the real
/// `php` + vendored DokuWiki pair, and a fake that mirrors it would only
/// ever test the fake.
pub fn dokuwiki_runtime_available() -> bool {
    let available = dokuwiki::discover_runtime().is_ok();
    if !available {
        eprintln!("SKIP: pinned DokuWiki runtime is not discoverable; install PHP 8.2+");
    }
    available
}

pub fn prepared_session(
    nodes: Vec<Node>,
    leaf_nodes: &[&str],
    summary: Summary,
) -> (TempDir, SessionState) {
    let repo = tempdir().expect("repository tempdir");
    let output = repo.path().join("docs");
    let mut state = session::create(repo.path(), &output).expect("create session");
    let components = nodes
        .into_iter()
        .map(|node| (node.id.clone(), node))
        .collect::<BTreeMap<_, _>>();
    let leaves = leaf_nodes
        .iter()
        .map(|id| (*id).to_string())
        .collect::<Vec<_>>();
    session::write_analysis_files(
        &mut state,
        &components,
        &leaves,
        &summary,
        &ArtifactIndex::default(),
    )
    .expect("write analysis files");
    (repo, state)
}
