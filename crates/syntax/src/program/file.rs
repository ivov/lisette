use ecow::EcoString;

use crate::ast::{Expression, ImportAlias, Span};

#[derive(Debug, Clone, PartialEq)]
pub struct File {
    pub id: u32,
    pub module_id: String,
    pub name: String,
    pub source: String,
    pub items: Vec<Expression>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileImport {
    pub name: EcoString,
    pub name_span: Span,
    pub alias: Option<ImportAlias>,
    pub span: Span,
}

impl FileImport {
    pub fn effective_alias(&self) -> Option<String> {
        match &self.alias {
            Some(ImportAlias::Named(name, _)) => Some(name.to_string()),
            Some(ImportAlias::Blank(_)) => None,
            None => Some(
                self.name
                    .strip_prefix("go:")
                    .unwrap_or(&self.name)
                    .split('/')
                    .next_back()
                    .unwrap_or(&self.name)
                    .to_string(),
            ),
        }
    }
}

impl File {
    pub fn new(module_id: &str, name: &str, source: &str, items: Vec<Expression>, id: u32) -> Self {
        File {
            id,
            module_id: module_id.to_string(),
            name: name.to_string(),
            source: source.to_string(),
            items,
        }
    }

    pub fn new_cached(module_id: &str, name: &str, source: &str, id: u32) -> Self {
        Self {
            id,
            module_id: module_id.to_string(),
            name: name.to_string(),
            source: source.to_string(),
            items: vec![],
        }
    }

    pub fn is_d_lis(&self) -> bool {
        self.name.ends_with(".d.lis")
    }

    pub fn is_lis(&self) -> bool {
        !self.is_d_lis()
    }

    pub fn imports(&self) -> Vec<FileImport> {
        self.items
            .iter()
            .filter_map(|item| match item {
                Expression::ModuleImport {
                    name,
                    name_span,
                    alias,
                    span,
                } => Some(FileImport {
                    name: name.clone(),
                    name_span: *name_span,
                    alias: alias.clone(),
                    span: *span,
                }),
                _ => None,
            })
            .collect()
    }

    pub fn go_filename(&self) -> String {
        std::path::Path::new(&self.name)
            .with_extension("go")
            .display()
            .to_string()
    }
}
