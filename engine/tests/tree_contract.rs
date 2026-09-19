use codewiki::docs;
use codewiki::model::{ArtifactIndex, Module, ModuleTree, Node, Summary};
use codewiki::session;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use tempfile::tempdir;

fn node(id: &str, component_type: &str, source_code: &str) -> Node {
    Node {
        id: id.to_string(),
        name: id.split("::").last().unwrap_or(id).to_string(),
        component_type: component_type.to_string(),
        relative_path: id.split("::").next().unwrap_or(id).to_string(),
        source_code: source_code.to_string(),
        language: "rust".to_string(),
        artifact_class: (component_type == "artifact").then(|| "build".to_string()),
        ..Node::default()
    }
}

fn prepared_session(
    nodes: &[(&str, &str, &str)],
    leaf_nodes: &[&str],
    summary: Summary,
) -> (tempfile::TempDir, codewiki::session::SessionState) {
    let repo = tempdir().expect("repository tempdir");
    let output = repo.path().join("docs");
    let mut state = session::create(repo.path(), &output).expect("create session");
    let nodes = nodes
        .iter()
        .map(|(id, kind, source)| ((*id).to_string(), node(id, kind, source)))
        .collect::<BTreeMap<_, _>>();
    session::write_analysis_files(
        &mut state,
        &nodes,
        &leaf_nodes
            .iter()
            .map(|id| (*id).to_string())
            .collect::<Vec<_>>(),
        &summary,
        &ArtifactIndex::default(),
    )
    .expect("write analysis files");
    (repo, state)
}

fn leaves(tree: &ModuleTree) -> Vec<String> {
    fn visit(modules: &ModuleTree, result: &mut Vec<String>) {
        for module in modules.values() {
            if module.children.is_empty() {
                result.extend(module.components.iter().cloned());
            } else {
                visit(&module.children, result);
            }
        }
    }
    let mut result = Vec::new();
    visit(tree, &mut result);
    result.sort();
    result
}

fn descendants(module: &Module) -> BTreeSet<String> {
    let mut ids = module.components.iter().cloned().collect::<BTreeSet<_>>();
    for child in module.children.values() {
        ids.extend(descendants(child));
    }
    ids
}

fn assert_recursive_invariants(tree: &ModuleTree, expected_leaf_ids: &[&str]) {
    let actual = leaves(tree).into_iter().collect::<BTreeSet<_>>();
    let expected = expected_leaf_ids
        .iter()
        .map(|id| (*id).to_string())
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, expected, "leaf ownership changed");

    fn visit(modules: &ModuleTree, parent_ids: Option<&BTreeSet<String>>) {
        for module in modules.values() {
            let own = module.components.iter().cloned().collect::<BTreeSet<_>>();
            if let Some(parent) = parent_ids {
                assert!(
                    own.iter().all(|id| parent.contains(id)),
                    "child contains IDs missing from its parent: child={own:?} parent={parent:?}"
                );
            }
            let aggregate = descendants(module);
            assert!(
                aggregate.iter().all(|id| own.contains(id)),
                "parent does not aggregate descendants"
            );
            visit(&module.children, Some(&own));
        }
    }
    visit(tree, None);
}

#[test]
fn cluster_response_preserves_exact_leaf_coverage_and_parent_aggregates() {
    let (_repo, state) = prepared_session(
        &[
            ("src/api.rs::Api", "class", "struct Api;"),
            ("src/runtime.rs::Runtime", "class", "struct Runtime;"),
            ("src/config.rs::Config", "class", "struct Config;"),
        ],
        &[
            "src/api.rs::Api",
            "src/runtime.rs::Runtime",
            "src/config.rs::Config",
        ],
        Summary::default(),
    );
    let mut tree = ModuleTree::new();
    let input = [
        "src/api.rs::Api".to_string(),
        "src/runtime.rs::Runtime".to_string(),
        "src/config.rs::Config".to_string(),
    ];
    let root = docs::apply_cluster_response(
        &state,
        &mut tree,
        r#"<GROUPED_COMPONENTS>{"Platform":{"path":"src","components":["src/api.rs::Api","src/runtime.rs::Runtime","src/config.rs::Config"]}}</GROUPED_COMPONENTS>"#,
        &input,
        "repo",
        &[],
    )
    .expect("apply root clustering");
    assert_eq!(root["fallback_used"], json!(false));

    let module_path = vec!["Platform".to_string()];
    let nested = docs::apply_cluster_response(
        &state,
        &mut tree,
        r#"<GROUPED_COMPONENTS>{"API":{"path":"src","components":["src/api.rs::Api"]},"Runtime":{"path":"src","components":["src/runtime.rs::Runtime"]},"Configuration":{"path":"src","components":["src/config.rs::Config"]}}</GROUPED_COMPONENTS>"#,
        &input,
        "module",
        &module_path,
    )
    .expect("apply recursive clustering");
    assert_eq!(nested["scope"], json!("module"));
    assert!(tree["Platform"].children.contains_key("API"));
    assert!(tree["Platform"].children.contains_key("Runtime"));
    assert!(tree["Platform"].children.contains_key("Configuration"));
    assert_recursive_invariants(
        &tree,
        &[
            "src/api.rs::Api",
            "src/runtime.rs::Runtime",
            "src/config.rs::Config",
        ],
    );
}

#[test]
fn malformed_or_partial_cluster_response_rescues_missing_ids_without_inventing_ids() {
    let (_repo, state) = prepared_session(
        &[
            ("src/a.rs::A", "class", "struct A;"),
            ("src/b.rs::B", "class", "struct B;"),
            ("src/c.rs::C", "class", "struct C;"),
        ],
        &["src/a.rs::A", "src/b.rs::B", "src/c.rs::C"],
        Summary::default(),
    );
    let input = [
        "src/a.rs::A".to_string(),
        "src/b.rs::B".to_string(),
        "src/c.rs::C".to_string(),
    ];
    let mut tree = ModuleTree::new();
    let result = docs::apply_cluster_response(
        &state,
        &mut tree,
        r#"<GROUPED_COMPONENTS>{"Core":{"components":["src/a.rs::A","src/a.rs::A","src/unknown.rs::X"]},"Runtime":{"components":["src/b.rs::B"]}}</GROUPED_COMPONENTS>"#,
        &input,
        "repo",
        &[],
    )
    .expect("bad model response is structurally recoverable");
    assert_eq!(result["fallback_used"], json!(true));
    assert!(result["diagnostics"]
        .as_array()
        .expect("diagnostics")
        .iter()
        .any(|item| item.as_str().unwrap_or_default().contains("duplicate")));
    assert!(!serde_json::to_string(&tree)
        .unwrap()
        .contains("src/unknown.rs::X"));
    assert_recursive_invariants(&tree, &["src/a.rs::A", "src/b.rs::B", "src/c.rs::C"]);

    let mut empty_tree = ModuleTree::new();
    let empty = docs::apply_cluster_response(
        &state,
        &mut empty_tree,
        "not a grouped response",
        &input,
        "repo",
        &[],
    )
    .expect("empty response fallback");
    assert_eq!(empty["fallback_used"], json!(true));
    assert_recursive_invariants(&empty_tree, &["src/a.rs::A", "src/b.rs::B", "src/c.rs::C"]);
}

#[test]
fn super_group_preserves_existing_pages_and_rejects_invalid_members() {
    let (_repo, _state) = prepared_session(
        &[
            ("src/a.rs::A", "class", "struct A;"),
            ("src/b.rs::B", "class", "struct B;"),
        ],
        &["src/a.rs::A", "src/b.rs::B"],
        Summary::default(),
    );
    let mut tree = ModuleTree::from([
        (
            "API".to_string(),
            Module {
                path: Some("src/api".to_string()),
                components: vec!["src/a.rs::A".to_string()],
                children: BTreeMap::new(),
            },
        ),
        (
            "Runtime".to_string(),
            Module {
                path: Some("src/runtime".to_string()),
                components: vec!["src/b.rs::B".to_string()],
                children: BTreeMap::new(),
            },
        ),
    ]);
    let result = docs::apply_super_group_response(
        &mut tree,
        r#"<GROUPED_MODULES>{"Platform":{"modules":["API","Runtime","Missing"]}}</GROUPED_MODULES>"#,
    )
    .expect("apply super group");
    assert_eq!(result["changed"], json!(true));
    assert!(result["diagnostics"]
        .as_array()
        .expect("diagnostics")
        .iter()
        .any(|item| item.as_str().unwrap_or_default().contains("unknown module")));
    assert_eq!(
        tree["Platform"].children["API"].path.as_deref(),
        Some("src/api")
    );
    assert_eq!(
        tree["Platform"].children["Runtime"].components,
        vec!["src/b.rs::B"]
    );
    assert_eq!(tree["Platform"].components.len(), 2);
}

#[test]
fn overview_context_strips_components_and_exposes_only_target_children_docs() {
    let mut child = BTreeMap::new();
    child.insert(
        "API".to_string(),
        Module {
            path: Some("src/api".to_string()),
            components: vec!["a".to_string()],
            children: BTreeMap::new(),
        },
    );
    let mut tree = ModuleTree::new();
    tree.insert(
        "Platform".to_string(),
        Module {
            path: Some("src".to_string()),
            components: vec!["a".to_string(), "b".to_string()],
            children: child,
        },
    );
    let output = tempdir().expect("overview docs directory");
    fs::write(output.path().join("API.md"), "# API\n").expect("write child page");
    let context = docs::overview_context(&tree, &["Platform".to_string()], output.path())
        .expect("overview context");
    let serialized = serde_json::to_string(&context).expect("serialize context");
    assert!(!serialized.contains("\"a\""));
    assert_eq!(context["Platform"]["components"], Value::Null);
    assert_eq!(
        context["Platform"]["is_target_for_overview_generation"],
        json!(true)
    );
    assert_eq!(
        context["Platform"]["children"]["API"]["docs_path"],
        json!(output.path().join("API.md"))
    );
    assert_eq!(context["Platform"]["docs_path"], Value::Null);
}

#[test]
fn artifact_coverage_rescues_artifact_leaves_and_keeps_existing_ownership() {
    let (_repo, state) = prepared_session(
        &[
            ("src/main.rs::main", "function", "fn main() {}"),
            ("Makefile::build", "artifact", "build:\n\tcargo build\n"),
            ("Dockerfile::Dockerfile", "artifact", "FROM rust:latest\n"),
        ],
        &[
            "src/main.rs::main",
            "Makefile::build",
            "Dockerfile::Dockerfile",
        ],
        Summary::default(),
    );
    let mut tree = ModuleTree::from([(
        "Runtime".to_string(),
        Module {
            path: Some("src".to_string()),
            components: vec!["src/main.rs::main".to_string()],
            children: BTreeMap::new(),
        },
    )]);
    let rescued = docs::ensure_artifact_coverage(&state, &mut tree).expect("rescue artifacts");
    assert_eq!(
        rescued,
        vec![
            "Dockerfile::Dockerfile".to_string(),
            "Makefile::build".to_string()
        ]
    );
    let artifact_module = tree
        .iter()
        .find(|(name, _)| name.starts_with("Build__Deployment_and_Configuration"))
        .map(|(_, module)| module)
        .unwrap_or_else(|| panic!("artifact module missing: {tree:?}"));
    assert_eq!(
        artifact_module.components.iter().collect::<BTreeSet<_>>(),
        BTreeSet::from([
            &"Dockerfile::Dockerfile".to_string(),
            &"Makefile::build".to_string()
        ])
    );
    assert_eq!(tree["Runtime"].components, vec!["src/main.rs::main"]);
}

#[test]
fn final_tree_quality_distinguishes_batch_size_from_leaf_size() {
    let (_repo, state) = prepared_session(
        &[
            ("src/a.rs::A", "class", "fn a() {}"),
            ("src/b.rs::B", "class", "fn b() {}"),
        ],
        &["src/a.rs::A", "src/b.rs::B"],
        Summary {
            max_token_per_module: 100,
            max_token_per_leaf_module: 1,
            cluster_batch_size: 1,
            ..Summary::default()
        },
    );
    let tree = ModuleTree::from([
        (
            "A".to_string(),
            Module {
                path: Some("src".to_string()),
                components: vec!["src/a.rs::A".to_string()],
                children: BTreeMap::new(),
            },
        ),
        (
            "B".to_string(),
            Module {
                path: Some("src".to_string()),
                components: vec!["src/b.rs::B".to_string()],
                children: BTreeMap::new(),
            },
        ),
    ]);
    let saved = docs::save_module_tree(&state, &tree, true).expect("save valid leaves");
    assert!(saved.quality_valid);
    let validation: Value =
        session::read_json(Path::new(&saved.validation_path)).expect("validation");
    assert_eq!(validation["quality_limits"]["cluster_batch_size"], json!(1));
    assert_eq!(validation["oversized_leaf_modules"], json!([]));
    assert_eq!(validation["oversized_leaf_warnings"], json!([]));
}
