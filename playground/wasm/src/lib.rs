//! WebAssembly bindings for the Lisette compiler.
//!
//! All functions are called directly from JavaScript via wasm-bindgen.
//! Diagnostics are serialised as JSON strings so the TS layer can decode them.

use serde::Serialize;
use wasm_bindgen::prelude::*;

use lisette_semantics::loader::{Files, Loader};
use lisette_semantics::analyze::{analyze, AnalyzeInput, CompilePhase, SemanticConfig};
use lisette_semantics::facts::Facts;
use lisette_syntax::ast::{Expression, Span};
use lisette_syntax::program::Definition;
use lisette_syntax::types::Type;
use rustc_hash::FxHashMap;

// ─── Panic hook ───────────────────────────────────────────────────────────────
#[wasm_bindgen(start)]
pub fn init() {
    console_error_panic_hook::set_once();
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

// ─── In-memory Loader ─────────────────────────────────────────────────────────

struct MemoryLoader {
    filename: String,
    source: String,
}

impl Loader for MemoryLoader {
    fn scan_folder(&self, folder: &str) -> Files {
        if folder == "_entry_" {
            let mut map: FxHashMap<String, String> = FxHashMap::default();
            map.insert(self.filename.clone(), self.source.clone());
            map
        } else {
            FxHashMap::default()
        }
    }
}

// ─── Convert LisetteDiagnostic to JsDiagnostic ────────────────────────────────

fn convert_lisette_diag(
    diag: &lisette_diagnostics::LisetteDiagnostic,
    source: &str,
) -> JsDiagnostic {
    let message = diag.plain_message().to_string();
    let severity = if diag.is_error() { "error" } else { "warning" }.to_string();

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
        code: None,
    }
}

fn convert_parse_error(e: &lisette_syntax::ParseError, source: &str) -> JsDiagnostic {
    let message = e.message.clone();
    let code = if e.code.is_empty() { None } else { Some(e.code.clone()) };

    // `labels` is `Vec<(Span, String)>` – first label is the primary one
    let (line, col, end_line, end_col) = if let Some((span, _)) = e.labels.first() {
        let offset = span.byte_offset as usize;
        let len = span.byte_length as usize;
        let (l, c) = offset_to_line_col(source, offset);
        let (el, ec) = offset_to_line_col(source, offset + len);
        (l, c, Some(el), Some(ec))
    } else {
        (1, 1, None, None)
    };

    JsDiagnostic {
        severity: "error".to_string(),
        message,
        line,
        col,
        end_line,
        end_col,
        code,
    }
}

// ─── Core pipeline ────────────────────────────────────────────────────────────

const PLAYGROUND_FILE: &str = "playground.lis";

/// Result of running the analysis pipeline (parse + semantic check).
#[allow(dead_code)]
struct AnalysisResult {
    sem_result: lisette_diagnostics::SemanticResult,
    facts: Facts,
    diagnostics: Vec<JsDiagnostic>,
    has_parse_errors: bool,
}

/// Run parse + semantic analysis, returning the full result for IDE features.
fn run_analysis(code: &str) -> AnalysisResult {
    let ast_result = lisette_syntax::build_ast(code, 0);

    let mut diagnostics: Vec<JsDiagnostic> = ast_result
        .errors
        .iter()
        .map(|e| convert_parse_error(e, code))
        .collect();

    if ast_result.failed() {
        return AnalysisResult {
            sem_result: lisette_diagnostics::SemanticResult::with_parse_errors(
                ast_result.errors,
                "_entry_",
            ),
            facts: Facts::new(),
            diagnostics,
            has_parse_errors: true,
        };
    }

    let loader = MemoryLoader {
        filename: PLAYGROUND_FILE.to_string(),
        source: code.to_string(),
    };

    let input = AnalyzeInput {
        config: SemanticConfig {
            run_lints: true,
            standalone_mode: true,
            load_siblings: false,
        },
        loader: &loader,
        source: code.to_string(),
        filename: PLAYGROUND_FILE.to_string(),
        ast: ast_result.ast,
        project_root: None,
        go_resolver: lisette_deps::GoDepResolver::default(),
        compile_phase: CompilePhase::Check,
    };

    let (sem_result, facts) = analyze(input);

    for e in &sem_result.errors {
        diagnostics.push(convert_lisette_diag(e, code));
    }
    for w in &sem_result.lints {
        diagnostics.push(convert_lisette_diag(w, code));
    }

    AnalysisResult {
        sem_result,
        facts,
        diagnostics,
        has_parse_errors: false,
    }
}

fn run_pipeline(
    code: &str,
    phase: CompilePhase,
) -> (Vec<lisette_emit::OutputFile>, Vec<JsDiagnostic>) {
    let ast_result = lisette_syntax::build_ast(code, 0);

    let mut diagnostics: Vec<JsDiagnostic> = ast_result
        .errors
        .iter()
        .map(|e| convert_parse_error(e, code))
        .collect();

    if ast_result.failed() {
        return (vec![], diagnostics);
    }

    let loader = MemoryLoader {
        filename: PLAYGROUND_FILE.to_string(),
        source: code.to_string(),
    };

    let input = AnalyzeInput {
        config: SemanticConfig {
            run_lints: true,
            standalone_mode: true,
            load_siblings: false,
        },
        loader: &loader,
        source: code.to_string(),
        filename: PLAYGROUND_FILE.to_string(),
        ast: ast_result.ast,
        project_root: None,
        compile_phase: phase.clone(),
    };

    let (sem_result, _facts) = analyze(input);

    for e in &sem_result.errors {
        diagnostics.push(convert_lisette_diag(e, code));
    }
    for w in &sem_result.lints {
        diagnostics.push(convert_lisette_diag(w, code));
    }

    if matches!(phase, CompilePhase::Check) || !sem_result.errors.is_empty() {
        return (vec![], diagnostics);
    }

    let emit_input = lisette_diagnostics::SemanticResult::into_emit_input(sem_result);
    let go_files = lisette_emit::Emitter::emit(
        &emit_input,
        "lisette_playground",
        lisette_emit::EmitOptions { debug: false },
    );

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
    let c = |e: &'a Expression| -> Option<&'a Expression> {
        offset_in_span(offset, &e.get_span()).then_some(e)
    };

    match expression {
        Expression::Function { body, .. } | Expression::Lambda { body, .. } => c(body),

        Expression::Block { items, .. }
        | Expression::TryBlock { items, .. }
        | Expression::RecoverBlock { items, .. } => items.iter().find_map(c),

        Expression::Let { value, else_block, .. } =>
            c(value).or_else(|| else_block.as_deref().and_then(c)),

        Expression::Call { expression, args, .. } =>
            c(expression).or_else(|| args.iter().find_map(c)),

        Expression::If { condition, consequence, alternative, .. } =>
            c(condition).or_else(|| c(consequence)).or_else(|| c(alternative)),

        Expression::IfLet { scrutinee, consequence, alternative, .. } =>
            c(scrutinee).or_else(|| c(consequence)).or_else(|| c(alternative)),

        Expression::Match { subject, arms, .. } => c(subject).or_else(|| {
            arms.iter().find_map(|arm| {
                arm.guard.as_deref().and_then(c).or_else(|| c(&arm.expression))
            })
        }),

        Expression::Tuple { elements, .. } => elements.iter().find_map(c),

        Expression::StructCall { field_assignments, spread, .. } =>
            field_assignments.iter().find_map(|fa| c(&fa.value))
                .or_else(|| spread.as_ref().as_ref().and_then(c)),

        Expression::DotAccess { expression, .. }
        | Expression::Return { expression, .. }
        | Expression::Propagate { expression, .. }
        | Expression::Unary { expression, .. }
        | Expression::Paren { expression, .. }
        | Expression::Const { expression, .. }
        | Expression::Reference { expression, .. }
        | Expression::Task { expression, .. }
        | Expression::Defer { expression, .. }
        | Expression::Cast { expression, .. } => c(expression),

        Expression::Assignment { target, value, .. } => c(target).or_else(|| c(value)),
        Expression::ImplBlock { methods, .. } => methods.iter().find_map(c),
        Expression::Binary { left, right, .. } => c(left).or_else(|| c(right)),
        Expression::Loop { body, .. } => c(body),
        Expression::While { condition, body, .. } => c(condition).or_else(|| c(body)),
        Expression::WhileLet { scrutinee, body, .. } => c(scrutinee).or_else(|| c(body)),
        Expression::For { iterable, body, .. } => c(iterable).or_else(|| c(body)),
        Expression::Break { value, .. } => value.as_deref().and_then(c),
        Expression::IndexedAccess { expression, index, .. } => c(expression).or_else(|| c(index)),

        Expression::Select { arms, .. } => arms.iter().find_map(|arm| {
            use lisette_syntax::ast::SelectArmPattern;
            match &arm.pattern {
                SelectArmPattern::Receive { receive_expression, body, .. } =>
                    c(receive_expression).or_else(|| c(body)),
                SelectArmPattern::Send { send_expression, body } =>
                    c(send_expression).or_else(|| c(body)),
                SelectArmPattern::MatchReceive { receive_expression, arms: match_arms } =>
                    c(receive_expression).or_else(|| {
                        match_arms.iter().find_map(|arm| {
                            arm.guard.as_deref().and_then(c).or_else(|| c(&arm.expression))
                        })
                    }),
                SelectArmPattern::WildCard { body } => c(body),
            }
        }),

        Expression::Range { start, end, .. } =>
            start.as_deref().and_then(c).or_else(|| end.as_deref().and_then(c)),

        Expression::Literal { literal, .. } => {
            use lisette_syntax::ast::{FormatStringPart, Literal};
            match literal {
                Literal::Slice(elements) => elements.iter().find_map(c),
                Literal::FormatString(parts) => parts.iter().find_map(|p| match p {
                    FormatStringPart::Expression(e) => c(e),
                    FormatStringPart::Text(_) => None,
                }),
                _ => None,
            }
        }

        Expression::Identifier { .. }
        | Expression::VariableDeclaration { .. }
        | Expression::RawGo { .. }
        | Expression::Enum { .. }
        | Expression::ValueEnum { .. }
        | Expression::Struct { .. }
        | Expression::TypeAlias { .. }
        | Expression::ModuleImport { .. }
        | Expression::Interface { .. }
        | Expression::Unit { .. }
        | Expression::Continue { .. }
        | Expression::NoOp => None,
    }
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
    match def {
        Definition::Struct { .. } => "type",
        Definition::Enum { .. } | Definition::ValueEnum { .. } => "enum",
        Definition::Interface { .. } => "type",
        Definition::TypeAlias { .. } => "type",
        Definition::Value { ty, .. } => {
            if matches!(ty, Type::Function { .. }) { "function" } else { "variable" }
        }
    }
}

/// Get the type name from a resolved Type (unwrap Ref<T> etc).
fn type_name(ty: &Type) -> Option<String> {
    match ty {
        Type::Constructor { id, params, .. } => {
            if id.as_str() == "Ref" {
                params.first().and_then(|inner| type_name(inner))
            } else {
                Some(id.to_string())
            }
        }
        _ => None,
    }
}

/// Build all completions for a dot-access context.
fn build_dot_completions(
    prefix: &str,
    sem_result: &lisette_diagnostics::SemanticResult,
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
        for (qname, def) in &sem_result.definitions {
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
                        detail: Some(format!("{}", def.ty())),
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
        let ty = expr.get_type().resolve();
        if let Some(tid) = type_name(&ty) {
            // Find struct fields
            if let Some(def) = sem_result.definitions.get(tid.as_str()) {
                if let Definition::Struct { fields, .. } = def {
                    for field in fields {
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
            for (qname, def) in &sem_result.definitions {
                if let Some(method) = qname.as_str().strip_prefix(&prefix_dot) {
                    if !method.contains('.') {
                        items.push(JsCompletionItem {
                            label: method.to_string(),
                            kind: if matches!(def.ty(), Type::Function { .. }) { "method" } else { "field" },
                            detail: Some(format!("{}", def.ty())),
                            insert_text: None,
                        });
                    }
                }
            }

            // For enums, add variants
            if let Some(def) = sem_result.definitions.get(tid.as_str()) {
                if let Definition::Enum { variants, .. } = def {
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

    let items_refs: Vec<Expression> = result.sem_result.files.values()
        .flat_map(|f| f.items.clone())
        .collect();

    // Check for dot-access context
    if let Some(prefix) = get_module_prefix(code, offset as usize) {
        let items = build_dot_completions(prefix, &result.sem_result, &items_refs, offset);
        return serde_json::to_string(&items).unwrap_or_else(|_| "[]".to_string());
    }

    // Top-level completions: local definitions in the entry module
    let mut items: Vec<JsCompletionItem> = Vec::new();
    for (qname, def) in &result.sem_result.definitions {
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
            detail: Some(format!("{}", def.ty())),
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

    let items: Vec<Expression> = result.sem_result.files.values()
        .flat_map(|f| f.items.clone())
        .collect();

    let expression = match find_expression_at(&items, offset) {
        Some(e) => e,
        None => return String::new(),
    };

    let (ty, span) = get_hover_type_and_span(expression, offset);
    let ty_resolved = ty.resolve();

    // Skip hover for ignored/unit types on non-definition expressions
    let type_str = format!("{}", ty_resolved);
    if type_str == "_" {
        return String::new();
    }

    let markdown = format_hover_markdown(expression, &ty_resolved, code, &span);

    // Add doc comment if available
    let mut full_markdown = markdown;
    if let Expression::Identifier { qualified: Some(qname), .. } = expression {
        if let Some(def) = result.sem_result.definitions.get(qname.as_str()) {
            if let Some(doc) = def.doc() {
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

    // First check facts.usages — direct usage→definition span mapping
    for usage in &result.facts.usages {
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
    let items: Vec<Expression> = result.sem_result.files.values()
        .flat_map(|f| f.items.clone())
        .collect();

    if let Some(expression) = find_expression_at(&items, offset) {
        if let Expression::Identifier { qualified: Some(qname), .. } = expression {
            if let Some(def) = result.sem_result.definitions.get(qname.as_str()) {
                if let Some(name_span) = def.name_span() {
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

    let items: Vec<Expression> = result.sem_result.files.values()
        .flat_map(|f| f.items.clone())
        .collect();

    let call_expr = match find_enclosing_call(&items, offset) {
        Some(e) => e,
        None => return String::new(),
    };

    if let Expression::Call { expression: callee, args, .. } = call_expr {
        let callee_ty = callee.get_type().resolve();
        if let Type::Function { params, return_type, .. } = &callee_ty {
            // Build the signature label
            let callee_name = callee.callee_name().unwrap_or_else(|| "fn".to_string());
            let param_strs: Vec<String> = params.iter().map(|p| format!("{}", p)).collect();
            let ret_str = format!("{}", return_type);
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
