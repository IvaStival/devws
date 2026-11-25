//! Which team groups work on opencode, and on which model: the user's switches
//! (`devagentics opencode`, the devws menu's OpenCode screen). The global
//! switch is mandatory: while it is off, opencode runs no agent, whatever the
//! groups' switches say, and a role that names opencode falls back to its
//! other programs.
use crate::home::Home;
use crate::roles::{Fallback, Group};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// The account devws registers for opencode unless the user names another.
pub const DEFAULT_ACCOUNT: &str = "opencode";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct OpencodeSettings {
    pub enabled: bool,
    /// The devws agent (`DEVWS_AGENTS`) that runs opencode.
    pub account: String,
    pub floor: GroupSettings,
    pub senior: GroupSettings,
    /// A reviewed task's leader, as its last resort: when its own program
    /// and its fallback have both used their limits (crate::leader).
    pub leader: GroupSettings,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct GroupSettings {
    pub enabled: bool,
    /// `provider/model`, as `opencode models` lists it. None until the user
    /// picks one: a group never runs on a model nobody chose (Zen's free ones
    /// have their own terms).
    pub model: Option<String>,
}

impl Default for OpencodeSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            account: DEFAULT_ACCOUNT.to_string(),
            floor: GroupSettings::default(),
            senior: GroupSettings::default(),
            leader: GroupSettings::default(),
        }
    }
}

impl OpencodeSettings {
    pub fn path(home: &Home) -> PathBuf {
        home.root().join("opencode.toml")
    }

    /// No file means nothing is switched on.
    pub fn load(home: &Home) -> Result<Self> {
        let path = Self::path(home);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => {
                return Err(error).with_context(|| format!("reading {}", path.display()));
            }
        };
        toml::from_str(&text).with_context(|| format!("reading {}", path.display()))
    }

    pub fn save(&self, home: &Home) -> Result<()> {
        let path = Self::path(home);
        fs::create_dir_all(home.root())
            .with_context(|| format!("creating {}", home.root().display()))?;
        fs::write(&path, toml::to_string(self)?)
            .with_context(|| format!("writing {}", path.display()))
    }

    pub fn group(&self, group: Group) -> Option<&GroupSettings> {
        match group {
            Group::Floor => Some(&self.floor),
            Group::Senior => Some(&self.senior),
            // A leader is never started as a worker is: it moves to opencode
            // only as its last resort (`leader_candidate`).
            Group::Cto => None,
        }
    }

    pub fn group_mut(&mut self, group: Group) -> Result<&mut GroupSettings> {
        match group {
            Group::Floor => Ok(&mut self.floor),
            Group::Senior => Ok(&mut self.senior),
            Group::Cto => bail!("the leader's opencode settings are `leader`, not a group's"),
        }
    }

    /// `on`/`off` for each switch, a group's model (`-` for none), and the
    /// account, tab-separated, one a line: what the devws menu shows. A group
    /// switched on while the global one is off reads `ignored`.
    pub fn describe(&self) -> String {
        let switch = |on: bool| if on { "on" } else { "off" };
        let group_line = |group: Group| {
            let settings = self.group(group).expect("floor and senior have settings");
            let state = if settings.enabled && !self.enabled {
                "ignored"
            } else {
                switch(settings.enabled)
            };
            format!(
                "{}\t{state}\t{}",
                group.name(),
                settings.model.as_deref().unwrap_or("-")
            )
        };
        let leader_state = if self.leader.enabled && !self.enabled {
            "ignored"
        } else {
            switch(self.leader.enabled)
        };
        [
            format!("global\t{}", switch(self.enabled)),
            group_line(Group::Floor),
            group_line(Group::Senior),
            format!(
                "leader\t{leader_state}\t{}",
                self.leader.model.as_deref().unwrap_or("-")
            ),
            format!("account\t{}", self.account),
        ]
        .join("\n")
    }

    /// Whether opencode may run an agent of `group`: the global switch and
    /// the group's are both on.
    pub fn allows(&self, group: Group) -> bool {
        self.enabled && self.group(group).is_some_and(|settings| settings.enabled)
    }

    /// The leader's last resort: the account and model, when the global and
    /// leader switches are on and the leader has a model.
    pub fn leader_candidate(&self) -> Option<Fallback> {
        if !(self.enabled && self.leader.enabled) {
            return None;
        }
        Some(Fallback {
            agent: self.account.clone(),
            model: Some(self.leader.model.clone()?),
            effort: None,
            approvals: None,
            sandbox: None,
        })
    }

    /// The program a `group` agent tries first, when opencode is on for it
    /// and it has a model.
    pub fn candidate(&self, group: Group) -> Option<Fallback> {
        if !self.allows(group) {
            return None;
        }
        let model = self.group(group)?.model.clone()?;
        Some(Fallback {
            agent: self.account.clone(),
            model: Some(model),
            effort: None,
            approvals: None,
            sandbox: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn switched_on(floor_model: Option<&str>) -> OpencodeSettings {
        OpencodeSettings {
            enabled: true,
            floor: GroupSettings {
                enabled: true,
                model: floor_model.map(str::to_string),
            },
            ..OpencodeSettings::default()
        }
    }

    #[test]
    fn without_a_file_nothing_is_switched_on() {
        let dir = TempDir::new().unwrap();
        let settings = OpencodeSettings::load(&Home::at(dir.path().into())).unwrap();
        assert_eq!(settings, OpencodeSettings::default());
        assert!(!settings.allows(Group::Floor));
        assert!(!settings.allows(Group::Senior));
    }

    #[test]
    fn settings_are_saved_and_read_back() {
        let dir = TempDir::new().unwrap();
        let home = Home::at(dir.path().into());
        let settings = switched_on(Some("opencode/deepseek-v4.1-flash"));
        settings.save(&home).unwrap();
        assert_eq!(OpencodeSettings::load(&home).unwrap(), settings);
    }

    #[test]
    fn an_unknown_setting_is_refused() {
        let dir = TempDir::new().unwrap();
        let home = Home::at(dir.path().into());
        fs::write(
            OpencodeSettings::path(&home),
            "enabled = true\nturbo = true\n",
        )
        .unwrap();
        assert!(OpencodeSettings::load(&home).is_err());
    }

    #[test]
    fn the_global_switch_overrides_the_groups() {
        let mut settings = switched_on(Some("opencode/deepseek-v4.1-flash"));
        assert!(settings.allows(Group::Floor));
        settings.enabled = false;
        assert!(!settings.allows(Group::Floor));
        assert_eq!(settings.candidate(Group::Floor), None);
    }

    #[test]
    fn a_group_runs_on_opencode_only_when_its_own_switch_is_on() {
        let settings = switched_on(Some("opencode/deepseek-v4.1-flash"));
        assert!(!settings.allows(Group::Senior));
        assert_eq!(settings.candidate(Group::Senior), None);
    }

    #[test]
    fn the_leader_is_never_a_worker_group_and_runs_on_opencode_only_when_switched_on() {
        let mut settings = switched_on(Some("opencode/deepseek-v4.1-flash"));
        assert!(!settings.allows(Group::Cto));
        assert!(settings.group_mut(Group::Cto).is_err());
        assert_eq!(settings.leader_candidate(), None, "its switch is off");
        settings.leader.enabled = true;
        assert_eq!(settings.leader_candidate(), None, "it has no model");
        settings.leader.model = Some("opencode/strong".into());
        assert_eq!(
            settings.leader_candidate().unwrap().model.as_deref(),
            Some("opencode/strong")
        );
        settings.enabled = false;
        assert_eq!(
            settings.leader_candidate(),
            None,
            "the global switch is off"
        );
        assert!(
            settings
                .describe()
                .contains("leader\tignored\topencode/strong")
        );
    }

    #[test]
    fn the_description_marks_a_group_the_global_switch_holds_off() {
        let mut settings = switched_on(Some("opencode/deepseek-v4.1-flash"));
        assert_eq!(
            settings.describe(),
            "global\ton\nfloor\ton\topencode/deepseek-v4.1-flash\nsenior\toff\t-\nleader\toff\t-\naccount\topencode"
        );
        settings.enabled = false;
        assert!(
            settings
                .describe()
                .contains("floor\tignored\topencode/deepseek-v4.1-flash")
        );
    }

    #[test]
    fn a_group_without_a_model_has_no_candidate() {
        assert_eq!(switched_on(None).candidate(Group::Floor), None);
    }

    #[test]
    fn the_candidate_is_the_account_on_the_groups_model() {
        let candidate = switched_on(Some("opencode/deepseek-v4.1-flash"))
            .candidate(Group::Floor)
            .unwrap();
        assert_eq!(candidate.agent, DEFAULT_ACCOUNT);
        assert_eq!(
            candidate.model.as_deref(),
            Some("opencode/deepseek-v4.1-flash")
        );
        assert_eq!(candidate.effort, None);
    }
}
