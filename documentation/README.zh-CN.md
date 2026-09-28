# RepoWiki

[English README](../README.md) · [开发说明](development.md) · [架构示例](../skill/references/few-shots/README.md)

RepoWiki 是一个用于生成仓库 Wiki 的 Agent Skill。它使用可移植的 Rust
分析器和运行时文档，让 Agent 能够分析仓库、整理模块，并写出对应的文档。

## 在 Agent Harness 中使用

在安装了本 Skill 的 Agent Harness 中，打开要分析的目标仓库，然后调用：

```text
/repo-wiki current repository
```

Skill 会分析当前工作目录，并将生成的 Wiki 写入 `.repowiki/`。然后可以
使用[独立 Reader](#独立-reader)在本地查看。

单独安装的 [Change Wiki Skill](../change-wiki/SKILL.md) 会生成不可变的变更版本。
调用 `/change-wiki <base-ref>...<head-ref>` 后，输出写入
`.repowiki/changes/<merge-base-full-SHA>..<head-full-SHA>/`；仓库版本保持不变，
可在独立 Reader 中切换版本查看。

RepoWiki 和 Change Wiki 保留原有的 Agent 工作流与 CLI 操作，但生成的页面 ID
和内容改用原生 DokuWiki 命名空间与语法，例如 `repo:system:api:start`、
`====== Heading ======` 和 `[[repo:system:api:start|API]]`。页面以原生 `.txt`
文件存放在 `.repowiki/dokuwiki/data/pages/` 下；每个变更版本独立存储页面。
旧 Markdown bundle 不向后兼容，必须使用更新后的 Skill 重新生成。

## 构建与安装

构建依赖：GNU Make、Rust/Cargo、Python 3，以及 `zip` 和 `unzip`。

运行时生成/校验 Wiki 和运行 Reader 需要启用 `mbstring` 与 `xml` 扩展的 PHP
8.2+。RepoWiki 与 Change Wiki 安装包内包含固定版本的 DokuWiki
（`release-2026-07-14c`，“Mort”）及 Mermaid 插件（`v11.15b`）源代码；运行时不会下载这些组件。

```bash
# 构建当前平台的可执行文件和 preview 包。
make preview

# 构建并校验可分发的 ZIP 包。
make build

# 交互选择安装 RepoWiki、Change Wiki 或两者。
make install

# 非交互选择安装内容（适用于脚本和 CI）。
make install INSTALL_SELECTION=repowiki
make install INSTALL_SELECTION=change-wiki
make install INSTALL_SELECTION=both

# 安装到指定的 Skill 集合目录。
make install INSTALL_DIR=/custom/agent/skills

# 删除生成的构建产物。
make clean
```

应在 Skill 最终运行的操作系统上构建。安装后的 Skill 使用包内的可执行文件，
目标机器不需要安装 Cargo。

## 验证改动

```bash
# 离线回放、Rust 检查和包校验。
make test

# 只运行本地架构提示词契约检查。
make test-contract

# 临时目录安装 smoke test。
make test-install

```

## 相关文档

- [英文 README](../README.md)
- [开发说明](development.md)
- [运行时 Skill 说明](../skill/SKILL.md)
- [参考项目架构 few-shot 示例](../skill/references/few-shots/README.md)

## 独立 Reader

构建并运行已有 `.repowiki` 目录的本地只读 WebUI：

```bash
make reader
.build/cargo-target/release/repowiki-reader /path/to/project/.repowiki
```

Reader 接口仍为 `repowiki-reader <.repowiki> [--port ...] [--no-open]`。
它需要系统 PHP 8.2+，并使用包内的 DokuWiki 引擎和插件源代码，不会在运行时
下载依赖。Reader 只监听本机、自动打开默认浏览器，并不会修改 Wiki。使用
`--no-open` 只打印 URL，或使用 `--port <端口>` 指定端口。

## 示例

下面的截图是 Codex 生成并通过独立 Reader 打开的一个示例；其他 Harness
也会生成相同的 `.repowiki` Reader 格式。

![通过独立 Reader 打开的 RepoWiki 示例](examples/codex_overview.png)
