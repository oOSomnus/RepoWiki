# Graph Report - RepoWiki  (2026-09-19)

## Corpus Check
- Large corpus: 264 files · ~560,924 words. Semantic extraction will be expensive (many Claude tokens). Consider running on a subfolder.

## Summary
- 3147 nodes · 7310 edges · 164 communities (111 shown, 53 thin omitted)
- Extraction: 96% EXTRACTED · 4% INFERRED · 0% AMBIGUOUS · INFERRED: 325 edges (avg confidence: 0.93)
- Token cost: 0 input · 0 output

## Community Hubs (Navigation)
- Update Orchestration
- Incremental Update Engine
- LLM Backend Services
- Configuration Management
- Core Graph Model
- Rust Graph Analyzer
- Rust CLI Interface
- Module Clustering
- Rust Update Planning
- TypeScript Tree-sitter Analyzer
- Workspace Edit Tools
- Session Analysis State
- Language Analysis Core
- CLI Branding
- Documentation Generation
- Documentation Architecture Concepts
- Documentation Generator Pipeline
- JavaScript Tree-sitter Analyzer
- PHP Tree-sitter Analyzer
- Dependency Graph Analysis
- Python AST Analyzer
- Rust Documentation Model
- LLM Service Factory
- Module Tree Utilities
- Rust Prompt Templates
- CodeWiki Architecture Concepts
- Generate Command Validation
- C++ Tree-sitter Analyzer
- C# Tree-sitter Analyzer
- Replay Differential Testing
- Scala Tree-sitter Analyzer
- Repository Analysis Service
- Backend Configuration
- Java Tree-sitter Analyzer
- Artifact Graph Analysis
- Repository Filtering Tests
- Web Application Routes
- Kotlin Tree-sitter Analyzer
- MCP Server Tools
- Recursive Module Workflow
- Web Route Handlers
- Web Viewer Application
- CLI Interaction Utilities
- Git Integration
- Background Job Worker
- MCP Workflow Concepts
- Ruby Tree-sitter Analyzer
- Documentation Quality Metrics
- Reference Replay Testing
- Session Persistence
- Multi-language Analysis Dispatch
- C-family Parser Frontend
- Document Editing Tools
- Analysis Cache Management
- HTML Documentation Viewer
- CLI Logging
- Module Tree Management
- Call Graph Resolution
- Entry Point Detection
- Documentation Web Serving
- Artifact Classification
- Prompt Template System
- Web Template Rendering
- Progress Tracking
- Change Detection
- Session Workspace Files
- Repository Analyzer
- Scala Analyzer Tests
- Dependency Parser Tests
- Skill Package Validation
- Repository Cloning
- C Tree-sitter Analyzer
- File Persistence
- CodeWiki Documentation
- Reference Probe Tests
- Session State Management
- Code Analysis Routing
- Backend Documentation Services
- Documentation Page Storage
- Subscription MCP Backend
- Application Error Types
- Filesystem Security
- Reference Resolution
- Configuration Security Boundary
- Static Viewer Template
- MCP Tool Registry
- Gitignore Filtering
- CLI Documentation Adapter
- Module Progress UI
- MCP Session Reading
- Documentation Pipeline State
- RepoWiki Skill Contract
- Analysis Timeout Handling
- Web Application Bootstrap
- CLI Module Integration
- Service Smoke Fixture
- CLI Backend Bridge
- Agent Tool Permissions
- Continuous Integration
- Reference Differential Testing
- Configuration Keyring
- Configuration Reset
- Symbol Resolution Indexes
- External Dependency Filtering
- Git Workflow Integration
- Static HTML Rendering
- Web Language Analyzers
- LLM Provider Catalog
- Brand Assets
- CodeWiki Framework
- Vertical Brand Assets
- Module Documentation Paths
- Client Service Calls
- Package Metadata
- Configuration File Paths
- Code File Discovery
- Owl Icon Assets
- Container Deployment
- CodeWiki Iconography
- Stale Page Cleanup
- Module Documentation
- Rust Package Metadata
- Go Service Fixture
- Backend Adapter Package
- CLI Commands Package
- CLI Package
- MCP Package
- MCP Tools Package
- Agent Tools Package
- C# Analysis Dispatch
- TypeScript Analysis Dispatch
- Backend Package
- Source Package
- Pre-commit Hook
- Parser Encoding Cache
- Differential Test Setup
- C++ Service Fixture
- C# Service Fixture
- Java Service Fixture
- JavaScript Service Fixture
- Kotlin Service Fixture
- PHP Service Fixture
- Python Service Fixture
- Ruby Service Fixture
- Rust Service Fixture
- Scala Service Fixture
- TypeScript Service Fixture
- CodeWiki Package Root
- Truncated Content Handling
- Core File Shortlisting
- Leaf Change Reports
- Web Routes Node
- FPT Logo Asset
- CodeWiki Logo Asset
- Owl Logo Asset
- CodeWiki Logo Variant
- CodeWiki Concept

## God Nodes (most connected - your core abstractions)
1. `Node` - 143 edges
2. `Config` - 62 edges
3. `TreeSitterTSAnalyzer` - 55 edges
4. `UpdateOptions` - 48 edges
5. `CallRelationship` - 46 edges
6. `TreeSitterJSAnalyzer` - 39 edges
7. `CallGraphAnalyzer` - 38 edges
8. `TreeSitterCSharpAnalyzer` - 37 edges
9. `TreeSitterCppAnalyzer` - 35 edges
10. `diff_graphs()` - 35 edges

## Surprising Connections (you probably didn't know these)
- `Path::Name Component IDs` --semantically_similar_to--> `Module Component Clustering`  [INFERRED] [semantically similar]
  reference/CodeWiki/CHANGELOG.md → engine/prompts/cluster_module.txt
- `Component-Level Incremental Updater` --semantically_similar_to--> `Component-Level Incremental Updater`  [INFERRED] [semantically similar]
  reference/CodeWiki/CHANGELOG.md → engine/prompts/update_leaf_system.txt
- `Generation and update prompt contracts` --semantically_similar_to--> `Five-phase wiki workflow`  [INFERRED] [semantically similar]
  skill/references/prompt-map.md → reference/CodeWiki/skills/codewiki-wiki-generator/SKILL.md
- `main()` --uses--> `DependencyParser`  [INFERRED]
  tools/reference_probe.py → reference/CodeWiki/codewiki/src/be/dependency_analyzer/ast_parser.py
- `names_of()` --uses--> `Node`  [INFERRED]
  reference/CodeWiki/codewiki/src/be/updater/reference_index.py → reference/CodeWiki/codewiki/src/be/dependency_analyzer/models/core.py

## Import Cycles
- None detected.

## Hyperedges (group relationships)
- **Repository Analysis to Packaged Wiki Workflow** — readme_repowiki, readme_rust_cli, readme_host_agent, readme_skill_package, engine_prompts_artifact_usage_artifact_behavior, engine_prompts_overview_artifact_addendum_build_configuration, reference_codewiki_changelog_artifact_aware_generation [INFERRED 0.85]
- **Module Documentation Architecture Pipeline** — engine_prompts_filter_folders_core_shortlisting, engine_prompts_cluster_repo_module_groups, engine_prompts_super_group_architectural_subsystems, engine_prompts_overview_repo_end_to_end_architecture, engine_prompts_system_complex_complex_docs, engine_prompts_system_leaf_leaf_docs, engine_prompts_user_module_documentation [INFERRED 0.85]
- **Incremental Wiki Maintenance and Repair** — engine_prompts_routing_user_orphan_routing, engine_prompts_update_leaf_system_incremental_updater, engine_prompts_update_leaf_system_write_set_roles, engine_prompts_stale_fix_system_stale_repair, engine_prompts_update_leaf_user_change_report, reference_codewiki_changelog_component_incremental_updater [INFERRED 0.85]
- **CodeWiki generation pipeline** — reference_codewiki_guides_cli_reference_codewiki_generate, reference_codewiki_guides_development_dependency_analysis_pipeline, reference_codewiki_skills_codewiki_wiki_generator_five_phase_workflow [INFERRED 0.75]
- **Artifact graph lifecycle** — reference_codewiki_guides_artifact_aware_generation_artifact_dependency_graph, reference_codewiki_guides_artifact_aware_generation_artifact_classes, reference_codewiki_guides_artifact_aware_generation_guaranteed_artifact_module, reference_codewiki_guides_development_artifact_analyzer, reference_codewiki_guides_cli_reference_artifact_analysis_flags [INFERRED 0.75]
- **Incremental update surface** — reference_codewiki_guides_incremental_updates_component_level_updates, reference_codewiki_guides_incremental_updates_dependency_graph_diff, reference_codewiki_guides_incremental_updates_full_build_fallback, reference_codewiki_guides_cli_reference_incremental_update_flags, reference_codewiki_guides_development_component_level_updater, reference_codewiki_skills_codewiki_wiki_generator_incremental_update_mode [INFERRED 0.75]
- **CodeWiki Documentation Pipeline** — reference_codewiki_readme_dependency_graph, reference_codewiki_readme_module_hierarchy, reference_codewiki_readme_recursive_agents, reference_codewiki_docs_backend_llm_documentation_services_documentation_generator_documentationgenerator, reference_codewiki_docs_cli_documentation_generation_five_stage_pipeline [EXTRACTED 1.00]
- **Tree-sitter to Documentation Graph Pipeline** — reference_codewiki_docs_c_family_tree_sitter_analyzers_tree_sitter, reference_codewiki_docs_c_family_tree_sitter_analyzers_call_graph_analyzer, reference_codewiki_docs_c_family_tree_sitter_analyzers_cross_file_resolution, reference_codewiki_docs_c_family_tree_sitter_analyzers_dependency_graph_builder, reference_codewiki_docs_backend_llm_documentation_services_documentation_generator_documentationgenerator [EXTRACTED 1.00]
- **Viewer Embedded State Contract** — codewiki_templates_github_pages_viewer_template_embedded_state, codewiki_templates_github_pages_viewer_template_hash_router, codewiki_templates_github_pages_viewer_template_document_rendering, reference_codewiki_docs_cli_html_viewer_tree_metadata_inputs [EXTRACTED 1.00]
- **Static Analysis Pipeline** — reference_codewiki_docs_code_analysis_engine_code_analysis_engine, reference_codewiki_docs_language_analyzers_language_analyzers, reference_codewiki_docs_dependency_analysis_service_dependency_analysis_service, reference_codewiki_docs_dependency_analyzer_core_dependency_analyzer_core, reference_codewiki_docs_documentation_generation_engine_documentation_generation_engine [EXTRACTED 1.00]
- **User Entry Points** — reference_codewiki_docs_user_interfaces_cli, reference_codewiki_docs_user_interfaces_frontend_web_app, reference_codewiki_docs_user_interfaces_mcp_session_management, reference_codewiki_docs_code_analysis_engine_code_analysis_engine, reference_codewiki_docs_documentation_generation_engine_documentation_generation_engine [EXTRACTED 1.00]
- **Web Request Processing Flow** — reference_codewiki_docs_frontend_web_app_web_routes_webroutes, reference_codewiki_docs_frontend_web_app_job_processing_backgroundworker, reference_codewiki_docs_frontend_web_app_github_config_githubrepoprocessor, reference_codewiki_docs_frontend_web_app_job_processing_cachemanager, reference_codewiki_docs_documentation_generation_engine_documentationgenerator [EXTRACTED 1.00]
- **Code-Themed Owl Logo** — reference_codewiki_codewiki_templates_github_pages_codewiki_icon_codewiki_icon, reference_codewiki_codewiki_templates_github_pages_codewiki_icon_stylized_owl, reference_codewiki_codewiki_templates_github_pages_codewiki_icon_code_chevrons [INFERRED 0.85]
- **Owl-and-Code Mascot Motif** — reference_codewiki_docs_codewiki_icon_image, reference_codewiki_docs_codewiki_icon_owl_mascot, reference_codewiki_docs_codewiki_icon_code_brackets [EXTRACTED 1.00]
- **CodeWiki visual identity** — reference_codewiki_img_black_background_logo_codewiki_logo, reference_codewiki_img_black_background_logo_owl_mascot, reference_codewiki_img_black_background_logo_code_symbols, reference_codewiki_img_black_background_logo_codewiki_wordmark [EXTRACTED 1.00]
- **Three-panel CodeWiki benchmark comparison** — reference_codewiki_img_benchmark_2_0_benchmark_results, reference_codewiki_img_benchmark_2_0_code_graph_expansion_c1, reference_codewiki_img_benchmark_2_0_artifacts_added_c2, reference_codewiki_img_benchmark_2_0_documentation_quality_by_stage [EXTRACTED 1.00]
- **Repository Decomposition Flow** — reference_codewiki_img_framework_overview_repository, reference_codewiki_img_framework_overview_ast_llm_parsing, reference_codewiki_img_framework_overview_dependency_graph, reference_codewiki_img_framework_overview_static_analysis, reference_codewiki_img_framework_overview_high_level_code_components, reference_codewiki_img_framework_overview_module_tree [EXTRACTED 1.00]
- **Recursive Documentation Loop** — reference_codewiki_img_framework_overview_module_tree, reference_codewiki_img_framework_overview_module_code, reference_codewiki_img_framework_overview_agent_input, reference_codewiki_img_framework_overview_recursive_agent, reference_codewiki_img_framework_overview_sub_agent [EXTRACTED 1.00]
- **Hierarchical Assembly Flow** — reference_codewiki_img_framework_overview_module_tree, reference_codewiki_img_framework_overview_recursive_order, reference_codewiki_img_framework_overview_child_module_documentations, reference_codewiki_img_framework_overview_llm, reference_codewiki_img_framework_overview_repo_overview_md [EXTRACTED 1.00]
- **CodeWiki vertical logo composition** — reference_codewiki_img_vertical_logo_codewiki_vertical_logo, reference_codewiki_img_vertical_logo_owl_emblem, reference_codewiki_img_vertical_logo_orange_code_angle_brackets, reference_codewiki_img_vertical_logo_codewiki_wordmark [EXTRACTED 1.00]

## Communities (164 total, 53 thin omitted)

### Community 0 - "Update Orchestration"
Cohesion: 0.04
Nodes (74): abc, dataclasses, LLMBackend, LLMBackend — unified abstraction over the API and subscription LLM paths.…, Abstract LLM backend used by the documentation generator. ``last_usage`` holds…, Single-shot text completion., build_reports(), _children_names() (+66 more)

### Community 1 - "Incremental Update Engine"
Cohesion: 0.06
Nodes (83): OrphanRouter, partition_leaf_nodes_by_structure(), chunk(), path_parts(), split(), Partition leaf nodes into batches that each satisfy ``fits``. Splits along the…, active_set(), body_hash() (+75 more)

### Community 2 - "LLM Backend Services"
Cohesion: 0.04
Nodes (69): AbstractEventLoop, caw, Context, contextlib, dotenv, mcp_server_fastmcp, pydantic_ai, CodeWikiDeps (+61 more)

### Community 3 - "Configuration Management"
Cohesion: 0.04
Nodes (77): keyring, keyring_errors, config_agent(), config_set(), config_show(), config_validate(), parse_patterns(), command (+69 more)

### Community 4 - "Core Graph Model"
Cohesion: 0.06
Nodes (69): builtins, json, os, pathlib, pathspec, pydantic, re, Logging utilities for CLI with colored output and progress tracking. (+61 more)

### Community 5 - "Rust Graph Analyzer"
Cohesion: 0.06
Nodes (77): artifactindex, command, add_resolution_language(), add_resolution_name(), ambiguous_same_language_dependencies_are_not_expanded(), AnalysisOutput, analyze(), AnalyzeOptions (+69 more)

### Community 6 - "Rust CLI Interface"
Cohesion: 0.08
Nodes (73): btreemap, clap, analyze_command(), AnalyzeArgs, Cli, close_session(), CloseSessionArgs, Command (+65 more)

### Community 7 - "Module Clustering"
Cohesion: 0.05
Nodes (65): ast, collections, collections_abc, Completer, MCP tool: get_prompt — serve CodeWiki's prompt templates to the IDE agent.…, _batch_fallback_name(), _cluster_batch_fits(), cluster_modules() (+57 more)

### Community 8 - "Rust Update Planning"
Cohesion: 0.08
Nodes (64): Default, ArtifactFile, ArtifactIndex, CallRelationship, ChangeSet, ComponentIndexEntry, GenerationInfo, Metadata (+56 more)

### Community 9 - "TypeScript Tree-sitter Analyzer"
Cohesion: 0.08
Nodes (17): Extract inheritance/implementation relationships, Record one relationship. Resolved callees are component ids in this file;…, Check if type name is a TypeScript/JavaScript built-in type., Get the parent context of a node for better top-level detection, Extract arrow function, Extract method entity (at any depth), qualified by its class., Extract lexical declaration entity (const/let)., Create Node object from entity data. (+9 more)

### Community 10 - "Workspace Edit Tools"
Cohesion: 0.05
Nodes (42): InsertLine, check_write_allowed(), EditTool, Filemap, flake8(), Flake8Error, format_flake8_output(), maybe_truncate() (+34 more)

### Community 11 - "Session Analysis State"
Cohesion: 0.08
Nodes (54): cleanup(), create(), is_expired(), load(), MAX_SESSIONS, module_tree_path(), output_dir(), prune() (+46 more)

### Community 12 - "Language Analysis Core"
Cohesion: 0.11
Nodes (41): analyze_file(), call_target(), collect_type_names(), Collector, Collector<'a>, Declaration, declaration_name(), DeclKind (+33 more)

### Community 13 - "CLI Branding"
Cohesion: 0.05
Nodes (52): click_testing, io, pytest, config_group(), group, Manage CodeWiki configuration (API credentials and settings)., cli(), main() (+44 more)

### Community 14 - "Documentation Generation"
Cohesion: 0.05
Nodes (44): colorama, CLIDocumentationGenerator, Any, Path, CLI adapter for documentation generator backend. This adapter wraps the…, Generate documentation with progress tracking. Returns: Completed…, Run the backend documentation generation with progress tracking., CLI adapter for documentation generation with progress reporting. This class… (+36 more)

### Community 15 - "Documentation Architecture Concepts"
Cohesion: 0.06
Nodes (56): Code Analysis Engine, Config, Core Config & Utils, FileManager, AnalysisService, CallGraphAnalyzer, Dependency Analysis Service, Dependency Analyzer Core (+48 more)

### Community 16 - "Documentation Generator Pipeline"
Cohesion: 0.06
Nodes (39): DocumentationGenerator, collect_modules(), IncompleteDocumentationError, Any, Exception, Check if a module is a leaf module (has no children or empty children)., Build structure for overview generation with 1-depth children doc paths and…, Recursively drop ``components`` lists — they dominate the tree's serialized… (+31 more)

### Community 17 - "JavaScript Tree-sitter Analyzer"
Cohesion: 0.10
Nodes (16): Get method name from method_definition node., Get field name from field_definition node., Check if field_definition contains an arrow function., Create a method node for relationship mapping., Extract class/abstract class/interface declaration., Extract export function or export default function, Extract arrow function or function expression from const/let/var declarations., Extract one relationship from a call_expression node. Plain identifier calls… (+8 more)

### Community 18 - "PHP Tree-sitter Analyzer"
Cohesion: 0.07
Nodes (26): NamespaceResolver, Analyzes PHP files using tree-sitter to extract nodes and relationships., Check if file is a PHP template that should be skipped., Get module path for the file., Get relative path from repo root., Generate component ID for a node., Parse and analyze the PHP file., Extract namespace and use statements from the AST. (+18 more)

### Community 19 - "Dependency Graph Analysis"
Cohesion: 0.08
Nodes (42): Render the artifact index as a compact text block, or ``""`` if none., render_artifact_index(), DependencyGraphBuilder, Any, Handles dependency analysis and graph building., Build and save dependency graph, returning components and leaf nodes. Returns:…, Dependency analyzer module for building and processing import dependency graphs…, compute_valid_leaf_types() (+34 more)

### Community 20 - "Python AST Analyzer"
Cohesion: 0.08
Nodes (21): AnnAssign, Assign, AsyncFunctionDef, Call, ClassDef, FunctionDef, Import, ImportFrom (+13 more)

### Community 21 - "Rust Documentation Model"
Cohesion: 0.11
Nodes (44): chrono, D, Deserialize, collect_expected_pages(), collect_metadata(), collect_processing(), document_path(), edit_document() (+36 more)

### Community 22 - "LLM Service Factory"
Cohesion: 0.06
Nodes (39): BadRequestError, ChatCompletion, inspect, OpenAI, openai_types, OpenAIChatModel, OpenAIChatModelSettings, pydantic_ai_exceptions (+31 more)

### Community 23 - "Module Tree Utilities"
Cohesion: 0.11
Nodes (42): copy, add_component(), ancestors(), components_of(), copy_tree(), insert_leaf(), is_leaf(), iter_leaves() (+34 more)

### Community 24 - "Rust Prompt Templates"
Cohesion: 0.09
Nodes (36): anyhow, catalog(), catalog_keeps_reference_prompt_names(), catalog_specs(), CLUSTER_OPTIONAL, CLUSTER_REQUIRED, CUSTOM_INSTRUCTIONS_OPTIONAL, FILTER_FOLDERS_REQUIRED (+28 more)

### Community 25 - "CodeWiki Architecture Concepts"
Cohesion: 0.05
Nodes (42): Artifact-Dependent Build Behavior, Artifact Source Citation, Essential Artifact Components, Module Component Clustering, Repository Module Groups, Repository Component Clustering, Artifact Index, Build, Deployment and Configuration (+34 more)

### Community 26 - "Generate Command Validation"
Cohesion: 0.07
Nodes (40): _detect_changed_files(), generate_command(), _invalidate_affected_modules(), _find_affected(), parse_patterns(), command, option, pass_context (+32 more)

### Community 27 - "C++ Tree-sitter Analyzer"
Cohesion: 0.09
Nodes (11): Strip ALL_CAPS attribute/specifier macros that sit in front of a declaration so…, Recursively extract top-level nodes (classes, functions, global variables)., Check if a declaration node is a global variable., Extract the declared function or method name from nested declarators., Find the class that contains this method definition., Find the function that contains this node., Find the function or method that contains this node., Find the class that contains this node. (+3 more)

### Community 28 - "C# Tree-sitter Analyzer"
Cohesion: 0.13
Nodes (7): Collect ``using`` directives (plain / alias / static) and the file-scoped…, The namespace in scope at ``node``: the file-scoped namespace (if any) followed…, Collect a ``///`` XML doc-comment block immediately preceding the declaration…, First base type of the type enclosing ``node`` (for ``base.X()``)., Types that can *never* be a project component: language primitives and generic…, Resolve a member call on a known receiver type to a `Type.member` candidate the…, TreeSitterCSharpAnalyzer

### Community 29 - "Replay Differential Testing"
Cohesion: 0.14
Nodes (35): canonical_analysis(), canonical_json(), canonical_metadata(), canonical_tree(), Any, Path, Canonical output helpers for the offline CodeWiki replay tests. The runtime…, Replace run-local path prefixes recursively without changing shapes. (+27 more)

### Community 30 - "Scala Tree-sitter Analyzer"
Cohesion: 0.16
Nodes (7): The logical name a class/trait/object/enum is registered under., Walk up to the nearest enclosing class/trait/object/enum and return its owner…, Walk up to the nearest enclosing method/function and return its logical (owner-…, Prefer the enclosing method; fall back to the enclosing type for expressions in…, Get the primary type name from a type node, stripping generics., Best-effort resolution of a local variable's declared type: the enclosing…, TreeSitterScalaAnalyzer

### Community 31 - "Repository Analysis Service"
Cohesion: 0.09
Nodes (19): AnalysisService, analyze_repository_structure_only(), Any, Perform complete repository analysis including call graph generation. Args:…, Perform lightweight structure-only analysis without call graph generation.…, Clone repository and return temp dir path., Parse GitHub URL and extract repository metadata., Analyze repository file structure with filtering. (+11 more)

### Community 32 - "Backend Configuration"
Cohesion: 0.07
Nodes (20): FallbackModel, get_backend(), Return the backend instance matching ``config.provider``., _call_llm_via_litellm(), create_fallback_models(), pop_last_usage(), Create fallback models chain from configuration., Call LLM via litellm for Bedrock/Anthropic providers. litellm handles the… (+12 more)

### Community 33 - "Java Tree-sitter Analyzer"
Cohesion: 0.15
Nodes (6): Check if type is a Java primitive or a JDK/runtime type., Types that can never be project components: primitives, JDK/runtime types, and…, Get relative path from repo root., Get identifier name from a node., Get type name from a type node., TreeSitterJavaAnalyzer

### Community 34 - "Artifact Graph Analysis"
Cohesion: 0.10
Nodes (29): fnmatch, posixpath, analyze_artifacts(), _make_node(), _walk_exports(), ArtifactAnalysis, build_artifact_index(), _dockerfile_units() (+21 more)

### Community 35 - "Repository Filtering Tests"
Cohesion: 0.14
Nodes (25): MonkeyPatch, Get current configuration. Returns: Configuration object or None if not loaded, Configuration, CodeWiki configuration data model. Attributes: base_url: LLM API base URL…, Convert to dictionary., Create Configuration from dictionary. Args: data: Configuration dictionary…, Check if all required fields are set. Subscription-mode providers (claude-code,…, Create AgentInstructions from dictionary. (+17 more)

### Community 36 - "Web Application Routes"
Cohesion: 0.14
Nodes (23): datetime, queue, Background worker for processing documentation generation jobs., Cache management for documentation generation results., Configuration class for web application settings., Configuration settings for the CodeWiki web application., WebAppConfig, GitHubRepoProcessor (+15 more)

### Community 37 - "Kotlin Tree-sitter Analyzer"
Cohesion: 0.13
Nodes (12): Extract class modifiers (abstract, data, enum, annotation, etc.)., Check if type is a Kotlin primitive or common built-in type., Get identifier name from a node., Get the primary type name from a type node, stripping generics., Get relative path from repo root., Get the root identifier from a chain of navigation_expressions., Walk up to find the containing class/object/interface name., Find the component ID of the containing class. (+4 more)

### Community 38 - "MCP Server Tools"
Cohesion: 0.11
Nodes (27): mcp_server_stdio, mcp_types, call_tool(), _legacy_generate_docs(), _legacy_get_module_tree(), _summarize_tree(), _load_config(), main() (+19 more)

### Community 39 - "Recursive Module Workflow"
Cohesion: 0.11
Nodes (28): Agent Input, AST/LLM Parsing, Child Module Documentations, Dependency Graph, High-level Code Components, LLM, Lower-level Component, Manageable Modules (+20 more)

### Community 40 - "Web Route Handlers"
Cohesion: 0.10
Nodes (16): HTMLResponse, RedirectResponse, Validate if the URL is a valid GitHub repository URL., Extract repository information from GitHub URL., Request, API endpoint to get job status., View generated documentation., Serve generated documentation files. (+8 more)

### Community 41 - "Web Viewer Application"
Cohesion: 0.10
Nodes (22): argparse, fastapi, fastapi_responses, fastapi_staticfiles, markdown_it, post, Startup script for CodeWiki Web Application, HTML templates for the CodeWiki web application. (+14 more)

### Community 42 - "CLI Interaction Utilities"
Cohesion: 0.10
Nodes (20): click, APIErrorHandler, Exception, LLM API error handling utilities with fail-fast behavior., Wrap an API call with error handling. Args: func: Function to call *args:…, Handler for LLM API errors with fail-fast behavior., Handle LLM API error and convert to APIError. Args: error: The original…, Display API error with formatting. Args: error: The API error module_name:… (+12 more)

### Community 43 - "Git Integration"
Cohesion: 0.09
Nodes (15): git, git_exc, GitManager, Path, Git operations manager for CodeWiki CLI., Commit generated documentation. Args: docs_path: Path to documentation…, Manages git operations for documentation generation. Handles: - Status checking…, Get remote repository URL. Args: remote_name: Name of remote (default: origin)… (+7 more)

### Community 44 - "Background Job Worker"
Cohesion: 0.10
Nodes (12): BackgroundWorker, Save job statuses to disk., Process a single documentation generation job., Background worker for processing documentation generation jobs., Start the background worker thread., Stop the background worker., Add a job to the processing queue., Get job status by ID. (+4 more)

### Community 45 - "MCP Workflow Concepts"
Cohesion: 0.10
Nodes (25): Artifact classes, Artifact dependency graph, Guaranteed artifact module, Artifact analysis flags, codewiki generate command, Incremental update flags, codewiki mcp command, Artifact analyzer (+17 more)

### Community 46 - "Ruby Tree-sitter Analyzer"
Cohesion: 0.21
Nodes (5): Get relative path from repo root., Emit an edge, resolving against the same-file symbol table., Flatten a constant / Foo::Bar scope_resolution to dotted text., Walk the enclosing method for `receiver_name = Const.new` to infer the…, TreeSitterRubyAnalyzer

### Community 47 - "Documentation Quality Metrics"
Cohesion: 0.17
Nodes (22): Artifacts added (C2), CodeWiki benchmark-2.0 results, C1 code graph stage, C2 artifacts stage, Code graph expansion (C1), CodeWiki (C1 + C2), CodeWiki (conference version), CodeWiki without artifacts (C1 only) (+14 more)

### Community 48 - "Reference Replay Testing"
Cohesion: 0.23
Nodes (19): difflib, canonical_json(), compare(), copy_reference_fixture(), DifferentialFailure, load_json(), main(), parse_args() (+11 more)

### Community 49 - "Session Persistence"
Cohesion: 0.27
Nodes (17): Remove a session. Returns True if it existed., In-memory store for all active MCP sessions (thread-safe)., SessionStore, _make_node(), _make_session(), Tests for save_module_tree validation of component ids. Verifies that a module…, _read_validation_file(), _save() (+9 more)

### Community 50 - "Multi-language Analysis Dispatch"
Cohesion: 0.10
Nodes (10): Analyze a single code file based on its language. Routes to appropriate…, Analyze Python file using Python AST analyzer. Args: file_path: Relative path…, Analyze JavaScript file using tree-sitter based AST analyzer Args: file_path:…, Analyze C file using tree-sitter based analyzer. Args: file_path: Relative path…, Analyze C++ file using tree-sitter based analyzer. Args: file_path: Relative…, Analyze Java file using tree-sitter based analyzer. Args: file_path: Relative…, Analyze Kotlin file using tree-sitter based analyzer. Args: file_path: Relative…, Analyze PHP file using tree-sitter based analyzer. Args: file_path: Relative… (+2 more)

### Community 51 - "C-family Parser Frontend"
Cohesion: 0.13
Nodes (20): C and C++ Analyzers, C and C++ Tree-sitter Analyzers, C++ Macro Recovery and Contextual Header Routing, CallGraphAnalyzer, Cross-File Resolution and External Filtering, C# Tree-sitter Analyzer, Resolve First, Filter Second, TreeSitterCSharpAnalyzer (+12 more)

### Community 52 - "Document Editing Tools"
Cohesion: 0.17
Nodes (18): asyncio, _ensure_parent_dirs(), handle_edit_doc_file(), handle_write_doc_file(), _is_within(), Any, Path, MCP tools: write_doc_file + edit_doc_file. These tools create and edit markdown… (+10 more)

### Community 53 - "Analysis Cache Management"
Cohesion: 0.17
Nodes (11): CacheManager, Remove documentation from cache., Remove expired cache entries., Manages documentation cache., Load cache index from disk., Save cache index to disk., Generate hash for repository URL., Get cached documentation path if available. (+3 more)

### Community 54 - "HTML Documentation Viewer"
Cohesion: 0.19
Nodes (11): HTMLGenerator, Any, Path, Generates static HTML documentation viewer for GitHub Pages. Creates a self-…, Build HTML content for repo info section. Args: metadata: Metadata dictionary…, Initialize HTML generator. Args: template_dir: Path to template directory…, Escape HTML special characters. Args: text: Text to escape Returns: Escaped text, Detect repository information from git. Args: repo_path: Repository path… (+3 more)

### Community 55 - "CLI Logging"
Cohesion: 0.12
Nodes (9): CLILogger, Logger for CLI with support for verbose and normal modes., Initialize the logger. Args: verbose: Enable verbose output, Log debug message (only in verbose mode)., Log success message in green., Log warning message in yellow., Log error message in red., Log a processing step. Args: message: Step description step: Current step… (+1 more)

### Community 56 - "Module Tree Management"
Cohesion: 0.18
Nodes (16): _cap(), _collect_component_ids(), _walk(), _get_processing_order(), _collect(), handle_get_processing_order(), handle_save_module_tree(), Any (+8 more)

### Community 57 - "Call Graph Resolution"
Cohesion: 0.17
Nodes (7): CallGraphAnalyzer, Resolve function call relationships across all languages. Attempts to match…, Project packages/namespaces, partitioned by language. Java and C# share the…, Initialize the call graph analyzer., The package (Java) / namespace (C#) a node lives in, derived from its dotted…, Generate clean format optimized for LLM consumption., Select the most connected nodes from the call graph. Args: target_count: The…

### Community 58 - "Entry Point Detection"
Cohesion: 0.12
Nodes (15): find_fallback_connectivity_files(), find_fallback_entry_points(), get_function_patterns_for_language(), has_high_connectivity_potential(), is_critical_function(), is_entry_point_file(), is_entry_point_path(), Code analysis patterns for different programming languages. This module… (+7 more)

### Community 59 - "Documentation Web Serving"
Cohesion: 0.15
Nodes (16): get_file_title(), index(), initialize_globals(), load_module_tree(), main(), markdown_to_html(), get, Path (+8 more)

### Community 60 - "Artifact Classification"
Cohesion: 0.19
Nodes (16): fixture, ArtifactOptions, classify_artifact(), Knobs for :func:`analyze_artifacts`., Return the artifact class of ``rel_path`` or ``None`` when it is not one.…, _artifact_nodes(), mini_repo(), parametrize (+8 more)

### Community 61 - "Prompt Template System"
Cohesion: 0.23
Nodes (15): _fence_language(), Markdown fence language for ``path`` (falls back to ``text``)., _clip(), format_routing_prompt(), format_stale_prompt(), format_update_user_prompt(), Any, Prompts for the incremental updater's agents. Three agents: the per-leaf… (+7 more)

### Community 62 - "Web Template Rendering"
Cohesion: 0.17
Nodes (12): BaseLoader, jinja2, Any, Custom Jinja2 loader for string templates., Template utilities for FastAPI applications using Jinja2., Render template using Jinja2. Args: template: HTML template string with Jinja2…, Render navigation HTML from module tree structure. Args: module_tree:…, Render job list HTML. Args: jobs: List of job objects Returns: HTML string for… (+4 more)

### Community 63 - "Progress Tracking"
Cohesion: 0.17
Nodes (8): ProgressTracker, Progress tracker with stages and ETA estimation. Stages: 1. Dependency Analysis…, Get overall progress percentage. Returns: Progress (0.0 to 1.0), Estimate time remaining. Returns: ETA string or None if cannot estimate, Initialize progress tracker. Args: total_stages: Number of stages verbose:…, Start a new stage. Args: stage: Stage number (1-5) description: Optional custom…, Update progress within current stage. Args: progress: Progress percentage (0.0…, Complete current stage. Args: message: Optional completion message

### Community 64 - "Change Detection"
Cohesion: 0.20
Nodes (15): _detect_changes(), _detect_via_git(), _add(), _normalize(), _detect_via_mtime(), _find_affected_modules(), _walk(), handle_analyze_repo() (+7 more)

### Community 65 - "Session Workspace Files"
Cohesion: 0.16
Nodes (10): Any, Path, Sanitize a component ID for use as a filename. Component IDs look like…, Manages the on-disk workspace for a single MCP session., Write *data* as pretty-printed JSON and return the file path., Write a single component's source code to the ``sources/`` dir., Read a JSON file from the workspace. Returns ``None`` if missing., Remove the session directory and try to prune empty parents. (+2 more)

### Community 66 - "Repository Analyzer"
Cohesion: 0.20
Nodes (6): True when ``path`` is an artifact the default ignore list must not drop.…, RepoAnalyzer, build_tree(), test_repo_analyzer_whitelist(), _tree_paths(), _walk()

### Community 67 - "Scala Analyzer Tests"
Cohesion: 0.31
Nodes (14): analyze_scala_file(), _analyze(), Path, Tests for the tree-sitter based Scala analyzer., test_curried_method_parameters_are_not_truncated(), test_dependency_parser_end_to_end(), test_empty_file_returns_no_components(), test_extracts_call_relationships() (+6 more)

### Community 68 - "Dependency Parser Tests"
Cohesion: 0.22
Nodes (9): DependencyParser, Parser for extracting code components from multi-language repositories., _analyze(), Path, Tests for the tree-sitter based Ruby analyzer., test_dependency_parser_end_to_end(), test_extracts_call_relationships(), test_extracts_docstring_and_parameters() (+1 more)

### Community 69 - "Skill Package Validation"
Cohesion: 0.34
Nodes (13): NoReturn, compare_packages(), fail(), load_directory(), load_package(), load_zip(), LoadedPackage, main() (+5 more)

### Community 70 - "Repository Cloning"
Cohesion: 0.18
Nodes (12): cleanup_repository(), cleanup_repository_safe(), clone_repository(), parse_github_url(), Sanitize GitHub URL to ensure proper format and remove extra path components.…, Windows-safe removal of the cloned repository directory. Handles read-only…, Remove the cloned repository directory (wrapper for backward compatibility).…, Parse GitHub URL to extract owner and repository name. Args: github_url: GitHub… (+4 more)

### Community 71 - "C Tree-sitter Analyzer"
Cohesion: 0.27
Nodes (4): Extract various types of relationships between top-level nodes., Find the function that contains this node., Recursively extract top-level nodes (functions, structs, and global variables)., TreeSitterCAnalyzer

### Community 72 - "File Persistence"
Cohesion: 0.17
Nodes (8): FileManager, Any, Handles file I/O operations., Create directory if it doesn't exist., Save data as JSON to file., Load JSON from file, return None if file doesn't exist., Save text content to file., Load text content from file.

### Community 73 - "CodeWiki Documentation"
Cohesion: 0.24
Nodes (12): Artifact-Aware and Incremental Features, CodeWiki Homepage, CodeWiki Framework Architecture, Multimodal Documentation Synthesis, Recursive Agentic System, Artifact-Aware and Incremental Generation, CodeWiki, CodeWiki ACL 2026 Paper (+4 more)

### Community 74 - "Reference Probe Tests"
Cohesion: 0.29
Nodes (11): canonical_components(), language_for(), main(), normalize_relative_path(), parse_args(), ProbeFailure, Any, Namespace (+3 more)

### Community 75 - "Session State Management"
Cohesion: 0.18
Nodes (6): Return the session or ``None`` if not found / expired., Remove all expired sessions. Caller must hold _lock., Mutable state shared across all MCP tool calls within a session., Update the last-accessed timestamp., Create a new session and return it., SessionState

### Community 76 - "Code Analysis Routing"
Cohesion: 0.18
Nodes (5): Route ambiguous .h headers per file. A header is parsed as C++ when its own…, Dotted module paths for every Python file in the repository., Complete analysis: Analyze all files to build complete call graph with all…, Deduplicate call relationships based on caller-callee pairs. Removes duplicate…, Generate visualization data for graph rendering. Creates Cytoscape.js…

### Community 77 - "Backend Documentation Services"
Cohesion: 0.33
Nodes (11): CawBackend, Backend LLM and Documentation Services, DocumentationGenerator Orchestrator, get_backend Provider Factory, LLMBackend Abstraction, CachingOpenAIModel, call_llm Completion Path, pydantic-ai Backend (+3 more)

### Community 78 - "Documentation Page Storage"
Cohesion: 0.29
Nodes (9): hashlib, changed_pages(), list_pages(), page_exists(), page_hashes(), page_path(), Small helpers over the flat docs directory (page stems <-> files, hashes)., Pages created, removed, or whose bytes changed. (+1 more)

### Community 79 - "Subscription MCP Backend"
Cohesion: 0.29
Nodes (10): CodeWikiDeps Context, Backend Agent Tools, EditTool, Mermaid Diagram Validation, Agent Path Confinement, CawBackend Implementation, CawToolKit MCP Toolkit, Subscription caw Backend (+2 more)

### Community 80 - "Application Error Types"
Cohesion: 0.22
Nodes (3): CodeWikiError, Exception, Base exception for CodeWiki CLI errors.

### Community 81 - "Filesystem Security"
Cohesion: 0.42
Nodes (7): Find and read the README file from the repository root., assert_safe_path(), _inside(), Path, Read at most ``max_bytes`` of ``target`` with the same symlink/escape checks as…, safe_open_text(), safe_read_head()

### Community 82 - "Reference Resolution"
Cohesion: 0.22
Nodes (5): artifact_file_node_id(), Map textual references (paths, ``module:function``) to known node ids., Collect ids referenced by shell-ish text (CI ``run:`` blocks, RUN lines,…, _refs_from_shell_text(), _Resolver

### Community 83 - "Configuration Security Boundary"
Cohesion: 0.28
Nodes (9): AgentInstructions, Backend Config Bridge, Configuration Model, CLI Configuration, Secure API-Key Storage, Security Policy, Private GitHub Vulnerability Reporting, Repository-to-System Trust Boundary (+1 more)

### Community 84 - "Static Viewer Template"
Cohesion: 0.32
Nodes (8): GitHub Pages Viewer Template, Markdown, Mermaid, and Code Rendering, Embedded Viewer State, Hash-Based Document Router, Static Documentation Viewer Template, CLI HTML Viewer, HTMLGenerator, Safe Template-Based Rendering

### Community 85 - "MCP Tool Registry"
Cohesion: 0.32
Nodes (8): _fine_grained_tools(), _legacy_tools(), list_tools(), Tool, Return the legacy tools that require CodeWiki LLM configuration., List all available CodeWiki MCP tools., Return the zero-config, IDE-driven tool set., test_mcp_analysis_tools_default_to_gitignore_enabled()

### Community 86 - "Gitignore Filtering"
Cohesion: 0.32
Nodes (4): GitIgnoreFilter, Path, Return whether a repository-relative path should be ignored., Evaluate Git ignore rules once per repository analysis. Git repositories use…

### Community 87 - "CLI Documentation Adapter"
Cohesion: 0.32
Nodes (8): CLIDocumentationGenerator Adapter, CLI Documentation Generation, Five-Stage Generation Pipeline, Documentation Job Models, Progress Tracking and Completeness Validation, CLILogger, CLI Utilities, ProgressTracker

### Community 88 - "Module Progress UI"
Cohesion: 0.29
Nodes (4): ModuleProgressBar, Progress bar for module-by-module generation., Initialize module progress bar. Args: total_modules: Total number of modules to…, Update progress for a module. Args: module_name: Name of the module cached:…

### Community 89 - "MCP Session Reading"
Cohesion: 0.29
Nodes (5): Session state management for the CodeWiki MCP Server. Each ``analyze_repo``…, handle_read_code_components(), Any, MCP tool: read_code_components — write component source code to disk. Instead…, Write the source code for given component IDs to workspace files. Returns a…

### Community 90 - "Documentation Pipeline State"
Cohesion: 0.52
Nodes (7): Module Clustering, Documentation Generator, DocumentationGenerator Pipeline, Leaf-First Processing Order, Persistent Module Tree State, Overview Synthesis and Completeness Validation, Module Tree and Metadata Inputs

### Community 91 - "RepoWiki Skill Contract"
Cohesion: 0.33
Nodes (7): RepoWiki skill registration, Document editing and output contract, JSON command contract, Module-tree validation, Bundled Rust CLI, Wiki output contract, RepoWiki wiki-generation skill

### Community 92 - "Analysis Timeout Handling"
Cohesion: 0.33
Nodes (6): Exception, Raised when file parsing exceeds timeout., Context manager for timeout on file parsing., timeout(), signal_handler(), TimeoutError

### Community 93 - "Web Application Bootstrap"
Cohesion: 0.33
Nodes (4): Ensure all required directories exist., Get absolute path for a given relative path., main(), Main function to run the web application.

### Community 94 - "CLI Module Integration"
Cohesion: 0.33
Nodes (6): CLI Module, ConfigManager, CLI Module, CLIDocumentationGenerator, GitManager, HTML Viewer

### Community 95 - "Service Smoke Fixture"
Cohesion: 0.50
Nodes (3): helper(), A small service used by the smoke fixture., Service

### Community 96 - "CLI Backend Bridge"
Cohesion: 0.40
Nodes (3): Convert CLI Configuration to Backend Config. This method bridges the gap…, Any, Create configuration for CLI context. Args: repo_path: Repository path…

### Community 97 - "Agent Tool Permissions"
Cohesion: 0.40
Nodes (4): _patch_claude_allowed_tools(), Popen(), Append ``--allowedTools mcp__<server>,...`` to a ``claude`` command. Pure…, _with_allowed_tools()

### Community 98 - "Continuous Integration"
Cohesion: 0.83
Nodes (4): GitHub Actions CI Workflow, Changed-File Lint Job, Automated Test Job, GitHub Actions CI

### Community 99 - "Reference Differential Testing"
Cohesion: 0.50
Nodes (4): Offline Differential Validation, Golden Data, Preview, and Archive, Reference Differential Tests, Pinned CodeWiki Upstream Commit

### Community 104 - "Git Workflow Integration"
Cohesion: 0.83
Nodes (4): Documentation Branch, Commit, and PR URL, CLI Git Integration, GitManager, RepositoryError

### Community 105 - "Static HTML Rendering"
Cohesion: 0.50
Nodes (4): Markdown Rendering, Mermaid Rendering, Embedded Module Tree, Static HTML Viewer

### Community 106 - "Web Language Analyzers"
Cohesion: 0.50
Nodes (4): JavaScript Analyzer, TreeSitterJSAnalyzer, TreeSitterTSAnalyzer, TypeScript Analyzer

### Community 107 - "LLM Provider Catalog"
Cohesion: 0.67
Nodes (4): LLM backend abstraction, Main, cluster, and fallback model roles, Provider catalog, Subscription mode

### Community 108 - "Brand Assets"
Cohesion: 0.50
Nodes (4): Code symbols, CodeWiki logo, CodeWiki wordmark, Owl mascot

### Community 109 - "CodeWiki Framework"
Cohesion: 0.50
Nodes (4): CodeWiki Framework Overview, Hierarchical Assembly and Documentation Synthesis, Recursive Documentation Generation, Repository Analysis and Hierarchical Module Decomposition

### Community 110 - "Vertical Brand Assets"
Cohesion: 0.50
Nodes (4): CodeWiki vertical logo, CodeWiki wordmark, Orange code angle brackets, Owl emblem

### Community 111 - "Module Documentation Paths"
Cohesion: 0.67
Nodes (3): Trimmed Module Tree, Child Module Documentation Paths, Module Overview

### Community 116 - "Owl Icon Assets"
Cohesion: 0.67
Nodes (3): Code Chevrons, CodeWiki Icon, Stylized Owl

### Community 117 - "Container Deployment"
Cohesion: 1.00
Nodes (3): CodeWiki Container Service, Docker Compose Deployment, Persistent Output Volume

### Community 118 - "CodeWiki Iconography"
Cohesion: 0.67
Nodes (3): Code Brackets, CodeWiki Icon, Owl Mascot

## Knowledge Gaps
- **145 isolated node(s):** `codewiki`, `MAX_ARTIFACT_FILES_PER_CLASS`, `MAX_ARTIFACT_BYTES`, `RESERVED_STEMS`, `VERSION` (+140 more)
  These have ≤1 connection - possible missing edges or undocumented components. (Counts symbols only; 1124 node(s) total have ≤1 connection when file, concept and rationale nodes are included.)
- **53 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `Node` connect `Core Graph Model` to `Update Orchestration`, `Incremental Update Engine`, `LLM Backend Services`, `Module Clustering`, `TypeScript Tree-sitter Analyzer`, `JavaScript Tree-sitter Analyzer`, `PHP Tree-sitter Analyzer`, `Dependency Graph Analysis`, `Python AST Analyzer`, `Module Tree Utilities`, `C++ Tree-sitter Analyzer`, `C# Tree-sitter Analyzer`, `Scala Tree-sitter Analyzer`, `Backend Configuration`, `Java Tree-sitter Analyzer`, `Artifact Graph Analysis`, `Kotlin Tree-sitter Analyzer`, `Ruby Tree-sitter Analyzer`, `Session Persistence`, `Call Graph Resolution`, `Artifact Classification`, `Prompt Template System`, `Scala Analyzer Tests`, `Dependency Parser Tests`, `C Tree-sitter Analyzer`, `Session State Management`, `MCP Session Reading`?**
  _High betweenness centrality (0.167) - this node is a cross-community bridge._
- **Why does `Config` connect `Backend Configuration` to `Change Detection`, `Update Orchestration`, `LLM Backend Services`, `Repository Filtering Tests`, `Configuration Management`, `Core Graph Model`, `MCP Server Tools`, `Module Clustering`, `CLI Backend Bridge`, `Web Application Routes`, `Background Job Worker`, `Documentation Generation`, `Documentation Generator Pipeline`, `Dependency Graph Analysis`, `LLM Service Factory`?**
  _High betweenness centrality (0.052) - this node is a cross-community bridge._
- **Why does `CallGraphAnalyzer` connect `Call Graph Resolution` to `C# Analysis Dispatch`, `TypeScript Analysis Dispatch`, `Core Graph Model`, `Symbol Resolution Indexes`, `External Dependency Filtering`, `Code Analysis Routing`, `Multi-language Analysis Dispatch`, `Code File Discovery`, `Repository Analysis Service`?**
  _High betweenness centrality (0.046) - this node is a cross-community bridge._
- **Are the 71 inferred relationships involving `Node` (e.g. with `SessionState` and `SessionStore`) actually correct?**
  _`Node` has 71 INFERRED edges - model-reasoned connections that need verification._
- **Are the 26 inferred relationships involving `Config` (e.g. with `CLIDocumentationGenerator` and `Configuration`) actually correct?**
  _`Config` has 26 INFERRED edges - model-reasoned connections that need verification._
- **Are the 2 inferred relationships involving `TreeSitterTSAnalyzer` (e.g. with `CallRelationship` and `Node`) actually correct?**
  _`TreeSitterTSAnalyzer` has 2 INFERRED edges - model-reasoned connections that need verification._
- **What connects `codewiki`, `MAX_ARTIFACT_FILES_PER_CLASS`, `MAX_ARTIFACT_BYTES` to the rest of the system?**
  _145 weakly-connected nodes found - possible documentation gaps or missing edges._