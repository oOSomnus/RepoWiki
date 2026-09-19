# RepoWiki

RepoWiki is an agent skill for generating repository wikis. Its user-facing
name is `RepoWiki` and its normalized agent trigger is `$repo-wiki`. It uses a Rust CLI
for repository analysis, component reads, module-tree validation, safe document
writes, incremental updates, and JSON artifact management. The host agent
generates the Markdown content from the embedded prompts.

The distributable Skill package contains only runtime documentation, agent
metadata, references, and the platform-specific `codewiki` executable. Cargo
source files are used to build the executable and are not included in the
preview directory or final ZIP archive.

## Project layout

```text
.
├── Makefile                 # build and installation entry points
├── README.md                # project and development documentation
├── engine/                  # Cargo build input; not included in the Skill package
│   ├── Cargo.toml
│   ├── Cargo.lock
│   ├── src/
│   ├── prompts/
│   └── tests/
├── skill/                   # tracked runtime Skill source
│   ├── SKILL.md
│   ├── agents/openai.yaml
│   └── references/
├── preview/                 # generated runtime package directory
├── dist/                    # generated ZIP archives
├── tools/                   # development-time package validators
└── reference/CodeWiki/      # read-only reference implementation
```

## Make commands

Requirements: GNU Make, Rust/Cargo, Python 3, `zip`, and `unzip` for
installation.

```bash
# Build the platform-specific executable and runtime package directory.
make preview

# Build preview and create dist/RepoWiki-<version>.zip.
make build

# Build, validate, and install to ~/.agents/skills/RepoWiki.
make install

# Install below a different Skill collection directory.
make install INSTALL_DIR=/custom/agent/skills

# Remove generated build artifacts.
make clean

# Run offline replay, formatting, Rust tests, Clippy, and package validation.
make test

# Run the temporary-directory installation smoke test.
make test-install

# Run the optional differential check against the reference Python parser.
make test-reference
```

`INSTALL_DIR` is the parent directory for installed skills. The install target
always installs the package at `<INSTALL_DIR>/RepoWiki`. It stages and validates
the package before replacing that exact directory, so a failed update leaves an
existing installation intact. Existing installations under other names are not
modified.

`make build` creates a platform-specific binary:

- POSIX: `preview/scripts/codewiki`
- Windows: `preview/scripts/codewiki.exe`

Build the package on the operating system where it will run. An installed Skill
does not require Cargo.

## Offline replay and differential validation

`make test` does not need an LLM, MCP server, API key, or reference Python
dependencies. It replays the fixed transcript in
`tests/differential/transcript.json` against the current 13-language fixture,
then compares the result with `tests/golden/mini-repo.json`. It also extracts
the ZIP archive and runs the same replay against the installed package, so the
golden data, preview, and archive must agree.

The replay can also be run directly:

```bash
python3 tests/differential/run_replay.py \
  --preview-dir preview \
  --archive dist/RepoWiki-0.1.0.zip
```

Use `--update-golden` only when intentionally changing the runtime contract.

`make test-reference` is an additional differential gate against the pinned
reference implementation. It checks the original 11-language baseline; the
Rust and Go extensions are covered by the main replay and Rust analyzer tests.
Reference dependency or parser failures are reported as failures rather than
silently skipped.

## Installing the package manually

The one-command installation is preferred:

```bash
make install
```

To install an already-built archive manually, extract its flat contents into a
new `RepoWiki` directory:

```bash
make build
mkdir -p ~/.agents/skills/RepoWiki
unzip dist/RepoWiki-*.zip -d ~/.agents/skills/RepoWiki
```

After installation, the agent discovers `SKILL.md` and uses the bundled
platform-specific executable. Users do not need to invoke the CLI manually or
install Cargo on the target machine.

## Runtime package contents

The package contains exactly:

```text
SKILL.md
agents/openai.yaml
references/cli-contract.md
references/prompt-map.md
scripts/codewiki                  # or scripts/codewiki.exe
```

Rust prompt source files are embedded in the executable at build time. Wiki
generation produces `overview.md`, module Markdown pages, `module_tree.json`,
`metadata.json`, artifact indexes, and dependency graphs.

The `reference/CodeWiki` directory is used only for compatibility checks and
understanding the original workflow. It is not copied into the Skill package
and is not removed by `make clean`.

## 中文说明

RepoWiki 是一个用于生成仓库 Wiki 的 Agent Skill。它的用户可见名称是
`RepoWiki`，规范化的 Agent 触发名是 `$repo-wiki`。它使用 Rust CLI 完成仓库
分析、组件读取、模块树校验、安全文档写入、增量更新和 JSON 产物管理；由宿主
Agent 根据内置 prompt 生成 Markdown 内容。

最终 Skill 包只包含运行时文档、Agent 元数据、references 和当前平台的
`codewiki` 可执行文件。Cargo 源码只用于构建，不会进入 `preview/` 或最终 ZIP。

### 项目结构

目录职责与上面的英文说明一致：`engine/` 是构建输入，`skill/` 是可追踪的
运行时 Skill 源文件，`preview/` 和 `dist/` 是构建产物，`reference/CodeWiki/`
是只读参考实现。

### 常用命令

```bash
# 构建当前平台的可执行文件和运行时包目录
make preview

# 构建 preview，并生成 dist/RepoWiki-<version>.zip
make build

# 构建、校验，并安装到 ~/.agents/skills/RepoWiki
make install

# 安装到指定的 Skill 集合目录
make install INSTALL_DIR=/custom/agent/skills

# 删除构建产物
make clean

# 运行离线回放、格式检查、Rust 测试、Clippy 和包校验
make test

# 运行临时目录安装 smoke test
make test-install

# 运行可选的 reference Python parser 差分检查
make test-reference
```

`INSTALL_DIR` 表示已安装 skill 的父目录，安装目标固定写入
`<INSTALL_DIR>/RepoWiki`。安装会先暂存并校验新包，再替换这个目录；如果更新
失败，已有安装会被恢复。其他名称的已有安装不会被修改。

### 离线回放与差分校验

`make test` 不需要实时 LLM、MCP、API key 或 reference Python 依赖。它会使用
固定 fixture 和 transcript 回放公开 CLI 流程，与版本控制中的 golden 文件比较，
并将 ZIP 解压后再次运行相同回放，确保 golden、preview 和 ZIP 三者一致。

`make test-reference` 是额外的 reference differential gate。它校验 reference
已有的 11 种语言基线；Rust 和 Go 的扩展由主离线回放及 Rust analyzer 测试覆盖。

### 手动安装

推荐使用：

```bash
make install
```

也可以手动解压已经构建好的包：

```bash
make build
mkdir -p ~/.agents/skills/RepoWiki
unzip dist/RepoWiki-*.zip -d ~/.agents/skills/RepoWiki
```

安装后，Agent 会自动发现 `SKILL.md` 并使用包内当前平台的可执行文件。目标机器
不需要安装 Cargo，也不需要用户手动运行 CLI。

### 运行时产物

运行时包固定包含 `SKILL.md`、`agents/openai.yaml`、两个 references 文件和
`scripts/codewiki`（Windows 下为 `scripts/codewiki.exe`）。生成的 Wiki 关键产物
包括 `overview.md`、模块 Markdown 页面、`module_tree.json`、`metadata.json`、
artifact index 和 dependency graph。

`reference/CodeWiki` 只用于兼容性检查和理解原始流程，不会被复制到 Skill 包，
也不会被 `make clean` 删除。
