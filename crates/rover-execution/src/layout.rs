//! Serializable terminal workspace, tab, split, zoom, popup, and scratch model.
//!
//! This module owns layout state only. It does not render widgets or imply that
//! a pane's referenced session is currently running.

use std::io;
use std::path::PathBuf;

use rover_core::Id;
#[cfg(unix)]
use rover_store::files::SafeDir;
use serde::{Deserialize, Serialize};

const WORKSPACE_SCHEMA: &str = "rover/terminal-workspace/v1";
const MAX_TITLE_BYTES: usize = 256;
const MAX_WORKSPACE_BYTES: usize = 4 * 1024 * 1024;
const MAX_JSON_DEPTH: usize = 96;
const MAX_TABS: usize = 128;
const MAX_PANES: usize = 256;
const MAX_SPLIT_DEPTH: usize = 16;
const MAX_RATIO_BASIS_POINTS: u16 = 10_000;

/// Direction used to divide one pane into two children.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum SplitAxis {
    /// Place children side by side.
    Horizontal,
    /// Place children one above the other.
    Vertical,
}

/// What a pane displays. A session reference is metadata, not proof the child
/// process exists; callers resolve it through the session server.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PaneKind {
    /// A named terminal session inside this workspace's namespace.
    Session { name: String },
    /// A temporary terminal owned by this workspace.
    Scratch,
}

/// A pane specification accepted when creating tabs or splitting panes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaneSpec {
    kind: PaneKind,
    cwd: Option<PathBuf>,
}

impl PaneSpec {
    /// Create a reference to a session in the workspace namespace.
    ///
    /// # Errors
    ///
    /// Returns `InvalidInput` when the session name or working directory is
    /// invalid.
    pub fn session(name: impl Into<String>, cwd: impl Into<PathBuf>) -> io::Result<Self> {
        let name = name.into();
        validate_id(&name, "session name")?;
        let cwd = cwd.into();
        validate_cwd(&cwd)?;
        Ok(Self {
            kind: PaneKind::Session { name },
            cwd: Some(cwd),
        })
    }

    /// Create a scratch pane with no associated directory.
    #[must_use]
    pub const fn scratch() -> Self {
        Self {
            kind: PaneKind::Scratch,
            cwd: None,
        }
    }

    /// Pane content type.
    #[must_use]
    pub const fn kind(&self) -> &PaneKind {
        &self.kind
    }

    /// Optional working directory associated with the pane.
    #[must_use]
    pub fn cwd(&self) -> Option<&PathBuf> {
        self.cwd.as_ref()
    }
}

/// One stable terminal pane.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Pane {
    id: String,
    kind: PaneKind,
    cwd: Option<PathBuf>,
}

impl Pane {
    /// Stable identifier retained across layout edits and serialization.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Pane content type.
    #[must_use]
    pub const fn kind(&self) -> &PaneKind {
        &self.kind
    }

    /// Optional working directory.
    #[must_use]
    pub fn cwd(&self) -> Option<&PathBuf> {
        self.cwd.as_ref()
    }
}

/// Binary split node with a stable ID and first-child size ratio.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Split {
    id: String,
    axis: SplitAxis,
    first_basis_points: u16,
    first: Box<PaneNode>,
    second: Box<PaneNode>,
}

impl Split {
    /// Stable identifier for resizing this split later.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Split orientation.
    #[must_use]
    pub const fn axis(&self) -> SplitAxis {
        self.axis
    }

    /// Size of the first child as basis points out of 10,000.
    #[must_use]
    pub const fn first_basis_points(&self) -> u16 {
        self.first_basis_points
    }

    /// First child in visual order.
    #[must_use]
    pub const fn first(&self) -> &PaneNode {
        &self.first
    }

    /// Second child in visual order.
    #[must_use]
    pub const fn second(&self) -> &PaneNode {
        &self.second
    }
}

/// Recursive terminal-pane layout tree.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PaneNode {
    /// A terminal or scratch pane.
    Leaf(Pane),
    /// A two-child split.
    Split(Split),
}

/// One named tab with independent focus, zoom, popup, and split tree.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Tab {
    id: String,
    title: String,
    root: PaneNode,
    active_pane: String,
    zoomed_pane: Option<String>,
    popup_pane: Option<String>,
}

impl Tab {
    /// Stable tab identifier.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// User-visible tab title.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Root layout for rendering.
    #[must_use]
    pub const fn root(&self) -> &PaneNode {
        &self.root
    }

    /// Pane currently receiving keyboard input.
    #[must_use]
    pub fn active_pane(&self) -> &str {
        &self.active_pane
    }

    /// Pane shown alone while zoom is active.
    #[must_use]
    pub fn zoomed_pane(&self) -> Option<&str> {
        self.zoomed_pane.as_deref()
    }

    /// Pane displayed in the popup overlay.
    #[must_use]
    pub fn popup_pane(&self) -> Option<&str> {
        self.popup_pane.as_deref()
    }

    /// Current render root, honoring this tab's zoom state.
    #[must_use]
    pub fn render_root(&self) -> &PaneNode {
        self.zoomed_pane
            .as_deref()
            .and_then(|id| find_node(&self.root, id))
            .unwrap_or(&self.root)
    }
}

/// One project-scoped terminal workspace with stable, serializable layouts.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Workspace {
    schema: String,
    id: String,
    namespace: String,
    title: String,
    tabs: Vec<Tab>,
    active_tab: String,
}

impl Workspace {
    /// Create a workspace with an initial tab and named session pane.
    ///
    /// # Errors
    ///
    /// Returns an I/O error for invalid names or title, or if secure ID
    /// generation fails.
    pub fn new(
        namespace: impl Into<String>,
        title: impl Into<String>,
        session_name: impl Into<String>,
        cwd: impl Into<PathBuf>,
    ) -> io::Result<Self> {
        let namespace = namespace.into();
        validate_id(&namespace, "workspace namespace")?;
        let title = validate_title(title.into(), "workspace title")?;
        let pane_spec = PaneSpec::session(session_name, cwd)?;
        let pane = pane_from_spec(pane_spec)?;
        let tab = Tab {
            id: generate_id("tab")?,
            title: "main".to_owned(),
            active_pane: pane.id.clone(),
            root: PaneNode::Leaf(pane),
            zoomed_pane: None,
            popup_pane: None,
        };
        let workspace = Self {
            schema: WORKSPACE_SCHEMA.to_owned(),
            id: generate_id("workspace")?,
            namespace,
            title,
            active_tab: tab.id.clone(),
            tabs: vec![tab],
        };
        workspace.validate()?;
        Ok(workspace)
    }

    /// Stable workspace identifier.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Namespace used to resolve all session panes.
    #[must_use]
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// User-visible workspace title.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Tabs in stable ordering.
    #[must_use]
    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    /// Currently selected tab.
    ///
    /// # Panics
    ///
    /// Panics only if an internal invariant is broken: every valid workspace
    /// keeps its active tab in `tabs`.
    #[must_use]
    pub fn active_tab(&self) -> &Tab {
        self.tabs
            .iter()
            .find(|tab| tab.id == self.active_tab)
            .expect("validated workspace keeps active tab present")
    }

    /// Add and select a tab containing a session or scratch pane.
    ///
    /// # Errors
    ///
    /// Returns `InvalidInput` for an invalid title or when the workspace is at
    /// its 128-tab limit, or an I/O error if secure ID generation fails.
    pub fn add_tab(&mut self, title: impl Into<String>, pane: PaneSpec) -> io::Result<String> {
        if self.tabs.len() >= MAX_TABS {
            return Err(invalid_input("workspace exceeds 128 tabs"));
        }
        let title = validate_title(title.into(), "tab title")?;
        let pane = pane_from_spec(pane)?;
        let tab = Tab {
            id: generate_id("tab")?,
            title,
            active_pane: pane.id.clone(),
            root: PaneNode::Leaf(pane),
            zoomed_pane: None,
            popup_pane: None,
        };
        self.active_tab.clone_from(&tab.id);
        let id = tab.id.clone();
        self.tabs.push(tab);
        Ok(id)
    }

    /// Select a tab by stable ID.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` if the tab does not belong to this workspace.
    pub fn focus_tab(&mut self, tab_id: &str) -> io::Result<()> {
        if !self.tabs.iter().any(|tab| tab.id == tab_id) {
            return Err(not_found("tab not found in workspace"));
        }
        tab_id.clone_into(&mut self.active_tab);
        Ok(())
    }

    /// Close a tab and select the adjacent surviving tab when necessary.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` for an unknown tab and `InvalidInput` when closing
    /// the workspace's final tab.
    pub fn close_tab(&mut self, tab_id: &str) -> io::Result<()> {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
            return Err(not_found("tab not found in workspace"));
        };
        if self.tabs.len() == 1 {
            return Err(invalid_input("workspace must keep at least one tab"));
        }
        self.tabs.remove(index);
        if self.active_tab == tab_id {
            self.active_tab = self.tabs[index.min(self.tabs.len() - 1)].id.clone();
        }
        Ok(())
    }

    /// Add a pane beside/below the focused pane and make it active.
    ///
    /// # Errors
    ///
    /// Returns `InvalidInput` for an invalid split ratio or pane limit, and an
    /// I/O error if secure IDs cannot be generated.
    pub fn split_active(
        &mut self,
        axis: SplitAxis,
        first_basis_points: u16,
        pane: PaneSpec,
    ) -> io::Result<String> {
        if first_basis_points == 0 || first_basis_points >= MAX_RATIO_BASIS_POINTS {
            return Err(invalid_input(
                "split ratio must be between 1 and 9999 basis points",
            ));
        }
        if self.pane_count() >= MAX_PANES {
            return Err(invalid_input("workspace exceeds 256 panes"));
        }
        let new_pane = pane_from_spec(pane)?;
        let new_id = new_pane.id.clone();
        let split_id = generate_id("split")?;
        let tab = self.active_tab_mut();
        replace_leaf_with_split(
            &mut tab.root,
            &tab.active_pane,
            Split {
                id: split_id,
                axis,
                first_basis_points,
                first: Box::new(PaneNode::Leaf(Pane {
                    id: String::new(),
                    kind: PaneKind::Scratch,
                    cwd: None,
                })),
                second: Box::new(PaneNode::Leaf(new_pane)),
            },
        )?;
        tab.active_pane.clone_from(&new_id);
        Ok(new_id)
    }

    /// Resize one split using its stable ID.
    ///
    /// # Errors
    ///
    /// Returns `InvalidInput` for an invalid ratio, or `NotFound` when no split
    /// has that ID in this workspace.
    pub fn resize_split(&mut self, split_id: &str, first_basis_points: u16) -> io::Result<()> {
        if first_basis_points == 0 || first_basis_points >= MAX_RATIO_BASIS_POINTS {
            return Err(invalid_input(
                "split ratio must be between 1 and 9999 basis points",
            ));
        }
        if set_split_ratio(
            &mut self.active_tab_mut().root,
            split_id,
            first_basis_points,
        ) {
            Ok(())
        } else {
            Err(not_found("split not found in active tab"))
        }
    }

    /// Focus a pane in the active tab.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` if the pane is not in the active tab.
    pub fn focus_pane(&mut self, pane_id: &str) -> io::Result<()> {
        let tab = self.active_tab_mut();
        if find_pane(&tab.root, pane_id).is_none() {
            return Err(not_found("pane not found in active tab"));
        }
        pane_id.clone_into(&mut tab.active_pane);
        Ok(())
    }

    /// Toggle zoom for the focused pane.
    pub fn toggle_zoom(&mut self) {
        let tab = self.active_tab_mut();
        if tab.zoomed_pane.as_deref() == Some(tab.active_pane.as_str()) {
            tab.zoomed_pane = None;
        } else {
            tab.zoomed_pane = Some(tab.active_pane.clone());
        }
    }

    /// Open a popup overlay for the focused pane.
    pub fn open_popup(&mut self) {
        let tab = self.active_tab_mut();
        tab.popup_pane = Some(tab.active_pane.clone());
    }

    /// Close an open popup, if any.
    pub fn close_popup(&mut self) {
        self.active_tab_mut().popup_pane = None;
    }

    /// Close a pane. Its sibling replaces the split and the tab keeps focus on
    /// a surviving pane. The final pane in a tab must be closed with the tab.
    ///
    /// # Errors
    ///
    /// Returns `NotFound` for an unknown pane and `InvalidInput` when it is the
    /// tab's final pane.
    ///
    /// # Panics
    ///
    /// Panics only if the pane-tree invariant is broken while removing a pane;
    /// a validated tree with more than one pane always has a surviving sibling.
    pub fn close_pane(&mut self, pane_id: &str) -> io::Result<()> {
        let tab = self.active_tab_mut();
        if find_pane(&tab.root, pane_id).is_none() {
            return Err(not_found("pane not found in active tab"));
        }
        if count_panes(&tab.root) == 1 {
            return Err(invalid_input("close the tab to remove its final pane"));
        }
        remove_pane(&mut tab.root, pane_id);
        if tab.active_pane == pane_id {
            tab.active_pane = first_pane(&tab.root)
                .expect("a sibling pane survives close")
                .to_owned();
        }
        if tab.zoomed_pane.as_deref() == Some(pane_id) {
            tab.zoomed_pane = None;
        }
        if tab.popup_pane.as_deref() == Some(pane_id) {
            tab.popup_pane = None;
        }
        Ok(())
    }

    /// Count all terminal and scratch panes in the workspace.
    #[must_use]
    pub fn pane_count(&self) -> usize {
        self.tabs.iter().map(|tab| count_panes(&tab.root)).sum()
    }

    /// Validate deserialized workspace state and all cross-references.
    ///
    /// # Errors
    ///
    /// Returns `InvalidData` if schema, IDs, titles, tree bounds, namespace
    /// references, active focus, zoom, or popup state are invalid.
    pub fn validate(&self) -> io::Result<()> {
        if self.schema != WORKSPACE_SCHEMA {
            return Err(invalid_data("unsupported terminal workspace schema"));
        }
        validate_id(&self.id, "workspace ID").map_err(|error| as_invalid_data(&error))?;
        validate_id(&self.namespace, "workspace namespace")
            .map_err(|error| as_invalid_data(&error))?;
        validate_title(self.title.clone(), "workspace title")
            .map_err(|error| as_invalid_data(&error))?;
        if self.tabs.is_empty() || self.tabs.len() > MAX_TABS {
            return Err(invalid_data(
                "workspace tab count is outside supported bounds",
            ));
        }
        let mut all_ids = vec![self.id.as_str()];
        let mut pane_total = 0;
        for tab in &self.tabs {
            validate_id(&tab.id, "tab ID").map_err(|error| as_invalid_data(&error))?;
            validate_title(tab.title.clone(), "tab title")
                .map_err(|error| as_invalid_data(&error))?;
            all_ids.push(&tab.id);
            validate_node(&tab.root, 0, &mut pane_total, &mut all_ids)?;
            if find_pane(&tab.root, &tab.active_pane).is_none() {
                return Err(invalid_data("active pane is missing from its tab"));
            }
            for optional_pane in [tab.zoomed_pane.as_deref(), tab.popup_pane.as_deref()]
                .into_iter()
                .flatten()
            {
                if find_pane(&tab.root, optional_pane).is_none() {
                    return Err(invalid_data("zoom or popup pane is missing from its tab"));
                }
            }
        }
        if pane_total > MAX_PANES {
            return Err(invalid_data("workspace exceeds 256 panes"));
        }
        if !self.tabs.iter().any(|tab| tab.id == self.active_tab) {
            return Err(invalid_data("active tab is missing from workspace"));
        }
        all_ids.sort_unstable();
        if all_ids.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(invalid_data("workspace contains duplicate stable IDs"));
        }
        Ok(())
    }

    /// Serialize to JSON after validating all current invariants.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if validation or JSON serialization fails.
    pub fn to_json(&self) -> io::Result<Vec<u8>> {
        self.validate()?;
        let bytes = serde_json::to_vec(self).map_err(|error| invalid_data(error.to_string()))?;
        if bytes.len() > MAX_WORKSPACE_BYTES {
            return Err(invalid_data("terminal workspace JSON exceeds 4 MiB"));
        }
        Ok(bytes)
    }

    /// Deserialize JSON and validate every ID and tree reference.
    ///
    /// # Errors
    ///
    /// Returns `InvalidData` for malformed JSON or invalid workspace state.
    pub fn from_json(bytes: &[u8]) -> io::Result<Self> {
        if bytes.len() > MAX_WORKSPACE_BYTES {
            return Err(invalid_data("terminal workspace JSON exceeds 4 MiB"));
        }
        validate_json_depth(bytes)?;
        let workspace: Self =
            serde_json::from_slice(bytes).map_err(|error| invalid_data(error.to_string()))?;
        workspace.validate()?;
        Ok(workspace)
    }

    /// Atomically persist this validated workspace as a private state file.
    ///
    /// The directory must already be an admitted private Rover state
    /// directory. The file is written with mode `0600`; the basename is
    /// validated by the descriptor-relative filesystem layer.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if workspace validation, serialization, or the
    /// atomic file write fails.
    #[cfg(unix)]
    pub fn save_to(&self, directory: &SafeDir, name: &str) -> io::Result<()> {
        directory.atomic_write(name, &self.to_json()?, 0o600)
    }

    /// Load a bounded, validated workspace from a private state directory.
    ///
    /// The method reads at most 4 MiB plus one byte so oversized layouts are
    /// rejected instead of being silently truncated.
    ///
    /// # Errors
    ///
    /// Returns an I/O error for missing/unsafe files, oversized data, or
    /// invalid workspace JSON.
    #[cfg(unix)]
    pub fn load_from(directory: &SafeDir, name: &str) -> io::Result<Self> {
        let bytes = directory.bounded_read(
            name,
            i64::try_from(MAX_WORKSPACE_BYTES + 1)
                .map_err(|_| invalid_data("workspace size limit exceeds file API"))?,
        )?;
        Self::from_json(&bytes)
    }

    fn active_tab_mut(&mut self) -> &mut Tab {
        self.tabs
            .iter_mut()
            .find(|tab| tab.id == self.active_tab)
            .expect("validated workspace keeps active tab present")
    }
}

fn validate_json_depth(bytes: &[u8]) -> io::Result<()> {
    let mut depth = 0_usize;
    let mut in_string = false;
    let mut escaped = false;
    for byte in bytes {
        if in_string {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth += 1;
                if depth > MAX_JSON_DEPTH {
                    return Err(invalid_data(
                        "terminal workspace JSON exceeds nesting limit",
                    ));
                }
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Ok(())
}

fn validate_node<'a>(
    node: &'a PaneNode,
    depth: usize,
    pane_total: &mut usize,
    all_ids: &mut Vec<&'a str>,
) -> io::Result<()> {
    if depth > MAX_SPLIT_DEPTH {
        return Err(invalid_data("workspace split tree exceeds depth limit"));
    }
    match node {
        PaneNode::Leaf(pane) => {
            *pane_total += 1;
            validate_id(&pane.id, "pane ID").map_err(|error| as_invalid_data(&error))?;
            all_ids.push(&pane.id);
            match &pane.kind {
                PaneKind::Session { name } => {
                    validate_id(name, "session name").map_err(|error| as_invalid_data(&error))?;
                    let cwd = pane
                        .cwd
                        .as_ref()
                        .ok_or_else(|| invalid_data("session pane has no working directory"))?;
                    validate_cwd(cwd).map_err(|error| as_invalid_data(&error))?;
                }
                PaneKind::Scratch if pane.cwd.is_none() => {}
                PaneKind::Scratch => {
                    return Err(invalid_data(
                        "scratch pane cannot claim a session directory",
                    ));
                }
            }
        }
        PaneNode::Split(split) => {
            validate_id(&split.id, "split ID").map_err(|error| as_invalid_data(&error))?;
            all_ids.push(&split.id);
            if split.first_basis_points == 0 || split.first_basis_points >= MAX_RATIO_BASIS_POINTS {
                return Err(invalid_data("split ratio is outside supported bounds"));
            }
            validate_node(&split.first, depth + 1, pane_total, all_ids)?;
            validate_node(&split.second, depth + 1, pane_total, all_ids)?;
        }
    }
    Ok(())
}

fn pane_from_spec(spec: PaneSpec) -> io::Result<Pane> {
    Ok(Pane {
        id: generate_id("pane")?,
        kind: spec.kind,
        cwd: spec.cwd,
    })
}

fn generate_id(prefix: &str) -> io::Result<String> {
    Id::generate(prefix)
        .map(|id| id.to_string())
        .map_err(|error| io::Error::other(error.to_string()))
}

fn validate_id(value: &str, label: &str) -> io::Result<()> {
    Id::parse(value.to_owned())
        .map(|_| ())
        .map_err(|_| invalid_input(format!("invalid {label}")))
}

fn validate_title(value: String, label: &str) -> io::Result<String> {
    if value.trim().is_empty()
        || value.len() > MAX_TITLE_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(invalid_input(format!("invalid {label}")));
    }
    Ok(value)
}

fn validate_cwd(cwd: &std::path::Path) -> io::Result<()> {
    if !cwd.is_absolute() || cwd.as_os_str().is_empty() {
        return Err(invalid_input("pane working directory must be absolute"));
    }
    Ok(())
}

fn replace_leaf_with_split(root: &mut PaneNode, pane_id: &str, split: Split) -> io::Result<()> {
    if matches!(root, PaneNode::Leaf(pane) if pane.id == pane_id) {
        let PaneNode::Leaf(existing) = std::mem::replace(
            root,
            PaneNode::Leaf(Pane {
                id: String::new(),
                kind: PaneKind::Scratch,
                cwd: None,
            }),
        ) else {
            unreachable!();
        };
        *root = PaneNode::Split(Split {
            first: Box::new(PaneNode::Leaf(existing)),
            ..split
        });
        return Ok(());
    }
    match root {
        PaneNode::Leaf(_) => Err(not_found("active pane not found in layout")),
        PaneNode::Split(current) => {
            if replace_leaf_with_split(&mut current.first, pane_id, split.clone()).is_ok() {
                Ok(())
            } else {
                replace_leaf_with_split(&mut current.second, pane_id, split)
            }
        }
    }
}

fn set_split_ratio(root: &mut PaneNode, split_id: &str, ratio: u16) -> bool {
    match root {
        PaneNode::Leaf(_) => false,
        PaneNode::Split(split) if split.id == split_id => {
            split.first_basis_points = ratio;
            true
        }
        PaneNode::Split(split) => {
            set_split_ratio(&mut split.first, split_id, ratio)
                || set_split_ratio(&mut split.second, split_id, ratio)
        }
    }
}

fn remove_pane(root: &mut PaneNode, pane_id: &str) -> bool {
    let PaneNode::Split(split) = root else {
        return false;
    };
    if matches!(split.first.as_ref(), PaneNode::Leaf(pane) if pane.id == pane_id) {
        *root = *split.second.clone();
        return true;
    }
    if matches!(split.second.as_ref(), PaneNode::Leaf(pane) if pane.id == pane_id) {
        *root = *split.first.clone();
        return true;
    }
    remove_pane(&mut split.first, pane_id) || remove_pane(&mut split.second, pane_id)
}

fn find_pane<'a>(root: &'a PaneNode, pane_id: &str) -> Option<&'a Pane> {
    match root {
        PaneNode::Leaf(pane) if pane.id == pane_id => Some(pane),
        PaneNode::Leaf(_) => None,
        PaneNode::Split(split) => {
            find_pane(&split.first, pane_id).or_else(|| find_pane(&split.second, pane_id))
        }
    }
}

fn find_node<'a>(root: &'a PaneNode, pane_id: &str) -> Option<&'a PaneNode> {
    match root {
        PaneNode::Leaf(pane) if pane.id == pane_id => Some(root),
        PaneNode::Leaf(_) => None,
        PaneNode::Split(split) => {
            find_node(&split.first, pane_id).or_else(|| find_node(&split.second, pane_id))
        }
    }
}

fn first_pane(root: &PaneNode) -> Option<&str> {
    match root {
        PaneNode::Leaf(pane) => Some(&pane.id),
        PaneNode::Split(split) => first_pane(&split.first).or_else(|| first_pane(&split.second)),
    }
}

fn count_panes(root: &PaneNode) -> usize {
    match root {
        PaneNode::Leaf(_) => 1,
        PaneNode::Split(split) => count_panes(&split.first) + count_panes(&split.second),
    }
}

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn not_found(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, message.into())
}

fn as_invalid_data(error: &io::Error) -> io::Error {
    invalid_data(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::{symlink, PermissionsExt};

    fn workspace() -> Workspace {
        Workspace::new("project", "Project", "main", PathBuf::from("/repo"))
            .expect("create workspace")
    }

    #[cfg(unix)]
    fn private_fixture_dir() -> (PathBuf, SafeDir) {
        let path = std::env::temp_dir().join(format!(
            "rover-layout-test-{}-{}",
            std::process::id(),
            generate_id("test").unwrap()
        ));
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        let directory = SafeDir::open(&path).unwrap();
        (path, directory)
    }

    #[test]
    fn splits_focus_and_resize_preserve_stable_ids() {
        let mut workspace = workspace();
        let first_id = workspace.active_tab().active_pane().to_owned();
        let second_id = workspace
            .split_active(SplitAxis::Horizontal, 4500, PaneSpec::scratch())
            .expect("split focused pane");
        let PaneNode::Split(split) = workspace.active_tab().root() else {
            panic!("split root expected");
        };
        let split_id = split.id().to_owned();
        assert_eq!(split.first_basis_points(), 4500);
        assert_eq!(workspace.active_tab().active_pane(), second_id);
        workspace.resize_split(&split_id, 6000).unwrap();
        workspace.focus_pane(&first_id).unwrap();
        assert_eq!(workspace.active_tab().active_pane(), first_id);
        assert_eq!(workspace.pane_count(), 2);
    }

    #[test]
    fn pane_close_collapses_tree_and_clears_zoom_and_popup_refs() {
        let mut workspace = workspace();
        let first = workspace.active_tab().active_pane().to_owned();
        let second = workspace
            .split_active(SplitAxis::Vertical, 5000, PaneSpec::scratch())
            .unwrap();
        workspace.toggle_zoom();
        workspace.open_popup();
        workspace.close_pane(&second).unwrap();
        assert_eq!(workspace.pane_count(), 1);
        assert!(matches!(workspace.active_tab().root(), PaneNode::Leaf(_)));
        assert_eq!(workspace.active_tab().active_pane(), first);
        assert_eq!(workspace.active_tab().zoomed_pane(), None);
        assert_eq!(workspace.active_tab().popup_pane(), None);
    }

    #[test]
    fn nested_split_and_close_preserve_sibling_subtrees() {
        let mut workspace = workspace();
        let first = workspace.active_tab().active_pane().to_owned();
        let second = workspace
            .split_active(SplitAxis::Horizontal, 4000, PaneSpec::scratch())
            .unwrap();
        workspace.focus_pane(&first).unwrap();
        let third = workspace
            .split_active(SplitAxis::Vertical, 6000, PaneSpec::scratch())
            .unwrap();

        assert_eq!(workspace.pane_count(), 3);
        assert_eq!(workspace.active_tab().active_pane(), third);
        workspace.validate().unwrap();
        workspace.close_pane(&first).unwrap();
        assert_eq!(workspace.pane_count(), 2);
        assert!(find_pane(workspace.active_tab().root(), &second).is_some());
        assert!(find_pane(workspace.active_tab().root(), &third).is_some());
        workspace.validate().unwrap();
    }

    #[test]
    fn tabs_keep_independent_focus_and_closing_active_selects_neighbor() {
        let mut workspace = workspace();
        let first_tab = workspace.active_tab().id().to_owned();
        let second_tab = workspace.add_tab("scratch", PaneSpec::scratch()).unwrap();
        assert_eq!(workspace.active_tab().id(), second_tab);
        workspace.focus_tab(&first_tab).unwrap();
        assert_eq!(workspace.active_tab().title(), "main");
        workspace.close_tab(&first_tab).unwrap();
        assert_eq!(workspace.active_tab().id(), second_tab);
    }

    #[test]
    fn zoom_and_popup_are_independent_per_tab() {
        let mut workspace = workspace();
        let pane = workspace.active_tab().active_pane().to_owned();
        workspace.toggle_zoom();
        workspace.open_popup();
        assert_eq!(workspace.active_tab().zoomed_pane(), Some(pane.as_str()));
        assert_eq!(workspace.active_tab().popup_pane(), Some(pane.as_str()));
        assert!(matches!(
            workspace.active_tab().render_root(),
            PaneNode::Leaf(_)
        ));
        workspace.close_popup();
        assert!(workspace.active_tab().popup_pane().is_none());
        workspace.toggle_zoom();
        assert!(workspace.active_tab().zoomed_pane().is_none());
    }

    #[test]
    fn serialization_round_trip_preserves_ids_order_and_layout() {
        let mut workspace = workspace();
        workspace
            .split_active(SplitAxis::Horizontal, 3333, PaneSpec::scratch())
            .unwrap();
        let expected = workspace.clone();
        let bytes = workspace.to_json().unwrap();
        let restored = Workspace::from_json(&bytes).unwrap();
        assert_eq!(restored, expected);
    }

    #[cfg(unix)]
    #[test]
    fn workspace_state_saves_atomically_with_private_mode_and_restores_layout() {
        let (path, directory) = private_fixture_dir();
        let mut workspace = workspace();
        workspace
            .split_active(SplitAxis::Horizontal, 4_500, PaneSpec::scratch())
            .unwrap();
        workspace.save_to(&directory, "workspace.json").unwrap();
        assert_eq!(
            Workspace::load_from(&directory, "workspace.json").unwrap(),
            workspace
        );
        assert_eq!(
            fs::metadata(path.join("workspace.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );

        let active = workspace.active_tab().active_pane().to_owned();
        workspace.close_pane(&active).unwrap();
        workspace.save_to(&directory, "workspace.json").unwrap();
        assert_eq!(
            Workspace::load_from(&directory, "workspace.json").unwrap(),
            workspace
        );
        drop(directory);
        fs::remove_dir_all(path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn workspace_state_refuses_symlinks_oversize_and_invalid_json() {
        let (path, directory) = private_fixture_dir();
        symlink("missing.json", path.join("linked.json")).unwrap();
        assert!(Workspace::load_from(&directory, "linked.json").is_err());

        fs::write(path.join("large.json"), vec![b' '; MAX_WORKSPACE_BYTES + 1]).unwrap();
        assert_eq!(
            Workspace::load_from(&directory, "large.json")
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );

        directory.atomic_write("broken.json", b"{}", 0o600).unwrap();
        assert_eq!(
            Workspace::load_from(&directory, "broken.json")
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        drop(directory);
        fs::remove_dir_all(path).unwrap();
    }

    #[test]
    fn deserialization_rejects_corrupt_schema_ids_and_references() {
        let workspace = workspace();
        let mut value: serde_json::Value =
            serde_json::from_slice(&workspace.to_json().unwrap()).unwrap();
        value["schema"] = "rover/terminal-workspace/v0".into();
        assert_eq!(
            Workspace::from_json(&serde_json::to_vec(&value).unwrap())
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );

        value = serde_json::from_slice(&workspace.to_json().unwrap()).unwrap();
        value["tabs"][0]["active_pane"] = "missing".into();
        assert_eq!(
            Workspace::from_json(&serde_json::to_vec(&value).unwrap())
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn deserialization_bounds_size_and_nesting_before_recursive_decode() {
        let too_deep = format!(
            "{}null{}",
            "[".repeat(MAX_JSON_DEPTH + 1),
            "]".repeat(MAX_JSON_DEPTH + 1)
        );
        assert_eq!(
            Workspace::from_json(too_deep.as_bytes())
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            Workspace::from_json(&vec![b' '; MAX_WORKSPACE_BYTES + 1])
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
        let oversized_path = PathBuf::from(format!("/{}", "x".repeat(MAX_WORKSPACE_BYTES)));
        assert!(Workspace::new("project", "Project", "main", oversized_path)
            .unwrap()
            .to_json()
            .is_err());

        let mut value: serde_json::Value =
            serde_json::from_slice(&workspace().to_json().unwrap()).unwrap();
        value["title"] = "[".repeat(MAX_JSON_DEPTH + 1).into();
        assert!(Workspace::from_json(&serde_json::to_vec(&value).unwrap()).is_ok());
    }

    #[test]
    fn invalid_titles_ratios_and_final_pane_close_are_refused() {
        assert!(Workspace::new("project", "\u{1b}[31m", "main", "/repo").is_err());
        let mut workspace = workspace();
        assert!(workspace
            .split_active(SplitAxis::Horizontal, 10_000, PaneSpec::scratch())
            .is_err());
        let pane = workspace.active_tab().active_pane().to_owned();
        assert!(workspace.close_pane(&pane).is_err());
        assert_eq!(workspace.pane_count(), 1);
    }

    #[test]
    fn stable_ids_are_unique_across_workspace_tabs_splits_and_panes() {
        let mut workspace = workspace();
        workspace
            .split_active(SplitAxis::Horizontal, 5000, PaneSpec::scratch())
            .unwrap();
        workspace.add_tab("other", PaneSpec::scratch()).unwrap();
        workspace.validate().unwrap();
    }
}
