use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;

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

const NO_VARIABLES: &[&str] = &[];
const CLUSTER_REQUIRED: &[&str] = &["potential_core_components"];
const CLUSTER_OPTIONAL: &[&str] = &["scope", "module_name", "module_tree"];
const SUPER_GROUP_REQUIRED: &[&str] = &["formatted_modules"];
const FILTER_FOLDERS_REQUIRED: &[&str] = &["project_name", "files"];
const MODULE_REQUIRED: &[&str] = &["module_name"];
const CUSTOM_INSTRUCTIONS_OPTIONAL: &[&str] = &["custom_instructions"];
const USER_REQUIRED: &[&str] = &[
    "module_name",
    "module_tree",
    "formatted_core_component_codes",
];
const OVERVIEW_MODULE_REQUIRED: &[&str] = &["module_name", "repo_structure"];
const OVERVIEW_REPO_REQUIRED: &[&str] = &["repo_name", "repo_structure"];
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
        PromptType::User => (USER_REQUIRED, NO_VARIABLES),
        PromptType::OverviewModule => (OVERVIEW_MODULE_REQUIRED, NO_VARIABLES),
        PromptType::OverviewRepo => (OVERVIEW_REPO_REQUIRED, NO_VARIABLES),
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
                json!({"scope=module": ["module_name", "module_tree"]})
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
    for key in optional {
        values
            .entry((*key).to_string())
            .or_insert_with(|| Value::String(String::new()));
    }

    let mut rendered = raw_prompt(kind, vars).to_string();
    for (key, value) in values {
        let replacement = match value {
            Value::String(value) => value.clone(),
            Value::Null => String::new(),
            _ => serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string()),
        };
        rendered = rendered.replace(&format!("{{{key}}}"), &replacement);
    }
    Ok(rendered)
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
    Ok(truncate_to_char_limit(&rendered, MAX_USER_PROMPT_CHARS))
}

fn truncate_to_char_limit(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let note = "\n\n[CodeWiki: prompt content truncated at the reference limit.]\n";
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
    fn catalog_keeps_reference_prompt_names() {
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
        assert!(render(PromptType::User, &vars).is_err());

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
        let prompt = render(PromptType::SystemComplex, &vars).expect("valid system prompt");
        assert!(!prompt.contains("{custom_instructions}"));
    }

    #[test]
    fn truncation_respects_unicode_boundaries() {
        assert_eq!(truncate_to_char_limit("a🙂中b", 2), "a🙂");
    }
}
