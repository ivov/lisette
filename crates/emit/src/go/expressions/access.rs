use rustc_hash::FxHashSet as HashSet;

use syntax::ast::{Expression, Span, StructFieldAssignment, StructKind, UnaryOperator};
use syntax::program::{Definition, DotAccessKind as SemanticDotKind, ReceiverCoercion};
use syntax::types::Type;

use crate::Emitter;
use crate::go::definitions::enum_layout;
use crate::go::go_name;
use crate::go::is_order_sensitive;
use crate::go::utils::Staged;
use crate::go::write_line;

/// Context for emitting a struct literal or enum variant construction.
///
/// This bundles the analyzed information needed for struct call emission,
/// making the code easier to follow than passing multiple variables around.
struct StructCallContext {
    /// The Go type string for the struct literal
    go_type: String,
    /// If this is an enum variant, the enum-specific context
    enum_ctx: Option<EnumCallContext>,
    /// Whether this is a prelude type
    is_prelude: bool,
}

/// Context for enum variant construction within a struct call.
struct EnumCallContext {
    /// The qualified enum ID (e.g., "events.Event")
    enum_id: String,
    /// The variant being constructed (e.g., "Click")
    variant_name: String,
    /// The tag constant (e.g., "events.EventClick" or just "EventClick")
    tag_constant: String,
    /// Fields that need pointer wrapping (recursive types)
    pointer_fields: HashSet<String>,
}

impl Emitter<'_> {
    pub(crate) fn emit_dot_access(
        &mut self,
        output: &mut String,
        expression: &Expression,
        member: &str,
        result_ty: &Type,
        span: Span,
    ) -> String {
        let dot_access_kind = self.ctx.resolutions.get_dot_access(span);

        // Phase 1: Early-return cases that don't need the receiver emitted first
        match dot_access_kind {
            Some(SemanticDotKind::ValueEnumVariant) => {
                if let Some(s) = self.emit_value_enum_variant(expression, member) {
                    return s;
                }
            }
            Some(SemanticDotKind::EnumVariant) => {
                if let Some(s) = self.emit_enum_variant_dot(expression, member, result_ty) {
                    return s;
                }
            }
            Some(SemanticDotKind::StaticMethod { .. }) => {
                if let Some(s) = self.emit_static_method_dot(expression, member, result_ty) {
                    return s;
                }
            }
            Some(SemanticDotKind::InstanceMethodValue {
                is_exported,
                is_pointer_receiver,
            }) => {
                if let Some(s) = self.emit_instance_method_value_dot(
                    expression,
                    member,
                    result_ty,
                    is_exported,
                    is_pointer_receiver,
                ) {
                    return s;
                }
            }
            Some(SemanticDotKind::ModuleMember) | None => {
                // ModuleMember and unresolved accesses may still need static method
                // or enum variant emission (e.g., cross-module or alias patterns)
                if let Some(s) = self.emit_enum_variant_dot(expression, member, result_ty) {
                    return s;
                }
                if let Some(s) = self.emit_static_method_dot(expression, member, result_ty) {
                    return s;
                }
            }
            _ => {}
        }

        // Phase 2: Post-receiver emission (struct fields, tuple fields, instance methods)
        let expression_string = self.emit_coerced_expression(output, expression);
        let expression_ty = expression.get_type();

        // Tuple element: direct field access using TUPLE_FIELDS names
        if let Some(SemanticDotKind::TupleElement) = dot_access_kind
            && let Ok(index) = member.parse::<usize>()
        {
            let field = syntax::parse::TUPLE_FIELDS
                .get(index)
                .expect("oversize tuple arity");
            return format!("{}.{}", expression_string, field);
        }

        // Tuple struct field: newtype cast or positional field access
        if let Some(SemanticDotKind::TupleStructField { is_newtype }) = dot_access_kind
            && let Ok(index) = member.parse::<usize>()
        {
            if is_newtype {
                let deref_ty = expression_ty.resolve().strip_refs();
                if let Type::Constructor { ref id, .. } = deref_ty
                    && let Some(Definition::Struct { fields, .. }) =
                        self.ctx.definitions.get(id.as_str())
                    && let Some(field) = fields.first()
                {
                    let expression = if expression_ty.resolve().is_ref() {
                        format!("*{}", expression_string)
                    } else {
                        expression_string
                    };
                    let go_type = self.go_type_as_string(&field.ty);
                    return if go_type.starts_with('*') {
                        format!("({})({})", go_type, expression)
                    } else {
                        format!("{}({})", go_type, expression)
                    };
                }
            }
            return format!("{}.F{}", expression_string, index);
        }

        // Determine whether to capitalize the Go name from pre-computed metadata.
        // Semantic `is_exported` covers cross-module + public visibility.
        // Emit-side checks are still needed for Go-specific concerns:
        // - `field_is_public`: also checks #[json] tag-exported fields
        // - `method_needs_export`: methods that must be capitalized for Go interfaces
        let is_exported = match dot_access_kind {
            Some(SemanticDotKind::StructField { is_exported }) => {
                is_exported || self.field_is_public(&expression_ty, member)
            }
            Some(SemanticDotKind::InstanceMethod { is_exported }) => {
                is_exported || self.method_needs_export(member)
            }
            _ => {
                // Fallback for ModuleMember/None/unresolved
                self.compute_is_exported_context(expression, &expression_ty)
                    || self.field_is_public(&expression_ty, member)
                    || (!self.has_field(&expression_ty, member) && self.method_needs_export(member))
            }
        };

        let is_prelude_type = expression_ty
            .resolve()
            .strip_refs()
            .get_qualified_id()
            .is_some_and(|id| id.starts_with(go_name::PRELUDE_PREFIX));

        let field = if is_exported {
            if is_prelude_type {
                go_name::snake_to_camel(member)
            } else {
                go_name::make_exported(member)
            }
        } else {
            go_name::escape_keyword(member).into_owned()
        };

        // Go nullable field wrapping
        if Self::is_go_imported_type(&expression_ty) && self.is_go_nullable(result_ty) {
            let raw_access = format!("{}.{}", expression_string, field);
            let raw_var = self.fresh_var(Some("raw"));
            self.declare(&raw_var);
            write_line!(output, "{} := {}", raw_var, raw_access);
            return self.maybe_wrap_go_nullable(output, &raw_var, result_ty);
        }

        // Regular field/method access with cross-module type args
        let result = format!("{}.{}", expression_string, field);
        if !self.emitting_call_callee {
            let resolved_expression_ty = expression_ty.resolve();
            if let Type::Constructor { ref id, .. } = resolved_expression_ty
                && let Some(module) = id.strip_prefix(go_name::IMPORT_PREFIX)
            {
                let qualified = format!("{}.{}", module, member);
                if let Some(type_args) = self.format_cross_module_type_args(&qualified, result_ty) {
                    return format!("{}{}", result, type_args);
                }
            }
        }
        result
    }

    /// Compute whether a dot access context requires exported (capitalized) Go names.
    /// Used as fallback when semantic DotAccessKind doesn't carry `is_exported`.
    fn compute_is_exported_context(&self, expression: &Expression, expression_ty: &Type) -> bool {
        matches!(
            expression,
            Expression::Identifier { ty: Type::Constructor { id, .. }, .. } if id.starts_with(go_name::IMPORT_PREFIX)
        ) || self.is_from_prelude(expression_ty)
            || if let Type::Constructor { id, .. } = expression_ty.resolve().strip_refs() {
                id.split_once('.')
                    .is_some_and(|(m, _)| m != self.current_module && m != go_name::PRELUDE_MODULE)
            } else {
                false
            }
    }

    /// Emit the base expression with receiver coercion applied.
    ///
    /// Handles explicit deref (`.*`), absorbed `Ref<T>` generics, and auto-address/auto-deref
    /// coercions. Returns the Go expression string ready for member access.
    fn emit_coerced_expression(&mut self, output: &mut String, expression: &Expression) -> String {
        let coercion = self.ctx.coercions.get_coercion(expression.get_span());

        let (expression_string, had_explicit_deref) = if let Expression::Unary {
            operator: UnaryOperator::Deref,
            expression: inner,
            ..
        } = expression
        {
            (self.emit_operand(output, inner), true)
        } else {
            (self.emit_operand(output, expression), false)
        };

        let is_absorbed_ref = self.is_absorbed_ref_generic(expression);

        match (coercion, had_explicit_deref) {
            _ if is_absorbed_ref => expression_string,
            (Some(ReceiverCoercion::AutoAddress), true) => expression_string,
            (Some(ReceiverCoercion::AutoAddress), false) => {
                if matches!(expression.unwrap_parens(), Expression::Call { .. }) {
                    let tmp = self.fresh_var(Some("ref"));
                    self.declare(&tmp);
                    write_line!(output, "{} := {}", tmp, expression_string);
                    tmp
                } else {
                    expression_string
                }
            }
            (Some(ReceiverCoercion::AutoDeref), _) => expression_string,
            (None, true) => expression_string,
            (None, false) => expression_string,
        }
    }

    /// Check if expression has an absorbed `Ref<T>` generic (T already emitted as `*Concrete`).
    /// When true, suppress auto-deref coercion — the pointer is already the right type.
    fn is_absorbed_ref_generic(&self, expression: &Expression) -> bool {
        let check_expression = if let Expression::Unary {
            operator: UnaryOperator::Deref,
            expression: inner,
            ..
        } = expression
        {
            inner.as_ref()
        } else {
            expression
        };
        let expression_ty = check_expression.get_type().resolve();
        expression_ty.is_ref()
            && expression_ty.inner().is_some_and(|inner| {
                matches!(inner.resolve(), Type::Parameter(name)
                    if self.module.absorbed_ref_generics.contains(name.as_ref()))
            })
    }

    pub(crate) fn try_emit_tuple_struct_field_access(
        &mut self,
        expression_string: &str,
        expression_ty: &Type,
        index: usize,
    ) -> Option<String> {
        let deref_ty = expression_ty.resolve().strip_refs();
        let Type::Constructor { ref id, .. } = deref_ty else {
            return None;
        };

        let Some(Definition::Struct {
            kind,
            fields,
            generics,
            ..
        }) = self.ctx.definitions.get(id.as_str())
        else {
            return None;
        };

        if *kind != StructKind::Tuple {
            return None;
        }

        if fields.len() == 1 && generics.is_empty() {
            let underlying_ty = self.go_type_as_string(&fields[0].ty);
            let expression = if expression_ty.resolve().is_ref() {
                format!("*{}", expression_string)
            } else {
                expression_string.to_string()
            };
            return Some(format!("{}({})", underlying_ty, expression));
        }

        Some(format!("{}.F{}", expression_string, index))
    }

    fn is_from_prelude(&self, ty: &Type) -> bool {
        let Type::Constructor { id, .. } = ty.resolve().strip_refs() else {
            return false;
        };
        // Only return true if the type actually comes from the prelude module.
        // User-defined types with the same name should NOT be treated as prelude types.
        id.starts_with(go_name::PRELUDE_PREFIX)
    }

    pub(crate) fn emit_index_access(
        &mut self,
        output: &mut String,
        expression: &Expression,
        index: &Expression,
    ) -> String {
        if let Expression::Range {
            start,
            end,
            inclusive,
            ..
        } = index
        {
            // Only slices need three-index sub-slicing for safety — strings
            // are immutable so backing array aliasing cannot cause mutation.
            let needs_cap = expression.get_type().resolve().has_name("Slice");

            // Stage base, start, end together for eval-order sequencing
            let base_staged = if let Expression::Unary {
                operator: UnaryOperator::Deref,
                expression: inner,
                ..
            } = expression
            {
                let s = self.stage_operand(inner);
                Staged {
                    value: format!("(*{})", s.value),
                    setup: s.setup,
                    has_side_effects: s.has_side_effects,
                }
            } else {
                self.stage_operand(expression)
            };

            let mut all_stages = vec![base_staged];
            if let Some(s) = start {
                all_stages.push(self.stage_operand(s));
            }
            if let Some(e) = end {
                all_stages.push(self.stage_operand(e));
            }
            let values = self.sequence(output, all_stages, "_base");
            let base_str = &values[0];

            let (start_str, end_expression) = if start.is_some() {
                (values[1].as_str(), values.get(2).map(|s| s.as_str()))
            } else {
                ("", values.get(1).map(|s| s.as_str()))
            };

            let end_str = match (end_expression, *inclusive) {
                (None, _) => String::new(),
                (Some(e), false) => e.to_string(),
                (Some(e), true) => format!("{}+1", e),
            };

            if !needs_cap {
                return format!("{}[{}:{}]", base_str, start_str, end_str);
            }

            if end_str.is_empty() {
                let len_var = self.fresh_var(Some("len"));
                self.declare(&len_var);
                write_line!(output, "{} := len({})", len_var, base_str);
                return format!("{}[{}:{}:{}]", base_str, start_str, len_var, len_var);
            }

            if end_str.contains('(') {
                let end_var = self.fresh_var(Some("end"));
                self.declare(&end_var);
                write_line!(output, "{} := {}", end_var, end_str);
                return format!("{}[{}:{}:{}]", base_str, start_str, end_var, end_var);
            }

            return format!("{}[{}:{}:{}]", base_str, start_str, end_str, end_str);
        }

        // Stage base + index for eval-order sequencing
        let base_staged = if let Expression::Unary {
            operator: UnaryOperator::Deref,
            expression: inner,
            ..
        } = expression
        {
            let s = self.stage_operand(inner);
            Staged {
                value: format!("(*{})", s.value),
                setup: s.setup,
                has_side_effects: s.has_side_effects,
            }
        } else {
            self.stage_operand(expression)
        };

        // Handle range-typed variables used as slice indices (e.g. `items[r]` where `r: Range<int>`)
        let index_ty = index.get_type().resolve();
        if let Some(range_kind) = index_ty.get_name()
            && matches!(
                range_kind,
                "Range" | "RangeInclusive" | "RangeFrom" | "RangeTo" | "RangeToInclusive"
            )
        {
            let needs_cap = expression.get_type().resolve().has_name("Slice");
            // emit_or_capture already handles complex index expressions
            output.push_str(&base_staged.setup);
            let index_string = self.emit_or_capture(output, index, "range");
            return self.emit_range_var_slice(
                &base_staged.value,
                &index_string,
                range_kind,
                needs_cap,
            );
        }

        let index_staged = self.stage_composite(index);
        let values = self.sequence(output, vec![base_staged, index_staged], "_base");
        format!("{}[{}]", values[0], values[1])
    }

    /// Emit a Go slice expression from a range-typed variable index.
    ///
    /// When `needs_cap` is true, appends a third index to cap capacity at
    /// length, preventing append-through-alias corruption on shared backing
    /// arrays. Range field accesses (e.g. `.End`) are pure, so repeating
    /// them in the cap position is safe.
    fn emit_range_var_slice(
        &self,
        base: &str,
        range: &str,
        range_kind: &str,
        needs_cap: bool,
    ) -> String {
        let (start, end) = match range_kind {
            "Range" => (format!("{}.Start", range), format!("{}.End", range)),
            "RangeInclusive" => (format!("{}.Start", range), format!("{}.End+1", range)),
            "RangeFrom" => (format!("{}.Start", range), String::new()),
            "RangeTo" => (String::new(), format!("{}.End", range)),
            "RangeToInclusive" => (String::new(), format!("{}.End+1", range)),
            _ => unreachable!("unexpected range kind: {}", range_kind),
        };

        if !needs_cap {
            return format!("{}[{}:{}]", base, start, end);
        }

        // For open-ended ranges, cap at len(base).
        let cap = if end.is_empty() {
            format!("len({})", base)
        } else {
            end.clone()
        };

        format!("{}[{}:{}:{}]", base, start, end, cap)
    }

    pub(crate) fn emit_struct_call(
        &mut self,
        output: &mut String,
        name: &str,
        field_assignments: &[StructFieldAssignment],
        spread: &Option<Expression>,
        ty: &Type,
    ) -> String {
        let ctx = self.analyze_struct_call(name, ty);

        let tag_field = ctx.enum_ctx.as_ref().map(|e| {
            (
                enum_layout::ENUM_TAG_FIELD.to_string(),
                e.tag_constant.clone(),
            )
        });

        let is_go_struct = Self::is_go_imported_type(ty);
        let stages: Vec<Staged> = field_assignments
            .iter()
            .map(|f| self.stage_composite(&f.value))
            .collect();
        let emitted_values = self.sequence(output, stages, "_field");
        let mut field_names: Vec<String> = Vec::new();
        let mut field_values: Vec<String> = Vec::new();
        for (fi, f) in field_assignments.iter().enumerate() {
            let field_name = self.resolve_struct_call_field_name(&f.name, ty, &ctx);
            let value = emitted_values[fi].clone();
            // For recursive enum fields (pointer types), wrap with &
            let value = if ctx
                .enum_ctx
                .as_ref()
                .is_some_and(|e| e.pointer_fields.contains(f.name.as_str()))
            {
                if matches!(*f.value, Expression::Reference { .. })
                    || f.value.get_type().resolve().is_ref()
                {
                    // Already a reference (&x) or a Ref<T> value — emit directly, no re-wrapping
                    value
                } else {
                    let temp = self.fresh_var(Some("ptr"));
                    self.declare(&temp);
                    write_line!(output, "{} := {}", temp, value);
                    format!("&{}", temp)
                }
            } else {
                value
            };
            // Unwrap Option<Ref<T>> / Slice<Option<Ref<T>>> to bare Go types
            let value = if is_go_struct {
                self.maybe_unwrap_go_nullable(output, &value, &f.value.get_type().resolve())
            } else {
                value
            };
            let value = if let Some(field_ty) = self.lookup_struct_field_ty(ty, &f.name) {
                self.maybe_wrap_as_go_interface(value, &f.value.get_type(), &field_ty)
            } else {
                value
            };
            field_names.push(field_name);
            field_values.push(value);
        }

        let mut field_pairs: Vec<(String, String)> =
            field_names.into_iter().zip(field_values).collect();

        if let Some(tag) = tag_field {
            field_pairs.insert(0, tag);
        }

        if let Some(base) = spread {
            // Never-typed spread base diverges — emit as statement and
            // return a zero-value struct literal (dead code follows).
            if base.get_type().is_never() {
                self.emit_statement(output, base);
                return format!("{}{{}}", ctx.go_type);
            }
            let mut field_side_effects: Vec<bool> = Vec::new();
            if ctx.enum_ctx.is_some() {
                field_side_effects.push(false); // tag field is a constant
            }
            field_side_effects.extend(
                field_assignments
                    .iter()
                    .map(|f| is_order_sensitive(&f.value)),
            );
            self.emit_struct_update(output, base, &field_pairs, &field_side_effects)
        } else {
            self.emit_struct_literal(&ctx.go_type, &field_pairs)
        }
    }

    /// Analyze a struct call to determine Go type and enum context.
    fn analyze_struct_call(&mut self, name: &str, ty: &Type) -> StructCallContext {
        let is_prelude = self.is_from_prelude(ty);
        let enum_id = self.as_enum(ty);

        let go_type = self.compute_struct_call_go_type(name, ty, is_prelude, enum_id.is_some());

        if let Some(ref id) = enum_id {
            self.add_enum_imports_if_needed(name, id);
        }

        let enum_ctx = enum_id.map(|id| self.compute_enum_call_context(name, &id));

        StructCallContext {
            go_type,
            enum_ctx,
            is_prelude,
        }
    }

    /// Compute the Go type string for a struct call.
    fn compute_struct_call_go_type(
        &mut self,
        name: &str,
        ty: &Type,
        is_prelude: bool,
        is_enum: bool,
    ) -> String {
        // For cross-module struct calls (including type aliases), use the original name
        // to preserve the alias. E.g., "api.PublicSecret" should emit as "api.PublicSecret"
        // not as the underlying "internal.Secret".
        if name.contains('.') && !is_prelude {
            let parts: Vec<&str> = name.split('.').collect();
            let type_args = if let Type::Constructor { params, .. } = ty {
                self.format_type_args(params)
            } else {
                String::new()
            };

            let pkg = self.go_pkg_qualifier(parts[0]);

            if is_enum && parts.len() == 3 {
                // Enum variant via type alias: "module.TypeAlias.Variant"
                // Emit as "module.TypeAlias" (the variant fields are handled separately)
                return format!(
                    "{}.{}{}",
                    pkg,
                    go_name::capitalize_first(parts[1]),
                    type_args
                );
            } else if !is_enum && parts.len() == 2 {
                // Cross-module struct reference
                return format!(
                    "{}.{}{}",
                    pkg,
                    go_name::capitalize_first(parts[1]),
                    type_args
                );
            }
        }

        self.go_type_as_string(ty)
    }

    /// Compute the enum-specific context for a struct call.
    fn compute_enum_call_context(&mut self, name: &str, enum_id: &str) -> EnumCallContext {
        let variant_name = name.split('.').next_back().unwrap_or(name).to_string();

        // Use resolve_variant for correct tag constant — handles cross-module
        let tag_constant = self.resolve_variant(name, enum_id);

        let pointer_fields = if let Some(layout) = self.module.enum_layouts.get(enum_id) {
            if let Some(variant) = layout.get_variant(&variant_name) {
                variant
                    .fields
                    .iter()
                    .filter(|f| f.go_type.starts_with('*'))
                    .map(|f| f.source_name.clone())
                    .collect()
            } else {
                HashSet::default()
            }
        } else {
            HashSet::default()
        };

        EnumCallContext {
            enum_id: enum_id.to_string(),
            variant_name,
            tag_constant,
            pointer_fields,
        }
    }

    fn add_enum_imports_if_needed(&mut self, name: &str, enum_id: &str) {
        let enum_module = enum_id.split('.').next().unwrap_or("");

        if enum_module != self.current_module {
            self.require_module_import(enum_module);
        }

        let parts: Vec<&str> = name.split('.').collect();
        if parts.len() == 3 {
            let module = self.resolve_alias_to_module(parts[0]).to_string();
            self.require_module_import(&module);
        }
    }

    /// Resolve the Go field name for a struct call field.
    fn resolve_struct_call_field_name(
        &mut self,
        field_name: &str,
        ty: &Type,
        ctx: &StructCallContext,
    ) -> String {
        if let Some(ref enum_ctx) = ctx.enum_ctx {
            // Use the enum layout to get the correct field name
            self.enum_struct_field_name(&enum_ctx.enum_id, &enum_ctx.variant_name, field_name)
                .unwrap_or_else(|| go_name::make_exported(field_name))
        } else if ctx.is_prelude || self.field_is_public(ty, field_name) {
            go_name::make_exported(field_name)
        } else {
            go_name::escape_keyword(field_name).into_owned()
        }
    }

    pub(crate) fn emit_struct_literal(&self, ty: &str, fields: &[(String, String)]) -> String {
        let raw = if fields.is_empty() {
            format!("{}{{}}", ty)
        } else if fields.len() == 1 {
            let (name, value) = &fields[0];
            format!("{}{{ {}: {} }}", ty, name, value)
        } else {
            let field_strs: Vec<String> = fields
                .iter()
                .map(|(name, value)| format!("{}: {},", name, value))
                .collect();
            format!("{}{{\n{}\n}}", ty, field_strs.join("\n"))
        };

        // Generic composite literals (`Type[Args]{...}`) need inner parens in
        // condition contexts because gofmt strips outer condition parens for
        // generics, producing invalid Go in `if`/`for`/`switch`.
        if self.in_condition && ty.contains('[') {
            format!("({})", raw)
        } else {
            raw
        }
    }

    fn emit_struct_update(
        &mut self,
        output: &mut String,
        base: &Expression,
        fields: &[(String, String)],
        field_side_effects: &[bool],
    ) -> String {
        if fields.is_empty() {
            return self.emit_operand(output, base);
        }

        let fields: Vec<(String, String)> = fields
            .iter()
            .enumerate()
            .map(|(i, (name, value))| {
                if field_side_effects.get(i).copied().unwrap_or(false) {
                    let temp = self.fresh_var(Some("field"));
                    self.declare(&temp);
                    write_line!(output, "{} := {}", temp, value);
                    (name.clone(), temp)
                } else {
                    (name.clone(), value.clone())
                }
            })
            .collect();

        let base_string = self.emit_operand(output, base);
        let tmp = self.fresh_var(Some("copy"));
        self.declare(&tmp);

        write_line!(output, "{} := {}", tmp, base_string);

        for (name, value) in &fields {
            write_line!(output, "{}.{} = {}", tmp, name, value);
        }

        tmp
    }
}
