//! Persistent coordination reservations for agents and named resources.
//!
//! These rows coordinate Rover-owned work. They are not operating-system locks,
//! port bindings, database access controls, or a mechanism for safely taking
//! over an owner that may still be running.

use std::error::Error;
use std::fmt;

use rover_core::{now, Id};
use rusqlite::TransactionBehavior;
use serde::de::Deserializer;
use serde::{Deserialize, Serialize};

use crate::{put_in_transaction, Store, StoreError};

const LEASE_KIND: &str = "lease";
const DEFAULT_CAPACITY: i64 = 4;
const MAX_CAPACITY: i64 = 128;
const MAX_RESOURCE_BYTES: usize = 96;

fn deserialize_max_agents<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<i64>::deserialize(deserializer)?.unwrap_or_default())
}

/// A persistent reservation for one resource slot or named resource.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Lease {
    /// Task or workflow that owns this reservation.
    pub owner: String,
    /// Stable record key for the slot or named resource.
    pub resource: String,
    /// Timestamp at which this reservation was last acquired.
    pub acquired_at: String,
}

/// A resource reservation failure.
#[derive(Debug)]
pub enum ResourceError {
    /// Owner, capacity, or resource names violate the Go storage contract.
    InvalidRequest,
    /// Another owner holds the agent slot or one of the named resources.
    Busy,
    /// The backing store failed.
    Store(StoreError),
}

impl fmt::Display for ResourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest => formatter.write_str("invalid resource request"),
            Self::Busy => formatter.write_str("resources busy"),
            Self::Store(error) => write!(formatter, "resource store failed: {error}"),
        }
    }
}

impl Error for ResourceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Store(error) => Some(error),
            Self::InvalidRequest | Self::Busy => None,
        }
    }
}

impl Store {
    /// Atomically reserve an agent slot and all named resources for one owner.
    ///
    /// A repeated acquisition by the same owner is idempotent and refreshes
    /// acquisition timestamps. No lease expires automatically: Rover must not
    /// transfer a reservation while the original process could still run.
    ///
    /// # Errors
    ///
    /// Returns [`ResourceError::InvalidRequest`] for invalid owners, resource
    /// names, or capacities outside 1..=128; [`ResourceError::Busy`] if the
    /// capacity or a named resource is held by another owner; or
    /// [`ResourceError::Store`] for database failures.
    pub fn acquire_resources(
        &self,
        owner: &str,
        resources: &[String],
        capacity: i64,
    ) -> Result<(), ResourceError> {
        validate_request(owner, resources, capacity)?;

        let mut connection = self.lock().map_err(ResourceError::Store)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(StoreError::from)
            .map_err(ResourceError::Store)?;

        let mut statement = transaction
            .prepare("SELECT id,payload FROM records WHERE kind='lease'")
            .map_err(StoreError::from)
            .map_err(ResourceError::Store)?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(StoreError::from)
            .map_err(ResourceError::Store)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
            .map_err(ResourceError::Store)?;
        drop(statement);

        let mut owners = std::collections::HashSet::new();
        let mut occupied = std::collections::HashMap::new();
        for (id, payload) in rows {
            let lease: Lease = serde_json::from_str(&payload)
                .map_err(StoreError::from)
                .map_err(ResourceError::Store)?;
            owners.insert(lease.owner.clone());
            occupied.insert(id, lease.owner);
        }

        if !owners.contains(owner) && i64::try_from(owners.len()).unwrap_or(i64::MAX) >= capacity {
            return Err(ResourceError::Busy);
        }

        let mut keys = Vec::with_capacity(resources.len() + 1);
        keys.push(format!("slot.{owner}"));
        keys.extend(
            resources
                .iter()
                .map(|resource| format!("resource.{resource}")),
        );
        if keys
            .iter()
            .any(|key| occupied.get(key).is_some_and(|held_by| held_by != owner))
        {
            return Err(ResourceError::Busy);
        }

        for key in keys {
            let acquired_at = now()
                .to_rfc3339()
                .map_err(|error| StoreError::Timestamp(error.to_string()))
                .map_err(ResourceError::Store)?;
            let lease = Lease {
                owner: owner.to_owned(),
                resource: key.clone(),
                acquired_at,
            };
            put_in_transaction(&transaction, LEASE_KIND, &key, &lease, "")
                .map_err(ResourceError::Store)?;
        }

        transaction
            .commit()
            .map_err(StoreError::from)
            .map_err(ResourceError::Store)
    }

    /// Release every slot and named resource owned by exactly `owner`.
    ///
    /// The delete is transactional and cannot release another owner's rows.
    /// It intentionally does not infer that an owner is dead or perform
    /// timeout-based takeover.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] if the database operation fails.
    pub fn release_resources(&self, owner: &str) -> Result<(), StoreError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut statement =
            transaction.prepare("SELECT id,payload FROM records WHERE kind='lease'")?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        drop(statement);

        for (id, payload) in rows {
            let lease: Lease = serde_json::from_str(&payload)?;
            if lease.owner == owner {
                transaction.execute(
                    "DELETE FROM records WHERE kind=?1 AND id=?2",
                    (LEASE_KIND, id),
                )?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    /// Read the configured agent capacity, using Rover's default of four.
    ///
    /// # Errors
    ///
    /// Returns [`ResourceError::InvalidRequest`] if a stored capacity is
    /// outside 1..=128, or [`ResourceError::Store`] for malformed settings or
    /// database failures.
    pub fn agent_capacity(&self) -> Result<i64, ResourceError> {
        #[derive(Deserialize)]
        struct RuntimeSettings {
            #[serde(default, deserialize_with = "deserialize_max_agents")]
            max_agents: i64,
        }

        match self.get_typed::<RuntimeSettings>("settings", "runtime") {
            Ok(settings) if (1..=MAX_CAPACITY).contains(&settings.max_agents) => {
                Ok(settings.max_agents)
            }
            Ok(_) => Err(ResourceError::InvalidRequest),
            Err(StoreError::NotFound) => Ok(DEFAULT_CAPACITY),
            Err(error) => Err(ResourceError::Store(error)),
        }
    }
}

fn validate_request(owner: &str, resources: &[String], capacity: i64) -> Result<(), ResourceError> {
    if Id::parse(owner).is_err() || !(1..=MAX_CAPACITY).contains(&capacity) {
        return Err(ResourceError::InvalidRequest);
    }
    for resource in resources {
        if Id::parse(resource.as_str()).is_err() || resource.len() > MAX_RESOURCE_BYTES {
            return Err(ResourceError::InvalidRequest);
        }
    }
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

    use super::{Lease, ResourceError};
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
                "rover-reservations-{}-{timestamp}-{}",
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
    fn named_resources_capacity_and_owner_cleanup_match_go_contract() {
        let store = store();
        let shared = vec!["database".to_owned(), "port-8080".to_owned()];
        store
            .acquire_resources("owner_a", &shared, 2)
            .expect("first owner reserves resources");
        assert!(matches!(
            store.acquire_resources("owner_b", &["database".to_owned()], 2),
            Err(ResourceError::Busy)
        ));
        store
            .acquire_resources("owner_a", &["database".to_owned()], 2)
            .expect("owner can re-acquire its own resource");
        store
            .acquire_resources("owner_b", &["other".to_owned()], 2)
            .expect("second owner uses a distinct resource");
        assert!(matches!(
            store.acquire_resources("owner_c", &[], 2),
            Err(ResourceError::Busy)
        ));

        store
            .release_resources("owner_a")
            .expect("owner releases its own reservations");
        let remaining = store
            .list_all_raw("lease", 10)
            .expect("read lease rows")
            .iter()
            .map(|row| serde_json::from_str::<Lease>(row).expect("decode lease"))
            .collect::<Vec<_>>();
        assert_eq!(remaining.len(), 2);
        assert!(remaining.iter().all(|lease| lease.owner == "owner_b"));
        store
            .acquire_resources("owner_c", &["database".to_owned()], 2)
            .expect("released resource can be acquired");
    }

    #[test]
    fn request_validation_and_capacity_defaults_match_go_contract() {
        let store = store();
        assert_eq!(store.agent_capacity().expect("default capacity"), 4);
        for (owner, resources, capacity) in [
            ("", vec![], 4),
            ("owner", vec!["bad/resource".to_owned()], 4),
            ("owner", vec!["x".repeat(97)], 4),
            ("owner", vec![], 0),
            ("owner", vec![], 129),
        ] {
            assert!(matches!(
                store.acquire_resources(owner, &resources, capacity),
                Err(ResourceError::InvalidRequest)
            ));
        }

        store
            .put(
                "settings",
                "runtime",
                &serde_json::json!({"max_agents": 7}),
                "",
            )
            .expect("configure capacity");
        assert_eq!(store.agent_capacity().expect("configured capacity"), 7);
        store
            .put(
                "settings",
                "runtime",
                &serde_json::json!({"max_agents": 0}),
                "",
            )
            .expect("configure invalid capacity");
        assert!(matches!(
            store.agent_capacity(),
            Err(ResourceError::InvalidRequest)
        ));
        for missing_or_null in [
            serde_json::json!({}),
            serde_json::json!({"max_agents": null}),
        ] {
            store
                .put("settings", "runtime", &missing_or_null, "")
                .expect("configure absent capacity");
            assert!(matches!(
                store.agent_capacity(),
                Err(ResourceError::InvalidRequest)
            ));
        }
    }

    #[test]
    fn separate_connections_cannot_acquire_the_same_resource() {
        let database = TestDatabase::new();
        let first = Arc::new(database.open());
        let second = Arc::new(database.open());
        let barrier = Arc::new(Barrier::new(3));
        let workers = [
            (Arc::clone(&first), "owner_a"),
            (Arc::clone(&second), "owner_b"),
        ]
        .into_iter()
        .map(|(store, owner)| {
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                store.acquire_resources(owner, &["port-8080".to_owned()], 4)
            })
        })
        .collect::<Vec<_>>();
        barrier.wait();

        let results = workers
            .into_iter()
            .map(|worker| worker.join().expect("reservation worker did not panic"))
            .collect::<Vec<_>>();
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Err(ResourceError::Busy)))
                .count(),
            1
        );
    }
}
