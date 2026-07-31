//! WebAssembly bindings for the Lisette compiler.
//!
//! All functions are called directly from JavaScript via wasm-bindgen.
//! Diagnostics are serialised as JSON strings so the TS layer can decode them.

use serde::Serialize;
use wasm_bindgen::prelude::*;

use lisette_passes::{Analysis, analyze};
use lisette_semantics::inference::{
    AnalysisScope, AnalyzeInput, CompilePhase, EntryFile, ProjectKind,
};
use lisette_semantics::loader::MemoryLoader;
use lisette_syntax::ast::{Expression, IdentifierResolution, Span};
use lisette_syntax::program::{Definition, DefinitionBody};
use lisette_syntax::types::{Type, unqualified_name};

// ─── Panic hook ───────────────────────────────────────────────────────────────
#[wasm_bindgen(start)]
pub fn init() {
    console_error_panic_hook::set_once();
}

/// The Lisette compiler version this WASM was built against.
#[wasm_bindgen]
pub fn version() -> String {
    env!("LIS_VERSION").to_string()
}

// ─── Serialisable output types ────────────────────────────────────────────────

#[derive(Serialize, Default)]
struct JsDiagnostic {
    severity: String,
    message: String,
    line: u32,
    col: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_col: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
}

#[derive(Serialize)]
struct JsCompileResult {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    go_source: Option<String>,
    diagnostics: Vec<JsDiagnostic>,
}

#[derive(Serialize)]
struct JsCompletionItem {
    label: String,
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    insert_text: Option<String>,
}

#[derive(Serialize)]
struct JsHoverResult {
    markdown: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    start_col: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_col: Option<u32>,
}

#[derive(Serialize)]
struct JsDefinitionResult {
    line: u32,
    col: u32,
    end_line: u32,
    end_col: u32,
}

#[derive(Serialize)]
struct JsSignatureHelp {
    label: String,
    parameters: Vec<String>,
    active_parameter: u32,
}

// ─── Helper: byte offset → (line, col), both 1-based ────────────────────────
fn offset_to_line_col(source: &str, byte_offset: usize) -> (u32, u32) {
    let clamped = byte_offset.min(source.len());
    let prefix = &source[..clamped];
    let line = (prefix.bytes().filter(|&b| b == b'\n').count() + 1) as u32;
    let col = (prefix
        .rfind('\n')
        .map(|i| clamped - i - 1)
        .unwrap_or(clamped)
        + 1) as u32;
    (line, col)
}

// ─── Convert LisetteDiagnostic to JsDiagnostic ────────────────────────────────

fn convert_lisette_diag(
    diag: &lisette_diagnostics::LisetteDiagnostic,
    source: &str,
) -> JsDiagnostic {
    let message = diag.plain_message().to_string();
    let severity = if diag.is_error() {
        "error"
    } else if diag.is_info() {
        "info"
    } else {
        "warning"
    }
    .to_string();

    let offset = diag.primary_offset();
    let (line, col, end_line, end_col) = {
        let (l, c) = offset_to_line_col(source, offset);
        (l, c, None, None)
    };

    JsDiagnostic {
        severity,
        message,
        line,
        col,
        end_line,
        end_col,
        code: diag.code_str().map(str::to_string),
    }
}

// ─── Core pipeline ────────────────────────────────────────────────────────────

const PLAYGROUND_FILE: &str = "playground.lis";

/// Result of running the analysis pipeline (parse + semantic check).
#[allow(dead_code)]
struct AnalysisResult {
    analysis: Analysis,
    diagnostics: Vec<JsDiagnostic>,
    has_parse_errors: bool,
}

/// Run parse + semantic analysis, returning the full result for IDE features.
fn run_analysis(code: &str) -> AnalysisResult {
    let mut loader = MemoryLoader::new();
    loader.add_file("_entry_", PLAYGROUND_FILE, code);

    let input = AnalyzeInput {
        load_siblings: false,
        scope: AnalysisScope::Standalone,
        loader: &loader,
        entry: Some(EntryFile::new(
            code.to_string(),
            PLAYGROUND_FILE.to_string(),
            PLAYGROUND_FILE.to_string(),
        )),
        locator: &lisette_deps::TypedefLocator::default(),
        compile_phase: CompilePhase::Check,
        project_kind: ProjectKind::Binary,
        go_module: "",
        disable_cache: false,
    };

    let analysis = analyze(input);
    let has_parse_errors = analysis.has_parse_errors();
    let diagnostics = analysis
        .diagnostics()
        .iter()
        .map(|diagnostic| convert_lisette_diag(diagnostic, code))
        .collect();

    AnalysisResult {
        analysis,
        diagnostics,
        has_parse_errors,
    }
}

fn run_pipeline(
    code: &str,
    phase: CompilePhase,
) -> (Vec<lisette_emit::OutputFile>, Vec<JsDiagnostic>) {
    let mut loader = MemoryLoader::new();
    loader.add_file("_entry_", PLAYGROUND_FILE, code);

    let input = AnalyzeInput {
        load_siblings: false,
        scope: AnalysisScope::Standalone,
        loader: &loader,
        entry: Some(EntryFile::new(
            code.to_string(),
            PLAYGROUND_FILE.to_string(),
            PLAYGROUND_FILE.to_string(),
        )),
        compile_phase: phase,
        project_kind: ProjectKind::Binary,
        locator: &lisette_deps::TypedefLocator::default(),
        go_module: "",
        disable_cache: false,
    };

    let analysis = analyze(input);
    let mut diagnostics = analysis
        .diagnostics()
        .iter()
        .map(|diagnostic| convert_lisette_diag(diagnostic, code))
        .collect();
    if analysis.has_parse_errors() {
        return (Vec::new(), diagnostics);
    }

    if matches!(phase, CompilePhase::Check) || analysis.failed() {
        return (vec![], diagnostics);
    }

    let go_files = match lisette_emit::Planner::emit(
        &analysis.emit_input,
        "",
        "main",
        lisette_emit::EmitOptions { sourcemap: false, emit_tests: false },
    ) {
        Ok(files) => files,
        Err(emit_diagnostics) => {
            diagnostics.extend(
                emit_diagnostics
                    .iter()
                    .map(|diagnostic| convert_lisette_diag(diagnostic, code)),
            );
            Vec::new()
        }
    };

    (go_files, diagnostics)
}

// ─── AST traversal (ported from lisette-lsp/traversal.rs) ────────────────────

fn offset_in_span(offset: u32, span: &Span) -> bool {
    offset >= span.byte_offset && offset < span.byte_offset + span.byte_length
}

/// Find the deepest expression containing the byte offset.
fn find_expression_at<'a>(items: &'a [Expression], offset: u32) -> Option<&'a Expression> {
    items.iter().find_map(|item| {
        if !offset_in_span(offset, &item.get_span()) {
            return None;
        }
        let mut current = item;
        loop {
            match child_containing_offset(current, offset) {
                Some(child) => current = child,
                None => return Some(current),
            }
        }
    })
}

/// Find which immediate child of `expression` contains `offset`.
fn child_containing_offset<'a>(expression: &'a Expression, offset: u32) -> Option<&'a Expression> {
    expression
        .children()
        .into_iter()
        .find(|child| offset_in_span(offset, &child.get_span()))
}

/// Find the deepest `Call` expression where `offset` falls in the arg region.
fn find_enclosing_call<'a>(items: &'a [Expression], offset: u32) -> Option<&'a Expression> {
    items.iter().find_map(|item| {
        if !offset_in_span(offset, &item.get_span()) {
            return None;
        }
        let mut current = item;
        let mut deepest_call = None;
        loop {
            if let Expression::Call { expression, .. } = current {
                let s = expression.get_span();
                if offset >= s.byte_offset + s.byte_length {
                    deepest_call = Some(current);
                }
            }
            match child_containing_offset(current, offset) {
                Some(child) => current = child,
                None => break,
            }
        }
        deepest_call
    })
}

// ─── Hover helpers (ported from lisette-lsp/hover.rs) ─────────────────────────

/// Get the type and span for the hovered expression, descending into patterns.
fn get_hover_type_and_span(expression: &Expression, offset: u32) -> (Type, Span) {
    match expression {
        Expression::Let { binding, .. } | Expression::For { binding, .. } => {
            let pat_span = binding.pattern.get_span();
            if offset_in_span(offset, &pat_span) {
                return (binding.ty.clone(), pat_span);
            }
        }
        Expression::Function { params, .. } | Expression::Lambda { params, .. } => {
            for p in params {
                let ps = p.pattern.get_span();
                if offset_in_span(offset, &ps) {
                    return (p.ty.clone(), ps);
                }
            }
        }
        Expression::StructCall { field_assignments, .. } => {
            if let Some(fa) = field_assignments.iter().find(|fa| offset_in_span(offset, &fa.name_span)) {
                return (fa.value.get_type(), fa.name_span);
            }
        }
        Expression::Struct { fields, .. } => {
            if let Some(f) = fields.iter().find(|f| offset_in_span(offset, &f.name_span)) {
                return (f.ty.clone(), f.name_span);
            }
        }
        _ => {}
    }
    (expression.get_type(), expression.get_span())
}

/// Format a type for hover display.
fn format_hover_markdown(expression: &Expression, ty: &Type, source: &str, span: &Span) -> String {
    let type_str = format!("{}", ty);

    // For definitions, show the definition signature (but only when hovering the
    // expression's own span, not a child like a parameter or field).
    let expr_span = expression.get_span();
    let hovering_whole_expr = *span == expr_span;
    if hovering_whole_expr {
        match expression {
            Expression::Function { name, params, return_type, .. } => {
                let params_str: Vec<String> = params.iter().map(|p| {
                    let pname = extract_word(source, p.pattern.get_span());
                    let pty = format!("{}", p.ty);
                    format!("{}: {}", pname, pty)
                }).collect();
                let ret = format!("{}", return_type);
                let ret_part = if ret == "()" { String::new() } else { format!(" -> {}", ret) };
                return format!("```lisette\nfn {}({}){}\n```", name, params_str.join(", "), ret_part);
            }
            Expression::Struct { name, fields, .. } => {
                let fields_str: Vec<String> = fields.iter().map(|f| {
                    format!("  {}: {}", f.name, f.ty)
                }).collect();
                return format!("```lisette\nstruct {} {{\n{}\n}}\n```", name, fields_str.join(",\n"));
            }
            Expression::Enum { name, variants, .. } => {
                let vars: Vec<String> = variants.iter().map(|v| format!("  {}", v.name)).collect();
                return format!("```lisette\nenum {} {{\n{}\n}}\n```", name, vars.join(",\n"));
            }
            _ => {}
        }
    }

    // For identifiers, variables, parameters — show "name: Type"
    let word = extract_word(source, *span);
    if !word.is_empty() && word.chars().next().map_or(false, |c| c.is_alphabetic() || c == '_') {
        return format!("```lisette\n{}: {}\n```", word, type_str);
    }

    format!("```lisette\n{}\n```", type_str)
}

/// Extract a word from source at the given span.
fn extract_word(source: &str, span: Span) -> String {
    let start = span.byte_offset as usize;
    let end = (span.byte_offset + span.byte_length) as usize;
    if end <= source.len() {
        source[start..end].to_string()
    } else {
        String::new()
    }
}

// ─── Completion helpers ──────────────────────────────────────────────────────

/// Detect if the cursor is after a dot on a module prefix or value.
fn get_module_prefix<'a>(source: &'a str, offset: usize) -> Option<&'a str> {
    if offset == 0 || offset > source.len() {
        return None;
    }
    let before = &source[..offset];
    // Walk backwards past whitespace
    let trimmed = before.trim_end();
    if !trimmed.ends_with('.') {
        return None;
    }
    let before_dot = &trimmed[..trimmed.len() - 1].trim_end();
    // Extract the identifier before the dot
    let start = before_dot.rfind(|c: char| !c.is_alphanumeric() && c != '_').map(|i| i + 1).unwrap_or(0);
    let prefix = &before_dot[start..];
    if prefix.is_empty() { None } else { Some(prefix) }
}

fn definition_to_completion_kind(def: &Definition) -> &'static str {
    match &def.body {
        DefinitionBody::Struct { .. } => "type",
        DefinitionBody::Enum { .. } => "enum",
        DefinitionBody::Interface { .. } => "type",
        DefinitionBody::TypeAlias { .. } => "type",
        DefinitionBody::Value { .. } => {
            if matches!(&def.ty, Type::Function(_)) { "function" } else { "variable" }
        }
    }
}

/// Get the type name from a resolved Type (unwrap Ref<T> etc).
fn type_name(ty: &Type) -> Option<String> {
    let stripped = ty.strip_refs();
    match stripped {
        Type::Nominal { id, .. } => Some(id.to_string()),
        _ => None,
    }
}

/// Build all completions for a dot-access context.
fn build_dot_completions(
    prefix: &str,
    analysis: &Analysis,
    file_items: &[Expression],
    offset: u32,
) -> Vec<JsCompletionItem> {
    let mut items = Vec::new();

    // Check if prefix is a module alias
    let module_name = file_items.iter().find_map(|item| {
        if let Expression::ModuleImport { name, alias, .. } = item {
            let effective_alias = match alias {
                Some(lisette_syntax::ast::ImportAlias::Named(a, _)) => a.to_string(),
                _ => name.strip_prefix("go:").unwrap_or(name)
                    .split('/').next_back().unwrap_or(name).to_string(),
            };
            if effective_alias == prefix {
                Some(name.to_string())
            } else {
                None
            }
        } else {
            None
        }
    });

    if let Some(module_name) = &module_name {
        // Module-level completions: find all definitions qualified with this module
        for (qname, def) in &analysis.emit_input.definitions {
            let qname_str = qname.as_str();
            // Match "module_name.X" definitions
            if let Some(member) = qname_str.strip_prefix(module_name.as_str())
                .and_then(|rest| rest.strip_prefix('.'))
            {
                // Skip nested members (e.g. "module.Type.method")
                if !member.contains('.') {
                    items.push(JsCompletionItem {
                        label: member.to_string(),
                        kind: definition_to_completion_kind(def),
                        detail: Some(format!("{}", def.ty)),
                        insert_text: None,
                    });
                }
            }
        }
        return items;
    }

    // Not a module — try to resolve as a variable/expression type
    // Find the expression before the dot and get its type
    if let Some(expr) = find_expression_at(file_items, offset.saturating_sub(2)) {
        let ty = expr.get_type();
        if let Some(tid) = type_name(&ty) {
            // Find struct fields
            if let Some(def) = analysis.emit_input.definitions.get(tid.as_str()) {
                if let DefinitionBody::Struct { fields, .. } = &def.body {
                    for field in fields.iter() {
                        items.push(JsCompletionItem {
                            label: field.name.to_string(),
                            kind: "field",
                            detail: Some(format!("{}", field.ty)),
                            insert_text: None,
                        });
                    }
                }
            }

            // Find methods (definitions like "TypeName.methodName")
            let prefix_dot = format!("{}.", tid);
            for (qname, def) in &analysis.emit_input.definitions {
                if let Some(method) = qname.as_str().strip_prefix(&prefix_dot) {
                    if !method.contains('.') {
                        items.push(JsCompletionItem {
                            label: method.to_string(),
                            kind: if matches!(def.ty, Type::Function(_)) { "method" } else { "field" },
                            detail: Some(format!("{}", def.ty)),
                            insert_text: None,
                        });
                    }
                }
            }

            // For enums, add variants
            if let Some(def) = analysis.emit_input.definitions.get(tid.as_str()) {
                if let DefinitionBody::Enum { variants, .. } = &def.body {
                    for v in variants {
                        items.push(JsCompletionItem {
                            label: v.name.to_string(),
                            kind: "enum",
                            detail: None,
                            insert_text: None,
                        });
                    }
                }
            }
        }
    }

    items
}

// ─── Public WASM API ──────────────────────────────────────────────────────────

/// Format Lisette source. Returns the formatted source, or the original on failure.
#[wasm_bindgen]
pub fn format(code: &str) -> String {
    match lisette_format::format_source(code) {
        Ok(formatted) => formatted,
        Err(_) => code.to_string(),
    }
}

/// Type-check source and return a JSON array of diagnostics.
#[wasm_bindgen]
pub fn check(code: &str) -> String {
    let (_files, diags) = run_pipeline(code, CompilePhase::Check);
    serde_json::to_string(&diags).unwrap_or_else(|_| "[]".to_string())
}

/// Compile Lisette → Go. Returns a JSON object:
/// `{ "ok": bool, "go_source": "...", "diagnostics": [...] }`
#[wasm_bindgen]
pub fn compile(code: &str) -> String {
    let (files, diags) = run_pipeline(code, CompilePhase::Emit);
    let has_errors = diags.iter().any(|d| d.severity == "error");

    let go_source = if !has_errors && !files.is_empty() {
        Some(
            files
                .iter()
                .map(|f| format!("// === {} ===\n{}", f.name, f.to_go()))
                .collect::<Vec<_>>()
                .join("\n\n"),
        )
    } else {
        None
    };

    let result = JsCompileResult {
        ok: !has_errors && go_source.is_some(),
        go_source,
        diagnostics: diags,
    };

    serde_json::to_string(&result).unwrap_or_else(|_| {
        r#"{"ok":false,"diagnostics":[{"severity":"error","message":"Internal error","line":1,"col":1}]}"#.to_string()
    })
}

/// Semantic completion items at byte offset (JSON array).
#[wasm_bindgen]
pub fn complete(code: &str, offset: u32) -> String {
    let result = run_analysis(code);
    if result.has_parse_errors {
        return "[]".to_string();
    }

    let items_refs: Vec<Expression> = result.analysis.emit_input.files.values()
        .flat_map(|f| f.items.clone())
        .collect();

    // Check for dot-access context
    if let Some(prefix) = get_module_prefix(code, offset as usize) {
        let items = build_dot_completions(prefix, &result.analysis, &items_refs, offset);
        return serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_string());
    }

    // Top-level completions: local definitions in the entry module
    let mut items: Vec<JsCompletionItem> = Vec::new();
    for (qname, def) in &result.analysis.emit_input.definitions {
        let qname_str = qname.as_str();
        // Only show definitions from the entry module (unqualified or _entry_ prefixed)
        let label = if let Some(rest) = qname_str.strip_prefix("_entry_.") {
            if rest.contains('.') { continue; } // skip methods
            rest.to_string()
        } else if !qname_str.contains('.') {
            qname_str.to_string()
        } else {
            continue;
        };

        items.push(JsCompletionItem {
            label,
            kind: definition_to_completion_kind(def),
            detail: Some(format!("{}", def.ty)),
            insert_text: None,
        });
    }

    serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_string())
}

/// Hover info at byte offset. Returns JSON `{ "markdown": "...", ... }` or empty string.
#[wasm_bindgen]
pub fn hover(code: &str, offset: u32) -> String {
    let result = run_analysis(code);
    if result.has_parse_errors {
        return String::new();
    }

    let items: Vec<Expression> = result.analysis.emit_input.files.values()
        .flat_map(|f| f.items.clone())
        .collect();

    let expression = match find_expression_at(&items, offset) {
        Some(e) => e,
        None => return String::new(),
    };

    let (ty, span) = get_hover_type_and_span(expression, offset);

    // Skip hover for ignored/unit types on non-definition expressions
    let type_str = format!("{}", ty);
    if type_str == "_" {
        return String::new();
    }

    let markdown = format_hover_markdown(expression, &ty, code, &span);

    // Add doc comment if available
    let mut full_markdown = markdown;
    if let Expression::Identifier { resolution: IdentifierResolution::Definition(qname), .. } = expression {
        if let Some(def) = result.analysis.emit_input.definitions.get(qname.as_str()) {
            if let Some(doc) = &def.doc {
                full_markdown.push_str(&format!("\n\n---\n\n{}", doc));
            }
        }
    }

    let (sl, sc) = offset_to_line_col(code, span.byte_offset as usize);
    let (el, ec) = offset_to_line_col(code, (span.byte_offset + span.byte_length) as usize);

    let hover_result = JsHoverResult {
        markdown: full_markdown,
        start_line: Some(sl),
        start_col: Some(sc),
        end_line: Some(el),
        end_col: Some(ec),
    };

    serde_json::to_string(&hover_result).unwrap_or_else(|_| String::new())
}

/// Go-to-definition at byte offset. Returns JSON `{ "line", "col", "end_line", "end_col" }` or empty.
#[wasm_bindgen]
pub fn goto_definition(code: &str, offset: u32) -> String {
    let result = run_analysis(code);
    if result.has_parse_errors {
        return String::new();
    }

    // First check the direct usage-to-definition span mapping.
    for usage in result.analysis.usages() {
        if offset >= usage.usage_span.byte_offset
            && offset < usage.usage_span.byte_offset + usage.usage_span.byte_length
        {
            let (sl, sc) = offset_to_line_col(code, usage.definition_span.byte_offset as usize);
            let (el, ec) = offset_to_line_col(code, (usage.definition_span.byte_offset + usage.definition_span.byte_length) as usize);
            let def_result = JsDefinitionResult {
                line: sl, col: sc, end_line: el, end_col: ec,
            };
            return serde_json::to_string(&def_result).unwrap_or_else(|_| String::new());
        }
    }

    // Fall back: check if the expression has a qualified name pointing to a definition
    let items: Vec<Expression> = result.analysis.emit_input.files.values()
        .flat_map(|f| f.items.clone())
        .collect();

    if let Some(expression) = find_expression_at(&items, offset) {
        if let Expression::Identifier { resolution: IdentifierResolution::Definition(qname), .. } = expression {
            if let Some(def) = result.analysis.emit_input.definitions.get(qname.as_str()) {
                if let Some(name_span) = def.name_span {
                    let (sl, sc) = offset_to_line_col(code, name_span.byte_offset as usize);
                    let (el, ec) = offset_to_line_col(code, (name_span.byte_offset + name_span.byte_length) as usize);
                    let def_result = JsDefinitionResult {
                        line: sl, col: sc, end_line: el, end_col: ec,
                    };
                    return serde_json::to_string(&def_result).unwrap_or_else(|_| String::new());
                }
            }
        }
    }

    String::new()
}

/// Signature help for a function call at byte offset. Returns JSON or empty string.
#[wasm_bindgen]
pub fn signature_help(code: &str, offset: u32) -> String {
    let result = run_analysis(code);
    if result.has_parse_errors {
        return String::new();
    }

    let items: Vec<Expression> = result.analysis.emit_input.files.values()
        .flat_map(|f| f.items.clone())
        .collect();

    let call_expr = match find_enclosing_call(&items, offset) {
        Some(e) => e,
        None => return String::new(),
    };

    if let Expression::Call { expression: callee, args, .. } = call_expr {
        let callee_ty = callee.get_type();
        let callee_ty_inner = match &callee_ty {
            Type::Forall { body, .. } => body.as_ref(),
            other => other,
        };
        if let Type::Function(f) = callee_ty_inner {
            // Build the signature label
            let callee_name = match callee.as_ref() {
                Expression::Identifier { value, .. } => unqualified_name(value),
                Expression::DotAccess { member, .. } => member.as_str(),
                _ => "fn",
            };
            let param_strs: Vec<String> = f.params.iter().map(|p| match &p.name {
                Some(name) => format!("{}: {}", name, p.ty),
                None => p.ty.to_string(),
            }).collect();
            let ret_str = format!("{}", f.return_type);
            let ret_part = if ret_str == "()" { String::new() } else { format!(" -> {}", ret_str) };
            let label = format!("{}({}){}", callee_name, param_strs.join(", "), ret_part);

            // Determine active parameter by counting args whose spans end before offset
            let active = args.iter().filter(|a| {
                let s = a.get_span();
                s.byte_offset + s.byte_length <= offset
            }).count() as u32;

            let sig = JsSignatureHelp {
                label,
                parameters: param_strs,
                active_parameter: active,
            };

            return serde_json::to_string(&sig).unwrap_or_else(|_| String::new());
        }
    }

    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_diagnostic_converts_to_info_severity() {
        let diag = lisette_diagnostics::LisetteDiagnostic::info("advisory");
        assert_eq!(convert_lisette_diag(&diag, "").severity, "info");
    }

    #[test]
    fn warning_diagnostic_converts_to_warning_severity() {
        let diag = lisette_diagnostics::LisetteDiagnostic::warn("w");
        assert_eq!(convert_lisette_diag(&diag, "").severity, "warning");
    }

    #[test]
    fn compile_reports_emitter_diagnostics() {
        let source = r#"
import FooBar "go:fmt"

pub fn foo_bar() -> int { 1 }

fn main() {
  let _ = FooBar.Println("x")
  let _ = foo_bar()
}
"#;

        let (files, diagnostics) = run_pipeline(source, CompilePhase::Emit);

        assert!(files.is_empty());
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code.as_deref() == Some("emit.go_name_collision"))
        );
    }
}
