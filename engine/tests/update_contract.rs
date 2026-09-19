use codewiki::docs;
use codewiki::model::{ArtifactIndex, ChangeSet, Module, ModuleTree, Node, Summary, UpdateRecord};
use codewiki::session;
use codewiki::update;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use tempfile::tempdir;

fn node(id: &str, path: &str, source: &str, depends_on: &[&str]) -> Node {
    Node {
        id: id.to_string(),
        name: id.split("::").last().unwrap_or(id).to_string(),
        component_type: "function".to_string(),
        file_path: path.to_string(),
        relative_path: path.to_string(),
        source_code: source.to_string(),
        depends_on: depends_on.iter().map(|id| (*id).to_string()).collect(),
        language: "rust".to_string(),
        ..Node::default()
    }
}

fn prepared_session(
    nodes: &[Node],
    leaf_nodes: &[&str],
) -> (tempfile::TempDir, codewiki::session::SessionState) {
    let repo = tempdir().expect("repository tempdir");
    let output = repo.path().join("docs");
    let mut state = session::create(repo.path(), &output).expect("create session");
    let components = nodes
        .iter()
        .cloned()
        .map(|node| (node.id.clone(), node))
        .collect::<BTreeMap<_, _>>();
    session::write_analysis_files(
        &mut state,
        &components,
        &leaf_nodes
            .iter()
            .map(|id| (*id).to_string())
            .collect::<Vec<_>>(),
        &Summary::default(),
        &ArtifactIndex::default(),
    )
    .expect("write analysis files");
    (repo, state)
}

fn nested_tree() -> ModuleTree {
    let mut root_children = BTreeMap::new();
    root_children.insert(
        "API".to_string(),
        Module {
            path: Some("src/api".to_string()),
            components: vec!["src/api.rs::Api".to_string()],
            children: BTreeMap::new(),
        },
    );
    root_children.insert(
        "Runtime".to_string(),
        Module {
            path: Some("src/runtime".to_string()),
            components: vec!["src/runtime.rs::Runtime".to_string()],
            children: BTreeMap::new(),
        },
    );
    ModuleTree::from([(
        "Platform".to_string(),
        Module {
            path: Some("src".to_string()),
            components: vec![
                "src/api.rs::Api".to_string(),
                "src/runtime.rs::Runtime".to_string(),
            ],
            children: root_children,
        },
    )])
}

#[test]
fn route_prefers_deepest_leaf_and_writes_orphan_context() {
    let api = node("src/api.rs::Api", "src/api.rs", "fn api() {}", &[]);
    let runtime = node(
        "src/runtime.rs::Runtime",
        "src/runtime.rs",
        "fn runtime() {}",
        &[],
    );
    let new = node(
        "src/api.rs::new_handler",
        "src/api.rs",
        "fn new_handler() { api(); }",
        &["src/api.rs::Api"],
    );
    let (repo, state) = prepared_session(
        &[api.clone(), runtime.clone(), new.clone()],
        &[
            "src/api.rs::Api",
            "src/runtime.rs::Runtime",
            "src/api.rs::new_handler",
        ],
    );
    let tree = nested_tree();
    docs::save_module_tree(&state, &tree, true).expect("save nested tree");
    let root = session::session_root(repo.path(), &state.session_id);
    session::write_json(
        &root.join("changes.json"),
        &ChangeSet {
            added: vec![new.id.clone()],
            ..ChangeSet::default()
        },
    )
    .expect("write changes");

    let routed = update::route(&state).expect("route changes");
    assert_eq!(routed["routes"][new.id.as_str()], json!("API"));
    let context: Value =
        session::read_json(&root.join("routing_context.json")).expect("routing context");
    assert_eq!(context["orphans"][0]["component_id"], json!(new.id));
    assert_eq!(
        context["module_tree"]["Platform"]["children"]["API"]["components"],
        json!(["src/api.rs::Api"])
    );
}

#[test]
fn route_apply_updates_leaf_and_all_ancestor_aggregate_ids() {
    let api = node("src/api.rs::Api", "src/api.rs", "fn api() {}", &[]);
    let runtime = node(
        "src/runtime.rs::Runtime",
        "src/runtime.rs",
        "fn runtime() {}",
        &[],
    );
    let new = node("src/api.rs::New", "src/api.rs", "fn new() {}", &[]);
    let (repo, state) = prepared_session(
        &[api.clone(), runtime.clone(), new.clone()],
        &[
            "src/api.rs::Api",
            "src/runtime.rs::Runtime",
            "src/api.rs::New",
        ],
    );
    docs::save_module_tree(&state, &nested_tree(), true).expect("save nested tree");
    let root = session::session_root(repo.path(), &state.session_id);
    session::write_json(
        &root.join("changes.json"),
        &ChangeSet {
            added: vec![new.id.clone()],
            ..ChangeSet::default()
        },
    )
    .expect("write changes");
    let decisions = root.join("decisions.json");
    session::write_json(
        &decisions,
        &json!({"decisions": [{"component_id": new.id, "action": "place", "leaf": "API"}]}),
    )
    .expect("write routing decisions");

    let result = update::apply_routes(&state, &decisions).expect("apply routing");
    assert_eq!(result["rejected"], json!([]));
    let saved: ModuleTree =
        session::read_json(&session::module_tree_path(&state)).expect("saved tree");
    assert!(saved["Platform"].children["API"]
        .components
        .contains(&new.id));
    assert!(saved["Platform"].components.contains(&new.id));
    assert_eq!(
        saved["Platform"].children["Runtime"].components,
        vec!["src/runtime.rs::Runtime"]
    );
    assert!(result["validation_path"].as_str().is_some());
}

#[test]
fn context_contains_upstream_referrers_and_orphan_context() {
    let base = node("src/base.rs::Base", "src/base.rs", "fn base() {}", &[]);
    let leaf = node(
        "src/api.rs::Api",
        "src/api.rs",
        "fn api() { base(); }",
        &["src/base.rs::Base"],
    );
    let referrer = node(
        "src/client.rs::Client",
        "src/client.rs",
        "fn client() { api(); }",
        &["src/api.rs::Api"],
    );
    let (repo, state) = prepared_session(
        &[base.clone(), leaf.clone(), referrer.clone()],
        &[
            "src/base.rs::Base",
            "src/api.rs::Api",
            "src/client.rs::Client",
        ],
    );
    let tree = ModuleTree::from([(
        "API".to_string(),
        Module {
            components: vec![leaf.id.clone()],
            ..Module::default()
        },
    )]);
    docs::save_module_tree(&state, &tree, true).expect("save tree");
    let root = session::session_root(repo.path(), &state.session_id);
    session::write_json(
        &root.join("changes.json"),
        &ChangeSet {
            modified_body: vec![leaf.id.clone()],
            added: vec![referrer.id.clone()],
            ..ChangeSet::default()
        },
    )
    .expect("write changes");
    let options = UpdateRecord {
        options: codewiki::model::UpdateOptions {
            k_hop: 1,
            ..Default::default()
        },
        ..UpdateRecord::default()
    };
    session::write_json(&root.join("update_record_draft.json"), &options)
        .expect("write update options");

    let result = update::context(&state).expect("build reports");
    assert_eq!(result["count"], json!(2));
    let api_report: Value = session::read_json(Path::new(
        result["reports"].as_array().unwrap()[0].as_str().unwrap(),
    ))
    .expect("read report");
    let reports = fs::read_dir(root.join("reports"))
        .expect("reports directory")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    assert!(!reports.is_empty());
    let all_reports = reports
        .iter()
        .map(|entry| session::read_json::<Value>(&entry.path()).expect("report JSON"))
        .collect::<Vec<_>>();
    assert!(all_reports.iter().any(|report| {
        report["component_id"] == json!(leaf.id)
            && report["up"].as_array().unwrap().contains(&json!(base.id))
            && report["referrers"]
                .as_array()
                .unwrap()
                .contains(&json!(referrer.id))
    }));
    assert!(api_report["module_tree"].is_object());
    assert!(result["orphan_context_path"].as_str().is_some());
}

#[test]
fn stale_scan_reports_missing_pages_broken_links_and_extra_pages() {
    let leaf = node("src/api.rs::Api", "src/api.rs", "fn api() {}", &[]);
    let (repo, state) = prepared_session(&[leaf], &["src/api.rs::Api"]);
    let tree = ModuleTree::from([(
        "API".to_string(),
        Module {
            components: vec!["src/api.rs::Api".to_string()],
            ..Module::default()
        },
    )]);
    docs::save_module_tree(&state, &tree, true).expect("save tree");
    let mut state = state;
    docs::write_document(&mut state, "API.md", "[missing](missing.md)\n").expect("write leaf page");
    docs::write_document(&mut state, "extra.md", "# extra\n").expect("write extra page");

    let result = update::stale_scan(&state).expect("stale scan");
    assert!(result["missing_pages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|page| page == "overview.md"));
    assert!(result["broken_links"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["target"] == "missing.md"));
    assert_eq!(result["extra_pages"], json!(["extra.md"]));
    let root = session::session_root(repo.path(), &state.session_id);
    assert!(root.join("stale_scan.json").is_file());
}

#[test]
fn finalize_records_verdicts_reports_stale_scan_and_metadata() {
    let leaf = node("src/api.rs::Api", "src/api.rs", "fn api() {}", &[]);
    let (repo, state) = prepared_session(&[leaf], &["src/api.rs::Api"]);
    let tree = ModuleTree::from([(
        "API".to_string(),
        Module {
            components: vec!["src/api.rs::Api".to_string()],
            ..Module::default()
        },
    )]);
    docs::save_module_tree(&state, &tree, true).expect("save tree");
    let mut state = state;
    docs::write_document(&mut state, "API.md", "# API\n").expect("write page");
    docs::write_document(&mut state, "overview.md", "# Overview\n").expect("write overview");
    let root = session::session_root(repo.path(), &state.session_id);
    session::write_json(
        &root.join("update_record_draft.json"),
        &UpdateRecord {
            outcome: "incremental".to_string(),
            options: Default::default(),
            ..UpdateRecord::default()
        },
    )
    .expect("write draft");
    let verdicts = root.join("verdicts.json");
    session::write_json(
        &verdicts,
        &json!({"verdicts": {"API.md": {"verdict": "patch", "reason": "signature"}}}),
    )
    .expect("write verdicts");

    let result =
        update::finalize(&state, "contract-test", Some(&verdicts)).expect("finalize update");
    assert!(result["update_record_path"].as_str().is_some());
    assert!(result["metadata_path"].as_str().is_some());
    let record: Value = session::read_json(&session::output_dir(&state).join("update_record.json"))
        .expect("update record");
    assert_eq!(record["verdicts"]["API"], json!("patch: signature"));
    assert_eq!(record["stale_scan"]["scanned"], json!(true));
    assert!(session::output_dir(&state).join("metadata.json").is_file());
    assert!(BTreeSet::<String>::from_iter(
        record["pages_written"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
    )
    .contains("overview.md"));
}
