#!/usr/bin/env python3
"""Check the architecture-reading prompt contract.

RepoWiki is intentionally not a compatibility implementation of the pinned
reference host. The reference material is used as a small set of few-shot
architecture examples; this gate checks the current prompt contract and the
examples shipped with the Skill instead of comparing prompt implementations.
"""

from __future__ import annotations

from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
CURRENT = ROOT / "engine" / "prompts"
FEW_SHOTS = ROOT / "skill" / "references" / "few-shots"


class PromptContractFailure(RuntimeError):
    pass


def require(text: str, fragments: list[str], label: str) -> None:
    missing = [fragment for fragment in fragments if fragment not in text]
    if missing:
        raise PromptContractFailure(f"{label} is missing: {missing}")


def main() -> int:
    current = {
        path.name: path.read_text(encoding="utf-8")
        for path in CURRENT.glob("*.txt")
    }
    required_current = {
        "cluster_repo.txt": [
            "architecture map",
            "representative component IDs",
            "not a directory classification task",
            "not an exhaustive partition",
            "<GROUPED_COMPONENTS>",
        ],
        "cluster_module.txt": [
            "existing module",
            "representative component IDs",
            "Keep the tree\nshallow",
            "directory-shaped child",
            "<GROUPED_COMPONENTS>",
        ],
        "super_group.txt": [
            "<MODULES>",
            "<GROUPED_MODULES>",
            "Preserve the existing module pages",
        ],
        "user.txt": [
            "<MODULE_TREE>",
            "<ARCHITECTURE_CONTEXT>",
            "<ARCHITECTURE_FEW_SHOTS>",
            "source-grounded architecture page",
            "Caller -> Entry -> Core -> Result",
        ],
        "overview_module.txt": [
            "<REPO_STRUCTURE>",
            "<ARCHITECTURE_CONTEXT>",
            "<ARCHITECTURE_FEW_SHOTS>",
            "every linked immediate child page",
            "Caller -> Entry -> Core -> Result",
        ],
        "overview_repo.txt": [
            "<REPO_STRUCTURE>",
            "<ARCHITECTURE_CONTEXT>",
            "<ARCHITECTURE_FEW_SHOTS>",
            "primary Mermaid architecture diagram",
            "end-to-end path",
            "generic placeholders",
        ],
        "system_leaf.txt": [
            "architecture documentation writer",
            "codewiki doc write",
            "Mermaid",
            "several source-grounded prose paragraphs",
            "quality floor",
        ],
        "system_complex.txt": [
            "architecture documentation writer",
            "already-selected architecture module",
            "codewiki doc write",
            "selected components form one module",
            "several source-grounded prose paragraphs",
            "fixed template",
        ],
        "filter_folders.txt": ["relative paths", "shortlist", "JSON format"],
        "update_leaf_user.txt": [
            "<WRITE_SET>",
            "<CHANGE_REPORT>",
            "<LEAF_COMPONENTS>",
            "verdict",
        ],
        "routing_system.txt": ["existing module tree", "JSON only"],
        "routing_user.txt": ["<MODULE_TREE>", "<ORPHANS>", "decisions"],
        "stale_fix_system.txt": ["stale references", "verdicts"],
        "stale_fix_user.txt": [
            "<STALE_ITEMS>",
            "codewiki doc view",
            "codewiki doc edit",
        ],
    }
    expected_prompt_files = {
        "artifact_usage.txt",
        "cluster_module.txt",
        "cluster_repo.txt",
        "code_truncated.txt",
        "filter_folders.txt",
        "module_tree_trimmed.txt",
        "overview_artifact_addendum.txt",
        "overview_module.txt",
        "overview_repo.txt",
        "routing_system.txt",
        "routing_user.txt",
        "stale_fix_system.txt",
        "stale_fix_user.txt",
        "super_group.txt",
        "system_complex.txt",
        "system_leaf.txt",
        "update_leaf_system.txt",
        "update_leaf_user.txt",
        "user.txt",
    }
    if set(current) != expected_prompt_files:
        raise PromptContractFailure(
            "current prompt catalog differs: "
            f"expected {sorted(expected_prompt_files)}, got {sorted(current)}"
        )
    for name, fragments in required_current.items():
        text = current.get(name)
        if text is None:
            raise PromptContractFailure(f"missing current prompt source {name}")
        require(text, fragments, f"current/{name}")
        for legacy in (
            "str_replace_editor",
            "read_code_components",
            "generate_sub_module_documentation",
        ):
            if legacy in text:
                raise PromptContractFailure(f"current/{name} contains legacy tool {legacy}")

    expected_few_shots = {
        "README.md",
        "clickhouse-overview.md",
        "clickhouse-storage-engine.md",
        "clickhouse-query-pipeline.md",
        "clickhouse-ast-create-query.md",
    }
    actual_few_shots = {path.name for path in FEW_SHOTS.iterdir() if path.is_file()}
    if actual_few_shots != expected_few_shots:
        raise PromptContractFailure(
            "architecture few-shot catalog differs: "
            f"expected {sorted(expected_few_shots)}, got {sorted(actual_few_shots)}"
        )
    require(
        (FEW_SHOTS / "README.md").read_text(encoding="utf-8"),
        ["clickhouse-overview.md", "clickhouse-storage-engine.md"],
        "few-shots/README.md",
    )
    for path in FEW_SHOTS.glob("*.md"):
        if path.name == "README.md":
            continue
        text = path.read_text(encoding="utf-8")
        require(text, ["architecture", "mermaid"], f"few-shots/{path.name}")

    print(
        "PASS architecture prompt contract: "
        f"{len(required_current)}/{len(current)} current prompts, "
        f"{len(expected_few_shots) - 1} reference-derived few shots"
    )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, PromptContractFailure) as exc:
        print(f"PROMPT CONTRACT FAILED: {exc}")
        raise SystemExit(1)
