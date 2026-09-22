use codewiki::docs::{self, EditOperation};
use codewiki::model::{ArtifactIndex, ChangeSet, Module, ModuleTree, Node, Summary};
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
fn tree_validation_separates_unknown_ids_from_architecture_anchors() {
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
    assert_eq!(validation["unmatched_architecture_ids"], json!(["unknown"]));
    assert_eq!(validation["valid"], json!(false));

    let order: Value = session::read_json(std::path::Path::new(&saved.processing_order_path))
        .expect("read processing order");
    assert_eq!(
        order,
        json!([
            {
                "module": "Leaf",
                "doc_path": "Leaf.md",
                "path": ["Root", "Leaf"],
                "is_leaf": true,
                "components": ["known-leaf"]
            },
            {
                "module": "Root",
                "doc_path": "Root.md",
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
    assert_eq!(validation["unmatched_architecture_ids"], json!([]));
    assert_eq!(
        validation["omitted_analysis_candidate_ids"],
        json!(["known-leaf"])
    );
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
fn processing_order_rejects_legacy_item_names() {
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

    assert!(docs::read_processing_order(&state).is_err());
}

#[test]
fn document_paths_require_current_flat_markdown_names() {
    let (_repo, mut state) = prepared_session(&[], &[]);
    for path in [".repowiki/guide.md", "docs/guide.md", "guide"] {
        assert!(
            docs::write_document(&mut state, path, "# Guide\n").is_err(),
            "old or extensionless path should be rejected: {path}"
        );
    }
    docs::write_document(&mut state, "guide.md", "# Guide\n").expect("write current path");
}

#[test]
fn edit_insert_is_multiline_and_undo_pops_each_saved_version() {
    let (_repo, mut state) = prepared_session(&[], &[]);
    docs::write_document(&mut state, "guide.md", "zero\none\n").expect("write document");

    let insert: EditOperation = serde_json::from_value(json!({
        "kind": "insert",
        "text": "alpha\nbeta",
        "line": 1
    }))
    .expect("decode current insert");
    docs::edit_document(&mut state, "guide.md", &[insert]).expect("insert lines");
    assert_eq!(
        fs::read_to_string(std::path::Path::new(&state.output_dir).join("guide.md"))
            .expect("read inserted document"),
        "zero\nalpha\nbeta\none\n"
    );

    let replace: EditOperation = serde_json::from_value(json!({
        "kind": "str_replace",
        "old": "beta",
        "new": "BETA"
    }))
    .expect("decode current replace");
    docs::edit_document(&mut state, "guide.md", &[replace]).expect("replace text");
    docs::edit_document(&mut state, "guide.md", &[EditOperation::Undo]).expect("undo replace");
    assert_eq!(
        fs::read_to_string(std::path::Path::new(&state.output_dir).join("guide.md"))
            .expect("read undone replacement"),
        "zero\nalpha\nbeta\none\n"
    );

    docs::edit_document(&mut state, "guide.md", &[EditOperation::Undo]).expect("undo insert");
    assert_eq!(
        fs::read_to_string(std::path::Path::new(&state.output_dir).join("guide.md"))
            .expect("read undone insertion"),
        "zero\none\n"
    );

    assert!(serde_json::from_value::<EditOperation>(json!({
        "command": "replace",
        "old_str": "beta",
        "new_str": "BETA"
    }))
    .is_err());
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
    assert!(invalid.quality_valid);
    assert!(invalid.quality_errors.is_empty());
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
fn documentation_quality_rejects_component_list_templates() {
    let (_repo, mut state) = prepared_session(&[("src/lib.rs::run", "rust")], &["src/lib.rs::run"]);
    let mut tree = BTreeMap::new();
    tree.insert(
        "Leaf".to_string(),
        module(&["src/lib.rs::run"], BTreeMap::new()),
    );
    docs::save_module_tree(&state, &tree, true).expect("save documentation tree");
    docs::write_document(
        &mut state,
        "Leaf.md",
        "# Leaf\n\n## Module location\n- src/lib.rs\n\n## Source files\n- src/lib.rs\n\n## Key components\n- src/lib.rs::run\n\n## Integration notes\nThis leaf documents a cohesive implementation area.\n\n[Missing](missing.md)\n",
    )
    .expect("write template page");
    docs::write_document(&mut state, "overview.md", "# Overview\n").expect("write overview page");
    docs::write_document(&mut state, "extra.md", "# Extra\n").expect("write extra page");

    let report =
        docs::validate_documentation_report(&state).expect("documentation report should render");
    assert_eq!(report["valid"], json!(false));
    assert_eq!(
        report["pages"]["Leaf.md"]["boilerplate_detected"],
        json!(true)
    );
    assert!(report["errors"]
        .as_array()
        .unwrap()
        .iter()
        .any(|error| error
            .as_str()
            .unwrap_or_default()
            .contains("component-list template")));
    assert_eq!(report["extra_pages"], json!(["extra.md"]));
    assert_eq!(report["broken_links"][0]["target"], json!("missing.md"));
}

#[test]
fn cluster_response_selects_architecture_anchors_without_structural_fallback() {
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
    assert_eq!(diagnostics["selected_count"], json!(2));
    assert_eq!(diagnostics["omitted_count"], json!(1));
    assert_eq!(tree["Core"].components, vec!["a", "b"]);
    assert!(!serde_json::to_string(&tree).unwrap().contains("\"c\""));

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
fn malformed_cluster_response_is_rejected_without_mutating_the_tree() {
    let (_repo, state) = prepared_session(&[("a", "rust"), ("b", "rust")], &["a", "b"]);
    let input = vec!["a".to_string(), "b".to_string()];
    let mut tree = ModuleTree::new();
    let error = docs::apply_cluster_response(
        &state,
        &mut tree,
        "not a grouped response",
        &input,
        "repo",
        &[],
    )
    .expect_err("malformed clustering response must be retried by the host");
    assert!(error.to_string().contains("no module anchors"));
    assert!(tree.is_empty());
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
fn malformed_super_group_response_is_rejected_without_mutating_the_tree() {
    let mut tree = BTreeMap::new();
    tree.insert("API".to_string(), module(&["a"], BTreeMap::new()));

    let error = docs::apply_super_group_response(&mut tree, "not a grouped response")
        .expect_err("malformed super-group response must be retried by the host");
    assert!(error.to_string().contains("invalid super-group response"));
    assert!(tree.contains_key("API"));
    assert!(!tree.contains_key("Platform"));
}

#[test]
fn overview_context_aggregates_full_dependency_graph_into_architecture_modules() {
    let (_repo, state) = prepared_session(
        &[("api", "rust"), ("runtime", "rust"), ("storage", "rust")],
        &["api"],
    );
    let mut nodes: BTreeMap<String, Node> =
        session::read_json(&session::session_value_path(&state, "components.json"))
            .expect("read graph nodes");
    nodes.get_mut("api").unwrap().relative_path = "src/api.rs".to_string();
    nodes.get_mut("api").unwrap().depends_on = vec!["runtime".to_string()];
    nodes.get_mut("runtime").unwrap().relative_path = "src/runtime.rs".to_string();
    nodes.get_mut("runtime").unwrap().depends_on = vec!["storage".to_string()];
    nodes.get_mut("storage").unwrap().relative_path = "src/storage.rs".to_string();
    session::write_json(
        &session::session_value_path(&state, "components.json"),
        &nodes,
    )
    .expect("write dependency graph");

    let tree = ModuleTree::from([
        (
            "API".to_string(),
            Module {
                path: Some("src/api".to_string()),
                components: vec!["api".to_string()],
                children: BTreeMap::new(),
            },
        ),
        (
            "Runtime".to_string(),
            Module {
                path: Some("src/runtime".to_string()),
                components: vec!["runtime".to_string()],
                children: BTreeMap::new(),
            },
        ),
        (
            "Storage".to_string(),
            Module {
                path: Some("src/storage".to_string()),
                components: Vec::new(),
                children: BTreeMap::new(),
            },
        ),
    ]);
    let context =
        docs::overview_context_for_session(&state, &tree, &[], &session::output_dir(&state))
            .expect("build architecture context");
    let edges = context["architecture_context"]["edges"]
        .as_array()
        .expect("architecture edges");
    assert!(
        edges
            .iter()
            .any(|edge| edge["from"] == "API" && edge["to"] == "Runtime"),
        "{context}"
    );
    assert!(
        edges
            .iter()
            .any(|edge| edge["from"] == "Runtime" && edge["to"] == "Storage"),
        "{context}"
    );
    assert_eq!(
        context["architecture_context"]["primary_paths"][0],
        json!(["API", "Runtime", "Storage"])
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
