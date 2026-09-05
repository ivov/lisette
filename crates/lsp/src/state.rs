use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::protocol::{Client, Url};
use deps::BindgenSetup;

use crate::imports::PackageIndex;
use crate::loader::ProjectState;
use crate::position::LineIndex;
use crate::snapshot::AnalysisSnapshot;
use std::ops::Deref;

pub struct SharedState {
    pub(crate) client: Client,
    pub(crate) project: ProjectState,
    workspace: RwLock<Workspace>,
    pub(crate) bindgen_setup: Option<Arc<dyn BindgenSetup>>,
    pub(crate) packages: Arc<PackageIndex>,
    pub(crate) insert_replace_support: AtomicBool,
}

impl SharedState {
    pub(crate) fn workspace(&self) -> RwLockReadGuard<'_, Workspace> {
        self.workspace
            .read()
            .unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn workspace_mut(&self) -> RwLockWriteGuard<'_, Workspace> {
        self.workspace
            .write()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// Under one lock so a build installs against a document set that cannot
/// shift underneath it.
#[derive(Default)]
pub(crate) struct Workspace {
    pub(crate) documents: HashMap<Url, DocumentState>,
    analyses: HashMap<AnalysisKey, SharedAnalysis>,
    generation: u64,
}

/// What an analysis is determined by, and so what it can be shared across.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) enum AnalysisKey {
    Package {
        external_test: bool,
        package_id: String,
    },
    /// Scripts, and files the package loader excludes from sibling loading.
    Document { uri: Url },
}

#[derive(Default)]
struct SharedAnalysis {
    current: Option<Arc<AnalysisSnapshot>>,
    pending_diagnostics: Option<CancellationToken>,
    build: Arc<Mutex<()>>,
}

impl Workspace {
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn keys(&self) -> Vec<AnalysisKey> {
        self.analyses.keys().cloned().collect()
    }

    pub(crate) fn snapshot(&self, key: &AnalysisKey) -> Option<Arc<AnalysisSnapshot>> {
        self.analyses.get(key)?.current.clone()
    }

    pub(crate) fn ensure(&mut self, key: &AnalysisKey) {
        self.analyses.entry(key.clone()).or_default();
    }

    pub(crate) fn build_lock(&self, key: &AnalysisKey) -> Option<Arc<Mutex<()>>> {
        Some(Arc::clone(&self.analyses.get(key)?.build))
    }

    pub(crate) fn install(
        &mut self,
        key: &AnalysisKey,
        generation: u64,
        snapshot: Arc<AnalysisSnapshot>,
    ) -> bool {
        if self.generation != generation {
            return false;
        }
        let Some(analysis) = self.analyses.get_mut(key) else {
            return false;
        };
        analysis.current = Some(snapshot);
        true
    }

    pub(crate) fn invalidate_all(&mut self) {
        self.generation += 1;
        for analysis in self.analyses.values_mut() {
            analysis.current = None;
        }
    }

    pub(crate) fn set_pending_diagnostics(&mut self, key: &AnalysisKey, token: CancellationToken) {
        let Some(analysis) = self.analyses.get_mut(key) else {
            token.cancel();
            return;
        };
        if let Some(previous) = analysis.pending_diagnostics.replace(token) {
            previous.cancel();
        }
    }

    pub(crate) fn finish_diagnostics(&mut self, key: &AnalysisKey, token: &CancellationToken) {
        if let Some(analysis) = self.analyses.get_mut(key)
            && analysis.pending_diagnostics.as_ref() == Some(token)
        {
            analysis.pending_diagnostics = None;
        }
    }

    pub(crate) fn evict(&mut self, key: &AnalysisKey) {
        if let Some(analysis) = self.analyses.remove(key)
            && let Some(token) = analysis.pending_diagnostics
        {
            token.cancel();
        }
    }
}

/// Identity of one scheduled diagnostics run: cancelling it makes the
/// debounce thread return without publishing.
#[derive(Clone)]
pub(crate) struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    pub(crate) fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    pub(crate) fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

impl PartialEq for CancellationToken {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

pub struct Backend {
    pub(crate) shared_state: Arc<SharedState>,
}

impl Deref for Backend {
    type Target = SharedState;
    fn deref(&self) -> &SharedState {
        &self.shared_state
    }
}

pub(crate) struct DocumentState {
    content: String,
    line_index: LineIndex,
    version: i32,
    last_usable: Option<Arc<AnalysisSnapshot>>,
}

impl DocumentState {
    pub(crate) fn new(content: String, version: i32) -> Self {
        Self {
            line_index: LineIndex::new(&content),
            content,
            version,
            last_usable: None,
        }
    }

    pub(crate) fn update(&mut self, content: String, version: i32) {
        self.line_index = LineIndex::new(&content);
        self.content = content;
        self.version = version;
    }

    pub(crate) fn content(&self) -> &str {
        &self.content
    }

    pub(crate) fn line_index(&self) -> &LineIndex {
        &self.line_index
    }

    pub(crate) fn version(&self) -> i32 {
        self.version
    }

    pub(crate) fn last_usable(&self) -> Option<Arc<AnalysisSnapshot>> {
        self.last_usable.clone()
    }

    pub(crate) fn set_last_usable(&mut self, snapshot: Arc<AnalysisSnapshot>) {
        self.last_usable = Some(snapshot);
    }
}

impl Backend {
    pub(crate) fn new(client: Client, bindgen_setup: Option<Arc<dyn BindgenSetup>>) -> Self {
        Self {
            shared_state: Arc::new(SharedState {
                client,
                project: ProjectState::new(),
                workspace: RwLock::default(),
                bindgen_setup,
                packages: Arc::default(),
                insert_replace_support: AtomicBool::new(false),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> AnalysisKey {
        AnalysisKey::Package {
            external_test: false,
            package_id: "_entry_".to_string(),
        }
    }

    #[test]
    fn replacing_pending_diagnostics_cancels_previous_run() {
        let old_token = CancellationToken::new();
        let new_token = CancellationToken::new();
        let mut workspace = Workspace::default();
        workspace.ensure(&key());
        workspace.set_pending_diagnostics(&key(), old_token.clone());

        workspace.set_pending_diagnostics(&key(), new_token.clone());

        assert!(old_token.is_cancelled());
        assert!(!new_token.is_cancelled());
    }

    #[test]
    fn eviction_cancels_pending_diagnostics() {
        let token = CancellationToken::new();
        let mut workspace = Workspace::default();
        workspace.ensure(&key());
        workspace.set_pending_diagnostics(&key(), token.clone());

        workspace.evict(&key());

        assert!(token.is_cancelled());
    }

    #[test]
    fn invalidation_moves_the_generation_an_in_flight_build_captured() {
        let mut workspace = Workspace::default();
        workspace.ensure(&key());
        let generation = workspace.generation();

        workspace.invalidate_all();

        assert_ne!(workspace.generation(), generation);
    }

    #[test]
    fn reopening_an_evicted_key_installs_a_different_build_lock() {
        let mut workspace = Workspace::default();
        workspace.ensure(&key());
        let held = workspace.build_lock(&key()).unwrap();

        workspace.evict(&key());
        workspace.ensure(&key());

        let current = workspace.build_lock(&key()).unwrap();
        assert!(
            !Arc::ptr_eq(&current, &held),
            "a waiter on the old lock must be able to tell it is stale"
        );
    }

    #[test]
    fn an_evicted_key_is_not_recreated_by_a_late_caller() {
        let mut workspace = Workspace::default();
        workspace.ensure(&key());
        workspace.evict(&key());

        let token = CancellationToken::new();
        workspace.set_pending_diagnostics(&key(), token.clone());

        assert!(workspace.build_lock(&key()).is_none());
        assert!(workspace.keys().is_empty());
        assert!(token.is_cancelled());
    }
}
