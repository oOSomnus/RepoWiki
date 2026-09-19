#!/usr/bin/env python3
"""Compare the Skill analysis contract with the real reference parser.

The command is intentionally opt-in because the reference implementation has
its own Python dependency set.  It never skips that implementation: an import
or parser failure is a non-zero result with an actionable diagnostic.
"""

from __future__ import annotations

import argparse
import difflib
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
FIXTURE = ROOT / "tests" / "differential" / "fixture"
PROBE = Path(__file__).resolve().with_name("reference_probe.py")
EXPECTED_LANGUAGES = [
    "c",
    "cpp",
    "csharp",
    "java",
    "javascript",
    "kotlin",
    "php",
    "python",
    "ruby",
    "scala",
    "typescript",
]


class DifferentialFailure(RuntimeError):
    """A user-actionable differential test failure."""


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise DifferentialFailure(f"cannot read JSON {path}: {exc}") from exc


def canonical_json(value: Any) -> str:
    return json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n"


def write_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(canonical_json(value), encoding="utf-8")


def copy_reference_fixture(destination: Path) -> None:
    """Keep this opt-in comparison on the languages implemented by reference."""

    shutil.copytree(FIXTURE, destination)
    for language in ("go", "rust"):
        shutil.rmtree(destination / language, ignore_errors=True)


def semantic_tree(component_ids: list[str]) -> dict[str, Any]:
    """Build the same deterministic nested tree used by reference_probe.py."""

    ordered_ids = sorted(component_ids)
    midpoint = max(1, len(ordered_ids) // 2)
    return {
        "Root": {
            "path": ".",
            "components": ordered_ids,
            "children": {
                "Alpha": {
                    "path": "alpha",
                    "components": ordered_ids[:midpoint],
                    "children": {},
                },
                "Beta": {
                    "path": "beta",
                    "components": ordered_ids[midpoint:],
                    "children": {},
                },
            },
        }
    }


def overview_shape(value: dict[str, Any]) -> dict[str, Any]:
    """Keep only the deterministic shape shared with the reference probe."""

    targets: list[str] = []
    docs_paths: list[str] = []
    components_present = False

    def walk(modules: dict[str, Any], prefix: list[str]) -> None:
        nonlocal components_present
        for name, info in modules.items():
            if not isinstance(info, dict):
                continue
            path = prefix + [name]
            components_present = components_present or "components" in info
            if info.get("is_target_for_overview_generation"):
                targets.append("/".join(path))
            if "docs_path" in info:
                docs_paths.append("present" if info["docs_path"] else "missing")
            children = info.get("children", {})
            if isinstance(children, dict):
                walk(children, path)

    walk(value, [])
    return {
        "components_present": components_present,
        "target_modules": sorted(targets),
        "docs_paths": sorted(docs_paths),
    }


def skill_semantics(
    binary: Path,
    repo: Path,
    output: Path,
    session_id: str,
    component_ids: list[str],
) -> dict[str, Any]:
    """Probe the Skill's deterministic tree and overview behavior."""

    tree_file = output.parent / "semantic-tree.json"
    write_json(tree_file, semantic_tree(component_ids))
    run_json(
        [
            str(binary),
            "tree",
            "save",
            "--repo-root",
            str(repo),
            "--session",
            session_id,
            "--tree-file",
            str(tree_file),
            "--first",
        ],
        label="Skill semantic tree save",
    )
    order = run_json(
        [
            str(binary),
            "tree",
            "order",
            "--repo-root",
            str(repo),
            "--session",
            session_id,
        ],
        label="Skill semantic tree order",
    )["processing_order"]

    target_file = output.parent / "semantic-target.json"
    write_json(target_file, ["Root"])
    context_file = output.parent / "semantic-overview-context.json"
    for name in ("Alpha", "Beta"):
        (output / f"{name}.md").write_text(f"# {name}\n", encoding="utf-8")
    run_json(
        [
            str(binary),
            "tree",
            "overview-context",
            "--repo-root",
            str(repo),
            "--session",
            session_id,
            "--tree-file",
            str(tree_file),
            "--target-path-file",
            str(target_file),
            "--output-file",
            str(context_file),
        ],
        label="Skill semantic overview context",
    )
    context_present = load_json(context_file)
    for name in ("Alpha", "Beta"):
        (output / f"{name}.md").unlink()
    run_json(
        [
            str(binary),
            "tree",
            "overview-context",
            "--repo-root",
            str(repo),
            "--session",
            session_id,
            "--tree-file",
            str(tree_file),
            "--target-path-file",
            str(target_file),
            "--output-file",
            str(context_file),
        ],
        label="Skill semantic overview context without pages",
    )
    context_missing = load_json(context_file)
    return {
        "tree": {
            "processing_order": [
                {
                    "module": item["module"],
                    "path": item["path"],
                    "is_leaf": item["is_leaf"],
                }
                for item in order
            ]
        },
        "overview": {
            "missing_child_pages": overview_shape(context_missing),
            "present_child_pages": overview_shape(context_present),
        },
    }


def resolve_binary(location: Path) -> Path:
    if location.is_file():
        return location.resolve()
    for name in ("codewiki", "codewiki.exe"):
        candidate = location / "scripts" / name
        if candidate.is_file():
            return candidate.resolve()
    raise DifferentialFailure(f"Skill binary not found under {location}")


def run_json(command: list[str], *, label: str) -> dict[str, Any]:
    completed = subprocess.run(command, text=True, capture_output=True, check=False)
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip()
        raise DifferentialFailure(
            f"{label} failed ({completed.returncode}): {' '.join(command)}\n{detail}"
        )
    try:
        result = json.loads(completed.stdout)
    except json.JSONDecodeError as exc:
        raise DifferentialFailure(
            f"{label} returned non-JSON output:\n{completed.stdout}"
        ) from exc
    if not isinstance(result, dict) or result.get("ok") is False:
        raise DifferentialFailure(f"{label} returned an error:\n{completed.stdout}")
    return result


def skill_contract(binary: Path, repo: Path, output: Path) -> dict[str, Any]:
    analysis = run_json(
        [str(binary), "generate", "--repo", str(repo), "--output", str(output)],
        label="Skill generate",
    )
    component_index = load_json(Path(analysis["component_index_path"]))
    graph = load_json(Path(analysis["graph_path"]))
    leaf_nodes = load_json(Path(analysis["leaf_nodes_path"]))
    components = []
    for item in component_index:
        node = graph[item["id"]]
        components.append(
            {
                "id": item["id"],
                "name": item["name"],
                "component_type": item["component_type"],
                "relative_path": item["relative_path"],
                "language": item["language"],
                "start_line": item["start_line"],
                "end_line": item["end_line"],
                "depends_on": sorted(node["depends_on"]),
            }
        )
    components.sort(key=lambda item: item["id"])
    summary = analysis["summary"]
    analysis_contract = {
        "summary": {
            "languages": sorted(summary["languages"]),
            "supported_files": summary["supported_files"],
            "total_components": summary["total_components"],
        },
        "components": components,
        "leaf_nodes": sorted(leaf_nodes),
        "dependencies": {item["id"]: item["depends_on"] for item in components},
    }
    if analysis_contract["summary"]["languages"] != EXPECTED_LANGUAGES:
        raise DifferentialFailure(
            f"Skill language coverage mismatch: expected {EXPECTED_LANGUAGES}, "
            f"got {analysis_contract['summary']['languages']}"
        )
    return {
        "analysis": analysis_contract,
        "semantics": skill_semantics(
            binary,
            repo,
            output,
            str(analysis["session_id"]),
            [item["id"] for item in components],
        ),
    }


def reference_contract(reference_root: Path, repo: Path) -> dict[str, Any]:
    completed = subprocess.run(
        [
            sys.executable,
            str(PROBE),
            "--reference-root",
            str(reference_root),
            "--repo",
            str(repo),
        ],
        text=True,
        capture_output=True,
        check=False,
    )
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip()
        raise DifferentialFailure(
            "reference differential did not run to completion "
            f"(exit {completed.returncode}):\n{detail}"
        )
    try:
        result = json.loads(completed.stdout)
    except json.JSONDecodeError as exc:
        raise DifferentialFailure(
            f"reference adapter returned non-JSON output:\n{completed.stdout}"
        ) from exc
    if not isinstance(result, dict):
        raise DifferentialFailure("reference adapter returned a non-object result")
    return result


def compare(skill: dict[str, Any], reference: dict[str, Any]) -> None:
    if skill == reference:
        return
    diff = "".join(
        difflib.unified_diff(
            canonical_json(reference).splitlines(keepends=True),
            canonical_json(skill).splitlines(keepends=True),
            fromfile="reference",
            tofile="skill",
        )
    )
    raise DifferentialFailure(f"Skill/reference analysis contract differs:\n{diff}")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, help="preview binary")
    parser.add_argument("--preview-dir", type=Path, help="preview directory containing scripts/")
    parser.add_argument("--reference-root", type=Path, required=True)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        binary = resolve_binary(args.binary or args.preview_dir or ROOT / "preview")
        reference_root = args.reference_root.resolve()
        if not reference_root.is_dir():
            raise DifferentialFailure(f"reference root does not exist: {reference_root}")
        with tempfile.TemporaryDirectory(prefix="codewiki-reference-diff-") as temporary:
            temporary_root = Path(temporary)
            skill_repo = temporary_root / "skill-repo"
            reference_repo = temporary_root / "reference-repo"
            copy_reference_fixture(skill_repo)
            copy_reference_fixture(reference_repo)
            skill = skill_contract(binary, skill_repo, temporary_root / "skill-output")
            reference = reference_contract(reference_root, reference_repo)
            compare(skill, reference)
        print(
            "PASS reference differential: "
            f"{len(EXPECTED_LANGUAGES)} languages, "
            f"{skill['analysis']['summary']['total_components']} components, "
            "analysis/tree/overview contracts"
        )
    except DifferentialFailure as exc:
        print(f"REFERENCE DIFFERENTIAL FAILED: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
