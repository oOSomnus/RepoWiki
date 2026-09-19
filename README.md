# CodeWiki Wiki Generator Skill

这是一个面向 Agent 的 CodeWiki Skill。它使用 Rust CLI 完成仓库分析、组件读取、模块树校验、文档安全写入、增量更新和 JSON 产物管理；由宿主 Agent 负责根据 prompt 生成 Markdown 内容。

最终 Skill 包只包含运行时所需的文档、Agent 元数据、references 和当前平台的 `codewiki` 可执行文件。Cargo 源码只用于构建，不会进入 `preview/` 或最终 ZIP。

## 项目结构

```text
.
├── Makefile                 # 唯一的 Skill 构建入口
├── README.md                # 项目与开发说明
├── engine/                  # Cargo 构建输入，不进入 Skill 包
│   ├── Cargo.toml
│   ├── Cargo.lock
│   ├── src/
│   ├── prompts/
│   └── tests/
├── skill/                   # 可追踪的运行时 Skill 源文件
│   ├── SKILL.md
│   ├── agents/openai.yaml
│   └── references/
├── preview/                 # make preview 生成的最终包内容
├── dist/                    # make build 生成的 ZIP
├── tools/                   # 开发期包校验工具
└── reference/CodeWiki/      # 只读参考实现，不参与构建
```

## Make 命令

需要 GNU Make、Rust/Cargo、Python 3 和 `zip` 命令。

```bash
# 使用当前环境的 Cargo 编译二进制，并生成运行时包目录
make preview

# 生成 preview，并创建 dist/codewiki-wiki-generator-<version>.zip
make build

# 删除 Cargo 构建目录、preview 和 dist
make clean

# 运行 Rust 格式检查、测试、Clippy、打包校验和离线 Skill 回放
make test

# 在离线回放通过后，再运行真实 reference Python parser 差分
make test-reference
```

`make build` 会根据当前构建环境生成平台专用二进制：

- POSIX 平台：`preview/scripts/codewiki`
- Windows：`preview/scripts/codewiki.exe`

因此在另一种操作系统上生成 Skill 包时，应在该操作系统上重新执行 `make build`。安装后的 Skill 运行不需要 Cargo。

## 离线回放与差分验证

`make test` 不需要实时 LLM、MCP、API key 或 reference Python 依赖。它会使用
`tests/differential/fixture/` 中覆盖当前 11 种语言的固定仓库和
`tests/differential/transcript.json`，通过公开 CLI 顺序回放：分析、prompt 获取、模块树保存与排序、组件读取、文档写入和 session close。
运行结果会经过稳定 canonicalization，与受版本控制的
`tests/golden/mini-repo.json` 比较；随后还会解压刚生成的 ZIP，用安装目录中的
`scripts/codewiki` 再跑一遍相同回放。因此 golden、preview 和 ZIP 三者必须同时一致。

也可以直接运行：

```bash
python3 tests/differential/run_replay.py \
  --preview-dir preview \
  --archive dist/codewiki-wiki-generator-0.1.0.zip
```

只有在有意接受新运行时契约变化时，才使用 `--update-golden` 更新 golden。

`make test-reference` 是额外的真实 reference differential gate。它调用
`tools/reference_probe.py` 加载 `reference/CodeWiki` 的 parser，并比较固定 fixture
的语言集合、文件数、组件 ID/类型/行号、叶节点和依赖边。reference 导入失败、依赖缺失或解析失败都会返回非零；不会把核心 Skill replay 静默标记为通过。
当前环境若缺少 reference 依赖，可按 reference 项目的安装说明安装后重新运行该 gate；
核心 `make test` 仍然必须独立通过。

## 安装 Skill

最终 ZIP 是扁平结构，需要解压到新建的 Skill 目录：

```bash
make build
mkdir -p ~/.codex/skills/codewiki-wiki-generator
unzip dist/codewiki-wiki-generator-*.zip \
  -d ~/.codex/skills/codewiki-wiki-generator
```

安装后，Agent 会自动发现 `SKILL.md`，并直接调用包内的当前平台二进制。用户不需要手动运行 CLI，也不需要在目标机器上安装 Cargo。

## 运行时产物

Skill 包只包含：

```text
SKILL.md
agents/openai.yaml
references/cli-contract.md
references/prompt-map.md
scripts/codewiki                  # 或 scripts/codewiki.exe
```

Rust prompt 源文件已经在编译阶段嵌入二进制。生成的 Wiki 关键产物包括 `overview.md`、模块 Markdown 页面、`module_tree.json`、`metadata.json`、artifact index 和 dependency graph。

## 测试与参考实现

Rust CLI 的测试位于 `engine/tests/`。`reference/CodeWiki` 只用于理解原始流程，不会被复制到 Skill 包，也不会被 `make clean` 删除。
