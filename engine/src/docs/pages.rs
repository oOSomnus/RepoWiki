//! Page identity: how a wiki namespace and a page ID become a file on disk.
//!
//! DokuWiki stores a page by replacing every `:` in its ID with a path
//! separator — see `wikiFN` and `metaFN` in `inc/pageutils.php` — so
//! `repo:platform:api:start` lives at `dokuwiki/data/pages/repo/platform/api/
//! start.txt`. The same rule places the `attic` and `meta` directories that
//! sit beside `pages`; there is no second spelling of a namespace as one
//! directory name. Assessment, staleness scanning, the reader's runtime merge
//! and the HTML export all come through here instead of deriving the layout.

use anyhow::{anyhow, Context, Result};
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

/// A validated DokuWiki namespace that owns every page an edition writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WikiId(String);

impl WikiId {
    /// Parse a namespace ID, refusing anything DokuWiki could not store.
    pub fn parse(wiki_id: &str) -> Result<Self> {
        if wiki_id.is_empty() || !wiki_id.split(':').all(is_canonical_segment) {
            return Err(anyhow!("invalid DokuWiki namespace ID '{wiki_id}'"));
        }
        Ok(Self(wiki_id.to_string()))
    }

    /// The ID exactly as it is written into page IDs and metadata.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The namespace's path components, deepest last.
    pub fn segments(&self) -> impl Iterator<Item = &str> {
        self.0.split(':')
    }

    /// This namespace's directory beneath `base`, wherever DokuWiki keeps a
    /// namespaced tree: `pages`, `attic` or `meta`.
    pub fn dir_under(&self, base: &Path) -> PathBuf {
        self.segments()
            .fold(base.to_path_buf(), |path, segment| path.join(segment))
    }
}

/// Which part of an edition's page tree to look at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageScope {
    /// Every page file in the edition, including namespaces other than the
    /// wiki's own — a quality report needs to see intruders to name them.
    All,
    /// Only pages inside the wiki's own namespace. A page outside it is not
    /// this edition's to update, so an unusable tree reports "nothing to do"
    /// rather than failing an incremental run.
    Edition,
}

/// One page file found on disk, judged against the wiki it was read for.
#[derive(Debug, Clone)]
pub struct PageFileOnDisk {
    pub page_id: String,
    pub path: PathBuf,
    /// Whether DokuWiki would find this page under `page_id`: a regular file
    /// whose namespace path round-trips through [`page_file_path`].
    pub canonical: bool,
    /// Whether the path may be read as text. A `page_id` reached through a
    /// symlink is still reported, so a report can refuse it by name.
    pub readable: bool,
}

/// The directory DokuWiki stores pages in for an edition.
pub fn pages_root(output: &Path) -> PathBuf {
    output.join("dokuwiki").join("data").join("pages")
}

/// Every page file the edition holds, ordered by path bytes.
pub fn enumerate_pages(
    output: &Path,
    wiki_id: &WikiId,
    scope: PageScope,
) -> Result<Vec<PageFileOnDisk>> {
    let pages = pages_root(output);
    let Some(walk_root) = walk_root(output, &pages, wiki_id, scope)? else {
        return Ok(Vec::new());
    };
    let mut found = Vec::new();
    collect_page_files(&walk_root, &mut found)?;
    found.sort_by(|left, right| left.0.cmp(&right.0));
    found
        .into_iter()
        .map(|(path, readable)| {
            let page_id = page_id_from_file(&pages, &path)?;
            let canonical = readable
                && page_file_path(output, wiki_id.as_str(), &page_id)
                    .is_ok_and(|expected_path| expected_path == path);
            Ok(PageFileOnDisk {
                page_id,
                path,
                canonical,
                readable,
            })
        })
        .collect()
}

/// The path a page ID maps to, or an error if anything on the way is not a
/// real directory — or the page itself is not a regular file.
pub fn require_canonical_page(output: &Path, wiki_id: &WikiId, page_id: &str) -> Result<PathBuf> {
    let path = page_path_under(output, wiki_id, page_id)?;
    if path.extension().and_then(|value| value.to_str()) != Some("txt") {
        return Err(anyhow!(
            "canonical page ID '{page_id}' did not map to a .txt page"
        ));
    }
    let relative = path
        .strip_prefix(output)
        .with_context(|| format!("page path for '{page_id}' escaped its edition"))?;
    let component_count = relative.components().count();
    let mut current = output.to_path_buf();
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
    Ok(path)
}

fn is_canonical_segment(segment: &str) -> bool {
    let bytes = segment.as_bytes();
    !bytes.is_empty()
        && !matches!(bytes[0], b'_' | b'-')
        && !matches!(bytes[bytes.len() - 1], b'_' | b'-')
        && !segment.contains("__")
        && bytes.iter().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_' || *byte == b'-'
        })
}

fn canonical_module_segment(name: &str) -> Result<String> {
    if name.is_empty()
        || !name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return Err(anyhow!(
            "invalid module name '{name}': use a non-empty ASCII name containing only letters, digits, '_' or '-'"
        ));
    }
    let segment = name.to_ascii_lowercase();
    if !is_canonical_segment(&segment) {
        return Err(anyhow!(
            "invalid module name '{name}': DokuWiki canonical IDs cannot start or end with '_' or '-' or contain '__'"
        ));
    }
    Ok(segment)
}

pub fn module_page_id(wiki_id: &str, path: &[String]) -> Result<String> {
    let wiki_id = WikiId::parse(wiki_id)?;
    if path.is_empty() {
        return Err(anyhow!("module path cannot be empty"));
    }
    let segments = path
        .iter()
        .map(|name| canonical_module_segment(name))
        .collect::<Result<Vec<_>>>()?;
    Ok(format!("{}:{}:start", wiki_id.as_str(), segments.join(":")))
}

pub fn overview_page_id(wiki_id: &str) -> Result<String> {
    let wiki_id = WikiId::parse(wiki_id)?;
    Ok(format!("{}:start", wiki_id.as_str()))
}

/// Refuse a page ID that DokuWiki could not store inside this namespace.
pub fn validate_page_id(wiki_id: &str, page_id: &str) -> Result<()> {
    page_path_under(Path::new(""), &WikiId::parse(wiki_id)?, page_id).map(|_| ())
}

pub fn page_file_path(output: &Path, wiki_id: &str, page_id: &str) -> Result<PathBuf> {
    page_path_under(output, &WikiId::parse(wiki_id)?, page_id)
}

fn page_path_under(output: &Path, wiki_id: &WikiId, page_id: &str) -> Result<PathBuf> {
    let namespace = wiki_id.as_str();
    let wiki_prefix = format!("{namespace}:");
    let page_suffix = page_id
        .strip_prefix(&wiki_prefix)
        .ok_or_else(|| anyhow!("page ID '{page_id}' is outside wiki namespace '{namespace}'"))?;
    let mut path = pages_root(output);
    for segment in wiki_id.segments().chain(page_suffix.split(':')) {
        if !is_canonical_segment(segment) {
            return Err(anyhow!("invalid DokuWiki page ID '{page_id}'"));
        }
        path.push(segment);
    }
    path.set_extension("txt");
    Ok(path)
}

fn page_id_from_file(pages: &Path, path: &Path) -> Result<String> {
    let relative = path.strip_prefix(pages)?;
    let mut segments = relative
        .parent()
        .into_iter()
        .flat_map(|parent| parent.components())
        .filter_map(|component| match component {
            Component::Normal(segment) => Some(segment.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let page_name = path
        .file_stem()
        .ok_or_else(|| anyhow!("DokuWiki page file has no name: {}", path.display()))?
        .to_string_lossy()
        .into_owned();
    segments.push(page_name);
    Ok(segments.join(":"))
}

/// Locate the directory the walk starts from, or `None` when there is nothing
/// to enumerate.
fn walk_root(
    output: &Path,
    pages: &Path,
    wiki_id: &WikiId,
    scope: PageScope,
) -> Result<Option<PathBuf>> {
    if scope == PageScope::All {
        if !pages.exists() {
            return Ok(None);
        }
        if !pages.canonicalize()?.starts_with(output.canonicalize()?) {
            return Err(anyhow!(
                "DokuWiki pages directory escapes edition output: {}",
                pages.display()
            ));
        }
        return Ok(Some(pages.to_path_buf()));
    }
    let canonical_output = output.canonicalize()?;
    let canonical_pages = match pages.canonicalize() {
        Ok(path) if path.starts_with(&canonical_output) => path,
        Ok(_) => return Ok(None),
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let namespace = wiki_id.dir_under(pages);
    let canonical_namespace = match namespace.canonicalize() {
        Ok(path) if path.starts_with(&canonical_pages) => path,
        Ok(_) => return Ok(None),
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !fs::symlink_metadata(&namespace)?.file_type().is_dir() || !canonical_namespace.is_dir() {
        return Ok(None);
    }
    Ok(Some(namespace))
}

fn collect_page_files(directory: &Path, found: &mut Vec<(PathBuf, bool)>) -> Result<()> {
    if !directory.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_page_files(&path, found)?;
        } else if path.extension().and_then(|value| value.to_str()) == Some("txt")
            && (file_type.is_file() || file_type.is_symlink())
        {
            found.push((path, file_type.is_file()));
        }
    }
    Ok(())
}
