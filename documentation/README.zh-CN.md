# RepoWiki

[English README](../README.md)

RepoWiki 是一个用于生成仓库 Wiki 的 Agent Skill。它使用可移植的 Rust
分析器和运行时文档，让 Agent 能够分析仓库、整理模块，并写出对应的文档。

## 在 Agent Harness 中使用

在 Agent Harness 中安装本 Skill，打开要分析的仓库，然后调用：

```text
/repo-wiki current repository
```

RepoWiki 会分析当前工作目录，并将生成的 DokuWiki 页面写入 `.repowiki/`。
Wiki 生成和 Reader 需要启用 `mbstring` 与 `xml` 扩展的 PHP 8.2+。

单独安装的 [Change Wiki Skill](../change-wiki/SKILL.md) 会生成独立的变更版本，
存放在 `.repowiki/changes/` 下。

构建、安装与验证说明见[开发指南](development.md#build-pipeline)。

## 独立 Reader

运行 `make reader`，即可构建并启动已有 `.repowiki` 目录的本地只读 Reader。
完整启动参数和行为见[Reader 说明](development.md#standalone-reader)。

## 示例

下面是通过独立 Reader 打开的 Wiki 示例。

![通过独立 Reader 打开的 RepoWiki 示例](examples/codex_overview.png)

## 相关文档

- [开发指南](development.md)
- [运行时 Skill 说明](../skill/SKILL.md)
- [架构示例](../skill/references/few-shots/README.md)

## 许可证

RepoWiki 原创内容采用 MIT 许可证，另有声明的文件或组件遵循各自许可证。
适用范围和组件说明见根目录 [LICENSE](../LICENSE)。
