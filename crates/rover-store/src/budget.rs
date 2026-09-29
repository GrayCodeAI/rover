//! Atomic local budget accounting shared by processes using one Rover store.
//!
//! The ledger is persisted under `settings/budget`. It coordinates one local
//! state store; it does not implement fleet-wide billing or provider usage
//! measurement.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use rover_core::Id;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use serde::de::Deserializer;
use serde::{Deserialize, Serialize};

use crate::{put_in_transaction, Store, StoreError};

const SETTINGS_KIND: &str = "settings";
const BUDGET_ID: &str = "budget";
const MAX_BUDGET_VALUE: i64 = 1 << 30;

fn deserialize_integer<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<i64>::deserialize(deserializer)?.unwrap_or_default())
}

fn deserialize_usage<'de, D>(deserializer: D) -> Result<BTreeMap<String, i64>, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(
        Option::<BTreeMap<String, Option<i64>>>::deserialize(deserializer)?
            .unwrap_or_default()
            .into_iter()
            .map(|(owner, value)| (owner, value.unwrap_or_default()))
            .collect(),
    )
}

/// Per-owner local accounting and an optional aggregate cap.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Budget {
    /// Aggregate cap; zero means enforcement is disabled.
    #[serde(default, deserialize_with = "deserialize_integer")]
    pub cap: i64,
    /// Increments after every successful cap, charge, or reset operation.
    #[serde(default, deserialize_with = "deserialize_integer")]
    pub version: i64,
    /// Charged usage by owner.
    #[serde(rename = "use", default, deserialize_with = "deserialize_usage")]
    pub usage: BTreeMap<String, i64>,
}

impl Budget {
    /// Sum all owner usage without integer overflow.
    #[must_use]
    pub fn total(&self) -> i128 {
        self.usage.values().map(|value| i128::from(*value)).sum()
    }
}

/// A budget ledger or charge failure.
#[derive(Debug)]
pub enum BudgetError {
    /// A cap, owner, or charge is outside the accepted range.
    InvalidRequest,
    /// The stored ledger contains invalid values.
    InvalidLedger,
    /// A valid charge would exceed a configured nonzero cap.
    Exhausted,
    /// The backing store failed.
    Store(StoreError),
}

impl fmt::Display for BudgetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest => formatter.write_str("invalid budget request"),
            Self::InvalidLedger => formatter.write_str("invalid budget ledger"),
            Self::Exhausted => formatter.write_str("budget exhausted"),
            Self::Store(error) => write!(formatter, "budget store failed: {error}"),
        }
    }
}

impl Error for BudgetError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Store(error) => Some(error),
            Self::InvalidRequest | Self::InvalidLedger | Self::Exhausted => None,
        }
    }
}

impl Store {
    /// Read the persisted per-owner budget ledger, defaulting to an uncapped
    /// empty ledger when it has not been created.
    ///
    /// # Errors
    ///
    /// Returns [`BudgetError::InvalidLedger`] for invalid stored values or
    /// [`BudgetError::Store`] for database and JSON errors.
    pub fn budget_use(&self) -> Result<Budget, BudgetError> {
        let connection = self.lock().map_err(BudgetError::Store)?;
        read_budget(&connection)
    }

    /// Set the aggregate cap; zero disables enforcement. Every successful
    /// update increments the ledger version.
    ///
    /// # Errors
    ///
    /// Returns [`BudgetError::InvalidRequest`] for caps outside 0..=2^30,
    /// [`BudgetError::InvalidLedger`] for corrupt stored values, or
    /// [`BudgetError::Store`] for database failures.
    pub fn set_budget_cap(&self, cap: i64) -> Result<(), BudgetError> {
        if !(0..=MAX_BUDGET_VALUE).contains(&cap) {
            return Err(BudgetError::InvalidRequest);
        }
        let mut connection = self.lock().map_err(BudgetError::Store)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(StoreError::from)
            .map_err(BudgetError::Store)?;
        let mut budget = read_budget(&transaction)?;
        budget.cap = cap;
        increment_version(&mut budget)?;
        put_in_transaction(
            &transaction,
            SETTINGS_KIND,
            BUDGET_ID,
            &budget,
            "budget.cap",
        )
        .map_err(BudgetError::Store)?;
        transaction
            .commit()
            .map_err(StoreError::from)
            .map_err(BudgetError::Store)
    }

    /// Add usage for `owner` atomically with aggregate cap enforcement.
    /// Rejected charges leave the budget record, version, and event ledger
    /// unchanged. A zero cap means uncapped accounting.
    ///
    /// # Errors
    ///
    /// Returns [`BudgetError::InvalidRequest`] for invalid owners or charges,
    /// [`BudgetError::InvalidLedger`] for corrupt stored values,
    /// [`BudgetError::Exhausted`] when the cap would be exceeded, or
    /// [`BudgetError::Store`] for database failures.
    pub fn charge_budget(&self, owner: &str, amount: i64) -> Result<(), BudgetError> {
        if Id::parse(owner).is_err() {
            return Err(BudgetError::InvalidRequest);
        }
        if !(0..=MAX_BUDGET_VALUE).contains(&amount) {
            return Err(BudgetError::InvalidRequest);
        }
        let mut connection = self.lock().map_err(BudgetError::Store)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(StoreError::from)
            .map_err(BudgetError::Store)?;
        let mut budget = read_budget(&transaction)?;
        let total_after_charge = budget.total() + i128::from(amount);
        if budget.cap > 0 && total_after_charge > i128::from(budget.cap) {
            return Err(BudgetError::Exhausted);
        }
        let usage = budget.usage.entry(owner.to_owned()).or_default();
        *usage += amount;
        increment_version(&mut budget)?;
        put_in_transaction(
            &transaction,
            SETTINGS_KIND,
            BUDGET_ID,
            &budget,
            "budget.charge",
        )
        .map_err(BudgetError::Store)?;
        transaction
            .commit()
            .map_err(StoreError::from)
            .map_err(BudgetError::Store)
    }

    /// Reset one owner's accounting or all accounting when `owner` is `*`.
    /// Every successful reset increments the ledger version.
    ///
    /// # Errors
    ///
    /// Returns [`BudgetError::InvalidRequest`] for an invalid owner,
    /// [`BudgetError::InvalidLedger`] for corrupt stored values, or
    /// [`BudgetError::Store`] for database failures.
    pub fn reset_budget(&self, owner: &str) -> Result<(), BudgetError> {
        if owner != "*" && Id::parse(owner).is_err() {
            return Err(BudgetError::InvalidRequest);
        }
        let mut connection = self.lock().map_err(BudgetError::Store)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(StoreError::from)
            .map_err(BudgetError::Store)?;
        let mut budget = read_budget(&transaction)?;
        if owner == "*" {
            budget.usage.clear();
        } else {
            budget.usage.remove(owner);
        }
        increment_version(&mut budget)?;
        put_in_transaction(
            &transaction,
            SETTINGS_KIND,
            BUDGET_ID,
            &budget,
            "budget.reset",
        )
        .map_err(BudgetError::Store)?;
        transaction
            .commit()
            .map_err(StoreError::from)
            .map_err(BudgetError::Store)
    }
}

fn read_budget(connection: &Connection) -> Result<Budget, BudgetError> {
    let encoded = connection
        .query_row(
            "SELECT payload FROM records WHERE kind=?1 AND id=?2",
            (SETTINGS_KIND, BUDGET_ID),
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(StoreError::from)
        .map_err(BudgetError::Store)?;
    let Some(encoded) = encoded else {
        return Ok(Budget::default());
    };
    let budget: Budget = serde_json::from_str(&encoded)
        .map_err(StoreError::from)
        .map_err(BudgetError::Store)?;
    if budget.cap < 0
        || budget.cap > MAX_BUDGET_VALUE
        || budget.version < 0
        || budget
            .usage
            .values()
            .any(|value| *value < 0 || *value > MAX_BUDGET_VALUE)
    {
        return Err(BudgetError::InvalidLedger);
    }
    Ok(budget)
}

fn increment_version(budget: &mut Budget) -> Result<(), BudgetError> {
    budget.version = budget
        .version
        .checked_add(1)
        .ok_or(BudgetError::InvalidLedger)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::{SystemTime, UNIX_EPOCH};

    use rusqlite::Connection;

    use super::{Budget, BudgetError};
    use crate::Store;

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn store() -> Store {
        Store::from_connection(Connection::open_in_memory().expect("in-memory SQLite"))
            .expect("migrate in-memory SQLite")
    }

    struct TestDatabase(PathBuf);

    impl TestDatabase {
        fn new() -> Self {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock follows epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "rover-budget-{}-{timestamp}-{}",
                std::process::id(),
                TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            Self(path)
        }

        fn open(&self) -> Store {
            Store::from_connection(Connection::open(&self.0).expect("open shared SQLite file"))
                .expect("migrate shared SQLite file")
        }
    }

    impl Drop for TestDatabase {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
            for suffix in ["-wal", "-shm"] {
                let mut sidecar = self.0.as_os_str().to_os_string();
                sidecar.push(suffix);
                let _ = fs::remove_file(PathBuf::from(sidecar));
            }
        }
    }

    #[test]
    fn cap_charge_usage_and_reset_match_go_contract() {
        let store = store();
        assert_eq!(
            store.budget_use().expect("initial ledger"),
            Budget::default()
        );

        store
            .charge_budget("agent_a", 5)
            .expect("uncapped charge succeeds");
        store.set_budget_cap(10).expect("set global cap");
        store
            .charge_budget("agent_a", 4)
            .expect("charge at cap minus one");
        let before_refusal = store.budget_use().expect("ledger before refusal");
        let events_before_refusal = store.events("budget").expect("budget events");
        assert_eq!(before_refusal.total(), 9);
        assert!(matches!(
            store.charge_budget("agent_b", 2),
            Err(BudgetError::Exhausted)
        ));
        assert_eq!(
            store.budget_use().expect("ledger after refusal"),
            before_refusal
        );
        assert_eq!(
            store.events("budget").expect("budget events after refusal"),
            events_before_refusal
        );

        store.reset_budget("agent_a").expect("reset one owner");
        let reset_owner = store.budget_use().expect("ledger after owner reset");
        assert_eq!(reset_owner.total(), 0);
        assert_eq!(reset_owner.cap, 10);
        store
            .charge_budget("agent_b", 1 << 20)
            .expect_err("cap still enforced until explicitly disabled");
        store.set_budget_cap(0).expect("disable cap");
        store
            .charge_budget("agent_b", 1 << 20)
            .expect("uncapped charge accepted");
        store.reset_budget("*").expect("reset every owner");
        let all_reset = store.budget_use().expect("ledger after full reset");
        assert_eq!(all_reset.total(), 0);
        assert!(all_reset.usage.is_empty());
        assert!(all_reset.version > reset_owner.version);
    }

    #[test]
    fn invalid_requests_and_corrupt_ledgers_are_rejected() {
        let store = store();
        for owner in ["", "bad/owner", "sp ace", "semi;colon"] {
            assert!(matches!(
                store.charge_budget(owner, 1),
                Err(BudgetError::InvalidRequest)
            ));
        }
        assert!(matches!(
            store.charge_budget("agent_a", -1),
            Err(BudgetError::InvalidRequest)
        ));
        assert!(matches!(
            store.set_budget_cap(-5),
            Err(BudgetError::InvalidRequest)
        ));
        assert!(matches!(
            store.reset_budget("bad/owner"),
            Err(BudgetError::InvalidRequest)
        ));

        store
            .put(
                "settings",
                "budget",
                &serde_json::json!({"cap": 0, "version": 0, "use": {"agent_a": -1}}),
                "",
            )
            .expect("write invalid ledger fixture");
        assert!(matches!(
            store.budget_use(),
            Err(BudgetError::InvalidLedger)
        ));

        for legacy in [
            serde_json::json!({"cap": 3}),
            serde_json::json!({"cap": 3, "use": null}),
            serde_json::json!({"cap": null, "version": null, "use": {"agent_a": null}}),
        ] {
            store
                .put("settings", "budget", &legacy, "")
                .expect("write legacy compatible ledger");
            let decoded = store.budget_use().expect("decode omitted or null use map");
            if legacy.get("cap").is_some_and(serde_json::Value::is_null) {
                assert_eq!(decoded.cap, 0);
                assert_eq!(decoded.usage.get("agent_a"), Some(&0));
            } else {
                assert_eq!(decoded.cap, 3);
            }
            assert_eq!(decoded.version, 0);
            if !legacy.get("use").is_some_and(serde_json::Value::is_object) {
                assert!(decoded.usage.is_empty());
            }
        }
    }

    #[test]
    fn independent_connections_cannot_overcharge_the_shared_cap() {
        let database = TestDatabase::new();
        let first = Arc::new(database.open());
        first.set_budget_cap(10).expect("set shared cap");
        let second = Arc::new(database.open());
        let barrier = Arc::new(Barrier::new(3));
        let workers = [
            (Arc::clone(&first), "agent_a"),
            (Arc::clone(&second), "agent_b"),
        ]
        .into_iter()
        .map(|(store, owner)| {
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                store.charge_budget(owner, 7)
            })
        })
        .collect::<Vec<_>>();
        barrier.wait();

        let results = workers
            .into_iter()
            .map(|worker| worker.join().expect("budget worker did not panic"))
            .collect::<Vec<_>>();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Err(BudgetError::Exhausted)))
                .count(),
            1
        );
        assert_eq!(first.budget_use().expect("read final ledger").total(), 7);
    }
}
