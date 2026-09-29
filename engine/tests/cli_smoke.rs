#[path = "common/mod.rs"]
mod common;

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
    let output = Command::new(env!("CARGO_BIN_EXE_repowiki"))
        .args(args)
        .output()
        .expect("run repowiki");
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
    let output = Command::new(env!("CARGO_BIN_EXE_repowiki"))
        .current_dir(directory)
        .args(args)
        .output()
        .expect("run repowiki");
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
    let output = Command::new(env!("CARGO_BIN_EXE_repowiki"))
        .args(args)
        .output()
        .expect("run repowiki");
    let value = serde_json::from_slice(&output.stdout).expect("JSON error stdout");
    (output.status.success(), value)
}

fn spawn<I, S>(args: I) -> std::process::Child
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    Command::new(env!("CARGO_BIN_EXE_repowiki"))
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn repowiki")
}

#[test]
fn default_output_dir_is_repowiki_and_document_paths_are_strict() {
    if !common::dokuwiki_runtime_available() {
        return;
    }
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

    for path in [".repowiki/guide.md", "docs/legacy.md", "guide.md"] {
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
            "====== Guide ======\n",
        ]);
        assert!(!success, "non-page-ID document path should fail: {path}");
    }
    let written = run([
        "doc",
        "write",
        "--repo-root",
        repo_arg.as_str(),
        "--session",
        session,
        "--path",
        "repo:guide:start",
        "--content",
        "====== Guide ======\n",
    ]);

    assert_eq!(written["result"]["path"], json!("repo:guide:start"));
    assert!(output
        .join("dokuwiki/data/pages/repo/guide/start.txt")
        .is_file());
    assert!(!output.join("guide.md").exists());
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
        .join(".state")
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
        .join(".state")
        .join("session-locks")
        .join(format!("{session}.lock"))
        .is_file());
    assert!(!original.join(".state").exists());
    assert!(!worktree.join(".state").exists());
    assert!(!worktree
        .join(".repowiki")
        .join(".state")
        .join("sessions")
        .join(session)
        .exists());
}

#[test]
fn file_side_workflow_creates_reference_artifacts() {
    if !common::dokuwiki_runtime_available() {
        return;
    }
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
        r#"{"module_name":"Service","doc_path":"repo:service:start"}"#,
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

    let service_doc = r#"====== Service ======

===== Purpose and responsibility =====

The Service module forms the single public entry point for this compact Python fixture. In app.py, app.py::Service owns app.py::Service.run, while app.py::helper supplies the reusable calculation invoked by that method. The class keeps callers independent from the implementation detail and returns the helper's integer result without adding another stateful layer. There is no network boundary, queue, persistent store, or external service in the supplied source, so this page describes the complete in-process responsibility rather than assuming infrastructure that is absent.

===== Architecture and request flow =====

A caller constructs Service and calls Service.run. The method delegates the calculation to helper in the same process, receives its result, and returns it to that caller. app.py::Service and app.py::Service.run anchor the public operation, while app.py::helper anchors the delegated operation. This sequence is synchronous: the method call itself establishes the control transfer, and no retry, asynchronous callback, or hidden persistence is visible in app.py. Keeping that separation makes the service responsible for orchestration and the helper responsible for the reusable computation.

===== Responsibilities and interfaces =====

The stable interface is Service.run, which returns the integer produced by helper. Callers do not need to know how helper computes that value, and the current fixture shows no additional configuration or lifecycle state. Changes to the public operation belong at the Service boundary; changes to the reusable calculation belong in helper. This division follows the two source-backed components in app.py and keeps the explanation focused on behavior that the implementation actually provides.
"#;
    let overview_doc = r#"====== Repository Overview ======

===== Purpose =====

This repository contains a compact Python service whose complete request path stays inside one process. The Service class in app.py receives work through Service.run and delegates a reusable calculation to helper. The resulting integer returns through the same call chain to the caller. The analyzed source exposes no network transport, background worker, persistent store, or external service, so this overview limits its architecture to the components actually present. The [[repo:service:start|Service module]] page explains the class boundary and the behavior owned by its public operation.

===== End-to-end architecture =====

<mermaid>
flowchart LR
  Service --> Helper
</mermaid>

The request enters the Service boundary at Service.run, crosses the local delegation to helper, and returns as the helper's integer result. Service owns orchestration and the externally visible method, while helper owns the reusable calculation. This is a synchronous path rather than a distributed interaction: the call to helper occurs in the same execution flow and the provided fixture does not introduce retries, queues, or stored state. The diagram names those two source-backed responsibilities and shows their ordering without adding infrastructure that the repository does not contain.

===== Where to read next =====

Start with the Service module page for its purpose, request flow, and public interface. Its discussion ties the Service class and Service.run to app.py, then follows the handoff to helper. That page provides the detailed implementation boundary behind this overview, while this page remains a navigation map of the complete small fixture. If the implementation changes, compare the same local entry, delegation, and return path against app.py rather than assuming a larger service topology.
"#;
    let written = run([
        "doc",
        "write",
        "--repo-root",
        repo_arg.as_str(),
        "--session",
        session,
        "--path",
        "repo:service:start",
        "--content",
        service_doc,
    ]);
    assert_eq!(written["result"]["path"], json!("repo:service:start"));
    assert!(output
        .path()
        .join("dokuwiki/data/pages/repo/service/start.txt")
        .is_file());
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
        "repo:start",
        "--content",
        overview_doc,
    ]);
    let viewed = run([
        "doc",
        "view",
        "--repo-root",
        repo_arg.as_str(),
        "--session",
        session,
        "--path",
        "repo:start",
    ]);
    assert_eq!(viewed["result"]["page_id"], json!("repo:start"));
    assert_eq!(viewed["result"]["content"], json!(overview_doc));
    assert_eq!(viewed["result"]["links"], json!(["repo:service:start"]));
    assert!(viewed["result"]["html"]
        .as_str()
        .unwrap()
        .contains("Service module"));

    let report = run([
        "doc",
        "validate",
        "--repo-root",
        repo_arg.as_str(),
        "--session",
        session,
    ]);
    assert_eq!(
        report["result"]["pages"]["repo:service:start"]["parser_succeeded"],
        json!(true)
    );
    assert_eq!(
        report["result"]["pages"]["repo:start"]["parser_succeeded"],
        json!(true)
    );
    assert_eq!(
        report["result"]["checks"]["canonical_page_ids"],
        json!(true)
    );

    assert_eq!(report["result"]["valid"], true, "{report}");
    let export = run([
        "html",
        "--repo-root",
        repo_arg.as_str(),
        "--session",
        session,
    ]);
    assert_eq!(export["ok"], json!(true));
    let html_path = output.path().join("index.html");
    let html = fs::read_to_string(&html_path).expect("read DokuWiki HTML export");
    assert!(html.contains("class=\"mermaid\""));
    assert!(html.contains("href=\"#page-repo--service--start\""));
    assert!(!html.contains("cdn.jsdelivr.net"));
    assert!(output.path().join("assets/mermaid.min.js").is_file());
    assert!(output.path().join("assets/mermaid.css").is_file());
    assert!(html.contains("assets/LICENSE-mermaid-plugin-GPL.txt"));
    assert!(html.contains("assets/LICENSE-mermaid-js-MIT.txt"));
    assert!(output
        .path()
        .join("assets/LICENSE-mermaid-plugin-GPL.txt")
        .is_file());
    assert!(output
        .path()
        .join("assets/LICENSE-mermaid-js-MIT.txt")
        .is_file());
    assert!(html.contains("assets/repowiki-viewer.js"));
    assert!(html.contains("assets/repowiki-viewer.css"));
    assert!(output.path().join("assets/repowiki-viewer.js").is_file());
    assert!(output.path().join("assets/repowiki-viewer.css").is_file());
    let viewer_js = fs::read_to_string(output.path().join("assets/repowiki-viewer.js"))
        .expect("read exported diagram viewer script");
    assert!(
        viewer_js.contains("repowiki-diagram-actions")
            && viewer_js.contains("open-tab")
            && viewer_js.contains("XMLSerializer"),
        "the exported viewer script must carry the corner actions and the new-tab opener"
    );
    assert!(
        !viewer_js.to_lowercase().contains("</script"),
        "the viewer script is serialized into generated documents and must not contain a script end tag"
    );
    assert!(
        !viewer_js.contains("cdn.jsdelivr.net"),
        "the exported viewer script must stay self-contained"
    );
    let viewer_css = fs::read_to_string(output.path().join("assets/repowiki-viewer.css"))
        .expect("read exported diagram viewer styles");
    assert!(
        viewer_css.contains("repowiki-diagram-actions"),
        "the exported viewer styles must carry the corner actions"
    );
    let closed = run([
        "session",
        "close",
        "--repo-root",
        repo_arg.as_str(),
        "--session",
        session,
    ]);
    assert_eq!(closed["cleaned"], true);
    let session_state_root = repo.path().join(".repowiki").join(".state");
    assert!(
        !session_state_root
            .join("session-locks")
            .join(format!("{session}.lock"))
            .exists(),
        "the session lock must not linger after close"
    );
    assert!(
        !session_state_root.exists(),
        "the session storage root must be reclaimed once no session remains"
    );
    assert!(output.path().join("metadata.json").exists());
    let metadata: Value =
        serde_json::from_slice(&fs::read(output.path().join("metadata.json")).expect("metadata"))
            .expect("metadata JSON");
    let generated = metadata["files_generated"]
        .as_array()
        .expect("generated files");
    for page_path in [
        "dokuwiki/data/pages/repo/service/start.txt",
        "dokuwiki/data/pages/repo/start.txt",
    ] {
        assert!(
            generated.iter().any(|path| path == page_path),
            "metadata omitted output-relative DokuWiki page storage path {page_path}"
        );
    }

    fs::write(
        repo.path().join("app.py"),
        "class Service:\n    def run(self):\n        return helper()\n\ndef helper():\n    return 1\n\ndef added_helper():\n    return 2\n",
    )
    .expect("add a source component for the update");
    let updated = run([
        "generate",
        "--repo",
        repo_arg.as_str(),
        "--output",
        output_arg.as_str(),
        "--update",
    ]);
    let update_session = updated["session_id"].as_str().expect("update session ID");
    assert_eq!(updated["update_plan"]["outcome"], json!("incremental"));
    assert!(updated["update_plan"]["diff"]["added"]
        .as_array()
        .expect("added component IDs")
        .contains(&json!("app.py::added_helper")));

    let routed = run([
        "update",
        "route",
        "--repo-root",
        repo_arg.as_str(),
        "--session",
        update_session,
    ]);
    assert_eq!(
        routed["routes"]["app.py::added_helper"],
        json!("repo:service:start"),
        "{routed}"
    );
    let decisions = repo.path().join("update-decisions.json");
    fs::write(
        &decisions,
        serde_json::to_vec(&json!({
            "decisions": [{
                "component_id": "app.py::added_helper",
                "action": "place",
                "leaf": "repo:service:start"
            }]
        }))
        .expect("serialize update decisions"),
    )
    .expect("write update decisions");
    let decisions_arg = decisions.to_string_lossy().to_string();
    run([
        "update",
        "route-apply",
        "--repo-root",
        repo_arg.as_str(),
        "--session",
        update_session,
        "--decisions-file",
        decisions_arg.as_str(),
    ]);

    let context = run([
        "update",
        "context",
        "--repo-root",
        repo_arg.as_str(),
        "--session",
        update_session,
    ]);
    assert_eq!(context["stale_scan"]["missing_pages"], json!([]));
    assert_eq!(context["stale_scan"]["broken_links"], json!([]));
    let stale = run([
        "update",
        "stale-scan",
        "--repo-root",
        repo_arg.as_str(),
        "--session",
        update_session,
    ]);
    assert_eq!(stale["scanned"], json!(true));

    let operations = repo.path().join("update-operations.json");
    fs::write(
        &operations,
        serde_json::to_vec(&json!([{
            "kind": "str_replace",
            "old": "The stable interface is Service.run",
            "new": "The stable interface is Service.run, alongside app.py::added_helper"
        }]))
        .expect("serialize DokuWiki edit operations"),
    )
    .expect("write DokuWiki edit operations");
    let operations_arg = operations.to_string_lossy().to_string();
    run([
        "doc",
        "edit",
        "--repo-root",
        repo_arg.as_str(),
        "--session",
        update_session,
        "--path",
        "repo:service:start",
        "--operations-file",
        operations_arg.as_str(),
    ]);
    let updated_page = run([
        "doc",
        "view",
        "--repo-root",
        repo_arg.as_str(),
        "--session",
        update_session,
        "--path",
        "repo:service:start",
    ]);
    assert!(updated_page["result"]["content"]
        .as_str()
        .expect("updated DokuWiki page source")
        .contains("app.py::added_helper"));

    let verdicts = repo.path().join("update-verdicts.json");
    fs::write(
        &verdicts,
        serde_json::to_vec(&json!({
            "verdicts": {
                "repo:service:start": "patch",
                "repo:start": "patch"
            }
        }))
        .expect("serialize canonical page verdicts"),
    )
    .expect("write update verdicts");
    let verdicts_arg = verdicts.to_string_lossy().to_string();
    let finalized = run([
        "update",
        "finalize",
        "--repo-root",
        repo_arg.as_str(),
        "--session",
        update_session,
        "--verdicts-file",
        verdicts_arg.as_str(),
    ]);
    assert_eq!(finalized["outcome"], json!("incremental"));
    let update_record: Value =
        serde_json::from_slice(&fs::read(output.path().join("update_record.json")).unwrap())
            .expect("update record JSON");
    assert_eq!(
        update_record["verdicts"]["repo:service:start"],
        json!("patch")
    );
    let updated_close = run([
        "session",
        "close",
        "--repo-root",
        repo_arg.as_str(),
        "--session",
        update_session,
    ]);
    assert_eq!(updated_close["cleaned"], json!(true));
}

#[test]
fn recursive_tree_commands_are_file_side_and_work_outside_repo_cwd() {
    if !common::dokuwiki_runtime_available() {
        return;
    }
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
    for (module, page_id) in [
        ("API", "repo:platform:api:start"),
        ("Runtime", "repo:platform:runtime:start"),
        ("Platform", "repo:platform:start"),
    ] {
        let item = items
            .iter()
            .find(|item| item["module"] == module)
            .expect("processing item");
        assert_eq!(item["doc_path"], json!(page_id));
    }

    let pages = [
        (
            "repo:platform:api:start",
            r#"====== API ======

===== Purpose and boundary =====

The API module owns the public request surface implemented in src/api.rs, where the analyzed Api type is defined. It gives callers one place to enter the request flow and keeps the runtime implementation behind a narrow boundary. In this fixture the API has no parser or transport layer of its own; its purpose is to accept work and hand it to the execution subsystem without exposing runtime details. That small responsibility makes the entry point clear without inventing protocols absent from the source.

===== Architecture and request flow =====

The Api component in src/api.rs is the first documented stage after the caller. It dispatches the request to Runtime, then returns the execution result through the same public boundary. Keeping that handoff in the API module lets the downstream module own execution while the caller-facing surface stays small. The selected source anchor src/api.rs::Api and its path identify the implementation behind this role. The fixture is intentionally compact, so this page does not infer validation branches or other behavior not shown in the source.

===== Responsibilities and interfaces =====

The API page links to the [[repo:platform:runtime:start|Runtime module]] because that module performs the next stage in the request path. API owns entry and dispatch; Runtime owns execution after the handoff. This relationship explains why the pages are separate rather than combining caller interaction and implementation detail. The source anchor and file path give a concrete starting point for both responsibilities, and the child page describes the behavior on the other side of the boundary.
"#,
        ),
        (
            "repo:platform:runtime:start",
            r#"====== Runtime ======

===== Purpose and boundary =====

The Runtime module owns the execution implementation in src/runtime.rs, where the analyzed Runtime type is defined. It receives work from the public API layer, performs the operation represented by the fixture, and returns a result. This boundary keeps execution details downstream from the caller-facing contract and gives the parent Platform page a concrete child responsibility to explain. The supplied code contains no separate storage, scheduler, or external service.

===== Architecture and execution path =====

The Runtime component in src/runtime.rs is the execution stage in the request path. API hands work to this module; Runtime performs the local operation and returns the result to the caller-facing layer. The selected source anchor src/runtime.rs::Runtime and its implementation path identify the behavior owned here. Since the available fixture has no additional service boundary or stored state, the page follows only the in-process transition visible between API and Runtime instead of adding unsupported stages.

===== Responsibilities and interfaces =====

Runtime participates in the Platform subsystem beside the API module, whose public boundary is documented separately. API owns entry and dispatch, while Runtime owns the execution behavior after that handoff. The parent page explains how these responsibilities compose, and the API page explains how callers enter the flow. This page stays focused on src/runtime.rs and the behavior represented by the Runtime component rather than repeating its sibling's interface description.
"#,
        ),
        (
            "repo:platform:start",
            r#"====== Platform ======

===== Purpose and boundary =====

Platform groups the caller-facing API and the runtime execution stage into one request path. The Api component in src/api.rs accepts and dispatches work; the Runtime component in src/runtime.rs performs the downstream operation. Their selected source anchors, src/api.rs::Api and src/runtime.rs::Runtime, define the two responsibilities represented by this parent. The module explains their connection, while each child page owns the detailed account of its implementation boundary. The fixture contains no storage or external integration.

===== Architecture: how the children compose =====

<mermaid>
flowchart LR
  API --> Runtime
</mermaid>

The [[repo:platform:api:start|API module]] is the public entry boundary. It hands accepted work to the [[repo:platform:runtime:start|Runtime module]], which performs execution and returns the result. This sequence is grounded in the selected Api and Runtime components and their source paths. It has two clear responsibilities rather than one broad page that mixes caller interaction with execution details. The parent provides a shared view of the handoff without duplicating each child's explanation.

===== Responsibilities and reading the subsystem =====

Read API first to understand the public request surface, then follow its handoff into Runtime. The parent exists to show why those modules belong together and how their responsibilities meet; it does not replace their child pages with a repeated component list. No storage or external integration is present in this fixture, so Platform stops at the API-to-runtime flow shown by the source. This gives readers a focused route from request entry to execution while keeping each page aligned with the responsibility represented by its own source anchors.

The split also gives changes a clear home: changes to the caller-facing contract start in API, while changes to execution behavior start in Runtime. Reviewers can use this parent page to understand the handoff first, then read the child page that owns the relevant responsibility. The parent-child structure is therefore an explanation of the implementation boundary as well as a navigation map; it does not add infrastructure or behavior that the fixture does not contain.
"#,
        ),
        (
            "repo:start",
            r#"====== Repository Overview ======

===== Purpose =====

This repository demonstrates a small Rust request path split across a public API and a runtime implementation. The Api component in src/api.rs receives work from the caller, while the Runtime component in src/runtime.rs performs the execution stage. The wiki organizes these responsibilities under Platform so a developer can understand the end-to-end shape before opening implementation details. The analyzed fixture contains no additional transport, persistence, or external system, so this overview stays within the boundaries shown by the source. The [[repo:platform:start|Platform module]] page explains how its children compose.

===== End-to-end architecture =====

<mermaid>
flowchart LR
  Platform --> API
</mermaid>

The request enters through API, crosses into Runtime, and returns as the result of the execution step. API owns the caller-facing contract and dispatch; Runtime owns the downstream work. Platform provides their shared subsystem context and documents the handoff between those roles. This path is the complete architecture visible in the fixture, rather than a placeholder for services or infrastructure the source does not contain. The diagram highlights the Platform-to-API navigation path and the module pages provide the detailed account of the API-to-Runtime handoff.

===== Where to read next =====

Start with the Platform module for the relationship between its children. The API page explains public entry and dispatch, and the Runtime page follows execution in src/runtime.rs. Together these pages provide a route from the overall request flow to the components that implement it without repeating each page's detailed explanation. The wiki is intended as a source-grounded map: readers can follow the parent link, then inspect the child that owns the relevant behavior.
"#,
        ),
    ];
    for (page, content) in pages {
        let written = run_from(
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
        assert_eq!(written["result"]["path"], json!(page));
    }
    for page_file in [
        "repo/platform/api/start.txt",
        "repo/platform/runtime/start.txt",
        "repo/platform/start.txt",
        "repo/start.txt",
    ] {
        assert!(
            output
                .path()
                .join("dokuwiki/data/pages")
                .join(page_file)
                .is_file(),
            "missing DokuWiki page storage file {page_file}"
        );
    }

    let report = run_from(
        scratch.path(),
        [
            "doc",
            "validate",
            "--repo-root",
            repo_arg.as_str(),
            "--session",
            session,
        ],
    );
    assert_eq!(report["result"]["valid"], true, "{report}");
    for page_id in [
        "repo:platform:api:start",
        "repo:platform:runtime:start",
        "repo:platform:start",
        "repo:start",
    ] {
        assert_eq!(
            report["result"]["pages"][page_id]["parser_succeeded"],
            json!(true),
            "missing native page report for {page_id}"
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
    let metadata: Value =
        serde_json::from_slice(&fs::read(output.path().join("metadata.json")).expect("metadata"))
            .expect("metadata JSON");
    let generated = metadata["files_generated"]
        .as_array()
        .expect("generated files");
    for page_path in [
        "dokuwiki/data/pages/repo/platform/api/start.txt",
        "dokuwiki/data/pages/repo/platform/runtime/start.txt",
        "dokuwiki/data/pages/repo/platform/start.txt",
        "dokuwiki/data/pages/repo/start.txt",
    ] {
        assert!(
            generated.iter().any(|path| path == page_path),
            "metadata omitted output-relative DokuWiki page path {page_path}"
        );
    }
}

#[test]
fn concurrent_document_writes_are_serialized_and_idempotent() {
    if !common::dokuwiki_runtime_available() {
        return;
    }
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
        let page_id = format!("repo:page-{index}:start");
        let content = format!("====== Page {index} ======\n");
        children.push(spawn([
            "doc",
            "write",
            "--repo-root",
            repo_arg.as_str(),
            "--session",
            session,
            "--path",
            page_id.as_str(),
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
        let page_file = format!("repo/page-{index}/start.txt");
        assert!(output
            .path()
            .join("dokuwiki/data/pages")
            .join(page_file)
            .is_file());
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
            "repo:same:start",
            "--content",
            "====== Same ======\n",
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
        "repo:same:start",
        "--content",
        "====== Same ======\n",
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
        "repo:same:start",
        "--content",
        "====== Different ======\n",
        "--if-existing",
        "same",
    ]);
    assert!(!success);
    assert!(error["error"]
        .as_str()
        .unwrap_or_default()
        .contains("different"));
    assert_eq!(
        fs::read_to_string(
            output
                .path()
                .join("dokuwiki/data/pages/repo/same/start.txt"),
        )
        .expect("read same page"),
        "====== Same ======\n"
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

#[test]
fn invalid_documentation_close_keeps_session_and_report() {
    if !common::dokuwiki_runtime_available() {
        return;
    }
    let repo = tempdir().expect("repo tempdir");
    let repo_arg = repo.path().to_string_lossy().to_string();
    fs::write(
        repo.path().join("app.py"),
        "class Service:\n    def run(self):\n        return helper()\n\ndef helper():\n    return 1\n",
    )
    .expect("write fixture");

    let analysis = run(["generate", "--repo", repo_arg.as_str()]);
    let session = analysis["session_id"].as_str().expect("session id");
    let tree = repo.path().join("tree.json");
    fs::write(
        &tree,
        r#"{"Service":{"path":".","components":["app.py::Service","app.py::Service.run","app.py::helper"],"children":{}}}"#,
    )
    .expect("write tree");
    let tree_arg = tree.to_string_lossy().to_string();
    run([
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

    for (page_id, content) in [
        ("repo:service:start", "====== Service ======\n"),
        ("repo:start", "====== Repository Overview ======\n"),
    ] {
        run([
            "doc",
            "write",
            "--repo-root",
            repo_arg.as_str(),
            "--session",
            session,
            "--path",
            page_id,
            "--content",
            content,
        ]);
    }
    let report = run([
        "doc",
        "validate",
        "--repo-root",
        repo_arg.as_str(),
        "--session",
        session,
    ]);
    assert_eq!(report["result"]["valid"], false, "{report}");
    assert_eq!(
        report["result"]["actual_page_ids"],
        json!(["repo:service:start", "repo:start"])
    );
    assert!(report["result"]["pages"]
        .as_object()
        .expect("page report map")
        .contains_key("repo:service:start"));
    assert!(report["result"]["pages"]
        .as_object()
        .expect("page report map")
        .contains_key("repo:start"));

    let (success, error) = run_failure([
        "session",
        "close",
        "--repo-root",
        repo_arg.as_str(),
        "--session",
        session,
    ]);
    assert!(!success);
    assert!(error["error"]
        .as_str()
        .unwrap_or_default()
        .contains("incomplete documentation"));

    let session_root = repo.path().join(".repowiki/.state/sessions").join(session);
    assert!(session_root.is_dir());
    let state: Value = serde_json::from_slice(
        &fs::read(session_root.join("state.json")).expect("read session state"),
    )
    .expect("session state JSON");
    assert_eq!(state["closed"], false);
    let persisted_report: Value = serde_json::from_slice(
        &fs::read(session_root.join("documentation_validation.json"))
            .expect("read persisted validation report"),
    )
    .expect("documentation report JSON");
    assert_eq!(persisted_report["valid"], false);
    assert_eq!(
        persisted_report["actual_page_ids"],
        json!(["repo:service:start", "repo:start"])
    );
    assert!(persisted_report["pages"]
        .as_object()
        .expect("persisted page report map")
        .contains_key("repo:service:start"));
    assert!(persisted_report["pages"]
        .as_object()
        .expect("persisted page report map")
        .contains_key("repo:start"));
    assert!(!repo.path().join(".repowiki/metadata.json").exists());
}

#[test]
fn prompt_get_hydrates_null_component_inputs_and_preserves_explicit_values() {
    let repo = tempdir().expect("repo tempdir");
    let repo_arg = repo.path().to_string_lossy().to_string();
    fs::write(
        repo.path().join("app.py"),
        "class Service:\n    def run(self):\n        return 7\n",
    )
    .expect("write fixture");
    let analysis = run(["generate", "--repo", repo_arg.as_str()]);
    let session = analysis["session_id"].as_str().expect("session id");

    let run_prompt = |name: &str, prompt_type: &str, vars: Value| {
        let vars_path = repo.path().join(format!("{name}.json"));
        fs::write(
            &vars_path,
            serde_json::to_vec(&vars).expect("serialize prompt variables"),
        )
        .expect("write prompt variables");
        let vars_arg = vars_path.to_string_lossy().to_string();
        let prompt = run([
            "prompt",
            "get",
            "--repo-root",
            repo_arg.as_str(),
            "--session",
            session,
            "--type",
            prompt_type,
            "--vars-file",
            vars_arg.as_str(),
        ]);
        fs::read_to_string(prompt["path"].as_str().expect("prompt path"))
            .expect("read rendered prompt")
    };

    let cluster = run_prompt(
        "cluster-null",
        "cluster",
        json!({
            "potential_core_components": null,
            "component_ids": ["app.py::Service"]
        }),
    );
    assert!(cluster.contains("app.py::Service"), "{cluster}");
    assert!(cluster.contains("class Service:"), "{cluster}");

    let user = run_prompt(
        "user-null",
        "user",
        json!({
            "module_name": "Service",
            "module_tree": {},
            "formatted_core_component_codes": null,
            "component_ids": ["app.py::Service"]
        }),
    );
    assert!(user.contains("app.py::Service"), "{user}");
    assert!(user.contains("class Service:"), "{user}");

    let cluster_explicit = run_prompt(
        "cluster-explicit",
        "cluster",
        json!({
            "potential_core_components": "",
            "component_ids": ["app.py::Service"]
        }),
    );
    assert!(
        !cluster_explicit.contains("app.py::Service"),
        "{cluster_explicit}"
    );
    assert!(
        !cluster_explicit.contains("class Service:"),
        "{cluster_explicit}"
    );

    let user_explicit = run_prompt(
        "user-explicit",
        "user",
        json!({
            "module_name": "Service",
            "module_tree": {},
            "formatted_core_component_codes": "KEEP_USER_FORMATTED_INPUT",
            "component_ids": ["app.py::Service"]
        }),
    );
    assert!(
        user_explicit.contains("KEEP_USER_FORMATTED_INPUT"),
        "{user_explicit}"
    );
    assert!(!user_explicit.contains("class Service:"), "{user_explicit}");
}
