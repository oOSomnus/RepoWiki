use crate::model::Node;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PromptType {
    Cluster,
    SuperGroup,
    FilterFolders,
    SystemComplex,
    SystemLeaf,
    User,
    OverviewModule,
    OverviewRepo,
    UpdateLeafSystem,
    UpdateLeafUser,
    RoutingSystem,
    RoutingUser,
    StaleFixSystem,
    StaleFixUser,
}

impl PromptType {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "cluster" => Ok(Self::Cluster),
            "super_group" => Ok(Self::SuperGroup),
            "filter_folders" => Ok(Self::FilterFolders),
            "system_complex" => Ok(Self::SystemComplex),
            "system_leaf" => Ok(Self::SystemLeaf),
            "user" => Ok(Self::User),
            "overview_module" => Ok(Self::OverviewModule),
            "overview_repo" => Ok(Self::OverviewRepo),
            "update_leaf_system" => Ok(Self::UpdateLeafSystem),
            "update_leaf_user" => Ok(Self::UpdateLeafUser),
            "routing_system" => Ok(Self::RoutingSystem),
            "routing_user" => Ok(Self::RoutingUser),
            "stale_fix_system" => Ok(Self::StaleFixSystem),
            "stale_fix_user" => Ok(Self::StaleFixUser),
            _ => Err(anyhow!("unknown prompt type: {value}")),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cluster => "cluster",
            Self::SuperGroup => "super_group",
            Self::FilterFolders => "filter_folders",
            Self::SystemComplex => "system_complex",
            Self::SystemLeaf => "system_leaf",
            Self::User => "user",
            Self::OverviewModule => "overview_module",
            Self::OverviewRepo => "overview_repo",
            Self::UpdateLeafSystem => "update_leaf_system",
            Self::UpdateLeafUser => "update_leaf_user",
            Self::RoutingSystem => "routing_system",
            Self::RoutingUser => "routing_user",
            Self::StaleFixSystem => "stale_fix_system",
            Self::StaleFixUser => "stale_fix_user",
        }
    }

    pub fn all() -> &'static [Self] {
        &[
            Self::Cluster,
            Self::SuperGroup,
            Self::FilterFolders,
            Self::SystemComplex,
            Self::SystemLeaf,
            Self::User,
            Self::OverviewModule,
            Self::OverviewRepo,
            Self::UpdateLeafSystem,
            Self::UpdateLeafUser,
            Self::RoutingSystem,
            Self::RoutingUser,
            Self::StaleFixSystem,
            Self::StaleFixUser,
        ]
    }
}

pub const MAX_USER_PROMPT_CHARS: usize = 900_000;
const MODULE_TREE_TRIMMED_NOTE: &str =
    "[CodeWiki: module tree component lists were trimmed; module names and hierarchy are complete. Read the referenced sources or pages when component detail is needed.]";

const NO_VARIABLES: &[&str] = &[];
const CLUSTER_REQUIRED: &[&str] = &[];
const CLUSTER_OPTIONAL: &[&str] = &[
    "potential_core_components",
    "component_ids",
    "scope",
    "module_name",
    "module_tree",
    "custom_instructions",
];
const SUPER_GROUP_REQUIRED: &[&str] = &["formatted_modules"];
const FILTER_FOLDERS_REQUIRED: &[&str] = &["project_name", "files"];
const MODULE_REQUIRED: &[&str] = &["module_name", "doc_path"];
const CUSTOM_INSTRUCTIONS_OPTIONAL: &[&str] = &["custom_instructions", "few_shot_examples"];
const USER_REQUIRED: &[&str] = &["module_name", "module_tree"];
const USER_OPTIONAL: &[&str] = &[
    "formatted_core_component_codes",
    "component_ids",
    "artifact_index",
    "few_shot_examples",
    "architecture_context",
];
const OVERVIEW_MODULE_REQUIRED: &[&str] = &["module_name", "repo_structure"];
const OVERVIEW_MODULE_OPTIONAL: &[&str] = &[
    "few_shot_examples",
    "architecture_context",
    "custom_instructions",
];
const OVERVIEW_REPO_REQUIRED: &[&str] = &["repo_name", "repo_structure"];
const OVERVIEW_REPO_OPTIONAL: &[&str] = &[
    "artifact_index",
    "few_shot_examples",
    "architecture_context",
    "custom_instructions",
];
const UPDATE_SYSTEM_REQUIRED: &[&str] = &["leaf_name"];
const UPDATE_USER_REQUIRED: &[&str] = &[
    "leaf_name",
    "mode",
    "mode_note",
    "write_set",
    "report",
    "module_tree",
    "leaf_components",
    "leaf_page",
];
const ROUTING_USER_REQUIRED: &[&str] = &["module_tree", "orphans"];
const STALE_USER_REQUIRED: &[&str] = &["page", "items"];

pub fn variable_contract(kind: PromptType) -> (&'static [&'static str], &'static [&'static str]) {
    match kind {
        PromptType::Cluster => (CLUSTER_REQUIRED, CLUSTER_OPTIONAL),
        PromptType::SuperGroup => (SUPER_GROUP_REQUIRED, NO_VARIABLES),
        PromptType::FilterFolders => (FILTER_FOLDERS_REQUIRED, NO_VARIABLES),
        PromptType::SystemComplex | PromptType::SystemLeaf => {
            (MODULE_REQUIRED, CUSTOM_INSTRUCTIONS_OPTIONAL)
        }
        PromptType::User => (USER_REQUIRED, USER_OPTIONAL),
        PromptType::OverviewModule => (OVERVIEW_MODULE_REQUIRED, OVERVIEW_MODULE_OPTIONAL),
        PromptType::OverviewRepo => (OVERVIEW_REPO_REQUIRED, OVERVIEW_REPO_OPTIONAL),
        PromptType::UpdateLeafSystem => (UPDATE_SYSTEM_REQUIRED, CUSTOM_INSTRUCTIONS_OPTIONAL),
        PromptType::UpdateLeafUser => (UPDATE_USER_REQUIRED, NO_VARIABLES),
        PromptType::RoutingSystem | PromptType::StaleFixSystem => (NO_VARIABLES, NO_VARIABLES),
        PromptType::RoutingUser => (ROUTING_USER_REQUIRED, NO_VARIABLES),
        PromptType::StaleFixUser => (STALE_USER_REQUIRED, NO_VARIABLES),
    }
}

pub fn validate_variables(kind: PromptType, vars: &BTreeMap<String, Value>) -> Result<()> {
    let (required, optional) = variable_contract(kind);
    let mut allowed = required.to_vec();
    allowed.extend(optional);

    for key in vars.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(anyhow!(
                "prompt '{}' does not accept variable '{}'",
                kind.as_str(),
                key
            ));
        }
    }
    for key in required {
        if vars.get(*key).is_none_or(Value::is_null) {
            return Err(anyhow!(
                "prompt '{}' requires variable '{}'",
                kind.as_str(),
                key
            ));
        }
    }
    if kind == PromptType::Cluster
        && vars
            .get("potential_core_components")
            .is_none_or(Value::is_null)
        && vars.get("component_ids").is_none_or(Value::is_null)
    {
        return Err(anyhow!(
            "prompt 'cluster' requires either 'potential_core_components' or 'component_ids'"
        ));
    }
    if let Some(value) = vars.get("component_ids") {
        let Some(values) = value.as_array() else {
            return Err(anyhow!("prompt variable 'component_ids' must be an array"));
        };
        if values.iter().any(|value| !value.is_string()) {
            return Err(anyhow!(
                "prompt variable 'component_ids' must contain strings"
            ));
        }
    }
    if kind == PromptType::User
        && vars
            .get("formatted_core_component_codes")
            .is_none_or(Value::is_null)
        && vars.get("component_ids").is_none_or(Value::is_null)
    {
        return Err(anyhow!(
            "prompt 'user' requires either 'formatted_core_component_codes' or 'component_ids'"
        ));
    }
    if kind == PromptType::Cluster {
        match vars.get("scope") {
            None => {}
            Some(Value::String(scope)) if scope == "repo" || scope == "module" => {
                if scope == "module" {
                    for key in ["module_name", "module_tree"] {
                        if vars.get(key).is_none_or(Value::is_null) {
                            return Err(anyhow!(
                                "prompt 'cluster' with scope=module requires variable '{}'",
                                key
                            ));
                        }
                    }
                }
            }
            Some(_) => {
                return Err(anyhow!(
                    "prompt 'cluster' variable 'scope' must be 'repo' or 'module'"
                ));
            }
        }
    }
    Ok(())
}

pub fn catalog_specs() -> Vec<Value> {
    PromptType::all()
        .iter()
        .map(|kind| {
            let (required, optional) = variable_contract(*kind);
            let conditional_required = if *kind == PromptType::Cluster {
                json!({
                    "scope=module": ["module_name", "module_tree"],
                    "one_of": ["potential_core_components", "component_ids"]
                })
            } else if *kind == PromptType::User {
                json!({"one_of": ["formatted_core_component_codes", "component_ids"]})
            } else {
                json!({})
            };
            json!({
                "type": kind.as_str(),
                "required_variables": required,
                "optional_variables": optional,
                "conditional_required": conditional_required,
            })
        })
        .collect()
}

pub fn raw_prompt(kind: PromptType, vars: &BTreeMap<String, Value>) -> &'static str {
    match kind {
        PromptType::Cluster => {
            if vars
                .get("scope")
                .and_then(Value::as_str)
                .is_some_and(|scope| scope == "module")
            {
                include_str!("../prompts/cluster_module.txt")
            } else {
                include_str!("../prompts/cluster_repo.txt")
            }
        }
        PromptType::SuperGroup => include_str!("../prompts/super_group.txt"),
        PromptType::FilterFolders => include_str!("../prompts/filter_folders.txt"),
        PromptType::SystemComplex => include_str!("../prompts/system_complex.txt"),
        PromptType::SystemLeaf => include_str!("../prompts/system_leaf.txt"),
        PromptType::User => include_str!("../prompts/user.txt"),
        PromptType::OverviewModule => include_str!("../prompts/overview_module.txt"),
        PromptType::OverviewRepo => include_str!("../prompts/overview_repo.txt"),
        PromptType::UpdateLeafSystem => include_str!("../prompts/update_leaf_system.txt"),
        PromptType::UpdateLeafUser => include_str!("../prompts/update_leaf_user.txt"),
        PromptType::RoutingSystem => include_str!("../prompts/routing_system.txt"),
        PromptType::RoutingUser => include_str!("../prompts/routing_user.txt"),
        PromptType::StaleFixSystem => include_str!("../prompts/stale_fix_system.txt"),
        PromptType::StaleFixUser => include_str!("../prompts/stale_fix_user.txt"),
    }
}

pub fn render(kind: PromptType, vars: &BTreeMap<String, Value>) -> Result<String> {
    validate_variables(kind, vars)?;
    let (_, optional) = variable_contract(kind);
    let mut values = vars.clone();
    if let Some(ids) = string_list(vars.get("component_ids")) {
        if kind == PromptType::Cluster && !values.contains_key("potential_core_components") {
            values.insert(
                "potential_core_components".to_string(),
                Value::String(format_component_listing(&ids, &BTreeMap::new())),
            );
        }
        if kind == PromptType::User && !values.contains_key("formatted_core_component_codes") {
            values.insert(
                "formatted_core_component_codes".to_string(),
                Value::String(
                    ids.iter()
                        .map(|id| format!("- {id}"))
                        .collect::<Vec<_>>()
                        .join("\n"),
                ),
            );
        }
    }
    if matches!(kind, PromptType::Cluster | PromptType::User)
        && values.get("module_tree").is_some_and(Value::is_object)
    {
        let tree = values
            .get("module_tree")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        let current = values.get("module_name").and_then(Value::as_str);
        values.insert(
            "module_tree".to_string(),
            Value::String(format_module_tree_outline(
                &Value::Object(tree),
                current,
                true,
            )),
        );
    }
    for key in optional {
        values
            .entry((*key).to_string())
            .or_insert_with(|| Value::String(String::new()));
    }

    let mut rendered = raw_prompt(kind, &values).to_string();
    for (key, value) in &values {
        let replacement = match value {
            Value::String(value) => value.clone(),
            Value::Null => String::new(),
            _ => serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string()),
        };
        rendered = rendered.replace(&format!("{{{key}}}"), &replacement);
    }
    if matches!(kind, PromptType::User | PromptType::OverviewRepo) {
        if let Some(artifact_index) = values.get("artifact_index") {
            let artifact_index = value_to_prompt_text(artifact_index);
            if !artifact_index.trim().is_empty() {
                let addendum = if kind == PromptType::OverviewRepo {
                    include_str!("../prompts/overview_artifact_addendum.txt")
                        .replace("{artifact_index}", &artifact_index)
                } else {
                    format!(
                        "<ARTIFACT_INDEX>\n{artifact_index}\n</ARTIFACT_INDEX>\n\n{}",
                        include_str!("../prompts/artifact_usage.txt")
                    )
                };
                rendered.push_str("\n\n");
                rendered.push_str(&addendum);
            }
        }
    }
    Ok(rendered)
}

/// Render a compact hierarchical tree outline for the architecture writer.
/// JSON is useful for storage but is needlessly expensive and hard for a model
/// to scan when the tree is large.
pub fn format_module_tree_outline(
    tree: &Value,
    current_module: Option<&str>,
    include_components: bool,
) -> String {
    fn walk(
        tree: &serde_json::Map<String, Value>,
        indent: usize,
        current_module: Option<&str>,
        include_components: bool,
        lines: &mut Vec<String>,
    ) {
        for (name, value) in tree {
            let marker = if current_module == Some(name.as_str()) {
                " (current module)"
            } else {
                ""
            };
            lines.push(format!("{}{}{}", "  ".repeat(indent), name, marker));
            let Some(info) = value.as_object() else {
                continue;
            };
            if include_components {
                let mut by_file = BTreeMap::<String, Vec<String>>::new();
                if let Some(components) = info.get("components").and_then(Value::as_array) {
                    for component in components.iter().filter_map(Value::as_str) {
                        let (file, name) = component
                            .split_once("::")
                            .map(|(file, name)| (file.to_string(), name.to_string()))
                            .unwrap_or_else(|| (String::new(), component.to_string()));
                        by_file.entry(file).or_default().push(name);
                    }
                }
                for (file, names) in by_file {
                    let label = if file.is_empty() {
                        names.join(", ")
                    } else {
                        format!("{file}: {}", names.join(", "))
                    };
                    lines.push(format!("{}{}", "  ".repeat(indent + 1), label));
                }
            }
            if let Some(children) = info.get("children").and_then(Value::as_object) {
                if !children.is_empty() {
                    lines.push(format!("{}Children:", "  ".repeat(indent + 1)));
                    walk(
                        children,
                        indent + 2,
                        current_module,
                        include_components,
                        lines,
                    );
                }
            }
        }
    }

    let mut lines = Vec::new();
    if let Some(object) = tree.as_object() {
        walk(object, 0, current_module, include_components, &mut lines);
    }
    lines.join("\n")
}

/// Format selected component IDs for a clustering prompt when the host passes
/// IDs instead of pre-rendering a large string.  The optional node map is used
/// by the CLI; an empty map remains useful for contract-only callers.
pub fn format_component_listing(ids: &[String], nodes: &BTreeMap<String, Node>) -> String {
    let mut groups = BTreeMap::<String, Vec<String>>::new();
    for id in ids {
        let path = nodes
            .get(id)
            .map(|node| node.relative_path.clone())
            .unwrap_or_else(|| {
                id.split_once("::")
                    .map(|(path, _)| path.to_string())
                    .unwrap_or_default()
            });
        groups.entry(path).or_default().push(id.clone());
    }
    let mut output = String::new();
    for (path, mut group) in groups {
        group.sort();
        let artifact = !group.is_empty()
            && group.iter().all(|id| {
                nodes
                    .get(id)
                    .is_some_and(|node| node.component_type == "artifact")
            });
        if artifact {
            let class = group
                .iter()
                .find_map(|id| nodes.get(id).and_then(|node| node.artifact_class.clone()))
                .unwrap_or_else(|| "config".to_string());
            output.push_str(&format!("# {path} (artifact: {class})\n"));
        } else {
            output.push_str(&format!("# {path}\n"));
        }
        for id in group {
            output.push_str(&format!("\t{id}\n"));
        }
    }
    output
}

/// Render source grouped by file so the architecture writer can compare
/// related anchors without losing their exact IDs.
pub fn format_component_codes(ids: &[String], nodes: &BTreeMap<String, Node>) -> String {
    let mut groups = BTreeMap::<String, Vec<&Node>>::new();
    for id in ids {
        if let Some(node) = nodes.get(id) {
            groups
                .entry(node.relative_path.clone())
                .or_default()
                .push(node);
        }
    }
    let mut output = String::new();
    for (path, group) in groups {
        output.push_str(&format!(
            "# File: {path}\n\n## Core Components in this file:\n"
        ));
        for node in &group {
            output.push_str(&format!("- {}\n", node.id));
        }
        output.push_str("\n## File Content:\n```");
        output.push_str(fence_language(&path));
        output.push('\n');
        let artifact_group = group.iter().all(|node| node.component_type == "artifact");
        let source = if artifact_group {
            group
                .first()
                .map(|node| node.source_code.clone())
                .unwrap_or_default()
        } else {
            group
                .iter()
                .find_map(|node| fs::read_to_string(Path::new(&node.file_path)).ok())
                .or_else(|| group.first().map(|node| node.source_code.clone()))
                .unwrap_or_default()
        };
        output.push_str(&source);
        if !source.ends_with('\n') {
            output.push('\n');
        }
        output.push_str("```\n\n");
    }
    output
}

fn string_list(value: Option<&Value>) -> Option<Vec<String>> {
    value?.as_array().map(|values| {
        values
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect()
    })
}

fn value_to_prompt_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        _ => serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()),
    }
}

fn fence_language(path: &str) -> &'static str {
    match path.rsplit('/').next().unwrap_or(path) {
        "Dockerfile" | "Containerfile" => "dockerfile",
        "Makefile" | "GNUmakefile" => "makefile",
        name if name.ends_with(".rs") => "rust",
        name if name.ends_with(".py") => "python",
        name if name.ends_with(".js") || name.ends_with(".jsx") => "javascript",
        name if name.ends_with(".ts") || name.ends_with(".tsx") => "typescript",
        name if name.ends_with(".json") => "json",
        name if name.ends_with(".yaml") || name.ends_with(".yml") => "yaml",
        name if name.ends_with(".toml") => "toml",
        name if name.ends_with(".go") => "go",
        name if name.ends_with(".java") => "java",
        name if name.ends_with(".cpp") || name.ends_with(".cc") => "cpp",
        name if name.ends_with(".c") || name.ends_with(".h") => "c",
        _ => "text",
    }
}

pub fn catalog() -> Vec<&'static str> {
    PromptType::all().iter().map(|kind| kind.as_str()).collect()
}

pub fn mode_note(mode: &str) -> &'static str {
    match mode {
        "edit" => "The leaf page exists. Decide per page: patch in place, no-op, or (leaf page only) 'rewrite' if a fresh page would be better than patching.",
        "create" => "The leaf page was just generated from scratch and must NOT be changed here. Your job is the related pages: make ancestors list and summarize the new module, and fix any referrer that should now point at it.",
        "delete" => "This module no longer exists and its page has been removed. Update the related pages: drop it from ancestor summaries and child lists, and remove or redirect every link or mention of it on the referrer pages.",
        "related_only" => "The leaf page was regenerated from scratch by the normal module agent and must NOT be changed here. Update the related pages so they match the regenerated leaf page.",
        _ => "",
    }
}

pub fn user_prompt_with_limits(kind: PromptType, vars: &BTreeMap<String, Value>) -> Result<String> {
    let rendered = render(kind, vars)?;
    if rendered.chars().count() <= MAX_USER_PROMPT_CHARS {
        return Ok(rendered);
    }
    if matches!(kind, PromptType::Cluster | PromptType::User)
        && vars.get("module_tree").is_some_and(Value::is_object)
    {
        let mut slim = vars.clone();
        let tree = slim
            .get("module_tree")
            .cloned()
            .unwrap_or_else(|| json!({}));
        slim.insert(
            "module_tree".to_string(),
            Value::String(format!(
                "{MODULE_TREE_TRIMMED_NOTE}\n\n{}",
                format_module_tree_outline(
                    &tree,
                    slim.get("module_name").and_then(Value::as_str),
                    false
                )
            )),
        );
        let slim_rendered = render(kind, &slim)?;
        if slim_rendered.chars().count() <= MAX_USER_PROMPT_CHARS {
            return Ok(slim_rendered);
        }
        return Ok(truncate_to_char_limit(
            &slim_rendered,
            MAX_USER_PROMPT_CHARS,
        ));
    }
    Ok(truncate_to_char_limit(&rendered, MAX_USER_PROMPT_CHARS))
}

fn truncate_to_char_limit(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let note = "\n\n[CodeWiki: prompt content truncated at the configured limit.]\n";
    if limit <= note.chars().count() {
        return text.chars().take(limit).collect();
    }
    let body_limit = limit - note.chars().count();
    let mut shortened = text.chars().take(body_limit).collect::<String>();
    shortened.push_str(note);
    shortened
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_contains_all_runtime_prompt_names() {
        let values = catalog();
        for name in [
            "cluster",
            "super_group",
            "filter_folders",
            "system_complex",
            "system_leaf",
            "user",
            "overview_module",
            "overview_repo",
        ] {
            assert!(values.contains(&name));
        }
    }

    #[test]
    fn renders_named_variables_without_changing_other_braces() {
        let mut vars = BTreeMap::new();
        vars.insert(
            "module_name".to_string(),
            Value::String("sample".to_string()),
        );
        vars.insert(
            "doc_path".to_string(),
            Value::String("sample.md".to_string()),
        );
        let prompt = render(PromptType::SystemComplex, &vars).expect("valid system prompt");
        assert!(prompt.contains("sample.md"));
    }

    #[test]
    fn validates_prompt_variables_and_catalog_specs() {
        let mut vars = BTreeMap::new();
        vars.insert(
            "module_name".to_string(),
            Value::String("sample".to_string()),
        );
        vars.insert(
            "doc_path".to_string(),
            Value::String("sample.md".to_string()),
        );
        assert!(render(PromptType::User, &vars).is_err());
        vars.remove("doc_path");

        vars.insert(
            "module_tree".to_string(),
            json!({"sample": {"components": [], "children": {}}}),
        );
        vars.insert(
            "formatted_core_component_codes".to_string(),
            Value::String("source".to_string()),
        );
        let prompt = render(PromptType::User, &vars).expect("valid user prompt");
        assert!(!prompt.contains("{formatted_core_component_codes}"));
        assert!(catalog_specs()
            .iter()
            .any(|spec| spec["type"] == "update_leaf_user"));

        let mut super_group = BTreeMap::new();
        super_group.insert(
            "formatted_modules".to_string(),
            Value::String("module list".to_string()),
        );
        assert!(render(PromptType::SuperGroup, &super_group).is_ok());
    }

    #[test]
    fn optional_variables_are_rendered_as_empty() {
        let mut vars = BTreeMap::new();
        vars.insert(
            "module_name".to_string(),
            Value::String("sample".to_string()),
        );
        vars.insert(
            "doc_path".to_string(),
            Value::String("sample.md".to_string()),
        );
        let prompt = render(PromptType::SystemComplex, &vars).expect("valid system prompt");
        assert!(!prompt.contains("{custom_instructions}"));
    }

    #[test]
    fn truncation_respects_unicode_boundaries() {
        assert_eq!(truncate_to_char_limit("a🙂中b", 2), "a🙂");
    }

    #[test]
    fn component_id_contract_renders_architecture_outline() {
        let mut vars = BTreeMap::new();
        vars.insert("module_name".to_string(), Value::String("Core".to_string()));
        vars.insert(
            "module_tree".to_string(),
            json!({
                "Core": {
                    "components": ["src/lib.rs::run"],
                    "children": {"Leaf": {"components": [], "children": {}}}
                }
            }),
        );
        vars.insert("component_ids".to_string(), json!(["src/lib.rs::run"]));
        let prompt = render(PromptType::User, &vars).expect("component id prompt");
        assert!(prompt.contains("Core (current module)"));
        assert!(prompt.contains("src/lib.rs: run"));
        assert!(!prompt.contains("\"children\": {}"));
    }

    #[test]
    fn overview_repo_accepts_artifact_context() {
        let mut vars = BTreeMap::new();
        vars.insert("repo_name".to_string(), Value::String("repo".to_string()));
        vars.insert(
            "repo_structure".to_string(),
            Value::String("Core".to_string()),
        );
        vars.insert(
            "artifact_index".to_string(),
            Value::String("Dockerfile (container)".to_string()),
        );
        let prompt = render(PromptType::OverviewRepo, &vars).expect("artifact overview prompt");
        assert!(prompt.contains("How it is built and run"));
        assert!(prompt.contains("Dockerfile (container)"));
    }
}
