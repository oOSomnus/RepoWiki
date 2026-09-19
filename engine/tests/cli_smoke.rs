use serde_json::Value;
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
        "# Service\n",
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
        "# Overview\n",
    ]);

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
