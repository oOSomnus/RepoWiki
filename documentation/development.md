# Development notes

[Back to README](../README.md) · [中文说明](README.zh-CN.md)

This document keeps repository-maintenance details out of the project landing
page. The root README is for the project overview and quick start; this page is
for contributors working on the build, tests, package layout, and Reader.

## Repository structure

- `engine/` contains the Rust analyzer, CLI, embedded prompt sources and tests,
  plus the RepoWiki DokuWiki integration plugins and bootstrap.
- `vendor/dokuwiki/` holds the pinned upstream DokuWiki and plugin sources and
  their license notices for packaging.
- `skill/` contains the runtime Skill source: `SKILL.md`, agent metadata, and
  references.
- `tools/` contains development-time replay and package validation tools.
- `reference/CodeWiki/` is read-only reference material used to curate the
  architecture few-shot examples under `skill/references/few-shots/`.
- `preview/`, `dist/`, and `.build/` are generated locally and are not runtime
  source directories.

## Build pipeline

Build requirements are GNU Make, Rust/Cargo, Python 3, `zip`, and `unzip`.
Runtime wiki generation and the Reader require PHP 8.2+ with `mbstring` and
`xml` enabled.

`make preview` builds the release `repowiki` executable and copies the runtime
Skill source into `preview/`. The binary is platform-specific, so build the
package on the platform where it will be installed.

`make build` packages `preview/` as `dist/RepoWiki-<version>.zip` and validates
both the preview directory and the archive. Each RepoWiki and Change Wiki
package contains the runtime instructions, executable, and the bundled
DokuWiki runtime sources:

```text
SKILL.md
agents/openai.yaml
references/cli-contract.md
references/prompt-map.md
scripts/repowiki                  # or scripts/repowiki.exe
vendor/dokuwiki/                  # pinned DokuWiki and plugin sources/notices
engine/dokuwiki/                  # RepoWiki DokuWiki adapter, plugins, and router
```

Both packages bundle the pinned DokuWiki `release-2026-07-14c` (“Mort”) and
Mermaid plugin `v11.15b` source; the Reader runtime carries the same sources.
These components are not downloaded at runtime, and PHP itself is not bundled.

The Rust prompt sources are embedded into the executable during the build.

## Wiki pages and Agent workflow

Each edition stores native DokuWiki pages in its own savedir:

```text
.repowiki/dokuwiki/data/pages/**/*.txt
.repowiki/changes/<base>..<head>/dokuwiki/data/pages/**/*.txt
```

The repository edition uses the stable `repo:` namespace. A Change Wiki uses
`change_<SHA256(complete base..head ID)>:`. Module page IDs retain their full
ancestry: for example, `System/API` becomes `repo:system:api:start`. Its source
file is `dokuwiki/data/pages/repo/system/api/start.txt`. The `tree order`
result's `doc_path` is this logical DokuWiki page ID, not a physical path.

The RepoWiki workflow runs analysis, clustering and tree review, `tree order`,
page-prompt generation leaf-to-root, `doc write`, `doc validate`, then
`session close`. Incremental updates use plan/route/context/repair/finalize.
Generated source uses native
DokuWiki syntax, including `====== Heading ======`, links such as
`[[repo:system:api:start|API]]`, `<code rust>...</code>`, and
`<mermaid>...</mermaid>`; the bundled engine and Mermaid plugin interpret the
pages.

Keep each component's license and corresponding source notices with distributed
packages. RepoWiki's original Rust source, prompts, and documentation are MIT;
the RepoWiki DokuWiki integration in `engine/dokuwiki/` is GPL-2.0-or-later.
The Cargo source package includes both sets of files, so its metadata declares
`MIT AND GPL-2.0-or-later`; this does not change the license of any individual
component. DokuWiki and the Mermaid plugin are GPLv2, Mermaid.js is MIT, and
Composer-managed dependencies retain their own licenses. See the root
[`LICENSE`](../LICENSE) for the project license scope.

## Standalone reader

The `repowiki-reader` command remains separate from the packaged Skill CLI and
keeps its invocation:
`repowiki-reader <.repowiki> [--port ...] [--no-open]`.
It runs the bundled DokuWiki/PHP engine to render native pages rather than an
embedded Markdown renderer:

```bash
make reader
.build/cargo-target/release/repowiki-reader /path/to/project/.repowiki
```

The Reader requires system PHP 8.2+ with `mbstring` and `xml` enabled, binds
to loopback, and chooses a free port by default. `--no-open` keeps the browser
closed for headless environments; `--port <port>` selects a fixed local port.
The package includes the pinned engine and plugin source, so runtime page loads
do not need a CDN or downloads. The Reader renders a read-only view and does not
write to the selected `.repowiki` directory.

Rust commands use the repository's latest Stable toolchain through
`rust-toolchain.toml`. Refresh it before verification with:

```bash
rustup update stable
rustup show active-toolchain
```

`make install` prompts for RepoWiki, Change Wiki, or both, then builds and
validates only the selected packages before installing them below `INSTALL_DIR`
(default: `~/.agents/skills`). Scripts and CI can set
`INSTALL_SELECTION=repowiki`, `change-wiki`, or `both` to skip the prompt. Each
installation is staged and validated before its existing skill directory is
replaced. Set `INSTALL_DIR` to install below a different directory, for example:

```bash
make install INSTALL_DIR=/custom/agent/skills
```

`make clean` removes generated build outputs, previews, archives, and local
packaging artifacts for both Skills.

To install an archive manually:

```bash
make build
mkdir -p ~/.agents/skills/RepoWiki
unzip dist/RepoWiki-*.zip -d ~/.agents/skills/RepoWiki
```

## Verification

`make test` runs the layered offline gate and requires PHP 8.2+ with the
`mbstring` and `xml` extensions for its real DokuWiki/Mermaid integration coverage:

- `test-contract` checks local architecture prompt semantics, the curated
  few-shot catalog, and rejects legacy tool names or missing module/overview
  obligations;
- the replay drives the real packaged CLI through analysis, prompt rendering,
  recursive tree saving, leaf-first ordering, overview-context generation,
  native DokuWiki page writes and validation, session close, and ZIP extraction,
  then compares the normalized page sources and metadata with
  `tests/golden/mini-repo.json`;
- Rust integration contracts cover prompt variables and rendering, semantic
  architecture-anchor selection and quality diagnostics, update routing/stale
  scans, and CLI behavior from a non-repository working directory. The CLI
  smoke layer also exercises cross-process session serialization, strict
  input-artifact roles, and same-content document retries;
- the Reader contract starts the supervised loopback PHP server, checks the
  native catalog and edition pages, confirms local Mermaid assets and
  edition-scoped search, rejects private PHP/page-source routes, and exercises
  Ctrl-C cleanup;
- formatting, tests, Clippy, and runtime-package validation complete the gate.

The replay does not call an LLM. Prompt hashes, the generated workflow host
contract, fixed native DokuWiki source, and processing-order assertions make
changes to the host-agent architecture contract visible while the tree
intentionally leaves low-level analysis candidates outside the published page
hierarchy.

Run the prompt/static layer alone with:

```bash
make test-contract
```

`make test-install` repeats the package validation through a temporary install
directory and checks that a stale file is removed during replacement.

The reference checkout is read-only source material for the architecture
few-shots. When changing prompts, add an example only when a reference page
adds a useful reading pattern or diagram discipline.

Use `make clean` to remove generated build, preview, archive, and local
packaging artifacts.
