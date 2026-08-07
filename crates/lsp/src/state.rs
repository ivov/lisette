use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::protocol::{Client, Url};
use deps::BindgenSetup;

use crate::imports::PackageIndex;
use crate::loader::ProjectState;
use crate::position::LineIndex;
use crate::snapshot::AnalysisSnapshot;

pub struct SharedState {
    pub(crate) client: Client,
    pub(crate) project: ProjectState,
    documents: RwLock<HashMap<Url, DocumentState>>,
    pub(crate) bindgen_setup: Option<Arc<dyn BindgenSetup>>,
    pub(crate) packages: Arc<PackageIndex>,
    pub(crate) insert_replace_support: AtomicBool,
}

impl SharedState {
    pub(crate) fn documents(&self) -> RwLockReadGuard<'_, HashMap<Url, DocumentState>> {
        self.documents
            .read()
            .unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn documents_mut(&self) -> RwLockWriteGuard<'_, HashMap<Url, DocumentState>> {
        self.documents
            .write()
            .unwrap_or_else(PoisonError::into_inner)
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

impl std::ops::Deref for Backend {
    type Target = SharedState;
    fn deref(&self) -> &SharedState {
        &self.shared_state
    }
}

pub(crate) struct DocumentState {
    line_index: LineIndex,
    version: i32,
    analysis: DocumentAnalysis,
    pending_diagnostics: Option<CancellationToken>,
}

struct DocumentAnalysis {
    revision: AnalysisRevision,
    result: AnalysisResult,
}

#[derive(Clone)]
pub(crate) struct AnalysisRevision(Arc<()>);

pub(crate) enum AnalysisRequest {
    Cached(Arc<AnalysisSnapshot>),
    Required(AnalysisRevision),
}

#[derive(Default)]
enum AnalysisResult {
    #[default]
    Empty,
    Stale(Arc<AnalysisSnapshot>),
    Valid(Arc<AnalysisSnapshot>),
    Invalid {
        current: Arc<AnalysisSnapshot>,
        last_valid: Option<Arc<AnalysisSnapshot>>,
    },
}

impl Default for DocumentAnalysis {
    fn default() -> Self {
        Self {
            revision: AnalysisRevision(Arc::new(())),
            result: AnalysisResult::default(),
        }
    }
}

impl DocumentAnalysis {
    fn current(&self) -> Option<Arc<AnalysisSnapshot>> {
        match &self.result {
            AnalysisResult::Valid(snapshot)
            | AnalysisResult::Invalid {
                current: snapshot, ..
            } => Some(Arc::clone(snapshot)),
            AnalysisResult::Empty | AnalysisResult::Stale(_) => None,
        }
    }

    fn last_valid(&self) -> Option<Arc<AnalysisSnapshot>> {
        match &self.result {
            AnalysisResult::Stale(snapshot) | AnalysisResult::Valid(snapshot) => {
                Some(Arc::clone(snapshot))
            }
            AnalysisResult::Invalid { last_valid, .. } => last_valid.clone(),
            AnalysisResult::Empty => None,
        }
    }

    fn request(&self) -> AnalysisRequest {
        match self.current() {
            Some(snapshot) => AnalysisRequest::Cached(snapshot),
            None => AnalysisRequest::Required(self.revision.clone()),
        }
    }

    fn invalidate(&mut self) {
        self.revision = AnalysisRevision(Arc::new(()));
        let current = std::mem::take(&mut self.result);
        self.result = match current {
            AnalysisResult::Valid(snapshot) | AnalysisResult::Stale(snapshot) => {
                AnalysisResult::Stale(snapshot)
            }
            AnalysisResult::Invalid {
                last_valid: Some(snapshot),
                ..
            } => AnalysisResult::Stale(snapshot),
            AnalysisResult::Empty
            | AnalysisResult::Invalid {
                last_valid: None, ..
            } => AnalysisResult::Empty,
        };
    }

    fn update(&mut self, revision: &AnalysisRevision, snapshot: Arc<AnalysisSnapshot>) -> bool {
        if !Arc::ptr_eq(&self.revision.0, &revision.0) {
            return false;
        }
        if snapshot.has_parse_errors() {
            self.result = AnalysisResult::Invalid {
                current: snapshot,
                last_valid: self.last_valid(),
            };
        } else {
            self.result = AnalysisResult::Valid(snapshot);
        }
        true
    }
}

impl DocumentState {
    pub(crate) fn new(content: String, version: i32) -> Self {
        Self {
            line_index: LineIndex::new(&content),
            version,
            analysis: DocumentAnalysis::default(),
            pending_diagnostics: None,
        }
    }

    pub(crate) fn update(&mut self, content: String, version: i32) {
        self.line_index = LineIndex::new(&content);
        self.version = version;
        self.analysis.invalidate();
    }

    pub(crate) fn content(&self) -> &str {
        self.line_index.source()
    }

    pub(crate) fn line_index(&self) -> &LineIndex {
        &self.line_index
    }

    pub(crate) fn version(&self) -> i32 {
        self.version
    }

    pub(crate) fn request_analysis(&self) -> AnalysisRequest {
        self.analysis.request()
    }

    pub(crate) fn last_valid_analysis(&self) -> Option<Arc<AnalysisSnapshot>> {
        self.analysis.last_valid()
    }

    pub(crate) fn cache_analysis(
        &mut self,
        revision: &AnalysisRevision,
        snapshot: Arc<AnalysisSnapshot>,
    ) -> bool {
        self.analysis.update(revision, snapshot)
    }

    pub(crate) fn invalidate_analysis(&mut self) {
        self.analysis.invalidate();
    }

    pub(crate) fn abort_pending_diagnostics(&mut self) {
        if let Some(token) = self.pending_diagnostics.take() {
            token.cancel();
        }
    }

    pub(crate) fn set_pending_diagnostics(&mut self, token: CancellationToken) {
        self.abort_pending_diagnostics();
        self.pending_diagnostics = Some(token);
    }

    pub(crate) fn finish_diagnostics(&mut self, token: &CancellationToken) {
        if self.pending_diagnostics.as_ref() == Some(token) {
            self.pending_diagnostics = None;
        }
    }
}

impl Drop for DocumentState {
    fn drop(&mut self) {
        self.abort_pending_diagnostics();
    }
}

impl Backend {
    pub(crate) fn new(client: Client, bindgen_setup: Option<Arc<dyn BindgenSetup>>) -> Self {
        Self {
            shared_state: Arc::new(SharedState {
                client,
                project: ProjectState::new(),
                documents: RwLock::new(HashMap::new()),
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

    #[test]
    fn dropping_document_cancels_pending_diagnostics() {
        let token = CancellationToken::new();
        let mut document = DocumentState::new(String::new(), 1);
        document.set_pending_diagnostics(token.clone());

        drop(document);

        assert!(token.is_cancelled());
    }

    #[test]
    fn replacing_pending_diagnostics_cancels_previous_run() {
        let old_token = CancellationToken::new();
        let new_token = CancellationToken::new();
        let mut document = DocumentState::new(String::new(), 1);
        document.set_pending_diagnostics(old_token.clone());

        document.set_pending_diagnostics(new_token.clone());

        assert!(old_token.is_cancelled());
        assert!(!new_token.is_cancelled());
    }

    #[test]
    fn invalidation_rejects_an_in_flight_analysis() {
        let mut document = DocumentState::new(String::new(), 1);
        let AnalysisRequest::Required(revision) = document.request_analysis() else {
            panic!("new document should require analysis");
        };

        document.invalidate_analysis();

        assert!(!Arc::ptr_eq(&document.analysis.revision.0, &revision.0));
    }
}
