//! Bounded incremental timing summaries; missing historical data stays unknown.
use crate::task::Task;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{Read, Seek, SeekFrom},
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Interval {
    pub category: String,
    pub start: u64,
    pub end: u64,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Summary {
    pub events: u64,
    pub aggregate_seconds: BTreeMap<String, u64>,
    pub elapsed_union_seconds: u64,
    pub unknown_historical_timing: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Timing {
    pub intervals: Vec<Interval>,
    pub log_offset: u64,
    pub log_events: u64,
    pub unknown_historical_timing: bool,
}
impl Timing {
    pub fn record(&mut self, category: &str, start: u64, end: u64) -> Result<()> {
        ensure!(
            end >= start && !category.trim().is_empty(),
            "invalid timing interval"
        );
        self.intervals.push(Interval {
            category: category.into(),
            start,
            end,
        });
        Ok(())
    }
    pub fn summary(&self) -> Summary {
        let mut summary = Summary {
            events: self.log_events,
            unknown_historical_timing: self.unknown_historical_timing,
            ..Default::default()
        };
        let mut intervals = self.intervals.iter().collect::<Vec<_>>();
        intervals.sort_by_key(|interval| interval.start);
        let mut union: Option<(u64, u64)> = None;
        for interval in intervals {
            *summary
                .aggregate_seconds
                .entry(interval.category.clone())
                .or_default() += interval.end - interval.start;
            match union {
                Some((start, end)) if interval.start <= end => {
                    union = Some((start, end.max(interval.end)))
                }
                Some((start, end)) => {
                    summary.elapsed_union_seconds += end - start;
                    union = Some((interval.start, interval.end));
                }
                None => union = Some((interval.start, interval.end)),
            }
        }
        if let Some((start, end)) = union {
            summary.elapsed_union_seconds += end - start;
        }
        summary
    }
}
/// Cap work per call and advance only over complete JSONL records.
pub fn update(task: &Task) -> Result<Summary> {
    let mut state = task.state()?;
    let path = task.dir().join("log.jsonl");
    if let Ok(mut file) = std::fs::File::open(path) {
        if file.metadata()?.len() < state.timing.log_offset {
            state.timing.log_offset = 0;
            state.timing.log_events = 0;
        }
        file.seek(SeekFrom::Start(state.timing.log_offset))?;
        let mut bytes = Vec::new();
        file.take(256 * 1024).read_to_end(&mut bytes)?;
        if let Some(last) = bytes.iter().rposition(|byte| *byte == b'\n') {
            for line in bytes[..=last]
                .split(|byte| *byte == b'\n')
                .filter(|line| !line.is_empty())
            {
                if serde_json::from_slice::<serde_json::Value>(line).is_ok() {
                    state.timing.log_events += 1;
                    state.timing.unknown_historical_timing = true;
                }
            }
            state.timing.log_offset += (last + 1) as u64;
        }
    }
    let timing = state.timing;
    task.transaction(|tx| {
        if tx.state.timing.log_offset <= timing.log_offset {
            tx.state.timing.log_offset = timing.log_offset;
            tx.state.timing.log_events = timing.log_events;
            tx.state.timing.unknown_historical_timing = timing.unknown_historical_timing;
        }
        Ok(tx.state.timing.summary())
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parallel_checks_are_not_double_counted_as_elapsed_time() {
        let mut timing = Timing::default();
        timing.record("checks", 10, 20).unwrap();
        timing.record("checks", 15, 30).unwrap();
        timing.record("review", 35, 40).unwrap();
        let result = timing.summary();
        assert_eq!(result.aggregate_seconds["checks"], 25);
        assert_eq!(result.elapsed_union_seconds, 25);
        assert!(timing.record("bad", 5, 2).is_err());
    }
}
