use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

fn run<I, S>(args: I) -> Value
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = Command::new(env!("CARGO_BIN_EXE_codewiki"))
        .args(args)
        .output()
        .expect("run codewiki");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("JSON stdout")
}

fn run_from<I, S>(directory: &Path, args: I) -> Value
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = Command::new(env!("CARGO_BIN_EXE_codewiki"))
        .current_dir(directory)
        .args(args)
        .output()
        .expect("run codewiki");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("JSON stdout")
}

fn run_failure<I, S>(args: I) -> (bool, Value)
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let output = Command::new(env!("CARGO_BIN_EXE_codewiki"))
        .args(args)
        .output()
        .expect("run codewiki");
    let value = serde_json::from_slice(&output.stdout).expect("JSON error stdout");
    (output.status.success(), value)
}

#[test]
fn default_output_dir_is_repowiki_and_document_prefixes_are_normalized() {
    let repo = tempdir().expect("repo tempdir");
    let repo_arg = repo.path().to_string_lossy().to_string();
    fs::write(repo.path().join("app.py"), "def run():\n    return 1\n").expect("write fixture");

    let analysis = run(["generate", "--repo", repo_arg.as_str()]);
    let session = analysis["session_id"].as_str().expect("session id");
    let output = repo.path().join(".repowiki");
    assert_eq!(
        analysis["summary"]["output_dir"].as_str().unwrap(),
        output.to_string_lossy().as_ref()
    );
    assert!(output.is_dir());
    assert!(!repo.path().join("docs").exists());

    run([
        "doc",
        "write",
        "--repo-root",
        repo_arg.as_str(),
        "--session",
        session,
        "--path",
        ".repowiki/guide.md",
        "--content",
        "# Guide\n",
    ]);
    run([
        "doc",
        "write",
        "--repo-root",
        repo_arg.as_str(),
        "--session",
        session,
        "--path",
        "docs/legacy.md",
        "--content",
        "# Legacy\n",
    ]);

    assert!(output.join("guide.md").is_file());
    assert!(output.join("legacy.md").is_file());
    assert!(!output.join(".repowiki").exists());
}

#[test]
fn file_side_workflow_creates_reference_artifacts() {
    let repo = tempdir().expect("repo tempdir");
    let output = tempdir().expect("output tempdir");
    let repo_arg = repo.path().to_string_lossy().to_string();
    let output_arg = output.path().to_string_lossy().to_string();
    fs::write(
        repo.path().join("app.py"),
        "class Service:\n    def run(self):\n        return helper()\n\ndef helper():\n    return 1\n",
    )
    .expect("write fixture");
    fs::write(repo.path().join(".gitignore"), "ignored.py\n").expect("write gitignore");
    fs::write(
        repo.path().join("ignored.py"),
        "def ignored():\n    return 0\n",
    )
    .expect("write ignored fixture");

    let analysis = run([
        "generate",
        "--repo",
        repo_arg.as_str(),
        "--output",
        output_arg.as_str(),
    ]);
    let session = analysis["session_id"].as_str().expect("session id");
    assert_eq!(analysis["summary"]["total_components"], 3);
    assert!(Path::new(analysis["component_index_path"].as_str().unwrap()).exists());
    assert!(Path::new(analysis["candidate_module_tree_path"].as_str().unwrap()).exists());

    let vars = repo.path().join("vars.json");
    let vars_arg = vars.to_string_lossy().to_string();
    fs::write(&vars, r#"{"module_name":"Service"}"#).expect("write vars");
    let prompt = run([
        "prompt",
        "get",
        "--repo-root",
        repo_arg.as_str(),
        "--session",
        session,
        "--type",
        "system_leaf",
        "--vars-file",
        vars_arg.as_str(),
    ]);
    assert!(Path::new(prompt["path"].as_str().unwrap()).exists());

    let tree = repo.path().join("tree.json");
    let tree_arg = tree.to_string_lossy().to_string();
    fs::write(
        &tree,
        r#"{"Service":{"path":".","components":["app.py::Service","app.py::run","app.py::helper"],"children":{}}}"#,
    )
    .expect("write tree");
    let saved = run([
        "tree",
        "save",
        "--repo-root",
        repo_arg.as_str(),
        "--session",
        session,
        "--tree-file",
        tree_arg.as_str(),
        "--first",
    ]);
    assert!(saved["result"]["unmatched_component_ids"]
        .as_array()
        .unwrap()
        .is_empty());

    let written = run([
        "doc",
        "write",
        "--repo-root",
        repo_arg.as_str(),
        "--session",
        session,
        "--path",
        "Service.md",
        "--content",
        "# Service\n\n## Purpose\n\nThe Service class in app.py is the small application entry point. It coordinates the public run operation with the shared helper function, so callers can request work without knowing how the result is assembled. This page explains the behavior represented by those analyzed components.\n\n## Architecture\n\nA caller enters through Service.run, which delegates the reusable calculation to helper and returns the value to the caller. The implementation is intentionally synchronous and keeps the orchestration in one module.\n\n## Responsibilities\n\nThe module owns the entry-point contract, the delegation relationship, and the source-level behavior documented from app.py.\n",
    ]);
    assert!(written["result"]["path"]
        .as_str()
        .unwrap()
        .ends_with("Service.md"));
    assert!(output.path().join("module_tree.json").exists());
    assert!(output.path().join("first_module_tree.json").exists());

    run([
        "doc",
        "write",
        "--repo-root",
        repo_arg.as_str(),
        "--session",
        session,
        "--path",
        "overview.md",
        "--content",
        "# Overview\n\n## Purpose\n\nThis repository contains a compact service example whose analyzed Python components demonstrate an entry point, a delegation helper, and the documentation workflow around them. The wiki explains how those pieces fit together and where to start reading the source.\n\n## Architecture\n\n\x60\x60\x60mermaid\nflowchart LR\n  caller --> Service --> helper\n\x60\x60\x60\n\nThe [Service module](Service.md) describes the source-level implementation and its responsibilities. Use that page for the detailed component behavior.\n",
    ]);

    let report = run([
        "doc",
        "validate",
        "--repo-root",
        repo_arg.as_str(),
        "--session",
        session,
    ]);
    assert_eq!(report["result"]["valid"], true, "{report}");

    let closed = run([
        "session",
        "close",
        "--repo-root",
        repo_arg.as_str(),
        "--session",
        session,
    ]);
    assert_eq!(closed["cleaned"], true);
    assert!(output.path().join("metadata.json").exists());
}

#[test]
fn recursive_tree_commands_are_file_side_and_work_outside_repo_cwd() {
    let repo = tempdir().expect("repo tempdir");
    let output = tempdir().expect("output tempdir");
    let scratch = tempdir().expect("scratch directory");
    fs::create_dir_all(repo.path().join("src")).expect("src directory");
    fs::write(repo.path().join("src/api.rs"), "pub struct Api;\n").expect("api fixture");
    fs::write(repo.path().join("src/runtime.rs"), "pub struct Runtime;\n")
        .expect("runtime fixture");
    let repo_arg = repo.path().to_string_lossy().to_string();
    let output_arg = output.path().to_string_lossy().to_string();
    let analysis = run([
        "generate",
        "--repo",
        repo_arg.as_str(),
        "--output",
        output_arg.as_str(),
    ]);
    let session = analysis["session_id"].as_str().expect("session id");
    let component_index: Value = serde_json::from_str(
        &fs::read_to_string(analysis["component_index_path"].as_str().unwrap())
            .expect("component index"),
    )
    .expect("component index JSON");
    let ids = component_index
        .as_array()
        .expect("component list")
        .iter()
        .filter_map(|item| item["id"].as_str().map(str::to_string))
        .collect::<Vec<_>>();
    assert!(
        ids.len() >= 2,
        "recursive fixture did not produce two components"
    );

    let work = repo.path().join("test-input");
    fs::create_dir_all(&work).expect("test input directory");
    let input_ids = work.join("input-ids.json");
    fs::write(
        &input_ids,
        serde_json::to_string(&ids).expect("serialize IDs"),
    )
    .expect("write input IDs");
    let response = work.join("cluster-response.txt");
    let grouping = json!({
        "API": {"path": "src", "components": [ids[0].clone()]},
        "Runtime": {"path": "src", "components": ids[1..].to_vec()}
    });
    fs::write(
        &response,
        format!("<GROUPED_COMPONENTS>{}</GROUPED_COMPONENTS>", grouping),
    )
    .expect("write cluster response");
    let root_tree = work.join("root-tree.json");
    fs::write(&root_tree, "{}").expect("write empty tree");
    let clustered_tree = work.join("clustered-tree.json");
    let clustered = run_from(
        scratch.path(),
        [
            "tree",
            "apply-cluster",
            "--repo-root",
            repo_arg.as_str(),
            "--session",
            session,
            "--tree-file",
            root_tree.to_str().unwrap(),
            "--response-file",
            response.to_str().unwrap(),
            "--input-ids-file",
            input_ids.to_str().unwrap(),
            "--scope",
            "repo",
            "--output-tree-file",
            clustered_tree.to_str().unwrap(),
        ],
    );
    assert_eq!(clustered["diagnostics"]["fallback_used"], false);

    let super_response = work.join("super-response.txt");
    fs::write(
        &super_response,
        r#"<GROUPED_MODULES>{"Platform":{"modules":["API","Runtime"]}}</GROUPED_MODULES>"#,
    )
    .expect("write super group response");
    let final_tree = work.join("final-tree.json");
    let super_grouped = run_from(
        scratch.path(),
        [
            "tree",
            "apply-super-group",
            "--repo-root",
            repo_arg.as_str(),
            "--session",
            session,
            "--tree-file",
            clustered_tree.to_str().unwrap(),
            "--response-file",
            super_response.to_str().unwrap(),
            "--output-tree-file",
            final_tree.to_str().unwrap(),
        ],
    );
    assert_eq!(super_grouped["diagnostics"]["changed"], true);

    let saved = run_from(
        scratch.path(),
        [
            "tree",
            "save",
            "--repo-root",
            repo_arg.as_str(),
            "--session",
            session,
            "--tree-file",
            final_tree.to_str().unwrap(),
            "--first",
        ],
    );
    assert_eq!(saved["result"]["unmatched_component_ids"], json!([]));
    assert_eq!(saved["result"]["quality_valid"], true);
    assert_eq!(saved["result"]["max_depth"], 2);

    let context_file = work.join("overview-context.json");
    let context = run_from(
        scratch.path(),
        [
            "tree",
            "overview-context",
            "--repo-root",
            repo_arg.as_str(),
            "--session",
            session,
            "--tree-file",
            final_tree.to_str().unwrap(),
            "--output-file",
            context_file.to_str().unwrap(),
        ],
    );
    assert!(context["context_path"].as_str().is_some());
    let context_json: Value =
        serde_json::from_str(&fs::read_to_string(&context_file).expect("overview context file"))
            .expect("overview context JSON");
    assert_eq!(context_json["Platform"]["components"], Value::Null);
    assert_eq!(context_json["Platform"]["docs_path"], Value::Null);

    let order = run_from(
        scratch.path(),
        [
            "tree",
            "order",
            "--repo-root",
            repo_arg.as_str(),
            "--session",
            session,
        ],
    );
    let items = order["processing_order"]
        .as_array()
        .expect("processing order");
    assert_eq!(items.last().unwrap()["module"], json!("Platform"));
    assert!(
        items
            .iter()
            .position(|item| item["module"] == "API")
            .unwrap()
            < items
                .iter()
                .position(|item| item["module"] == "Platform")
                .unwrap()
    );

    for page in ["API.md", "Runtime.md", "Platform.md", "overview.md"] {
        let content = match page {
            "API.md" => "# API\n\n## Purpose\n\nThe API module owns the public Rust entry point in src/api.rs and presents a stable request surface to callers. Its implementation keeps request decoding and dispatch close together so the runtime can depend on a small, understandable interface.\n\n## Architecture\n\nThe src/api.rs entry point validates a request and hands it to the runtime contract described by this module. The page is grounded in the analyzed Api component.\n\n## Responsibilities\n\nIt exposes the API contract and coordinates the first step of request handling.\n",
            "Runtime.md" => "# Runtime\n\n## Purpose\n\nThe Runtime module implements the execution path in src/runtime.rs. It receives work from the API layer, performs the runtime operation, and returns a result while keeping execution details hidden behind a narrow module interface.\n\n## Architecture\n\nThe src/runtime.rs implementation is the downstream execution node for the request flow. The page is grounded in the analyzed Runtime component and explains how the module participates in the parent subsystem.\n\n## Responsibilities\n\nIt owns execution behavior, runtime state transitions, and the result returned to the API layer.\n",
            "Platform.md" => "# Platform\n\n## Purpose\n\nPlatform is the parent subsystem that connects the API and Runtime modules into one request path. It provides the architectural context for the child pages while keeping their detailed implementation explanations in separate documents.\n\n## Architecture\n\n\x60\x60\x60mermaid\nflowchart LR\n  API --> Runtime\n\x60\x60\x60\n\nThe parent page summarizes the relationship between the [API module](API.md) and the [Runtime module](Runtime.md). Read those child pages for source-level details.\n\n## Responsibilities\n\nPlatform defines the subsystem shape, child ownership, and the integration path between the public interface and execution implementation.\n",
            _ => "# Overview\n\n## Purpose\n\nThis repository demonstrates a two-stage Rust request path. The API module accepts work and the Runtime module executes it; the wiki organizes both implementations under the Platform subsystem so a developer can move from the end-to-end shape to the source details.\n\n## Architecture\n\n\x60\x60\x60mermaid\nflowchart LR\n  caller --> Platform --> API --> Runtime\n\x60\x60\x60\n\nThe [Platform module](Platform.md) is the starting point for the subsystem. Its child pages explain the [API](API.md) and [Runtime](Runtime.md) responsibilities and source locations.\n\n## Responsibilities\n\nThe repository-level overview identifies the execution path, navigation order, and module relationships without duplicating the child documentation.\n",
        };
        run_from(
            scratch.path(),
            [
                "doc",
                "write",
                "--repo-root",
                repo_arg.as_str(),
                "--session",
                session,
                "--path",
                page,
                "--content",
                content,
            ],
        );
    }
    let closed = run_from(
        scratch.path(),
        [
            "session",
            "close",
            "--repo-root",
            repo_arg.as_str(),
            "--session",
            session,
        ],
    );
    assert_eq!(closed["cleaned"], true);
    assert!(output.path().join("metadata.json").is_file());
}

#[test]
fn cli_failures_are_json_and_nonzero() {
    let (success, error) = run_failure([
        "prompt",
        "get",
        "--session",
        "missing-session",
        "--type",
        "unknown-prompt",
    ]);
    assert!(!success);
    assert_eq!(error["ok"], false);
    assert!(error["error"].as_str().is_some());
    assert!(error["chain"].as_array().is_some());
}
