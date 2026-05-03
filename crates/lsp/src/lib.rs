mod analysis;
mod completion;
mod definition;
mod document;
mod hover;
mod loader;
mod paths;
mod position;
mod project;
mod signature_help;
mod snapshot;
mod state;
mod traversal;
mod validation;

use tower_lsp::LanguageServer;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;

use crate::analysis::{offset_in_span, type_name};
use crate::completion::{
    DotContext, definition_to_completion_kind, detect_dot_context, get_instance_completions,
    get_module_prefix, get_type_completions, resolve_variable_type,
};
use crate::definition::{
    find_struct_field_span, lookup_definition_span, resolve_definition_span,
    resolve_dot_access_definition, resolve_enum_in_pattern, resolve_import_span,
    resolve_match_pattern_definition, resolve_struct_call_field, resolve_word_at_offset,
    word_at_offset,
};
use crate::paths::uri_to_module_file;
use crate::project::find_project_root;
use crate::snapshot::AnalysisSnapshot;
use crate::traversal::find_expression_at;

pub use crate::state::{Backend, SharedState};

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let workspace_root = params
            .root_uri
            .and_then(|uri| uri.to_file_path().ok())
            .or_else(|| {
                params
                    .workspace_folders
                    .as_ref()?
                    .first()?
                    .uri
                    .to_file_path()
                    .ok()
            });

        if let Some(root) = workspace_root
            && let Some(config) = find_project_root(&root)
        {
            {
                let mut loader = self.loader.write().await;
                loader.set_config(config.clone());
            }
            *self.project_config.write().await = Some(config);
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                document_formatting_provider: Some(OneOf::Left(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Right(RenameOptions {
                    prepare_provider: Some(true),
                    work_done_progress_options: Default::default(),
                })),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![".".to_string()]),
                    ..Default::default()
                }),
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
                    retrigger_characters: None,
                    work_done_progress_options: Default::default(),
                }),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Lisette LSP initialized")
            .await;
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let content = params.text_document.text;
        self.snapshots.remove(&uri);
        self.update_document(uri.clone(), content, params.text_document.version)
            .await;
        self.publish_diagnostics(uri).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Some(change) = params.content_changes.into_iter().last() {
            self.snapshots.clear();
            self.update_document(uri.clone(), change.text, params.text_document.version)
                .await;
            self.shared_state.schedule_diagnostics(uri).await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        self.publish_diagnostics(params.text_document.uri).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = &params.text_document.uri;
        if let Some((_, (_, handle))) = self.pending_diagnostics.remove(uri) {
            handle.abort();
        }
        self.documents.remove(uri);
        self.snapshots.remove(uri);
        self.last_valid_snapshot.remove(uri);

        if let Some(config) = self.project_config.read().await.as_ref()
            && let Some((module_id, filename)) = uri_to_module_file(config, uri)
        {
            let mut loader = self.loader.write().await;
            loader.remove_overlay(&module_id, &filename);
        }

        self.client
            .publish_diagnostics(uri.clone(), vec![], None)
            .await;
    }

    async fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let uri = &params.text_document.uri;
        let (source, end_position) = {
            let Some(doc) = self.documents.get(uri) else {
                return Ok(None);
            };
            let end = doc.line_index.offset_to_position(doc.content.len() as u32);
            (doc.content.clone(), end)
        };

        let formatted = match format::format_source(&source) {
            Ok(formatted) => formatted,
            Err(_parse_errors) => {
                self.client
                    .log_message(MessageType::WARNING, "Cannot format: file has parse errors")
                    .await;
                return Ok(None);
            }
        };

        if formatted == source {
            return Ok(None);
        }

        Ok(Some(vec![TextEdit {
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: end_position,
            },
            new_text: formatted,
        }]))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let Some(snapshot) = self.get_snapshot(uri).await else {
            return Ok(None);
        };
        let Some(file_id) = snapshot.get_file_id(uri) else {
            return Ok(None);
        };
        let Some(file) = snapshot.files().get(&file_id) else {
            return Ok(None);
        };
        let Some(line_index) = snapshot.get_line_index(file_id) else {
            return Ok(None);
        };

        let Some(offset) = line_index.position_to_offset(position) else {
            return Ok(None);
        };

        let Some(expression) = find_expression_at(&file.items, offset) else {
            return Ok(None);
        };

        let (ty, span) = hover::get_hover_type_and_span(expression, offset);

        if ty.is_type_var() || ty.is_error() {
            return Ok(None);
        }

        let doc = hover::get_hover_doc(expression, offset, file, &snapshot);

        let content = match doc {
            Some(doc) => format!("```lisette\n{ty}\n```\n\n---\n\n{doc}"),
            None => format!("```lisette\n{ty}\n```"),
        };

        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: content,
            }),
            range: Some(line_index.span_to_range(span)),
        }))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let Some(snapshot) = self.get_snapshot(uri).await else {
            return Ok(None);
        };
        let Some(file_id) = snapshot.get_file_id(uri) else {
            return Ok(None);
        };
        let Some(file) = snapshot.files().get(&file_id) else {
            return Ok(None);
        };
        let Some(line_index) = snapshot.get_line_index(file_id) else {
            return Ok(None);
        };

        let Some(offset) = line_index.position_to_offset(position) else {
            return Ok(None);
        };

        let Some(expression) = find_expression_at(&file.items, offset) else {
            return Ok(None);
        };

        let find_binding = || {
            snapshot
                .facts()
                .bindings
                .values()
                .find(|b| b.span.file_id == file_id && offset_in_span(offset, &b.span))
                .map(|b| b.span)
        };

        let definition_span = match expression {
            syntax::ast::Expression::Identifier {
                binding_id: Some(id),
                ..
            } => snapshot.facts().bindings.get(id).map(|b| b.span),

            syntax::ast::Expression::Identifier {
                value,
                qualified: Some(qname),
                span: id_span,
                ..
            } => {
                if value.contains('.') {
                    let cursor_in_value = offset.saturating_sub(id_span.byte_offset) as usize;
                    let prefix = &value.as_str()[..cursor_in_value.min(value.len())];
                    if !prefix.contains('.') {
                        let first = value.split('.').next().unwrap_or(value);
                        if let Some(span) =
                            lookup_definition_span(first, file, &snapshot).or_else(|| {
                                resolve_import_span(first, file, &snapshot.result.go_package_names)
                            })
                            && let Some(uri) = snapshot.get_uri(span.file_id)
                            && let Some(idx) = snapshot.get_line_index(span.file_id)
                        {
                            return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                                uri: uri.clone(),
                                range: idx.span_to_range(span),
                            })));
                        }
                    }
                }
                snapshot
                    .definitions()
                    .get(qname.as_str())
                    .and_then(|d| d.name_span())
            }

            syntax::ast::Expression::DotAccess {
                expression, member, ..
            } => resolve_dot_access_definition(expression, member, file, &snapshot),

            syntax::ast::Expression::StructCall {
                name,
                field_assignments,
                ty,
                ..
            } => resolve_struct_call_field(field_assignments, name, ty, offset, file, &snapshot),

            syntax::ast::Expression::Function { name_span, .. }
                if offset_in_span(offset, name_span) =>
            {
                Some(*name_span)
            }

            syntax::ast::Expression::Interface { name_span, .. }
                if offset_in_span(offset, name_span) =>
            {
                Some(*name_span)
            }

            syntax::ast::Expression::Struct {
                name,
                name_span,
                fields,
                ..
            } => fields
                .iter()
                .find(|f| offset_in_span(offset, &f.name_span))
                .and_then(|f| {
                    let qualified = format!("{}.{}", file.module_id, name);
                    find_struct_field_span(&qualified, &f.name, &snapshot)
                })
                .or_else(|| offset_in_span(offset, name_span).then_some(*name_span)),

            syntax::ast::Expression::Enum {
                name,
                name_span,
                variants,
                ..
            } => variants
                .iter()
                .find(|v| offset_in_span(offset, &v.name_span))
                .and_then(|v| {
                    let qualified = format!("{}.{}.{}", file.module_id, name, v.name);
                    snapshot
                        .definitions()
                        .get(qualified.as_str())
                        .and_then(|d| d.name_span())
                })
                .or_else(|| offset_in_span(offset, name_span).then_some(*name_span)),

            syntax::ast::Expression::Identifier { value, .. } => {
                lookup_definition_span(value, file, &snapshot)
                    .or_else(|| resolve_import_span(value, file, &snapshot.result.go_package_names))
            }

            syntax::ast::Expression::Match { arms, .. } => {
                resolve_match_pattern_definition(arms, offset, file, &snapshot)
                    .or_else(&find_binding)
                    .or_else(|| resolve_word_at_offset(&file.source, offset, file, &snapshot))
            }

            syntax::ast::Expression::IfLet {
                pattern,
                typed_pattern,
                ..
            }
            | syntax::ast::Expression::WhileLet {
                pattern,
                typed_pattern,
                ..
            } => resolve_enum_in_pattern(pattern, typed_pattern.as_ref(), offset, file, &snapshot)
                .or_else(&find_binding)
                .or_else(|| resolve_word_at_offset(&file.source, offset, file, &snapshot)),

            _ => find_binding()
                .or_else(|| resolve_word_at_offset(&file.source, offset, file, &snapshot)),
        };

        let Some(definition_span) = definition_span else {
            return Ok(None);
        };

        if let Some(target_file) = snapshot.files().get(&definition_span.file_id) {
            let end = (definition_span.byte_offset as usize)
                .saturating_add(definition_span.byte_length as usize);
            if end > target_file.source.len() {
                return Ok(None);
            }
        }

        let Some(target_uri) = snapshot.get_uri(definition_span.file_id) else {
            return Ok(None);
        };
        let Some(target_line_index) = snapshot.get_line_index(definition_span.file_id) else {
            return Ok(None);
        };

        let range = target_line_index.span_to_range(definition_span);

        Ok(Some(GotoDefinitionResponse::Scalar(Location {
            uri: target_uri.clone(),
            range,
        })))
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        use tower_lsp::lsp_types::{DocumentSymbol, SymbolKind};

        let uri = &params.text_document.uri;

        let Some(snapshot) = self.get_snapshot(uri).await else {
            return Ok(None);
        };
        let Some(file_id) = snapshot.get_file_id(uri) else {
            return Ok(None);
        };
        let Some(file) = snapshot.files().get(&file_id) else {
            return Ok(None);
        };
        let Some(line_index) = snapshot.get_line_index(file_id) else {
            return Ok(None);
        };

        fn expression_to_symbol(
            expression: &syntax::ast::Expression,
            line_index: &crate::position::LineIndex,
        ) -> Option<DocumentSymbol> {
            use syntax::ast::Expression;

            let (name, name_span, span, kind, detail) = match expression {
                Expression::Function {
                    name,
                    name_span,
                    ty,
                    span,
                    ..
                } => (
                    name,
                    name_span,
                    span,
                    SymbolKind::FUNCTION,
                    Some(ty.to_string()),
                ),
                Expression::Struct {
                    name,
                    name_span,
                    span,
                    ..
                } => (name, name_span, span, SymbolKind::STRUCT, None),
                Expression::Enum {
                    name,
                    name_span,
                    span,
                    ..
                }
                | Expression::ValueEnum {
                    name,
                    name_span,
                    span,
                    ..
                } => (name, name_span, span, SymbolKind::ENUM, None),
                Expression::Interface {
                    name,
                    name_span,
                    span,
                    ..
                } => (name, name_span, span, SymbolKind::INTERFACE, None),
                Expression::TypeAlias {
                    name,
                    name_span,
                    span,
                    ..
                } => (name, name_span, span, SymbolKind::CLASS, None),
                Expression::Const {
                    identifier,
                    identifier_span,
                    ty,
                    span,
                    ..
                } => (
                    identifier,
                    identifier_span,
                    span,
                    SymbolKind::CONSTANT,
                    Some(ty.to_string()),
                ),
                Expression::VariableDeclaration {
                    name,
                    name_span,
                    ty,
                    span,
                    ..
                } => (
                    name,
                    name_span,
                    span,
                    SymbolKind::VARIABLE,
                    Some(ty.to_string()),
                ),
                _ => return None,
            };

            #[allow(deprecated)]
            Some(DocumentSymbol {
                name: name.to_string(),
                detail,
                kind,
                tags: None,
                deprecated: None,
                range: line_index.span_to_range(*span),
                selection_range: line_index.span_to_range(*name_span),
                children: None,
            })
        }

        let symbols: Vec<DocumentSymbol> = file
            .items
            .iter()
            .filter_map(|item| expression_to_symbol(item, line_index))
            .collect();

        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        let Some(snapshot) = self.get_snapshot(uri).await else {
            return Ok(None);
        };
        let Some(file_id) = snapshot.get_file_id(uri) else {
            return Ok(None);
        };
        let Some(file) = snapshot.files().get(&file_id) else {
            return Ok(None);
        };
        let Some(line_index) = snapshot.get_line_index(file_id) else {
            return Ok(None);
        };

        let Some(offset) = line_index.position_to_offset(position) else {
            return Ok(None);
        };

        let definition_span = resolve_definition_span(
            &snapshot,
            file,
            file_id,
            offset,
            |expression| match expression {
                syntax::ast::Expression::Identifier {
                    qualified: Some(qname),
                    ..
                } => snapshot
                    .definitions()
                    .get(qname.as_str())
                    .and_then(|d| d.name_span()),

                syntax::ast::Expression::DotAccess {
                    expression, member, ..
                } => resolve_dot_access_definition(expression, member, file, &snapshot),

                syntax::ast::Expression::Match { arms, .. } => {
                    resolve_match_pattern_definition(arms, offset, file, &snapshot)
                        .or_else(|| resolve_word_at_offset(&file.source, offset, file, &snapshot))
                }

                syntax::ast::Expression::IfLet {
                    pattern,
                    typed_pattern,
                    ..
                }
                | syntax::ast::Expression::WhileLet {
                    pattern,
                    typed_pattern,
                    ..
                } => resolve_enum_in_pattern(
                    pattern,
                    typed_pattern.as_ref(),
                    offset,
                    file,
                    &snapshot,
                )
                .or_else(|| resolve_word_at_offset(&file.source, offset, file, &snapshot)),

                _ => resolve_word_at_offset(&file.source, offset, file, &snapshot),
            },
        );

        let Some(definition_span) = definition_span else {
            return Ok(None);
        };

        let Some(definition_uri) = snapshot.get_uri(definition_span.file_id).cloned() else {
            return Ok(None);
        };

        let mut locations = Vec::new();

        if params.context.include_declaration
            && let Some(definition_line_index) = snapshot.get_line_index(definition_span.file_id)
        {
            locations.push(Location {
                uri: definition_uri.clone(),
                range: definition_line_index.span_to_range(definition_span),
            });
        }

        for entry in self.snapshots.iter() {
            let snap = &entry.value().snapshot;
            let Some(target_file_id) = snap.get_file_id(&definition_uri) else {
                continue;
            };
            let target_span = syntax::ast::Span::new(
                target_file_id,
                definition_span.byte_offset,
                definition_span.byte_length,
            );
            for usage in &snap.facts().usages {
                if usage.definition_span == target_span
                    && let Some(usage_uri) = snap.get_uri(usage.usage_span.file_id)
                    && let Some(usage_line_index) = snap.get_line_index(usage.usage_span.file_id)
                {
                    locations.push(Location {
                        uri: usage_uri.clone(),
                        range: usage_line_index.span_to_range(usage.usage_span),
                    });
                }
            }
        }

        locations.sort_by(|a, b| {
            a.uri
                .as_str()
                .cmp(b.uri.as_str())
                .then_with(|| a.range.start.line.cmp(&b.range.start.line))
                .then_with(|| a.range.start.character.cmp(&b.range.start.character))
        });
        locations.dedup_by(|a, b| a.uri == b.uri && a.range == b.range);

        if locations.is_empty() {
            Ok(None)
        } else {
            Ok(Some(locations))
        }
    }

    async fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        let uri = &params.text_document.uri;
        let position = params.position;

        let Some(snapshot) = self.get_snapshot(uri).await else {
            return Ok(None);
        };
        let Some(file_id) = snapshot.get_file_id(uri) else {
            return Ok(None);
        };
        let Some(file) = snapshot.files().get(&file_id) else {
            return Ok(None);
        };
        let Some(line_index) = snapshot.get_line_index(file_id) else {
            return Ok(None);
        };
        let Some(offset) = line_index.position_to_offset(position) else {
            return Ok(None);
        };

        for binding in snapshot.facts().bindings.values() {
            let span = binding.span;
            if span.file_id == file_id && offset_in_span(offset, &span) {
                return Ok(Some(PrepareRenameResponse::RangeWithPlaceholder {
                    range: line_index.span_to_range(span),
                    placeholder: binding.name.clone(),
                }));
            }
        }

        let Some(expression) = find_expression_at(&file.items, offset) else {
            return Ok(None);
        };

        if let syntax::ast::Expression::StructCall {
            field_assignments,
            ty,
            ..
        } = expression
            && let Some(fa) = field_assignments
                .iter()
                .find(|fa| offset_in_span(offset, &fa.name_span))
            && type_name(ty)
                .and_then(|type_id| find_struct_field_span(&type_id, &fa.name, &snapshot))
                .is_some()
        {
            return Ok(Some(PrepareRenameResponse::RangeWithPlaceholder {
                range: line_index.span_to_range(fa.name_span),
                placeholder: fa.name.to_string(),
            }));
        }

        match expression {
            syntax::ast::Expression::Identifier {
                value,
                binding_id: Some(id),
                span,
                ..
            } => {
                if let Some(binding) = snapshot.facts().bindings.get(id)
                    && binding.span.file_id == file_id
                {
                    Ok(Some(PrepareRenameResponse::RangeWithPlaceholder {
                        range: line_index.span_to_range(*span),
                        placeholder: value.to_string(),
                    }))
                } else {
                    Ok(None)
                }
            }

            syntax::ast::Expression::Identifier {
                value,
                qualified: Some(qname),
                span,
                ..
            } => {
                validation::check_rename_guards(qname.as_str())?;
                if snapshot.definitions().contains_key(qname.as_str()) {
                    let short_name = syntax::types::unqualified_name(value);
                    Ok(Some(PrepareRenameResponse::RangeWithPlaceholder {
                        range: line_index.span_to_range(*span),
                        placeholder: short_name.to_string(),
                    }))
                } else {
                    Ok(None)
                }
            }

            syntax::ast::Expression::Function {
                name, name_span, ..
            }
            | syntax::ast::Expression::Interface {
                name, name_span, ..
            }
            | syntax::ast::Expression::TypeAlias {
                name, name_span, ..
            } => {
                let qname = format!("{}.{}", file.module_id, name);
                validation::check_rename_guards(&qname)?;
                Ok(Some(PrepareRenameResponse::RangeWithPlaceholder {
                    range: line_index.span_to_range(*name_span),
                    placeholder: name.to_string(),
                }))
            }

            syntax::ast::Expression::Struct {
                name,
                name_span,
                fields,
                ..
            } => {
                let qname = format!("{}.{}", file.module_id, name);
                if let Some(field) = fields.iter().find(|f| offset_in_span(offset, &f.name_span))
                    && find_struct_field_span(&qname, &field.name, &snapshot).is_some()
                {
                    validation::check_rename_guards(&qname)?;
                    return Ok(Some(PrepareRenameResponse::RangeWithPlaceholder {
                        range: line_index.span_to_range(field.name_span),
                        placeholder: field.name.to_string(),
                    }));
                }
                validation::check_rename_guards(&qname)?;
                Ok(Some(PrepareRenameResponse::RangeWithPlaceholder {
                    range: line_index.span_to_range(*name_span),
                    placeholder: name.to_string(),
                }))
            }

            syntax::ast::Expression::Enum {
                name,
                name_span,
                variants,
                ..
            } => {
                if let Some(variant) = variants
                    .iter()
                    .find(|v| offset_in_span(offset, &v.name_span))
                {
                    let qname = format!("{}.{}.{}", file.module_id, name, variant.name);
                    validation::check_rename_guards(&qname)?;
                    return Ok(Some(PrepareRenameResponse::RangeWithPlaceholder {
                        range: line_index.span_to_range(variant.name_span),
                        placeholder: variant.name.to_string(),
                    }));
                }
                let qualified_name = format!("{}.{}", file.module_id, name);
                validation::check_rename_guards(&qualified_name)?;
                Ok(Some(PrepareRenameResponse::RangeWithPlaceholder {
                    range: line_index.span_to_range(*name_span),
                    placeholder: name.to_string(),
                }))
            }

            syntax::ast::Expression::ValueEnum {
                name,
                name_span,
                variants,
                ..
            } => {
                if let Some(variant) = variants
                    .iter()
                    .find(|v| offset_in_span(offset, &v.name_span))
                {
                    let qname = format!("{}.{}.{}", file.module_id, name, variant.name);
                    validation::check_rename_guards(&qname)?;
                    return Ok(Some(PrepareRenameResponse::RangeWithPlaceholder {
                        range: line_index.span_to_range(variant.name_span),
                        placeholder: variant.name.to_string(),
                    }));
                }
                let qname = format!("{}.{}", file.module_id, name);
                validation::check_rename_guards(&qname)?;
                Ok(Some(PrepareRenameResponse::RangeWithPlaceholder {
                    range: line_index.span_to_range(*name_span),
                    placeholder: name.to_string(),
                }))
            }

            syntax::ast::Expression::VariableDeclaration {
                name, name_span, ..
            } => Ok(Some(PrepareRenameResponse::RangeWithPlaceholder {
                range: line_index.span_to_range(*name_span),
                placeholder: name.to_string(),
            })),

            syntax::ast::Expression::Const {
                identifier,
                identifier_span,
                ..
            } => {
                let qname = format!("{}.{}", file.module_id, identifier);
                validation::check_rename_guards(&qname)?;
                Ok(Some(PrepareRenameResponse::RangeWithPlaceholder {
                    range: line_index.span_to_range(*identifier_span),
                    placeholder: identifier.to_string(),
                }))
            }

            syntax::ast::Expression::DotAccess {
                expression,
                member,
                span,
                ..
            } if !member.is_empty() => {
                if resolve_dot_access_definition(expression, member, file, &snapshot).is_some() {
                    let member_span = syntax::ast::Span::new(
                        span.file_id,
                        span.byte_offset + span.byte_length - member.len() as u32,
                        member.len() as u32,
                    );
                    Ok(Some(PrepareRenameResponse::RangeWithPlaceholder {
                        range: line_index.span_to_range(member_span),
                        placeholder: member.to_string(),
                    }))
                } else {
                    Ok(None)
                }
            }

            syntax::ast::Expression::Match { arms, .. } => {
                if resolve_match_pattern_definition(arms, offset, file, &snapshot).is_some()
                    && let Some((word, start, end)) = word_at_offset(&file.source, offset)
                {
                    let span = syntax::ast::Span::new(file_id, start as u32, (end - start) as u32);
                    return Ok(Some(PrepareRenameResponse::RangeWithPlaceholder {
                        range: line_index.span_to_range(span),
                        placeholder: word.to_string(),
                    }));
                }
                Ok(None)
            }

            syntax::ast::Expression::IfLet {
                pattern,
                typed_pattern,
                ..
            }
            | syntax::ast::Expression::WhileLet {
                pattern,
                typed_pattern,
                ..
            } => {
                if resolve_enum_in_pattern(pattern, typed_pattern.as_ref(), offset, file, &snapshot)
                    .is_some()
                    && let Some((word, start, end)) = word_at_offset(&file.source, offset)
                {
                    let span = syntax::ast::Span::new(file_id, start as u32, (end - start) as u32);
                    return Ok(Some(PrepareRenameResponse::RangeWithPlaceholder {
                        range: line_index.span_to_range(span),
                        placeholder: word.to_string(),
                    }));
                }
                Ok(None)
            }

            _ => {
                if let Some((word, start, end)) = word_at_offset(&file.source, offset)
                    && lookup_definition_span(word, file, &snapshot).is_some()
                {
                    let span = syntax::ast::Span::new(file_id, start as u32, (end - start) as u32);
                    Ok(Some(PrepareRenameResponse::RangeWithPlaceholder {
                        range: line_index.span_to_range(span),
                        placeholder: word.to_string(),
                    }))
                } else {
                    Ok(None)
                }
            }
        }
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let new_name = params.new_name;

        validation::validate_rename(&new_name).map_err(validation::rename_error)?;

        let Some(snapshot) = self.get_snapshot(uri).await else {
            return Ok(None);
        };
        let Some(file_id) = snapshot.get_file_id(uri) else {
            return Ok(None);
        };
        let Some(file) = snapshot.files().get(&file_id) else {
            return Ok(None);
        };
        let Some(line_index) = snapshot.get_line_index(file_id) else {
            return Ok(None);
        };
        let Some(offset) = line_index.position_to_offset(position) else {
            return Ok(None);
        };

        let mut edits: std::collections::HashMap<Url, Vec<TextEdit>> =
            std::collections::HashMap::new();

        let definition_span = resolve_definition_span(
            &snapshot,
            file,
            file_id,
            offset,
            |expression| match expression {
                syntax::ast::Expression::Identifier {
                    qualified: Some(qname),
                    ..
                } => {
                    if validation::check_rename_guards(qname.as_str()).is_err() {
                        return None;
                    }
                    snapshot
                        .definitions()
                        .get(qname.as_str())
                        .and_then(|d| d.name_span())
                }

                syntax::ast::Expression::DotAccess {
                    expression, member, ..
                } => resolve_dot_access_definition(expression, member, file, &snapshot),

                syntax::ast::Expression::Match { arms, .. } => {
                    resolve_match_pattern_definition(arms, offset, file, &snapshot)
                        .or_else(|| resolve_word_at_offset(&file.source, offset, file, &snapshot))
                }

                syntax::ast::Expression::IfLet {
                    pattern,
                    typed_pattern,
                    ..
                }
                | syntax::ast::Expression::WhileLet {
                    pattern,
                    typed_pattern,
                    ..
                } => resolve_enum_in_pattern(
                    pattern,
                    typed_pattern.as_ref(),
                    offset,
                    file,
                    &snapshot,
                )
                .or_else(|| resolve_word_at_offset(&file.source, offset, file, &snapshot)),

                _ => resolve_word_at_offset(&file.source, offset, file, &snapshot),
            },
        );

        let Some(definition_span) = definition_span else {
            return Ok(None);
        };

        let Some(definition_uri) = snapshot.get_uri(definition_span.file_id).cloned() else {
            return Ok(None);
        };

        if let Some(definition_line_index) = snapshot.get_line_index(definition_span.file_id) {
            edits
                .entry(definition_uri.clone())
                .or_default()
                .push(TextEdit {
                    range: definition_line_index.span_to_range(definition_span),
                    new_text: new_name.clone(),
                });
        }

        for entry in self.snapshots.iter() {
            let snap = &entry.value().snapshot;
            let Some(target_file_id) = snap.get_file_id(&definition_uri) else {
                continue;
            };
            let target_span = syntax::ast::Span::new(
                target_file_id,
                definition_span.byte_offset,
                definition_span.byte_length,
            );
            for usage in &snap.facts().usages {
                if usage.definition_span == target_span
                    && let Some(usage_uri) = snap.get_uri(usage.usage_span.file_id)
                    && let Some(usage_line_index) = snap.get_line_index(usage.usage_span.file_id)
                {
                    let replace_span = trailing_segment_span(usage.usage_span, target_span, snap);

                    edits.entry(usage_uri.clone()).or_default().push(TextEdit {
                        range: usage_line_index.span_to_range(replace_span),
                        new_text: new_name.clone(),
                    });
                }
            }
        }

        if edits.is_empty() {
            return Ok(None);
        }

        Ok(Some(WorkspaceEdit {
            changes: Some(edits),
            ..Default::default()
        }))
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        let Some(snapshot) = self.get_snapshot(uri).await else {
            return Ok(None);
        };
        let Some(file_id) = snapshot.get_file_id(uri) else {
            return Ok(None);
        };
        let Some(file) = snapshot.files().get(&file_id) else {
            return Ok(None);
        };
        let Some(line_index) = snapshot.get_line_index(file_id) else {
            return Ok(None);
        };
        let Some(offset) = line_index.position_to_offset(position) else {
            return Ok(None);
        };

        if let Some(module_name) = get_module_prefix(&file.source, offset as usize)
            && let Some(imp) = file.imports().iter().find(|imp| {
                imp.effective_alias(&snapshot.result.go_package_names)
                    .as_deref()
                    == Some(module_name)
            })
        {
            let mut items = Vec::new();
            for (qname, definition) in snapshot.definitions().iter() {
                if let Some(rest) = qname.strip_prefix(imp.name.as_str())
                    && let Some(name) = rest.strip_prefix('.')
                    && !name.contains('.')
                    && definition.visibility().is_public()
                {
                    items.push(CompletionItem {
                        label: name.to_string(),
                        kind: Some(definition_to_completion_kind(definition)),
                        detail: Some(definition.ty().to_string()),
                        ..Default::default()
                    });
                }
            }
            return Ok(Some(CompletionResponse::Array(items)));
        }

        if let Some(ctx) = detect_dot_context(file, offset, &snapshot) {
            let items = match ctx {
                DotContext::Instance(type_id) => {
                    let same_module = type_id.starts_with(file.module_id.as_str())
                        && type_id.as_bytes().get(file.module_id.len()) == Some(&b'.');
                    get_instance_completions(&type_id, &snapshot, same_module)
                }
                DotContext::TypeLevel(type_id) => {
                    let same_module = type_id.starts_with(file.module_id.as_str())
                        && type_id.as_bytes().get(file.module_id.len()) == Some(&b'.');
                    get_type_completions(&type_id, &snapshot, same_module)
                }
            };
            return Ok(Some(CompletionResponse::Array(items)));
        }

        if let Some(prefix) = get_module_prefix(&file.source, offset as usize) {
            if prefix == "self" {
                if let Some(impl_type) = traversal::find_enclosing_impl_type(&file.items, offset) {
                    let type_id = format!("{}.{}", file.module_id, impl_type);
                    let items = get_instance_completions(&type_id, &snapshot, true);
                    return Ok(Some(CompletionResponse::Array(items)));
                }
            } else {
                for module in [file.module_id.as_str(), "prelude"] {
                    let qualified = format!("{module}.{prefix}");
                    if let Some(definition) = snapshot.definitions().get(qualified.as_str())
                        && definition.is_type_definition()
                    {
                        let same_module = module == file.module_id.as_str();
                        let items = get_type_completions(&qualified, &snapshot, same_module);
                        return Ok(Some(CompletionResponse::Array(items)));
                    }
                }

                for import in file.imports() {
                    let qualified = format!("{}.{}", import.name, prefix);
                    if let Some(definition) = snapshot.definitions().get(qualified.as_str())
                        && definition.is_type_definition()
                        && definition.visibility().is_public()
                    {
                        let items = get_type_completions(&qualified, &snapshot, false);
                        return Ok(Some(CompletionResponse::Array(items)));
                    }
                }

                let indexed =
                    offset as usize >= 2 && file.source.as_bytes()[offset as usize - 2] == b']';
                if let Some(type_id) =
                    resolve_variable_type(prefix, file, offset, &snapshot, indexed)
                {
                    let same_module = type_id.starts_with(file.module_id.as_str())
                        && type_id.as_bytes().get(file.module_id.len()) == Some(&b'.');
                    let items = get_instance_completions(&type_id, &snapshot, same_module);
                    return Ok(Some(CompletionResponse::Array(items)));
                }
            }

            return Ok(Some(CompletionResponse::Array(vec![])));
        }

        let mut items = Vec::new();

        const KEYWORDS: &[&str] = &[
            "fn",
            "let",
            "if",
            "else",
            "match",
            "enum",
            "struct",
            "type",
            "interface",
            "impl",
            "const",
            "return",
            "defer",
            "import",
            "mut",
            "pub",
            "for",
            "in",
            "while",
            "loop",
            "break",
            "continue",
            "select",
            "task",
            "try",
            "recover",
            "as",
            "true",
            "false",
        ];
        for kw in KEYWORDS {
            items.push(CompletionItem {
                label: kw.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                ..Default::default()
            });
        }

        const PRELUDE_TYPES: &[&str] = &[
            "int", "int8", "int16", "int32", "int64", "uint", "uint8", "uint16", "uint32",
            "uint64", "float32", "float64", "string", "bool", "rune", "byte", "Option", "Result",
            "Slice", "Map", "Channel",
        ];
        for ty in PRELUDE_TYPES {
            items.push(CompletionItem {
                label: ty.to_string(),
                kind: Some(CompletionItemKind::TYPE_PARAMETER),
                ..Default::default()
            });
        }

        const PRELUDE_VALUES: &[&str] = &[
            "Some", "None", "Ok", "Err", "Unit", "println", "print", "panic", "len", "cap", "make",
            "append", "copy",
        ];
        for val in PRELUDE_VALUES {
            items.push(CompletionItem {
                label: val.to_string(),
                kind: Some(CompletionItemKind::FUNCTION),
                ..Default::default()
            });
        }

        let module_prefix = format!("{}.", file.module_id);
        for (qname, definition) in snapshot.definitions().iter() {
            if let Some(name) = qname.strip_prefix(&module_prefix)
                && !name.contains('.')
            {
                items.push(CompletionItem {
                    label: name.to_string(),
                    kind: Some(definition_to_completion_kind(definition)),
                    detail: Some(definition.ty().to_string()),
                    ..Default::default()
                });
            }
        }

        for binding in snapshot.facts().bindings.values() {
            if binding.span.file_id == file_id && binding.span.byte_offset < offset {
                items.push(CompletionItem {
                    label: binding.name.clone(),
                    kind: Some(CompletionItemKind::VARIABLE),
                    ..Default::default()
                });
            }
        }

        for import in file.imports() {
            let alias = import
                .effective_alias(&snapshot.result.go_package_names)
                .unwrap_or_else(|| import.name.to_string());
            items.push(CompletionItem {
                label: alias,
                kind: Some(CompletionItemKind::MODULE),
                ..Default::default()
            });
        }

        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let Some(snapshot) = self.get_snapshot(uri).await else {
            return Ok(None);
        };
        let Some(file_id) = snapshot.get_file_id(uri) else {
            return Ok(None);
        };
        let Some(file) = snapshot.files().get(&file_id) else {
            return Ok(None);
        };
        let Some(line_index) = snapshot.get_line_index(file_id) else {
            return Ok(None);
        };
        let Some(offset) = line_index.position_to_offset(position) else {
            return Ok(None);
        };

        Ok(signature_help::handle(&file.items, offset))
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

/// When a usage span is wider than the definition span (e.g. `Color.Red` usage
/// vs `Red` definition), return the trailing segment after the last `.` so that
/// rename preserves the qualifier.
fn trailing_segment_span(
    usage_span: syntax::ast::Span,
    definition_span: syntax::ast::Span,
    snapshot: &AnalysisSnapshot,
) -> syntax::ast::Span {
    if usage_span.byte_length <= definition_span.byte_length {
        return usage_span;
    }
    let Some(source_file) = snapshot.files().get(&usage_span.file_id) else {
        return usage_span;
    };
    let start = usage_span.byte_offset as usize;
    let end = start + usage_span.byte_length as usize;
    if end > source_file.source.len() {
        return usage_span;
    }
    let usage_text = &source_file.source[start..end];
    let Some(dot_position) = usage_text.rfind('.') else {
        return usage_span;
    };
    syntax::ast::Span::new(
        usage_span.file_id,
        usage_span.byte_offset + dot_position as u32 + 1,
        usage_span.byte_length - dot_position as u32 - 1,
    )
}
