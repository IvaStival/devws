mod agent_args;
mod board;
mod board_tui;
mod board_usage;
mod board_view;
mod checks;
mod devws;
mod home;
mod lanes;
mod leader;
mod log;
mod mcp;
mod orders;
mod plans;
mod prs;
mod questions;
mod roles;
mod start;
mod state;
mod status;
mod task;
mod team;
mod watch;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use devws::CommandDevws;
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
    /// A task: one branch, one team
    Task {
        #[command(subcommand)]
        action: TaskAction,
    },
    /// The roles agents can have
    Role {
        #[command(subcommand)]
        action: RoleAction,
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

#[derive(Subcommand)]
enum TaskAction {
    /// Make the task for a worktree, or find it; prints its id, the leader's
    /// agent and the leader's arguments file (what `devws agentic` calls)
    Start {
        folder: PathBuf,
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
    Pause { task: String },
    /// Take it off pause, and start again the agents whose panels went
    Resume { task: String },
    /// Stop every agent (the worktrees stay)
    Cancel { task: String },
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

/// Claude or Codex: the program of the role's agent (`claude_work` is a
/// Claude account), which decides the values its settings take.
fn role_kind(
    home: &Home,
    repo: Option<&std::path::Path>,
    name: &str,
    devws: &dyn devws::Devws,
) -> Result<agent_args::Kind> {
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
        Command::Task { action } => match action {
            TaskAction::Start { folder, leader } => {
                let command = std::env::current_exe()?.display().to_string();
                let started =
                    start::start_task(&home, &devws, &folder, &command, leader.as_deref())?;
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
            TaskAction::Pause { task } => {
                Team {
                    task: &open(&task)?,
                    home: &home,
                    devws: &devws,
                }
                .pause()?;
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
        Command::Leader {
            action: LeaderAction::Switch { task, reason },
        } => println!(
            "{}",
            leader::request_switch(&home, &open(&task)?, &devws, reason.as_deref())?
        ),
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
        Command::Report { outcome, summary } => {
            let pane = this_pane()?;
            let task = task_of_this_pane(&home)?;
            let outcome = match outcome {
                ReportOutcome::Done => orders::Outcome::Done,
                ReportOutcome::Blocked => orders::Outcome::Blocked,
            };
            println!(
                "{}",
                orders::report(&task, &pane, outcome, &summary, orders::check_timeout())?
            );
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
            let mut dispatcher = watch::Dispatcher::new(&task, &devws, watch::STALL_GRACE);
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
