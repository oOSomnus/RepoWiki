//! A standalone, read-only WebUI for generated `.repowiki` directories.
//!
//! The public interface is intentionally small: callers provide a wiki
//! directory and a couple of launch options, while this module owns manifest
//! construction, page-path validation, HTTP routing, and browser launch.

use crate::docs::{collect_expected_pages, module_page_filename, validate_module_page_paths};
use crate::model::ModuleTree;
use crate::session;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const MAX_REQUEST_BYTES: usize = 64 * 1024;
const MAX_PAGE_BYTES: u64 = 32 * 1024 * 1024;

const INDEX_HTML: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/reader/static/index.html"
));
const APP_JS: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/reader/static/app.js"));
const STYLES_CSS: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/reader/static/styles.css"
));
const MARKED_JS: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/reader/static/vendor/marked.min.js"
));
const MARKED_HEADING_ID_JS: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/reader/static/vendor/marked-gfm-heading-id.min.js"
));
const MERMAID_JS: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/reader/static/vendor/mermaid.min.js"
));
const HIGHLIGHT_JS: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/reader/static/vendor/highlight.min.js"
));
const HIGHLIGHT_LIGHT_CSS: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/reader/static/vendor/highlight-github.min.css"
));
const HIGHLIGHT_DARK_CSS: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/reader/static/vendor/highlight-github-dark.min.css"
));
const PURIFY_JS: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/reader/static/vendor/purify.min.js"
));

#[derive(Debug, Clone)]
pub struct ReaderConfig {
    pub wiki_dir: PathBuf,
    pub port: u16,
    pub open_browser: bool,
}

impl ReaderConfig {
    pub fn new(wiki_dir: impl Into<PathBuf>) -> Self {
        Self {
            wiki_dir: wiki_dir.into(),
            port: 0,
            open_browser: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ReaderEditionKind {
    Repository,
    Change,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReaderEdition {
    pub id: String,
    pub kind: ReaderEditionKind,
    pub label: String,
    pub title: String,
    pub navigation: Vec<NavigationNode>,
    pub pages: Vec<PageDescriptor>,
    pub info: ReaderInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReaderCatalog {
    pub title: String,
    pub default_edition: String,
    pub editions: Vec<ReaderEdition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavigationNode {
    pub name: String,
    pub filename: String,
    pub children: Vec<NavigationNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageDescriptor {
    pub filename: String,
    pub title: String,
    pub path: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReaderInfo {
    pub generated_at: Option<String>,
    pub model: Option<String>,
    pub commit: Option<String>,
    pub total_components: Option<usize>,
    pub module_count: Option<usize>,
    pub leaf_count: Option<usize>,
}

/// The filesystem-backed adapter behind the Reader seam.
#[derive(Debug, Clone)]
pub struct WikiReader {
    root: PathBuf,
    catalog: ReaderCatalog,
    edition_roots: BTreeMap<String, PathBuf>,
}

impl WikiReader {
    pub fn open(wiki_dir: &Path) -> Result<Self> {
        let root = wiki_dir
            .canonicalize()
            .with_context(|| format!("wiki directory does not exist: {}", wiki_dir.display()))?;
        if !root.is_dir() {
            return Err(anyhow!("wiki path is not a directory: {}", root.display()));
        }

        let mut editions = Vec::new();
        let mut edition_roots = BTreeMap::new();
        if has_bundle_marker(&root)? {
            let edition = build_edition(&root, "repository", ReaderEditionKind::Repository, None)
                .context("invalid repository edition")?;
            edition_roots.insert(edition.id.clone(), root.clone());
            editions.push(edition);
        }

        if let Some(changes_root) = canonical_changes_directory(&root)? {
            let mut change_directories = Vec::new();
            for entry in fs::read_dir(&changes_root)? {
                let entry = entry?;
                let file_name = entry.file_name();
                let Some(id) = file_name.to_str().map(str::to_owned) else {
                    continue;
                };
                if !is_valid_change_id(&id) {
                    continue;
                }

                let file_type = entry.file_type()?;
                if file_type.is_symlink() {
                    return Err(anyhow!(
                        "change edition must not be a symlink: {}",
                        entry.path().display()
                    ));
                }
                if !file_type.is_dir() {
                    return Err(anyhow!(
                        "change edition must be a directory: {}",
                        entry.path().display()
                    ));
                }

                let edition_root = entry.path().canonicalize().with_context(|| {
                    format!(
                        "canonicalize change edition directory '{}'",
                        entry.path().display()
                    )
                })?;
                if edition_root.parent() != Some(changes_root.as_path())
                    || !edition_root.starts_with(&root)
                {
                    return Err(anyhow!(
                        "change edition directory escapes its catalog root: {}",
                        entry.path().display()
                    ));
                }
                change_directories.push((id, edition_root));
            }
            change_directories.sort_by(|left, right| left.0.cmp(&right.0));

            for (id, edition_root) in change_directories {
                let (base, head) = id
                    .split_once("..")
                    .expect("validated change edition id has a range separator");
                let label = format!("{}..{}", &base[..8], &head[..8]);
                let edition =
                    build_edition(&edition_root, &id, ReaderEditionKind::Change, Some(label))
                        .with_context(|| format!("invalid change edition '{id}'"))?;
                edition_roots.insert(id, edition_root);
                editions.push(edition);
            }
        }

        if editions.is_empty() {
            return Err(anyhow!(
                "no valid wiki editions found in {}",
                root.display()
            ));
        }

        let repository = editions
            .iter()
            .find(|edition| edition.kind == ReaderEditionKind::Repository);
        let title = repository
            .map(|edition| edition.title.clone())
            .unwrap_or_else(|| editions[0].title.clone());
        let default_edition = repository
            .map(|edition| edition.id.clone())
            .unwrap_or_else(|| editions[0].id.clone());
        let catalog = ReaderCatalog {
            title,
            default_edition,
            editions,
        };

        Ok(Self {
            root,
            catalog,
            edition_roots,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn manifest(&self) -> Result<ReaderCatalog> {
        Ok(self.catalog.clone())
    }

    pub fn read_page(&self, edition_id: &str, filename: &str) -> Result<String> {
        let edition_root = self
            .edition_roots
            .get(edition_id)
            .ok_or_else(|| anyhow!("unknown wiki edition: {edition_id}"))?;
        let path = safe_page_path(edition_root, filename)?;
        let size = fs::metadata(&path)?.len();
        if size > MAX_PAGE_BYTES {
            return Err(anyhow!(
                "page is larger than {} MiB: {}",
                MAX_PAGE_BYTES / (1024 * 1024),
                filename
            ));
        }
        fs::read_to_string(&path).with_context(|| format!("read wiki page {filename}"))
    }
}

pub fn load_manifest(wiki_dir: &Path) -> Result<ReaderCatalog> {
    WikiReader::open(wiki_dir)?.manifest()
}

pub fn run(config: ReaderConfig) -> Result<()> {
    let reader = Arc::new(WikiReader::open(&config.wiki_dir)?);
    let initial_manifest = reader.manifest()?;

    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, config.port))
        .with_context(|| format!("bind local reader port {}", config.port))?;
    let address = listener.local_addr()?;
    let url = format!("http://{address}/");

    println!("RepoWiki Reader: {}", initial_manifest.title);
    println!("Listening on {url}");
    println!("Press Ctrl-C to stop.");
    io::stdout().flush()?;

    if config.open_browser {
        if let Err(error) = open::that(&url) {
            eprintln!("warning: could not open the default browser: {error}");
        }
    }

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let reader = Arc::clone(&reader);
                thread::spawn(move || {
                    if let Err(error) = handle_connection(stream, &reader) {
                        eprintln!("reader request error: {error:#}");
                    }
                });
            }
            Err(error) => eprintln!("reader connection error: {error}"),
        }
    }
    Ok(())
}

fn has_bundle_marker(root: &Path) -> Result<bool> {
    for marker in ["metadata.json", "module_tree.json", "overview.md"] {
        match fs::symlink_metadata(root.join(marker)) {
            Ok(_) => return Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).with_context(|| format!("inspect {marker}")),
        }
    }
    Ok(false)
}

fn canonical_changes_directory(root: &Path) -> Result<Option<PathBuf>> {
    let path = root.join("changes");
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| "inspect changes directory"),
        Ok(_) => {}
    }

    let canonical = path
        .canonicalize()
        .with_context(|| format!("canonicalize changes directory: {}", path.display()))?;
    if !canonical.is_dir() || canonical.as_path() == root || !canonical.starts_with(root) {
        return Err(anyhow!(
            "changes directory escapes its wiki root: {}",
            path.display()
        ));
    }
    Ok(Some(canonical))
}

fn is_valid_change_id(id: &str) -> bool {
    let Some((base, head)) = id.split_once("..") else {
        return false;
    };
    matches!(base.len(), 40 | 64)
        && head.len() == base.len()
        && base.bytes().all(|byte| byte.is_ascii_hexdigit())
        && head.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn build_edition(
    root: &Path,
    id: &str,
    kind: ReaderEditionKind,
    label: Option<String>,
) -> Result<ReaderEdition> {
    let (metadata, tree) = read_current_output(root)?;
    let title = repository_title(root, Some(&metadata));
    let label = label.unwrap_or_else(|| title.clone());
    let info = reader_info(Some(&metadata));

    let mut pages = BTreeMap::new();
    let mut page_order = Vec::new();
    if root.join("overview.md").is_file() {
        add_page(
            &mut pages,
            &mut page_order,
            PageDescriptor {
                filename: "overview.md".to_string(),
                title: "Overview".to_string(),
                path: Vec::new(),
            },
        );
    }

    let mut navigation = Vec::new();
    for (name, module) in &tree {
        navigation.push(build_navigation_node(
            root,
            name,
            module,
            &[],
            &mut pages,
            &mut page_order,
        )?);
    }

    for filename in top_level_markdown_files(root)? {
        if pages.contains_key(&filename) {
            continue;
        }
        add_page(
            &mut pages,
            &mut page_order,
            PageDescriptor {
                title: page_title(&filename),
                filename,
                path: Vec::new(),
            },
        );
    }

    let pages = page_order
        .into_iter()
        .filter_map(|filename| pages.remove(&filename))
        .collect();

    Ok(ReaderEdition {
        id: id.to_string(),
        kind,
        label,
        title,
        navigation,
        pages,
        info,
    })
}

fn build_navigation_node(
    root: &Path,
    name: &str,
    module: &crate::model::Module,
    parent_path: &[String],
    pages: &mut BTreeMap<String, PageDescriptor>,
    page_order: &mut Vec<String>,
) -> Result<NavigationNode> {
    let filename = module_page_filename(name)
        .with_context(|| format!("build page path for module '{name}'"))?;
    safe_page_path(root, &filename)
        .with_context(|| format!("module page is missing or unsafe: {filename}"))?;

    let mut path = parent_path.to_owned();
    path.push(name.to_string());
    add_page(
        pages,
        page_order,
        PageDescriptor {
            filename: filename.clone(),
            title: name.to_string(),
            path: path.clone(),
        },
    );

    let mut children = Vec::new();
    for (child_name, child) in &module.children {
        children.push(build_navigation_node(
            root, child_name, child, &path, pages, page_order,
        )?);
    }

    Ok(NavigationNode {
        name: name.to_string(),
        filename,
        children,
    })
}

fn add_page(
    pages: &mut BTreeMap<String, PageDescriptor>,
    page_order: &mut Vec<String>,
    page: PageDescriptor,
) {
    if pages.contains_key(&page.filename) {
        return;
    }
    page_order.push(page.filename.clone());
    pages.insert(page.filename.clone(), page);
}

fn read_current_output(root: &Path) -> Result<(Value, ModuleTree)> {
    let metadata: Value =
        session::read_json(&root.join("metadata.json")).context("read current metadata.json")?;
    let tree: ModuleTree = session::read_json(&root.join("module_tree.json"))
        .context("read current module_tree.json")?;
    validate_module_page_paths(&tree).context("validate module_tree.json page paths")?;

    let mut expected = BTreeSet::from(["overview.md".to_string()]);
    collect_expected_pages(&tree, &mut expected)?;
    for filename in expected {
        safe_page_path(root, &filename)
            .with_context(|| format!("read required documentation page {filename}"))?;
    }
    Ok((metadata, tree))
}

fn reader_info(metadata: Option<&Value>) -> ReaderInfo {
    let Some(metadata) = metadata else {
        return ReaderInfo::default();
    };
    let generation = metadata.get("generation_info");
    let statistics = metadata.get("statistics");
    ReaderInfo {
        generated_at: string_field(generation, "timestamp"),
        model: string_field(generation, "main_model"),
        commit: string_field(generation, "commit_id").map(|value| value.chars().take(8).collect()),
        total_components: usize_field(statistics, "total_components"),
        module_count: usize_field(statistics, "module_count"),
        leaf_count: usize_field(statistics, "leaf_nodes"),
    }
}

fn repository_title(root: &Path, metadata: Option<&Value>) -> String {
    let metadata_title = metadata
        .and_then(|value| value.get("generation_info"))
        .and_then(|value| value.get("repo_path"))
        .and_then(Value::as_str)
        .and_then(|value| Path::new(value).file_name())
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty() && *value != ".repowiki");
    if let Some(title) = metadata_title {
        return title.to_string();
    }
    root.parent()
        .and_then(Path::file_name)
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("RepoWiki")
        .to_string()
}

fn string_field(value: Option<&Value>, name: &str) -> Option<String> {
    value
        .and_then(|value| value.get(name))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn usize_field(value: Option<&Value>, name: &str) -> Option<usize> {
    value
        .and_then(|value| value.get(name))
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
}

fn top_level_markdown_files(root: &Path) -> Result<Vec<String>> {
    let mut files = BTreeSet::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("md") {
            continue;
        }
        if let Some(filename) = path.file_name().and_then(|value| value.to_str()) {
            files.insert(filename.to_string());
        }
    }
    Ok(files.into_iter().collect())
}

fn safe_page_path(root: &Path, filename: &str) -> Result<PathBuf> {
    let relative = Path::new(filename);
    if filename.is_empty()
        || filename.contains('/')
        || filename.contains('\\')
        || relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
        || relative.extension().and_then(|value| value.to_str()) != Some("md")
    {
        return Err(anyhow!("unsafe wiki page path: {filename}"));
    }
    let candidate = root.join(relative);
    let canonical = candidate.canonicalize()?;
    if canonical.parent() != Some(root) {
        return Err(anyhow!("wiki page escapes the selected directory"));
    }
    Ok(canonical)
}

fn page_title(filename: &str) -> String {
    filename
        .strip_suffix(".md")
        .unwrap_or(filename)
        .replace('_', " ")
}

#[derive(Debug)]
struct HttpRequest {
    method: String,
    target: String,
}

#[derive(Debug)]
struct HttpResponse {
    status: u16,
    reason: &'static str,
    content_type: &'static str,
    body: Vec<u8>,
    cache_control: &'static str,
    allow: Option<&'static str>,
}

fn handle_connection(mut stream: TcpStream, reader: &WikiReader) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let request = match read_request(&mut stream) {
        Ok(request) => request,
        Err(error) => {
            write_response(
                &mut stream,
                &HttpResponse::error(400, "Bad Request", error.to_string()),
                false,
            )?;
            return Ok(());
        }
    };
    let head = request.method == "HEAD";
    let response = route_request(&request, reader);
    write_response(&mut stream, &response, head)
}

fn read_request(stream: &mut TcpStream) -> Result<HttpRequest> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Err(anyhow!("connection closed before request headers"));
        }
        buffer.extend_from_slice(&chunk[..read]);
        if buffer.windows(4).any(|window| window == b"\r\n\r\n") {
            break;
        }
        if buffer.len() > MAX_REQUEST_BYTES {
            return Err(anyhow!("request headers are too large"));
        }
    }

    let request = String::from_utf8(buffer).context("request headers are not UTF-8")?;
    let line = request
        .split_once("\r\n")
        .map(|(line, _)| line)
        .ok_or_else(|| anyhow!("malformed HTTP request"))?;
    let mut parts = line.split_whitespace();
    let method = parts.next().ok_or_else(|| anyhow!("missing HTTP method"))?;
    let target = parts.next().ok_or_else(|| anyhow!("missing HTTP target"))?;
    let version = parts
        .next()
        .ok_or_else(|| anyhow!("missing HTTP version"))?;
    if parts.next().is_some() || !version.starts_with("HTTP/") {
        return Err(anyhow!("malformed HTTP request line"));
    }
    Ok(HttpRequest {
        method: method.to_string(),
        target: target.to_string(),
    })
}

fn route_request(request: &HttpRequest, reader: &WikiReader) -> HttpResponse {
    if request.method != "GET" && request.method != "HEAD" {
        return HttpResponse::error_with_allow(
            405,
            "Method Not Allowed",
            "only GET and HEAD are supported",
        );
    }

    let target = request
        .target
        .split_once('?')
        .map_or(request.target.as_str(), |(path, _)| path);
    if !target.starts_with('/') {
        return HttpResponse::error(
            400,
            "Bad Request",
            "request target must be an absolute path",
        );
    }
    let path = match percent_decode(target) {
        Ok(path) => path,
        Err(error) => return HttpResponse::error(400, "Bad Request", error.to_string()),
    };

    match path.as_str() {
        "/" | "/index.html" => HttpResponse::asset(INDEX_HTML, "text/html; charset=utf-8"),
        "/assets/app.js" => HttpResponse::asset(APP_JS, "text/javascript; charset=utf-8"),
        "/assets/styles.css" => HttpResponse::asset(STYLES_CSS, "text/css; charset=utf-8"),
        "/assets/vendor/marked.min.js" => {
            HttpResponse::asset(MARKED_JS, "text/javascript; charset=utf-8")
        }
        "/assets/vendor/marked-gfm-heading-id.min.js" => {
            HttpResponse::asset(MARKED_HEADING_ID_JS, "text/javascript; charset=utf-8")
        }
        "/assets/vendor/mermaid.min.js" => {
            HttpResponse::asset(MERMAID_JS, "text/javascript; charset=utf-8")
        }
        "/assets/vendor/highlight.min.js" => {
            HttpResponse::asset(HIGHLIGHT_JS, "text/javascript; charset=utf-8")
        }
        "/assets/vendor/highlight-github.min.css" => {
            HttpResponse::asset(HIGHLIGHT_LIGHT_CSS, "text/css; charset=utf-8")
        }
        "/assets/vendor/highlight-github-dark.min.css" => {
            HttpResponse::asset(HIGHLIGHT_DARK_CSS, "text/css; charset=utf-8")
        }
        "/assets/vendor/purify.min.js" => {
            HttpResponse::asset(PURIFY_JS, "text/javascript; charset=utf-8")
        }
        "/api/manifest" => match reader
            .manifest()
            .and_then(|manifest| serde_json::to_vec(&manifest).context("serialize reader manifest"))
        {
            Ok(body) => HttpResponse::json(body),
            Err(error) => HttpResponse::error(500, "Internal Server Error", error.to_string()),
        },
        _ if path.starts_with("/api/editions/") => {
            let route = path.trim_start_matches("/api/editions/");
            let Some((edition_id, filename)) = route.split_once("/pages/") else {
                return HttpResponse::error(404, "Not Found", "resource not found");
            };
            match reader.read_page(edition_id, filename) {
                Ok(contents) => HttpResponse::text(contents),
                Err(error) => HttpResponse::error(404, "Not Found", error.to_string()),
            }
        }
        _ => HttpResponse::error(404, "Not Found", "resource not found"),
    }
}

impl HttpResponse {
    fn asset(body: &'static [u8], content_type: &'static str) -> Self {
        Self {
            status: 200,
            reason: "OK",
            content_type,
            body: body.to_vec(),
            cache_control: "public, max-age=3600",
            allow: None,
        }
    }

    fn json(body: Vec<u8>) -> Self {
        Self {
            status: 200,
            reason: "OK",
            content_type: "application/json; charset=utf-8",
            body,
            cache_control: "no-store",
            allow: None,
        }
    }

    fn text(body: String) -> Self {
        Self {
            status: 200,
            reason: "OK",
            content_type: "text/markdown; charset=utf-8",
            body: body.into_bytes(),
            cache_control: "no-store",
            allow: None,
        }
    }

    fn error(status: u16, reason: &'static str, message: impl Into<String>) -> Self {
        let body = json!({"error": message.into()}).to_string().into_bytes();
        Self {
            status,
            reason,
            content_type: "application/json; charset=utf-8",
            body,
            cache_control: "no-store",
            allow: None,
        }
    }

    fn error_with_allow(status: u16, reason: &'static str, message: impl Into<String>) -> Self {
        let mut response = Self::error(status, reason, message);
        response.allow = Some("GET, HEAD");
        response
    }
}

fn write_response(stream: &mut TcpStream, response: &HttpResponse, head: bool) -> Result<()> {
    let headers = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: {}\r\n{}Connection: close\r\nX-Content-Type-Options: nosniff\r\nContent-Security-Policy: default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; connect-src 'self'\r\n\r\n",
        response.status,
        response.reason,
        response.content_type,
        response.body.len(),
        response.cache_control,
        response
            .allow
            .map_or(String::new(), |allow| format!("Allow: {allow}\r\n")),
    );
    stream.write_all(headers.as_bytes())?;
    if !head {
        stream.write_all(&response.body)?;
    }
    stream.flush().map_err(Into::into)
}

fn percent_decode(input: &str) -> Result<String> {
    let bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let byte = if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(anyhow!("incomplete percent escape"));
            }
            let high = hex_value(bytes[index + 1])?;
            let low = hex_value(bytes[index + 2])?;
            index += 3;
            (high << 4) | low
        } else {
            let byte = bytes[index];
            index += 1;
            byte
        };
        if byte == 0 || byte < 0x20 || byte == 0x7f {
            return Err(anyhow!("control character in request path"));
        }
        decoded.push(byte);
    }
    String::from_utf8(decoded).context("request path is not valid UTF-8")
}

fn hex_value(value: u8) -> Result<u8> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => Err(anyhow!("invalid percent escape")),
    }
}

#[cfg(test)]
mod tests {
    use super::{page_title, percent_decode};

    #[test]
    fn decodes_utf8_and_rejects_bad_escapes() {
        assert_eq!(percent_decode("/A%26B.md").unwrap(), "/A&B.md");
        assert_eq!(percent_decode("/%E4%B8%AD.md").unwrap(), "/中.md");
        assert!(percent_decode("/%ZZ.md").is_err());
    }

    #[test]
    fn derives_extra_page_titles() {
        assert_eq!(page_title("Some_Page.md"), "Some Page");
        assert_eq!(page_title("guide.md"), "guide");
    }
}
