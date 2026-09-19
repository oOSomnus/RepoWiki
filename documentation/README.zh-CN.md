# RepoWiki

[English README](../README.md) · [开发说明](development.md) · [参考实现配置](../reference/README.md)

RepoWiki 是一个用于生成仓库 Wiki 的 Agent Skill。它使用可移植的 Rust
分析器和运行时文档，让 Agent 能够分析仓库、整理模块，并写出对应的文档。

## 构建与安装

依赖：GNU Make、Rust/Cargo、Python 3，以及 `zip` 和 `unzip`。

```bash
# 构建当前平台的可执行文件和 preview 包。
make preview

# 构建并校验可分发的 ZIP 包。
make build

# 构建、校验并安装到 ~/.agents/skills/RepoWiki。
make install

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

# 临时目录安装 smoke test。
make test-install

# 可选的 reference parser 差分检查。
make test-reference
```

`make test-reference` 所需的 reference 环境见
[`reference/README.md`](../reference/README.md)。

## 相关文档

- [英文 README](../README.md)
- [开发说明](development.md)
- [运行时 Skill 说明](../skill/SKILL.md)
- [参考实现配置](../reference/README.md)
