SHELL := /bin/sh
.SHELLFLAGS := -eu -c
.DEFAULT_GOAL := build

PROJECT_ROOT := $(abspath .)
ENGINE_DIR := $(PROJECT_ROOT)/engine
SKILL_DIR := $(PROJECT_ROOT)/skill
CHANGE_SKILL_DIR := $(PROJECT_ROOT)/change-wiki
BUILD_DIR ?= $(PROJECT_ROOT)/.build
CARGO_TARGET_DIR ?= $(BUILD_DIR)/cargo-target
PREVIEW_DIR := $(PROJECT_ROOT)/preview
CHANGE_PREVIEW_DIR := $(BUILD_DIR)/change-wiki-preview
DIST_DIR := $(PROJECT_ROOT)/dist
SKILL_NAME := RepoWiki
CHANGE_SKILL_NAME := ChangeWiki
PACKAGE_NAME := $(SKILL_NAME)
VERSION := $(shell sed -n 's/^version = "\([^"]*\)".*/\1/p' $(ENGINE_DIR)/Cargo.toml | head -n 1)
ARCHIVE := $(DIST_DIR)/$(PACKAGE_NAME)-$(VERSION).zip
CHANGE_ARCHIVE := $(DIST_DIR)/$(CHANGE_SKILL_NAME)-$(VERSION).zip
INSTALL_DIR ?= $(HOME)/.agents/skills

PYTHON ?= python3
CARGO ?= cargo
ZIP ?= zip
UNZIP ?= unzip
SKILL_VALIDATOR ?= $(if $(CODEX_HOME),$(CODEX_HOME),$(HOME)/.codex)/skills/.system/skill-creator/scripts/quick_validate.py

RUNTIME_PATHS := SKILL.md agents references
PACKAGE_PATHS := SKILL.md agents references scripts

.PHONY: build preview reader install install-change-wiki test-install test-install-change-wiki clean test test-contract preview-change-wiki build-change-wiki

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

build-change-wiki: preview-change-wiki
	@mkdir -p "$(DIST_DIR)"
	@rm -f "$(CHANGE_ARCHIVE)"
	@command -v "$(ZIP)" >/dev/null 2>&1 || { echo "zip is required to build the Skill archive" >&2; exit 127; }
	@cd "$(CHANGE_PREVIEW_DIR)" && "$(ZIP)" -X -q -r "$(CHANGE_ARCHIVE)" $(PACKAGE_PATHS)
	@$(PYTHON) tools/validate-skill-package.py --profile change-wiki "$(CHANGE_PREVIEW_DIR)" "$(CHANGE_ARCHIVE)"
	@printf 'created %s\n' "$(CHANGE_ARCHIVE)"

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

install-change-wiki: build-change-wiki
	@command -v "$(UNZIP)" >/dev/null 2>&1 || { echo "unzip is required to install the Skill" >&2; exit 127; }
	@install_root="$(INSTALL_DIR)"; \
	[ -n "$$install_root" ] || { echo "INSTALL_DIR must not be empty" >&2; exit 2; }; \
	mkdir -p "$$install_root"; \
	target="$$install_root/$(CHANGE_SKILL_NAME)"; \
	staging="$$(mktemp -d "$$install_root/.$(CHANGE_SKILL_NAME).install.XXXXXX")"; \
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
	"$(UNZIP)" -q "$(CHANGE_ARCHIVE)" -d "$$staging"; \
	"$(PYTHON)" tools/validate-skill-package.py --profile change-wiki "$$staging"; \
	if [ -e "$$target" ] || [ -L "$$target" ]; then \
		backup="$$(mktemp -d "$$install_root/.$(CHANGE_SKILL_NAME).backup.XXXXXX")"; \
		rmdir "$$backup"; \
		mv "$$target" "$$backup"; \
	fi; \
	mv "$$staging" "$$target"; \
	staging=""; \
	printf 'installed %s to %s\n' "$(CHANGE_SKILL_NAME)" "$$target"

test-install: build test-install-change-wiki
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

test-install-change-wiki: build-change-wiki
	@temporary_install_root="$$(mktemp -d)"; \
	cleanup() { status=$$?; trap - EXIT HUP INT TERM; rm -rf "$$temporary_install_root"; exit "$$status"; }; \
	trap cleanup EXIT HUP INT TERM; \
	mkdir -p "$$temporary_install_root/$(CHANGE_SKILL_NAME)"; \
	printf 'stale file\n' > "$$temporary_install_root/$(CHANGE_SKILL_NAME)/stale.txt"; \
	$(MAKE) --no-print-directory install-change-wiki INSTALL_DIR="$$temporary_install_root"; \
	"$(PYTHON)" tools/validate-skill-package.py --profile change-wiki "$$temporary_install_root/$(CHANGE_SKILL_NAME)"; \
	test ! -e "$$temporary_install_root/$(CHANGE_SKILL_NAME)/stale.txt"; \
	installed_binary="$$temporary_install_root/$(CHANGE_SKILL_NAME)/scripts/codewiki"; \
	if [ -f "$$installed_binary.exe" ]; then installed_binary="$$installed_binary.exe"; fi; \
	test -f "$$installed_binary"; \
	case "$$installed_binary" in *.exe) ;; *) test -x "$$installed_binary" ;; esac; \
	printf 'PASS install smoke: %s\n' "$$temporary_install_root/$(CHANGE_SKILL_NAME)"

preview:
	@rm -rf "$(PREVIEW_DIR)"
	@mkdir -p "$(PREVIEW_DIR)"
	@$(CARGO) build --release --locked --manifest-path "$(ENGINE_DIR)/Cargo.toml" --target-dir "$(CARGO_TARGET_DIR)" --bin codewiki
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

preview-change-wiki:
	@rm -rf "$(CHANGE_PREVIEW_DIR)"
	@mkdir -p "$(CHANGE_PREVIEW_DIR)"
	@$(CARGO) build --release --locked --manifest-path "$(ENGINE_DIR)/Cargo.toml" --target-dir "$(CARGO_TARGET_DIR)" --bin codewiki
	@for path in $(RUNTIME_PATHS); do \
		if [ ! -e "$(CHANGE_SKILL_DIR)/$$path" ]; then \
			echo "missing runtime Skill source: change-wiki/$$path" >&2; exit 2; \
		fi; \
		cp -a "$(CHANGE_SKILL_DIR)/$$path" "$(CHANGE_PREVIEW_DIR)/$$path"; \
	done
	@mkdir -p "$(CHANGE_PREVIEW_DIR)/scripts"
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
		*.exe) cp -p "$$binary" "$(CHANGE_PREVIEW_DIR)/scripts/codewiki.exe" ;; \
		*) cp -p "$$binary" "$(CHANGE_PREVIEW_DIR)/scripts/codewiki"; chmod +x "$(CHANGE_PREVIEW_DIR)/scripts/codewiki" ;; \
	esac
	@$(PYTHON) tools/validate-skill-package.py --profile change-wiki "$(CHANGE_PREVIEW_DIR)"
	@printf 'created preview at %s\n' "$(CHANGE_PREVIEW_DIR)"

reader:
	@$(CARGO) build --release --locked --manifest-path "$(ENGINE_DIR)/Cargo.toml" --target-dir "$(CARGO_TARGET_DIR)" --bin repowiki-reader
	@printf 'created reader at %s\n' "$(CARGO_TARGET_DIR)/release/repowiki-reader"

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

test-contract:
	@$(PYTHON) tests/differential/prompt_contract.py

test: build build-change-wiki test-contract
	@$(PYTHON) tests/differential/run_replay.py \
		--preview-dir "$(PREVIEW_DIR)" \
		--archive "$(ARCHIVE)"
	@$(PYTHON) tests/differential/run_replay.py \
		--preview-dir "$(CHANGE_PREVIEW_DIR)" \
		--archive "$(CHANGE_ARCHIVE)"
	@$(CARGO) fmt --manifest-path "$(ENGINE_DIR)/Cargo.toml" --all -- --check
	@$(CARGO) test --manifest-path "$(ENGINE_DIR)/Cargo.toml"
	@$(CARGO) clippy --manifest-path "$(ENGINE_DIR)/Cargo.toml" --all-targets -- -D warnings
	@if [ -f "$(SKILL_VALIDATOR)" ]; then \
		$(PYTHON) "$(SKILL_VALIDATOR)" "$(PREVIEW_DIR)"; \
		$(PYTHON) "$(SKILL_VALIDATOR)" "$(CHANGE_PREVIEW_DIR)"; \
	else \
		echo "skill-creator quick validator not found; runtime package validator was already run"; \
	fi
