//! Validated, execution-neutral task briefs and dependency graphs.
//!
//! A valid brief is not evidence that its quality-gate text is executable.
//! Gate syntax and execution policy belong to a later task-service contract.

use crate::Id;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

const TITLE_LIMIT: usize = 120;
const PATHS_LIMIT: usize = 8 * 1024;
const DEPENDENCIES_LIMIT: usize = 4 * 1024;
const QUALITY_GATE_LIMIT: usize = 1024;
const PROMPT_LIMIT: usize = 16 * 1024;
const MAX_PATHS: usize = 128;
const MAX_DEPENDENCIES: usize = 128;
const MAX_GRAPH_TASKS: usize = 10_000;

/// User-authored task definition fields captured by Rover's TUI.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TaskBrief {
    pub title: String,
    /// Newline-separated relative path globs. Supports `*`, `**`, and `?`.
    pub paths: String,
    /// Newline-separated Rover task IDs that must finish successfully first.
    pub dependencies: String,
    /// Opaque quality-gate description. It is never executed by this type.
    pub quality_gate: String,
    pub prompt: String,
}

/// One task's dependency declaration, before task records are resolved.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskGraphNode {
    pub id: Id,
    pub dependencies: Vec<Id>,
}

/// Invalid task brief or dependency graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TaskBriefError {
    EmptyTitle,
    EmptyPrompt,
    FieldTooLarge(&'static str),
    ForbiddenCharacter(&'static str),
    InvalidPathGlob,
    TooManyPaths,
    InvalidDependencyId,
    DuplicateDependency,
    TooManyDependencies,
}

impl fmt::Display for TaskBriefError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyTitle => formatter.write_str("task title must not be empty"),
            Self::EmptyPrompt => formatter.write_str("task prompt must not be empty"),
            Self::FieldTooLarge(field) => write!(formatter, "task {field} exceeds its byte limit"),
            Self::ForbiddenCharacter(field) => {
                write!(
                    formatter,
                    "task {field} contains a forbidden control or bidi character"
                )
            }
            Self::InvalidPathGlob => {
                formatter.write_str("task path glob is invalid or unsupported")
            }
            Self::TooManyPaths => formatter.write_str("task has too many path globs"),
            Self::InvalidDependencyId => {
                formatter.write_str("task dependency is not a valid Rover ID")
            }
            Self::DuplicateDependency => formatter.write_str("task has a duplicate dependency"),
            Self::TooManyDependencies => formatter.write_str("task has too many dependencies"),
        }
    }
}

impl std::error::Error for TaskBriefError {}

/// Invalid graph structure. Missing dependencies and cycles fail closed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TaskGraphError {
    TooManyTasks,
    DuplicateTask(Id),
    DuplicateDependency { task: Id, prerequisite: Id },
    MissingDependency { task: Id, prerequisite: Id },
    SelfDependency(Id),
    Cycle(Vec<Id>),
}

impl fmt::Display for TaskGraphError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyTasks => write!(formatter, "task graph exceeds {MAX_GRAPH_TASKS} tasks"),
            Self::DuplicateTask(id) => write!(formatter, "duplicate task ID {id}"),
            Self::DuplicateDependency { task, prerequisite } => {
                write!(formatter, "task {task} repeats dependency {prerequisite}")
            }
            Self::MissingDependency { task, prerequisite } => {
                write!(
                    formatter,
                    "task {task} depends on missing task {prerequisite}"
                )
            }
            Self::SelfDependency(id) => write!(formatter, "task {id} depends on itself"),
            Self::Cycle(ids) => write!(
                formatter,
                "task dependency cycle includes {} task(s)",
                ids.len()
            ),
        }
    }
}

impl std::error::Error for TaskGraphError {}

impl TaskBrief {
    /// Validate user-provided fields before creating a durable task.
    ///
    /// Draft persistence intentionally accepts incomplete drafts; call this
    /// only when the user requests task creation.
    ///
    /// # Errors
    ///
    /// Returns an error when required fields, field bounds, path globs, or
    /// dependency IDs are invalid.
    pub fn validate(&self) -> Result<(), TaskBriefError> {
        if self.title.trim().is_empty() {
            return Err(TaskBriefError::EmptyTitle);
        }
        if self.prompt.trim().is_empty() {
            return Err(TaskBriefError::EmptyPrompt);
        }
        for (name, value, limit, multiline) in [
            ("title", self.title.as_str(), TITLE_LIMIT, false),
            ("paths", self.paths.as_str(), PATHS_LIMIT, true),
            (
                "dependencies",
                self.dependencies.as_str(),
                DEPENDENCIES_LIMIT,
                true,
            ),
            (
                "quality gate",
                self.quality_gate.as_str(),
                QUALITY_GATE_LIMIT,
                true,
            ),
            ("prompt", self.prompt.as_str(), PROMPT_LIMIT, true),
        ] {
            if value.len() > limit {
                return Err(TaskBriefError::FieldTooLarge(name));
            }
            if value
                .chars()
                .any(|ch| (ch.is_control() && !(multiline && ch == '\n')) || is_bidi_format(ch))
            {
                return Err(TaskBriefError::ForbiddenCharacter(name));
            }
        }

        let paths = nonempty_lines(&self.paths);
        if paths.len() > MAX_PATHS {
            return Err(TaskBriefError::TooManyPaths);
        }
        if paths.iter().any(|path| !valid_path_glob(path)) {
            return Err(TaskBriefError::InvalidPathGlob);
        }

        let dependencies = nonempty_lines(&self.dependencies);
        if dependencies.len() > MAX_DEPENDENCIES {
            return Err(TaskBriefError::TooManyDependencies);
        }
        let mut seen = BTreeSet::new();
        for dependency in dependencies {
            let id = Id::parse(dependency.to_owned())
                .map_err(|_| TaskBriefError::InvalidDependencyId)?;
            if !seen.insert(id) {
                return Err(TaskBriefError::DuplicateDependency);
            }
        }
        Ok(())
    }
}

/// Validate task IDs and ensure dependencies exist and form a DAG.
///
/// Returns a stable topological order; ties are ordered lexically by task ID.
///
/// # Errors
///
/// Returns an error for an oversized graph, duplicate task or dependency IDs,
/// missing prerequisites, self-dependencies, or a dependency cycle.
pub fn validate_task_graph(nodes: &[TaskGraphNode]) -> Result<Vec<Id>, TaskGraphError> {
    if nodes.len() > MAX_GRAPH_TASKS {
        return Err(TaskGraphError::TooManyTasks);
    }
    let mut dependencies = BTreeMap::<Id, BTreeSet<Id>>::new();
    for node in nodes {
        if dependencies.contains_key(&node.id) {
            return Err(TaskGraphError::DuplicateTask(node.id.clone()));
        }
        let prerequisites = node.dependencies.iter().cloned().collect::<BTreeSet<_>>();
        if prerequisites.len() != node.dependencies.len() {
            let mut seen = BTreeSet::new();
            for prerequisite in &node.dependencies {
                if !seen.insert(prerequisite.clone()) {
                    return Err(TaskGraphError::DuplicateDependency {
                        task: node.id.clone(),
                        prerequisite: prerequisite.clone(),
                    });
                }
            }
        }
        dependencies.insert(node.id.clone(), prerequisites);
    }
    for (task, prerequisites) in &dependencies {
        for prerequisite in prerequisites {
            if task == prerequisite {
                return Err(TaskGraphError::SelfDependency(task.clone()));
            }
            if !dependencies.contains_key(prerequisite) {
                return Err(TaskGraphError::MissingDependency {
                    task: task.clone(),
                    prerequisite: prerequisite.clone(),
                });
            }
        }
    }

    let mut dependents = BTreeMap::<Id, Vec<Id>>::new();
    let mut ready = BTreeSet::<Id>::new();
    for (task, prerequisites) in &dependencies {
        if prerequisites.is_empty() {
            ready.insert(task.clone());
        }
        for prerequisite in prerequisites {
            dependents
                .entry(prerequisite.clone())
                .or_default()
                .push(task.clone());
        }
    }
    let mut order = Vec::with_capacity(nodes.len());
    while let Some(id) = ready.pop_first() {
        order.push(id.clone());
        if let Some(children) = dependents.get(&id) {
            for child in children {
                let Some(prerequisites) = dependencies.get_mut(child) else {
                    continue;
                };
                prerequisites.remove(&id);
                if prerequisites.is_empty() {
                    ready.insert(child.clone());
                }
            }
        }
    }
    if order.len() != nodes.len() {
        let completed = order.into_iter().collect::<BTreeSet<_>>();
        let cycle = dependencies
            .into_keys()
            .filter(|id| !completed.contains(id))
            .collect();
        return Err(TaskGraphError::Cycle(cycle));
    }
    Ok(order)
}

fn nonempty_lines(value: &str) -> Vec<&str> {
    value
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect()
}

fn valid_path_glob(path: &str) -> bool {
    if path.is_empty()
        || path == "."
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains('\0')
        || path
            .chars()
            .any(|ch| matches!(ch, '[' | ']' | '{' | '}' | '\r'))
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return false;
    }
    true
}

fn is_bidi_format(ch: char) -> bool {
    matches!(
        ch,
        '\u{061c}'
            | '\u{200e}'..='\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}'
            | '\u{206a}'..='\u{206f}'
    )
}

#[cfg(test)]
mod tests {
    use super::{validate_task_graph, TaskBrief, TaskBriefError, TaskGraphError, TaskGraphNode};
    use crate::Id;

    fn brief() -> TaskBrief {
        TaskBrief {
            title: "Feature work".into(),
            paths: "src/**\nCargo.toml".into(),
            dependencies: "task_base".into(),
            quality_gate: "cargo test -p rover-core".into(),
            prompt: "Implement the requested behavior".into(),
        }
    }

    fn node(id: &str, dependencies: &[&str]) -> TaskGraphNode {
        TaskGraphNode {
            id: Id::parse(id).unwrap(),
            dependencies: dependencies
                .iter()
                .map(|value| Id::parse(*value).unwrap())
                .collect(),
        }
    }

    #[test]
    fn validates_brief_and_supported_relative_globs() {
        assert!(brief().validate().is_ok());
        for invalid in [
            "/etc/**",
            "../secret",
            "src/../secret",
            "src\\**",
            "src/[ab]",
            "src//lib.rs",
            "src/",
        ] {
            let mut value = brief();
            value.paths = invalid.into();
            assert_eq!(
                value.validate(),
                Err(TaskBriefError::InvalidPathGlob),
                "{invalid}"
            );
        }
    }

    #[test]
    fn validates_dependency_ids_and_duplicates() {
        let mut value = brief();
        value.dependencies = "task_base\ntask_base".into();
        assert_eq!(value.validate(), Err(TaskBriefError::DuplicateDependency));
        value.dependencies = "bad/id".into();
        assert_eq!(value.validate(), Err(TaskBriefError::InvalidDependencyId));
    }

    #[test]
    fn returns_deterministic_topological_order() {
        let graph = [
            node("task_z", &["task_a"]),
            node("task_a", &[]),
            node("task_m", &[]),
        ];
        let order = validate_task_graph(&graph).unwrap();
        assert_eq!(
            order.iter().map(Id::as_str).collect::<Vec<_>>(),
            ["task_a", "task_m", "task_z"]
        );
    }

    #[test]
    fn rejects_missing_self_duplicate_and_cyclic_dependencies() {
        assert!(matches!(
            validate_task_graph(&[node("task_a", &["task_missing"])]),
            Err(TaskGraphError::MissingDependency { .. })
        ));
        assert_eq!(
            validate_task_graph(&[node("task_a", &["task_a"])]),
            Err(TaskGraphError::SelfDependency(Id::parse("task_a").unwrap()))
        );
        assert_eq!(
            validate_task_graph(&[node("task_a", &[]), node("task_a", &[])]),
            Err(TaskGraphError::DuplicateTask(Id::parse("task_a").unwrap()))
        );
        assert!(matches!(
            validate_task_graph(&[node("task_a", &["task_b", "task_b"]), node("task_b", &[])]),
            Err(TaskGraphError::DuplicateDependency { .. })
        ));
        assert!(matches!(
            validate_task_graph(&[node("task_a", &["task_b"]), node("task_b", &["task_a"])]),
            Err(TaskGraphError::Cycle(_))
        ));
    }
}
