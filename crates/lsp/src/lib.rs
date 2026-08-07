mod analysis;
mod completion;
mod definition;
mod document;
mod hover;
mod imports;
mod inlay_hints;
mod loader;
mod paths;
mod patterns;
mod position;
mod project;
pub mod protocol;
mod signature_help;
mod snapshot;
mod state;
mod traversal;
mod validation;

use crate::protocol::RpcResult as Result;
use crate::protocol::*;
use syntax::ast::IdentifierResolution;

use crate::analysis::{convert_diagnostic, offset_in_span, type_name};
use crate::completion::{
    DotContext, attribute_completions, definition_to_completion_kind, detect_dot_context,
    detect_struct_literal_field_context, get_instance_completions, get_package_prefix,
    get_struct_literal_completions, get_type_completions, id_is_in_package, resolve_variable_type,
};
use crate::definition::{
    find_struct_field_span, is_generated_typedef_span, lookup_definition_span,
    resolve_annotation_definition, resolve_dot_access_definition, resolve_enum_in_pattern,
    resolve_import_span, resolve_match_pattern_definition, resolve_struct_call_field,
    resolve_symbol_definition_span, resolve_word_at_offset, word_at_offset,
};
use crate::imports::EditTarget;
use crate::position::LineIndex;
use crate::project::find_project_root;
use crate::snapshot::{AnalysisSnapshot, SnapshotDocument};
use crate::traversal::find_expression_at;
use syntax::program::File;

pub use crate::state::{Backend, SharedState};

impl Backend {
    fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        self.insert_replace_support.store(
            params
                .capabilities
                .pointer("/textDocument/completion/completionItem/insertReplaceSupport")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            std::sync::atomic::Ordering::Relaxed,
        );

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
            self.project.initialize(config);
        }

        // The first run for a version writes the full stdlib.
        deps::ensure_stdlib_extracted(deps::Target::host());
        deps::ensure_prelude_extracted();

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                document_formatting_provider: Some(OneOf::Left(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                inlay_hint_provider: Some(OneOf::Left(true)),
                definition_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                rename_provider: Some(OneOf::Right(RenameOptions {
                    prepare_provider: Some(true),
                })),
                completion_provider: Some(CompletionOptions {
                    // `.` for member access; `#`/`[` to open attribute completions
                    trigger_characters: Some(vec![
                        ".".to_string(),
                        "#".to_string(),
                        "[".to_string(),
                    ]),
                    ..Default::default()
                }),
                signature_help_provider: Some(SignatureHelpOptions {
                    work_done_progress: None,
                    trigger_characters: Some(vec!["(".to_string(), ",".to_string()]),
                    retrigger_characters: None,
                }),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Lisette LSP initialized");
    }

    fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri;
        let content = params.text_document.text;
        self.update_document(uri.clone(), content, params.text_document.version);
        self.publish_diagnostics(uri);
    }

    fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Some(change) = params.content_changes.into_iter().last() {
            self.update_document(uri, change.text, params.text_document.version);
            self.shared_state.recheck_open_documents();
        }
    }

    fn did_save(&self, params: DidSaveTextDocumentParams) {
        self.publish_diagnostics(params.text_document.uri);
    }

    fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = &params.text_document.uri;
        self.documents_mut().remove(uri);

        let overlay_removed = self.project.remove_overlay(uri);

        self.client.publish_diagnostics(uri.clone(), vec![], None);

        if overlay_removed {
            self.shared_state.recheck_open_documents();
        }
    }

    fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let uri = &params.text_document.uri;
        let (source, end_position) = {
            let documents = self.documents();
            let Some(doc) = documents.get(uri) else {
                return Ok(None);
            };
            let end = doc
                .line_index()
                .offset_to_position(doc.content().len() as u32);
            (doc.content().to_string(), end)
        };

        let formatted = match format::format_source(&source) {
            Ok(formatted) => formatted,
            Err(_parse_errors) => {
                self.client
                    .log_message(MessageType::WARNING, "Cannot format: file has parse errors");
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

    fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let Some(snapshot) = self.get_snapshot(uri) else {
            return Ok(None);
        };
        let Some(cursor) = snapshot.position(uri, position) else {
            return Ok(None);
        };
        let file = cursor.document.file;
        let line_index = cursor.document.line_index;
        let offset = cursor.offset;

        let Some(expression) = find_expression_at(&file.items, offset) else {
            return Ok(None);
        };

        let (ty, span) = hover::resolve_declaration_hover(expression, offset, file, &snapshot)
            .unwrap_or_else(|| hover::get_hover_type_and_span(&snapshot, expression, offset));

        if ty.is_variable() || ty.is_placeholder() || ty.is_error() {
            return Ok(None);
        }

        let doc = hover::get_hover_doc(expression, offset, file, &snapshot).or_else(|| {
            let type_id = ty.get_qualified_id()?;
            snapshot.definitions().get(type_id)?.doc.clone()
        });

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

    fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        let uri = &params.text_document.uri;

        let Some(snapshot) = self.get_snapshot(uri) else {
            return Ok(None);
        };
        let Some(document) = snapshot.document(uri) else {
            return Ok(None);
        };
        let file = document.file;
        let line_index = document.line_index;

        let eof = file.source.len() as u32;
        let start = line_index
            .position_to_offset(params.range.start)
            .unwrap_or(eof);
        let end = line_index
            .position_to_offset(params.range.end)
            .unwrap_or(eof);

        Ok(Some(inlay_hints::collect(
            &snapshot,
            &file.items,
            (start, end),
            line_index,
        )))
    }

    fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let Some(snapshot) = self.get_snapshot(uri) else {
            return Ok(None);
        };
        let Some(cursor) = snapshot.position(uri, position) else {
            return Ok(None);
        };
        let file_id = cursor.document.file_id;
        let file = cursor.document.file;
        let offset = cursor.offset;

        let Some(expression) = find_expression_at(&file.items, offset) else {
            return Ok(None);
        };

        let find_binding = || {
            snapshot
                .bindings()
                .values()
                .find(|b| b.span.file_id == file_id && offset_in_span(offset, &b.span))
                .map(|b| b.span)
        };
        let binding_or_word_fallback = |resolved: Option<syntax::ast::Span>| {
            resolved
                .or_else(&find_binding)
                .or_else(|| resolve_word_at_offset(&file.source, offset, file, &snapshot))
        };

        let definition_span = match expression {
            syntax::ast::Expression::Identifier {
                resolution: IdentifierResolution::Binding(id),
                ..
            } => snapshot.bindings().get(id).map(|b| b.span),

            syntax::ast::Expression::Identifier {
                value,
                resolution: IdentifierResolution::Definition(qname),
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
                                resolve_import_span(
                                    first,
                                    file,
                                    &snapshot.analysis.emit_input.go_package_names,
                                )
                            })
                            && let Some(source) = snapshot.source(span.file_id)
                        {
                            return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                                uri: source.uri.clone(),
                                range: source.line_index.span_to_range(span),
                            })));
                        }
                    }
                }
                snapshot
                    .definitions()
                    .get(qname.as_str())
                    .and_then(|d| d.name_span)
            }

            syntax::ast::Expression::DotAccess {
                expression,
                member,
                span,
                ..
            } => resolve_dot_access_definition(expression, member, *span, file, &snapshot),

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

            syntax::ast::Expression::TypeAlias {
                name_span,
                annotation,
                ..
            } => {
                if offset_in_span(offset, name_span) {
                    Some(*name_span)
                } else {
                    resolve_annotation_definition(annotation, offset, file, &snapshot)
                }
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
                    let qualified = format!("{}.{}", file.package_id, name);
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
                    let qualified = format!("{}.{}.{}", file.package_id, name, v.name);
                    snapshot
                        .definitions()
                        .get(qualified.as_str())
                        .and_then(|d| d.name_span)
                })
                .or_else(|| offset_in_span(offset, name_span).then_some(*name_span)),

            syntax::ast::Expression::Identifier { value, .. } => {
                lookup_definition_span(value, file, &snapshot)
                    .or_else(|| {
                        resolve_import_span(
                            value,
                            file,
                            &snapshot.analysis.emit_input.go_package_names,
                        )
                    })
                    // A dotted callee like `Array.new` doesn't resolve whole, so
                    // fall back to the type word at the cursor (`Array` -> its decl).
                    .or_else(|| resolve_word_at_offset(&file.source, offset, file, &snapshot))
            }

            syntax::ast::Expression::Match { arms, .. } => binding_or_word_fallback(
                resolve_match_pattern_definition(arms, offset, file, &snapshot),
            ),

            syntax::ast::Expression::IfLet { pattern, .. }
            | syntax::ast::Expression::WhileLet { pattern, .. } => {
                binding_or_word_fallback(resolve_enum_in_pattern(pattern, offset, file, &snapshot))
            }

            _ => binding_or_word_fallback(None),
        };

        Ok(definition_span
            .and_then(|span| location_for(span, &snapshot))
            .map(GotoDefinitionResponse::Scalar))
    }

    fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        use crate::protocol::{DocumentSymbol, SymbolKind};

        let uri = &params.text_document.uri;

        let Some(snapshot) = self.get_snapshot(uri) else {
            return Ok(None);
        };
        let Some(document) = snapshot.document(uri) else {
            return Ok(None);
        };
        let file = document.file;
        let line_index = document.line_index;

        fn expression_to_symbol(
            expression: &syntax::ast::Expression,
            line_index: &LineIndex,
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

    fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        let Some(snapshot) = self.get_snapshot(uri) else {
            return Ok(None);
        };
        let Some(cursor) = snapshot.position(uri, position) else {
            return Ok(None);
        };
        let file_id = cursor.document.file_id;
        let file = cursor.document.file;
        let offset = cursor.offset;

        let definition_span = resolve_symbol_definition_span(&snapshot, file, file_id, offset);

        let Some(definition_span) = definition_span else {
            return Ok(None);
        };

        let Some(definition_source) = snapshot.source(definition_span.file_id) else {
            return Ok(None);
        };
        let definition_uri = definition_source.uri.clone();

        let mut locations = Vec::new();

        if params.context.include_declaration {
            locations.push(Location {
                uri: definition_uri.clone(),
                range: definition_source.line_index.span_to_range(definition_span),
            });
        }

        locations.extend(usage_locations(
            self.open_document_snapshots(),
            &definition_uri,
            definition_span,
        ));

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

    fn prepare_rename(
        &self,
        params: TextDocumentPositionParams,
    ) -> Result<Option<PrepareRenameResponse>> {
        let uri = &params.text_document.uri;
        let position = params.position;

        let Some(snapshot) = self.get_snapshot(uri) else {
            return Ok(None);
        };
        let Some(cursor) = snapshot.position(uri, position) else {
            return Ok(None);
        };
        let file_id = cursor.document.file_id;
        let file = cursor.document.file;
        let line_index = cursor.document.line_index;
        let offset = cursor.offset;

        let rename_response =
            |span: syntax::ast::Span, placeholder: &str| -> Result<Option<PrepareRenameResponse>> {
                Ok(Some(PrepareRenameResponse::RangeWithPlaceholder {
                    range: line_index.span_to_range(span),
                    placeholder: placeholder.to_string(),
                }))
            };
        let rename_word_if_resolved =
            |resolved: Option<syntax::ast::Span>| -> Result<Option<PrepareRenameResponse>> {
                if let Some(definition_span) = resolved
                    && !is_generated_typedef_span(&snapshot, &definition_span)
                    && let Some((word, start, end)) = word_at_offset(&file.source, offset)
                {
                    let span = syntax::ast::Span::new(file_id, start as u32, (end - start) as u32);
                    return rename_response(span, word);
                }
                Ok(None)
            };

        for binding in snapshot.bindings().values() {
            let span = binding.span;
            if span.file_id == file_id && offset_in_span(offset, &span) {
                return rename_response(span, &binding.name);
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
            && type_name(ty, &snapshot)
                .and_then(|type_id| find_struct_field_span(&type_id, &fa.name, &snapshot))
                .is_some()
        {
            return rename_response(fa.name_span, &fa.name);
        }

        match expression {
            syntax::ast::Expression::Identifier {
                value,
                resolution: IdentifierResolution::Binding(id),
                span,
                ..
            } => {
                if let Some(binding) = snapshot.bindings().get(id)
                    && binding.span.file_id == file_id
                {
                    rename_response(*span, value)
                } else {
                    Ok(None)
                }
            }

            syntax::ast::Expression::Identifier {
                value,
                resolution: IdentifierResolution::Definition(qname),
                span,
                ..
            } => {
                validation::check_rename_guards(qname.as_str())?;
                if snapshot.definitions().contains_key(qname.as_str()) {
                    rename_response(*span, syntax::types::unqualified_name(value))
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
                let qname = format!("{}.{}", file.package_id, name);
                validation::check_rename_guards(&qname)?;
                rename_response(*name_span, name)
            }

            syntax::ast::Expression::Struct {
                name,
                name_span,
                fields,
                ..
            } => {
                let qname = format!("{}.{}", file.package_id, name);
                if let Some(field) = fields.iter().find(|f| offset_in_span(offset, &f.name_span))
                    && find_struct_field_span(&qname, &field.name, &snapshot).is_some()
                {
                    validation::check_rename_guards(&qname)?;
                    return rename_response(field.name_span, &field.name);
                }
                validation::check_rename_guards(&qname)?;
                rename_response(*name_span, name)
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
                    let qname = format!("{}.{}.{}", file.package_id, name, variant.name);
                    validation::check_rename_guards(&qname)?;
                    return rename_response(variant.name_span, &variant.name);
                }
                let qualified_name = format!("{}.{}", file.package_id, name);
                validation::check_rename_guards(&qualified_name)?;
                rename_response(*name_span, name)
            }

            syntax::ast::Expression::VariableDeclaration {
                name, name_span, ..
            } => rename_response(*name_span, name),

            syntax::ast::Expression::Const {
                identifier,
                identifier_span,
                ..
            } => {
                let qname = format!("{}.{}", file.package_id, identifier);
                validation::check_rename_guards(&qname)?;
                rename_response(*identifier_span, identifier)
            }

            syntax::ast::Expression::DotAccess {
                expression,
                member,
                span,
                ..
            } if !member.is_empty() => {
                let resolved =
                    resolve_dot_access_definition(expression, member, *span, file, &snapshot);
                if let Some(definition_span) = resolved
                    && !is_generated_typedef_span(&snapshot, &definition_span)
                {
                    let member_span = syntax::ast::Span::new(
                        span.file_id,
                        span.byte_offset + span.byte_length - member.len() as u32,
                        member.len() as u32,
                    );
                    rename_response(member_span, member)
                } else {
                    Ok(None)
                }
            }

            syntax::ast::Expression::Match { arms, .. } => rename_word_if_resolved(
                resolve_match_pattern_definition(arms, offset, file, &snapshot),
            ),

            syntax::ast::Expression::IfLet { pattern, .. }
            | syntax::ast::Expression::WhileLet { pattern, .. } => {
                rename_word_if_resolved(resolve_enum_in_pattern(pattern, offset, file, &snapshot))
            }

            _ => rename_word_if_resolved(
                word_at_offset(&file.source, offset)
                    .and_then(|(word, _, _)| lookup_definition_span(word, file, &snapshot)),
            ),
        }
    }

    fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let new_name = params.new_name;

        validation::validate_rename(&new_name).map_err(validation::rename_error)?;

        let Some(snapshot) = self.get_snapshot(uri) else {
            return Ok(None);
        };
        let Some(cursor) = snapshot.position(uri, position) else {
            return Ok(None);
        };
        let file_id = cursor.document.file_id;
        let file = cursor.document.file;
        let offset = cursor.offset;

        let mut edits: std::collections::HashMap<Url, Vec<TextEdit>> =
            std::collections::HashMap::new();

        if let Some(syntax::ast::Expression::Identifier {
            resolution: IdentifierResolution::Definition(qname),
            ..
        }) = find_expression_at(&file.items, offset)
            && validation::check_rename_guards(qname.as_str()).is_err()
        {
            return Ok(None);
        }

        let definition_span = resolve_symbol_definition_span(&snapshot, file, file_id, offset);

        let Some(definition_span) = definition_span else {
            return Ok(None);
        };

        if is_generated_typedef_span(&snapshot, &definition_span) {
            return Ok(None);
        }

        let Some(definition_source) = snapshot.source(definition_span.file_id) else {
            return Ok(None);
        };
        let definition_uri = definition_source.uri.clone();

        edits
            .entry(definition_uri.clone())
            .or_default()
            .push(TextEdit {
                range: definition_source.line_index.span_to_range(definition_span),
                new_text: new_name.clone(),
            });

        for location in usage_locations(
            self.open_document_snapshots(),
            &definition_uri,
            definition_span,
        ) {
            edits.entry(location.uri).or_default().push(TextEdit {
                range: location.range,
                new_text: new_name.clone(),
            });
        }

        if edits.is_empty() {
            return Ok(None);
        }

        Ok(Some(WorkspaceEdit {
            changes: Some(edits),
            ..Default::default()
        }))
    }

    fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let uri = &params.text_document.uri;

        let Some(snapshot) = self.get_snapshot(uri) else {
            return Ok(None);
        };
        let Some(document) = snapshot.document(uri) else {
            return Ok(None);
        };
        let file_id = document.file_id;
        let line_index = document.line_index;

        let mut actions: Vec<CodeActionOrCommand> = Vec::new();

        for diagnostic in snapshot.analysis.lints() {
            if diagnostic.file_id() != Some(file_id) {
                continue;
            }
            let Some(fix) = diagnostic.fix() else {
                continue;
            };

            let lsp_diagnostic = convert_diagnostic(diagnostic, line_index);
            if !ranges_overlap(params.range, lsp_diagnostic.range) {
                continue;
            }

            let text_edits: Vec<TextEdit> = fix
                .edits()
                .map(|edit| TextEdit {
                    range: line_index.span_to_range(edit.span()),
                    new_text: edit.content().to_string(),
                })
                .collect();

            let mut changes = std::collections::HashMap::new();
            changes.insert(uri.clone(), text_edits);

            actions.push(CodeActionOrCommand::CodeAction(Box::new(CodeAction {
                title: fix.message().to_string(),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(vec![lsp_diagnostic]),
                edit: Some(WorkspaceEdit {
                    changes: Some(changes),
                    ..Default::default()
                }),
                is_preferred: Some(true),
                ..Default::default()
            })));
        }

        if actions.is_empty() {
            return Ok(None);
        }

        Ok(Some(actions))
    }

    fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        let Some(snapshot) = self.get_snapshot(uri) else {
            return Ok(None);
        };
        let Some(cursor) = snapshot.position(uri, position) else {
            return Ok(None);
        };
        let document = &cursor.document;
        let file = document.file;
        let offset = cursor.offset;

        // An in-progress `#[ ... ]` is exclusive: when the cursor is in attribute
        // position, offer only the attributes relevant to the target it attaches
        // to, never the general keyword/identifier completions below.
        let is_test_file = uri.path().ends_with(".test.lis");
        if let Some(items) = attribute_completions(&file.source, offset as usize, is_test_file) {
            return Ok(Some(CompletionResponse::Array(items)));
        }

        let target = self.edit_target(uri, position);

        let package_prefix = get_package_prefix(&file.source, offset as usize);

        if let Some((package_name, _)) = package_prefix
            && let Some(items) = imported_package_completions(package_name, file, &snapshot)
        {
            return Ok(Some(CompletionResponse::Array(items)));
        }

        if let Some(items) = dot_context_completions(file, offset, &snapshot) {
            return Ok(Some(CompletionResponse::Array(items)));
        }

        if let Some((prefix, dot_offset)) = package_prefix {
            let after_dot = dot_offset as u32 + 1;
            let items = package_prefix_completions(prefix, document, after_dot, target, &snapshot);
            return Ok(Some(CompletionResponse::Array(items)));
        }

        if let Some(items) = struct_literal_field_completions(file, offset, &snapshot) {
            return Ok(Some(CompletionResponse::Array(items)));
        }

        Ok(Some(CompletionResponse::Array(general_completions(
            document, offset, target, &snapshot,
        ))))
    }

    fn edit_target(&self, uri: &Url, position: Position) -> Option<EditTarget> {
        let documents = self.documents();
        let document = documents.get(uri)?;
        let line_index = document.line_index();
        let offset = line_index.position_to_offset(position)? as usize;
        let source = document.content();

        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        let start = source[..offset]
            .rfind(|c: char| !is_word(c))
            .map(|index| index + source[index..].chars().next().map_or(1, char::len_utf8))
            .unwrap_or(0);
        let end = offset
            + source[offset..]
                .find(|c: char| !is_word(c))
                .unwrap_or(source.len() - offset);
        let start = line_index.offset_to_position(start as u32);

        Some(EditTarget {
            insert: Range {
                start,
                end: position,
            },
            replace: Range {
                start,
                end: line_index.offset_to_position(end as u32),
            },
            insert_replace_support: self
                .insert_replace_support
                .load(std::sync::atomic::Ordering::Relaxed),
        })
    }

    fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let Some(snapshot) = self.get_snapshot(uri) else {
            return Ok(None);
        };
        let Some(cursor) = snapshot.position(uri, position) else {
            return Ok(None);
        };
        let file = cursor.document.file;
        let offset = cursor.offset;

        Ok(signature_help::handle(&file.items, offset))
    }

    fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

fn location_for(span: syntax::ast::Span, snapshot: &AnalysisSnapshot) -> Option<Location> {
    if span.is_dummy() {
        return None;
    }

    if let Some(target_file) = snapshot.files().get(&span.file_id) {
        let end = (span.byte_offset as usize).saturating_add(span.byte_length as usize);
        if end > target_file.source.len() {
            return None;
        }
    }

    if let Some(path) = snapshot.typedef_path(span.file_id)
        && !path.exists()
    {
        return None;
    }

    let target = snapshot.source(span.file_id)?;
    Some(Location {
        uri: target.uri.clone(),
        range: target.line_index.span_to_range(span),
    })
}

fn imported_package_completions(
    package_name: &str,
    file: &File,
    snapshot: &AnalysisSnapshot,
) -> Option<Vec<CompletionItem>> {
    let imports = file.imports();
    let imp = imports.iter().find(|imp| {
        imp.effective_alias(&snapshot.analysis.emit_input.go_package_names)
            .as_deref()
            == Some(package_name)
    })?;

    let mut items = Vec::new();
    for (qname, definition) in snapshot.definitions().iter() {
        if let Some(rest) = qname.strip_prefix(imp.name.as_str())
            && let Some(name) = rest.strip_prefix('.')
            && !name.contains('.')
            && definition.visibility.is_public()
        {
            items.push(CompletionItem {
                label: name.to_string(),
                kind: Some(definition_to_completion_kind(definition)),
                detail: Some(definition.ty.to_string()),
                ..Default::default()
            });
        }
    }
    Some(items)
}

fn dot_context_completions(
    file: &File,
    offset: u32,
    snapshot: &AnalysisSnapshot,
) -> Option<Vec<CompletionItem>> {
    let ctx = detect_dot_context(file, offset, snapshot)?;
    Some(match ctx {
        DotContext::Instance(type_id) => {
            let same_package = id_is_in_package(&type_id, &file.package_id);
            get_instance_completions(&type_id, snapshot, same_package)
        }
        DotContext::TypeLevel(type_id) => {
            get_type_completions(&type_id, snapshot, &file.package_id)
        }
    })
}

/// A `foo.` prefix that resolves to nothing still returns an (empty) result
/// here: it must never fall through to the general completions below.
fn package_prefix_completions(
    prefix: &str,
    document: &SnapshotDocument,
    offset: u32,
    target: Option<EditTarget>,
    snapshot: &AnalysisSnapshot,
) -> Vec<CompletionItem> {
    let file = document.file;
    if prefix == "self" {
        if let Some(impl_type) = traversal::find_enclosing_impl_type(&file.items, offset) {
            let type_id = format!("{}.{}", file.package_id, impl_type);
            return get_instance_completions(&type_id, snapshot, true);
        }
        return Vec::new();
    }

    for package in [file.package_id.as_str(), "prelude"] {
        let qualified = format!("{package}.{prefix}");
        if let Some(definition) = snapshot.definitions().get(qualified.as_str())
            && definition.is_type_definition()
        {
            return get_type_completions(&qualified, snapshot, &file.package_id);
        }
    }

    for import in file.imports() {
        let qualified = format!("{}.{}", import.name, prefix);
        if let Some(definition) = snapshot.definitions().get(qualified.as_str())
            && definition.is_type_definition()
            && definition.visibility.is_public()
        {
            return get_type_completions(&qualified, snapshot, &file.package_id);
        }
    }

    let indexed = offset as usize >= 2 && file.source.as_bytes()[offset as usize - 2] == b']';
    if let Some(type_id) = resolve_variable_type(prefix, file, offset, snapshot, indexed) {
        let same_package = id_is_in_package(&type_id, &file.package_id);
        return get_instance_completions(&type_id, snapshot, same_package);
    }

    let packages = snapshot.importable_packages();
    imports::not_yet_imported(&packages, file, snapshot)
        .into_iter()
        .filter(|importable| importable.name == prefix)
        .flat_map(|importable| {
            imports::member_completions(importable, file, document.line_index, target)
        })
        .collect()
}

/// In a struct literal's field-name position, offers the unassigned fields.
fn struct_literal_field_completions(
    file: &File,
    offset: u32,
    snapshot: &AnalysisSnapshot,
) -> Option<Vec<CompletionItem>> {
    let (name, ty, assigned) = detect_struct_literal_field_context(file, offset)?;
    let type_id = type_name(ty, snapshot)?;
    let same_package = id_is_in_package(&type_id, &file.package_id);
    Some(get_struct_literal_completions(
        &type_id,
        name,
        snapshot,
        same_package,
        assigned,
        offset,
    ))
}

fn general_completions(
    document: &SnapshotDocument,
    offset: u32,
    target: Option<EditTarget>,
    snapshot: &AnalysisSnapshot,
) -> Vec<CompletionItem> {
    let file = document.file;
    let mut items = Vec::new();

    for kw in validation::KEYWORDS {
        items.push(CompletionItem {
            label: kw.to_string(),
            kind: Some(CompletionItemKind::KEYWORD),
            ..Default::default()
        });
    }

    const PRELUDE_TYPES: &[&str] = &[
        "int", "int8", "int16", "int32", "int64", "uint", "uint8", "uint16", "uint32", "uint64",
        "float32", "float64", "string", "bool", "rune", "byte", "Option", "Result", "Slice", "Map",
        "Channel", "Array",
    ];
    for ty in PRELUDE_TYPES {
        items.push(CompletionItem {
            label: ty.to_string(),
            kind: Some(CompletionItemKind::TYPE_PARAMETER),
            ..Default::default()
        });
    }

    // `len`, `make` and `println` are Go builtins the compiler rejects by name.
    const PRELUDE_VALUES: &[&str] = &["Some", "None", "Ok", "Err", "panic"];
    for val in PRELUDE_VALUES {
        items.push(CompletionItem {
            label: val.to_string(),
            kind: Some(CompletionItemKind::FUNCTION),
            ..Default::default()
        });
    }

    let package_prefix = format!("{}.", file.package_id);
    for (qname, definition) in snapshot.definitions().iter() {
        if let Some(name) = qname.strip_prefix(&package_prefix)
            && !name.contains('.')
        {
            items.push(CompletionItem {
                label: name.to_string(),
                kind: Some(definition_to_completion_kind(definition)),
                detail: Some(definition.ty.to_string()),
                ..Default::default()
            });
        }
    }

    for binding in snapshot.bindings().values() {
        if binding.span.file_id == document.file_id && binding.span.byte_offset < offset {
            items.push(CompletionItem {
                label: binding.name.clone(),
                kind: Some(CompletionItemKind::VARIABLE),
                ..Default::default()
            });
        }
    }

    for import in file.imports() {
        let alias = import
            .effective_alias(&snapshot.analysis.emit_input.go_package_names)
            .unwrap_or_else(|| import.name.to_string());
        items.push(CompletionItem {
            label: alias,
            kind: Some(CompletionItemKind::MODULE),
            ..Default::default()
        });
    }

    let packages = snapshot.importable_packages();
    let unimported = imports::not_yet_imported(&packages, file, snapshot);
    items.extend(imports::package_completions(
        &unimported,
        file,
        document.line_index,
        target,
    ));

    items
}

pub(crate) fn ranges_overlap(a: Range, b: Range) -> bool {
    let position_le = |x: Position, y: Position| (x.line, x.character) <= (y.line, y.character);
    position_le(a.start, b.end) && position_le(b.start, a.end)
}

/// Narrows a usage span to just the trailing member token, dropping any
/// qualifier (`Color.Red`) and any payload (`Red(x)`).
fn trailing_segment_span(
    usage_span: syntax::ast::Span,
    snapshot: &AnalysisSnapshot,
) -> syntax::ast::Span {
    let Some(source_file) = snapshot.files().get(&usage_span.file_id) else {
        return usage_span;
    };
    let start = usage_span.byte_offset as usize;
    let end = start + usage_span.byte_length as usize;
    if end > source_file.source.len() {
        return usage_span;
    }
    let usage_text = &source_file.source[start..end];
    match member_token_range(usage_text) {
        Some((offset, length)) => {
            syntax::ast::Span::new(usage_span.file_id, usage_span.byte_offset + offset, length)
        }
        None => usage_span,
    }
}

fn usage_locations(
    snapshots: impl IntoIterator<Item = std::sync::Arc<AnalysisSnapshot>>,
    definition_uri: &Url,
    definition_span: syntax::ast::Span,
) -> Vec<Location> {
    let mut locations = Vec::new();
    for snapshot in snapshots {
        let Some(target_document) = snapshot.document(definition_uri) else {
            continue;
        };
        let target_span = syntax::ast::Span::new(
            target_document.file_id,
            definition_span.byte_offset,
            definition_span.byte_length,
        );
        for usage in snapshot.usages() {
            if usage.definition_span == target_span
                && let Some(source) = snapshot.source(usage.usage_span.file_id)
            {
                locations.push(Location {
                    uri: source.uri.clone(),
                    range: source
                        .line_index
                        .span_to_range(trailing_segment_span(usage.usage_span, &snapshot)),
                });
            }
        }
    }
    locations
}

/// Last identifier token in `usage_text`'s head (the run of id chars, dots and
/// whitespace before any payload like `(` or `{`). For `Wrap(Color.Red)` the
/// head is `Wrap`, so the inner `.Red` cannot be mistaken for the outer name.
fn member_token_range(usage_text: &str) -> Option<(u32, u32)> {
    let head_end = head_extent(usage_text);
    let mut last_id_start: Option<usize> = None;
    let mut last_id_end: usize = 0;
    let mut byte_pos = 0;
    let mut in_id = false;
    for c in usage_text[..head_end].chars() {
        let char_len = c.len_utf8();
        if c.is_alphanumeric() || c == '_' {
            if !in_id {
                last_id_start = Some(byte_pos);
                in_id = true;
            }
            byte_pos += char_len;
            last_id_end = byte_pos;
        } else {
            in_id = false;
            byte_pos += char_len;
        }
    }
    last_id_start.map(|start| (start as u32, (last_id_end - start) as u32))
}

/// Byte length of a pattern/call head: id chars, dots, and whitespace, stopping
/// at the first payload character.
fn head_extent(text: &str) -> usize {
    let mut byte_pos = 0;
    for c in text.chars() {
        if c.is_alphanumeric() || c == '_' || c == '.' || c.is_whitespace() {
            byte_pos += c.len_utf8();
        } else {
            break;
        }
    }
    byte_pos
}

#[cfg(test)]
mod tests {
    use super::{member_token_range, ranges_overlap};
    use crate::protocol::{Position, Range};

    fn range(sl: u32, sc: u32, el: u32, ec: u32) -> Range {
        Range {
            start: Position::new(sl, sc),
            end: Position::new(el, ec),
        }
    }

    #[test]
    fn ranges_overlap_detects_intersection() {
        assert!(ranges_overlap(range(0, 0, 0, 5), range(0, 3, 0, 8)));
        assert!(ranges_overlap(range(1, 4, 1, 4), range(1, 0, 1, 10)));
        assert!(ranges_overlap(range(0, 5, 0, 5), range(0, 0, 0, 5)));
        assert!(!ranges_overlap(range(0, 0, 0, 2), range(0, 3, 0, 5)));
        assert!(!ranges_overlap(range(0, 0, 0, 9), range(1, 0, 1, 1)));
    }

    #[test]
    fn member_token_range_extracts_trailing_token() {
        assert_eq!(member_token_range("Red"), Some((0, 3)));
        assert_eq!(member_token_range("Color.Red"), Some((6, 3)));
        assert_eq!(member_token_range("palette.Color.Red"), Some((14, 3)));
        assert_eq!(member_token_range("Red(x)"), Some((0, 3)));
        assert_eq!(member_token_range("Color.Red(x)"), Some((6, 3)));
        assert_eq!(member_token_range("Move { x, y }"), Some((0, 4)));
        assert_eq!(member_token_range("key.0"), Some((4, 1)));
        // Whitespace between segments must not split or truncate the token.
        assert_eq!(member_token_range("Color . Red"), Some((8, 3)));
        assert_eq!(member_token_range("Color . Red(x)"), Some((8, 3)));
        assert_eq!(member_token_range("Shape . Move { x: 1 }"), Some((8, 4)));
        // Payload delimiters bound the head: a dotted payload does not narrow the outer.
        assert_eq!(member_token_range("Wrap(Color.Red)"), Some((0, 4)));
        assert_eq!(member_token_range("Some(Color.Red)"), Some((0, 4)));
    }
}
