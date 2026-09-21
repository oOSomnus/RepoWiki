#!/usr/bin/env python3
"""Run the deterministic host-agent replay against a Skill binary.

This test intentionally uses only the standard library.  It exercises the
installed runtime contract through subprocesses and a fixed transcript; it
does not import the reference Python implementation, call an LLM, or use
MCP.  The optional ZIP path is extracted into a temporary Skill directory and
run separately from the preview directory.
"""

from __future__ import annotations

import argparse
import difflib
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path
from typing import Any

try:
    from canonical import canonical_analysis, canonical_json, canonical_metadata, canonical_tree
except ImportError:  # pragma: no cover - supports ``python -m`` execution
    from tests.differential.canonical import (  # type: ignore[no-redef]
        canonical_analysis,
        canonical_json,
        canonical_metadata,
        canonical_tree,
    )


ROOT = Path(__file__).resolve().parents[2]
FIXTURE = Path(__file__).resolve().parent / "fixture"
TRANSCRIPT_PATH = Path(__file__).resolve().parent / "transcript.json"
GOLDEN_PATH = Path(__file__).resolve().parent.parent / "golden" / "mini-repo.json"
FEW_SHOT_FILES = [
    ROOT / "skill" / "references" / "few-shots" / "clickhouse-overview.md",
    ROOT / "skill" / "references" / "few-shots" / "clickhouse-query-pipeline.md",
]
FEW_SHOT_EXAMPLES = "\n\n".join(
    path.read_text(encoding="utf-8") for path in FEW_SHOT_FILES
)
LANGUAGES = [
    "c",
    "cpp",
    "csharp",
    "go",
    "java",
    "javascript",
    "kotlin",
    "php",
    "python",
    "ruby",
    "rust",
    "scala",
    "typescript",
]
RESERVED_DOCUMENT_STEMS = {
    "overview",
    "module_tree",
    "first_module_tree",
    "metadata",
    "index",
}


class ReplayFailure(RuntimeError):
    """A user-actionable failure in the public Skill contract."""


def load_json(path: Path) -> Any:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ReplayFailure(f"cannot read JSON {path}: {exc}") from exc


def write_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(canonical_json(value), encoding="utf-8")


def run_command(binary: Path, args: list[str], *, cwd: Path | None = None) -> dict[str, Any]:
    command = [str(binary), *args]
    try:
        completed = subprocess.run(
            command,
            cwd=cwd,
            text=True,
            capture_output=True,
            check=False,
        )
    except OSError as exc:
        raise ReplayFailure(f"cannot execute {' '.join(command)}: {exc}") from exc
    if completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip()
        raise ReplayFailure(
            f"command failed ({completed.returncode}): {' '.join(command)}\n{detail}"
        )
    try:
        value = json.loads(completed.stdout)
    except json.JSONDecodeError as exc:
        raise ReplayFailure(
            f"command did not return JSON: {' '.join(command)}\n{completed.stdout}"
        ) from exc
    if not isinstance(value, dict) or value.get("ok") is False:
        raise ReplayFailure(f"command returned an error: {' '.join(command)}\n{completed.stdout}")
    return value


def resolve_binary(location: Path) -> Path:
    if location.is_file():
        return location.resolve()
    for name in ("codewiki", "codewiki.exe"):
        candidate = location / "scripts" / name
        if candidate.is_file():
            return candidate.resolve()
    raise ReplayFailure(f"Skill binary not found under {location}")


def extract_archive(archive: Path, destination: Path) -> Path:
    if not archive.is_file():
        raise ReplayFailure(f"Skill archive does not exist: {archive}")
    with zipfile.ZipFile(archive) as package:
        members = package.infolist()
        for member in members:
            name = member.filename
            path = Path(name)
            if path.is_absolute() or ".." in path.parts:
                raise ReplayFailure(f"unsafe archive member: {name}")
            package.extract(member, destination)
            extracted = destination / path
            # Python's ZipFile.extract() intentionally does not restore Unix
            # mode bits.  Preserve the executable bit recorded by `zip` so
            # this is a real installed-binary smoke, not just a file check.
            mode = (member.external_attr >> 16) & 0o777
            if mode and extracted.is_file():
                extracted.chmod(mode)
    binary = resolve_binary(destination)
    if os.name != "nt" and not os.access(binary, os.X_OK):
        raise ReplayFailure(
            f"installed Skill binary is not executable after ZIP extraction: {binary}; "
            "the archive must preserve the executable mode"
        )
    return binary


def prompt_vars(
    prompt_type: str,
    tree: dict[str, Any],
    module_name: str,
    source_text: str,
    architecture_context: str = "",
) -> dict[str, Any]:
    tree_text = json.dumps(tree, ensure_ascii=False, indent=2, sort_keys=True)
    if prompt_type == "cluster":
        return {"scope": "repo", "potential_core_components": source_text}
    if prompt_type == "system_leaf":
        return {
            "module_name": module_name,
            "doc_path": document_path_for(module_name),
            "custom_instructions": "offline replay",
            "few_shot_examples": FEW_SHOT_EXAMPLES,
        }
    if prompt_type == "user":
        return {
            "module_name": module_name,
            "module_tree": tree_text,
            "formatted_core_component_codes": source_text,
            "few_shot_examples": FEW_SHOT_EXAMPLES,
            "architecture_context": architecture_context,
        }
    if prompt_type == "overview_module":
        return {
            "module_name": module_name,
            "repo_structure": source_text,
            "few_shot_examples": FEW_SHOT_EXAMPLES,
            "architecture_context": architecture_context,
        }
    if prompt_type == "overview_repo":
        return {
            "repo_name": "replay-repo",
            "repo_structure": source_text or tree_text,
            "few_shot_examples": FEW_SHOT_EXAMPLES,
            "architecture_context": architecture_context,
        }
    raise ReplayFailure(f"unsupported replay prompt type: {prompt_type}")


def get_prompt(
    binary: Path,
    repo: Path,
    session_id: str,
    prompt_type: str,
    variables: dict[str, Any],
    work: Path,
) -> str:
    variables_path = work / f"{prompt_type}-{len(list(work.glob('*.json')))}.json"
    write_json(variables_path, variables)
    result = run_command(
        binary,
        [
            "prompt",
            "get",
            "--repo-root",
            str(repo),
            "--session",
            session_id,
            "--type",
            prompt_type,
            "--vars-file",
            str(variables_path),
        ],
    )
    path = Path(result["path"])
    if not path.is_file():
        raise ReplayFailure(f"prompt path does not exist: {path}")
    prompt = path.read_text(encoding="utf-8")
    actual_sha256 = hashlib.sha256(prompt.encode("utf-8")).hexdigest()
    if result.get("sha256") != actual_sha256:
        raise ReplayFailure(
            f"prompt hash mismatch for {prompt_type}: CLI returned {result.get('sha256')}, "
            f"file hashes to {actual_sha256}"
        )
    # Overview prompts intentionally contain absolute child-page paths so the
    # host can open them.  Those paths point into this replay's random
    # TemporaryDirectory and must not make the checked-in golden nondeterministic.
    stable_prompt = prompt.replace(str(repo.parent.resolve()), "<replay-root>")
    return hashlib.sha256(stable_prompt.encode("utf-8")).hexdigest()


def assert_equal(label: str, actual: Any, expected: Any) -> None:
    if actual == expected:
        return
    actual_text = canonical_json(actual)
    expected_text = canonical_json(expected)
    diff = "".join(
        difflib.unified_diff(
            expected_text.splitlines(keepends=True),
            actual_text.splitlines(keepends=True),
            fromfile=f"expected/{label}",
            tofile=f"actual/{label}",
        )
    )
    raise ReplayFailure(f"{label} differs:\n{diff}")


def expected_component_ids(tree: dict[str, Any]) -> list[str]:
    ids: set[str] = set()

    def visit(modules: dict[str, Any]) -> None:
        for module in modules.values():
            ids.update(module.get("components", []))
            visit(module.get("children", {}))

    visit(tree)
    return sorted(ids)


def document_path_for(module_name: str) -> str:
    """Mirror the documented module-page naming convention for new runtimes."""

    if not module_name or any(
        not (character.isascii() and (character.isalnum() or character in "_-"))
        for character in module_name
    ):
        raise ReplayFailure(f"replay module name is not page-safe: {module_name}")
    stem = "".join(
        character
        if character.isascii() and (character.isalnum() or character in "_-")
        else "_"
        for character in module_name
    ) or "module"
    if stem in RESERVED_DOCUMENT_STEMS:
        stem = f"{stem}_module"
    return f"{stem}.md"


def execute_replay(binary: Path, transcript: dict[str, Any], root: Path) -> dict[str, Any]:
    repo = root / "repo"
    output = root / "output"
    shutil.copytree(FIXTURE, repo)
    output.mkdir()
    work = root / "work"
    work.mkdir()

    analysis = run_command(
        binary,
        [
            "generate",
            "--repo",
            str(repo),
            "--output",
            str(output),
            "--max-depth",
            "3",
        ],
    )
    session_id = str(analysis["session_id"])
    session = Path(analysis["session_path"])
    if not session.is_dir():
        raise ReplayFailure(f"analysis session does not exist: {session}")

    try:
        summary = analysis["summary"]
        if sorted(summary["languages"]) != LANGUAGES:
            raise ReplayFailure(
                f"13-language coverage mismatch: expected {LANGUAGES}, got {summary['languages']}"
            )
        if summary["supported_files"] != len(LANGUAGES):
            raise ReplayFailure(
                f"ignored-file filtering mismatch: expected {len(LANGUAGES)} supported files, "
                f"got {summary['supported_files']}"
            )
        if summary["total_components"] != len(LANGUAGES):
            raise ReplayFailure(
                f"fixture component count mismatch: expected {len(LANGUAGES)}, "
                f"got {summary['total_components']}"
            )

        component_index = load_json(Path(analysis["component_index_path"]))
        leaf_nodes = load_json(Path(analysis["leaf_nodes_path"]))
        graph = load_json(Path(analysis["graph_path"]))
        selected_ids = expected_component_ids(transcript["module_tree"])
        analyzed_ids = sorted(item["id"] for item in component_index)
        if not set(selected_ids).issubset(analyzed_ids):
            raise ReplayFailure("transcript module tree contains an unknown architecture anchor")
        if len(selected_ids) >= len(analyzed_ids):
            raise ReplayFailure(
                "replay fixture must leave low-level analysis candidates outside the architecture tree"
            )
        if "MustNotBeAnalyzed" in json.dumps(component_index):
            raise ReplayFailure(".gitignore fixture leaked into the component index")

        analysis_contract = canonical_analysis(
            analysis,
            component_index,
            leaf_nodes,
            graph,
            repo,
            output,
            session,
        )

        prompt_hashes: dict[str, list[str]] = {
            "cluster": [],
            "system_leaf": [],
            "user": [],
            "overview_module": [],
        }
        prompt_hashes["cluster"].append(
            get_prompt(
                binary,
                repo,
                session_id,
                "cluster",
                prompt_vars(
                    "cluster", transcript["module_tree"], "Repository", "fixed cluster input"
                ),
                work,
            )
        )

        tree_path = work / "module-tree.json"
        write_json(tree_path, transcript["module_tree"])
        saved = run_command(
            binary,
            [
                "tree",
                "save",
                "--repo-root",
                str(repo),
                "--session",
                session_id,
                "--tree-file",
                str(tree_path),
                "--first",
            ],
        )
        save_result = saved["result"]
        if save_result["unmatched_architecture_ids"]:
            raise ReplayFailure(f"transcript tree has unmatched IDs: {save_result}")

        ids_path = work / "component-ids.json"
        write_json(ids_path, expected_component_ids(transcript["module_tree"]))
        components_result = run_command(
            binary,
            [
                "components",
                "read",
                "--repo-root",
                str(repo),
                "--session",
                session_id,
                "--ids-file",
                str(ids_path),
            ],
        )
        source_by_id: dict[str, str] = {}
        for item in components_result["components"]:
            source_path = Path(item["path"])
            if not source_path.is_file():
                raise ReplayFailure(f"component source path does not exist: {source_path}")
            source_by_id[item["id"]] = source_path.read_text(encoding="utf-8")

        ordered = run_command(
            binary,
            ["tree", "order", "--repo-root", str(repo), "--session", session_id],
        )["processing_order"]
        expected_documents = transcript["documents"]
        for item in ordered:
            module_name = item.get("module_name", item.get("module"))
            if not module_name:
                raise ReplayFailure(f"processing order item has no module name: {item}")
            doc_path = item.get("doc_path", document_path_for(module_name))
            if item["is_leaf"]:
                ids = item["components"]
                source_text = "\n".join(source_by_id[component_id] for component_id in ids)
                variables = prompt_vars(
                    "system_leaf", transcript["module_tree"], module_name, source_text
                )
                prompt_hashes["system_leaf"].append(
                    get_prompt(binary, repo, session_id, "system_leaf", variables, work)
                )
                variables = prompt_vars(
                    "user", transcript["module_tree"], module_name, source_text
                )
                prompt_hashes["user"].append(
                    get_prompt(binary, repo, session_id, "user", variables, work)
                )
            else:
                target_path = work / f"{module_name}-target.json"
                write_json(target_path, item["path"])
                context_path = work / f"{module_name}-overview-context.json"
                run_command(
                    binary,
                    [
                        "tree",
                        "overview-context",
                        "--repo-root",
                        str(repo),
                        "--session",
                        session_id,
                        "--tree-file",
                        str(tree_path),
                        "--target-path-file",
                        str(target_path),
                        "--output-file",
                        str(context_path),
                    ],
                )
                context_value = load_json(context_path)
                context_text = canonical_json(context_value.get("repo_structure", context_value))
                architecture_text = canonical_json(
                    context_value.get("architecture_context", {})
                )
                variables = prompt_vars(
                    "overview_module",
                    transcript["module_tree"],
                    module_name,
                    context_text,
                    architecture_text,
                )
                prompt_hashes["overview_module"].append(
                    get_prompt(binary, repo, session_id, "overview_module", variables, work)
                )
            if module_name not in expected_documents:
                raise ReplayFailure(f"transcript has no document for module {module_name}")
            content_path = work / f"{doc_path}.content"
            content_path.write_text(expected_documents[module_name], encoding="utf-8")
            run_command(
                binary,
                [
                    "doc",
                    "write",
                    "--repo-root",
                    str(repo),
                    "--session",
                    session_id,
                    "--path",
                    doc_path,
                    "--content-file",
                    str(content_path),
                ],
            )

        repo_context_path = work / "repo-overview-context.json"
        run_command(
            binary,
            [
                "tree",
                "overview-context",
                "--repo-root",
                str(repo),
                "--session",
                session_id,
                "--tree-file",
                str(tree_path),
                "--output-file",
                str(repo_context_path),
            ],
        )
        repo_context_value = load_json(repo_context_path)
        repo_context = canonical_json(repo_context_value.get("repo_structure", repo_context_value))
        repo_architecture_context = canonical_json(
            repo_context_value.get("architecture_context", {})
        )
        prompt_hashes["overview_repo"] = [
            get_prompt(
                binary,
                repo,
                session_id,
                    "overview_repo",
                    prompt_vars(
                        "overview_repo",
                        transcript["module_tree"],
                        "Repository",
                        repo_context,
                        repo_architecture_context,
                    ),
                work,
            )
        ]
        overview_path = work / "overview.md.content"
        overview_path.write_text(transcript["overview"], encoding="utf-8")
        run_command(
            binary,
            [
                "doc",
                "write",
                "--repo-root",
                str(repo),
                "--session",
                session_id,
                "--path",
                "overview.md",
                "--content-file",
                str(overview_path),
            ],
        )

        validation = load_json(session / "module_tree_validation.json")
        processing_order = load_json(session / "processing_order.json")
        closed = run_command(
            binary,
            [
                "session",
                "close",
                "--repo-root",
                str(repo),
                "--session",
                session_id,
                "--model",
                "replay-model",
            ],
        )
        if session.exists():
            raise ReplayFailure(f"session was not cleaned after close: {session}")
        metadata = load_json(output / "metadata.json")
        documents = {
            path.relative_to(output).as_posix(): path.read_text(encoding="utf-8")
            for path in output.rglob("*.md")
        }
        output_files = sorted(
            path.relative_to(output).as_posix()
            for path in output.rglob("*")
            if path.is_file()
        )
        replay = {
            "analysis": analysis_contract,
            "module_tree": canonical_tree(load_json(output / "module_tree.json")),
            "first_module_tree": canonical_tree(load_json(output / "first_module_tree.json")),
            "processing_order": processing_order,
            "validation": {
                "valid": validation["valid"],
                "complete": validation["complete"],
                "quality_valid": validation["quality_valid"],
                "unmatched_architecture_ids": validation["unmatched_architecture_ids"],
                "omitted_analysis_candidate_ids": validation[
                    "omitted_analysis_candidate_ids"
                ],
                "architecture_anchor_count": validation["architecture_anchor_count"],
                "module_count": validation["module_count"],
                "leaf_count": validation["leaf_count"],
                "max_depth": validation["max_depth"],
                "orphaned_candidate_ids": validation["orphaned_candidate_ids"],
                "oversized_leaf_modules": validation["oversized_leaf_modules"],
                "tree_relationship_errors": validation["tree_relationship_errors"],
            },
            "prompt_hashes": prompt_hashes,
            "documents": documents,
            "metadata": canonical_metadata(metadata, repo, output),
            "output_files": output_files,
            "close": {
                "cleaned": closed["cleaned"],
                "metadata_present": closed["metadata"] is not None,
            },
        }
        return replay
    finally:
        if session.exists():
            try:
                run_command(
                    binary,
                    [
                        "session",
                        "close",
                        "--repo-root",
                        str(repo),
                        "--session",
                        session_id,
                        "--model",
                        "replay-cleanup",
                    ],
                )
            except ReplayFailure:
                pass


def run(binary: Path, archive: Path | None, update_golden: bool) -> None:
    transcript = load_json(TRANSCRIPT_PATH)
    with tempfile.TemporaryDirectory(prefix="codewiki-replay-") as temporary:
        temporary_root = Path(temporary)
        preview_result = execute_replay(binary, transcript, temporary_root / "preview-run")

        if update_golden:
            GOLDEN_PATH.parent.mkdir(parents=True, exist_ok=True)
            GOLDEN_PATH.write_text(canonical_json(preview_result), encoding="utf-8")
            print(f"updated golden: {GOLDEN_PATH}")
        elif not GOLDEN_PATH.is_file():
            raise ReplayFailure(
                f"golden file is missing: {GOLDEN_PATH}; use --update-golden intentionally"
            )

        expected = load_json(GOLDEN_PATH)
        assert_equal("preview replay", preview_result, expected)
        print(
            "PASS Skill replay: "
            f"{len(LANGUAGES)} languages, {preview_result['analysis']['summary']['total_components']} "
            "components, fixed transcript, cleaned session"
        )

        if archive is None:
            return
        install_root = temporary_root / "installed-skill"
        installed_binary = extract_archive(archive, install_root)
        version = run_command(installed_binary, ["version"])
        if version.get("version") != "0.1.0":
            raise ReplayFailure(f"unexpected installed binary version: {version}")
        installed_result = execute_replay(
            installed_binary, transcript, temporary_root / "installed-run"
        )
        assert_equal("installed replay vs golden", installed_result, expected)
        assert_equal("preview vs installed replay", installed_result, preview_result)
        print(f"PASS ZIP install smoke: {archive} -> {installed_binary}")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, help="preview binary or its preview directory")
    parser.add_argument("--preview-dir", type=Path, help="preview directory containing scripts/")
    parser.add_argument("--archive", type=Path, help="Skill ZIP to extract and execute")
    parser.add_argument(
        "--update-golden",
        action="store_true",
        help="rewrite the checked-in golden from the preview run (intentional maintenance action)",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        binary_location = args.binary or args.preview_dir or ROOT / "preview"
        run(resolve_binary(binary_location), args.archive, args.update_golden)
    except ReplayFailure as exc:
        print(f"REPLAY FAILED: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
