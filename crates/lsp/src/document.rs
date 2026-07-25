use std::sync::Arc;
use std::time::Duration;

use tower_lsp::lsp_types::Url;

use crate::state::{DocumentState, SharedState};

impl SharedState {
    pub(crate) async fn update_document(&self, uri: Url, content: String, version: i32) {
        self.project.update_overlay(&uri, content.clone()).await;

        if let Some(mut document) = self.documents.get_mut(&uri) {
            document.update(content, version);
        } else {
            self.documents
                .insert(uri, DocumentState::new(content, version));
        }
    }

    pub(crate) async fn publish_diagnostics(&self, uri: Url) {
        if uri
            .to_file_path()
            .is_ok_and(|p| deps::is_generated_typedef_path(&p))
        {
            self.client.publish_diagnostics(uri, vec![], None).await;
            return;
        }

        let version = self.documents.get(&uri).map(|document| document.version());

        let Some(diagnostics) = self.analyze_and_convert(&uri).await else {
            return;
        };

        let current_version = self.documents.get(&uri).map(|document| document.version());
        if version != current_version {
            return; // Discard stale results
        }

        self.client
            .publish_diagnostics(uri, diagnostics, version)
            .await;
    }

    pub(crate) async fn recheck_open_documents(self: &Arc<Self>) {
        let mut uris = Vec::with_capacity(self.documents.len());
        for mut document in self.documents.iter_mut() {
            document.invalidate_analysis();
            uris.push(document.key().clone());
        }
        for uri in uris {
            self.schedule_diagnostics(uri).await;
        }
    }

    async fn schedule_diagnostics(self: &Arc<Self>, uri: Url) {
        let Some(mut document) = self.documents.get_mut(&uri) else {
            return;
        };

        let state = Arc::clone(self);
        let diagnostics_uri = uri.clone();
        let handle = tokio::spawn(async move {
            let task_id = tokio::task::id();
            tokio::time::sleep(Duration::from_millis(300)).await;
            state.publish_diagnostics(diagnostics_uri.clone()).await;
            if let Some(mut document) = state.documents.get_mut(&diagnostics_uri) {
                document.finish_diagnostics(task_id);
            }
        });
        document.set_pending_diagnostics(handle.abort_handle());
    }
}
