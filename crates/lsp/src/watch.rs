//! Refresh analysis after editor-reported changes outside document buffers.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use crate::protocol::{FileChangeType, FileEvent, Url};
use crate::state::SharedState;

impl SharedState {
    pub(crate) fn watched_files_changed(self: &Arc<Self>, changes: Vec<FileEvent>) {
        let mut seen = HashSet::new();
        let mut changed = false;
        let mut configuration_changed = false;
        for event in changes {
            if ![
                FileChangeType::CREATED,
                FileChangeType::CHANGED,
                FileChangeType::DELETED,
            ]
            .contains(&event.kind)
                || !seen.insert(event.uri.clone())
            {
                continue;
            }
            let Ok(path) = event.uri.to_file_path() else {
                continue;
            };
            if !is_analysis_input(&path) {
                continue;
            }
            changed = true;
            configuration_changed |= path.file_name().is_some_and(|name| name == "lisette.toml");
        }
        if changed {
            self.refresh_from_disk(configuration_changed);
        }
    }

    pub(crate) fn save_document(self: &Arc<Self>, uri: &Url) {
        if uri
            .to_file_path()
            .is_ok_and(|path| is_analysis_input(&path))
        {
            // Saving also refreshes configuration for clients without file watching.
            self.refresh_from_disk(true);
        }
    }

    pub(crate) fn refresh_from_disk(self: &Arc<Self>, reload_projects: bool) {
        let mut workspace = self.workspace_mut();
        if workspace.documents.is_empty() {
            return;
        }
        if reload_projects {
            self.projects.clear();
            for (uri, document) in &workspace.documents {
                self.projects
                    .update_overlay(uri, document.content().to_string());
            }
            let keys: HashSet<_> = workspace
                .documents
                .keys()
                .filter_map(|uri| self.key_for(uri))
                .collect();
            for previous in workspace.keys() {
                if !keys.contains(&previous) {
                    workspace.evict(&previous);
                }
            }
            for key in keys {
                workspace.ensure(&key);
            }
        }
        // As with a document edit, reject older builds and retain last_usable.
        workspace.invalidate_all();
        drop(workspace);
        self.reschedule_all();
    }
}

fn is_analysis_input(path: &Path) -> bool {
    if deps::is_generated_typedef_path(path) {
        return false;
    }
    // Bindgen writes these while analyzing. Watching them can feed its own
    // output back into invalidation before that analysis has installed.
    if path.ancestors().any(|ancestor| {
        ancestor.file_name().is_some_and(|name| name == ".lisette")
            && ancestor
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|name| name == "target")
    }) {
        return false;
    }
    path.extension().is_some_and(|extension| extension == "lis")
        || path.file_name().is_some_and(|name| name == "lisette.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watches_sources_and_manifests_but_not_generated_cache_files() {
        assert!(is_analysis_input(Path::new("project/src/main.lis")));
        assert!(is_analysis_input(Path::new("project/src/api.d.lis")));
        assert!(is_analysis_input(Path::new("project/lisette.toml")));
        assert!(is_analysis_input(Path::new("project/src/target/main.lis")));
        assert!(!is_analysis_input(Path::new(
            "project/target/.lisette/typedefs/api.d.lis"
        )));
        assert!(!is_analysis_input(Path::new(
            "project/target/.lisette/lisette.toml"
        )));
        assert!(!is_analysis_input(Path::new("project/README.md")));
    }
}
