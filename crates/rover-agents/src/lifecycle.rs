//! Sequenced lifecycle authority over screen-based estimates.
//!
//! This bounded in-memory registry is scoped to one pane by its owner. It
//! provides source ordering and release, not transport, persistence, or hooks.

use std::collections::BTreeMap;
use std::io;

use crate::AgentState;

const MAX_SOURCES_PER_PANE: usize = 32;
const MAX_SOURCE_ID_BYTES: usize = 80;

/// One source-scoped lifecycle event.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LifecycleEvent {
    /// Report semantic state with a monotonically increasing source sequence.
    Report {
        source: String,
        sequence: u64,
        state: AgentState,
    },
    /// Release a source with a monotonically increasing source sequence.
    Release { source: String, sequence: u64 },
}

/// Result of applying one ordered lifecycle event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LifecycleResult {
    /// Event became the latest state for its source.
    Applied,
    /// Event was valid but its sequence was older than or equal to the latest.
    IgnoredOutOfOrder,
}

/// Effective state, authority owner, and screen fallback disposition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthorityDecision {
    /// Active source state, screen state, or unknown on active-source conflict.
    pub state: AgentState,
    /// Sole active lifecycle source; absent during fallback or conflict.
    pub source: Option<String>,
    /// Whether lifecycle reports suppress screen fallback.
    pub screen_suppressed: bool,
    /// Whether multiple active sources made the state ambiguous.
    pub source_conflict: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SourceState {
    last_sequence: u64,
    active: bool,
    state: AgentState,
}

/// Bounded lifecycle authority registry for one pane.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LifecycleAuthority {
    sources: BTreeMap<String, SourceState>,
}

impl LifecycleAuthority {
    /// Apply a source-sequenced report or release.
    ///
    /// Reports accept only idle, working, and blocked. High-water marks remain
    /// after release for this registry's lifetime to prevent delayed revival.
    ///
    /// # Errors
    ///
    /// Returns `InvalidInput` for unsafe source IDs, unsupported report states,
    /// or source-limit overflow.
    pub fn apply(&mut self, event: LifecycleEvent) -> io::Result<LifecycleResult> {
        let (source, sequence, state) = match event {
            LifecycleEvent::Report {
                source,
                sequence,
                state,
            } => {
                if !matches!(
                    state,
                    AgentState::Idle | AgentState::Working | AgentState::Blocked
                ) {
                    return Err(invalid_input("unsupported lifecycle report state"));
                }
                (source, sequence, Some(state))
            }
            LifecycleEvent::Release { source, sequence } => (source, sequence, None),
        };
        if !valid_source(&source) {
            return Err(invalid_input("invalid lifecycle source"));
        }
        if !self.sources.contains_key(&source) && self.sources.len() == MAX_SOURCES_PER_PANE {
            return Err(invalid_input("lifecycle source limit exceeded"));
        }
        if self
            .sources
            .get(&source)
            .is_some_and(|record| sequence <= record.last_sequence)
        {
            return Ok(LifecycleResult::IgnoredOutOfOrder);
        }
        let record = self.sources.entry(source).or_insert(SourceState {
            last_sequence: sequence,
            active: false,
            state: AgentState::Unknown,
        });
        record.last_sequence = sequence;
        if let Some(state) = state {
            record.active = true;
            record.state = state;
        } else {
            record.active = false;
            record.state = AgentState::Unknown;
        }
        Ok(LifecycleResult::Applied)
    }

    /// Resolve lifecycle authority over a screen estimate.
    ///
    /// One active lifecycle source suppresses screen state. Multiple active
    /// sources also suppress fallback but resolve to unknown. With no active
    /// source, the provided screen state is returned unchanged.
    #[must_use]
    pub fn resolve(&self, screen_state: AgentState) -> AuthorityDecision {
        let mut active = self.sources.iter().filter(|(_, record)| record.active);
        let Some((source, record)) = active.next() else {
            return AuthorityDecision {
                state: screen_state,
                source: None,
                screen_suppressed: false,
                source_conflict: false,
            };
        };
        if active.next().is_some() {
            return AuthorityDecision {
                state: AgentState::Unknown,
                source: None,
                screen_suppressed: true,
                source_conflict: true,
            };
        }
        AuthorityDecision {
            state: record.state,
            source: Some(source.clone()),
            screen_suppressed: true,
            source_conflict: false,
        }
    }
}

fn valid_source(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_SOURCE_ID_BYTES
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, ':' | '.' | '_' | '-')
        })
}

fn invalid_input(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(source: &str, sequence: u64, state: AgentState) -> LifecycleEvent {
        LifecycleEvent::Report {
            source: source.to_owned(),
            sequence,
            state,
        }
    }

    fn release(source: &str, sequence: u64) -> LifecycleEvent {
        LifecycleEvent::Release {
            source: source.to_owned(),
            sequence,
        }
    }

    #[test]
    fn authoritative_report_suppresses_screen_until_release() {
        let mut registry = LifecycleAuthority::default();
        registry
            .apply(report("hook:agent", 1, AgentState::Working))
            .unwrap();
        let decision = registry.resolve(AgentState::Blocked);
        assert_eq!(decision.state, AgentState::Working);
        assert_eq!(decision.source.as_deref(), Some("hook:agent"));
        assert!(decision.screen_suppressed);
        registry.apply(release("hook:agent", 2)).unwrap();
        let decision = registry.resolve(AgentState::Blocked);
        assert_eq!(decision.state, AgentState::Blocked);
        assert!(!decision.screen_suppressed);
    }

    #[test]
    fn old_events_cannot_reorder_state_or_revive_released_authority() {
        let mut registry = LifecycleAuthority::default();
        registry
            .apply(report("hook:agent", 4, AgentState::Working))
            .unwrap();
        assert_eq!(
            registry
                .apply(report("hook:agent", 3, AgentState::Blocked))
                .unwrap(),
            LifecycleResult::IgnoredOutOfOrder
        );
        registry.apply(release("hook:agent", 5)).unwrap();
        assert_eq!(
            registry
                .apply(report("hook:agent", 4, AgentState::Working))
                .unwrap(),
            LifecycleResult::IgnoredOutOfOrder
        );
        assert_eq!(registry.resolve(AgentState::Idle).state, AgentState::Idle);
    }

    #[test]
    fn equal_sequence_is_ignored_and_zero_is_valid_first_sequence() {
        let mut registry = LifecycleAuthority::default();
        registry
            .apply(report("hook:agent", 0, AgentState::Idle))
            .unwrap();
        assert_eq!(
            registry
                .apply(report("hook:agent", 0, AgentState::Working))
                .unwrap(),
            LifecycleResult::IgnoredOutOfOrder
        );
        assert_eq!(
            registry.resolve(AgentState::Blocked).state,
            AgentState::Idle
        );
    }

    #[test]
    fn multiple_active_sources_suppress_screen_and_fail_closed() {
        let mut registry = LifecycleAuthority::default();
        registry
            .apply(report("hook:a", 1, AgentState::Working))
            .unwrap();
        registry
            .apply(report("hook:b", 1, AgentState::Working))
            .unwrap();
        let decision = registry.resolve(AgentState::Idle);
        assert_eq!(decision.state, AgentState::Unknown);
        assert!(decision.screen_suppressed);
        assert!(decision.source_conflict);
    }

    #[test]
    fn unsafe_states_sources_and_source_overflow_are_rejected() {
        let mut registry = LifecycleAuthority::default();
        assert!(registry
            .apply(report("hook", 1, AgentState::Unknown))
            .is_err());
        assert!(registry
            .apply(report("bad source", 1, AgentState::Idle))
            .is_err());
        for index in 0..MAX_SOURCES_PER_PANE {
            registry
                .apply(release(&format!("hook:{index}"), 1))
                .unwrap();
        }
        assert!(registry.apply(release("hook:overflow", 1)).is_err());
    }
}
