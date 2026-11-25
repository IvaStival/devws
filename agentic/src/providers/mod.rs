//! The AI programs devAgentics runs, one module each (claude.rs, codex.rs…),
//! behind `Provider`. Installing a program is its module here, a `Kind`
//! variant and its line in `Kind::provider`, beside its devws module
//! (config/tmuxinator/agents/<n>-<kind>/agent.sh); a test checks the two
//! sides list the same programs (docs/architecture/agent-modules.md).
//! Nothing outside this module matches on a `Kind`: it asks its provider.

mod antigravity;
mod claude;
mod codex;
mod opencode;

#[cfg(test)]
pub use antigravity::FLOOR_MODEL as FLOOR_ANTIGRAVITY_MODEL;
#[cfg(test)]
pub use claude::FLOOR_MODEL as FLOOR_CLAUDE_MODEL;
#[cfg(test)]
pub use codex::FLOOR_MODEL as FLOOR_CODEX_MODEL;

use crate::agent_args::Launch;
use crate::roles::{Fallback, Role};

/// The program an agent runs. devws names agents (`claude_work`); this is what
/// they run, which decides the flags they take.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Claude,
    Codex,
    Opencode,
    Antigravity,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Claude, Kind::Codex, Kind::Opencode, Kind::Antigravity];

    /// The one place a program is named: every question about it goes to
    /// its provider.
    pub fn provider(self) -> &'static dyn Provider {
        match self {
            Kind::Claude => &claude::Claude,
            Kind::Codex => &codex::Codex,
            Kind::Opencode => &opencode::Opencode,
            Kind::Antigravity => &antigravity::Antigravity,
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }

    /// The name `parse` reads, and devws's module kind.
    pub fn name(self) -> &'static str {
        self.provider().name()
    }
}

/// The sandbox a role runs in on its program, as the board and task_status
/// show it: an agent devAgentics cannot place (an account only devws knows)
/// is left to its program.
pub fn sandbox_label(role: &Role) -> String {
    Kind::parse(&role.agent)
        .and_then(|kind| kind.provider().sandbox(role))
        .unwrap_or_else(|| "provider-managed".to_string())
}

/// The approval mode a role runs with: its own, else its program's default.
pub fn approvals_label(role: &Role) -> String {
    role.approvals.clone().unwrap_or_else(|| {
        Kind::parse(&role.agent)
            .map_or("provider-default", |kind| {
                kind.provider().default_approvals(role)
            })
            .to_string()
    })
}

/// How a reviewed task runs a program: the locked-down adapter it uses, its
/// turns driven by the worker panel (worker_tools.rs). Codex's needs the
/// user's opt-in (team.rs's CodexPolicy).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewedAdapter {
    Claude,
    Codex,
    Opencode,
}

/// What devAgentics knows about a program.
pub trait Provider: Sync {
    fn name(&self) -> &'static str;
    /// The arguments for its program, after devws's own (`devws agent spawn
    /// --agent-args`).
    fn arguments(&self, launch: &Launch) -> Vec<String>;
    /// The values a role's settings take on it (`devagentics role set`); for
    /// a model, the known ones, as any other may be typed.
    fn models(&self) -> &'static [&'static str];
    fn efforts(&self) -> &'static [&'static str];
    fn approval_modes(&self) -> &'static [&'static str];
    /// The factory floor's family on it and its model there: a model the role
    /// names stays only when it is of the family. None keeps the named one.
    fn floor(&self) -> Option<(&'static str, &'static str)>;
    /// A role's next program when it names no fallback.
    fn default_fallback(&self) -> Fallback;
    /// Its approval mode for a role that names none (the board, task_status).
    fn default_approvals(&self, role: &Role) -> &'static str;
    /// Its sandbox for the role, when the program has one devAgentics sets.
    fn sandbox(&self, _role: &Role) -> Option<String> {
        None
    }
    /// Whether it can run a team: take the leader's MCP tools.
    fn can_lead(&self) -> bool {
        true
    }
    /// Whether the board's model picker asks devws for its account's models.
    fn lists_models(&self) -> bool {
        false
    }
    /// Whether its models are named `provider/model`.
    fn provider_models(&self) -> bool {
        false
    }
    /// Whether the user's own switches (opencode.toml) decide where it runs,
    /// and the board shows its own rows for it.
    fn own_switches(&self) -> bool {
        false
    }
    /// Whether its account is prepaid: none starts on it once spent.
    fn prepaid(&self) -> bool {
        false
    }
    /// The reviewed adapter a worker runs on it with; None: a reviewed task
    /// does not run it.
    fn reviewed_worker(&self) -> Option<ReviewedAdapter> {
        None
    }
    /// The reviewed adapter a leader runs on it with.
    fn reviewed_leader(&self) -> Option<ReviewedAdapter> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_is_known_by_its_provider_s_name() {
        for kind in Kind::ALL {
            assert_eq!(Kind::parse(kind.name()), Some(kind));
        }
        assert_eq!(Kind::parse("openrouter"), None);
    }

    /// Installing a program on one side only is a test failure: devws's
    /// modules and these providers list the same programs, in one order.
    #[test]
    fn devws_has_a_module_for_every_provider() {
        let modules =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../config/tmuxinator/agents");
        let mut kinds: Vec<_> = std::fs::read_dir(&modules)
            .unwrap()
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                if !path.join("agent.sh").is_file() {
                    return None;
                }
                Some(path.file_name()?.to_str()?.to_string())
            })
            .collect();
        kinds.sort();
        let kinds: Vec<_> = kinds
            .iter()
            .map(|folder| {
                folder
                    .split_once('-')
                    .map_or(folder.as_str(), |(_, kind)| kind)
            })
            .collect();
        let providers: Vec<_> = Kind::ALL.iter().map(|kind| kind.name()).collect();
        assert_eq!(kinds, providers);
    }
}
