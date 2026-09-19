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
SKILL_NAME := RepoWiki
PACKAGE_NAME := $(SKILL_NAME)
VERSION := $(shell sed -n 's/^version = "\([^"]*\)".*/\1/p' $(ENGINE_DIR)/Cargo.toml | head -n 1)
ARCHIVE := $(DIST_DIR)/$(PACKAGE_NAME)-$(VERSION).zip
INSTALL_DIR ?= $(HOME)/.agents/skills

PYTHON ?= python3
CARGO ?= cargo
ZIP ?= zip
UNZIP ?= unzip
SKILL_VALIDATOR ?= $(if $(CODEX_HOME),$(CODEX_HOME),$(HOME)/.codex)/skills/.system/skill-creator/scripts/quick_validate.py

RUNTIME_PATHS := SKILL.md agents references
PACKAGE_PATHS := SKILL.md agents references scripts

.PHONY: build preview install test-install clean test test-reference

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

install: build
	@command -v "$(UNZIP)" >/dev/null 2>&1 || { echo "unzip is required to install the Skill" >&2; exit 127; }
	@install_root="$(INSTALL_DIR)"; \
	[ -n "$$install_root" ] || { echo "INSTALL_DIR must not be empty" >&2; exit 2; }; \
	mkdir -p "$$install_root"; \
	target="$$install_root/$(SKILL_NAME)"; \
	staging="$$(mktemp -d "$$install_root/.$(SKILL_NAME).install.XXXXXX")"; \
	backup=""; \
	cleanup() { \
		status=$$?; \
		trap - EXIT HUP INT TERM; \
		if [ "$$status" -ne 0 ]; then \
			if [ -e "$$target" ] || [ -L "$$target" ]; then rm -rf "$$target"; fi; \
			if [ -n "$$backup" ] && { [ -e "$$backup" ] || [ -L "$$backup" ]; }; then mv "$$backup" "$$target"; fi; \
		elif [ -n "$$backup" ] && { [ -e "$$backup" ] || [ -L "$$backup" ]; }; then \
			rm -rf "$$backup"; \
		fi; \
		if [ -n "$$staging" ] && [ -e "$$staging" ]; then rm -rf "$$staging"; fi; \
		exit "$$status"; \
	}; \
	trap cleanup EXIT HUP INT TERM; \
	"$(UNZIP)" -q "$(ARCHIVE)" -d "$$staging"; \
	"$(PYTHON)" tools/validate-skill-package.py "$$staging"; \
	if [ -e "$$target" ] || [ -L "$$target" ]; then \
		backup="$$(mktemp -d "$$install_root/.$(SKILL_NAME).backup.XXXXXX")"; \
		rmdir "$$backup"; \
		mv "$$target" "$$backup"; \
	fi; \
	mv "$$staging" "$$target"; \
	staging=""; \
	printf 'installed %s to %s\n' "$(SKILL_NAME)" "$$target"

test-install: build
	@temporary_install_root="$$(mktemp -d)"; \
	cleanup() { status=$$?; trap - EXIT HUP INT TERM; rm -rf "$$temporary_install_root"; exit "$$status"; }; \
	trap cleanup EXIT HUP INT TERM; \
	mkdir -p "$$temporary_install_root/$(SKILL_NAME)"; \
	printf 'stale file\n' > "$$temporary_install_root/$(SKILL_NAME)/stale.txt"; \
	$(MAKE) --no-print-directory install INSTALL_DIR="$$temporary_install_root"; \
	"$(PYTHON)" tools/validate-skill-package.py "$$temporary_install_root/$(SKILL_NAME)"; \
	test ! -e "$$temporary_install_root/$(SKILL_NAME)/stale.txt"; \
	installed_binary="$$temporary_install_root/$(SKILL_NAME)/scripts/codewiki"; \
	if [ -f "$$installed_binary.exe" ]; then installed_binary="$$installed_binary.exe"; fi; \
	test -f "$$installed_binary"; \
	case "$$installed_binary" in *.exe) ;; *) test -x "$$installed_binary" ;; esac; \
	printf 'PASS install smoke: %s\n' "$$temporary_install_root/$(SKILL_NAME)"

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
