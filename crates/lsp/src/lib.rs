mod analysis;
mod completion;
mod definition;
mod document;
mod heap;
mod hover;
mod imports;
mod inlay_hints;
mod loader;
mod paths;
mod patterns;
mod position;
mod project;
pub mod protocol;
mod router;
mod scope;
mod signature_help;
mod snapshot;
mod state;
mod symbol;
mod traversal;
mod validation;

use std::sync::atomic::Ordering;

use crate::protocol::RpcResult as Result;
use crate::protocol::*;
use syntax::ast::IdentifierResolution;

use crate::analysis::{convert_diagnostic, cursor_offset, offset_in_span, type_name};
use crate::completion::{
    DotContext, attribute_completions, definition_to_completion_kind, detect_dot_context,
    detect_struct_literal_field_context, get_instance_completions, get_package_prefix,
    get_struct_literal_completions, get_type_completions, id_is_in_package, resolve_variable_type,
};
use crate::definition::is_generated_typedef_span;
use crate::imports::EditTarget;
use crate::position::LineIndex;
use crate::snapshot::{AnalysisSnapshot, SnapshotDocument};
use crate::symbol::resolve_symbol;
use crate::traversal::find_expression_at;
use std::collections::HashMap;
use std::sync::Arc;
use syntax::ast::Expression;
use syntax::ast::Span;
use syntax::doc::to_markdown;
use syntax::program::File;
use syntax::types::SELF_TYPE_NAME;

pub use crate::state::{Backend, SharedState};

impl Backend {
    fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        self.insert_replace_support.store(
            params
                .capabilities
                .pointer("/textDocument/completion/completionItem/insertReplaceSupport")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            Ordering::Relaxed,
        );

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
        self.shared_state.open_document(
            params.text_document.uri,
            params.text_document.text,
            params.text_document.version,
        );
    }

    fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Some(change) = params.content_changes.into_iter().last() {
            self.shared_state
                .change_document(uri, change.text, params.text_document.version);
        }
    }

    fn did_save(&self, params: DidSaveTextDocumentParams) {
        self.publish_diagnostics(params.text_document.uri);
    }

    fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.shared_state.close_document(&params.text_document.uri);
    }

    fn formatting(&self, params: DocumentFormattingParams) -> Result<Option<Vec<TextEdit>>> {
        let uri = &params.text_document.uri;
        let (source, end_position) = {
            let workspace = self.workspace();
            let documents = &workspace.documents;
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
        let Some(offset) = cursor_offset(&file.source, cursor.offset) else {
            return Ok(None);
        };

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
            Some(doc) => format!("```lisette\n{ty}\n```\n\n---\n\n{}", to_markdown(&doc)),
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
        Ok(resolve_symbol(&snapshot, &cursor)
            .and_then(|symbol| location_for(symbol.definition_span, &snapshot))
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
            expression: &Expression,
            line_index: &LineIndex,
        ) -> Option<DocumentSymbol> {
            use Expression;

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
        let Some(symbol) = resolve_symbol(&snapshot, &cursor) else {
            return Ok(None);
        };
        let definition_span = symbol.reference_definition_span();

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
        let Some(symbol) = resolve_symbol(&snapshot, &cursor) else {
            return Ok(None);
        };
        let file = cursor.document.file;
        if let Some(Expression::Identifier {
            resolution: IdentifierResolution::Definition(name),
            ..
        }) = find_expression_at(&file.items, symbol.occurrence_span.byte_offset)
        {
            validation::check_rename_guards(name.as_str())?;
        }
        if symbol.is_import()
            || is_generated_typedef_span(&snapshot, &symbol.reference_definition_span())
        {
            return Ok(None);
        }
        let span = symbol.occurrence_span;
        let Some(placeholder) = file
            .source
            .get(span.byte_offset as usize..(span.byte_offset + span.byte_length) as usize)
        else {
            return Ok(None);
        };
        Ok(Some(PrepareRenameResponse::RangeWithPlaceholder {
            range: cursor.document.line_index.span_to_range(span),
            placeholder: placeholder.to_string(),
        }))
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
        let Some(symbol) = resolve_symbol(&snapshot, &cursor) else {
            return Ok(None);
        };
        let definition_span = symbol.reference_definition_span();
        if symbol.is_import() || is_generated_typedef_span(&snapshot, &definition_span) {
            return Ok(None);
        }
        let mut edits: HashMap<Url, Vec<TextEdit>> = HashMap::new();

        if new_name == SELF_TYPE_NAME && names_a_type(&snapshot, definition_span) {
            return Err(validation::rename_error(format!(
                "'{SELF_TYPE_NAME}' is reserved for the type of an `impl` block"
            )));
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

            let mut changes = HashMap::new();
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
        let workspace = self.workspace();
        let documents = &workspace.documents;
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
            insert_replace_support: self.insert_replace_support.load(Ordering::Relaxed),
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

fn location_for(span: Span, snapshot: &AnalysisSnapshot) -> Option<Location> {
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
            get_instance_completions(&type_id, snapshot, &file.package_id)
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
            return get_instance_completions(&type_id, snapshot, &file.package_id);
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
        return get_instance_completions(&type_id, snapshot, &file.package_id);
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
        "int",
        "int8",
        "int16",
        "int32",
        "int64",
        "uint",
        "uint8",
        "uint16",
        "uint32",
        "uint64",
        "float32",
        "float64",
        "string",
        "bool",
        "rune",
        "byte",
        "Option",
        "Result",
        "Slice",
        "Map",
        "Channel",
        "Array",
        "Ref",
        SELF_TYPE_NAME,
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

    let bindings = scope::visible_bindings(file, offset, snapshot);
    items.retain(|item| !bindings.contains_key(&item.label));
    items.extend(bindings.into_iter().map(|(name, ty)| CompletionItem {
        label: name,
        kind: Some(CompletionItemKind::VARIABLE),
        detail: Some(ty.to_string()),
        ..Default::default()
    }));

    items
}

pub(crate) fn ranges_overlap(a: Range, b: Range) -> bool {
    let position_le = |x: Position, y: Position| (x.line, x.character) <= (y.line, y.character);
    position_le(a.start, b.end) && position_le(b.start, a.end)
}

/// Narrows a usage span to just the trailing member token, dropping any
/// qualifier (`Color.Red`) and any payload (`Red(x)`).
fn trailing_segment_span(usage_span: Span, snapshot: &AnalysisSnapshot) -> Span {
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
            Span::new(usage_span.file_id, usage_span.byte_offset + offset, length)
        }
        None => usage_span,
    }
}

fn names_a_type(snapshot: &AnalysisSnapshot, definition_span: Span) -> bool {
    let Some(file) = snapshot.files().get(&definition_span.file_id) else {
        return false;
    };
    let Some(expression) = find_expression_at(&file.items, definition_span.byte_offset) else {
        return false;
    };
    matches!(
        expression,
        Expression::Struct { name_span, .. }
            | Expression::Enum { name_span, .. }
            | Expression::Interface { name_span, .. }
            | Expression::TypeAlias { name_span, .. }
        if *name_span == definition_span
    )
}

fn usage_locations(
    snapshots: impl IntoIterator<Item = Arc<AnalysisSnapshot>>,
    definition_uri: &Url,
    definition_span: Span,
) -> Vec<Location> {
    let mut locations = Vec::new();
    for snapshot in snapshots {
        let Some(target_document) = snapshot.document(definition_uri) else {
            continue;
        };
        let target_span = Span::new(
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
