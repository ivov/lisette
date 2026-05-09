use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use dashmap::DashMap;
use deps::BindgenSetup;
use tokio::task::AbortHandle;
use tower_lsp::Client;
use tower_lsp::lsp_types::Url;

use crate::loader::OverlayLoader;
use crate::position::LineIndex;
use crate::project::ProjectConfig;
use crate::snapshot::AnalysisSnapshot;

pub struct SharedState {
    pub(crate) client: Client,
    pub(crate) project_config: tokio::sync::RwLock<Option<ProjectConfig>>,
    pub(crate) documents: DashMap<Url, DocumentState>,
    pub(crate) loader: tokio::sync::RwLock<OverlayLoader>,
    pub(crate) snapshots: DashMap<Url, CachedSnapshot>,
    pub(crate) last_valid_snapshot: DashMap<Url, Arc<AnalysisSnapshot>>,
    pub(crate) pending_diagnostics: DashMap<Url, (u64, AbortHandle)>,
    pub(crate) diagnostics_generation: AtomicU64,
    pub(crate) bindgen_setup: Option<Arc<dyn BindgenSetup>>,
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

pub(crate) struct CachedSnapshot {
    pub(crate) snapshot: Arc<AnalysisSnapshot>,
    pub(crate) version: i32,
}

pub(crate) struct DocumentState {
    pub(crate) content: String,
    pub(crate) line_index: LineIndex,
    pub(crate) version: i32,
}

impl Backend {
    pub fn new(client: Client, bindgen_setup: Option<Arc<dyn BindgenSetup>>) -> Self {
        let placeholder_config = ProjectConfig {
            root: PathBuf::from("."),
            standalone_mode: true,
        };

        Self {
            shared_state: Arc::new(SharedState {
                client,
                project_config: tokio::sync::RwLock::new(None),
                documents: DashMap::new(),
                loader: tokio::sync::RwLock::new(OverlayLoader::new(placeholder_config)),
                snapshots: DashMap::new(),
                last_valid_snapshot: DashMap::new(),
                pending_diagnostics: DashMap::new(),
                diagnostics_generation: AtomicU64::new(0),
                bindgen_setup,
            }),
        }
    }
}
