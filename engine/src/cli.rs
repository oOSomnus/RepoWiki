use crate::analyzer::{self, AnalyzeOptions};
use crate::docs::{self, EditOperation};
use crate::html;
use crate::model::{ModuleTree, Node, UpdateOptions};
use crate::prompts::{self, PromptType};
use crate::session::{self, SessionState};
use crate::update;
use anyhow::{anyhow, Context, Result};
use clap::{error::ErrorKind, Args, Parser, Subcommand, ValueEnum};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(
    name = "codewiki",
    version,
    about = "Agent-driven repository documentation"
)]
pub struct Cli {
    #[arg(
        long = "repo-root",
        global = true,
        help = "Repository containing the session workspace"
    )]
    session_repo: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

fn parse_update_rung(value: &str) -> std::result::Result<String, String> {
    if update::VALID_RUNGS.contains(&value) {
        Ok(value.to_string())
    } else {
        Err(format!(
            "invalid update rung '{value}'; expected one of {}",
            update::VALID_RUNGS.join(", ")
        ))
    }
}

#[derive(Debug, Subcommand)]
enum Command {
    Version,
    Generate(GenerateArgs),
    Analyze(AnalyzeArgs),
    Components {
        #[command(subcommand)]
        command: ComponentsCommand,
    },
    Prompt {
        #[command(subcommand)]
        command: PromptCommand,
    },
    Tree {
        #[command(subcommand)]
        command: TreeCommand,
    },
    Doc {
        #[command(subcommand)]
        command: DocCommand,
    },
    Update {
        #[command(subcommand)]
        command: UpdateCommand,
    },
    Html(HtmlArgs),
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },
}

#[derive(Debug, Args, Clone)]
struct CommonAnalysisArgs {
    #[arg(long, default_value = ".")]
    repo: PathBuf,
    #[arg(long, default_value = ".repowiki")]
    output: PathBuf,
    #[arg(long, action = clap::ArgAction::Append)]
    include: Vec<String>,
    #[arg(long, action = clap::ArgAction::Append)]
    exclude: Vec<String>,
    #[arg(long)]
    focus: Option<String>,
    #[arg(long)]
    doc_type: Option<String>,
    #[arg(long)]
    instructions: Option<String>,
    #[arg(long, default_value_t = 2)]
    max_depth: usize,
    #[arg(long, default_value_t = 36_369)]
    max_token_per_module: usize,
    #[arg(long, default_value_t = 16_000)]
    max_token_per_leaf_module: usize,
    #[arg(long, default_value_t = 200_000)]
    artifact_token_budget: usize,
    #[arg(long, action = clap::ArgAction::Append)]
    artifact_exclude: Vec<String>,
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    gitignore: bool,
    #[arg(long, default_value_t = false)]
    no_gitignore: bool,
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    artifacts: bool,
    #[arg(long, default_value_t = false)]
    no_artifacts: bool,
    #[arg(long, default_value_t = false)]
    with_prose: bool,
}

#[derive(Debug, Args)]
struct AnalyzeArgs {
    #[command(flatten)]
    common: CommonAnalysisArgs,
    #[arg(long)]
    session: Option<String>,
}

#[derive(Debug, Args)]
struct GenerateArgs {
    #[command(flatten)]
    common: CommonAnalysisArgs,
    #[arg(long)]
    session: Option<String>,
    #[arg(long, default_value_t = false)]
    update: bool,
    #[command(flatten)]
    update_options: UpdateArgs,
}

#[derive(Debug, Args, Clone)]
struct UpdateArgs {
    #[arg(long, default_value = "3", value_parser = parse_update_rung)]
    rung: String,
    #[arg(long, default_value_t = 0.95)]
    tau_ren: f64,
    #[arg(long, default_value_t = 8000)]
    max_diff_tokens: usize,
    #[arg(long, default_value_t = 0.5)]
    tau_nb: f64,
    #[arg(long, default_value_t = 0.33)]
    tau_grow: f64,
    #[arg(long)]
    k_hop: Option<usize>,
    #[arg(long, default_value_t = 0.5)]
    tau_full: f64,
    #[arg(long, default_value_t = 0.3)]
    tau_tree: f64,
}

impl From<UpdateArgs> for UpdateOptions {
    fn from(value: UpdateArgs) -> Self {
        let rung = value.rung;
        Self {
            k_hop: value
                .k_hop
                .unwrap_or_else(|| if rung == "3b" { 2 } else { 1 }),
            rung,
            tau_ren: value.tau_ren,
            max_diff_tokens: value.max_diff_tokens,
            tau_nb: value.tau_nb,
            tau_grow: value.tau_grow,
            tau_full: value.tau_full,
            tau_tree: value.tau_tree,
        }
    }
}

#[derive(Debug, Subcommand)]
enum ComponentsCommand {
    Read(ReadComponentsArgs),
}

#[derive(Debug, Args)]
struct ReadComponentsArgs {
    #[arg(long)]
    session: String,
    #[arg(long)]
    ids_file: Option<PathBuf>,
    #[arg(long, value_delimiter = ',')]
    ids: Vec<String>,
}

#[derive(Debug, Subcommand)]
enum PromptCommand {
    Get(GetPromptArgs),
    List,
}

#[derive(Debug, Args)]
struct GetPromptArgs {
    #[arg(long)]
    session: String,
    #[arg(long = "type")]
    prompt_type: String,
    #[arg(long)]
    vars_file: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
enum TreeCommand {
    Save(SaveTreeArgs),
    ApplyCluster(ApplyClusterArgs),
    ApplySuperGroup(ApplySuperGroupArgs),
    OverviewContext(OverviewContextArgs),
    Order(SessionArg),
}

#[derive(Debug, Args)]
struct SaveTreeArgs {
    #[arg(long)]
    session: String,
    #[arg(long)]
    tree_file: PathBuf,
    #[arg(long, default_value_t = false)]
    first: bool,
    #[arg(long, default_value_t = false)]
    require_decomposition_review: bool,
}

#[derive(Debug, Args)]
struct ApplyClusterArgs {
    #[arg(long)]
    session: String,
    #[arg(long)]
    tree_file: PathBuf,
    #[arg(long)]
    response_file: PathBuf,
    #[arg(long)]
    input_ids_file: PathBuf,
    #[arg(long, default_value = "repo")]
    scope: String,
    #[arg(long)]
    parent_path_file: Option<PathBuf>,
    #[arg(long)]
    output_tree_file: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct ApplySuperGroupArgs {
    #[arg(long)]
    session: String,
    #[arg(long)]
    tree_file: PathBuf,
    #[arg(long)]
    response_file: PathBuf,
    #[arg(long)]
    output_tree_file: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct OverviewContextArgs {
    #[arg(long)]
    session: String,
    #[arg(long)]
    tree_file: Option<PathBuf>,
    #[arg(long)]
    target_path_file: Option<PathBuf>,
    #[arg(long)]
    output_file: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct SessionArg {
    #[arg(long)]
    session: String,
}

#[derive(Debug, Subcommand)]
enum DocCommand {
    Write(WriteDocArgs),
    Edit(EditDocArgs),
    View(ViewDocArgs),
    Validate(ValidateDocArgs),
}

#[derive(Debug, Args)]
struct WriteDocArgs {
    #[arg(long)]
    session: String,
    #[arg(long)]
    path: String,
    #[arg(long)]
    content_file: Option<PathBuf>,
    #[arg(long)]
    content: Option<String>,
    #[arg(long = "if-existing", value_enum, default_value_t = ExistingDocumentPolicy::Error)]
    if_existing: ExistingDocumentPolicy,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ExistingDocumentPolicy {
    Error,
    Same,
}

#[derive(Debug, Args)]
struct EditDocArgs {
    #[arg(long)]
    session: String,
    #[arg(long)]
    path: String,
    #[arg(long)]
    operations_file: PathBuf,
}

#[derive(Debug, Args)]
struct ViewDocArgs {
    #[arg(long)]
    session: String,
    #[arg(long)]
    path: String,
}

#[derive(Debug, Args)]
struct ValidateDocArgs {
    #[arg(long)]
    session: String,
}

#[derive(Debug, Subcommand)]
enum UpdateCommand {
    Plan(UpdatePlanArgs),
    Route(SessionArg),
    RouteApply(UpdateRouteApplyArgs),
    Context(SessionArg),
    StaleScan(SessionArg),
    Finalize(FinalizeArgs),
}

#[derive(Debug, Args)]
struct UpdatePlanArgs {
    #[arg(long)]
    session: String,
    #[command(flatten)]
    options: UpdateArgs,
}

#[derive(Debug, Args)]
struct UpdateRouteApplyArgs {
    #[arg(long)]
    session: String,
    #[arg(long)]
    decisions_file: PathBuf,
}

#[derive(Debug, Args)]
struct FinalizeArgs {
    #[arg(long)]
    session: String,
    #[arg(long, default_value = "host-agent")]
    model: String,
    #[arg(long)]
    verdicts_file: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct HtmlArgs {
    #[arg(long)]
    session: String,
}

#[derive(Debug, Subcommand)]
enum SessionCommand {
    Close(CloseSessionArgs),
    Info(SessionArg),
}

#[derive(Debug, Args)]
struct CloseSessionArgs {
    #[arg(long)]
    session: String,
    #[arg(long, default_value = "host-agent")]
    model: String,
}

pub fn run() -> Result<()> {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            print!("{}", error);
            return Ok(());
        }
        Err(error) => return Err(anyhow!("CLI argument error: {error}")),
    };
    if let Some(repo) = &cli.session_repo {
        std::env::set_var("CODEWIKI_SESSION_REPO", repo);
    }
    match cli.command {
        None => {
            Cli::parse_from(["codewiki", "--help"]);
            Ok(())
        }
        Some(command) => dispatch(command),
    }
}

pub fn print_error(error: &anyhow::Error) {
    let value = json!({
        "ok": false,
        "error": error.to_string(),
        "chain": error.chain().map(ToString::to_string).collect::<Vec<_>>(),
    });
    println!(
        "{}",
        serde_json::to_string(&value).unwrap_or_else(|_| error.to_string())
    );
}

fn dispatch(command: Command) -> Result<()> {
    let value = match command {
        Command::Version => json!({"ok": true, "version": crate::VERSION}),
        Command::Analyze(args) => analyze_command(args.common, args.session, false, None)?,
        Command::Generate(args) => analyze_command(
            args.common,
            args.session,
            true,
            args.update.then(|| args.update_options.into()),
        )?,
        Command::Components { command } => match command {
            ComponentsCommand::Read(args) => read_components(args)?,
        },
        Command::Prompt { command } => match command {
            PromptCommand::Get(args) => get_prompt(args)?,
            PromptCommand::List => json!({
                "ok": true,
                "prompt_types": prompts::catalog(),
                "prompt_specs": prompts::catalog_specs(),
            }),
        },
        Command::Tree { command } => match command {
            TreeCommand::Save(args) => save_tree(args)?,
            TreeCommand::ApplyCluster(args) => apply_cluster(args)?,
            TreeCommand::ApplySuperGroup(args) => apply_super_group(args)?,
            TreeCommand::OverviewContext(args) => overview_context(args)?,
            TreeCommand::Order(args) => load_order(args)?,
        },
        Command::Doc { command } => match command {
            DocCommand::Write(args) => write_doc(args)?,
            DocCommand::Edit(args) => edit_doc(args)?,
            DocCommand::View(args) => view_doc(args)?,
            DocCommand::Validate(args) => validate_doc(args)?,
        },
        Command::Update { command } => match command {
            UpdateCommand::Plan(args) => update_plan(args)?,
            UpdateCommand::Route(args) => update_route(args)?,
            UpdateCommand::RouteApply(args) => update_route_apply(args)?,
            UpdateCommand::Context(args) => update_context(args)?,
            UpdateCommand::StaleScan(args) => update_stale_scan(args)?,
            UpdateCommand::Finalize(args) => update_finalize(args)?,
        },
        Command::Html(args) => generate_html(args)?,
        Command::Session { command } => match command {
            SessionCommand::Close(args) => close_session(args)?,
            SessionCommand::Info(args) => session_info(args)?,
        },
    };
    print_json(&value)
}

fn analyze_command(
    common: CommonAnalysisArgs,
    session_id: Option<String>,
    generate: bool,
    update_options: Option<UpdateOptions>,
) -> Result<Value> {
    let CommonAnalysisArgs {
        repo,
        output,
        include,
        exclude,
        focus,
        doc_type,
        instructions,
        max_depth,
        max_token_per_module,
        max_token_per_leaf_module,
        artifact_token_budget,
        artifact_exclude,
        gitignore,
        no_gitignore,
        artifacts,
        no_artifacts,
        with_prose,
    } = common;
    let options = AnalyzeOptions {
        include,
        exclude,
        focus,
        gitignore: gitignore && !no_gitignore,
        artifacts: artifacts && !no_artifacts,
        artifact_token_budget,
        artifact_exclude: artifact_exclude.clone(),
        max_depth,
        max_token_per_module,
        max_token_per_leaf_module,
        with_prose,
    };
    let (state, output, nodes) =
        analyzer::analyze(&repo, &output, &options, session_id.as_deref())?;
    let mut value = serde_json::to_value(&output)?;
    value["ok"] = json!(true);
    if generate {
        let _session_lock = if session_id.is_some() {
            Some(session::SessionLock::acquire(
                Path::new(&state.repo_path),
                &state.session_id,
            )?)
        } else {
            None
        };
        let tree_path = PathBuf::from(&output.summary.output_dir).join("module_tree.json");
        if !tree_path.exists() {
            let leaf_nodes: Vec<String> = session::read_json(Path::new(&output.leaf_nodes_path))?;
            let candidate = analyzer::build_initial_module_tree(&nodes, &leaf_nodes);
            let candidate_path = session::session_value_path(&state, "candidate_module_tree.json");
            session::write_json(&candidate_path, &candidate)?;
            value["candidate_module_tree_path"] = json!(candidate_path);
            value["candidate_tree_is_final"] = json!(false);
        }
        let workflow = json!({
            "session_id": state.session_id,
            "phase": if update_options.is_some() { "update" } else { "cluster" },
            "doc_type": doc_type,
            "instructions": instructions,
            "max_depth": max_depth,
            "max_token_per_module": max_token_per_module,
            "max_token_per_leaf_module": max_token_per_leaf_module,
            "clustering_policy": {
                "candidate_tree_is_final": false,
                "recursive_module_clustering": true,
                "cluster_batch_size": crate::model::DEFAULT_CLUSTER_BATCH_SIZE,
                "quality_gate": "session_close",
                "requires_host_model": true,
                "static_synthesis_forbidden": true,
                "subagents_allowed": true,
                "host_writes_are_serialized": true
            },
            "host_contract": {
                "session_writes": "serialized",
                "model_calls_may_parallelize": true,
                "required_barriers": [
                    "tree_order_before_page_writes",
                    "response_file_before_tree_apply",
                    "validate_before_session_close"
                ],
                "artifact_roles": {
                    "prompt_vars": "json_object",
                    "input_ids": "json_string_array_or_lines",
                    "model_response": "non_empty_file",
                    "page_content": "markdown_file"
                },
                "retry_policy": {
                    "tree_apply": "never_without_response",
                    "doc_write": "same_content_only"
                },
                "large_values": "file_side_only"
            },
            "artifact_token_budget": artifact_token_budget,
            "artifact_exclude": artifact_exclude,
            "prompt_types": prompts::catalog(),
            "prompt_specs": prompts::catalog_specs(),
            "logical_tool_mapping": {
                "analyze_repo": "codewiki analyze",
                "read_code_components": "codewiki components read",
                "get_prompt": "codewiki prompt get",
                "save_module_tree": "codewiki tree save",
                "apply_cluster": "codewiki tree apply-cluster",
                "apply_super_group": "codewiki tree apply-super-group",
                "overview_context": "codewiki tree overview-context",
                "get_processing_order": "codewiki tree order",
                "write_doc_file": "codewiki doc write",
                "edit_doc_file": "codewiki doc edit",
                "validate_doc": "codewiki doc validate",
                "close_session": "codewiki session close"
            },
            "generation_contract": {
                "prompt_get_is_transport_only": true,
                "model_must_return_cluster_or_markdown": true,
                "host_must_read_component_sources": true,
                "host_must_validate_before_close": true,
                "module_keys_must_be_ascii_page_safe": true,
                "page_writes_must_use_processing_order_doc_path": true,
                "natural_cjk_prose_is_counted_without_inserted_spaces": true,
                "template_only_pages_are_rejected": true
            },
            "next": if update_options.is_some() {
                vec!["codewiki update plan", "codewiki update route", "host-agent routing_user decision", "codewiki update route-apply", "codewiki update context", "codewiki update stale-scan", "host-agent document edits", "codewiki update finalize"]
            } else {
                vec!["host-agent root cluster response", "host-agent recursive scope=module clustering", "codewiki tree save", "host-agent leaf-first documentation", "host-agent overview documentation", "codewiki session close"]
            }
        });
        let workflow_path = session::session_value_path(&state, "workflow.json");
        session::write_json(&workflow_path, &workflow)?;
        value["workflow_path"] = json!(workflow_path);
        if let Some(options) = update_options {
            value["update_plan"] = update::plan(&state, &options)?;
        }
    }
    Ok(value)
}

fn read_components(args: ReadComponentsArgs) -> Result<Value> {
    let session = args.session.clone();
    with_locked_session(&session, move |state| {
        let mut ids = args.ids;
        if let Some(path) = args.ids_file {
            ids.extend(read_component_id_list(&path)?);
        }
        if ids.is_empty() {
            return Err(anyhow!("components read requires --ids or --ids-file"));
        }
        let nodes: BTreeMap<String, crate::model::Node> =
            session::read_json(&session::session_value_path(state, "components.json"))?;
        let mut result = Vec::new();
        for id in ids {
            let node = nodes
                .get(&id)
                .ok_or_else(|| anyhow!("unknown component id: {id}"))?;
            result.push(json!({
                "id": id,
                "language": node.language,
                "path": session::session_value_path(state, &format!("sources/{}", session::safe_source_filename(&node.id))),
                "start_line": node.start_line,
                "end_line": node.end_line,
            }));
        }
        Ok(json!({"ok": true, "components": result}))
    })
}

fn get_prompt(args: GetPromptArgs) -> Result<Value> {
    let session = args.session.clone();
    with_locked_session(&session, move |state| {
        let kind = PromptType::parse(&args.prompt_type)?;
        let mut vars = if let Some(path) = args.vars_file.as_ref() {
            let value: Value = session::read_json(path)?;
            value
                .as_object()
                .ok_or_else(|| anyhow!("prompt vars file must contain a JSON object"))?
                .clone()
                .into_iter()
                .collect::<BTreeMap<_, _>>()
        } else {
            BTreeMap::new()
        };
        let nodes: BTreeMap<String, Node> =
            session::read_json(&session::session_value_path(state, "components.json"))?;
        if matches!(kind, PromptType::User | PromptType::OverviewRepo)
            && !vars.contains_key("artifact_index")
        {
            if let Some(artifact_index) = artifact_prompt_value(state)? {
                vars.insert("artifact_index".to_string(), artifact_index);
            }
        }
        let rendered = prompts::render_with_components(kind, &vars, &nodes)?;
        let filename = format!("{}-{}.txt", kind.as_str(), Uuid::new_v4().simple());
        let path = session::session_value_path(state, &format!("prompts/{filename}"));
        session::write_text(&path, &rendered)?;
        let mut hasher = Sha256::new();
        hasher.update(rendered.as_bytes());
        Ok(json!({
            "ok": true,
            "prompt_type": kind.as_str(),
            "path": path,
            "chars": rendered.chars().count(),
            "sha256": format!("{:x}", hasher.finalize()),
            "requires_host_model": true,
            "response_is_not_generated_by_cli": true,
        }))
    })
}

fn artifact_prompt_value(state: &SessionState) -> Result<Option<Value>> {
    let path = session::session_value_path(state, "artifact_index.json");
    if !path.is_file() {
        return Ok(None);
    }
    let value: Value = session::read_json(&path)?;
    let has_files = value
        .get("files")
        .and_then(Value::as_object)
        .is_some_and(|files| !files.is_empty());
    Ok(has_files.then_some(value))
}

fn save_tree(args: SaveTreeArgs) -> Result<Value> {
    let session = args.session.clone();
    with_locked_session(&session, move |state| {
        let tree: ModuleTree = docs::read_tree_file(&args.tree_file)?;
        let result = docs::save_module_tree_with_review(
            state,
            &tree,
            args.first,
            args.require_decomposition_review,
        )?;
        Ok(json!({
            "ok": true,
            "result": result,
            "architecture_tree": true,
        }))
    })
}

fn apply_cluster(args: ApplyClusterArgs) -> Result<Value> {
    let session = args.session.clone();
    with_locked_session(&session, move |state| {
        let mut tree = docs::read_tree_file(&args.tree_file)?;
        let response = read_non_empty_file(&args.response_file, "cluster response")?;
        let input_ids = read_component_id_list(&args.input_ids_file)?;
        let parent_path = if let Some(path) = args.parent_path_file.as_ref() {
            read_string_list(path)?
        } else {
            Vec::new()
        };
        let diagnostics = docs::apply_cluster_response(
            state,
            &mut tree,
            &response,
            &input_ids,
            &args.scope,
            &parent_path,
        )?;
        docs::validate_module_page_paths(&tree)?;
        let output = args.output_tree_file.unwrap_or(args.tree_file);
        session::write_json(&output, &tree)?;
        Ok(json!({
            "ok": true,
            "tree_path": output,
            "diagnostics": diagnostics,
        }))
    })
}

fn apply_super_group(args: ApplySuperGroupArgs) -> Result<Value> {
    let session = args.session.clone();
    with_locked_session(&session, move |_state| {
        let mut tree = docs::read_tree_file(&args.tree_file)?;
        let response = read_non_empty_file(&args.response_file, "super-group response")?;
        let diagnostics = docs::apply_super_group_response(&mut tree, &response)?;
        docs::validate_module_page_paths(&tree)?;
        let output = args.output_tree_file.unwrap_or(args.tree_file);
        session::write_json(&output, &tree)?;
        Ok(json!({
            "ok": true,
            "tree_path": output,
            "diagnostics": diagnostics,
        }))
    })
}

fn overview_context(args: OverviewContextArgs) -> Result<Value> {
    let session = args.session.clone();
    with_locked_session(&session, move |state| {
        let tree_path = args
            .tree_file
            .unwrap_or_else(|| session::module_tree_path(state));
        let tree = docs::read_tree_file(&tree_path)?;
        let target_path = args
            .target_path_file
            .as_ref()
            .map(|path| read_string_list(path))
            .transpose()?
            .unwrap_or_default();
        let context = docs::overview_context_for_session(
            state,
            &tree,
            &target_path,
            &session::output_dir(state),
        )?;
        let output = if let Some(path) = args.output_file {
            path
        } else {
            let suffix = if target_path.is_empty() {
                "repo".to_string()
            } else {
                target_path
                    .iter()
                    .map(|name| {
                        docs::module_page_filename(name)
                            .map(|page| page.strip_suffix(".md").unwrap_or("module").to_string())
                    })
                    .collect::<Result<Vec<_>>>()?
                    .join("__")
            };
            session::session_value_path(state, &format!("overview_context_{suffix}.json"))
        };
        session::write_json(&output, &context)?;
        Ok(json!({
            "ok": true,
            "context_path": output,
            "target_path": target_path,
        }))
    })
}

fn read_string_list(path: &Path) -> Result<Vec<String>> {
    let contents =
        fs::read_to_string(path).with_context(|| format!("read string list {}", path.display()))?;
    if let Ok(values) = serde_json::from_str::<Vec<String>>(&contents) {
        return Ok(values);
    }
    Ok(contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect())
}

fn read_non_empty_file(path: &Path, role: &str) -> Result<String> {
    let contents =
        fs::read_to_string(path).with_context(|| format!("read {role} {}", path.display()))?;
    if contents.trim().is_empty() {
        return Err(anyhow!(
            "{role} must be a non-empty file: {}",
            path.display()
        ));
    }
    Ok(contents)
}

fn read_component_id_list(path: &Path) -> Result<Vec<String>> {
    let contents = fs::read_to_string(path)
        .with_context(|| format!("read component ID list {}", path.display()))?;
    if let Ok(value) = serde_json::from_str::<Value>(&contents) {
        return match value {
            Value::Array(_) => serde_json::from_value(value).with_context(|| {
                format!(
                    "component ID list must contain only strings: {}",
                    path.display()
                )
            }),
            _ => Err(anyhow!(
                "input IDs file must contain a JSON string array or one ID per line: {}",
                path.display()
            )),
        };
    }
    Ok(contents
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect())
}

fn load_order(args: SessionArg) -> Result<Value> {
    let session = args.session.clone();
    with_locked_session(&session, |state| {
        Ok(json!({"ok": true, "processing_order": docs::read_processing_order(state)?}))
    })
}

fn write_doc(mut args: WriteDocArgs) -> Result<Value> {
    let session = args.session.clone();
    with_locked_session(&session, move |state| {
        let content = match (args.content_file.take(), args.content.take()) {
            (Some(path), None) => {
                fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?
            }
            (None, Some(content)) => content,
            _ => {
                return Err(anyhow!(
                    "doc write requires exactly one of --content-file or --content"
                ))
            }
        };
        let reuse_if_same = matches!(args.if_existing, ExistingDocumentPolicy::Same);
        Ok(json!({
            "ok": true,
            "result": docs::write_document_with_policy(
                state,
                &args.path,
                &content,
                reuse_if_same,
            )?
        }))
    })
}

fn edit_doc(args: EditDocArgs) -> Result<Value> {
    let session = args.session.clone();
    with_locked_session(&session, move |state| {
        let operations: Vec<EditOperation> = session::read_json(&args.operations_file)?;
        Ok(json!({
            "ok": true,
            "result": docs::edit_document(state, &args.path, &operations)?
        }))
    })
}

fn view_doc(args: ViewDocArgs) -> Result<Value> {
    let session = args.session.clone();
    with_locked_session(&session, move |state| {
        Ok(json!({"ok": true, "result": docs::view_document(state, &args.path)?}))
    })
}

fn validate_doc(args: ValidateDocArgs) -> Result<Value> {
    let session = args.session.clone();
    with_locked_session(&session, |state| {
        Ok(json!({
            "ok": true,
            "result": docs::validate_documentation_report(state)?
        }))
    })
}

fn update_plan(args: UpdatePlanArgs) -> Result<Value> {
    let session = args.session.clone();
    with_locked_session(&session, move |state| {
        Ok(json!({
            "ok": true,
            "result": update::plan(state, &args.options.into())?
        }))
    })
}

fn update_route(args: SessionArg) -> Result<Value> {
    let session = args.session.clone();
    with_locked_session(&session, |state| Ok(ok_value(update::route(state)?)))
}

fn update_route_apply(args: UpdateRouteApplyArgs) -> Result<Value> {
    let session = args.session.clone();
    with_locked_session(&session, move |state| {
        Ok(ok_value(update::apply_routes(state, &args.decisions_file)?))
    })
}

fn update_context(args: SessionArg) -> Result<Value> {
    let session = args.session.clone();
    with_locked_session(&session, |state| Ok(ok_value(update::context(state)?)))
}

fn update_stale_scan(args: SessionArg) -> Result<Value> {
    let session = args.session.clone();
    with_locked_session(&session, |state| Ok(ok_value(update::stale_scan(state)?)))
}

fn update_finalize(args: FinalizeArgs) -> Result<Value> {
    let session = args.session.clone();
    with_locked_session(&session, move |state| {
        Ok(ok_value(update::finalize(
            state,
            &args.model,
            args.verdicts_file.as_deref(),
        )?))
    })
}

fn generate_html(args: HtmlArgs) -> Result<Value> {
    let session = args.session.clone();
    with_locked_session(&session, |state| {
        Ok(json!({"ok": true, "index_path": html::generate(state)?}))
    })
}

fn session_info(args: SessionArg) -> Result<Value> {
    let session = args.session.clone();
    with_locked_session(&session, |state| {
        Ok(ok_value(serde_json::to_value(state.clone())?))
    })
}

fn close_session(args: CloseSessionArgs) -> Result<Value> {
    let session_id = args.session.clone();
    with_locked_session(&session_id, |state| {
        docs::validate_documentation(state)?;
        let metadata = Some(docs::finalize_metadata(state, &args.model)?);
        state.closed = true;
        session::save_state(state)?;
        let session_path = session::session_root(Path::new(&state.repo_path), &state.session_id);
        session::cleanup(Path::new(&state.repo_path), &state.session_id)?;
        Ok(json!({
            "ok": true,
            "session_id": state.session_id,
            "metadata": metadata,
            "cleaned": true,
            "session_path": session_path,
        }))
    })
}

fn with_locked_session<T, F>(session_id: &str, operation: F) -> Result<T>
where
    F: FnOnce(&mut SessionState) -> Result<T>,
{
    let existing = load_session(session_id)?;
    let repo = PathBuf::from(&existing.repo_path);
    session::with_locked_session(&repo, session_id, operation)
}

fn load_session(session_id: &str) -> Result<SessionState> {
    let repo = std::env::var_os("CODEWIKI_SESSION_REPO")
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir()?);
    session::load(&repo, session_id)
}

fn print_json(value: &Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn ok_value(value: Value) -> Value {
    match value {
        Value::Object(mut object) => {
            object.entry("ok").or_insert_with(|| json!(true));
            Value::Object(object)
        }
        value => json!({"ok": true, "result": value}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_defaults_match_architecture_workflow() {
        let cli = Cli::try_parse_from(["codewiki", "generate", "--update"])
            .expect("default update arguments");
        let Some(Command::Generate(args)) = cli.command else {
            panic!("expected generate command");
        };
        let options = UpdateOptions::from(args.update_options);
        assert_eq!(options.rung, "3");
        assert_eq!(options.max_diff_tokens, 8000);
        assert_eq!(options.k_hop, 1);
        let cli = Cli::try_parse_from(["codewiki", "generate", "--update", "--rung", "3b"])
            .expect("3b update arguments");
        let Some(Command::Generate(args)) = cli.command else {
            panic!("expected generate command");
        };
        assert_eq!(UpdateOptions::from(args.update_options).k_hop, 2);
        assert!(Cli::try_parse_from(["codewiki", "generate", "--update", "--rung", "4"]).is_err());
    }

    #[test]
    fn tree_save_can_require_complete_decomposition_reviews() {
        let cli = Cli::try_parse_from([
            "codewiki",
            "tree",
            "save",
            "--session",
            "session-id",
            "--tree-file",
            "tree.json",
            "--require-decomposition-review",
        ])
        .expect("strict decomposition review flag should parse");
        let Some(Command::Tree {
            command: TreeCommand::Save(args),
        }) = cli.command
        else {
            panic!("expected tree save command");
        };
        assert!(args.require_decomposition_review);
    }
}
