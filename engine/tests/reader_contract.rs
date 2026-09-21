use codewiki::reader::{load_manifest, WikiReader};
use serde_json::json;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::process::{Command, Stdio};
use tempfile::TempDir;

fn fixture() -> (TempDir, std::path::PathBuf) {
    let repository = tempfile::tempdir().expect("repository tempdir");
    let wiki = repository.path().join(".repowiki");
    fs::create_dir(&wiki).expect("create wiki directory");
    fs::write(
        wiki.join("overview.md"),
        "# Repo overview\n\nA **readable** page with [Platform](Platform.md#architecture).\n\n```rust\nfn main() {}\n```\n\n```mermaid\nflowchart LR\n  Client --> Platform\n```\n",
    )
    .expect("write overview");
    fs::write(
        wiki.join("Platform.md"),
        "# Platform\n\n## Architecture\n\nThe platform boundary.\n",
    )
    .expect("write platform page");
    fs::write(wiki.join("API.md"), "# API\n\nThe public API.\n").expect("write api page");
    fs::write(wiki.join("notes.md"), "# Notes\n\nAn extra page.\n").expect("write extra page");
    fs::write(
        wiki.join("module_tree.json"),
        serde_json::to_vec_pretty(&json!({
            "Platform": {
                "path": "src/platform",
                "components": ["src/platform.rs::Platform"],
                "children": {
                    "API": {
                        "path": "src/api",
                        "components": ["src/api.rs::Api"],
                        "children": {}
                    }
                }
            }
        }))
        .expect("serialize module tree"),
    )
    .expect("write module tree");
    fs::write(
        wiki.join("metadata.json"),
        serde_json::to_vec(&json!({
            "generation_info": {
                "timestamp": "2026-09-20T12:00:00Z",
                "main_model": "fixture-model",
                "repo_path": "/tmp/repo-name",
                "commit_id": "1234567890abcdef"
            },
            "statistics": {
                "total_components": 2,
                "module_count": 2,
                "leaf_nodes": 1
            }
        }))
        .expect("serialize metadata"),
    )
    .expect("write metadata");
    (repository, wiki)
}

fn http_request(address: SocketAddr, method: &str, target: &str) -> String {
    let mut stream = TcpStream::connect(address).expect("connect reader");
    write!(
        stream,
        "{method} {target} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
    )
    .expect("write HTTP request");
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .expect("read HTTP response");
    String::from_utf8(response).expect("HTTP response is UTF-8")
}

fn response_body(response: &str) -> &str {
    response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body)
        .expect("HTTP response headers")
}

#[test]
fn manifest_follows_tree_and_keeps_extra_pages() {
    let (_repository, wiki) = fixture();
    let manifest = load_manifest(&wiki).expect("load reader manifest");

    assert_eq!(manifest.title, "repo-name");
    assert_eq!(manifest.info.model.as_deref(), Some("fixture-model"));
    assert_eq!(manifest.info.commit.as_deref(), Some("12345678"));
    assert_eq!(manifest.info.total_components, Some(2));
    assert_eq!(manifest.navigation[0].name, "Platform");
    assert_eq!(manifest.navigation[0].children[0].name, "API");
    assert_eq!(manifest.navigation[0].children[0].filename, "API.md");
    assert_eq!(manifest.pages[0].filename, "overview.md");
    assert!(manifest
        .pages
        .iter()
        .any(|page| page.filename == "notes.md"));
    assert!(manifest.warnings.is_empty());
}

#[test]
fn page_reads_are_limited_to_direct_markdown_files() {
    let (repository, wiki) = fixture();
    fs::write(repository.path().join("secret.md"), "not part of the wiki").expect("write secret");
    let reader = WikiReader::open(&wiki).expect("open reader");

    assert!(reader
        .read_page("overview.md")
        .unwrap()
        .contains("Repo overview"));
    assert!(reader.read_page("../secret.md").is_err());
    assert!(reader.read_page("metadata.json").is_err());

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(repository.path().join("secret.md"), wiki.join("link.md"))
            .expect("create outside symlink");
        assert!(reader.read_page("link.md").is_err());
    }
}

#[test]
fn binary_serves_offline_assets_manifest_pages_and_safe_errors() {
    let (_repository, wiki) = fixture();
    let mut child = Command::new(env!("CARGO_BIN_EXE_repowiki-reader"))
        .arg(&wiki)
        .args(["--port", "0", "--no-open"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start reader binary");
    let stdout = child.stdout.take().expect("reader stdout");
    let mut lines = BufReader::new(stdout).lines();
    let mut listening = None;
    let mut status_lines = Vec::new();
    for line in lines.by_ref().take(3) {
        let line = line.expect("read reader status");
        status_lines.push(line.clone());
        if let Some(url) = line.strip_prefix("Listening on ") {
            listening = Some(url.trim_end_matches('/').to_string());
            break;
        }
    }
    let url = match listening {
        Some(url) => url,
        None => {
            let status = child.try_wait().expect("inspect reader status");
            let mut stderr = String::new();
            if status.is_some() {
                child
                    .stderr
                    .take()
                    .expect("reader stderr")
                    .read_to_string(&mut stderr)
                    .expect("read reader stderr");
            }
            let _ = child.kill();
            panic!("reader listening URL missing; lines={status_lines:?}, status={status:?}, stderr={stderr}");
        }
    };
    let address = url
        .strip_prefix("http://")
        .expect("loopback HTTP URL")
        .parse::<SocketAddr>()
        .expect("reader socket address");

    let root = http_request(address, "GET", "/");
    assert!(root.starts_with("HTTP/1.1 200 OK"));
    assert!(root.contains("/assets/vendor/mermaid.min.js"));
    assert!(!root.contains("cdn.jsdelivr.net"));
    for asset in [
        "/assets/app.js",
        "/assets/styles.css",
        "/assets/vendor/marked.min.js",
        "/assets/vendor/marked-gfm-heading-id.min.js",
        "/assets/vendor/mermaid.min.js",
        "/assets/vendor/highlight.min.js",
        "/assets/vendor/highlight-github.min.css",
        "/assets/vendor/highlight-github-dark.min.css",
        "/assets/vendor/purify.min.js",
    ] {
        let response = http_request(address, "GET", asset);
        assert!(
            response.starts_with("HTTP/1.1 200 OK"),
            "asset failed: {asset}"
        );
    }

    let styles = http_request(address, "GET", "/assets/styles.css");
    assert!(response_body(&styles).contains("[hidden] { display: none !important; }"));

    let manifest = http_request(address, "GET", "/api/manifest");
    assert!(manifest.starts_with("HTTP/1.1 200 OK"));
    assert!(response_body(&manifest).contains("\"navigation\""));
    assert!(response_body(&manifest).contains("Platform"));

    let page = http_request(address, "GET", "/api/pages/overview.md");
    assert!(page.starts_with("HTTP/1.1 200 OK"));
    assert!(response_body(&page).contains("Repo overview"));

    let traversal = http_request(address, "GET", "/api/pages/%2e%2e%2fsecret.md");
    assert!(traversal.starts_with("HTTP/1.1 404 Not Found"));

    let method = http_request(address, "POST", "/");
    assert!(method.starts_with("HTTP/1.1 405 Method Not Allowed"));
    assert!(method.contains("Allow: GET, HEAD"));

    let head = http_request(address, "HEAD", "/api/pages/overview.md");
    assert!(head.starts_with("HTTP/1.1 200 OK"));
    assert!(response_body(&head).is_empty());

    child.kill().expect("stop reader");
    child.wait().expect("wait for reader");
}
