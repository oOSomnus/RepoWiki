//! A standalone, read-only WebUI for generated `.repowiki` directories.
//!
//! The public interface is intentionally small: callers provide a wiki
//! directory and a couple of launch options, while this module owns manifest
//! construction, page-path validation, HTTP routing, and browser launch.

use crate::docs::module_page_filename;
use crate::model::ModuleTree;
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReaderManifest {
    pub title: String,
    pub navigation: Vec<NavigationNode>,
    pub pages: Vec<PageDescriptor>,
    pub info: ReaderInfo,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NavigationNode {
    pub name: String,
    pub filename: String,
    pub available: bool,
    pub children: Vec<NavigationNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageDescriptor {
    pub filename: String,
    pub title: String,
    pub path: Vec<String>,
    pub available: bool,
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
}

impl WikiReader {
    pub fn open(wiki_dir: &Path) -> Result<Self> {
        let root = wiki_dir
            .canonicalize()
            .with_context(|| format!("wiki directory does not exist: {}", wiki_dir.display()))?;
        if !root.is_dir() {
            return Err(anyhow!("wiki path is not a directory: {}", root.display()));
        }
        if !has_markdown_page(&root)? {
            return Err(anyhow!(
                "wiki directory contains no top-level Markdown pages: {}",
                root.display()
            ));
        }
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn manifest(&self) -> Result<ReaderManifest> {
        build_manifest(&self.root)
    }

    pub fn read_page(&self, filename: &str) -> Result<String> {
        let path = self.safe_page_path(filename)?;
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

    fn safe_page_path(&self, filename: &str) -> Result<PathBuf> {
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

        let candidate = self.root.join(relative);
        let canonical = candidate
            .canonicalize()
            .with_context(|| format!("wiki page not found: {filename}"))?;
        if canonical.parent() != Some(self.root.as_path()) {
            return Err(anyhow!(
                "wiki page escapes the selected directory: {filename}"
            ));
        }
        Ok(canonical)
    }
}

pub fn load_manifest(wiki_dir: &Path) -> Result<ReaderManifest> {
    WikiReader::open(wiki_dir)?.manifest()
}

pub fn run(config: ReaderConfig) -> Result<()> {
    let reader = Arc::new(WikiReader::open(&config.wiki_dir)?);
    let initial_manifest = reader.manifest()?;
    for warning in &initial_manifest.warnings {
        eprintln!("warning: {warning}");
    }

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

fn build_manifest(root: &Path) -> Result<ReaderManifest> {
    let mut warnings = Vec::new();
    let metadata = read_optional_json(root, "metadata.json", &mut warnings);
    let tree = read_tree(root, &mut warnings);
    let title = repository_title(root, metadata.as_ref());
    let info = reader_info(metadata.as_ref());

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
                available: true,
            },
        );
    } else {
        warnings.push("overview.md is missing; the first available page will be used".to_string());
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
            &mut warnings,
        ));
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
                available: true,
            },
        );
    }

    let pages = page_order
        .into_iter()
        .filter_map(|filename| pages.remove(&filename))
        .collect();

    Ok(ReaderManifest {
        title,
        navigation,
        pages,
        info,
        warnings,
    })
}

fn build_navigation_node(
    root: &Path,
    name: &str,
    module: &crate::model::Module,
    parent_path: &[String],
    pages: &mut BTreeMap<String, PageDescriptor>,
    page_order: &mut Vec<String>,
    warnings: &mut Vec<String>,
) -> NavigationNode {
    let filename = module_page_filename(name);
    let available = safe_page_path(root, &filename).is_ok();
    if !available {
        warnings.push(format!("module page is missing or unsafe: {filename}"));
    }

    let mut path = parent_path.to_owned();
    path.push(name.to_string());
    add_page(
        pages,
        page_order,
        PageDescriptor {
            filename: filename.clone(),
            title: name.to_string(),
            path: path.clone(),
            available,
        },
    );

    let mut children = Vec::new();
    for (child_name, child) in &module.children {
        children.push(build_navigation_node(
            root, child_name, child, &path, pages, page_order, warnings,
        ));
    }

    NavigationNode {
        name: name.to_string(),
        filename,
        available,
        children,
    }
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

fn read_tree(root: &Path, warnings: &mut Vec<String>) -> ModuleTree {
    let path = root.join("module_tree.json");
    if !path.is_file() {
        warnings.push("module_tree.json is missing; using a flat page list".to_string());
        return ModuleTree::new();
    }
    match fs::read_to_string(&path)
        .with_context(|| format!("read {}", path.display()))
        .and_then(|contents| {
            serde_json::from_str(&contents).with_context(|| format!("parse {}", path.display()))
        }) {
        Ok(tree) => tree,
        Err(error) => {
            warnings.push(format!("could not load module_tree.json: {error:#}"));
            ModuleTree::new()
        }
    }
}

fn read_optional_json(root: &Path, filename: &str, warnings: &mut Vec<String>) -> Option<Value> {
    let path = root.join(filename);
    if !path.is_file() {
        return None;
    }
    match fs::read_to_string(&path)
        .with_context(|| format!("read {filename}"))
        .and_then(|contents| {
            serde_json::from_str(&contents).with_context(|| format!("parse {filename}"))
        }) {
        Ok(value) => Some(value),
        Err(error) => {
            warnings.push(format!("could not load {filename}: {error:#}"));
            None
        }
    }
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

fn has_markdown_page(root: &Path) -> Result<bool> {
    Ok(!top_level_markdown_files(root)?.is_empty())
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
        _ if path.starts_with("/api/pages/") => {
            let filename = path.trim_start_matches("/api/pages/");
            if filename.is_empty() {
                return HttpResponse::error(400, "Bad Request", "page filename is required");
            }
            match reader.read_page(filename) {
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
