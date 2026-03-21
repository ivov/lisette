use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tower_lsp::lsp_types::Url;

use crate::paths::uri_to_module_file;
use crate::position::LineIndex;
use crate::state::{DocumentState, SharedState};

impl SharedState {
    pub(crate) async fn ensure_config(
        &self,
        file_uri: &Url,
    ) -> Option<crate::project::ProjectConfig> {
        if let Some(config) = self.project_config.read().await.as_ref() {
            return Some(config.clone());
        }

        let file_path = file_uri.to_file_path().ok()?;

        let config = crate::project::find_project_root(&file_path)
            .unwrap_or_else(|| crate::project::resolve_standalone_root(&file_path));

        {
            let mut loader = self.loader.write().await;
            loader.set_config(config.clone());
        }

        *self.project_config.write().await = Some(config.clone());
        Some(config)
    }

    pub(crate) async fn update_document(&self, uri: Url, content: String, version: i32) {
        let line_index = LineIndex::new(&content);

        if let Some(config) = self.ensure_config(&uri).await
            && let Some((module_id, filename)) = uri_to_module_file(&config, &uri)
        {
            let mut loader = self.loader.write().await;
            loader.set_overlay(&module_id, &filename, content.clone());
        }

        self.documents.insert(
            uri,
            DocumentState {
                content,
                line_index,
                version,
            },
        );
    }

    pub(crate) async fn publish_diagnostics(&self, uri: Url) {
        let version = self.documents.get(&uri).map(|d| d.version);

        let diagnostics = self.analyze_and_convert(&uri).await;

        let current_version = self.documents.get(&uri).map(|d| d.version);
        if version != current_version {
            return; // Discard stale results
        }

        self.client
            .publish_diagnostics(uri, diagnostics, version)
            .await;
    }

    pub(crate) async fn schedule_diagnostics(self: &Arc<Self>, uri: Url) {
        if let Some((_, (_, old_handle))) = self.pending_diagnostics.remove(&uri) {
            old_handle.abort();
        }

        let generation = self.diagnostics_generation.fetch_add(1, Ordering::Relaxed);
        let state = Arc::clone(self);
        let diagnostics_uri = uri.clone();
        let handle = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            state.publish_diagnostics(diagnostics_uri.clone()).await;
            state
                .pending_diagnostics
                .remove_if(&diagnostics_uri, |_, (g, _)| *g == generation);
        });

        self.pending_diagnostics
            .insert(uri, (generation, handle.abort_handle()));
    }
}
