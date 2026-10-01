//! Routes each document to a project with its own configuration and overlays.

use std::collections::HashMap;
use std::sync::{Arc, PoisonError, RwLock};

use crate::loader::{ProjectAnalysis, ProjectState};
use crate::project::{ProjectConfig, find_project_root, resolve_script_root};
use crate::protocol::Url;
use crate::state::AnalysisKey;

#[derive(Default)]
pub(crate) struct ProjectRouter {
    routes: RwLock<Routes>,
}

#[derive(Default)]
struct Routes {
    documents: HashMap<Url, ProjectConfig>,
    projects: HashMap<ProjectConfig, Arc<ProjectState>>,
}

impl ProjectRouter {
    pub(crate) fn clear(&self) {
        *self.routes.write().unwrap_or_else(PoisonError::into_inner) = Routes::default();
    }

    fn project_for(&self, uri: &Url) -> Option<Arc<ProjectState>> {
        {
            let routes = self.routes.read().unwrap_or_else(PoisonError::into_inner);
            if let Some(config) = routes.documents.get(uri) {
                return routes.projects.get(config).cloned();
            }
        }

        let path = uri.to_file_path().ok()?;
        // Preserve the client's path spelling in both the identity and loader.
        // Canonicalizing only one would mix overlays whose document URIs differ.
        let config = find_project_root(&path).unwrap_or_else(|| resolve_script_root(&path));
        let mut routes = self.routes.write().unwrap_or_else(PoisonError::into_inner);
        let config = routes
            .documents
            .entry(uri.clone())
            .or_insert(config)
            .clone();
        let project = routes.projects.entry(config.clone()).or_insert_with(|| {
            let project = ProjectState::new();
            project.initialize(config);
            Arc::new(project)
        });
        Some(Arc::clone(project))
    }

    pub(crate) fn config_for(&self, uri: &Url) -> Option<ProjectConfig> {
        self.project_for(uri)?.config_for(uri)
    }

    pub(crate) fn update_overlay(&self, uri: &Url, content: String) {
        if let Some(project) = self.project_for(uri) {
            project.update_overlay(uri, content);
        }
    }

    pub(crate) fn remove_overlay(&self, uri: &Url) {
        let project = {
            let mut routes = self.routes.write().unwrap_or_else(PoisonError::into_inner);
            let Some(config) = routes.documents.remove(uri) else {
                return;
            };
            let project = routes.projects.get(&config).cloned();
            if !routes.documents.values().any(|open| *open == config) {
                routes.projects.remove(&config);
            }
            project
        };
        if let Some(project) = project {
            project.remove_overlay(uri);
        }
    }

    pub(crate) fn for_key(&self, key: &AnalysisKey) -> Option<ProjectAnalysis> {
        let project = match key {
            AnalysisKey::Document { uri } => self.project_for(uri)?,
            AnalysisKey::Package { project_root, .. } => {
                let config = ProjectConfig::Workspace(project_root.clone());
                self.routes
                    .read()
                    .unwrap_or_else(PoisonError::into_inner)
                    .projects
                    .get(&config)
                    .cloned()?
            }
        };
        project.for_key(key)
    }
}
