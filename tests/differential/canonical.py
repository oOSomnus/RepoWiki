"""Canonical output helpers for the offline CodeWiki replay tests.

The runtime deliberately emits session IDs, absolute paths, timestamps, and
other run-local values.  This module removes only those documented volatile
values; component IDs, languages, dependency edges, module order, and page
contents remain exact.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any


def replace_paths(value: Any, replacements: list[tuple[str, str]]) -> Any:
    """Replace run-local path prefixes recursively without changing shapes."""

    if isinstance(value, dict):
        return {key: replace_paths(item, replacements) for key, item in value.items()}
    if isinstance(value, list):
        return [replace_paths(item, replacements) for item in value]
    if isinstance(value, str):
        result = value
        for source, target in sorted(replacements, key=lambda item: len(item[0]), reverse=True):
            result = result.replace(source, target)
        return result
    return value


def canonical_json(value: Any) -> str:
    """Serialize a value for stable diffs and golden files."""

    return json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n"


def canonical_analysis(
    analysis: dict[str, Any],
    component_index: list[dict[str, Any]],
    leaf_nodes: list[str],
    graph: dict[str, dict[str, Any]],
    repo_root: Path,
    output_root: Path,
    session_root: Path,
) -> dict[str, Any]:
    """Keep the cross-run analysis contract and discard only run-local paths."""

    replacements = [
        (str(session_root.resolve()), "<session>"),
        (str(output_root.resolve()), "<output>"),
        (str(repo_root.resolve()), "<repo>"),
    ]

    summary = replace_paths(analysis["summary"], replacements)
    components = replace_paths(component_index, replacements)
    graph = replace_paths(graph, replacements)
    return {
        "summary": {
            "total_components": summary["total_components"],
            "leaf_nodes": summary["leaf_nodes"],
            "max_depth": summary["max_depth"],
            "max_token_per_module": summary["max_token_per_module"],
            "max_token_per_leaf_module": summary["max_token_per_leaf_module"],
            "cluster_batch_size": summary["cluster_batch_size"],
            "supported_files": summary["supported_files"],
            "languages": sorted(summary["languages"]),
            "warnings": summary["warnings"],
        },
        "components": sorted(components, key=lambda item: item["id"]),
        "leaf_nodes": sorted(leaf_nodes),
        "graph": {key: graph[key] for key in sorted(graph)},
    }


def canonical_metadata(metadata: dict[str, Any], repo_root: Path, output_root: Path) -> dict[str, Any]:
    """Remove only metadata fields that are inherently run-local."""

    result = replace_paths(
        metadata,
        [
            (str(repo_root.resolve()), "<repo>"),
            (str(output_root.resolve()), "<output>"),
        ],
    )
    generation_info = dict(result["generation_info"])
    generation_info.pop("timestamp", None)
    result["generation_info"] = generation_info
    return result


def canonical_tree(tree: dict[str, Any]) -> dict[str, Any]:
    """Return the JSON tree with recursively sorted object keys."""

    return json.loads(canonical_json(tree))
