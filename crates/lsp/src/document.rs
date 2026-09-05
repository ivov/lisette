use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::heap;
use crate::protocol::Url;

use crate::state::{AnalysisKey, CancellationToken, DocumentState, SharedState};

impl SharedState {
    pub(crate) fn open_document(self: &Arc<Self>, uri: Url, content: String, version: i32) {
        let mut workspace = self.workspace_mut();
        self.project.update_overlay(&uri, content.clone());
        let key = self.key_for(&uri);
        workspace.invalidate_unseen(&uri, &content);
        let mut document = DocumentState::new(content, version);
        if let Some(key) = &key
            && let Some(snapshot) = workspace.snapshot(key)
            && self.validate(&uri).is_none()
            && snapshot.is_usable(&uri)
        {
            document.set_last_usable(snapshot);
        }
        workspace.documents.insert(uri, document);
        if let Some(key) = key {
            workspace.ensure(&key);
        }
        drop(workspace);

        self.reschedule_all();
    }

    pub(crate) fn change_document(self: &Arc<Self>, uri: Url, content: String, version: i32) {
        let mut workspace = self.workspace_mut();
        self.project.update_overlay(&uri, content.clone());
        let key = self.key_for(&uri);
        match workspace.documents.get_mut(&uri) {
            Some(document) => document.update(content, version),
            None => {
                workspace
                    .documents
                    .insert(uri.clone(), DocumentState::new(content, version));
            }
        }
        if let Some(key) = key {
            workspace.ensure(&key);
        }
        workspace.invalidate_all();
        drop(workspace);

        self.reschedule_all();
    }

    pub(crate) fn close_document(self: &Arc<Self>, uri: &Url) {
        let mut workspace = self.workspace_mut();
        let key = self.key_for(uri);
        self.project.remove_overlay(uri);
        workspace.documents.remove(uri);
        let evicted = key.is_some_and(|key| {
            let still_open = workspace
                .documents
                .keys()
                .any(|open| self.key_for(open).as_ref() == Some(&key));
            if still_open {
                return false;
            }
            workspace.evict(&key);
            true
        });
        workspace.invalidate_all();
        drop(workspace);

        if evicted {
            heap::release_freed_pages();
        }
        self.client.publish_diagnostics(uri.clone(), vec![], None);
        self.reschedule_all();
    }

    pub(crate) fn publish_diagnostics(&self, uri: Url) {
        if uri
            .to_file_path()
            .is_ok_and(|p| deps::is_generated_typedef_path(&p))
        {
            self.client.publish_diagnostics(uri, vec![], None);
            return;
        }

        let (version, generation) = {
            let workspace = self.workspace();
            let Some(document) = workspace.documents.get(&uri) else {
                return;
            };
            (document.version(), workspace.generation())
        };

        let Some(diagnostics) = self.diagnostics_for(&uri) else {
            return;
        };

        let workspace = self.workspace();
        if workspace.generation() != generation || !workspace.documents.contains_key(&uri) {
            return;
        }
        self.client
            .publish_diagnostics(uri, diagnostics, Some(version));
    }

    fn reschedule_all(self: &Arc<Self>) {
        let keys = self.workspace().keys();
        for key in keys {
            self.schedule_diagnostics(key);
        }
    }

    fn schedule_diagnostics(self: &Arc<Self>, key: AnalysisKey) {
        let state = Arc::clone(self);
        let token = CancellationToken::new();
        let run_token = token.clone();
        let run_key = key.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(300));
            if run_token.is_cancelled() {
                return;
            }
            for uri in state.documents_for(&run_key) {
                if run_token.is_cancelled() {
                    return;
                }
                state.publish_diagnostics(uri);
            }
            state
                .workspace_mut()
                .finish_diagnostics(&run_key, &run_token);
        });
        self.workspace_mut().set_pending_diagnostics(&key, token);
    }

    fn documents_for(&self, key: &AnalysisKey) -> Vec<Url> {
        let uris: Vec<Url> = self.workspace().documents.keys().cloned().collect();
        uris.into_iter()
            .filter(|uri| self.key_for(uri).as_ref() == Some(key))
            .collect()
    }
}
