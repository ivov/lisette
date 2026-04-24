mod bindings;
pub(crate) mod calls;
mod collectors;
pub(crate) mod control_flow;
pub(crate) mod definitions;
pub(crate) mod expressions;
mod imports;
pub(crate) mod names;
mod output;
pub(crate) mod patterns;
pub(crate) mod queries;
pub(crate) mod statements;
pub(crate) mod types;
mod utils;

pub(crate) use bindings::Bindings;
pub(crate) use calls::go_interop::GoCallStrategy;
pub(crate) use definitions::enum_layout::EnumLayout;
pub(crate) use names::go_name;
pub(crate) use names::go_name::escape_reserved;
pub(crate) use output::OutputCollector;
pub(crate) use types::emitter::{ArmPosition, EmitFlags, LineIndex, LoopContext, Position};
pub(crate) use types::prelude::PreludeType;
pub(crate) use utils::is_order_sensitive;
pub(crate) use utils::write_line;

pub use names::go_name::PRELUDE_IMPORT_PATH;
pub use output::OutputFile;

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::sync::Arc;

use ecow::EcoString;
use imports::ImportBuilder;
use syntax::ast::{Generic, Span};
use syntax::program::{Definition, EmitInput, File, ModuleId, MutationInfo, UnusedInfo};
use syntax::types::{Symbol, Type};

#[derive(Clone, Debug, Default)]
pub struct EmitOptions {
    pub debug: bool,
}

pub struct TestEmitConfig<'a> {
    pub definitions: &'a HashMap<Symbol, Definition>,
    pub module_id: &'a str,
    pub go_module: &'a str,
    pub unused: &'a UnusedInfo,
    pub mutations: &'a MutationInfo,
    pub ufcs_methods: &'a HashSet<(String, String)>,
    pub go_package_names: &'a HashMap<String, String>,
}

struct EmitContext<'a> {
    definitions: &'a HashMap<Symbol, Definition>,
    unused: &'a UnusedInfo,
    mutations: &'a MutationInfo,
    ufcs_methods: &'a HashSet<(String, String)>,
    go_package_names: &'a HashMap<String, String>,
    entry_module: ModuleId,
    go_module: String,
    options: EmitOptions,
    /// file_id -> byte offset to line lookup.
    line_indexes: Arc<HashMap<u32, LineIndex>>,
}

struct ModuleData {
    make_functions: HashMap<String, String>,
    enum_layouts: HashMap<String, EnumLayout>,
    /// Fields that were exported due to serialization tags (e.g. `#[json]`).
    /// Key is "TypeId.field_name". Checked during field access to match
    /// the capitalization used in the struct definition.
    tag_exported_fields: HashSet<String>,
    /// Method names that appear in any pub interface.
    /// These must be capitalized in Go to satisfy the interface,
    /// regardless of the concrete method's own visibility.
    exported_method_names: HashSet<String>,
    /// Bounds from constrained impl blocks, keyed by receiver name.
    /// Go requires type parameter constraints on the type definition itself,
    /// so we pre-scan impl blocks and merge their bounds into struct generics.
    impl_bounds: HashMap<String, Vec<Generic>>,
    /// Types that have unconstrained impl blocks (impl<T> Type<T> with no bounds).
    /// Used to detect when a type has both constrained and unconstrained impl blocks.
    unconstrained_impl_receivers: HashSet<String>,
    /// Maps module IDs to their import aliases (e.g., "lib" → "L", "models/user" → "user").
    /// Used when emitting cross-module references to use the correct alias.
    module_aliases: HashMap<String, String>,
    /// Reverse of `module_aliases`: maps alias → module_id (e.g., "L" → "lib").
    /// Used for O(1) lookup when resolving an alias back to a module name.
    reverse_module_aliases: HashMap<String, String>,
    /// Generic type parameters whose Ref has been absorbed into the Go type parameter.
    /// When a function has `item: Ref<T>` where T has interface bounds, Go requires
    /// T itself (not *T) to satisfy the interface. So we emit `item T` and let Go
    /// infer T = *ConcreteType. Ref<T> for these params should emit as just T.
    absorbed_ref_generics: HashSet<String>,
    /// Pre-computed wrapping strategy per Go function.
    go_call_strategies: HashMap<String, GoCallStrategy>,
}

struct ScopeState {
    next_var: usize,
    bindings: Bindings,
    /// Stack of Go variable names declared at each scope level.
    declared: Vec<HashSet<String>>,
    /// Current Go block scope depth (0 = function level).
    scope_depth: usize,
    /// Stack of loop contexts (result var + optional label) per nesting level.
    loop_stack: Vec<LoopContext>,
    /// Go variable names currently used as block-to-var assign targets.
    assign_targets: HashSet<String>,
    /// Go identifiers emitted as `const` (not `var`).
    go_const_bindings: HashSet<String>,
}

impl ScopeState {
    fn reset_for_top_level(&mut self) {
        self.next_var = 0;
        self.bindings.reset();
        self.declared.clear();
        self.declared.push(HashSet::default());
    }
}

pub struct Emitter<'a> {
    ctx: EmitContext<'a>,
    module: ModuleData,
    scope: ScopeState,

    current_module: ModuleId,

    synthesized_adapter_types: HashMap<(EcoString, EcoString), String>,
    pending_adapter_types: Vec<String>,

    // Per-file accumulated state (reset between files)
    flags: EmitFlags,
    ensure_imported: HashSet<ModuleId>,

    // Temporary emission context (saved/restored per-expression).
    // These are implicit arguments — ideally parameters, but
    // plumbing through deep call chains is impractical.
    position: Position,
    current_return_context: Option<Type>,
    /// Target type for Option/Result assignment (interface coercion).
    assign_target_ty: Option<Type>,
    /// Generic function identifiers should NOT add type args when used as callees
    /// (the call site handles instantiation), only when used as values.
    emitting_call_callee: bool,
    /// Set while emitting expressions that will appear in Go `if`/`for`/`switch`
    /// conditions. Generic composite literals (`Type[Args]{...}`) need inner parens
    /// in these contexts because gofmt strips outer condition parens for generics.
    in_condition: bool,
    /// When true, `emit_regular_call` skips array return wrapping (`arr := call; arr[:]`)
    /// and returns the raw call string. Set by `emit_go_call_discarded` for discarded calls
    /// where the array-to-slice conversion is unnecessary.
    skip_array_return_wrap: bool,
    /// Declared slot type during tuple staging; recovers Go alias type args that
    /// call-site inference loses in assign-position match arms.
    current_slot_expected_ty: Option<Type>,
}

impl<'a> Emitter<'a> {
    pub fn emit(analysis: &'a EmitInput, go_module: &str, options: EmitOptions) -> Vec<OutputFile> {
        let mut output = vec![];

        let line_indexes: Arc<HashMap<u32, LineIndex>> = Arc::new(if options.debug {
            analysis
                .files
                .iter()
                .map(|(file_id, file)| {
                    let path = if file.module_id == analysis.entry_module_id {
                        format!("src/{}", file.name)
                    } else {
                        format!("{}/{}", file.module_id, file.name)
                    };
                    (*file_id, LineIndex::from_source(path, &file.source))
                })
                .collect()
        } else {
            HashMap::default()
        });

        for (module_id, module_info) in &analysis.modules {
            if analysis.cached_modules.contains(module_id) {
                continue;
            }

            let ctx = EmitContext {
                definitions: &analysis.definitions,
                unused: &analysis.unused,
                mutations: &analysis.mutations,
                ufcs_methods: &analysis.ufcs_methods,
                go_package_names: &analysis.go_package_names,
                entry_module: analysis.entry_module_id.to_string(),
                go_module: go_module.to_string(),
                options: options.clone(),
                line_indexes: line_indexes.clone(),
            };
            let mut emitter = Self::new(ctx, module_id);

            let files: Vec<_> = module_info
                .file_ids
                .iter()
                .filter_map(|fid| analysis.files.get(fid))
                .collect();

            let mut module_output = emitter.emit_files(&files, module_id);

            if module_id != &analysis.entry_module_id {
                for file in &mut module_output {
                    file.name = format!("{}/{}", module_info.path, file.name);
                }
            }

            output.extend(module_output);
        }

        output
    }

    pub fn new_for_tests(config: &TestEmitConfig<'a>, source: Option<&str>) -> Self {
        let (debug, line_indexes) = match source {
            Some(src) => (
                true,
                Arc::new(HashMap::from_iter([(
                    0u32,
                    LineIndex::from_source("src/test.lis".to_string(), src),
                )])),
            ),
            None => (false, Arc::new(HashMap::default())),
        };
        let ctx = EmitContext {
            definitions: config.definitions,
            unused: config.unused,
            mutations: config.mutations,
            ufcs_methods: config.ufcs_methods,
            go_package_names: config.go_package_names,
            entry_module: config.module_id.to_string(),
            go_module: config.go_module.to_string(),
            options: EmitOptions { debug },
            line_indexes,
        };
        Self::new(ctx, config.module_id)
    }

    fn new(ctx: EmitContext<'a>, current_module: &str) -> Self {
        Self {
            ctx,
            module: ModuleData {
                make_functions: HashMap::default(),
                enum_layouts: HashMap::default(),
                tag_exported_fields: HashSet::default(),
                exported_method_names: HashSet::default(),
                impl_bounds: HashMap::default(),
                unconstrained_impl_receivers: HashSet::default(),
                module_aliases: HashMap::default(),
                reverse_module_aliases: HashMap::default(),
                absorbed_ref_generics: HashSet::default(),
                go_call_strategies: HashMap::default(),
            },
            scope: ScopeState {
                next_var: 0,
                bindings: Bindings::new(),
                declared: vec![HashSet::default()],
                scope_depth: 0,
                loop_stack: Vec::new(),
                assign_targets: HashSet::default(),
                go_const_bindings: HashSet::default(),
            },
            current_module: current_module.to_string(),
            synthesized_adapter_types: HashMap::default(),
            pending_adapter_types: Vec::new(),
            flags: EmitFlags::default(),
            ensure_imported: HashSet::default(),
            position: Position::Expression,
            current_return_context: None,
            assign_target_ty: None,
            emitting_call_callee: false,
            in_condition: false,
            skip_array_return_wrap: false,
            current_slot_expected_ty: None,
        }
    }

    pub(crate) fn emit_condition_operand(
        &mut self,
        output: &mut String,
        expression: &syntax::ast::Expression,
    ) -> String {
        let prev = self.in_condition;
        self.in_condition = true;
        let result = self.emit_operand(output, expression);
        self.in_condition = prev;
        result
    }

    pub(crate) fn push_loop(&mut self, result_var: impl Into<String>) {
        self.scope.loop_stack.push(LoopContext {
            result_var: result_var.into(),
            label: None,
        });
    }

    pub(crate) fn pop_loop(&mut self) {
        self.scope.loop_stack.pop();
    }

    pub(crate) fn current_loop_result_var(&self) -> Option<&str> {
        self.scope
            .loop_stack
            .last()
            .map(|ctx| ctx.result_var.as_str())
    }

    pub(crate) fn current_loop_label(&self) -> Option<&str> {
        self.scope
            .loop_stack
            .last()
            .and_then(|ctx| ctx.label.as_deref())
    }

    pub(crate) fn with_position<F, R>(&mut self, position: Position, f: F) -> R
    where
        F: FnOnce(&mut Self) -> R,
    {
        let saved = std::mem::replace(&mut self.position, position);
        let result = f(self);
        self.position = saved;
        result
    }

    pub(crate) fn wrap_value(&self, value: &str) -> String {
        if value.is_empty() {
            return String::new();
        }
        match &self.position {
            Position::Tail => format!("return {}\n", value),
            Position::Statement => format!("{}\n", value),
            Position::Expression => value.to_string(),
            Position::Assign(var) => format!("{} = {}\n", var, value),
        }
    }

    pub(crate) fn emit_unreachable_if_needed(&self, output: &mut String, has_catchall: bool) {
        if self.position.is_tail() && !has_catchall {
            output.push_str("panic(\"unreachable\")\n");
        }
    }

    /// Computes the position for match arms based on the current position and result type.
    ///
    /// For control flow constructs that need to produce values (match, if-else), this
    /// determines whether we need a temporary result variable and what position the
    /// inner branches should use.
    ///
    /// If `output` is provided, declares the result variable when needed.
    pub(crate) fn compute_arm_position(
        &mut self,
        output: Option<&mut String>,
        ty: &Type,
    ) -> ArmPosition {
        if self.position.is_tail() {
            return ArmPosition::from_position(Position::Tail);
        }

        if let Some(var) = self.position.assign_target() {
            return ArmPosition::from_position(Position::Assign(var.to_string()));
        }

        if self.position.is_expression() && !ty.is_unit() {
            let var = self.fresh_var(Some("result"));
            if let Some(out) = output {
                let go_ty = self.go_type_as_string(ty);
                write_line!(out, "var {} {}", var, go_ty);
            }
            return ArmPosition::with_result_var(var);
        }

        ArmPosition::from_position(Position::Statement)
    }

    /// Checks if a Go variable name has been declared in the current scope.
    /// Tracks declarations at each scope level so variable shadowing works correctly.
    /// Returns true if this is a new declaration (use :=), false if already declared (use =).
    pub(crate) fn try_declare(&mut self, go_name: &str) -> bool {
        if let Some(current_scope) = self.scope.declared.last_mut() {
            if current_scope.contains(go_name) {
                false
            } else {
                current_scope.insert(go_name.to_string());
                true
            }
        } else {
            true
        }
    }

    pub(crate) fn is_declared(&self, go_name: &str) -> bool {
        self.scope
            .declared
            .iter()
            .any(|scope| scope.contains(go_name))
    }

    /// Unconditionally marks a Go variable name as declared in the current scope.
    /// Use this for parameters, which are always "declared" at function entry.
    pub(crate) fn declare(&mut self, go_name: &str) {
        if let Some(current_scope) = self.scope.declared.last_mut() {
            current_scope.insert(go_name.to_string());
        }
    }

    pub(crate) fn enter_scope(&mut self) {
        self.scope.scope_depth += 1;
        self.scope.bindings.save();
        self.scope.declared.push(HashSet::default());
    }

    pub(crate) fn exit_scope(&mut self) {
        self.scope.scope_depth = self.scope.scope_depth.saturating_sub(1);
        self.scope.bindings.restore();
        if self.scope.declared.len() > 1 {
            self.scope.declared.pop();
        }
    }

    pub(crate) fn current_module(&self) -> &str {
        &self.current_module
    }

    pub(crate) fn module_alias_for_type(&self, ty: &Type) -> Option<String> {
        if let Type::Nominal { id, .. } = ty {
            let module = names::go_name::module_of_type_id(id);
            self.module.module_aliases.get(module).cloned()
        } else {
            None
        }
    }

    pub(crate) fn maybe_line_directive(&self, span: &Span) -> String {
        if !self.ctx.options.debug || span.is_dummy() {
            return String::new();
        }

        let Some(source) = self.ctx.line_indexes.get(&span.file_id) else {
            return String::new();
        };

        let line = source.line_for_offset(span.byte_offset);
        let col = source.col_for_offset(span.byte_offset);

        format!("//line {}:{}:{}\n", source.path, line, col)
    }

    fn unused_imports_for_current_module<'u>(
        unused: &'u UnusedInfo,
        current_module: &str,
    ) -> &'u HashSet<EcoString> {
        static EMPTY: std::sync::LazyLock<HashSet<EcoString>> =
            std::sync::LazyLock::new(HashSet::default);
        unused
            .imports_by_module
            .get(current_module)
            .unwrap_or(&EMPTY)
    }

    pub fn emit_files(&mut self, files: &[&File], module_id: &str) -> Vec<OutputFile> {
        self.current_module = module_id.to_string();
        self.collect_module_aliases(files);
        self.collect_go_call_strategies();
        self.collect_exported_method_names(files);
        self.collect_impl_bounds(files);
        self.collect_enum_layouts();
        let make_functions = self.collect_make_functions();

        let mut output_files = Vec::new();
        let should_create_bootstrap = files.len() > 1 && !make_functions.is_empty();

        let package_name = if module_id == self.ctx.entry_module {
            "main".to_string()
        } else {
            let raw = module_id.rsplit('/').next().unwrap_or(module_id);
            go_name::sanitize_package_name(raw).into_owned()
        };

        if should_create_bootstrap {
            let mut bootstrap_source = OutputCollector::new();
            for function in &make_functions {
                bootstrap_source.collect_with_blank(function.clone());
            }

            let bootstrap_source_str = bootstrap_source.render();

            let unused_imports =
                Self::unused_imports_for_current_module(self.ctx.unused, &self.current_module);
            let mut import_builder = ImportBuilder::new(
                &self.ctx.go_module,
                unused_imports,
                self.ctx.go_package_names,
            );

            // Collect imports from all files in the module to get aliased imports
            for file in files {
                import_builder.collect_from_file(file);
            }

            import_builder.extend_with_modules(&self.ensure_imported);
            if self.flags.needs_stdlib {
                import_builder.require_stdlib();
            }
            import_builder.filter_unreferenced(&bootstrap_source_str);

            output_files.push(OutputFile {
                name: "bootstrap.go".to_string(),
                imports: import_builder.build(),
                source: bootstrap_source_str,
                package_name: package_name.clone(),
            });
        }

        for file in files {
            let mut source = OutputCollector::new();

            if !should_create_bootstrap {
                for function in &make_functions {
                    source.collect_with_blank(function.clone());
                }
            }

            self.pending_adapter_types.clear();

            for expression in &file.items {
                self.scope.reset_for_top_level();
                let code = self.emit_top_item(expression);
                if !code.is_empty() {
                    source.collect_with_blank(code);
                }
            }

            for adapter_decl in std::mem::take(&mut self.pending_adapter_types) {
                source.collect_with_blank(adapter_decl);
            }

            let unused_imports =
                Self::unused_imports_for_current_module(self.ctx.unused, &self.current_module);
            let mut import_builder = ImportBuilder::new(
                &self.ctx.go_module,
                unused_imports,
                self.ctx.go_package_names,
            );
            import_builder.collect_from_file(file);

            let ensure_imported = std::mem::take(&mut self.ensure_imported);
            import_builder.extend_with_modules(&ensure_imported);

            let flags = std::mem::take(&mut self.flags);
            if flags.needs_fmt {
                import_builder.require_fmt();
            }
            if flags.needs_stdlib {
                import_builder.require_stdlib();
            }
            if flags.needs_errors {
                import_builder.require_errors();
            }
            if flags.needs_slices {
                import_builder.require_slices();
            }
            if flags.needs_strings {
                import_builder.require_strings();
            }
            if flags.needs_maps {
                import_builder.require_maps();
            }

            let rendered_source = source.render();
            import_builder.filter_unreferenced(&rendered_source);

            output_files.push(OutputFile {
                name: file.go_filename(),
                imports: import_builder.build(),
                source: rendered_source,
                package_name: package_name.clone(),
            });
        }

        output_files
    }
}
