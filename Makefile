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
INSTALL_SELECTION ?=

PYTHON ?= python3
CARGO ?= cargo
ZIP ?= zip
UNZIP ?= unzip
SKILL_VALIDATOR ?= $(if $(CODEX_HOME),$(CODEX_HOME),$(HOME)/.codex)/skills/.system/skill-creator/scripts/quick_validate.py

RUNTIME_PATHS := SKILL.md agents references
WIKI_RUNTIME_PATHS := vendor engine/dokuwiki
PACKAGE_PATHS := SKILL.md agents references scripts $(WIKI_RUNTIME_PATHS)

.PHONY: build preview reader install _install-package test-install clean test test-contract preview-change-wiki build-change-wiki

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

install:
	@set -eu; \
	selection="$(INSTALL_SELECTION)"; \
	if [ -z "$$selection" ]; then \
		if [ ! -t 0 ]; then \
			echo "Set INSTALL_SELECTION to repowiki, change-wiki, or both when no terminal is available" >&2; exit 2; \
		fi; \
		printf 'Select Skills to install:\n'; \
		printf '  1) RepoWiki\n  2) Change Wiki\n  3) Both\n  0) Cancel\n'; \
		printf 'Choice: '; \
		IFS= read -r choice || { echo "No selection received" >&2; exit 2; }; \
		case "$$choice" in \
			1) selection=repowiki ;; \
			2) selection=change-wiki ;; \
			3) selection=both ;; \
			0|q|Q) printf 'installation cancelled\n'; exit 0 ;; \
			*) echo "Invalid selection: $$choice" >&2; exit 2 ;; \
		esac; \
	fi; \
	case "$$selection" in \
		repowiki|change-wiki|both) ;; \
		*) echo "INSTALL_SELECTION must be repowiki, change-wiki, or both" >&2; exit 2 ;; \
	esac; \
	if [ "$$selection" = repowiki ] || [ "$$selection" = both ]; then \
		$(MAKE) --no-print-directory build; \
	fi; \
	if [ "$$selection" = change-wiki ] || [ "$$selection" = both ]; then \
		$(MAKE) --no-print-directory build-change-wiki; \
	fi; \
	if [ "$$selection" = repowiki ] || [ "$$selection" = both ]; then \
		$(MAKE) --no-print-directory _install-package \
			INSTALL_SKILL_NAME="$(SKILL_NAME)" \
			INSTALL_ARCHIVE="$(ARCHIVE)" \
			INSTALL_PROFILE=repowiki; \
	fi; \
	if [ "$$selection" = change-wiki ] || [ "$$selection" = both ]; then \
		$(MAKE) --no-print-directory _install-package \
			INSTALL_SKILL_NAME="$(CHANGE_SKILL_NAME)" \
			INSTALL_ARCHIVE="$(CHANGE_ARCHIVE)" \
			INSTALL_PROFILE=change-wiki; \
	fi

_install-package:
	@command -v "$(UNZIP)" >/dev/null 2>&1 || { echo "unzip is required to install the Skill" >&2; exit 127; }
	@install_root="$(INSTALL_DIR)"; \
	[ -n "$$install_root" ] || { echo "INSTALL_DIR must not be empty" >&2; exit 2; }; \
	[ -n "$(INSTALL_SKILL_NAME)" ] || { echo "INSTALL_SKILL_NAME must not be empty" >&2; exit 2; }; \
	[ -n "$(INSTALL_ARCHIVE)" ] || { echo "INSTALL_ARCHIVE must not be empty" >&2; exit 2; }; \
	mkdir -p "$$install_root"; \
	target="$$install_root/$(INSTALL_SKILL_NAME)"; \
	staging="$$(mktemp -d "$$install_root/.$(INSTALL_SKILL_NAME).install.XXXXXX")"; \
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
	"$(UNZIP)" -q "$(INSTALL_ARCHIVE)" -d "$$staging"; \
	case "$(INSTALL_PROFILE)" in \
		repowiki) "$(PYTHON)" tools/validate-skill-package.py "$$staging" ;; \
		change-wiki) "$(PYTHON)" tools/validate-skill-package.py --profile change-wiki "$$staging" ;; \
		*) echo "INSTALL_PROFILE must be repowiki or change-wiki" >&2; exit 2 ;; \
	esac; \
	if [ -e "$$target" ] || [ -L "$$target" ]; then \
		backup="$$(mktemp -d "$$install_root/.$(INSTALL_SKILL_NAME).backup.XXXXXX")"; \
		rmdir "$$backup"; \
		mv "$$target" "$$backup"; \
	fi; \
	mv "$$staging" "$$target"; \
	staging=""; \
	printf 'installed %s to %s\n' "$(INSTALL_SKILL_NAME)" "$$target"

test-install:
	@temporary_install_root="$$(mktemp -d)"; \
	cleanup() { status=$$?; trap - EXIT HUP INT TERM; rm -rf "$$temporary_install_root"; exit "$$status"; }; \
	trap cleanup EXIT HUP INT TERM; \
	repowiki_root="$$temporary_install_root/repowiki"; \
	change_root="$$temporary_install_root/change-wiki"; \
	both_root="$$temporary_install_root/both"; \
	check_installed_skill() { \
		root=$$1; skill_name=$$2; profile=$$3; \
		if [ "$$profile" = change-wiki ]; then \
			"$(PYTHON)" tools/validate-skill-package.py --profile change-wiki "$$root/$$skill_name"; \
		else \
			"$(PYTHON)" tools/validate-skill-package.py "$$root/$$skill_name"; \
		fi; \
		test ! -e "$$root/$$skill_name/stale.txt"; \
		installed_binary="$$root/$$skill_name/scripts/repowiki"; \
		if [ -f "$$installed_binary.exe" ]; then installed_binary="$$installed_binary.exe"; fi; \
		test -f "$$installed_binary"; \
		case "$$installed_binary" in *.exe) ;; *) test -x "$$installed_binary" ;; esac; \
	}; \
	mkdir -p "$$repowiki_root/$(SKILL_NAME)"; \
	printf 'stale file\n' > "$$repowiki_root/$(SKILL_NAME)/stale.txt"; \
	$(MAKE) --no-print-directory install INSTALL_DIR="$$repowiki_root" INSTALL_SELECTION=repowiki; \
	check_installed_skill "$$repowiki_root" "$(SKILL_NAME)" repowiki; \
	test ! -e "$$repowiki_root/$(CHANGE_SKILL_NAME)"; \
	mkdir -p "$$change_root/$(CHANGE_SKILL_NAME)"; \
	printf 'stale file\n' > "$$change_root/$(CHANGE_SKILL_NAME)/stale.txt"; \
	$(MAKE) --no-print-directory install INSTALL_DIR="$$change_root" INSTALL_SELECTION=change-wiki; \
	check_installed_skill "$$change_root" "$(CHANGE_SKILL_NAME)" change-wiki; \
	test ! -e "$$change_root/$(SKILL_NAME)"; \
	mkdir -p "$$both_root/$(SKILL_NAME)" "$$both_root/$(CHANGE_SKILL_NAME)"; \
	printf 'stale file\n' > "$$both_root/$(SKILL_NAME)/stale.txt"; \
	printf 'stale file\n' > "$$both_root/$(CHANGE_SKILL_NAME)/stale.txt"; \
	$(MAKE) --no-print-directory install INSTALL_DIR="$$both_root" INSTALL_SELECTION=both; \
	check_installed_skill "$$both_root" "$(SKILL_NAME)" repowiki; \
	check_installed_skill "$$both_root" "$(CHANGE_SKILL_NAME)" change-wiki; \
	if $(MAKE) --no-print-directory install INSTALL_DIR="$$temporary_install_root/no-selection" INSTALL_SELECTION= </dev/null; then \
		echo "install without a selection should fail when stdin is not a terminal" >&2; exit 1; \
	fi; \
	if $(MAKE) --no-print-directory install INSTALL_DIR="$$temporary_install_root/invalid-selection" INSTALL_SELECTION=invalid; then \
		echo "install with an invalid selection should fail" >&2; exit 1; \
	fi; \
	test ! -e "$$temporary_install_root/no-selection"; \
	test ! -e "$$temporary_install_root/invalid-selection"; \
	printf 'PASS install smoke: repowiki, change-wiki, both, and selection errors\n'

preview:
	@rm -rf "$(PREVIEW_DIR)"
	@mkdir -p "$(PREVIEW_DIR)"
	@$(CARGO) build --release --locked --manifest-path "$(ENGINE_DIR)/Cargo.toml" --target-dir "$(CARGO_TARGET_DIR)" --bin repowiki
	@for path in $(RUNTIME_PATHS); do \
		if [ ! -e "$(SKILL_DIR)/$$path" ]; then \
			echo "missing runtime Skill source: skill/$$path" >&2; exit 2; \
		fi; \
		cp -a "$(SKILL_DIR)/$$path" "$(PREVIEW_DIR)/$$path"; \
	done
	@cp -a "$(PROJECT_ROOT)/vendor" "$(PREVIEW_DIR)/vendor"
	@mkdir -p "$(PREVIEW_DIR)/engine"
	@cp -a "$(ENGINE_DIR)/dokuwiki" "$(PREVIEW_DIR)/engine/dokuwiki"
	@mkdir -p "$(PREVIEW_DIR)/scripts"
	@binary=""; \
	if [ -n "$${CARGO_BUILD_TARGET:-}" ] && [ -f "$(CARGO_TARGET_DIR)/$${CARGO_BUILD_TARGET}/release/repowiki.exe" ]; then \
		binary="$(CARGO_TARGET_DIR)/$${CARGO_BUILD_TARGET}/release/repowiki.exe"; \
	elif [ -n "$${CARGO_BUILD_TARGET:-}" ] && [ -f "$(CARGO_TARGET_DIR)/$${CARGO_BUILD_TARGET}/release/repowiki" ]; then \
		binary="$(CARGO_TARGET_DIR)/$${CARGO_BUILD_TARGET}/release/repowiki"; \
	elif [ -f "$(CARGO_TARGET_DIR)/release/repowiki.exe" ]; then \
		binary="$(CARGO_TARGET_DIR)/release/repowiki.exe"; \
	elif [ -f "$(CARGO_TARGET_DIR)/release/repowiki" ]; then \
		binary="$(CARGO_TARGET_DIR)/release/repowiki"; \
	fi; \
	if [ -z "$$binary" ]; then \
		echo "Cargo completed but no repowiki executable was found" >&2; exit 1; \
	fi; \
	case "$$binary" in \
		*.exe) cp -p "$$binary" "$(PREVIEW_DIR)/scripts/repowiki.exe" ;; \
		*) cp -p "$$binary" "$(PREVIEW_DIR)/scripts/repowiki"; chmod +x "$(PREVIEW_DIR)/scripts/repowiki" ;; \
	esac
	@$(PYTHON) tools/validate-skill-package.py "$(PREVIEW_DIR)"
	@printf 'created preview at %s\n' "$(PREVIEW_DIR)"

preview-change-wiki:
	@rm -rf "$(CHANGE_PREVIEW_DIR)"
	@mkdir -p "$(CHANGE_PREVIEW_DIR)"
	@$(CARGO) build --release --locked --manifest-path "$(ENGINE_DIR)/Cargo.toml" --target-dir "$(CARGO_TARGET_DIR)" --bin repowiki
	@for path in $(RUNTIME_PATHS); do \
		if [ ! -e "$(CHANGE_SKILL_DIR)/$$path" ]; then \
			echo "missing runtime Skill source: change-wiki/$$path" >&2; exit 2; \
		fi; \
		cp -a "$(CHANGE_SKILL_DIR)/$$path" "$(CHANGE_PREVIEW_DIR)/$$path"; \
	done
	@cp -a "$(PROJECT_ROOT)/vendor" "$(CHANGE_PREVIEW_DIR)/vendor"
	@mkdir -p "$(CHANGE_PREVIEW_DIR)/engine"
	@cp -a "$(ENGINE_DIR)/dokuwiki" "$(CHANGE_PREVIEW_DIR)/engine/dokuwiki"
	@mkdir -p "$(CHANGE_PREVIEW_DIR)/scripts"
	@binary=""; \
	if [ -n "$${CARGO_BUILD_TARGET:-}" ] && [ -f "$(CARGO_TARGET_DIR)/$${CARGO_BUILD_TARGET}/release/repowiki.exe" ]; then \
		binary="$(CARGO_TARGET_DIR)/$${CARGO_BUILD_TARGET}/release/repowiki.exe"; \
	elif [ -n "$${CARGO_BUILD_TARGET:-}" ] && [ -f "$(CARGO_TARGET_DIR)/$${CARGO_BUILD_TARGET}/release/repowiki" ]; then \
		binary="$(CARGO_TARGET_DIR)/$${CARGO_BUILD_TARGET}/release/repowiki"; \
	elif [ -f "$(CARGO_TARGET_DIR)/release/repowiki.exe" ]; then \
		binary="$(CARGO_TARGET_DIR)/release/repowiki.exe"; \
	elif [ -f "$(CARGO_TARGET_DIR)/release/repowiki" ]; then \
		binary="$(CARGO_TARGET_DIR)/release/repowiki"; \
	fi; \
	if [ -z "$$binary" ]; then \
		echo "Cargo completed but no repowiki executable was found" >&2; exit 1; \
	fi; \
	case "$$binary" in \
		*.exe) cp -p "$$binary" "$(CHANGE_PREVIEW_DIR)/scripts/repowiki.exe" ;; \
		*) cp -p "$$binary" "$(CHANGE_PREVIEW_DIR)/scripts/repowiki"; chmod +x "$(CHANGE_PREVIEW_DIR)/scripts/repowiki" ;; \
	esac
	@$(PYTHON) tools/validate-skill-package.py --profile change-wiki "$(CHANGE_PREVIEW_DIR)"
	@printf 'created preview at %s\n' "$(CHANGE_PREVIEW_DIR)"

reader:
	@rm -rf "$(BUILD_DIR)/reader"
	@mkdir -p "$(BUILD_DIR)/reader" "$(BUILD_DIR)/reader/engine"
	@$(CARGO) build --release --locked --manifest-path "$(ENGINE_DIR)/Cargo.toml" --target-dir "$(CARGO_TARGET_DIR)" --bin repowiki-reader
	@reader_binary=""; \
	if [ -n "$${CARGO_BUILD_TARGET:-}" ] && [ -f "$(CARGO_TARGET_DIR)/$${CARGO_BUILD_TARGET}/release/repowiki-reader.exe" ]; then \
		reader_binary="$(CARGO_TARGET_DIR)/$${CARGO_BUILD_TARGET}/release/repowiki-reader.exe"; \
	elif [ -n "$${CARGO_BUILD_TARGET:-}" ] && [ -f "$(CARGO_TARGET_DIR)/$${CARGO_BUILD_TARGET}/release/repowiki-reader" ]; then \
		reader_binary="$(CARGO_TARGET_DIR)/$${CARGO_BUILD_TARGET}/release/repowiki-reader"; \
	elif [ -f "$(CARGO_TARGET_DIR)/release/repowiki-reader.exe" ]; then \
		reader_binary="$(CARGO_TARGET_DIR)/release/repowiki-reader.exe"; \
	elif [ -f "$(CARGO_TARGET_DIR)/release/repowiki-reader" ]; then \
		reader_binary="$(CARGO_TARGET_DIR)/release/repowiki-reader"; \
	fi; \
	if [ -z "$$reader_binary" ]; then \
		echo "Cargo completed but no repowiki-reader executable was found" >&2; exit 1; \
	fi; \
	cp -p "$$reader_binary" "$(BUILD_DIR)/reader/repowiki-reader"; \
	case "$$reader_binary" in *.exe) mv "$(BUILD_DIR)/reader/repowiki-reader" "$(BUILD_DIR)/reader/repowiki-reader.exe" ;; esac
	@cp -a "$(PROJECT_ROOT)/vendor" "$(BUILD_DIR)/reader/vendor"
	@cp -a "$(ENGINE_DIR)/dokuwiki" "$(BUILD_DIR)/reader/engine/dokuwiki"
	@printf 'created reader package at %s\n' "$(BUILD_DIR)/reader"

clean:
	@rm -rf \
		"$(BUILD_DIR)" \
		"$(PREVIEW_DIR)" \
		"$(DIST_DIR)" \
		"$(PROJECT_ROOT)/target" \
		"$(ENGINE_DIR)/target" \
		"$(SKILL_DIR)/.repowiki-build" \
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
