use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
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
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
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
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
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

fn spawn<I, S>(args: I) -> std::process::Child
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    Command::new(env!("CARGO_BIN_EXE_codewiki"))
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn codewiki")
}

#[test]
fn default_output_dir_is_repowiki_and_document_paths_are_strict() {
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

    for path in [".repowiki/guide.md", "docs/legacy.md"] {
        let (success, _) = run_failure([
            "doc",
            "write",
            "--repo-root",
            repo_arg.as_str(),
            "--session",
            session,
            "--path",
            path,
            "--content",
            "# Guide\n",
        ]);
        assert!(!success, "legacy document path should fail: {path}");
    }
    run([
        "doc",
        "write",
        "--repo-root",
        repo_arg.as_str(),
        "--session",
        session,
        "--path",
        "guide.md",
        "--content",
        "# Guide\n",
    ]);

    assert!(output.join("guide.md").is_file());
    assert!(!output.join(".repowiki").exists());
}

#[test]
fn worktree_sessions_are_stored_under_repo_root() {
    fn git(repo: &Path, args: &[&str]) {
        let output = Command::new("git")
            .current_dir(repo)
            .args(args)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let root = tempdir().expect("fixture tempdir");
    let original = root.path().join("original");
    fs::create_dir(&original).expect("create original repository");
    fs::write(original.join("app.py"), "def run():\n    return 1\n").expect("write fixture");
    git(&original, &["init", "--quiet"]);
    git(&original, &["config", "user.name", "CLI Smoke"]);
    git(
        &original,
        &["config", "user.email", "cli-smoke@example.test"],
    );
    git(&original, &["add", "app.py"]);
    git(&original, &["commit", "--quiet", "-m", "initial"]);

    let worktree = root.path().join("worktree");
    let worktree_arg = worktree.to_string_lossy().to_string();
    git(
        &original,
        &[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            worktree_arg.as_str(),
            "HEAD",
        ],
    );

    let original_arg = original.to_string_lossy().to_string();
    let analysis = run([
        "generate",
        "--repo-root",
        original_arg.as_str(),
        "--repo",
        worktree_arg.as_str(),
    ]);
    let session = analysis["session_id"].as_str().expect("session id");
    let session_root = original
        .join(".repowiki")
        .join(".codewiki")
        .join("sessions")
        .join(session);
    let expected_session_root = session_root.to_string_lossy().into_owned();
    assert_eq!(
        analysis["session_path"].as_str(),
        Some(expected_session_root.as_str())
    );
    let expected_component_index = session_root
        .join("component_index.json")
        .to_string_lossy()
        .into_owned();
    assert_eq!(
        analysis["component_index_path"].as_str(),
        Some(expected_component_index.as_str())
    );
    assert!(session_root.join("state.json").is_file());
    let state: Value = serde_json::from_slice(
        &fs::read(session_root.join("state.json")).expect("read session state"),
    )
    .expect("session state JSON");
    let analyzed_repo = worktree
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert_eq!(state["repo_path"], json!(analyzed_repo.as_str()));

    let info = run([
        "session",
        "info",
        "--repo-root",
        original_arg.as_str(),
        "--session",
        session,
    ]);
    assert_eq!(info["session_id"], json!(session));
    assert_eq!(info["repo_path"], state["repo_path"]);
    assert!(original
        .join(".repowiki")
        .join(".codewiki")
        .join("session-locks")
        .join(format!("{session}.lock"))
        .is_file());
    assert!(!original.join(".codewiki").exists());
    assert!(!worktree.join(".codewiki").exists());
    assert!(!worktree
        .join(".repowiki")
        .join(".codewiki")
        .join("sessions")
        .join(session)
        .exists());
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
    let workflow: Value = serde_json::from_str(
        &fs::read_to_string(analysis["workflow_path"].as_str().unwrap())
            .expect("workflow contract"),
    )
    .expect("workflow contract JSON");
    assert_eq!(
        workflow["host_contract"]["session_writes"],
        json!("serialized")
    );
    assert_eq!(
        workflow["host_contract"]["retry_policy"]["doc_write"],
        json!("same_content_only")
    );

    let vars = repo.path().join("vars.json");
    let vars_arg = vars.to_string_lossy().to_string();
    fs::write(&vars, r#"{"module_name":"Service"}"#).expect("write incomplete vars");
    let (success, error) = run_failure([
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
    assert!(!success);
    assert!(error["error"]
        .as_str()
        .unwrap_or_default()
        .contains("doc_path"));

    fs::write(
        &vars,
        r#"{"module_name":"Service","doc_path":"Service.md"}"#,
    )
    .expect("write current vars");
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
        r#"{"Service":{"path":".","components":["app.py::Service","app.py::Service.run","app.py::helper"],"children":{}}}"#,
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
    assert!(saved["result"]["unmatched_architecture_ids"]
        .as_array()
        .unwrap()
        .is_empty());

    let service_doc = "# Service\n\n## Purpose and responsibility\n\nThe Service class in `app.py` is the entry point for this small application. It owns the public `Service.run` operation and keeps callers independent from the helper that performs the calculation. The analyzed source identifies these components as `app.py::Service` and `app.py::Service.run`; this page describes their relationship rather than treating them as unrelated symbols.\n\n## Architecture and request flow\n\nA caller invokes `Service.run`, which delegates the reusable calculation to `helper` and returns the resulting integer. The path is synchronous: there is no queue, persistent state, or external service in the supplied implementation. That boundary matters because the service is responsible for orchestration while the helper owns the calculation itself.\n\n## Interface and module boundary\n\n`Service.run` is the interface visible to the caller. The helper remains an internal implementation detail in `app.py`, represented by `app.py::helper`; callers do not need to know how its result is assembled. Keeping the delegation in one method gives the module a clear entry point and lets the helper remain independently understandable. The page covers only behavior present in the analyzed source.\n";
    let overview_doc = "# Repository Overview\n\n## Purpose\n\nThis repository contains a compact Python service. The analyzed `app.py` source has a public `Service.run` entry point and a helper that performs the calculation. The wiki follows the request from the caller through that entry point and delegation boundary, then explains where to read the implementation. This is a synchronous in-process example: the available source shows no network hop, worker queue, persistent store, or hidden service layer.\n\n## End-to-end architecture\n\nThe caller invokes `Service.run` in `app.py`. The method delegates the reusable calculation to `helper`, receives its integer value, and returns that value to the caller. The `Service` module owns the public orchestration boundary, while the helper owns the calculation. These responsibilities are small but distinct, so readers can begin with the module page and follow the exact path through the source without inferring a larger architecture that is not present.\n\n```mermaid\nflowchart LR\n  Client[Python caller] --> Entry[Service.run in app.py]\n  Entry --> Helper[helper in app.py]\n  Helper --> Value[integer returned to caller]\n```\n\nThe [Service module](Service.md) describes the entry-point contract, the helper relationship, and the synchronous behavior grounded in `app.py`. Start there when tracing the implementation. The repository has no other documented runtime module in this fixture, so the overview keeps the architecture focused on the complete path that the source actually supports. This separation also helps a reader distinguish public orchestration from the reusable calculation, then follow each responsibility back to its source location and understand where a behavior change belongs.\n";
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
        service_doc,
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
        overview_doc,
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
        "API": {
            "path": "src/api.rs",
            "components": [ids[0].clone()],
            "children": {},
            "decomposition_review": {
                "decision": "retain_leaf",
                "breadth_risk": "low",
                "reason": "The API fixture has one public type and no independent internal boundary."
            }
        },
        "Runtime": {
            "path": "src/runtime.rs",
            "components": ids[1..].to_vec(),
            "children": {},
            "decomposition_review": {
                "decision": "retain_leaf",
                "breadth_risk": "low",
                "reason": "The runtime fixture is one cohesive implementation unit."
            }
        }
    });
    fs::write(
        &response,
        format!("<GROUPED_COMPONENTS>{}</GROUPED_COMPONENTS>", grouping),
    )
    .expect("write cluster response");
    let root_tree = work.join("root-tree.json");
    fs::write(&root_tree, "{}").expect("write empty tree");

    let missing_response = work.join("missing-response.txt");
    let missing_tree = work.join("missing-response-tree.json");
    let (success, error) = run_failure([
        "tree",
        "apply-cluster",
        "--repo-root",
        repo_arg.as_str(),
        "--session",
        session,
        "--tree-file",
        root_tree.to_str().unwrap(),
        "--response-file",
        missing_response.to_str().unwrap(),
        "--input-ids-file",
        input_ids.to_str().unwrap(),
        "--scope",
        "repo",
        "--output-tree-file",
        missing_tree.to_str().unwrap(),
    ]);
    assert!(!success);
    assert!(error["error"]
        .as_str()
        .unwrap_or_default()
        .contains("cluster response"));
    assert!(!missing_tree.exists());
    assert_eq!(
        fs::read_to_string(&root_tree).expect("read untouched tree"),
        "{}"
    );

    let empty_response = work.join("empty-response.txt");
    fs::write(&empty_response, "\n \n").expect("write empty response");
    let (success, error) = run_failure([
        "tree",
        "apply-cluster",
        "--repo-root",
        repo_arg.as_str(),
        "--session",
        session,
        "--tree-file",
        root_tree.to_str().unwrap(),
        "--response-file",
        empty_response.to_str().unwrap(),
        "--input-ids-file",
        input_ids.to_str().unwrap(),
        "--scope",
        "repo",
        "--output-tree-file",
        missing_tree.to_str().unwrap(),
    ]);
    assert!(!success);
    assert!(error["error"]
        .as_str()
        .unwrap_or_default()
        .contains("non-empty"));
    assert!(!missing_tree.exists());

    let invalid_ids = work.join("invalid-ids.json");
    fs::write(&invalid_ids, r#"{"module_name":"not-an-id"}"#).expect("write invalid ID object");
    let (success, error) = run_failure([
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
        invalid_ids.to_str().unwrap(),
        "--scope",
        "repo",
        "--output-tree-file",
        missing_tree.to_str().unwrap(),
    ]);
    assert!(!success);
    assert!(error["error"]
        .as_str()
        .unwrap_or_default()
        .contains("input IDs"));

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
    assert_eq!(clustered["diagnostics"]["selected_count"], json!(ids.len()));
    assert_eq!(clustered["diagnostics"]["omitted_count"], json!(0));

    let super_response = work.join("super-response.txt");
    fs::write(
        &super_response,
        r#"<GROUPED_MODULES>{"Platform":{"modules":["API","Runtime"],"decomposition_review":{"decision":"split","breadth_risk":"medium","reason":"The public API and runtime execution are distinct module responsibilities."}}}</GROUPED_MODULES>"#,
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
            "--require-decomposition-review",
        ],
    );
    assert_eq!(saved["result"]["unmatched_architecture_ids"], json!([]));
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
    assert_eq!(
        context_json["repo_structure"]["Platform"]["components"],
        Value::Null
    );
    assert_eq!(
        context_json["repo_structure"]["Platform"]["docs_path"],
        Value::Null
    );
    assert!(context_json["architecture_context"]["nodes"]
        .as_array()
        .is_some());
    assert!(context_json["architecture_context"]["edges"]
        .as_array()
        .is_some());

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
            "API.md" => {
                r#"# API

## Purpose and boundary

The API module owns the public request surface in `src/api.rs`, where the analyzed `Api` type is defined. It gives callers one place to enter the request flow and keeps the runtime implementation behind a narrow boundary. In this fixture, the module has no parser or transport layer of its own; its purpose is to accept work and hand it to the execution subsystem without exposing runtime details.

## Architecture and request flow

The `Api` component in `src/api.rs` is the first documented stage after the caller. It validates and dispatches the request, then passes responsibility to Runtime. Keeping that handoff at the API boundary lets the downstream module own execution while the public surface stays small. The source fixture is intentionally compact, so the page describes only the boundary represented by `Api` rather than inferring validation branches or protocols that are not present.

## Responsibilities and interfaces

The API page is the entry point for reading the [Runtime module](Runtime.md). Runtime performs the next stage and returns the execution result to this layer. This relationship explains why the API and Runtime pages are separate: one describes the caller-facing contract, while the other describes work performed after dispatch. The selected source anchors and file path provide the implementation starting point for both responsibilities.
"#
            }
            "Runtime.md" => {
                r#"# Runtime

## Purpose and boundary

The Runtime module owns the execution implementation in `src/runtime.rs`, where the analyzed `Runtime` type is defined. It receives work from the public API layer, performs the operation represented by the fixture, and returns a result. The module boundary keeps execution details downstream from the caller-facing contract and gives the parent Platform page a concrete child responsibility to explain.

## Architecture and execution path

The `Runtime` component in `src/runtime.rs` is the second stage in the request path. API hands work to this module; Runtime performs the execution step and returns the result to the caller-facing layer. The available source fixture contains no separate storage, scheduler, or external service, so this page keeps the flow local and does not invent additional stages or state transitions.

## Responsibilities and interfaces

Runtime participates in the [Platform subsystem](Platform.md) alongside the [API module](API.md). API owns entry and dispatch, while Runtime owns the execution behavior after that handoff. The parent page explains how the two responsibilities compose; this page stays focused on the source in `src/runtime.rs` and the behavior represented by the `Runtime` component.
"#
            }
            "Platform.md" => {
                r#"# Platform

## Purpose and boundary

Platform groups the caller-facing API and the runtime execution stage into one request path. The `Api` component in `src/api.rs` accepts and dispatches work; the `Runtime` component in `src/runtime.rs` performs the downstream operation. This parent page explains their connection, while each child page owns the detailed explanation of its source boundary.

## Architecture: how the children compose

```mermaid
flowchart LR
  API --> Runtime
```

The [API module](API.md) is the public entry boundary. It hands accepted work to the [Runtime module](Runtime.md), which performs execution and returns the result. The sequence is grounded in the selected `Api` and `Runtime` components and their source paths. It has two clear responsibilities, rather than one broad page that mixes caller interaction with execution details.

## Responsibilities and reading the subsystem

Read API first to understand the public request surface, then follow its handoff into Runtime. The parent exists to show why those modules belong together and how their responsibilities meet; it does not replace their child pages with a repeated component list. No storage or external integration is present in this fixture, so Platform stops at the API-to-runtime flow shown by the source.

The split also gives changes a clear home: changes to the caller-facing contract start in API, while changes to execution behavior start in Runtime. Reviewers can use the parent page to understand the handoff first, then read the child page that owns the relevant responsibility. This keeps the subsystem map useful without duplicating implementation details.
"#
            }
            _ => {
                r#"# Repository Overview

## Purpose

This repository demonstrates a small Rust request path split across a public API and a runtime implementation. The `Api` component in `src/api.rs` receives work from the caller, while the `Runtime` component in `src/runtime.rs` performs the execution stage. The wiki organizes these modules under Platform so a developer can understand the end-to-end shape before opening implementation details. The analyzed fixture contains no additional transport, persistence, or external system, so the overview stays within those source-backed boundaries.

## End-to-end architecture

```mermaid
flowchart LR
  Platform --> API --> Runtime
```

The request enters through API, crosses into Runtime, and returns as the result of the runtime operation. API owns the caller-facing contract and dispatch; Runtime owns execution. Platform provides their shared subsystem context and documents the handoff between those roles. This simple path is the complete architecture visible in the repository fixture, rather than a placeholder for services or infrastructure that the source does not contain. The overview serves as a quick navigation map: it names the two source-backed roles, shows their order, and points from each role to the detailed page where its implementation boundary is described.

## Where to read next

Start with the [Platform module](Platform.md) for the relationship between its children. The [API page](API.md) explains the public entry and dispatch boundary, and the [Runtime page](Runtime.md) follows the execution stage in `src/runtime.rs`. Together these pages provide a route from the overall request flow to the components that implement it without repeating each page's detailed explanation.
"#
            }
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
fn concurrent_document_writes_are_serialized_and_idempotent() {
    let repo = tempdir().expect("repo tempdir");
    let output = tempdir().expect("output tempdir");
    let repo_arg = repo.path().to_string_lossy().to_string();
    let output_arg = output.path().to_string_lossy().to_string();
    fs::write(repo.path().join("app.py"), "def run():\n    return 1\n").expect("write fixture");
    let analysis = run([
        "generate",
        "--repo",
        repo_arg.as_str(),
        "--output",
        output_arg.as_str(),
    ]);
    let session = analysis["session_id"].as_str().expect("session id");

    let mut children = Vec::new();
    for index in 0..8 {
        let path = format!("page-{index}.md");
        let content = format!("# Page {index}\n");
        children.push(spawn([
            "doc",
            "write",
            "--repo-root",
            repo_arg.as_str(),
            "--session",
            session,
            "--path",
            path.as_str(),
            "--content",
            content.as_str(),
        ]));
    }
    for child in children {
        let output = child.wait_with_output().expect("wait for document writer");
        assert!(
            output.status.success(),
            "stdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    for index in 0..8 {
        assert!(output.path().join(format!("page-{index}.md")).is_file());
    }
    let info = run([
        "session",
        "info",
        "--repo-root",
        repo_arg.as_str(),
        "--session",
        session,
    ]);
    assert_eq!(info["docs_written"], json!(8));

    let mut same_path_children = Vec::new();
    for _ in 0..2 {
        same_path_children.push(spawn([
            "doc",
            "write",
            "--repo-root",
            repo_arg.as_str(),
            "--session",
            session,
            "--path",
            "same.md",
            "--content",
            "# Same\n",
        ]));
    }
    let results = same_path_children
        .into_iter()
        .map(|child| child.wait_with_output().expect("wait for same-path writer"))
        .collect::<Vec<_>>();
    assert_eq!(
        results
            .iter()
            .filter(|result| result.status.success())
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|result| !result.status.success())
            .count(),
        1
    );

    let reused = run([
        "doc",
        "write",
        "--repo-root",
        repo_arg.as_str(),
        "--session",
        session,
        "--path",
        "same.md",
        "--content",
        "# Same\n",
        "--if-existing",
        "same",
    ]);
    assert_eq!(reused["result"]["created"], json!(false));
    assert_eq!(reused["result"]["reused"], json!(true));

    let (success, error) = run_failure([
        "doc",
        "write",
        "--repo-root",
        repo_arg.as_str(),
        "--session",
        session,
        "--path",
        "same.md",
        "--content",
        "# Different\n",
        "--if-existing",
        "same",
    ]);
    assert!(!success);
    assert!(error["error"]
        .as_str()
        .unwrap_or_default()
        .contains("different"));
    assert_eq!(
        fs::read_to_string(output.path().join("same.md")).expect("read same page"),
        "# Same\n"
    );
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
