use std::sync::Arc;
use std::time::Duration;

use crate::protocol::Url;

use crate::state::{DocumentState, SharedState};

impl SharedState {
    pub(crate) async fn update_document(&self, uri: Url, content: String, version: i32) {
        self.project.update_overlay(&uri, content.clone()).await;

        let mut documents = self.documents.write().await;
        if let Some(document) = documents.get_mut(&uri) {
            document.update(content, version);
        } else {
            documents.insert(uri, DocumentState::new(content, version));
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

        let version = self
            .documents
            .read()
            .await
            .get(&uri)
            .map(DocumentState::version);

        let Some(diagnostics) = self.analyze_and_convert(&uri).await else {
            return;
        };

        let current_version = self
            .documents
            .read()
            .await
            .get(&uri)
            .map(DocumentState::version);
        if version != current_version {
            return; // Discard stale results
        }

        self.client
            .publish_diagnostics(uri, diagnostics, version)
            .await;
    }

    pub(crate) async fn recheck_open_documents(self: &Arc<Self>) {
        let mut documents = self.documents.write().await;
        let mut uris = Vec::with_capacity(documents.len());
        for (uri, document) in documents.iter_mut() {
            document.invalidate_analysis();
            uris.push(uri.clone());
        }
        drop(documents);
        for uri in uris {
            self.schedule_diagnostics(uri).await;
        }
    }

    async fn schedule_diagnostics(self: &Arc<Self>, uri: Url) {
        let mut documents = self.documents.write().await;
        let Some(document) = documents.get_mut(&uri) else {
            return;
        };

        let state = Arc::clone(self);
        let diagnostics_uri = uri.clone();
        let handle = tokio::spawn(async move {
            let task_id = tokio::task::id();
            tokio::time::sleep(Duration::from_millis(300)).await;
            state.publish_diagnostics(diagnostics_uri.clone()).await;
            if let Some(document) = state.documents.write().await.get_mut(&diagnostics_uri) {
                document.finish_diagnostics(task_id);
            }
        });
        document.set_pending_diagnostics(handle.abort_handle());
    }
}
