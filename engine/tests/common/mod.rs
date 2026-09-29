#![allow(dead_code)]

use repowiki::model::{ArtifactIndex, Node, Summary};
use repowiki::session::{self, SessionState};
use std::collections::BTreeMap;
use tempfile::{tempdir, TempDir};

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
