//! A read-only DokuWiki runtime for generated `.repowiki` directories.
//!
//! The reader validates every edition before starting PHP, merges each edition's
//! canonical page namespace into an isolated temporary data root, then serves
//! the pinned DokuWiki application through RepoWiki's restricted router.

use crate::{docs, dokuwiki, model::ModuleTree, session};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;

const CATALOG_ACTION: &str = "/doku.php?do=repowiki_catalog";
const MAX_CATALOG_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_millis(500);
const SOCKET_READ_TIMEOUT: Duration = Duration::from_secs(2);
const READINESS_TIMEOUT: Duration = Duration::from_secs(15);
const READINESS_RETRY_INTERVAL: Duration = Duration::from_millis(100);
const SERVER_POLL_INTERVAL: Duration = Duration::from_millis(100);
const EPHEMERAL_PORT_ATTEMPTS: usize = 8;

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

/// The JSON contract consumed by the native RepoWiki DokuWiki plugin.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReaderCatalog {
    pub title: String,
    pub default_edition: String,
    pub editions: Vec<ReaderEdition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReaderEdition {
    pub id: String,
    pub wiki_id: String,
    pub title: String,
    pub label: String,
    pub start_id: String,
    pub tree: Vec<NavigationNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct NavigationNode {
    pub page_id: String,
    pub title: String,
    pub children: Vec<NavigationNode>,
}

struct EditionSource {
    catalog_entry: ReaderEdition,
    output_dir: PathBuf,
    page_ids: BTreeSet<String>,
}

struct LoadedCatalog {
    catalog: ReaderCatalog,
    editions: Vec<EditionSource>,
}

impl LoadedCatalog {
    fn open(wiki_dir: &Path) -> Result<Self> {
        let root = wiki_dir
            .canonicalize()
            .with_context(|| format!("wiki directory does not exist: {}", wiki_dir.display()))?;
        if !root.is_dir() {
            return Err(anyhow!("wiki path is not a directory: {}", root.display()));
        }

        let mut editions = Vec::new();
        let mut namespaces = BTreeSet::new();
        if has_repository_markers(&root)? {
            namespaces.insert(session::REPOSITORY_WIKI_ID.to_string());
            editions.push(
                build_edition(
                    &root,
                    "repository",
                    session::REPOSITORY_WIKI_ID,
                    ReaderEditionKind::Repository,
                    None,
                )
                .context("invalid repository edition")?,
            );
        }

        if let Some(changes_root) = canonical_changes_directory(&root)? {
            let mut change_directories = Vec::new();
            for entry in fs::read_dir(&changes_root)? {
                let entry = entry?;
                let Some(id) = entry.file_name().to_str().map(str::to_owned) else {
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
                let wiki_id = session::change_wiki_id(&id)
                    .with_context(|| format!("invalid change edition '{id}'"))?;
                if !namespaces.insert(wiki_id.clone()) {
                    return Err(anyhow!(
                        "change editions map to duplicate DokuWiki namespace '{wiki_id}'"
                    ));
                }
                let (base, head) = id
                    .split_once("..")
                    .expect("validated change edition ID has a range separator");
                let label = format!("{}..{}", &base[..8], &head[..8]);
                editions.push(
                    build_edition(
                        &edition_root,
                        &id,
                        &wiki_id,
                        ReaderEditionKind::Change,
                        Some(label),
                    )
                    .with_context(|| format!("invalid change edition '{id}'"))?,
                );
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
            .find(|edition| edition.catalog_entry.id == "repository");
        let title = repository
            .map(|edition| edition.catalog_entry.title.clone())
            .unwrap_or_else(|| editions[0].catalog_entry.title.clone());
        let default_edition = repository
            .map(|edition| edition.catalog_entry.id.clone())
            .unwrap_or_else(|| editions[0].catalog_entry.id.clone());
        let catalog = ReaderCatalog {
            title,
            default_edition,
            editions: editions
                .iter()
                .map(|edition| edition.catalog_entry.clone())
                .collect(),
        };

        Ok(Self { catalog, editions })
    }
}

#[derive(Clone, Copy)]
enum ReaderEditionKind {
    Repository,
    Change,
}

pub fn load_manifest(wiki_dir: &Path) -> Result<ReaderCatalog> {
    Ok(LoadedCatalog::open(wiki_dir)?.catalog)
}

pub fn run(config: ReaderConfig) -> Result<()> {
    let loaded = LoadedCatalog::open(&config.wiki_dir)?;
    let _signals = SignalGuard::install()?;
    let runtime = dokuwiki::discover_runtime()?;
    dokuwiki::ensure_php_82(&runtime)?;
    if shutdown_requested() {
        return Ok(());
    }
    let temporary = TemporaryRuntime::create()?;
    let context = prepare_isolated_runtime(&loaded, runtime, &temporary)?;
    if shutdown_requested() {
        return Ok(());
    }

    let attempts = if config.port == 0 {
        EPHEMERAL_PORT_ATTEMPTS
    } else {
        1
    };
    let mut last_start_error = None;
    for attempt in 0..attempts {
        let port = if config.port == 0 {
            select_ephemeral_port()?
        } else {
            config.port
        };
        let mut server = spawn_server(&context, port)?;
        match wait_until_ready(&mut server.child, port, &loaded.catalog) {
            Ok(()) => {
                let url = format!("http://127.0.0.1:{port}/");
                println!("RepoWiki Reader: {}", loaded.catalog.title);
                println!("Listening on {}", url.trim_end_matches('/'));
                println!("Press Ctrl-C to stop.");
                io::stdout().flush()?;
                if config.open_browser {
                    if let Err(error) = open::that(&url) {
                        eprintln!("warning: could not open the default browser: {error}");
                    }
                }
                return supervise_server(&mut server);
            }
            Err(error) => {
                let exited = server.child.try_wait().ok().flatten().is_some();
                server.stop();
                if config.port == 0 && exited && attempt + 1 < attempts {
                    last_start_error = Some(error);
                    continue;
                }
                return Err(error);
            }
        }
    }

    Err(last_start_error.unwrap_or_else(|| anyhow!("could not start the DokuWiki server")))
}

fn has_repository_markers(root: &Path) -> Result<bool> {
    for marker in ["metadata.json", "module_tree.json", "dokuwiki"] {
        match fs::symlink_metadata(root.join(marker)) {
            Ok(_) => return Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).with_context(|| format!("inspect {marker}")),
        }
    }
    Ok(false)
}

fn canonical_changes_directory(root: &Path) -> Result<Option<PathBuf>> {
    let path = root.join("changes");
    let metadata = match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| "inspect changes directory"),
        Ok(metadata) => metadata,
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(anyhow!(
            "changes directory must be a real directory inside the wiki root: {}",
            path.display()
        ));
    }

    let canonical = path
        .canonicalize()
        .with_context(|| format!("canonicalize changes directory: {}", path.display()))?;
    if canonical.parent() != Some(root) || !canonical.starts_with(root) {
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
    output_dir: &Path,
    id: &str,
    wiki_id: &str,
    kind: ReaderEditionKind,
    label: Option<String>,
) -> Result<EditionSource> {
    let (metadata, tree, page_ids) = read_current_output(output_dir, wiki_id)?;
    let title = edition_title(output_dir, &metadata);
    let label = match kind {
        ReaderEditionKind::Repository => title.clone(),
        ReaderEditionKind::Change => label.unwrap_or_else(|| title.clone()),
    };
    let start_id = docs::overview_page_id(wiki_id)?;
    let navigation = build_navigation_tree(wiki_id, &tree, &[])?;

    Ok(EditionSource {
        catalog_entry: ReaderEdition {
            id: id.to_string(),
            wiki_id: wiki_id.to_string(),
            title,
            label,
            start_id,
            tree: navigation,
        },
        output_dir: output_dir.to_path_buf(),
        page_ids,
    })
}

fn read_current_output(
    output_dir: &Path,
    expected_wiki_id: &str,
) -> Result<(Value, ModuleTree, BTreeSet<String>)> {
    let metadata_path = output_dir.join("metadata.json");
    let tree_path = output_dir.join("module_tree.json");
    require_regular_file(&metadata_path).context("metadata.json is missing or unsafe")?;
    require_regular_file(&tree_path).context("module_tree.json is missing or unsafe")?;

    let metadata: Value = session::read_json(&metadata_path).context("read metadata.json")?;
    let actual_wiki_id = metadata.get("wiki_id").and_then(Value::as_str);
    if actual_wiki_id != Some(expected_wiki_id) {
        return Err(anyhow!(
            "metadata wiki_id must be '{expected_wiki_id}', found {}",
            actual_wiki_id
                .map(|value| format!("'{value}'"))
                .unwrap_or_else(|| "a missing or non-string value".to_string())
        ));
    }

    let tree: ModuleTree = session::read_json(&tree_path).context("read module_tree.json")?;
    docs::validate_module_page_paths(expected_wiki_id, &tree)
        .context("validate module_tree.json page IDs")?;

    let start_id = docs::overview_page_id(expected_wiki_id)?;
    let mut page_ids = BTreeSet::from([start_id]);
    docs::collect_expected_pages(expected_wiki_id, &tree, &mut page_ids)
        .context("collect canonical module page IDs")?;
    for page_id in &page_ids {
        let path = docs::page_file_path(output_dir, expected_wiki_id, page_id)
            .with_context(|| format!("map canonical page ID '{page_id}'"))?;
        validate_page_file(output_dir, &path, page_id)
            .with_context(|| format!("read required documentation page '{page_id}'"))?;
    }

    Ok((metadata, tree, page_ids))
}

fn require_regular_file(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("inspect required file {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(anyhow!(
            "required path is not a regular file: {}",
            path.display()
        ));
    }
    Ok(())
}

fn validate_page_file(output_dir: &Path, path: &Path, page_id: &str) -> Result<()> {
    if path.extension().and_then(|value| value.to_str()) != Some("txt") {
        return Err(anyhow!(
            "canonical page ID '{page_id}' did not map to a .txt page"
        ));
    }
    let relative = path
        .strip_prefix(output_dir)
        .with_context(|| format!("page path for '{page_id}' escaped its edition"))?;
    let component_count = relative.components().count();
    let mut current = output_dir.to_path_buf();
    for (index, component) in relative.components().enumerate() {
        let Component::Normal(name) = component else {
            return Err(anyhow!("unsafe page path for canonical ID '{page_id}'"));
        };
        current.push(name);
        let metadata = fs::symlink_metadata(&current).with_context(|| {
            format!(
                "required page for canonical ID '{page_id}' is missing: {}",
                current.display()
            )
        })?;
        if metadata.file_type().is_symlink() {
            return Err(anyhow!(
                "page path for canonical ID '{page_id}' contains a symlink: {}",
                current.display()
            ));
        }
        let is_last = index + 1 == component_count;
        if (is_last && !metadata.is_file()) || (!is_last && !metadata.is_dir()) {
            return Err(anyhow!(
                "page path for canonical ID '{page_id}' has an invalid component: {}",
                current.display()
            ));
        }
    }
    Ok(())
}

fn build_navigation_tree(
    wiki_id: &str,
    tree: &ModuleTree,
    parent_path: &[String],
) -> Result<Vec<NavigationNode>> {
    tree.iter()
        .map(|(title, module)| {
            let mut path = parent_path.to_vec();
            path.push(title.clone());
            Ok(NavigationNode {
                page_id: docs::module_page_id(wiki_id, &path)?,
                title: title.clone(),
                children: build_navigation_tree(wiki_id, &module.children, &path)?,
            })
        })
        .collect()
}

fn edition_title(output_dir: &Path, metadata: &Value) -> String {
    let metadata_title = metadata
        .get("generation_info")
        .and_then(|value| value.get("repo_path"))
        .and_then(Value::as_str)
        .and_then(|value| Path::new(value).file_name())
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty() && *value != ".repowiki");
    if let Some(title) = metadata_title {
        return title.to_string();
    }
    output_dir
        .parent()
        .and_then(Path::file_name)
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("RepoWiki")
        .to_string()
}

fn prepare_isolated_runtime(
    loaded: &LoadedCatalog,
    runtime: dokuwiki::RuntimePaths,
    temporary: &TemporaryRuntime,
) -> Result<dokuwiki::WikiContext> {
    let savedir = temporary.path.join("dokuwiki/data");
    let context = dokuwiki::prepare_context(dokuwiki::WikiContextConfig {
        runtime,
        config_dir: temporary.path.join("conf"),
        plugin_dir: temporary.path.join("plugins"),
        savedir: savedir.clone(),
        workspace: temporary.path.clone(),
        wiki_id: session::REPOSITORY_WIKI_ID.to_string(),
        title: loaded.catalog.title.clone(),
        read_only: true,
    })?;

    for edition in &loaded.editions {
        merge_edition_namespace(edition, &temporary.path)?;
    }
    write_navigation_sidebar(&temporary.path)?;
    rebuild_runtime_search_index(&context)?;

    let catalog_path = savedir.join("repowiki_catalog.json");
    fs::write(&catalog_path, serde_json::to_vec(&loaded.catalog)?)
        .with_context(|| format!("write native DokuWiki catalog {}", catalog_path.display()))?;
    Ok(context)
}

/// Write the root sidebar page that hosts the catalog navigation.
///
/// The bundled default template renders its aside whenever the nearest
/// `sidebar` page exists, so every edition namespace picks this one up.
fn write_navigation_sidebar(runtime_root: &Path) -> Result<()> {
    const SIDEBAR_PAGE_CONTENT: &str = "~~REPOWIKI_NAV~~\n";

    let page = runtime_root.join("dokuwiki/data/pages/sidebar.txt");
    let parent = page
        .parent()
        .ok_or_else(|| anyhow!("sidebar page has no parent directory"))?;
    fs::create_dir_all(parent)?;
    fs::write(&page, SIDEBAR_PAGE_CONTENT)
        .with_context(|| format!("write DokuWiki sidebar page {}", page.display()))?;
    Ok(())
}

fn rebuild_runtime_search_index(context: &dokuwiki::WikiContext) -> Result<()> {
    const INDEXER_BOOTSTRAP: &str = concat!(
        "require getenv('REPOWIKI_DOKUWIKI_INTEGRATION_DIR') . '/bootstrap.php';",
        "repowiki_dokuwiki_define_paths();",
        "$argv = [DOKU_INC . 'bin/indexer.php', '--clear', '--loglevel=error'];",
        "require DOKU_INC . 'bin/indexer.php';"
    );

    let mut command = Command::new(&context.runtime.php);
    command
        .arg("-r")
        .arg(INDEXER_BOOTSTRAP)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env(
            "REPOWIKI_DOKUWIKI_INTEGRATION_DIR",
            &context.runtime.integration_dir,
        );
    dokuwiki::configure_process(&mut command, context);
    let output = command
        .output()
        .context("rebuild the isolated DokuWiki search index")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() || !stderr.trim().is_empty() || stdout.contains("indexing error") {
        return Err(anyhow!(
            "could not rebuild the isolated DokuWiki search index ({}): {}{}",
            output.status,
            stdout.trim(),
            stderr.trim()
        ));
    }
    Ok(())
}

fn merge_edition_namespace(edition: &EditionSource, runtime_root: &Path) -> Result<()> {
    let wiki_id = &edition.catalog_entry.wiki_id;
    for page_id in &edition.page_ids {
        let source = docs::page_file_path(&edition.output_dir, wiki_id, page_id)?;
        let destination = docs::page_file_path(runtime_root, wiki_id, page_id)?;
        validate_page_file(&edition.output_dir, &source, page_id)?;
        let parent = destination
            .parent()
            .ok_or_else(|| anyhow!("runtime page has no parent directory"))?;
        fs::create_dir_all(parent)?;
        fs::copy(&source, &destination).with_context(|| {
            format!("copy canonical page '{page_id}' into the isolated runtime")
        })?;
    }
    for storage in ["attic", "meta"] {
        copy_edition_storage(edition, runtime_root, storage)?;
    }
    Ok(())
}

fn copy_edition_storage(edition: &EditionSource, runtime_root: &Path, storage: &str) -> Result<()> {
    let wiki_id = &edition.catalog_entry.wiki_id;
    let mut source = edition.output_dir.clone();
    for component in ["dokuwiki", "data", storage, wiki_id] {
        source.push(component);
        let metadata = match fs::symlink_metadata(&source) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "inspect {storage} data for edition '{}'",
                        edition.catalog_entry.id
                    )
                })
            }
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(anyhow!(
                "unsafe {storage} data path for edition '{}': {}",
                edition.catalog_entry.id,
                source.display()
            ));
        }
    }

    let destination = runtime_root
        .join("dokuwiki/data")
        .join(storage)
        .join(wiki_id);
    copy_regular_tree(&source, &destination).with_context(|| {
        format!(
            "copy {storage} data for edition '{}'",
            edition.catalog_entry.id
        )
    })
}

fn copy_regular_tree(source: &Path, destination: &Path) -> Result<()> {
    fs::create_dir_all(destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = entry.file_type()?;
        if metadata.is_symlink() {
            return Err(anyhow!(
                "DokuWiki data tree contains a symlink: {}",
                source_path.display()
            ));
        }
        if metadata.is_dir() {
            copy_regular_tree(&source_path, &destination_path)?;
        } else if metadata.is_file() {
            fs::copy(&source_path, &destination_path)?;
        } else {
            return Err(anyhow!(
                "DokuWiki data tree contains a non-regular file: {}",
                source_path.display()
            ));
        }
    }
    Ok(())
}

struct TemporaryRuntime {
    path: PathBuf,
}

impl TemporaryRuntime {
    fn create() -> Result<Self> {
        for _ in 0..16 {
            let path = std::env::temp_dir().join(format!("repowiki-reader-{}", Uuid::new_v4()));
            match fs::create_dir(&path) {
                Ok(()) => {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        if let Err(error) =
                            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
                        {
                            let _ = fs::remove_dir(&path);
                            return Err(error)
                                .context("secure temporary DokuWiki runtime directory");
                        }
                    }
                    return Ok(Self { path });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(error).context("create temporary DokuWiki runtime directory")
                }
            }
        }
        Err(anyhow!(
            "could not allocate a unique temporary DokuWiki runtime directory"
        ))
    }
}

impl Drop for TemporaryRuntime {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn select_ephemeral_port() -> Result<u16> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .context("select a free loopback port for DokuWiki")?;
    Ok(listener.local_addr()?.port())
}

fn spawn_server(context: &dokuwiki::WikiContext, port: u16) -> Result<ServerProcess> {
    let address = format!("127.0.0.1:{port}");
    let router = context.runtime.integration_dir.join("router.php");
    let mut command = Command::new(&context.runtime.php);
    command
        .arg("-S")
        .arg(&address)
        .arg("-t")
        .arg(&context.runtime.core_dir)
        .arg(&router)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    dokuwiki::configure_process(&mut command, context);
    let child = command.spawn().with_context(|| {
        format!(
            "start DokuWiki PHP server using '{}'",
            context.runtime.php.to_string_lossy()
        )
    })?;
    Ok(ServerProcess { child })
}

fn wait_until_ready(child: &mut Child, port: u16, expected: &ReaderCatalog) -> Result<()> {
    let deadline = Instant::now() + READINESS_TIMEOUT;
    loop {
        if shutdown_requested() {
            return Err(anyhow!("DokuWiki startup interrupted"));
        }
        if let Some(status) = child
            .try_wait()
            .context("inspect DokuWiki PHP server startup")?
        {
            return Err(anyhow!(
                "DokuWiki PHP server exited before its native catalog action became ready ({status})"
            ));
        }
        if probe_native_catalog(port, expected)? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(anyhow!(
                "DokuWiki PHP server did not expose {CATALOG_ACTION} before the readiness timeout"
            ));
        }
        thread::sleep(READINESS_RETRY_INTERVAL);
    }
}

fn probe_native_catalog(port: u16, expected: &ReaderCatalog) -> Result<bool> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = match TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) {
        Ok(stream) => stream,
        Err(error) if is_transient_socket_error(&error) => return Ok(false),
        Err(error) => return Err(error).context("connect to DokuWiki catalog action"),
    };
    stream
        .set_read_timeout(Some(SOCKET_READ_TIMEOUT))
        .context("set DokuWiki catalog read timeout")?;
    stream
        .set_write_timeout(Some(SOCKET_READ_TIMEOUT))
        .context("set DokuWiki catalog write timeout")?;
    if let Err(error) = write!(
        stream,
        "GET {CATALOG_ACTION} HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\nAccept: application/json\r\nAccept-Encoding: identity\r\nConnection: close\r\n\r\n"
    ) {
        if is_transient_socket_error(&error) {
            return Ok(false);
        }
        return Err(error).context("request the native DokuWiki catalog action");
    }

    let mut response = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                if response.len() + read > MAX_CATALOG_RESPONSE_BYTES {
                    return Err(anyhow!("DokuWiki catalog response exceeded its size limit"));
                }
                response.extend_from_slice(&buffer[..read]);
                if response_body_complete(&response) {
                    break;
                }
            }
            Err(error) if is_transient_socket_error(&error) => return Ok(false),
            Err(error) => return Err(error).context("read the native DokuWiki catalog response"),
        }
    }

    let Some(separator) = response.windows(4).position(|window| window == b"\r\n\r\n") else {
        if response.is_empty() {
            return Ok(false);
        }
        return Err(anyhow!(
            "DokuWiki catalog action returned malformed HTTP headers"
        ));
    };
    let headers = std::str::from_utf8(&response[..separator])
        .context("DokuWiki catalog action returned non-UTF-8 HTTP headers")?;
    let status = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| anyhow!("DokuWiki catalog action returned malformed HTTP status"))?;
    if status != 200 {
        return Err(anyhow!(
            "DokuWiki native catalog action {CATALOG_ACTION} returned HTTP {status}"
        ));
    }
    let body = &response[separator + 4..];
    let actual: ReaderCatalog = serde_json::from_slice(body).with_context(|| {
        format!(
            "DokuWiki native catalog action {CATALOG_ACTION} did not return the RepoWiki catalog JSON contract"
        )
    })?;
    if &actual != expected {
        return Err(anyhow!(
            "DokuWiki native catalog action returned a catalog that differs from the validated editions"
        ));
    }
    Ok(true)
}

fn response_body_complete(response: &[u8]) -> bool {
    let Some(separator) = response.windows(4).position(|window| window == b"\r\n\r\n") else {
        return false;
    };
    let Ok(headers) = std::str::from_utf8(&response[..separator]) else {
        return false;
    };
    let Some(length) = headers.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse::<usize>().ok())
            .flatten()
    }) else {
        return false;
    };
    response.len() >= separator + 4 + length
}

fn is_transient_socket_error(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::ConnectionRefused
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::NotConnected
            | io::ErrorKind::TimedOut
            | io::ErrorKind::WouldBlock
            | io::ErrorKind::UnexpectedEof
            | io::ErrorKind::BrokenPipe
    )
}

fn supervise_server(server: &mut ServerProcess) -> Result<()> {
    loop {
        if shutdown_requested() {
            server.stop();
            return Ok(());
        }
        if let Some(status) = server
            .child
            .try_wait()
            .context("wait for DokuWiki PHP server")?
        {
            return status_result(status, "DokuWiki PHP server exited");
        }
        thread::sleep(SERVER_POLL_INTERVAL);
    }
}

fn status_result(status: ExitStatus, message: &str) -> Result<()> {
    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("{message} ({status})"))
    }
}

struct ServerProcess {
    child: Child,
}

impl ServerProcess {
    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        self.stop();
    }
}

struct SignalGuard {
    #[cfg(unix)]
    previous_interrupt: usize,
    #[cfg(unix)]
    previous_terminate: usize,
}

#[cfg(any(unix, windows))]
static SHUTDOWN_SIGNAL: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
#[cfg(unix)]
extern "C" fn handle_shutdown_signal(signal: i32) {
    SHUTDOWN_SIGNAL.store(signal, std::sync::atomic::Ordering::SeqCst);
}

#[cfg(windows)]
unsafe extern "system" fn handle_console_control_event(event: u32) -> i32 {
    use std::sync::atomic::Ordering;

    if matches!(event, 0 | 1 | 2 | 5 | 6) {
        SHUTDOWN_SIGNAL.store(event as i32 + 1, Ordering::SeqCst);
        1
    } else {
        0
    }
}
#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    #[link_name = "SetConsoleCtrlHandler"]
    fn set_console_ctrl_handler(
        handler: Option<unsafe extern "system" fn(u32) -> i32>,
        add: i32,
    ) -> i32;
}

#[cfg(unix)]
impl SignalGuard {
    fn install() -> Result<Self> {
        use std::sync::atomic::Ordering;
        const SIGINT: i32 = 2;
        const SIGTERM: i32 = 15;
        extern "C" {
            fn signal(signal: i32, handler: usize) -> usize;
        }

        SHUTDOWN_SIGNAL.store(0, Ordering::SeqCst);
        let handler = handle_shutdown_signal as *const () as usize;
        let previous_interrupt = unsafe { signal(SIGINT, handler) };
        if previous_interrupt == usize::MAX {
            return Err(io::Error::last_os_error()).context("handle Ctrl-C for DokuWiki cleanup");
        }
        let previous_terminate = unsafe { signal(SIGTERM, handler) };
        if previous_terminate == usize::MAX {
            unsafe {
                signal(SIGINT, previous_interrupt);
            }
            return Err(io::Error::last_os_error())
                .context("handle termination for DokuWiki cleanup");
        }
        Ok(Self {
            previous_interrupt,
            previous_terminate,
        })
    }
}

#[cfg(unix)]
impl Drop for SignalGuard {
    fn drop(&mut self) {
        const SIGINT: i32 = 2;
        const SIGTERM: i32 = 15;
        extern "C" {
            fn signal(signal: i32, handler: usize) -> usize;
        }
        unsafe {
            signal(SIGINT, self.previous_interrupt);
            signal(SIGTERM, self.previous_terminate);
        }
    }
}

#[cfg(windows)]
impl SignalGuard {
    fn install() -> Result<Self> {
        use std::sync::atomic::Ordering;

        SHUTDOWN_SIGNAL.store(0, Ordering::SeqCst);
        let registered = unsafe { set_console_ctrl_handler(Some(handle_console_control_event), 1) };
        if registered == 0 {
            return Err(io::Error::last_os_error())
                .context("handle console shutdown for DokuWiki cleanup");
        }
        Ok(Self {})
    }
}

#[cfg(windows)]
impl Drop for SignalGuard {
    fn drop(&mut self) {
        unsafe {
            set_console_ctrl_handler(Some(handle_console_control_event), 0);
        }
    }
}

#[cfg(any(unix, windows))]
fn shutdown_requested() -> bool {
    SHUTDOWN_SIGNAL.load(std::sync::atomic::Ordering::SeqCst) != 0
}

#[cfg(not(any(unix, windows)))]
impl SignalGuard {
    fn install() -> Result<Self> {
        Ok(Self {})
    }
}

#[cfg(not(any(unix, windows)))]
fn shutdown_requested() -> bool {
    false
}

#[cfg(not(any(unix, windows)))]
impl Drop for SignalGuard {
    fn drop(&mut self) {}
}
