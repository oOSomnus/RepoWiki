use codewiki::docs::{self, EditOperation};
use codewiki::model::{ArtifactIndex, Module, Node, Summary};
use codewiki::session;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use tempfile::tempdir;

fn node(id: &str, language: &str) -> Node {
    Node {
        id: id.to_string(),
        language: language.to_string(),
        ..Node::default()
    }
}

fn prepared_session(
    nodes: &[(&str, &str)],
    leaf_nodes: &[&str],
) -> (tempfile::TempDir, codewiki::session::SessionState) {
    let repo = tempdir().expect("repository tempdir");
    let output = repo.path().join("docs");
    let mut state = session::create(repo.path(), &output).expect("create session");
    let nodes = nodes
        .iter()
        .map(|(id, language)| ((*id).to_string(), node(id, language)))
        .collect::<BTreeMap<_, _>>();
    let leaves = leaf_nodes
        .iter()
        .map(|id| (*id).to_string())
        .collect::<Vec<_>>();
    session::write_analysis_files(
        &mut state,
        &nodes,
        &leaves,
        &Summary::default(),
        &ArtifactIndex::default(),
    )
    .expect("write analysis files");
    (repo, state)
}

fn module(components: &[&str], children: BTreeMap<String, Module>) -> Module {
    Module {
        path: None,
        components: components.iter().map(|id| (*id).to_string()).collect(),
        children,
    }
}

#[test]
fn tree_validation_separates_unknown_ids_from_leaf_coverage() {
    let (_repo, state) = prepared_session(
        &[("known-leaf", "python"), ("known-parent", "python")],
        &["known-leaf"],
    );
    let mut children = BTreeMap::new();
    children.insert("Leaf".to_string(), module(&["known-leaf"], BTreeMap::new()));
    let mut tree = BTreeMap::new();
    tree.insert(
        "Root".to_string(),
        module(&["known-parent", "unknown"], children),
    );

    let saved = docs::save_module_tree(&state, &tree, true).expect("save module tree");
    let validation: Value = session::read_json(std::path::Path::new(&saved.validation_path))
        .expect("read tree validation");
    assert_eq!(validation["unmatched_component_ids"], json!(["unknown"]));
    assert_eq!(validation["leftover_candidate_ids"], json!([]));
    assert_eq!(validation["unmatched_ids"], json!(["unknown"]));
    assert_eq!(validation["leftover_component_ids"], json!([]));
    assert_eq!(validation["valid"], json!(false));

    let order: Value = session::read_json(std::path::Path::new(&saved.processing_order_path))
        .expect("read processing order");
    assert_eq!(
        order,
        json!([
            {
                "module": "Leaf",
                "path": ["Root", "Leaf"],
                "is_leaf": true,
                "components": ["known-leaf"]
            },
            {
                "module": "Root",
                "path": ["Root"],
                "is_leaf": false,
                "children": ["Leaf"],
                "components": ["known-parent", "unknown"]
            }
        ])
    );

    let mut incomplete_tree = BTreeMap::new();
    incomplete_tree.insert(
        "Root".to_string(),
        module(&["known-parent"], BTreeMap::new()),
    );
    let saved =
        docs::save_module_tree(&state, &incomplete_tree, false).expect("save incomplete tree");
    let validation: Value = session::read_json(std::path::Path::new(&saved.validation_path))
        .expect("read incomplete validation");
    assert_eq!(validation["unmatched_component_ids"], json!([]));
    assert_eq!(validation["leftover_candidate_ids"], json!(["known-leaf"]));
    assert_eq!(validation["valid"], json!(true));
}

#[test]
fn languages_are_written_as_stable_component_counts() {
    let (_repo, state) = prepared_session(
        &[("z", "python"), ("a", "python"), ("m", "javascript")],
        &["z", "a", "m"],
    );
    let languages: Value =
        session::read_json(&session::session_value_path(&state, "languages.json"))
            .expect("read language counts");
    assert_eq!(languages, json!({"javascript": 1, "python": 2}));
}

#[test]
fn processing_order_accepts_legacy_item_names() {
    let (_repo, state) = prepared_session(&[("leaf", "python")], &["leaf"]);
    let path = session::session_value_path(&state, "processing_order.json");
    session::write_json(
        &path,
        &json!([{
            "module_name": "leaf",
            "doc_path": "leaf.md",
            "is_leaf": true,
            "components": ["leaf"],
            "children": []
        }]),
    )
    .expect("write legacy processing order");

    let order = docs::read_processing_order(&state).expect("read legacy processing order");
    let serialized = serde_json::to_value(order).expect("serialize processing order");
    assert_eq!(serialized[0]["module"], json!("leaf"));
    assert_eq!(serialized[0]["path"], json!(["leaf"]));
    assert_eq!(serialized[0]["is_leaf"], json!(true));
    assert_eq!(serialized[0]["components"], json!(["leaf"]));
    assert_eq!(serialized[0]["children"], Value::Null);
}

#[test]
fn edit_insert_is_multiline_and_undo_pops_each_saved_version() {
    let (_repo, mut state) = prepared_session(&[], &[]);
    docs::write_document(&mut state, "guide.md", "zero\none\n").expect("write document");

    let insert: EditOperation = serde_json::from_value(json!({
        "command": "insert",
        "new_str": "alpha\nbeta",
        "insert_line": 1
    }))
    .expect("decode reference insert");
    docs::edit_document(&mut state, "guide.md", &[insert]).expect("insert lines");
    assert_eq!(
        fs::read_to_string(std::path::Path::new(&state.output_dir).join("guide.md"))
            .expect("read inserted document"),
        "zero\nalpha\nbeta\none\n"
    );

    let replace: EditOperation = serde_json::from_value(json!({
        "command": "str_replace",
        "old_str": "beta",
        "new_str": "BETA"
    }))
    .expect("decode reference replace");
    docs::edit_document(&mut state, "guide.md", &[replace]).expect("replace text");
    docs::edit_document(
        &mut state,
        "guide.md",
        &[EditOperation {
            kind: Some("undo".to_string()),
            ..EditOperation::default()
        }],
    )
    .expect("undo replace");
    assert_eq!(
        fs::read_to_string(std::path::Path::new(&state.output_dir).join("guide.md"))
            .expect("read undone replacement"),
        "zero\nalpha\nbeta\none\n"
    );

    docs::edit_document(
        &mut state,
        "guide.md",
        &[EditOperation {
            command: Some("undo".to_string()),
            ..EditOperation::default()
        }],
    )
    .expect("undo insert");
    assert_eq!(
        fs::read_to_string(std::path::Path::new(&state.output_dir).join("guide.md"))
            .expect("read undone insertion"),
        "zero\none\n"
    );
}

#[test]
fn component_and_candidate_order_is_deterministic() {
    let (_repo, state) = prepared_session(&[("b", "python"), ("a", "python")], &["b", "a"]);
    let components: Value =
        session::read_json(&session::session_value_path(&state, "components.json"))
            .expect("read components");
    let keys = components
        .as_object()
        .expect("component map")
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    assert_eq!(keys.into_iter().collect::<Vec<_>>(), vec!["a", "b"]);
}
