use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::protocol::Url;

use crate::state::{CancellationToken, DocumentState, SharedState};

impl SharedState {
    pub(crate) fn update_document(&self, uri: Url, content: String, version: i32) {
        self.project.update_overlay(&uri, content.clone());

        let mut documents = self.documents_mut();
        if let Some(document) = documents.get_mut(&uri) {
            document.update(content, version);
        } else {
            documents.insert(uri, DocumentState::new(content, version));
        }
    }

    pub(crate) fn publish_diagnostics(&self, uri: Url) {
        if uri
            .to_file_path()
            .is_ok_and(|p| deps::is_generated_typedef_path(&p))
        {
            self.client.publish_diagnostics(uri, vec![], None);
            return;
        }

        let version = self.documents().get(&uri).map(DocumentState::version);

        let Some(diagnostics) = self.analyze_and_convert(&uri) else {
            return;
        };

        let current_version = self.documents().get(&uri).map(DocumentState::version);
        if version != current_version {
            return; // Discard stale results
        }

        self.client.publish_diagnostics(uri, diagnostics, version);
    }

    pub(crate) fn recheck_open_documents(self: &Arc<Self>) {
        let mut documents = self.documents_mut();
        let mut uris = Vec::with_capacity(documents.len());
        for (uri, document) in documents.iter_mut() {
            document.invalidate_analysis();
            uris.push(uri.clone());
        }
        drop(documents);
        for uri in uris {
            self.schedule_diagnostics(uri);
        }
    }

    fn schedule_diagnostics(self: &Arc<Self>, uri: Url) {
        let mut documents = self.documents_mut();
        let Some(document) = documents.get_mut(&uri) else {
            return;
        };

        let state = Arc::clone(self);
        let diagnostics_uri = uri.clone();
        let token = CancellationToken::new();
        let run_token = token.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(300));
            if run_token.is_cancelled() {
                return;
            }
            state.publish_diagnostics(diagnostics_uri.clone());
            if let Some(document) = state.documents_mut().get_mut(&diagnostics_uri) {
                document.finish_diagnostics(&run_token);
            }
        });
        document.set_pending_diagnostics(token);
    }
}
