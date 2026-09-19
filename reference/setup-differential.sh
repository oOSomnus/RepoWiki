#!/usr/bin/env bash

set -euo pipefail

REFERENCE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "${REFERENCE_DIR}/.." && pwd)"
VENV_DIR="${REFERENCE_DIR}/.venv"
PYTHON_BIN="${PYTHON_BIN:-python3}"

"${PYTHON_BIN}" -c 'import sys; sys.exit("Python 3.12 or newer is required") if sys.version_info < (3, 12) else None'

if [ ! -x "${VENV_DIR}/bin/python" ]; then
    "${PYTHON_BIN}" -m venv "${VENV_DIR}"
fi

"${VENV_DIR}/bin/python" -m pip install \
    -r "${ROOT_DIR}/reference/CodeWiki/requirements.txt"

# The reference snapshot currently publishes a Tree-sitter core that cannot
# load its C# grammar, and omits tree-sitter-php from requirements.txt.
"${VENV_DIR}/bin/python" -m pip install \
    --upgrade \
    "tree-sitter==0.25.2" \
    "tree-sitter-php==0.24.1"

printf 'reference environment ready: %s\n' "${VENV_DIR}/bin/python"
printf 'run: make test-reference PYTHON=%s\n' "${VENV_DIR}/bin/python"
