//! Deterministic, freshness-aware agent state aggregation.
//!
//! The caller supplies snapshots with wall-clock Unix timestamps and a
//! freshness window. This module does not sample processes or infer lifecycle
//! events. `done_unseen` is an explicit input from an event/viewed-state owner.

use std::io;

use crate::AgentState;

const MAX_OBSERVATIONS: usize = 4096;
const MAX_ID_BYTES: usize = 128;

/// UI state after applying explicit completion and freshness information.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayState {
    /// No usable state evidence exists.
    Unknown,
    /// A fresh observation indicates no current work.
    Idle,
    /// An agent finished and its completion has not been viewed.
    Done,
    /// At least one fresh agent is actively working.
    Working,
    /// At least one fresh agent needs human input.
    Blocked,
    /// Observations exist, but every observation is outside the freshness window.
    Stale,
}

/// One agent's latest state snapshot. `observed_at_ms` is a Unix timestamp in
/// milliseconds from the same wall clock used as the rollup's `now_ms`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentObservation {
    /// Workspace containing the pane.
    pub workspace_id: String,
    /// Tab containing the pane.
    pub tab_id: String,
    /// Stable pane identifier; must be unique in one rollup input.
    pub pane_id: String,
    /// State classified by detection or an authoritative lifecycle event.
    pub state: AgentState,
    /// Whether an idle state represents an unviewed completion event.
    pub done_unseen: bool,
    /// Time at which this state was observed, in Unix milliseconds.
    pub observed_at_ms: u64,
}

/// Aggregate for a pane, tab, or workspace, with freshness counts retained.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub struct Rollup {
    /// Scope type (`pane`, `tab`, or `workspace`).
    pub scope: &'static str,
    /// Stable identifier of the selected scope.
    pub id: String,
    /// Highest-priority state among fresh observations, or `stale`/`unknown`.
    pub state: DisplayState,
    /// Number of observations in scope.
    pub observation_count: usize,
    /// Number of observations at or inside the freshness window.
    pub fresh_count: usize,
    /// Number of observations outside the freshness window.
    pub stale_count: usize,
    /// Number of fresh unknown observations.
    pub unknown_count: usize,
    /// Number of fresh, unviewed completion observations.
    pub done_unseen_count: usize,
    /// Pane identifiers contributing a fresh blocked state, in sorted order.
    pub blocked_pane_ids: Vec<String>,
}

/// Aggregate observations for one pane.
///
/// # Errors
///
/// Returns `InvalidInput` for invalid IDs, freshness bounds, duplicate pane
/// observations, or completion flags inconsistent with the semantic state.
pub fn rollup_pane(
    pane_id: &str,
    observations: &[AgentObservation],
    now_ms: u64,
    stale_after_ms: u64,
) -> io::Result<Rollup> {
    if !valid_id(pane_id) {
        return Err(invalid_input("invalid pane id"));
    }
    let selected = observations
        .iter()
        .filter(|item| item.pane_id == pane_id)
        .collect::<Vec<_>>();
    aggregate("pane", pane_id, &selected, now_ms, stale_after_ms)
}

/// Aggregate observations for one tab.
///
/// # Errors
///
/// Returns `InvalidInput` for invalid IDs, freshness bounds, duplicate pane
/// observations, or completion flags inconsistent with the semantic state.
pub fn rollup_tab(
    workspace_id: &str,
    tab_id: &str,
    observations: &[AgentObservation],
    now_ms: u64,
    stale_after_ms: u64,
) -> io::Result<Rollup> {
    if !valid_id(workspace_id) || !valid_id(tab_id) {
        return Err(invalid_input("invalid workspace or tab id"));
    }
    let selected = observations
        .iter()
        .filter(|item| item.workspace_id == workspace_id && item.tab_id == tab_id)
        .collect::<Vec<_>>();
    aggregate("tab", tab_id, &selected, now_ms, stale_after_ms)
}

/// Aggregate observations for one workspace.
///
/// # Errors
///
/// Returns `InvalidInput` for invalid IDs, freshness bounds, duplicate pane
/// observations, or completion flags inconsistent with the semantic state.
pub fn rollup_workspace(
    workspace_id: &str,
    observations: &[AgentObservation],
    now_ms: u64,
    stale_after_ms: u64,
) -> io::Result<Rollup> {
    if !valid_id(workspace_id) {
        return Err(invalid_input("invalid workspace id"));
    }
    let selected = observations
        .iter()
        .filter(|item| item.workspace_id == workspace_id)
        .collect::<Vec<_>>();
    aggregate("workspace", workspace_id, &selected, now_ms, stale_after_ms)
}

fn aggregate(
    scope: &'static str,
    id: &str,
    items: &[&AgentObservation],
    now_ms: u64,
    stale_after_ms: u64,
) -> io::Result<Rollup> {
    if stale_after_ms == 0 || items.len() > MAX_OBSERVATIONS {
        return Err(invalid_input("invalid agent rollup bounds"));
    }
    let mut pane_ids = std::collections::BTreeSet::new();
    for item in items {
        if !valid_id(&item.workspace_id)
            || !valid_id(&item.tab_id)
            || !valid_id(&item.pane_id)
            || !pane_ids.insert(item.pane_id.as_str())
            || (item.done_unseen && item.state != AgentState::Idle)
        {
            return Err(invalid_input("invalid or duplicate agent observation"));
        }
    }

    let mut fresh_count = 0;
    let mut stale_count = 0;
    let mut unknown_count = 0;
    let mut done_unseen_count = 0;
    let mut blocked_pane_ids = Vec::new();
    let mut state = DisplayState::Unknown;
    for item in items {
        let age_ms = now_ms.saturating_sub(item.observed_at_ms);
        if age_ms > stale_after_ms {
            stale_count += 1;
            continue;
        }
        fresh_count += 1;
        let candidate = match item.state {
            AgentState::Unknown => {
                unknown_count += 1;
                DisplayState::Unknown
            }
            AgentState::Idle if item.done_unseen => {
                done_unseen_count += 1;
                DisplayState::Done
            }
            AgentState::Idle => DisplayState::Idle,
            AgentState::Working => DisplayState::Working,
            AgentState::Blocked => {
                blocked_pane_ids.push(item.pane_id.clone());
                DisplayState::Blocked
            }
        };
        state = state.max(candidate);
    }
    if fresh_count == 0 && stale_count > 0 {
        state = DisplayState::Stale;
    }
    blocked_pane_ids.sort();
    Ok(Rollup {
        scope,
        id: id.to_owned(),
        state,
        observation_count: items.len(),
        fresh_count,
        stale_count,
        unknown_count,
        done_unseen_count,
        blocked_pane_ids,
    })
}

fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_ID_BYTES && !value.chars().any(char::is_control)
}

fn invalid_input(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(pane_id: &str, state: AgentState) -> AgentObservation {
        AgentObservation {
            workspace_id: "ws".to_owned(),
            tab_id: "tab".to_owned(),
            pane_id: pane_id.to_owned(),
            state,
            done_unseen: false,
            observed_at_ms: 1_000,
        }
    }

    #[test]
    fn priority_is_blocked_then_working_then_done_then_idle() {
        let mut done = observation("done", AgentState::Idle);
        done.done_unseen = true;
        let items = vec![
            observation("idle", AgentState::Idle),
            done,
            observation("working", AgentState::Working),
            observation("blocked", AgentState::Blocked),
        ];
        let result = rollup_workspace("ws", &items, 1_000, 500).unwrap();
        assert_eq!(result.state, DisplayState::Blocked);
        assert_eq!(result.done_unseen_count, 1);
        assert_eq!(result.blocked_pane_ids, ["blocked"]);
    }

    #[test]
    fn done_remains_visible_until_the_owner_clears_unseen_flag() {
        let mut item = observation("p1", AgentState::Idle);
        item.done_unseen = true;
        assert_eq!(
            rollup_pane("p1", &[item.clone()], 1_000, 500)
                .unwrap()
                .state,
            DisplayState::Done
        );
        item.done_unseen = false;
        assert_eq!(
            rollup_pane("p1", &[item], 1_000, 500).unwrap().state,
            DisplayState::Idle
        );
    }

    #[test]
    fn stale_is_shown_only_when_every_observation_is_stale() {
        let stale = observation("old", AgentState::Blocked);
        let result = rollup_workspace("ws", &[stale.clone()], 2_000, 500).unwrap();
        assert_eq!(result.state, DisplayState::Stale);
        assert_eq!(result.stale_count, 1);
        let mut current = observation("current", AgentState::Idle);
        current.observed_at_ms = 1_900;
        let mixed = rollup_workspace("ws", &[stale, current], 2_000, 500).unwrap();
        assert_eq!(mixed.state, DisplayState::Idle);
        assert_eq!(mixed.stale_count, 1);
    }

    #[test]
    fn unknown_and_empty_inputs_do_not_invent_activity() {
        let unknown = observation("p1", AgentState::Unknown);
        assert_eq!(
            rollup_tab("ws", "tab", &[unknown], 1_000, 500)
                .unwrap()
                .state,
            DisplayState::Unknown
        );
        assert_eq!(
            rollup_tab("ws", "tab", &[], 1_000, 500).unwrap().state,
            DisplayState::Unknown
        );
    }

    #[test]
    fn scope_filters_and_duplicate_or_inconsistent_records_fail_closed() {
        let mut items = vec![observation("p1", AgentState::Working)];
        items.push(AgentObservation {
            workspace_id: "elsewhere".into(),
            ..observation("p2", AgentState::Blocked)
        });
        assert_eq!(
            rollup_workspace("ws", &items, 1_000, 500).unwrap().state,
            DisplayState::Working
        );
        items.push(observation("p1", AgentState::Idle));
        assert_eq!(
            rollup_workspace("ws", &items, 1_000, 500)
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        let mut invalid = observation("p3", AgentState::Working);
        invalid.done_unseen = true;
        assert!(rollup_workspace("ws", &[invalid], 1_000, 500).is_err());
    }

    #[test]
    fn timestamps_are_inclusive_and_future_clock_skew_is_clamped() {
        let item = observation("p1", AgentState::Working);
        assert_eq!(
            rollup_pane("p1", &[item.clone()], 1_500, 500)
                .unwrap()
                .fresh_count,
            1
        );
        assert_eq!(
            rollup_pane("p1", &[item], 999, 500).unwrap().state,
            DisplayState::Working
        );
        assert!(rollup_workspace("ws", &[], 1_000, 0).is_err());
    }
}
