//! Project-bound durable task plans, separate from executable task runs.
//!
//! Plans preserve user intent and dependencies. They are not dispatched and
//! do not claim execution, agent selection, or quality-gate results.

use rover_core::{
    validate_task_graph, Id, RepositoryIdentity, Sha256Digest, TaskBrief, TaskGraphError,
    TaskGraphNode,
};
use rover_store::{RecordValue, Store, StoreError};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

const RECORD_KIND: &str = "task_plan";
const RECORD_SCHEMA: &str = "rover/task-plan/v1";
const MAX_TASK_PLANS: i64 = 10_000;
const MAX_NOTE_BYTES: usize = 4096;
const MAX_TASK_ATTEMPTS: usize = 10;

/// Durable task-plan lifecycle. A ready plan still needs an explicit start.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskPlanStatus {
    Ready,
    Waiting,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

impl TaskPlanStatus {
    /// Stable serialized status spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "READY",
            Self::Waiting => "WAITING",
            Self::Running => "RUNNING",
            Self::Succeeded => "SUCCEEDED",
            Self::Failed => "FAILED",
            Self::Cancelled => "CANCELLED",
        }
    }
}

/// Attempt manually recorded through the plan service.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TaskPlanAttempt {
    pub number: u32,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub outcome: Option<String>,
    pub note: Option<String>,
    pub authority: String,
}

/// Durable intent bound to one canonical project path and digest.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TaskPlan {
    pub schema: String,
    pub id: Id,
    pub project_id: String,
    pub repository: String,
    pub brief: TaskBrief,
    pub dependencies: Vec<Id>,
    pub status: TaskPlanStatus,
    pub attempts: Vec<TaskPlanAttempt>,
    pub created_at: String,
    pub updated_at: String,
}

/// Derived state of a plan's prerequisites.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskReadiness {
    Ready,
    Waiting,
    Blocked,
}

/// Task-plan service error.
#[derive(Debug)]
pub enum TaskPlanError {
    Store(StoreError),
    Brief(rover_core::TaskBriefError),
    Graph(TaskGraphError),
    InvalidRecord,
    InvalidProjectPath,
    MissingDependency(Id),
    DependencyFailed(Id),
    NotFound,
    ProjectMismatch,
    InvalidTransition,
    InvalidNote,
}

impl fmt::Display for TaskPlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "task-plan storage failed: {error}"),
            Self::Brief(error) => write!(formatter, "invalid task brief: {error}"),
            Self::Graph(error) => write!(formatter, "invalid task graph: {error}"),
            Self::InvalidRecord => formatter.write_str("stored task plan is invalid"),
            Self::InvalidProjectPath => formatter.write_str("repository path is not valid UTF-8"),
            Self::MissingDependency(id) => write!(
                formatter,
                "task dependency {id} is missing from this project"
            ),
            Self::DependencyFailed(id) => write!(formatter, "task dependency {id} did not succeed"),
            Self::NotFound => formatter.write_str("task plan was not found in this project"),
            Self::ProjectMismatch => formatter.write_str("task plan belongs to another project"),
            Self::InvalidTransition => formatter.write_str("task-plan transition is not allowed"),
            Self::InvalidNote => formatter.write_str("task note is empty, oversized, or unsafe"),
        }
    }
}

impl std::error::Error for TaskPlanError {}

impl From<StoreError> for TaskPlanError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<rover_core::TaskBriefError> for TaskPlanError {
    fn from(error: rover_core::TaskBriefError) -> Self {
        Self::Brief(error)
    }
}

impl From<TaskGraphError> for TaskPlanError {
    fn from(error: TaskGraphError) -> Self {
        Self::Graph(error)
    }
}

/// Service for creating and manually tracking project-bound task plans.
pub struct TaskPlanService<'a> {
    store: &'a Store,
}

impl<'a> TaskPlanService<'a> {
    /// Bind the service to an already-open Rover store.
    #[must_use]
    pub const fn new(store: &'a Store) -> Self {
        Self { store }
    }

    /// Validate and durably create a plan. Dependencies must already exist in
    /// the same project. This operation never dispatches a process or agent.
    ///
    /// # Errors
    ///
    /// Returns an error for invalid fields, missing dependencies, malformed
    /// stored task plans, unsupported repository paths, or store failures.
    pub fn create(
        &self,
        project: &RepositoryIdentity,
        brief: TaskBrief,
    ) -> Result<TaskPlan, TaskPlanError> {
        brief.validate()?;
        let repository = project
            .path()
            .to_str()
            .ok_or(TaskPlanError::InvalidProjectPath)?
            .to_owned();
        let project_id = project.project_id().to_hex();
        let current = self.list(project)?;
        let known = current
            .iter()
            .map(|plan| (plan.id.clone(), plan))
            .collect::<BTreeMap<_, _>>();
        let dependencies = parse_dependencies(&brief)?;
        for dependency in &dependencies {
            if !known.contains_key(dependency) {
                return Err(TaskPlanError::MissingDependency(dependency.clone()));
            }
        }

        let id = Id::generate("plan").map_err(|_| TaskPlanError::InvalidRecord)?;
        let graph = current
            .iter()
            .map(|plan| TaskGraphNode {
                id: plan.id.clone(),
                dependencies: plan.dependencies.clone(),
            })
            .chain(std::iter::once(TaskGraphNode {
                id: id.clone(),
                dependencies: dependencies.clone(),
            }))
            .collect::<Vec<_>>();
        validate_task_graph(&graph)?;

        let initial_readiness =
            dependencies
                .iter()
                .try_fold(TaskReadiness::Ready, |state, dependency| {
                    let prerequisite = known
                        .get(dependency)
                        .ok_or_else(|| TaskPlanError::MissingDependency(dependency.clone()))?;
                    match prerequisite.status {
                        TaskPlanStatus::Succeeded => Ok(state),
                        TaskPlanStatus::Failed | TaskPlanStatus::Cancelled => {
                            Err(TaskPlanError::DependencyFailed(dependency.clone()))
                        }
                        _ => Ok(TaskReadiness::Waiting),
                    }
                })?;
        let now = rover_core::now().to_string();
        let plan = TaskPlan {
            schema: RECORD_SCHEMA.to_owned(),
            id: id.clone(),
            project_id: project_id.clone(),
            repository,
            brief,
            dependencies,
            status: match initial_readiness {
                TaskReadiness::Ready => TaskPlanStatus::Ready,
                _ => TaskPlanStatus::Waiting,
            },
            attempts: Vec::new(),
            created_at: now.clone(),
            updated_at: now,
        };
        let digest =
            Sha256Digest::of(&serde_json::to_vec(&plan).map_err(|_| TaskPlanError::InvalidRecord)?)
                .to_hex();
        let request_key = format!("task-plan:create:{project_id}:{id}");
        self.store
            .create_once(RECORD_KIND, id.as_str(), &request_key, &digest, &plan)?;
        Ok(plan)
    }

    /// List this project's plans. Corrupt records fail closed; results are
    /// stable by creation time and then task ID.
    ///
    /// # Errors
    ///
    /// Returns an error on store access or malformed records.
    pub fn list(&self, project: &RepositoryIdentity) -> Result<Vec<TaskPlan>, TaskPlanError> {
        let repository = project
            .path()
            .to_str()
            .ok_or(TaskPlanError::InvalidProjectPath)?;
        let project_id = project.project_id().to_hex();
        let records = self.store.list_all(RECORD_KIND, MAX_TASK_PLANS)?;
        let mut plans = records
            .iter()
            .map(decode_plan)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|plan| plan.project_id == project_id && plan.repository == repository)
            .collect::<Vec<_>>();
        validate_task_graph(
            &plans
                .iter()
                .map(|plan| TaskGraphNode {
                    id: plan.id.clone(),
                    dependencies: plan.dependencies.clone(),
                })
                .collect::<Vec<_>>(),
        )?;
        plans.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(plans)
    }

    /// List plans with readiness derived from one consistent in-memory
    /// snapshot of the returned project plans.
    ///
    /// # Errors
    ///
    /// Returns an error on malformed task graphs or store access failure.
    pub fn list_with_readiness(
        &self,
        project: &RepositoryIdentity,
    ) -> Result<Vec<(TaskPlan, TaskReadiness)>, TaskPlanError> {
        let plans = self.list(project)?;
        let readiness = readiness_snapshot(&plans)?;
        plans
            .iter()
            .map(|plan| {
                readiness
                    .get(&plan.id)
                    .copied()
                    .map(|state| (plan.clone(), state))
                    .ok_or(TaskPlanError::InvalidRecord)
            })
            .collect()
    }

    /// Load a plan only when both its canonical path and project digest match.
    ///
    /// # Errors
    ///
    /// Returns `NotFound`, `ProjectMismatch`, or an error for corrupt storage.
    pub fn get(&self, project: &RepositoryIdentity, id: &Id) -> Result<TaskPlan, TaskPlanError> {
        let plan: TaskPlan = self
            .store
            .get_typed(RECORD_KIND, id.as_str())
            .map_err(|error| {
                if matches!(error, StoreError::NotFound) {
                    TaskPlanError::NotFound
                } else {
                    TaskPlanError::Store(error)
                }
            })?;
        validate_record(&plan)?;
        if project.path().to_str() != Some(plan.repository.as_str())
            || project.project_id().to_hex() != plan.project_id
        {
            return Err(TaskPlanError::ProjectMismatch);
        }
        Ok(plan)
    }

    /// Compute dependency readiness from current durable plan states.
    ///
    /// # Errors
    ///
    /// Returns an error when the plan or a prerequisite is unavailable.
    pub fn readiness(
        &self,
        project: &RepositoryIdentity,
        id: &Id,
    ) -> Result<TaskReadiness, TaskPlanError> {
        let plans = self.list(project)?;
        readiness_snapshot(&plans)?
            .get(id)
            .copied()
            .ok_or(TaskPlanError::NotFound)
    }

    /// Start a ready plan as a manually tracked attempt; no process is started.
    ///
    /// # Errors
    ///
    /// Returns an error if the project, status, dependency state, or store is invalid.
    pub fn start_manual(
        &self,
        project: &RepositoryIdentity,
        id: &Id,
    ) -> Result<TaskPlan, TaskPlanError> {
        let current = self.get(project, id)?;
        if !matches!(
            current.status,
            TaskPlanStatus::Ready | TaskPlanStatus::Waiting
        ) || current.attempts.len() >= MAX_TASK_ATTEMPTS
            || self.readiness(project, id)? != TaskReadiness::Ready
        {
            return Err(TaskPlanError::InvalidTransition);
        }
        self.store.mutate(
            RECORD_KIND,
            id.as_str(),
            "task_plan.started_manual",
            |value| {
                let mut plan: TaskPlan = serde_json::from_value(value.clone())
                    .map_err(|_| StoreError::MutationRejected("invalid task plan".into()))?;
                if !matches!(plan.status, TaskPlanStatus::Ready | TaskPlanStatus::Waiting) {
                    return Err(StoreError::MutationRejected(
                        "task plan is not startable".into(),
                    ));
                }
                let now = rover_core::now().to_string();
                let number = u32::try_from(plan.attempts.len() + 1)
                    .map_err(|_| StoreError::MutationRejected("attempt count overflow".into()))?;
                plan.attempts.push(TaskPlanAttempt {
                    number,
                    started_at: now.clone(),
                    finished_at: None,
                    outcome: None,
                    note: None,
                    authority: "manual".into(),
                });
                plan.status = TaskPlanStatus::Running;
                plan.updated_at = now;
                serde_json::to_value(plan)
                    .map_err(|_| StoreError::MutationRejected("task plan encoding failed".into()))
            },
        )?;
        self.get(project, id)
    }

    /// Return a failed task to ready after its dependencies have succeeded.
    ///
    /// # Errors
    ///
    /// Returns an error unless the task is failed and every prerequisite succeeded.
    pub fn retry_manual(
        &self,
        project: &RepositoryIdentity,
        id: &Id,
    ) -> Result<TaskPlan, TaskPlanError> {
        let current = self.get(project, id)?;
        if current.status != TaskPlanStatus::Failed
            || current.attempts.len() >= MAX_TASK_ATTEMPTS
            || self.dependencies_readiness(project, &current)? != TaskReadiness::Ready
        {
            return Err(TaskPlanError::InvalidTransition);
        }
        self.store.mutate(
            RECORD_KIND,
            id.as_str(),
            "task_plan.retried_manual",
            |value| {
                let mut plan: TaskPlan = serde_json::from_value(value.clone())
                    .map_err(|_| StoreError::MutationRejected("invalid task plan".into()))?;
                if plan.status != TaskPlanStatus::Failed {
                    return Err(StoreError::MutationRejected(
                        "task plan is not failed".into(),
                    ));
                }
                plan.status = TaskPlanStatus::Ready;
                plan.updated_at = rover_core::now().to_string();
                serde_json::to_value(plan)
                    .map_err(|_| StoreError::MutationRejected("task plan encoding failed".into()))
            },
        )?;
        self.get(project, id)
    }

    /// Record a manual outcome. The note is retained as a user assertion and
    /// is never presented as process or quality-gate evidence.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid outcome, note, lifecycle state, or store.
    pub fn finish_manual(
        &self,
        project: &RepositoryIdentity,
        id: &Id,
        outcome: TaskPlanStatus,
        note: &str,
    ) -> Result<TaskPlan, TaskPlanError> {
        if !matches!(
            outcome,
            TaskPlanStatus::Succeeded | TaskPlanStatus::Failed | TaskPlanStatus::Cancelled
        ) || note.trim().is_empty()
            || note.len() > MAX_NOTE_BYTES
            || note
                .chars()
                .any(|ch| ch.is_control() && ch != '\n' || is_bidi_format(ch))
        {
            return Err(TaskPlanError::InvalidNote);
        }
        self.get(project, id)?;
        self.store.mutate(
            RECORD_KIND,
            id.as_str(),
            "task_plan.finished_manual",
            |value| {
                let mut plan: TaskPlan = serde_json::from_value(value.clone())
                    .map_err(|_| StoreError::MutationRejected("invalid task plan".into()))?;
                if plan.status != TaskPlanStatus::Running {
                    return Err(StoreError::MutationRejected(
                        "task plan is not running".into(),
                    ));
                }
                let attempt = plan.attempts.last_mut().ok_or_else(|| {
                    StoreError::MutationRejected("running plan has no attempt".into())
                })?;
                if attempt.finished_at.is_some() {
                    return Err(StoreError::MutationRejected(
                        "attempt already finished".into(),
                    ));
                }
                let now = rover_core::now().to_string();
                attempt.finished_at = Some(now.clone());
                attempt.outcome = Some(status_name(outcome).to_owned());
                attempt.note = Some(note.to_owned());
                plan.status = outcome;
                plan.updated_at = now;
                serde_json::to_value(plan)
                    .map_err(|_| StoreError::MutationRejected("task plan encoding failed".into()))
            },
        )?;
        self.get(project, id)
    }

    fn dependencies_readiness(
        &self,
        project: &RepositoryIdentity,
        plan: &TaskPlan,
    ) -> Result<TaskReadiness, TaskPlanError> {
        let plans = self.list(project)?;
        let by_id = plans
            .iter()
            .map(|candidate| (candidate.id.clone(), candidate))
            .collect::<BTreeMap<_, _>>();
        let readiness = readiness_snapshot(&plans)?;
        dependency_readiness(plan, &by_id, &readiness)
    }
}

fn parse_dependencies(brief: &TaskBrief) -> Result<Vec<Id>, TaskPlanError> {
    brief
        .dependencies
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| Id::parse(line.to_owned()).map_err(|_| TaskPlanError::InvalidRecord))
        .collect()
}

fn decode_plan(value: &RecordValue) -> Result<TaskPlan, TaskPlanError> {
    let plan = serde_json::from_value(value.as_value().clone())
        .map_err(|_| TaskPlanError::InvalidRecord)?;
    validate_record(&plan)?;
    Ok(plan)
}

fn validate_record(plan: &TaskPlan) -> Result<(), TaskPlanError> {
    if plan.schema != RECORD_SCHEMA
        || plan.brief.validate().is_err()
        || plan.attempts.len() > MAX_TASK_ATTEMPTS
    {
        return Err(TaskPlanError::InvalidRecord);
    }
    let project_id =
        Sha256Digest::parse_hex(&plan.project_id).map_err(|_| TaskPlanError::InvalidRecord)?;
    let repository =
        RepositoryIdentity::resolve(&plan.repository).map_err(|_| TaskPlanError::InvalidRecord)?;
    if project_id.to_hex() != plan.project_id
        || repository.project_id() != project_id
        || repository.path().to_str() != Some(plan.repository.as_str())
    {
        return Err(TaskPlanError::InvalidRecord);
    }
    if parse_dependencies(&plan.brief)? != plan.dependencies {
        return Err(TaskPlanError::InvalidRecord);
    }
    let mut unfinished_attempts = 0;
    for (index, attempt) in plan.attempts.iter().enumerate() {
        if usize::try_from(attempt.number).ok() != Some(index + 1)
            || attempt.authority != "manual"
            || attempt.note.as_ref().is_some_and(|note| {
                note.is_empty()
                    || note.len() > MAX_NOTE_BYTES
                    || note
                        .chars()
                        .any(|ch| (ch.is_control() && ch != '\n') || is_bidi_format(ch))
            })
            || attempt
                .outcome
                .as_deref()
                .is_some_and(|outcome| !matches!(outcome, "SUCCEEDED" | "FAILED" | "CANCELLED"))
            || attempt.finished_at.is_some() != attempt.outcome.is_some()
            || attempt.finished_at.is_some() != attempt.note.is_some()
        {
            return Err(TaskPlanError::InvalidRecord);
        }
        if attempt.finished_at.is_none() {
            unfinished_attempts += 1;
            if index + 1 != plan.attempts.len() {
                return Err(TaskPlanError::InvalidRecord);
            }
        }
    }
    match plan.status {
        TaskPlanStatus::Running if unfinished_attempts == 1 => {}
        TaskPlanStatus::Running => return Err(TaskPlanError::InvalidRecord),
        TaskPlanStatus::Succeeded | TaskPlanStatus::Failed | TaskPlanStatus::Cancelled => {
            if unfinished_attempts != 0
                || plan
                    .attempts
                    .last()
                    .and_then(|attempt| attempt.outcome.as_deref())
                    != Some(status_name(plan.status))
            {
                return Err(TaskPlanError::InvalidRecord);
            }
        }
        TaskPlanStatus::Ready | TaskPlanStatus::Waiting if unfinished_attempts != 0 => {
            return Err(TaskPlanError::InvalidRecord)
        }
        _ => {}
    }
    Ok(())
}

fn readiness_snapshot(plans: &[TaskPlan]) -> Result<BTreeMap<Id, TaskReadiness>, TaskPlanError> {
    let by_id = plans
        .iter()
        .map(|plan| (plan.id.clone(), plan))
        .collect::<BTreeMap<_, _>>();
    let graph = plans
        .iter()
        .map(|plan| TaskGraphNode {
            id: plan.id.clone(),
            dependencies: plan.dependencies.clone(),
        })
        .collect::<Vec<_>>();
    let mut readiness = BTreeMap::new();
    for id in validate_task_graph(&graph)? {
        let plan = by_id.get(&id).ok_or(TaskPlanError::InvalidRecord)?;
        let state = if matches!(
            plan.status,
            TaskPlanStatus::Succeeded | TaskPlanStatus::Failed | TaskPlanStatus::Cancelled
        ) {
            TaskReadiness::Blocked
        } else {
            dependency_readiness(plan, &by_id, &readiness)?
        };
        readiness.insert(id, state);
    }
    Ok(readiness)
}

fn dependency_readiness(
    plan: &TaskPlan,
    by_id: &BTreeMap<Id, &TaskPlan>,
    readiness: &BTreeMap<Id, TaskReadiness>,
) -> Result<TaskReadiness, TaskPlanError> {
    let mut result = TaskReadiness::Ready;
    for id in &plan.dependencies {
        let dependency = by_id.get(id).ok_or_else(|| {
            TaskPlanError::Graph(TaskGraphError::MissingDependency {
                task: plan.id.clone(),
                prerequisite: id.clone(),
            })
        })?;
        match dependency.status {
            TaskPlanStatus::Succeeded => {}
            TaskPlanStatus::Failed | TaskPlanStatus::Cancelled => {
                return Ok(TaskReadiness::Blocked)
            }
            _ if readiness.get(id) == Some(&TaskReadiness::Blocked) => {
                return Ok(TaskReadiness::Blocked)
            }
            _ => result = TaskReadiness::Waiting,
        }
    }
    Ok(result)
}

fn status_name(status: TaskPlanStatus) -> &'static str {
    status.as_str()
}

fn is_bidi_format(ch: char) -> bool {
    matches!(ch, '\u{061c}' | '\u{200e}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{206a}'..='\u{206f}')
}

#[cfg(test)]
mod tests {
    use super::{TaskPlanError, TaskPlanService, TaskPlanStatus, TaskReadiness, MAX_TASK_ATTEMPTS};
    use rover_core::{RepositoryIdentity, TaskBrief};
    use rover_store::Store;
    use rusqlite::Connection;

    fn store() -> Store {
        Store::from_connection(Connection::open_in_memory().expect("open memory SQLite"))
            .expect("store")
    }
    fn brief(title: &str, dependencies: &str) -> TaskBrief {
        TaskBrief {
            title: title.into(),
            paths: "src/**".into(),
            dependencies: dependencies.into(),
            quality_gate: "manual review".into(),
            prompt: format!("Implement {title}"),
        }
    }

    #[test]
    fn creates_project_bound_plans_and_derives_readiness() {
        let store = store();
        let service = TaskPlanService::new(&store);
        let project = RepositoryIdentity::resolve("/tmp/rover-plan-a").unwrap();
        let first = service.create(&project, brief("First", "")).unwrap();
        let second = service
            .create(&project, brief("Second", first.id.as_str()))
            .unwrap();
        assert_eq!(first.status, TaskPlanStatus::Ready);
        assert_eq!(second.status, TaskPlanStatus::Waiting);
        assert_eq!(
            service.readiness(&project, &second.id).unwrap(),
            TaskReadiness::Waiting
        );
        assert_eq!(service.list(&project).unwrap().len(), 2);
        let other = RepositoryIdentity::resolve("/tmp/rover-plan-b").unwrap();
        assert!(service.list(&other).unwrap().is_empty());
        assert!(matches!(
            service.get(&other, &first.id),
            Err(TaskPlanError::ProjectMismatch)
        ));
    }

    #[test]
    fn manual_lifecycle_records_attempt_history_and_retry() {
        let store = store();
        let service = TaskPlanService::new(&store);
        let project = RepositoryIdentity::resolve("/tmp/rover-plan").unwrap();
        let plan = service.create(&project, brief("Review", "")).unwrap();
        assert_eq!(
            service.start_manual(&project, &plan.id).unwrap().status,
            TaskPlanStatus::Running
        );
        service
            .finish_manual(
                &project,
                &plan.id,
                TaskPlanStatus::Failed,
                "Needs another pass",
            )
            .unwrap();
        service.retry_manual(&project, &plan.id).unwrap();
        assert_eq!(
            service
                .start_manual(&project, &plan.id)
                .unwrap()
                .attempts
                .len(),
            2
        );
        let finished = service
            .finish_manual(
                &project,
                &plan.id,
                TaskPlanStatus::Succeeded,
                "Reviewed manually",
            )
            .unwrap();
        assert_eq!(finished.status, TaskPlanStatus::Succeeded);
        assert_eq!(finished.attempts.len(), 2);
        assert!(service.start_manual(&project, &plan.id).is_err());
    }

    #[test]
    fn dependencies_gate_start_and_missing_ids_fail() {
        let store = store();
        let service = TaskPlanService::new(&store);
        let project = RepositoryIdentity::resolve("/tmp/rover-plan").unwrap();
        assert!(matches!(
            service.create(&project, brief("Child", "plan_missing")),
            Err(TaskPlanError::MissingDependency(_))
        ));
        let parent = service.create(&project, brief("Parent", "")).unwrap();
        let child = service
            .create(&project, brief("Child", parent.id.as_str()))
            .unwrap();
        let grandchild = service
            .create(&project, brief("Grandchild", child.id.as_str()))
            .unwrap();
        assert!(matches!(
            service.start_manual(&project, &child.id),
            Err(TaskPlanError::InvalidTransition)
        ));
        service.start_manual(&project, &parent.id).unwrap();
        service
            .finish_manual(
                &project,
                &parent.id,
                TaskPlanStatus::Succeeded,
                "Approved manually",
            )
            .unwrap();
        assert_eq!(
            service.readiness(&project, &child.id).unwrap(),
            TaskReadiness::Ready
        );
        assert_eq!(
            service.readiness(&project, &grandchild.id).unwrap(),
            TaskReadiness::Waiting
        );
        assert_eq!(
            service.start_manual(&project, &child.id).unwrap().status,
            TaskPlanStatus::Running
        );
    }

    #[test]
    fn cancelled_or_failed_prerequisites_block_children_and_project_checks_hold() {
        let store = store();
        let service = TaskPlanService::new(&store);
        let project = RepositoryIdentity::resolve("/tmp/rover-plan").unwrap();
        let parent = service.create(&project, brief("Parent", "")).unwrap();
        let child = service
            .create(&project, brief("Child", parent.id.as_str()))
            .unwrap();
        let grandchild = service
            .create(&project, brief("Grandchild", child.id.as_str()))
            .unwrap();
        service.start_manual(&project, &parent.id).unwrap();
        service
            .finish_manual(
                &project,
                &parent.id,
                TaskPlanStatus::Cancelled,
                "Cancelled manually",
            )
            .unwrap();
        assert_eq!(
            service.readiness(&project, &child.id).unwrap(),
            TaskReadiness::Blocked
        );
        assert_eq!(
            service.readiness(&project, &grandchild.id).unwrap(),
            TaskReadiness::Blocked
        );
        assert!(service
            .finish_manual(&project, &child.id, TaskPlanStatus::Succeeded, "too soon")
            .is_err());
    }

    #[test]
    fn manual_attempt_history_is_bounded() {
        let store = store();
        let service = TaskPlanService::new(&store);
        let project = RepositoryIdentity::resolve("/tmp/rover-plan").unwrap();
        let mut plan = service
            .create(&project, brief("Bounded retry", ""))
            .unwrap();
        for attempt in 1..=MAX_TASK_ATTEMPTS {
            plan = service.start_manual(&project, &plan.id).unwrap();
            plan = service
                .finish_manual(&project, &plan.id, TaskPlanStatus::Failed, "retry fixture")
                .unwrap();
            assert_eq!(plan.attempts.len(), attempt);
            if attempt < MAX_TASK_ATTEMPTS {
                plan = service.retry_manual(&project, &plan.id).unwrap();
            }
        }
        assert!(matches!(
            service.retry_manual(&project, &plan.id),
            Err(TaskPlanError::InvalidTransition)
        ));
    }
}
