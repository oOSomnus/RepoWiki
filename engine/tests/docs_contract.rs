use codewiki::docs::{self, EditOperation};
use codewiki::model::{ArtifactIndex, ChangeSet, Module, Node, Summary};
use codewiki::session;
use codewiki::update;
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
    prepared_session_with_summary(nodes, leaf_nodes, Summary::default())
}

fn prepared_session_with_summary(
    nodes: &[(&str, &str)],
    leaf_nodes: &[&str],
    summary: Summary,
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
        &summary,
        &ArtifactIndex::default(),
    )
    .expect("write analysis files");
    (repo, state)
}

fn source_node(id: &str, language: &str, source_code: &str) -> Node {
    Node {
        id: id.to_string(),
        language: language.to_string(),
        source_code: source_code.to_string(),
        ..Node::default()
    }
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

#[test]
fn recursive_tree_quality_and_metadata_use_documentation_leaf_counts() {
    let (_repo, state) = prepared_session_with_summary(
        &[("leaf", "python")],
        &["leaf"],
        Summary {
            max_token_per_module: 100,
            max_token_per_leaf_module: 100,
            cluster_batch_size: 600,
            ..Summary::default()
        },
    );
    let mut children = BTreeMap::new();
    children.insert("Leaf".to_string(), module(&["leaf"], BTreeMap::new()));
    let mut tree = BTreeMap::new();
    tree.insert("Root".to_string(), module(&["leaf"], children));

    let saved = docs::save_module_tree(&state, &tree, true).expect("save nested tree");
    let validation: Value = session::read_json(std::path::Path::new(&saved.validation_path))
        .expect("read nested validation");
    assert_eq!(validation["quality_valid"], json!(true));
    assert_eq!(validation["module_count"], json!(2));
    assert_eq!(validation["leaf_count"], json!(1));
    assert_eq!(validation["max_depth"], json!(2));

    let metadata = docs::finalize_metadata(&state, "test-model").expect("finalize metadata");
    assert_eq!(metadata.statistics.analysis_leaf_candidates, 1);
    assert_eq!(metadata.statistics.leaf_nodes, 1);
    assert_eq!(metadata.statistics.module_count, 2);
    assert_eq!(metadata.statistics.max_depth, 2);

    let mut invalid_tree = BTreeMap::new();
    let mut invalid_children = BTreeMap::new();
    invalid_children.insert("Leaf".to_string(), module(&["leaf"], BTreeMap::new()));
    invalid_tree.insert("Root".to_string(), module(&[], invalid_children));
    let invalid = docs::save_module_tree(&state, &invalid_tree, false)
        .expect("save invalid aggregate tree for diagnostics");
    assert!(!invalid.quality_valid);
    assert!(invalid
        .quality_errors
        .iter()
        .any(|error| error.contains("aggregate") || error.contains("parent")));
}

#[test]
fn oversized_leaf_is_reported_and_blocks_documentation_close() {
    let (_repo, mut state) = prepared_session_with_summary(
        &[("a", "python"), ("b", "python")],
        &["a", "b"],
        Summary {
            max_token_per_module: 1,
            max_token_per_leaf_module: 1,
            cluster_batch_size: 600,
            ..Summary::default()
        },
    );
    let components: BTreeMap<String, Node> =
        session::read_json(&session::session_value_path(&state, "components.json"))
            .expect("read fixture components");
    let mut components = components;
    components.insert(
        "a".to_string(),
        source_node("a", "python", "def a(): return alpha + beta"),
    );
    components.insert(
        "b".to_string(),
        source_node("b", "python", "def b(): return gamma + delta"),
    );
    session::write_json(
        &session::session_value_path(&state, "components.json"),
        &components,
    )
    .expect("write fixture sources");

    let mut tree = BTreeMap::new();
    tree.insert("TooLarge".to_string(), module(&["a", "b"], BTreeMap::new()));
    let saved = docs::save_module_tree(&state, &tree, true).expect("save oversized tree");
    assert!(!saved.quality_valid);
    assert!(!saved.quality_errors.is_empty());

    docs::write_document(&mut state, "TooLarge.md", "# Too large\n").expect("write module page");
    docs::write_document(&mut state, "overview.md", "# Overview\n").expect("write overview");
    let error = docs::validate_documentation(&state).expect_err("quality gate should block close");
    assert!(error.to_string().contains("quality gate"));
}

#[test]
fn oversized_singleton_is_a_warning_but_remains_documentable() {
    let (_repo, state) = prepared_session_with_summary(
        &[("only", "rust")],
        &["only"],
        Summary {
            max_token_per_module: 1,
            ..Summary::default()
        },
    );
    let mut nodes: BTreeMap<String, Node> =
        session::read_json(&session::session_value_path(&state, "components.json"))
            .expect("read singleton");
    nodes.insert(
        "only".to_string(),
        source_node("only", "rust", &"fn only() { return_value(); }".repeat(20)),
    );
    session::write_json(
        &session::session_value_path(&state, "components.json"),
        &nodes,
    )
    .expect("write singleton source");
    let mut tree = BTreeMap::new();
    tree.insert("Only".to_string(), module(&["only"], BTreeMap::new()));
    let saved = docs::save_module_tree(&state, &tree, true).expect("save singleton");
    assert!(saved.quality_valid);
    let validation: Value = session::read_json(std::path::Path::new(&saved.validation_path))
        .expect("read singleton validation");
    assert_eq!(validation["oversized_leaf_modules"], json!([]));
    assert_eq!(
        validation["oversized_leaf_warnings"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn cluster_response_is_applied_recursively_with_structural_fallback() {
    let (_repo, state) = prepared_session(
        &[("a", "rust"), ("b", "rust"), ("c", "rust")],
        &["a", "b", "c"],
    );
    let mut tree = BTreeMap::new();
    let input = vec!["a".to_string(), "b".to_string(), "c".to_string()];
    let diagnostics = docs::apply_cluster_response(
        &state,
        &mut tree,
        r#"<GROUPED_COMPONENTS>{"Core":{"path":"src","components":["a","b"]}}</GROUPED_COMPONENTS>"#,
        &input,
        "repo",
        &[],
    )
    .expect("apply root response");
    assert_eq!(tree["Core"].components, vec!["a", "b"]);
    assert_eq!(diagnostics["fallback_used"], json!(true));
    assert_eq!(
        tree.values()
            .flat_map(|module| module.components.iter())
            .cloned()
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["a".to_string(), "b".to_string(), "c".to_string()])
    );

    let parent_path = vec!["Core".to_string()];
    docs::apply_cluster_response(
        &state,
        &mut tree,
        r#"{"API":{"components":["a"]},"Runtime":{"components":["b"]}}"#,
        &input[..2],
        "module",
        &parent_path,
    )
    .expect("apply nested response");
    assert_eq!(tree["Core"].components, vec!["a", "b"]);
    assert!(tree["Core"].children.contains_key("API"));
    assert!(tree["Core"].children.contains_key("Runtime"));
}

#[test]
fn super_group_preserves_existing_modules_and_overview_context_links_children() {
    let (_repo, state) = prepared_session(&[("a", "rust"), ("b", "rust")], &["a", "b"]);
    let mut tree = BTreeMap::new();
    tree.insert("API".to_string(), module(&["a"], BTreeMap::new()));
    tree.insert("Runtime".to_string(), module(&["b"], BTreeMap::new()));
    let result = docs::apply_super_group_response(
        &mut tree,
        r#"<GROUPED_MODULES>{"Platform":{"modules":["API","Runtime"]}}</GROUPED_MODULES>"#,
    )
    .expect("apply super group");
    assert_eq!(result["changed"], json!(true));
    assert!(tree["Platform"].children.contains_key("API"));
    assert_eq!(tree["Platform"].components, vec!["a", "b"]);

    let output = session::output_dir(&state);
    fs::create_dir_all(&output).expect("create docs output");
    fs::write(output.join("Platform.md"), "# Platform\n").expect("write parent page");
    let context = docs::overview_context(&tree, &[], &output).expect("build overview context");
    assert_eq!(context["Platform"]["components"], Value::Null);
    assert_eq!(
        context["Platform"]["docs_path"],
        json!(output.join("Platform.md"))
    );
    assert_eq!(
        context["Platform"]["children"]["API"]["docs_path"],
        Value::Null
    );
}

#[test]
fn update_routing_decisions_reuse_tree_ownership_and_stale_scan() {
    let (repo, state) = prepared_session(&[("a", "rust"), ("b", "rust")], &["a", "b"]);
    let mut tree = BTreeMap::new();
    tree.insert("Runtime".to_string(), module(&["a"], BTreeMap::new()));
    docs::save_module_tree(&state, &tree, true).expect("save starting tree");

    let root = session::session_root(repo.path(), &state.session_id);
    session::write_json(
        &root.join("changes.json"),
        &ChangeSet {
            added: vec!["b".to_string()],
            ..ChangeSet::default()
        },
    )
    .expect("write change set");
    let decisions = root.join("decisions.json");
    session::write_json(
        &decisions,
        &json!({
            "decisions": [{"component_id": "b", "action": "place", "leaf": "Runtime"}]
        }),
    )
    .expect("write routing decisions");

    let applied = update::apply_routes(&state, &decisions).expect("apply routing decisions");
    assert_eq!(applied["rejected"], json!([]));
    let saved: BTreeMap<String, Module> =
        session::read_json(&session::module_tree_path(&state)).expect("read updated tree");
    assert_eq!(saved["Runtime"].components, vec!["a", "b"]);

    let stale = update::stale_scan(&state).expect("scan stale pages");
    assert!(stale["missing_pages"]
        .as_array()
        .expect("missing pages")
        .iter()
        .any(|page| page == "overview.md"));
}
