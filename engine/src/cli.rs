use crate::analyzer::{self, AnalyzeOptions};
use crate::docs::{self, EditOperation};
use crate::html;
use crate::model::{Module, ModuleTree, Node, UpdateOptions};
use crate::prompts::{self, PromptType};
use crate::session::{self, SessionState};
use crate::update;
use anyhow::{anyhow, Context, Result};
use clap::{error::ErrorKind, Args, Parser, Subcommand};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

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
    #[arg(long, default_value = "docs")]
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
    #[arg(long, default_value_t = false)]
    github_pages: bool,
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
    #[arg(
        long,
        alias = "update-rung",
        default_value = "3",
        value_parser = parse_update_rung
    )]
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

#[derive(Debug, Subcommand)]
enum UpdateCommand {
    Plan(UpdatePlanArgs),
    Route(SessionArg),
    Context(SessionArg),
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
            TreeCommand::Order(args) => load_order(args)?,
        },
        Command::Doc { command } => match command {
            DocCommand::Write(args) => write_doc(args)?,
            DocCommand::Edit(args) => edit_doc(args)?,
            DocCommand::View(args) => view_doc(args)?,
        },
        Command::Update { command } => match command {
            UpdateCommand::Plan(args) => update_plan(args)?,
            UpdateCommand::Route(args) => ok_value(update::route(&load_session(&args.session)?)?),
            UpdateCommand::Context(args) => {
                ok_value(update::context(&load_session(&args.session)?)?)
            }
            UpdateCommand::Finalize(args) => ok_value(update::finalize(
                &load_session(&args.session)?,
                &args.model,
                args.verdicts_file.as_deref(),
            )?),
        },
        Command::Html(args) => {
            let state = load_session(&args.session)?;
            json!({"ok": true, "index_path": html::generate(&state)?})
        }
        Command::Session { command } => match command {
            SessionCommand::Close(args) => close_session(args)?,
            SessionCommand::Info(args) => {
                ok_value(serde_json::to_value(load_session(&args.session)?)?)
            }
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
        github_pages,
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
        with_prose,
    };
    let (state, output, nodes) =
        analyzer::analyze(&repo, &output, &options, session_id.as_deref())?;
    let mut value = serde_json::to_value(&output)?;
    value["ok"] = json!(true);
    if generate {
        let tree_path = PathBuf::from(&output.summary.output_dir).join("module_tree.json");
        if !tree_path.exists() {
            let candidate = analyzer::build_initial_module_tree(&nodes);
            let candidate_path = session::session_value_path(&state, "candidate_module_tree.json");
            session::write_json(&candidate_path, &candidate)?;
            value["candidate_module_tree_path"] = json!(candidate_path);
        }
        let workflow = json!({
            "session_id": state.session_id,
            "phase": if update_options.is_some() { "update" } else { "cluster" },
            "doc_type": doc_type,
            "instructions": instructions,
            "max_depth": max_depth,
            "max_token_per_module": max_token_per_module,
            "max_token_per_leaf_module": max_token_per_leaf_module,
            "artifact_token_budget": artifact_token_budget,
            "artifact_exclude": artifact_exclude,
            "github_pages": github_pages,
            "prompt_types": prompts::catalog(),
            "prompt_specs": prompts::catalog_specs(),
            "logical_tool_mapping": {
                "analyze_repo": "codewiki analyze",
                "read_code_components": "codewiki components read",
                "get_prompt": "codewiki prompt get",
                "save_module_tree": "codewiki tree save",
                "get_processing_order": "codewiki tree order",
                "write_doc_file": "codewiki doc write",
                "edit_doc_file": "codewiki doc edit",
                "close_session": "codewiki session close"
            },
            "next": if update_options.is_some() {
                vec!["codewiki update plan", "codewiki update route", "codewiki update context", "host-agent document edits", "codewiki update finalize"]
            } else {
                vec!["host-agent cluster response", "codewiki tree save", "host-agent leaf-first documentation", "host-agent overview documentation", "codewiki session close"]
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
    let state = load_session(&args.session)?;
    let mut ids = args.ids;
    if let Some(path) = args.ids_file {
        let contents = fs::read_to_string(&path)
            .with_context(|| format!("read component id file {}", path.display()))?;
        if let Ok(values) = serde_json::from_str::<Vec<String>>(&contents) {
            ids.extend(values);
        } else {
            ids.extend(
                contents
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(str::to_string),
            );
        }
    }
    if ids.is_empty() {
        return Err(anyhow!("components read requires --ids or --ids-file"));
    }
    let nodes: BTreeMap<String, crate::model::Node> =
        session::read_json(&session::session_value_path(&state, "components.json"))?;
    let mut result = Vec::new();
    for id in ids {
        let node = nodes
            .get(&id)
            .ok_or_else(|| anyhow!("unknown component id: {id}"))?;
        result.push(json!({
            "id": id,
            "language": node.language,
            "path": session::session_value_path(&state, &format!("sources/{}", session::safe_source_filename(&node.id))),
            "start_line": node.start_line,
            "end_line": node.end_line,
        }));
    }
    Ok(json!({"ok": true, "components": result}))
}

fn get_prompt(args: GetPromptArgs) -> Result<Value> {
    let state = load_session(&args.session)?;
    let kind = PromptType::parse(&args.prompt_type)?;
    let vars = if let Some(path) = args.vars_file {
        let value: Value = session::read_json(&path)?;
        value
            .as_object()
            .ok_or_else(|| anyhow!("prompt vars file must contain a JSON object"))?
            .clone()
            .into_iter()
            .collect::<BTreeMap<_, _>>()
    } else {
        BTreeMap::new()
    };
    let rendered = if matches!(kind, PromptType::User | PromptType::Cluster) {
        prompts::user_prompt_with_limits(kind, &vars)?
    } else {
        prompts::render(kind, &vars)?
    };
    let filename = format!(
        "{}-{}.txt",
        kind.as_str(),
        chrono::Utc::now().timestamp_millis()
    );
    let path = session::session_value_path(&state, &format!("prompts/{filename}"));
    session::write_text(&path, &rendered)?;
    let mut hasher = Sha256::new();
    hasher.update(rendered.as_bytes());
    Ok(json!({
        "ok": true,
        "prompt_type": kind.as_str(),
        "path": path,
        "chars": rendered.chars().count(),
        "sha256": format!("{:x}", hasher.finalize()),
    }))
}

fn save_tree(args: SaveTreeArgs) -> Result<Value> {
    let state = load_session(&args.session)?;
    let mut tree: ModuleTree = docs::read_tree_file(&args.tree_file)?;
    normalize_tree_component_ids(&state, &mut tree)?;
    Ok(json!({"ok": true, "result": docs::save_module_tree(&state, &tree, args.first)?}))
}

fn normalize_tree_component_ids(state: &SessionState, tree: &mut ModuleTree) -> Result<()> {
    let nodes: BTreeMap<String, Node> =
        session::read_json(&session::session_value_path(state, "components.json"))?;
    let mut aliases = BTreeMap::<String, String>::new();
    let mut ambiguous = BTreeMap::<String, bool>::new();
    for node in nodes.values() {
        let short_name = node.name.rsplit('.').next().unwrap_or(&node.name);
        let alias = format!("{}::{}", node.relative_path, short_name);
        if let Some(existing) = aliases.get(&alias) {
            if existing != &node.id {
                ambiguous.insert(alias.clone(), true);
            }
        } else {
            aliases.insert(alias, node.id.clone());
        }
    }
    for alias in ambiguous.keys() {
        aliases.remove(alias);
    }
    fn visit(module: &mut Module, aliases: &BTreeMap<String, String>) {
        for component in &mut module.components {
            if let Some(canonical) = aliases.get(component) {
                *component = canonical.clone();
            }
        }
        for child in module.children.values_mut() {
            visit(child, aliases);
        }
    }
    for module in tree.values_mut() {
        visit(module, &aliases);
    }
    Ok(())
}

fn load_order(args: SessionArg) -> Result<Value> {
    let state = load_session(&args.session)?;
    Ok(json!({"ok": true, "processing_order": docs::read_processing_order(&state)?}))
}

fn write_doc(mut args: WriteDocArgs) -> Result<Value> {
    let mut state = load_session(&args.session)?;
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
    Ok(json!({"ok": true, "result": docs::write_document(&mut state, &args.path, &content)?}))
}

fn edit_doc(args: EditDocArgs) -> Result<Value> {
    let mut state = load_session(&args.session)?;
    let operations: Vec<EditOperation> = session::read_json(&args.operations_file)?;
    Ok(json!({"ok": true, "result": docs::edit_document(&mut state, &args.path, &operations)?}))
}

fn view_doc(args: ViewDocArgs) -> Result<Value> {
    let state = load_session(&args.session)?;
    Ok(json!({"ok": true, "result": docs::view_document(&state, &args.path)?}))
}

fn update_plan(args: UpdatePlanArgs) -> Result<Value> {
    let state = load_session(&args.session)?;
    Ok(json!({"ok": true, "result": update::plan(&state, &args.options.into())?}))
}

fn close_session(args: CloseSessionArgs) -> Result<Value> {
    let mut state = load_session(&args.session)?;
    docs::validate_documentation(&state)?;
    let metadata = Some(docs::finalize_metadata(&state, &args.model)?);
    state.closed = true;
    session::save_state(&state)?;
    let session_path = session::session_root(Path::new(&state.repo_path), &state.session_id);
    session::cleanup(Path::new(&state.repo_path), &state.session_id)?;
    Ok(json!({
        "ok": true,
        "session_id": state.session_id,
        "metadata": metadata,
        "cleaned": true,
        "session_path": session_path,
    }))
}

fn load_session(session_id: &str) -> Result<SessionState> {
    let repo = std::env::var_os("CODEWIKI_SESSION_REPO")
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir()?);
    session::load(&repo, session_id).or_else(|_| {
        find_session(&repo, session_id)?.ok_or_else(|| anyhow!("session not found: {session_id}"))
    })
}

fn find_session(start: &Path, session_id: &str) -> Result<Option<SessionState>> {
    let mut current = Some(start);
    while let Some(path) = current {
        if let Ok(state) = session::load(path, session_id) {
            return Ok(Some(state));
        }
        current = path.parent();
    }
    Ok(None)
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
    fn update_defaults_match_reference() {
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
}
