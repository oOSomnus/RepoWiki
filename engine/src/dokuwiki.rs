use crate::session::{self, files, SessionState};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use uuid::Uuid;

const PINNED_DOKUWIKI_VERSION: &str = "2026-07-14c \"Mort\"";
const PHP_MINIMUM_VERSION_ID: u32 = 80200;

#[derive(Debug, Clone)]
pub struct RuntimePaths {
    pub core_dir: PathBuf,
    pub package_root: PathBuf,
    pub integration_dir: PathBuf,
    pub php: OsString,
}

#[derive(Debug, Clone)]
pub struct WikiContext {
    pub runtime: RuntimePaths,
    pub config_dir: PathBuf,
    pub plugin_dir: PathBuf,
    pub savedir: PathBuf,
    pub workspace: PathBuf,
    pub wiki_id: String,
    pub title: String,
    pub read_only: bool,
}

#[derive(Debug, Clone)]
pub struct WikiContextConfig {
    pub runtime: RuntimePaths,
    pub config_dir: PathBuf,
    pub plugin_dir: PathBuf,
    pub savedir: PathBuf,
    pub workspace: PathBuf,
    pub wiki_id: String,
    pub title: String,
    pub read_only: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ParsedPage {
    pub links: Vec<String>,
    pub html: String,
    /// How DokuWiki's own parser read the page.
    ///
    /// Defaulted because the field is additive on the wire: a binary from either
    /// side of an upgrade must still decode the other's response.
    #[serde(default)]
    pub structure: PageStructure,
}

/// The parts of a page that carry meaning about its source rather than its text.
///
/// Every offset is a **byte** offset into the page source with CRLF folded to LF,
/// which is what DokuWiki's lexer reports; they are not character indices. Slice
/// Rust text at them only after folding line endings the same way and rounding to
/// a character boundary with [`str::floor_char_boundary`], or a page containing
/// multibyte text will panic mid-slice.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PageStructure {
    pub headings: Vec<Heading>,
    pub spans: Vec<Span>,
}

/// A heading line, from its first byte to the end of that line.
#[derive(Debug, Clone, Deserialize)]
pub struct Heading {
    /// DokuWiki's heading level, derived from the length of the `=` runs.
    pub level: u8,
    /// Title as the lexer reports it: `=` delimiters removed, inline markup kept.
    pub text: String,
    pub start: usize,
    pub end: usize,
}

/// A run of source DokuWiki renders without parsing it.
#[derive(Debug, Clone, Deserialize)]
pub struct Span {
    pub kind: SpanKind,
    /// Delimiter-inclusive: points at `<code`, `<file`, `<nowiki>` or `%%`.
    pub start: usize,
    /// One past the closing delimiter.
    pub end: usize,
}

/// Which construct a [`Span`] was produced by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SpanKind {
    Code,
    File,
    Nowiki,
    /// The `%%…%%` spelling of DokuWiki's unformatted mode.
    Unformatted,
}

#[derive(Serialize)]
struct PluginRequest<'a> {
    op: &'a str,
    page_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<&'a str>,
}

pub fn discover_runtime() -> Result<RuntimePaths> {
    let configured = std::env::var_os("REPOWIKI_DOKUWIKI_DIR").map(PathBuf::from);
    let core_dir = if let Some(configured) = configured {
        resolve_core_dir(&configured).with_context(|| {
            format!(
                "REPOWIKI_DOKUWIKI_DIR does not contain the pinned DokuWiki source: {}",
                configured.display()
            )
        })?
    } else {
        let executable = std::env::current_exe().context("locate RepoWiki executable")?;
        let from_executable = executable.ancestors().find_map(|ancestor| {
            let candidate = ancestor.join("vendor/dokuwiki");
            candidate.join("VERSION").is_file().then_some(candidate)
        });
        from_executable
            .or_else(|| {
                let candidate = Path::new(env!("CARGO_MANIFEST_DIR")).join("../vendor/dokuwiki");
                candidate.join("VERSION").is_file().then_some(candidate)
            })
            .ok_or_else(|| {
                anyhow!("bundled DokuWiki source not found; set REPOWIKI_DOKUWIKI_DIR")
            })?
    };
    let core_dir = core_dir
        .canonicalize()
        .with_context(|| format!("canonicalize DokuWiki source: {}", core_dir.display()))?;
    let version = fs::read_to_string(core_dir.join("VERSION"))
        .with_context(|| format!("read DokuWiki version from {}", core_dir.display()))?;
    if version.trim() != PINNED_DOKUWIKI_VERSION {
        return Err(anyhow!(
            "unsupported DokuWiki source version '{}'; RepoWiki requires {}",
            version.trim(),
            PINNED_DOKUWIKI_VERSION
        ));
    }
    let package_root = core_dir
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| anyhow!("DokuWiki source must be under vendor/dokuwiki"))?
        .to_path_buf();
    let integration_dir = package_root.join("engine/dokuwiki");
    if !integration_dir.join("bin/repowiki.php").is_file() {
        return Err(anyhow!(
            "RepoWiki DokuWiki adapter is missing: {}",
            integration_dir.display()
        ));
    }
    let php = std::env::var_os("REPOWIKI_PHP_BIN").unwrap_or_else(|| OsString::from("php"));
    let runtime = RuntimePaths {
        core_dir,
        package_root,
        integration_dir,
        php,
    };
    ensure_php_82(&runtime)?;
    Ok(runtime)
}

fn resolve_core_dir(path: &Path) -> Option<PathBuf> {
    if path.join("VERSION").is_file() {
        return Some(path.to_path_buf());
    }
    let nested = path.join("vendor/dokuwiki");
    nested.join("VERSION").is_file().then_some(nested)
}

fn ensure_php_82(runtime: &RuntimePaths) -> Result<()> {
    let output = Command::new(&runtime.php)
        .args(["-n", "-r", "echo PHP_VERSION_ID;"])
        .output()
        .with_context(|| {
            format!(
                "PHP 8.2 or newer is required; could not start '{}'",
                runtime.php.to_string_lossy()
            )
        })?;
    if !output.status.success() {
        return Err(anyhow!(
            "PHP 8.2 or newer is required; version check failed: {}",
            output_message(&output)
        ));
    }
    let version_id = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u32>()
        .context("PHP returned an invalid version number")?;
    if version_id < PHP_MINIMUM_VERSION_ID {
        let major = version_id / 10_000;
        let minor = (version_id / 100) % 100;
        return Err(anyhow!(
            "PHP 8.2 or newer is required; found PHP {major}.{minor}"
        ));
    }
    Ok(())
}

pub fn prepare_context(config: WikiContextConfig) -> Result<WikiContext> {
    let WikiContextConfig {
        runtime,
        config_dir,
        plugin_dir,
        savedir,
        workspace,
        wiki_id,
        title,
        read_only,
    } = config;
    fs::create_dir_all(&workspace)?;
    prepare_data_directories(&savedir)?;
    prepare_config(&runtime, &config_dir, &savedir, &wiki_id, &title, read_only)?;
    prepare_plugin_overlay(&runtime, &plugin_dir)?;
    Ok(WikiContext {
        runtime,
        config_dir,
        plugin_dir,
        savedir,
        workspace,
        wiki_id,
        title,
        read_only,
    })
}
fn prepare_data_directories(savedir: &Path) -> Result<()> {
    for directory in [
        "attic",
        "cache",
        "index",
        "locks",
        "log",
        "media",
        "media_attic",
        "media_meta",
        "meta",
        "pages",
        "tmp",
    ] {
        fs::create_dir_all(savedir.join(directory))?;
    }
    Ok(())
}

pub fn session_context(state: &SessionState) -> Result<WikiContext> {
    let runtime = discover_runtime()?;
    let workspace = session::session_file(state, files::DOKUWIKI_RUNTIME)?;
    let savedir = Path::new(&state.output_dir).join("dokuwiki/data");
    prepare_context(WikiContextConfig {
        runtime,
        config_dir: workspace.join("conf"),
        plugin_dir: workspace.join("plugins"),
        savedir,
        workspace,
        wiki_id: state.wiki_id.clone(),
        title: wiki_title(Path::new(&state.repo_path)),
        read_only: false,
    })
}

pub fn configure_process(command: &mut Command, context: &WikiContext) {
    command
        .env("REPOWIKI_DOKUWIKI_DIR", &context.runtime.core_dir)
        .env("REPOWIKI_DOKU_CONF", &context.config_dir)
        .env("REPOWIKI_DOKU_PLUGIN_DIR", &context.plugin_dir)
        .env("REPOWIKI_DOKU_SAVEDIR", &context.savedir)
        .env("REPOWIKI_WIKI_ID", &context.wiki_id)
        .env("REPOWIKI_START_ID", format!("{}:start", context.wiki_id))
        .env("REPOWIKI_WIKI_TITLE", &context.title)
        .env(
            "REPOWIKI_READ_ONLY",
            if context.read_only { "1" } else { "0" },
        );
}

impl WikiContext {
    pub fn read(&self, page_id: &str) -> Result<String> {
        let value = invoke_plugin(self, "read", page_id, None)?;
        value["content"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow!("DokuWiki read returned no page content for {page_id}"))
    }

    pub fn write(&self, page_id: &str, content: &str) -> Result<()> {
        invoke_plugin(self, "write", page_id, Some(content))?;
        Ok(())
    }

    pub fn parse(&self, page_id: &str, content: &str) -> Result<ParsedPage> {
        let value = invoke_plugin(self, "parse", page_id, Some(content))?;
        serde_json::from_value(value)
            .with_context(|| format!("invalid DokuWiki parse response for {page_id}"))
    }

    pub fn render(&self, page_id: &str) -> Result<String> {
        let value = invoke_plugin(self, "render", page_id, None)?;
        value["html"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| anyhow!("DokuWiki render returned no HTML for {page_id}"))
    }
}

fn invoke_plugin(
    context: &WikiContext,
    op: &str,
    page_id: &str,
    content: Option<&str>,
) -> Result<Value> {
    let request_path = context
        .workspace
        .join(format!("request-{}.json", Uuid::new_v4().simple()));
    session::write_json(
        &request_path,
        &PluginRequest {
            op,
            page_id,
            content,
        },
    )?;
    let result = (|| {
        let mut command = Command::new(&context.runtime.php);
        command
            .arg(context.runtime.integration_dir.join("bin/repowiki.php"))
            .arg(&request_path);
        configure_process(&mut command, context);
        let output = command
            .output()
            .with_context(|| format!("start DokuWiki {op} operation"))?;
        if !output.status.success() {
            return Err(anyhow!(
                "DokuWiki {op} failed for {page_id}: {}",
                output_message(&output)
            ));
        }
        serde_json::from_slice(&output.stdout)
            .with_context(|| format!("decode DokuWiki {op} response for {page_id}"))
    })();
    let _ = fs::remove_file(&request_path);
    result
}

fn output_message(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    } else {
        stderr
    }
}

fn wiki_title(repo_path: &Path) -> String {
    repo_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("RepoWiki")
        .to_string()
}

fn prepare_config(
    runtime: &RuntimePaths,
    config_dir: &Path,
    savedir: &Path,
    wiki_id: &str,
    title: &str,
    read_only: bool,
) -> Result<()> {
    validate_namespace(wiki_id)?;
    let default_config = config_dir.join("dokuwiki.php");
    if !default_config.is_file() {
        fs::create_dir_all(config_dir)?;
        copy_tree(&runtime.core_dir.join("conf"), config_dir)?;
    }
    let local_config = r#"<?php
$conf['title'] = getenv('REPOWIKI_WIKI_TITLE') ?: 'RepoWiki';
$conf['start'] = getenv('REPOWIKI_START_ID') ?: 'repo:start';
$conf['savedir'] = getenv('REPOWIKI_DOKU_SAVEDIR') ?: '';
if ($conf['savedir'] === '') {
    throw new RuntimeException('REPOWIKI_DOKU_SAVEDIR is required');
}
$conf['useacl'] = getenv('REPOWIKI_READ_ONLY') === '1' ? 1 : 0;
$conf['disableactions'] = getenv('REPOWIKI_READ_ONLY') === '1'
    ? 'edit,source,media,register,profile,admin,login,logout,subscribe,resendpwd,save,delete'
    : '';
$conf['syntax'] = 'dw';
$conf['remote'] = 0;
$conf['remotecors'] = '';
$conf['jquerycdn'] = 0;
$conf['updatecheck'] = 0;
$conf['sitemap'] = 0;
// The visit-history trace labels every canonical page "start"; the RepoWiki
// plugin renders a real ancestor path instead.
$conf['breadcrumbs'] = 0;
$conf['youarehere'] = 0;
$conf['plugin']['mermaid']['location'] = 'local';
$conf['plugin']['mermaid']['showSaveButton'] = 0;
$conf['plugin']['mermaid']['showLockButton'] = 0;
"#;
    fs::write(config_dir.join("local.php"), local_config)?;
    if read_only {
        fs::write(config_dir.join("acl.auth.php"), "* @ALL 1\n")?;
        fs::write(config_dir.join("users.auth.php"), "")?;
    } else {
        let acl_path = config_dir.join("acl.auth.php");
        if acl_path.exists() {
            fs::remove_file(acl_path)?;
        }
    }
    let _ = (savedir, title);
    Ok(())
}

fn prepare_plugin_overlay(runtime: &RuntimePaths, plugin_dir: &Path) -> Result<()> {
    let source = runtime.core_dir.join("lib/plugins");
    if !source.is_dir() {
        return Err(anyhow!(
            "DokuWiki plugin directory is missing: {}",
            source.display()
        ));
    }
    let integration_plugin = runtime.integration_dir.join("plugins/repowiki");
    if !integration_plugin.is_dir() {
        return Err(anyhow!(
            "RepoWiki DokuWiki plugin is missing: {}",
            integration_plugin.display()
        ));
    }
    let mermaid_source = source.join("mermaid");
    if !mermaid_source.join("plugin.info.txt").is_file()
        || !mermaid_source.join("syntax.php").is_file()
        || !mermaid_source.join("mermaid.min.js").is_file()
    {
        return Err(anyhow!(
            "bundled DokuWiki Mermaid plugin is missing from {}",
            mermaid_source.display()
        ));
    }
    if plugin_dir.exists() {
        let metadata = fs::symlink_metadata(plugin_dir)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(anyhow!(
                "unsafe DokuWiki plugin directory: {}",
                plugin_dir.display()
            ));
        }
        let overlay_ready = plugin_dir.join("repowiki/cli.php").is_file()
            && plugin_dir.join("mermaid/syntax.php").is_file()
            && fs::read_dir(&source)?.all(|entry| {
                entry
                    .map(|entry| {
                        !entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false)
                            || plugin_dir.join(entry.file_name()).exists()
                    })
                    .unwrap_or(false)
            });
        if overlay_ready {
            return Ok(());
        }
        fs::remove_dir_all(plugin_dir)?;
    }
    fs::create_dir_all(plugin_dir)?;
    for entry in fs::read_dir(&source)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name();
        if name == "repowiki" {
            return Err(anyhow!(
                "RepoWiki plugin conflicts with upstream plugin directory"
            ));
        }
        link_or_copy_dir(&entry.path(), &plugin_dir.join(name))?;
    }
    link_or_copy_dir(&integration_plugin, &plugin_dir.join("repowiki"))?;
    Ok(())
}

fn copy_tree(source: &Path, destination: &Path) -> Result<()> {
    for entry in
        fs::read_dir(source).with_context(|| format!("read directory {}", source.display()))?
    {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            return Err(anyhow!(
                "unexpected symlink in bundled runtime: {}",
                source_path.display()
            ));
        } else if file_type.is_dir() {
            fs::create_dir_all(&destination_path)?;
            copy_tree(&source_path, &destination_path)?;
        } else if file_type.is_file() {
            fs::copy(&source_path, &destination_path)
                .with_context(|| format!("copy runtime file {}", source_path.display()))?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn link_or_copy_dir(source: &Path, destination: &Path) -> Result<()> {
    match std::os::unix::fs::symlink(source, destination) {
        Ok(()) => Ok(()),
        Err(symlink_error) => copy_tree(source, destination).with_context(|| {
            format!(
                "link or copy DokuWiki plugin {}: {symlink_error}",
                source.display()
            )
        }),
    }
}

#[cfg(windows)]
fn link_or_copy_dir(source: &Path, destination: &Path) -> Result<()> {
    match std::os::windows::fs::symlink_dir(source, destination) {
        Ok(()) => Ok(()),
        Err(symlink_error) => copy_tree(source, destination).with_context(|| {
            format!(
                "link or copy DokuWiki plugin {}: {symlink_error}",
                source.display()
            )
        }),
    }
}

#[cfg(not(any(unix, windows)))]
fn link_or_copy_dir(source: &Path, destination: &Path) -> Result<()> {
    copy_tree(source, destination)
}

pub fn validate_namespace(wiki_id: &str) -> Result<()> {
    if wiki_id == session::REPOSITORY_WIKI_ID {
        return Ok(());
    }
    let Some(hash) = wiki_id.strip_prefix("change_") else {
        return Err(anyhow!("invalid DokuWiki edition namespace: {wiki_id}"));
    };
    if hash.len() != 64
        || !hash
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(anyhow!("invalid DokuWiki change namespace: {wiki_id}"));
    }
    Ok(())
}
