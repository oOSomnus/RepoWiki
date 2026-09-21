use codewiki::model::Node;
use codewiki::prompts::{self, PromptType, MAX_USER_PROMPT_CHARS};
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn string_vars(pairs: &[(&str, &str)]) -> BTreeMap<String, Value> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_string(), Value::String((*value).to_string())))
        .collect()
}

fn renderable_vars(kind: PromptType) -> BTreeMap<String, Value> {
    match kind {
        PromptType::Cluster => string_vars(&[
            ("potential_core_components", "src/lib.rs::run"),
            ("scope", "repo"),
        ]),
        PromptType::SuperGroup => string_vars(&[("formatted_modules", "API\nRuntime")]),
        PromptType::FilterFolders => string_vars(&[("project_name", "fixture"), ("files", "src")]),
        PromptType::SystemComplex | PromptType::SystemLeaf => {
            string_vars(&[("module_name", "Runtime")])
        }
        PromptType::User => string_vars(&[
            ("module_name", "Runtime"),
            ("module_tree", "Runtime\n  API"),
            ("formatted_core_component_codes", "# File: src/lib.rs\n"),
        ]),
        PromptType::OverviewModule => {
            string_vars(&[("module_name", "Runtime"), ("repo_structure", "Runtime")])
        }
        PromptType::OverviewRepo => {
            string_vars(&[("repo_name", "fixture"), ("repo_structure", "Runtime")])
        }
        PromptType::UpdateLeafSystem => string_vars(&[("leaf_name", "Runtime")]),
        PromptType::UpdateLeafUser => string_vars(&[
            ("leaf_name", "Runtime"),
            ("mode", "edit"),
            ("mode_note", "patch"),
            ("write_set", "Runtime.md"),
            ("report", "changed"),
            ("module_tree", "Runtime"),
            ("leaf_components", "src/lib.rs::run"),
            ("leaf_page", "# Runtime"),
        ]),
        PromptType::RoutingSystem | PromptType::StaleFixSystem => BTreeMap::new(),
        PromptType::RoutingUser => string_vars(&[("module_tree", "Runtime"), ("orphans", "[]")]),
        PromptType::StaleFixUser => string_vars(&[("page", "Runtime.md"), ("items", "none")]),
    }
}

fn node(
    id: &str,
    path: &str,
    component_type: &str,
    source_code: &str,
    artifact_class: Option<&str>,
) -> Node {
    Node {
        id: id.to_string(),
        name: id.split("::").last().unwrap_or(id).to_string(),
        component_type: component_type.to_string(),
        relative_path: path.to_string(),
        source_code: source_code.to_string(),
        language: "rust".to_string(),
        artifact_class: artifact_class.map(str::to_string),
        ..Node::default()
    }
}

#[test]
fn prompt_catalog_round_trips_every_runtime_prompt() {
    let names = prompts::catalog();
    assert_eq!(names.len(), PromptType::all().len());
    assert_eq!(
        names,
        PromptType::all()
            .iter()
            .map(|kind| kind.as_str())
            .collect::<Vec<_>>()
    );

    for kind in PromptType::all() {
        assert_eq!(
            PromptType::parse(kind.as_str()).expect("prompt parses"),
            *kind
        );
        assert!(
            prompts::catalog_specs()
                .iter()
                .any(|spec| spec["type"] == kind.as_str()),
            "missing catalog specification for {}",
            kind.as_str()
        );
    }
}

#[test]
fn every_prompt_renders_without_known_unresolved_placeholders_or_legacy_tools() {
    let placeholders = [
        "{potential_core_components}",
        "{component_ids}",
        "{scope}",
        "{module_name}",
        "{module_tree}",
        "{formatted_modules}",
        "{formatted_core_component_codes}",
        "{repo_name}",
        "{repo_structure}",
        "{leaf_name}",
        "{custom_instructions}",
        "{few_shot_examples}",
        "{architecture_context}",
        "{mode}",
        "{mode_note}",
        "{write_set}",
        "{report}",
        "{leaf_components}",
        "{leaf_page}",
        "{orphans}",
        "{page}",
        "{items}",
    ];
    for kind in PromptType::all() {
        let prompt = prompts::render(*kind, &renderable_vars(*kind)).expect("prompt renders");
        for placeholder in placeholders {
            assert!(
                !prompt.contains(placeholder),
                "{} left unresolved placeholder {}",
                kind.as_str(),
                placeholder
            );
        }
        for legacy in [
            "str_replace_editor",
            "read_code_components",
            "generate_sub_module_documentation",
        ] {
            assert!(
                !prompt.contains(legacy),
                "{} contains legacy host tool {}",
                kind.as_str(),
                legacy
            );
        }
    }
}

#[test]
fn cluster_scope_selects_repository_or_module_architecture_contract() {
    let mut repo = string_vars(&[
        ("potential_core_components", "src/lib.rs::run"),
        ("scope", "repo"),
    ]);
    let repo_prompt = prompts::render(PromptType::Cluster, &repo).expect("repo prompt");
    assert!(repo_prompt.contains("architecture map"));
    assert!(repo_prompt.contains("representative component IDs"));
    assert!(repo_prompt.contains("directory classification"));

    repo.insert("scope".to_string(), Value::String("module".to_string()));
    repo.insert(
        "module_name".to_string(),
        Value::String("Runtime".to_string()),
    );
    repo.insert(
        "module_tree".to_string(),
        json!({
            "Runtime": {
                "components": ["src/lib.rs::run"],
                "children": {"API": {"components": [], "children": {}}}
            }
        }),
    );
    let module_prompt = prompts::render(PromptType::Cluster, &repo).expect("module prompt");
    assert!(module_prompt.contains("existing module"));
    assert!(module_prompt.contains("representative component IDs"));
    assert!(module_prompt.contains("Keep the tree\nshallow"));
    assert!(module_prompt.contains("Do not create a directory-shaped child"));
}

#[test]
fn component_listing_and_source_are_grouped_by_file_and_preserve_ids() {
    let ids = vec![
        "src/api.rs::Api".to_string(),
        "src/api.rs::Api.run".to_string(),
        "Makefile::build".to_string(),
    ];
    let nodes = BTreeMap::from([
        (
            ids[0].clone(),
            node(&ids[0], "src/api.rs", "class", "struct Api;\n", None),
        ),
        (
            ids[1].clone(),
            node(&ids[1], "src/api.rs", "method", "fn run() {}\n", None),
        ),
        (
            ids[2].clone(),
            node(
                &ids[2],
                "Makefile",
                "artifact",
                "build:\n\tcargo build\n",
                Some("build"),
            ),
        ),
    ]);
    let listing = prompts::format_component_listing(&ids, &nodes);
    assert!(listing.contains("# src/api.rs"));
    assert!(listing.contains("# Makefile (artifact: build)"));
    for id in &ids {
        assert!(listing.contains(id), "listing lost {id}");
    }

    let source = prompts::format_component_codes(&ids, &nodes);
    assert!(source.contains("# File: src/api.rs"));
    assert!(source.contains("```rust"));
    assert!(source.contains("struct Api;"));
    assert!(source.contains("# File: Makefile"));
    assert!(source.contains("```makefile"));
    assert!(source.contains("cargo build"));
    for id in &ids {
        assert!(source.contains(id), "source prompt lost {id}");
    }
}

#[test]
fn artifact_context_is_opt_in_and_overview_explains_build_and_run() {
    let mut user = string_vars(&[
        ("module_name", "Build"),
        ("module_tree", "Build"),
        ("formatted_core_component_codes", "Makefile"),
    ]);
    user.insert(
        "artifact_index".to_string(),
        Value::String("Makefile (build)".to_string()),
    );
    let prompt = prompts::render(PromptType::User, &user).expect("artifact user prompt");
    assert!(prompt.contains("<ARTIFACT_INDEX>"));
    assert!(prompt.contains("Makefile (build)"));
    assert!(prompt.contains("How it is built and run") || prompt.contains("read the relevant"));

    user.remove("artifact_index");
    let without_artifact = prompts::render(PromptType::User, &user).expect("code-only prompt");
    assert!(!without_artifact.contains("<ARTIFACT_INDEX>"));

    let mut overview = string_vars(&[
        ("repo_name", "fixture"),
        ("repo_structure", "Build\n  Runtime"),
    ]);
    overview.insert(
        "artifact_index".to_string(),
        Value::String("Makefile (build)".to_string()),
    );
    let repo_prompt =
        prompts::render(PromptType::OverviewRepo, &overview).expect("overview prompt");
    assert!(repo_prompt.contains("How it is built and run"));
    assert!(repo_prompt.contains("Makefile (build)"));
}

#[test]
fn architecture_context_and_few_shots_are_first_class_prompt_inputs() {
    let vars = string_vars(&[
        ("module_name", "Query Pipeline"),
        ("module_tree", "Query Pipeline\n  Storage"),
        ("formatted_core_component_codes", "src/query.rs::execute"),
        (
            "architecture_context",
            "Query Pipeline -> Storage: reads parts",
        ),
        (
            "few_shot_examples",
            "A reference page explains one concrete data path.",
        ),
    ]);
    let prompt = prompts::render(PromptType::User, &vars).expect("architecture user prompt");
    assert!(prompt.contains("Query Pipeline -> Storage: reads parts"));
    assert!(prompt.contains("A reference page explains one concrete data path."));

    let overview_vars = string_vars(&[
        ("repo_name", "fixture"),
        ("repo_structure", "Query Pipeline"),
        (
            "architecture_context",
            "Query Pipeline -> Storage: reads parts",
        ),
        (
            "few_shot_examples",
            "A reference page explains one concrete data path.",
        ),
    ]);
    let overview =
        prompts::render(PromptType::OverviewRepo, &overview_vars).expect("overview prompt");
    assert!(overview.contains("Query Pipeline -> Storage: reads parts"));
    assert!(overview.contains("primary Mermaid architecture diagram"));
}

#[test]
fn prompt_validation_rejects_bad_scope_and_component_input() {
    let mut cluster = string_vars(&[("potential_core_components", "src/lib.rs::run")]);
    cluster.insert("scope".to_string(), Value::String("bad".to_string()));
    assert!(prompts::render(PromptType::Cluster, &cluster).is_err());

    let mut module = string_vars(&[("scope", "module"), ("potential_core_components", "x")]);
    assert!(prompts::render(PromptType::Cluster, &module).is_err());

    module.insert(
        "module_name".to_string(),
        Value::String("Runtime".to_string()),
    );
    module.insert("module_tree".to_string(), json!({}));
    module.insert("component_ids".to_string(), json!(["ok", 3]));
    assert!(prompts::render(PromptType::Cluster, &module).is_err());
}

#[test]
fn oversized_prompt_is_capped_without_splitting_unicode() {
    let mut vars = string_vars(&[("module_name", "Runtime"), ("module_tree", "Runtime")]);
    vars.insert(
        "formatted_core_component_codes".to_string(),
        Value::String("🙂中".repeat(MAX_USER_PROMPT_CHARS)),
    );
    let prompt = prompts::user_prompt_with_limits(PromptType::User, &vars).expect("bounded prompt");
    assert!(prompt.chars().count() <= MAX_USER_PROMPT_CHARS);
    assert!(prompt.contains("prompt content truncated") || prompt.contains("🙂"));
    assert!(std::str::from_utf8(prompt.as_bytes()).is_ok());
}
