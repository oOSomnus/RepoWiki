#[path = "common/mod.rs"]
mod common;

use repowiki::docs;
use repowiki::model::{ArtifactIndex, ChangeSet, Module, ModuleTree, Node, Summary, UpdateRecord};
use repowiki::session;
use repowiki::update;
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
) -> (tempfile::TempDir, repowiki::session::SessionState) {
    common::prepared_session(nodes.to_vec(), leaf_nodes, Summary::default())
}

fn nested_tree() -> ModuleTree {
    let mut root_children = BTreeMap::new();
    root_children.insert(
        "API".to_string(),
        Module {
            path: Some("src/api".to_string()),
            components: vec!["src/api.rs::Api".to_string()],
            children: BTreeMap::new(),
            decomposition_review: None,
        },
    );
    root_children.insert(
        "Runtime".to_string(),
        Module {
            path: Some("src/runtime".to_string()),
            components: vec!["src/runtime.rs::Runtime".to_string()],
            children: BTreeMap::new(),
            decomposition_review: None,
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
            decomposition_review: None,
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
    assert_eq!(
        routed["routes"][new.id.as_str()],
        json!("repo:platform:api:start")
    );
    let context: Value =
        session::read_json(&root.join("routing_context.json")).expect("routing context");
    assert_eq!(context["orphans"][0]["component_id"], json!(new.id));
    assert_eq!(
        context["module_tree"]["Platform"]["children"]["API"]["components"],
        json!(["src/api.rs::Api"])
    );
}

#[test]
fn same_named_modules_under_different_parents_route_to_distinct_page_ids() {
    let admin_api = node(
        "src/admin/api.rs::Api",
        "src/admin/api.rs",
        "fn api() {}",
        &[],
    );
    let public_api = node(
        "src/public/api.rs::Api",
        "src/public/api.rs",
        "fn api() {}",
        &[],
    );
    let admin_new = node(
        "src/admin/api.rs::New",
        "src/admin/api.rs",
        "fn new() {}",
        &[admin_api.id.as_str()],
    );
    let public_new = node(
        "src/public/api.rs::New",
        "src/public/api.rs",
        "fn new() {}",
        &[public_api.id.as_str()],
    );
    let (repo, state) = prepared_session(
        &[
            admin_api.clone(),
            public_api.clone(),
            admin_new.clone(),
            public_new.clone(),
        ],
        &[
            "src/admin/api.rs::Api",
            "src/public/api.rs::Api",
            "src/admin/api.rs::New",
            "src/public/api.rs::New",
        ],
    );
    let mut admin_children = BTreeMap::new();
    admin_children.insert(
        "API".to_string(),
        Module {
            components: vec![admin_api.id.clone()],
            ..Module::default()
        },
    );
    let mut public_children = BTreeMap::new();
    public_children.insert(
        "API".to_string(),
        Module {
            components: vec![public_api.id.clone()],
            ..Module::default()
        },
    );
    let tree = ModuleTree::from([
        (
            "Admin".to_string(),
            Module {
                components: vec![admin_api.id.clone()],
                children: admin_children,
                ..Module::default()
            },
        ),
        (
            "Public".to_string(),
            Module {
                components: vec![public_api.id.clone()],
                children: public_children,
                ..Module::default()
            },
        ),
    ]);
    docs::save_module_tree(&state, &tree, true).expect("save tree");
    let root = session::session_root(repo.path(), &state.session_id);
    session::write_json(
        &root.join("changes.json"),
        &ChangeSet {
            added: vec![admin_new.id.clone(), public_new.id.clone()],
            ..ChangeSet::default()
        },
    )
    .expect("write changes");

    let routed = update::route(&state).expect("route changes");
    assert_eq!(
        routed["routes"][admin_new.id.as_str()],
        json!("repo:admin:api:start")
    );
    assert_eq!(
        routed["routes"][public_new.id.as_str()],
        json!("repo:public:api:start")
    );

    let decisions = root.join("decisions.json");
    session::write_json(
        &decisions,
        &json!({
            "decisions": [
                {
                    "component_id": admin_new.id,
                    "action": "place",
                    "leaf": "repo:admin:api:start"
                },
                {
                    "component_id": public_new.id,
                    "action": "place",
                    "leaf": "repo:public:api:start"
                }
            ]
        }),
    )
    .expect("write routing decisions");
    let applied = update::apply_routes(&state, &decisions).expect("apply routing decisions");
    assert_eq!(applied["rejected"], json!([]));
    let saved: ModuleTree =
        session::read_json(&session::module_tree_path(&state)).expect("read updated tree");
    assert!(saved["Admin"].children["API"]
        .components
        .contains(&admin_new.id));
    assert!(!saved["Admin"].children["API"]
        .components
        .contains(&public_new.id));
    assert!(saved["Public"].children["API"]
        .components
        .contains(&public_new.id));
    assert!(!saved["Public"].children["API"]
        .components
        .contains(&admin_new.id));
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
        &json!({
            "decisions": [{
                "component_id": new.id,
                "action": "place",
                "leaf": "repo:platform:api:start"
            }]
        }),
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
fn update_plan_write_sets_use_canonical_page_ids() {
    let api = node("src/api.rs::Api", "src/api.rs", "fn api() {}", &[]);
    let (repo, state) = prepared_session(std::slice::from_ref(&api), &["src/api.rs::Api"]);
    let tree = ModuleTree::from([(
        "System".to_string(),
        Module {
            components: vec![api.id.clone()],
            children: BTreeMap::from([(
                "API".to_string(),
                Module {
                    components: vec![api.id.clone()],
                    ..Module::default()
                },
            )]),
            ..Module::default()
        },
    )]);
    docs::save_module_tree(&state, &tree, true).expect("save tree");
    let graph = session::output_dir(&state)
        .join("temp")
        .join("dependency_graphs")
        .join("current.json");
    fs::create_dir_all(graph.parent().expect("graph directory")).expect("create graph directory");
    session::write_json(&graph, &BTreeMap::from([(api.id.clone(), api.clone())]))
        .expect("write current graph");

    update::plan(&state, &Default::default()).expect("plan update");
    let record: UpdateRecord = session::read_json(
        &session::session_root(repo.path(), &state.session_id).join("update_record_draft.json"),
    )
    .expect("read update plan");
    assert_eq!(
        record.write_sets[&api.id],
        vec![
            "repo:system:api:start".to_string(),
            "repo:start".to_string()
        ]
    );
}

#[test]
fn update_plan_write_sets_stay_in_change_wiki_namespace() {
    let repo = tempdir().expect("repository tempdir");
    let change_id = format!("{}..{}", "a".repeat(40), "b".repeat(40));
    let output = repo
        .path()
        .join(".repowiki")
        .join("changes")
        .join(change_id.clone());
    let mut state = session::create(repo.path(), &output).expect("create change session");
    let api = node("src/api.rs::Api", "src/api.rs", "fn api() {}", &[]);
    let components = BTreeMap::from([(api.id.clone(), api.clone())]);
    session::write_analysis_files(
        &mut state,
        &components,
        &["src/api.rs::Api".to_string()],
        &Summary::default(),
        &ArtifactIndex::default(),
    )
    .expect("write analysis files");
    let tree = ModuleTree::from([(
        "API".to_string(),
        Module {
            components: vec![api.id.clone()],
            ..Module::default()
        },
    )]);
    docs::save_module_tree(&state, &tree, true).expect("save tree");
    let graph = session::output_dir(&state)
        .join("temp")
        .join("dependency_graphs")
        .join("current.json");
    fs::create_dir_all(graph.parent().expect("graph directory")).expect("create graph directory");
    session::write_json(&graph, &components).expect("write current graph");

    update::plan(&state, &Default::default()).expect("plan change update");
    let record: UpdateRecord = session::read_json(
        &session::session_root(repo.path(), &state.session_id).join("update_record_draft.json"),
    )
    .expect("read update plan");
    let wiki_id = session::change_wiki_id(&change_id).expect("change wiki ID");
    assert_eq!(state.wiki_id, wiki_id);
    assert_eq!(
        record.write_sets[&api.id],
        vec![format!("{wiki_id}:api:start"), format!("{wiki_id}:start")]
    );
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
        options: repowiki::model::UpdateOptions {
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
    if !common::dokuwiki_runtime_available() {
        return;
    }
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
    let api_page =
        docs::module_page_id(&state.wiki_id, &["API".to_string()]).expect("module page ID");
    let overview_page = docs::overview_page_id(&state.wiki_id).expect("overview page ID");
    let missing_page = format!("{}:missing:start", state.wiki_id);
    let extra_page = format!("{}:extra:start", state.wiki_id);
    let api_content = format!("====== API ======\n\n[[{missing_page}|missing page]]\n");
    docs::write_document(&mut state, &api_page, &api_content).expect("write API page");
    docs::write_document(&mut state, &extra_page, "====== Extra ======\n")
        .expect("write extra page");

    let result = update::stale_scan(&state).expect("stale scan");
    assert!(result["missing_pages"]
        .as_array()
        .unwrap()
        .iter()
        .any(|page| page.as_str() == Some(overview_page.as_str())));
    assert!(result["broken_links"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| {
            item["page"].as_str() == Some(api_page.as_str())
                && item["target"].as_str() == Some(missing_page.as_str())
        }));
    assert_eq!(result["extra_pages"], json!([extra_page]));
    let api_file = docs::page_file_path(&session::output_dir(&state), &state.wiki_id, &api_page)
        .expect("page file path");
    assert_eq!(
        api_file
            .extension()
            .and_then(|extension| extension.to_str()),
        Some("txt")
    );
    assert!(api_file.is_file());
    let root = session::session_root(repo.path(), &state.session_id);
    assert!(root.join("stale_scan.json").is_file());
}

#[test]
fn finalize_records_verdicts_reports_stale_scan_and_metadata() {
    if !common::dokuwiki_runtime_available() {
        return;
    }
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
    let api_page =
        docs::module_page_id(&state.wiki_id, &["API".to_string()]).expect("module page ID");
    let overview_page = docs::overview_page_id(&state.wiki_id).expect("overview page ID");
    docs::write_document(&mut state, &api_page, "====== API ======\n").expect("write API page");
    docs::write_document(&mut state, &overview_page, "====== Overview ======\n")
        .expect("write overview");
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
        &json!({
            "verdicts": {
                "repo:api:start": {"verdict": "patch", "reason": "signature"}
            }
        }),
    )
    .expect("write verdicts");

    let result =
        update::finalize(&state, "contract-test", Some(&verdicts)).expect("finalize update");
    assert!(result["update_record_path"].as_str().is_some());
    assert!(result["metadata_path"].as_str().is_some());
    let record: Value = session::read_json(&session::output_dir(&state).join("update_record.json"))
        .expect("update record");
    assert_eq!(
        record["verdicts"]["repo:api:start"],
        json!("patch: signature")
    );
    assert_eq!(record["stale_scan"]["scanned"], json!(true));
    assert!(session::output_dir(&state).join("metadata.json").is_file());
    let pages_written = BTreeSet::<String>::from_iter(
        record["pages_written"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string),
    );
    assert!(pages_written.contains("repo:start"));
    assert!(pages_written.contains("repo:api:start"));
    let api_file = docs::page_file_path(&session::output_dir(&state), &state.wiki_id, &api_page)
        .expect("page file path");
    assert!(api_file.is_file());
}
