//! A task's specs: the plan parts its steps belong to, and their release
//! versions. The task is major version 1; each spec is a minor release, in
//! the order it started (`1.1.0`, `1.2.0`…), so a spec keeps its number as
//! more work comes; each of its steps is a patch (`1.3.1`, `1.3.2`…). The
//! leader names new work the same way (`next_version`), so old and new read
//! alike; a spec already named as a release (`1.87.0`) keeps that name.

use crate::state::Step;

/// The task's major version: every spec is a minor release of it.
const MAJOR: u32 = 1;

/// A release version's numbers: `1.83.2` → (1, 83, 2).
fn release(name: &str) -> Option<(u32, u32, u32)> {
    let mut numbers = name.split('.').map(|part| part.parse::<u32>().ok());
    let version = (numbers.next()??, numbers.next()??, numbers.next()??);
    numbers.next().is_none().then_some(version)
}

/// The spec a step belongs to: the one it was ordered with; for a step named
/// as a release, its minor release (`1.87.2` → `1.87.0`); else its id up to
/// its last numbered part, before any `.` (`H2f.3`, `H2f-execution` → `H2f`;
/// `RP-B3-support` → `RP-B3`); else the step alone (`replay-contracts`).
pub fn spec_of(step: &Step) -> String {
    if let Some(spec) = &step.spec {
        return spec.clone();
    }
    if let Some((major, minor, _)) = release(&step.id) {
        return format!("{major}.{minor}.0");
    }
    let head = step.id.split('.').next().unwrap_or(&step.id);
    let parts: Vec<&str> = head.split('-').collect();
    match parts
        .iter()
        .rposition(|part| part.chars().any(|char| char.is_ascii_digit()))
    {
        Some(last) => parts[..=last].join("-"),
        None => step.id.clone(),
    }
}

/// Every spec, in the order its first step came, with its release version.
pub fn versions(steps: &[Step]) -> Vec<(String, String)> {
    let mut specs: Vec<String> = Vec::new();
    for step in steps {
        let spec = spec_of(step);
        if !specs.contains(&spec) {
            specs.push(spec);
        }
    }
    specs
        .into_iter()
        .enumerate()
        .map(|(index, spec)| {
            let version = match release(&spec) {
                Some((major, minor, 0)) => format!("{major}.{minor}.0"),
                _ => format!("{MAJOR}.{}.0", index + 1),
            };
            (spec, version)
        })
        .collect()
}

/// The patch release of a spec's `place`-th step: `1.83.0` and 2 → `1.83.2`.
pub fn step_version(spec_version: &str, place: usize) -> String {
    let line = spec_version.strip_suffix(".0").unwrap_or(spec_version);
    format!("{line}.{place}")
}

/// Each step's release version, by step id: its spec's, with its place in
/// it as the patch.
pub fn step_versions(steps: &[Step]) -> std::collections::BTreeMap<String, String> {
    let versions = versions(steps);
    let mut places: std::collections::BTreeMap<String, usize> = Default::default();
    steps
        .iter()
        .map(|step| {
            let spec = spec_of(step);
            let version = versions
                .iter()
                .find(|(name, _)| *name == spec)
                .map_or(spec.as_str(), |(_, version)| version.as_str())
                .to_string();
            let place = places.entry(spec).or_default();
            *place += 1;
            (step.id.clone(), step_version(&version, *place))
        })
        .collect()
}

/// The release a new spec gets: the minor after every spec there is.
pub fn next_version(steps: &[Step]) -> String {
    format!("{MAJOR}.{}.0", versions(steps).len() + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::StepStatus;

    fn step(id: &str, spec: Option<&str>) -> Step {
        Step {
            id: id.into(),
            title: format!("title {id}"),
            status: StepStatus::Accepted,
            attempts: 1,
            spec: spec.map(str::to_string),
        }
    }

    #[test]
    fn specs_are_minor_releases_in_the_order_they_started() {
        let steps = [
            step("p0", None),
            step("H2f-execution", None),
            step("RP-B3", None),
            step("H2f.1", None),
            step("RP-B3-support", None),
            step("replay-contracts", None),
            step("pf", Some("H2f")),
        ];
        assert_eq!(
            versions(&steps),
            [
                ("p0".to_string(), "1.1.0".to_string()),
                ("H2f".into(), "1.2.0".into()),
                ("RP-B3".into(), "1.3.0".into()),
                ("replay-contracts".into(), "1.4.0".into()),
            ]
        );
        assert_eq!(next_version(&steps), "1.5.0");
        let numbered = step_versions(&steps);
        assert_eq!(numbered["H2f-execution"], "1.2.1");
        assert_eq!(numbered["H2f.1"], "1.2.2");
        assert_eq!(numbered["pf"], "1.2.3", "its spec puts it in H2f");
        assert_eq!(numbered["replay-contracts"], "1.4.1");
    }

    #[test]
    fn work_the_leader_named_as_a_release_keeps_its_name() {
        let steps = [
            step("p0", None),
            step("1.2.1", None),
            step("1.2.2", None),
            step("check", Some("1.2.0")),
        ];
        assert_eq!(spec_of(&steps[1]), "1.2.0");
        assert_eq!(
            versions(&steps),
            [
                ("p0".to_string(), "1.1.0".to_string()),
                ("1.2.0".into(), "1.2.0".into()),
            ]
        );
        assert_eq!(next_version(&steps), "1.3.0");
        assert_eq!(step_versions(&steps)["check"], "1.2.3");
        assert_eq!(release("1.2"), None);
        assert_eq!(release("1.2.3.4"), None);
        assert_eq!(release("H2.1.0"), None);
    }
}
