mod accounts;
mod agent_args;
mod bench;
mod board;
mod board_tui;
mod board_usage;
mod board_view;
mod check_profiles;
mod checks;
mod details_view;
mod devws;
mod executable;
mod execution;
mod graph;
mod history;
mod home;
mod images;
mod integration;
mod lanes;
mod leader;
mod log;
mod mcp;
mod memory;
mod milestones;
mod notes;
mod opencode;
mod opencode_worker;
mod orders;
mod panel_view;
mod performance;
mod plans;
mod providers;
mod prs;
mod questions;
mod recovery;
mod reviews;
mod roles;
mod runner;
mod seen;
mod specs;
mod start;
mod state;
mod status;
mod task;
mod team;
mod watch;
mod worker;
mod worker_tools;
mod workflow;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use devws::{CommandDevws, Devws};
use home::Home;
use std::io::Read;
use std::path::PathBuf;
use std::time::Duration;
use task::Task;
use team::Team;

#[derive(Parser)]
#[command(
    name = "devagentics",
    version,
    about = "Agent teams on top of devws: a leader, and the agents it opens"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    WorkerMcp {
        task: String,
        agent: String,
        #[arg(long)]
        identity: PathBuf,
    },
    WorkerPanel {
        task: String,
        agent: String,
        #[arg(long)]
        identity: PathBuf,
        #[arg(long)]
        model: Option<String>,
    },
    /// Structured application planning and independent review controls
    Workflow {
        task: String,
        action: String,
        /// JSON arguments (defaults to an empty object)
        #[arg(long, default_value = "{}", conflicts_with = "input_file")]
        input: String,
        /// Read the JSON object from a file instead of a command argument
        #[arg(long)]
        input_file: Option<PathBuf>,
    },
    /// Submit a review from its assigned read-only reviewer pane
    ReviewSubmit {
        task: String,
        review: String,
        #[arg(long)]
        input: String,
    },
    /// A task: one branch, one team
    Task {
        #[command(subcommand)]
        action: TaskAction,
    },
    /// The team's history in the repo (.devagentics/history/<task>/)
    History {
        #[command(subcommand)]
        action: HistoryAction,
    },
    /// Measure a team run: first-pass acceptance, corrections and their
    /// causes, waits, what you had to do and the cost
    Bench {
        #[command(subcommand)]
        action: BenchAction,
    },
    /// Run checks for an exact delivered order
    Check {
        #[command(subcommand)]
        action: CheckAction,
    },
    /// Amend an existing order
    Order {
        #[command(subcommand)]
        action: OrderAction,
    },
    /// The roles agents can have
    Role {
        #[command(subcommand)]
        action: RoleAction,
    },
    /// Team agents on opencode: the switches and the models
    Opencode {
        #[command(subcommand)]
        action: OpencodeAction,
    },
    /// The leader's program
    Leader {
        #[command(subcommand)]
        action: LeaderAction,
    },
    /// The plan of a task
    Plan {
        #[command(subcommand)]
        action: PlanAction,
    },
    /// A factory-floor agent asks about its order (in its pane): the senior
    /// who planned it answers, or the leader
    Ask { question: String },
    /// A senior answers a question it was asked (in its pane)
    Answer { question: String, answer: String },
    /// An agent reports on its order (run by the agent itself, in its pane)
    Report {
        #[arg(long)]
        order: String,
        #[arg(value_enum)]
        outcome: ReportOutcome,
        /// What it did, or what stops it
        summary: String,
    },
    /// The leader's tools, as an MCP server on standard input and output
    Mcp { task: String },
    /// The team board and the dispatcher (the workspace's team pane)
    Board {
        task: String,
        /// Print the board as text, once, and stop (no dispatcher)
        #[arg(long)]
        once: bool,
        #[arg(long, default_value_t = 100)]
        width: u16,
        /// Rows of the printed board; 0 is as many as it takes
        #[arg(long, default_value_t = 0)]
        height: u16,
        /// Initial board view
        #[arg(long, value_enum, default_value_t = board_view::View::Overview)]
        view: board_view::View,
    },
    /// The dispatcher and the log of a task, as plain lines
    Watch {
        task: String,
        /// One pass, then stop
        #[arg(long)]
        once: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Switch {
    On,
    Off,
}

#[derive(Subcommand)]
enum BenchAction {
    /// A task's run
    Report {
        task: String,
        /// As JSON, to collect runs
        #[arg(long)]
        json: bool,
    },
    /// Two tasks' runs side by side (a legacy and a reviewed one)
    Compare { first: String, second: String },
}

#[derive(Subcommand)]
enum HistoryAction {
    /// Write what the task accepted, its plan and execution plans into the
    /// repo and commit them on the task's branch (each accepted step does it
    /// too)
    Save { task: String },
}

#[derive(Subcommand)]
enum CheckAction {
    Run {
        #[arg(long)]
        order: String,
        #[arg(long)]
        task: Option<String>,
    },
}

#[derive(Subcommand)]
enum OrderAction {
    Amend {
        id: String,
        #[arg(long)]
        task: Option<String>,
        #[arg(long, required = true)]
        allow: Vec<String>,
        #[arg(long)]
        reason: String,
    },
}

#[derive(Subcommand)]
enum TaskAction {
    /// Make the task for a worktree, or find it; prints its id, the leader's
    /// agent and the leader's arguments file (what `devws agentic` calls)
    Start {
        folder: PathBuf,
        /// Explicitly create a legacy task; existing tasks always keep their workflow
        #[arg(long)]
        legacy: bool,
        /// The devws agent that leads, instead of the leader role's; kept
        /// for the task
        #[arg(long)]
        leader: Option<String>,
    },
    /// The devws agent that leads a worktree's team: its task's leader, or
    /// the leader role's when there is no task yet
    Leader { folder: PathBuf },
    /// You approve the plan: agents that write may start
    Approve { task: String },
    /// Where the team stands
    Status { task: String },
    /// Stop new deliveries and new agents
    Pause {
        task: String,
        #[arg(long)]
        interrupt: bool,
    },
    /// Take it off pause, and start again the agents whose panels went
    Resume { task: String },
    /// Stop every agent (the worktrees stay)
    Cancel { task: String },
    /// You let Codex run this task's read-only code reviewers (on) or not
    /// (off). In its read-only sandbox it cannot write, but its shell can
    /// read host files; writers always run on Claude.
    CodexReviewers { task: String, switch: Switch },
    /// You put this reviewed task's whole team, leader included, on Codex
    /// (on) or not (off). Codex cannot turn its shell off: a worker's writes
    /// stay in its snapshot and nothing has network, but its shell can read
    /// host files. Restart the leader to move it
    CodexTeam { task: String, switch: Switch },
    /// You turn the code graph (Graphify, code only and offline) on or off
    /// for this task's leader and workers: built per commit in the task
    /// folder, never in the project
    Graph { task: String, switch: Switch },
    /// You allow a step more attempts than the task's limit (orders and
    /// reviews asking for changes): `limit` in all. The leader can only
    /// record your yes from its conversation (allow_attempts).
    Attempts {
        task: String,
        step: String,
        limit: u32,
    },
    /// The tasks
    List,
}

#[derive(Subcommand)]
enum RoleAction {
    /// The roles, and where each comes from
    List {
        /// A repo whose own `.devws/roles` count
        #[arg(long)]
        repo: Option<PathBuf>,
    },
    /// Change a role's model, effort or approvals in its file; an empty
    /// value removes it, for the program's default. Agents started from now
    /// on use it; running ones keep theirs.
    Set {
        role: String,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        effort: Option<String>,
        #[arg(long)]
        approvals: Option<String>,
        /// A repo whose own `.devws/roles` count
        #[arg(long)]
        repo: Option<PathBuf>,
    },
    /// The values a role's program takes for a setting, one a line (for a
    /// model, the known ones: any other may be typed)
    Choices {
        role: String,
        setting: SettingArg,
        /// A repo whose own `.devws/roles` count
        #[arg(long)]
        repo: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum OpencodeAction {
    /// The switches, the groups' models and the account, tab-separated
    Show,
    /// Turn a switch on or off. `global` is mandatory: while it is off,
    /// opencode runs no agent, whatever the groups' switches say
    Switch { switch: SwitchArg, state: OnOff },
    /// The model a group runs on opencode (`provider/model`, as `opencode
    /// models` lists it); an empty one removes it
    Model { group: GroupArg, model: String },
    /// The devws agent that runs opencode (`DEVWS_AGENTS`)
    Account { name: String },
}

#[derive(Clone, Copy, ValueEnum)]
enum SwitchArg {
    Global,
    Floor,
    Senior,
}

#[derive(Clone, Copy, ValueEnum)]
enum GroupArg {
    Floor,
    Senior,
}

impl From<GroupArg> for roles::Group {
    fn from(group: GroupArg) -> Self {
        match group {
            GroupArg::Floor => roles::Group::Floor,
            GroupArg::Senior => roles::Group::Senior,
        }
    }
}

#[derive(Clone, Copy, PartialEq, ValueEnum)]
enum OnOff {
    On,
    Off,
}

#[derive(Clone, Copy, ValueEnum)]
enum SettingArg {
    Model,
    Effort,
    Approvals,
}

impl From<SettingArg> for roles::Setting {
    fn from(setting: SettingArg) -> Self {
        match setting {
            SettingArg::Model => roles::Setting::Model,
            SettingArg::Effort => roles::Setting::Effort,
            SettingArg::Approvals => roles::Setting::Approvals,
        }
    }
}

#[derive(Subcommand)]
enum LeaderAction {
    /// Move the leader to its other program (its role's or its fallback's)
    /// once its turn is over; the board does it
    Switch {
        task: String,
        /// Why, for the log and the new leader
        #[arg(long)]
        reason: Option<String>,
    },
    /// Move the leader to an account you pick, any that can lead, once its
    /// turn is over; it starts there from then on
    Use {
        task: String,
        /// The devws account (`leader choices` lists them)
        agent: String,
        /// Why, for the log and the new leader
        #[arg(long)]
        reason: Option<String>,
    },
    /// The accounts that can lead: name, program and how each stands, tab-separated
    Choices { task: String },
}

#[derive(Subcommand)]
enum PlanAction {
    /// Store a plan, given on standard input: from an agent's pane, the
    /// task's plan until the user approves one, then a senior's execution
    /// plan for the step it was ordered
    Write {
        /// The task's plan of this task, from anywhere
        #[arg(long)]
        task: Option<String>,
    },
    /// Print a step's execution plan
    Show {
        step: String,
        /// The task; by default the one whose agent runs in this pane
        #[arg(long)]
        task: Option<String>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum ReportOutcome {
    Done,
    Blocked,
}

fn this_pane() -> Result<String> {
    std::env::var("TMUX_PANE").context("this is run by a team agent, in its pane (no TMUX_PANE)")
}

/// Claude, Codex, opencode or Antigravity: the program of the role's agent (`claude_work` is a
/// Claude account), which decides the values its settings take.
fn role_kind(
    home: &Home,
    repo: Option<&std::path::Path>,
    name: &str,
    devws: &dyn devws::Devws,
) -> Result<providers::Kind> {
    let roles = roles::Roles::load(home, repo)?;
    let role = roles
        .get(name)
        .with_context(|| format!("no role is named {name}"))?;
    devws.agent_kind(&role.agent)
}

/// The task whose agent runs in this pane.
fn task_of_this_pane(home: &Home) -> Result<Task> {
    let pane = this_pane()?;
    for id in home.task_ids()? {
        let task = Task::open(home, &id)?;
        if task.state()?.agent_in_pane(&pane).is_some() {
            return Ok(task);
        }
    }
    bail!("{pane} is not the pane of an agent of any task")
}

fn task_of_control_pane(home: &Home, devws: &dyn devws::Devws) -> Result<Task> {
    let pane = this_pane()?;
    for id in home.task_ids()? {
        let task = Task::open(home, &id)?;
        if task.state()?.agent_in_pane(&pane).is_some()
            || devws
                .pane_with_role(&task.config()?.workspace, "agent")?
                .as_deref()
                == Some(&pane)
        {
            return Ok(task);
        }
    }
    bail!("no task for this pane; pass --task <task>")
}

fn task_list(home: &Home) -> Result<String> {
    let mut lines = Vec::new();
    for id in home.task_ids()? {
        let task = Task::open(home, &id)?;
        let (config, state) = (task.config()?, task.state()?);
        let accepted = state
            .steps
            .iter()
            .filter(|step| step.status == state::StepStatus::Accepted)
            .count();
        let phase = if state.paused {
            "paused"
        } else if state.approved {
            "approved"
        } else {
            "planning"
        };
        lines.push(format!(
            "{id}  {}  {phase}  {} of {} steps accepted  {} agents running",
            config.branch,
            accepted,
            state.steps.len(),
            state.agents.iter().filter(|agent| agent.live).count()
        ));
    }
    Ok(if lines.is_empty() {
        "No tasks yet: `devws agentic` starts one.".to_string()
    } else {
        lines.join("\n")
    })
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let home = Home::from_env()?;
    let devws = CommandDevws::from_env();
    let open = |id: &str| Task::open(&home, id);

    match cli.command {
        Command::WorkerMcp {
            task,
            agent,
            identity,
        } => worker_tools::mcp(&open(&task)?, &agent, &identity)?,
        Command::WorkerPanel {
            task,
            agent,
            identity,
            model,
        } => worker_tools::panel(&open(&task)?, &agent, &identity, model.as_deref())?,
        Command::Workflow {
            task,
            action,
            input,
            input_file,
        } => {
            let task = open(&task)?;
            let input = match input_file {
                Some(path) => std::fs::read_to_string(path)?,
                None => input,
            };
            let input: serde_json::Value = serde_json::from_str(&input)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&workflow::call(&task, &action, &input)?)?
            );
        }
        Command::ReviewSubmit {
            task,
            review,
            input,
        } => {
            let task = open(&task)?;
            let input: serde_json::Value = serde_json::from_str(&input)?;
            reviews::submit(
                &task,
                &this_pane()?,
                &review,
                serde_json::from_value(input["verdict"].clone())?,
                serde_json::from_value(input["findings"].clone())?,
                input["summary"].as_str().context("summary required")?,
            )?;
            println!("Review recorded; only the leader may accept and merge.");
        }
        Command::Task { action } => match action {
            TaskAction::Start {
                folder,
                leader,
                legacy,
            } => {
                let command = executable::own()?.display().to_string();
                let started = if legacy {
                    start::start_task(&home, &devws, &folder, &command, leader.as_deref())?
                } else {
                    start::start_task_with_workflow(
                        &home,
                        &devws,
                        &folder,
                        &command,
                        leader.as_deref(),
                        execution::Workflow::Reviewed,
                    )?
                };
                println!(
                    "{}\n{}\n{}",
                    started.id,
                    started.leader_agent,
                    started.args_file.display()
                );
            }
            TaskAction::Leader { folder } => println!("{}", start::leader_for(&home, &folder)?),
            TaskAction::Approve { task } => {
                Team {
                    task: &open(&task)?,
                    home: &home,
                    devws: &devws,
                }
                .approve()?;
                println!("Plan approved: agents that write may start.");
            }
            TaskAction::Status { task } => println!(
                "{}",
                serde_json::to_string_pretty(&status::summary(&open(&task)?, &devws, &home)?)?
            ),
            TaskAction::Pause { task, interrupt } => {
                Team {
                    task: &open(&task)?,
                    home: &home,
                    devws: &devws,
                }
                .pause_mode(interrupt, std::env::var("TMUX_PANE").ok().as_deref())?;
                println!("Paused: nothing new is delivered or started.");
            }
            TaskAction::Resume { task } => {
                let restarted = Team {
                    task: &open(&task)?,
                    home: &home,
                    devws: &devws,
                }
                .resume()?;
                println!(
                    "Resumed.{}",
                    if restarted.is_empty() {
                        String::new()
                    } else {
                        format!(" Started again: {}.", restarted.join(", "))
                    }
                );
            }
            TaskAction::Cancel { task } => {
                Team {
                    task: &open(&task)?,
                    home: &home,
                    devws: &devws,
                }
                .cancel()?;
                println!("Every agent is stopped; the worktrees stay.");
            }
            TaskAction::CodexReviewers { task, switch } => {
                let enabled = matches!(switch, Switch::On);
                open(&task)?.transaction(|tx| {
                    tx.state.codex_reviewers = enabled;
                    tx.log(
                        log::Kind::Review,
                        None,
                        None,
                        if enabled {
                            "the user let Codex run read-only code reviewers"
                        } else {
                            "code reviewers run on Claude"
                        },
                    );
                    Ok(())
                })?;
                println!(
                    "{}",
                    if enabled {
                        "Code reviewers started from now on may run on Codex (read-only)."
                    } else {
                        "Code reviewers started from now on run on Claude."
                    }
                );
            }
            TaskAction::CodexTeam { task, switch } => {
                let enabled = matches!(switch, Switch::On);
                open(&task)?.transaction(|tx| {
                    tx.state.codex_team = enabled;
                    tx.state.codex_reviewers |= enabled;
                    tx.log(
                        log::Kind::Review,
                        None,
                        None,
                        if enabled {
                            "the user put the whole team on Codex (its shell can read the host)"
                        } else {
                            "the team's writers and leader run on Claude again"
                        },
                    );
                    Ok(())
                })?;
                println!(
                    "{}",
                    if enabled {
                        "The team runs on Codex from now on: agents started from now, and the leader once restarted."
                    } else {
                        "Writers and the leader run on Claude from now on; reviewers stay as codex-reviewers says."
                    }
                );
            }
            TaskAction::Graph { task, switch } => {
                let enabled = matches!(switch, Switch::On);
                if enabled {
                    graph::Graphify::from_env()?.available()?;
                }
                open(&task)?.transaction(|tx| {
                    tx.state.graph_enabled = enabled;
                    tx.log(
                        log::Kind::Review,
                        None,
                        None,
                        if enabled {
                            "the user turned the code graph on"
                        } else {
                            "the user turned the code graph off"
                        },
                    );
                    Ok(())
                })?;
                println!(
                    "{}",
                    if enabled {
                        "The code graph is on: the leader and workers can ask it (graph)."
                    } else {
                        "The code graph is off."
                    }
                );
            }
            TaskAction::Attempts { task, step, limit } => {
                orders::allow_attempts(&open(&task)?, &step, limit)?;
                println!("Step {step} may have {limit} attempts in all.");
            }
            TaskAction::List => println!("{}", task_list(&home)?),
        },
        Command::Role {
            action: RoleAction::List { repo },
        } => {
            for role in roles::Roles::load(&home, repo.as_deref())?.all() {
                println!(
                    "{:<14} {:<7} {:<12} {:<18} {:<7} {:<11} {:<7} {}",
                    role.name,
                    role.group.name(),
                    role.agent,
                    role.model.as_deref().unwrap_or("-"),
                    role.effort.as_deref().unwrap_or("-"),
                    role.approvals.as_deref().unwrap_or("-"),
                    if role.writes { "writes" } else { "reads" },
                    role.source.label()
                );
            }
        }
        Command::Role {
            action:
                RoleAction::Set {
                    role,
                    model,
                    effort,
                    approvals,
                    repo,
                },
        } => {
            let changes: Vec<_> = [
                (roles::Setting::Model, model),
                (roles::Setting::Effort, effort),
                (roles::Setting::Approvals, approvals),
            ]
            .into_iter()
            .filter_map(|(setting, value)| value.map(|value| (setting, value)))
            .collect();
            if changes.is_empty() {
                bail!("say what to change: --model, --effort or --approvals");
            }
            let kind = role_kind(&home, repo.as_deref(), &role, &devws)?;
            let path = roles::set(&home, repo.as_deref(), &role, kind, &changes)?;
            println!(
                "{role} changed in {}. Agents started from now on use it; running ones keep theirs.",
                path.display()
            );
        }
        Command::Role {
            action:
                RoleAction::Choices {
                    role,
                    setting,
                    repo,
                },
        } => {
            let kind = role_kind(&home, repo.as_deref(), &role, &devws)?;
            for choice in roles::Setting::from(setting).choices(kind) {
                println!("{choice}");
            }
        }
        Command::Opencode { action } => {
            let mut settings = opencode::OpencodeSettings::load(&home)?;
            match action {
                OpencodeAction::Show => {
                    println!("{}", settings.describe());
                    return Ok(());
                }
                OpencodeAction::Switch { switch, state } => {
                    let on = state == OnOff::On;
                    match switch {
                        SwitchArg::Global => settings.enabled = on,
                        SwitchArg::Floor => settings.floor.enabled = on,
                        SwitchArg::Senior => settings.senior.enabled = on,
                    }
                }
                OpencodeAction::Model { group, model } => {
                    let model = model.trim();
                    if !model.is_empty() && !model.contains('/') {
                        bail!(
                            "name the model as provider/model, as `opencode models` lists it: {model}"
                        );
                    }
                    settings.group_mut(group.into())?.model =
                        Some(model.to_string()).filter(|model| !model.is_empty());
                }
                OpencodeAction::Account { name } => {
                    devws.agent_kind(&name).ok().filter(|kind| kind.provider().own_switches()).with_context(|| {
                        format!("{name} is not a devws opencode agent: register it in the devws menu")
                    })?;
                    settings.account = name;
                }
            }
            settings.save(&home)?;
            println!(
                "{}\nAgents started from now on follow it; running ones keep theirs.",
                settings.describe()
            );
        }
        Command::Leader {
            action: LeaderAction::Switch { task, reason },
        } => println!(
            "{}",
            leader::request_switch(&home, &open(&task)?, &devws, reason.as_deref())?
        ),
        Command::Leader {
            action:
                LeaderAction::Use {
                    task,
                    agent,
                    reason,
                },
        } => println!(
            "{}",
            leader::request_lead(&home, &open(&task)?, &devws, &agent, reason.as_deref())?
        ),
        Command::Leader {
            action: LeaderAction::Choices { task },
        } => {
            for line in leader::choices(&open(&task)?, &devws)? {
                println!("{line}");
            }
        }
        Command::Plan {
            action: PlanAction::Write { task },
        } => {
            let mut plan = String::new();
            std::io::stdin()
                .read_to_string(&mut plan)
                .context("reading the plan")?;
            let stored = match task {
                Some(id) => {
                    let task = open(&id)?;
                    Team {
                        task: &task,
                        home: &home,
                        devws: &devws,
                    }
                    .write_plan(&plan)?;
                    plans::Stored::Overall
                }
                None => {
                    let task = task_of_this_pane(&home)?;
                    plans::store(
                        &Team {
                            task: &task,
                            home: &home,
                            devws: &devws,
                        },
                        &this_pane()?,
                        &plan,
                    )?
                }
            };
            match stored {
                plans::Stored::Overall => println!(
                    "Plan stored. The leader goes through it with the user, who approves it."
                ),
                plans::Stored::Execution { step } => println!(
                    "Execution plan of step {step} stored. Report done; the leader accepts it and gives its steps to the factory floor."
                ),
            }
        }
        Command::Plan {
            action: PlanAction::Show { step, task },
        } => {
            let task = match task {
                Some(id) => open(&id)?,
                None => task_of_this_pane(&home)?,
            };
            print!("{}", plans::read(&task, &step)?);
        }
        Command::Ask { question } => {
            let task = task_of_this_pane(&home)?;
            println!("{}", questions::ask(&task, &this_pane()?, &question)?);
        }
        Command::Answer { question, answer } => {
            let task = task_of_this_pane(&home)?;
            let pane = this_pane()?;
            println!(
                "{}",
                questions::answer(&task, questions::Answerer::Pane(&pane), &question, &answer)?
            );
        }
        Command::Bench {
            action: BenchAction::Report { task, json },
        } => {
            let report = bench::report(&open(&task)?, &devws)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print!("{}", bench::render(&report));
            }
        }
        Command::Bench {
            action: BenchAction::Compare { first, second },
        } => {
            let first = bench::report(&open(&first)?, &devws)?;
            let second = bench::report(&open(&second)?, &devws)?;
            print!("{}", bench::compare(&first, &second));
        }
        Command::History {
            action: HistoryAction::Save { task },
        } => {
            let task = open(&task)?;
            if history::save(&task, None)? {
                println!(
                    "The team's history is committed on {}.",
                    task.config()?.branch
                );
            } else {
                println!("The team's history in the repo is up to date.");
            }
        }
        Command::Check {
            action: CheckAction::Run { order, task },
        } => {
            let task = match task {
                Some(id) => open(&id)?,
                None => task_of_control_pane(&home, &devws)?,
            };
            let pane = std::env::var("TMUX_PANE").ok();
            let agent_pane = if let Some(pane) = &pane {
                if task.state()?.agent_in_pane(pane).is_some() {
                    Some(pane.as_str())
                } else {
                    use devws::Devws;
                    if devws
                        .pane_with_role(&task.config()?.workspace, "agent")?
                        .as_deref()
                        != Some(pane)
                    {
                        bail!("not this task's leader or agent pane");
                    }
                    None
                }
            } else {
                None
            };
            let evidence = check_profiles::run(&task, &order, agent_pane)?;
            println!("{}", serde_json::to_string_pretty(&evidence.response())?);
            if !evidence.passed() {
                bail!("checks failed; see structured phase results");
            }
        }
        Command::Order {
            action:
                OrderAction::Amend {
                    id,
                    task,
                    allow,
                    reason,
                },
        } => {
            let task = match task {
                Some(id) => open(&id)?,
                None => task_of_control_pane(&home, &devws)?,
            };
            println!("{}", {
                if let Ok(pane) = std::env::var("TMUX_PANE") {
                    if task.state()?.agent_in_pane(&pane).is_some() {
                        bail!("order amendments require the leader or a local operator");
                    }
                }
                orders::amend(&task, &id, allow, &reason, "operator/leader CLI")?
            });
        }
        Command::Report {
            order,
            outcome,
            summary,
        } => {
            let pane = this_pane()?;
            let task = task_of_this_pane(&home)?;
            let (outcome, word) = match outcome {
                ReportOutcome::Done => (orders::Outcome::Done, "done"),
                ReportOutcome::Blocked => (orders::Outcome::Blocked, "blocked"),
            };
            let state = task.state()?;
            let reported = state.orders.iter().find(|each| each.id == order).cloned();
            let author = state
                .agents
                .iter()
                .find(|agent| agent.pane == pane)
                .map(|agent| agent.id.clone());
            println!(
                "{}",
                orders::report(
                    &task,
                    &pane,
                    &order,
                    outcome,
                    &summary,
                    orders::check_timeout()
                )?
            );
            // Every report is kept for the agents after it.
            if let (Some(reported), Some(author)) = (reported, author) {
                notes::record_report(&task, &devws, &author, &reported, word, &summary);
            }
        }
        Command::Mcp { task } => {
            let task = open(&task)?;
            mcp::Server {
                task: &task,
                home: &home,
                devws: &devws,
                gh: prs::gh_command(),
            }
            .serve(std::io::stdin().lock(), std::io::stdout().lock())?;
        }
        Command::Board {
            task,
            once,
            width,
            height,
            view,
        } => {
            let task = open(&task)?;
            if once {
                println!(
                    "{}",
                    board_tui::once(&task, &devws, &home, width, height, view)?
                );
            } else {
                board_tui::run(&task, &devws, &home, view)?;
            }
        }
        Command::Watch { task, once } => {
            let task = open(&task)?;
            let mut dispatcher = watch::Dispatcher::new(&task, &devws, &home, watch::STALL_GRACE);
            dispatcher.run(
                &mut std::io::stdout(),
                Duration::from_secs(1),
                once.then_some(1),
            )?;
        }
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("devagentics: {error:#}");
        std::process::exit(1);
    }
}
