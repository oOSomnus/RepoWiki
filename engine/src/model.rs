use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SUPPORTED_LANGUAGES: &[&str] = &[
    "Python",
    "Java",
    "JavaScript",
    "TypeScript",
    "Go",
    "Rust",
    "C",
    "C++",
    "C#",
    "Kotlin",
    "PHP",
    "Ruby",
    "Scala",
];

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Node {
    pub id: String,
    pub name: String,
    pub component_type: String,
    pub file_path: String,
    pub relative_path: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub source_code: String,
    pub start_line: usize,
    pub end_line: usize,
    #[serde(default)]
    pub has_docstring: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docstring: Option<String>,
    #[serde(default)]
    pub parameters: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_type: Option<String>,
    #[serde(default)]
    pub base_classes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component_id: Option<String>,
    pub language: String,
    pub qualified_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_class: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq, Ord, PartialOrd)]
pub struct CallRelationship {
    pub caller: String,
    pub callee: String,
    pub call_line: usize,
    pub is_resolved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ComponentIndexEntry {
    pub id: String,
    pub name: String,
    pub file_path: String,
    pub relative_path: String,
    pub language: String,
    pub component_type: String,
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ArtifactFile {
    pub path: String,
    pub class: String,
    pub size_bytes: u64,
    pub included: bool,
    #[serde(default)]
    pub units: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ArtifactIndex {
    pub files: BTreeMap<String, ArtifactFile>,
    pub classes: BTreeMap<String, Vec<String>>,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Summary {
    pub repo_path: String,
    pub output_dir: String,
    pub total_components: usize,
    pub leaf_nodes: usize,
    pub max_depth: usize,
    pub supported_files: usize,
    pub languages: Vec<String>,
    pub analyzed_commit: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Module {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default)]
    pub components: Vec<String>,
    #[serde(default)]
    pub children: BTreeMap<String, Module>,
}

pub type ModuleTree = BTreeMap<String, Module>;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProcessingItem {
    pub module_name: String,
    pub doc_path: String,
    pub is_leaf: bool,
    pub components: Vec<String>,
    pub children: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChangeSet {
    pub added: Vec<String>,
    pub deleted: Vec<String>,
    pub modified_interface: Vec<String>,
    pub modified_body: Vec<String>,
    pub edge_changes: Vec<String>,
    pub renamed: Vec<(String, String)>,
    pub structural_ratio: f64,
    pub active_leaf_ratio: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateOptions {
    pub rung: String,
    pub tau_ren: f64,
    pub max_diff_tokens: usize,
    pub tau_nb: f64,
    pub tau_grow: f64,
    pub k_hop: usize,
    pub tau_full: f64,
    pub tau_tree: f64,
}

impl Default for UpdateOptions {
    fn default() -> Self {
        Self {
            rung: "3".to_string(),
            tau_ren: 0.95,
            max_diff_tokens: 8000,
            tau_nb: 0.5,
            tau_grow: 0.33,
            k_hop: 1,
            tau_full: 0.5,
            tau_tree: 0.3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateRecord {
    pub started_at: String,
    pub finished_at: String,
    pub outcome: String,
    pub options: UpdateOptions,
    pub revision: Option<String>,
    pub diff: ChangeSet,
    pub repair: serde_json::Value,
    pub reclustered: bool,
    pub reports: Vec<String>,
    pub active: Vec<String>,
    pub write_sets: BTreeMap<String, Vec<String>>,
    pub fallback: Option<String>,
    pub verdicts: BTreeMap<String, String>,
    pub pages_written: Vec<String>,
    pub pages_removed: Vec<String>,
    pub violations: Vec<String>,
    pub stale_scan: serde_json::Value,
    pub calls: usize,
    pub notes: Vec<String>,
    pub errors: Vec<String>,
    pub wall_seconds: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Metadata {
    pub generation_info: GenerationInfo,
    pub statistics: Statistics,
    pub files_generated: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_update: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GenerationInfo {
    pub timestamp: String,
    pub main_model: String,
    pub generator_version: String,
    pub repo_path: String,
    pub commit_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Statistics {
    pub total_components: usize,
    pub leaf_nodes: usize,
    pub max_depth: usize,
}
