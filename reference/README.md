# CodeWiki reference

`CodeWiki` is tracked as a Git submodule and pinned to a known upstream
commit. The parent repository records the submodule commit so differential
tests remain reproducible.

Initialize the checkout and its Python environment with:

```bash
git submodule update --init --recursive
reference/setup-differential.sh
```

Run the reference comparison with:

```bash
make test-reference PYTHON=reference/.venv/bin/python
```

The first reference-parser import may download the `tiktoken` encoding cache.
The virtual environment is local-only and must not be committed.

To update the pinned reference deliberately:

```bash
git -C reference/CodeWiki fetch origin
git -C reference/CodeWiki checkout <commit>
make test-reference PYTHON=reference/.venv/bin/python
git add reference/CodeWiki
git commit -m "test: update CodeWiki reference"
```
