SHELL := /bin/sh
.SHELLFLAGS := -eu -c
.DEFAULT_GOAL := build

PROJECT_ROOT := $(abspath .)
ENGINE_DIR := $(PROJECT_ROOT)/engine
SKILL_DIR := $(PROJECT_ROOT)/skill
BUILD_DIR ?= $(PROJECT_ROOT)/.build
CARGO_TARGET_DIR ?= $(BUILD_DIR)/cargo-target
PREVIEW_DIR := $(PROJECT_ROOT)/preview
DIST_DIR := $(PROJECT_ROOT)/dist
PACKAGE_NAME := codewiki-wiki-generator
VERSION := $(shell sed -n 's/^version = "\([^"]*\)".*/\1/p' $(ENGINE_DIR)/Cargo.toml | head -n 1)
ARCHIVE := $(DIST_DIR)/$(PACKAGE_NAME)-$(VERSION).zip

PYTHON ?= python3
CARGO ?= cargo
ZIP ?= zip
SKILL_VALIDATOR ?= $(if $(CODEX_HOME),$(CODEX_HOME),$(HOME)/.codex)/skills/.system/skill-creator/scripts/quick_validate.py

RUNTIME_PATHS := SKILL.md agents references
PACKAGE_PATHS := SKILL.md agents references scripts

.PHONY: build preview clean test test-reference

ifeq ($(strip $(VERSION)),)
$(error Could not read the package version from engine/Cargo.toml)
endif

build: preview
	@mkdir -p "$(DIST_DIR)"
	@rm -f "$(ARCHIVE)"
	@command -v "$(ZIP)" >/dev/null 2>&1 || { echo "zip is required to build the Skill archive" >&2; exit 127; }
	@cd "$(PREVIEW_DIR)" && "$(ZIP)" -X -q -r "$(ARCHIVE)" $(PACKAGE_PATHS)
	@$(PYTHON) tools/validate-skill-package.py "$(PREVIEW_DIR)" "$(ARCHIVE)"
	@printf 'created %s\n' "$(ARCHIVE)"

preview:
	@rm -rf "$(PREVIEW_DIR)"
	@mkdir -p "$(PREVIEW_DIR)"
	@$(CARGO) build --release --locked --manifest-path "$(ENGINE_DIR)/Cargo.toml" --target-dir "$(CARGO_TARGET_DIR)"
	@for path in $(RUNTIME_PATHS); do \
		if [ ! -e "$(SKILL_DIR)/$$path" ]; then \
			echo "missing runtime Skill source: skill/$$path" >&2; exit 2; \
		fi; \
		cp -a "$(SKILL_DIR)/$$path" "$(PREVIEW_DIR)/$$path"; \
	done
	@mkdir -p "$(PREVIEW_DIR)/scripts"
	@binary=""; \
	if [ -n "$${CARGO_BUILD_TARGET:-}" ] && [ -f "$(CARGO_TARGET_DIR)/$${CARGO_BUILD_TARGET}/release/codewiki.exe" ]; then \
		binary="$(CARGO_TARGET_DIR)/$${CARGO_BUILD_TARGET}/release/codewiki.exe"; \
	elif [ -n "$${CARGO_BUILD_TARGET:-}" ] && [ -f "$(CARGO_TARGET_DIR)/$${CARGO_BUILD_TARGET}/release/codewiki" ]; then \
		binary="$(CARGO_TARGET_DIR)/$${CARGO_BUILD_TARGET}/release/codewiki"; \
	elif [ -f "$(CARGO_TARGET_DIR)/release/codewiki.exe" ]; then \
		binary="$(CARGO_TARGET_DIR)/release/codewiki.exe"; \
	elif [ -f "$(CARGO_TARGET_DIR)/release/codewiki" ]; then \
		binary="$(CARGO_TARGET_DIR)/release/codewiki"; \
	fi; \
	if [ -z "$$binary" ]; then \
		echo "Cargo completed but no codewiki executable was found" >&2; exit 1; \
	fi; \
	case "$$binary" in \
		*.exe) cp -p "$$binary" "$(PREVIEW_DIR)/scripts/codewiki.exe" ;; \
		*) cp -p "$$binary" "$(PREVIEW_DIR)/scripts/codewiki"; chmod +x "$(PREVIEW_DIR)/scripts/codewiki" ;; \
	esac
	@$(PYTHON) tools/validate-skill-package.py "$(PREVIEW_DIR)"
	@printf 'created preview at %s\n' "$(PREVIEW_DIR)"

clean:
	@rm -rf \
		"$(BUILD_DIR)" \
		"$(PREVIEW_DIR)" \
		"$(DIST_DIR)" \
		"$(PROJECT_ROOT)/target" \
		"$(ENGINE_DIR)/target" \
		"$(SKILL_DIR)/.codewiki-build" \
		"$(PROJECT_ROOT)/tools/__pycache__" \
		"$(ENGINE_DIR)/__pycache__" \
		"$(PROJECT_ROOT)/packaging"
	@printf 'cleaned generated build artifacts\n'

test: build
	@$(PYTHON) tests/differential/run_replay.py \
		--preview-dir "$(PREVIEW_DIR)" \
		--archive "$(ARCHIVE)"
	@$(CARGO) fmt --manifest-path "$(ENGINE_DIR)/Cargo.toml" --all -- --check
	@$(CARGO) test --manifest-path "$(ENGINE_DIR)/Cargo.toml"
	@$(CARGO) clippy --manifest-path "$(ENGINE_DIR)/Cargo.toml" --all-targets -- -D warnings
	@if [ -f "$(SKILL_VALIDATOR)" ]; then \
		$(PYTHON) "$(SKILL_VALIDATOR)" "$(PREVIEW_DIR)"; \
	else \
		echo "skill-creator quick validator not found; runtime package validator was already run"; \
	fi

# Runs the same mandatory offline Skill replay first, then invokes the real
# Python reference adapter. Missing reference dependencies are a hard failure
# for this target; the core replay never silently degrades to a reference skip.
test-reference: build
	@$(PYTHON) tests/differential/run_replay.py \
		--preview-dir "$(PREVIEW_DIR)" \
		--archive "$(ARCHIVE)"
	@$(PYTHON) tools/run-reference-differential.py \
		--preview-dir "$(PREVIEW_DIR)" \
		--reference-root "$(PROJECT_ROOT)/reference/CodeWiki"
