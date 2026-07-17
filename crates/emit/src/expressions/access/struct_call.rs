use crate::expressions::access::dot_access::is_from_prelude;
use rustc_hash::FxHashSet as HashSet;

use syntax::ast::{Expression, StructFieldAssignment, StructSpread};
use syntax::program::{Definition, DefinitionBody};
use syntax::types::{CompoundKind, SimpleKind, Type, unqualified_name};

use crate::Planner;
use crate::abi::coercion::CoercionPlan;
use crate::abi::layout::{SlotOrigin, ValueLayout};
use crate::context::expression::ExpressionContext;
use crate::definitions::enum_layout;
use crate::go_name;
use crate::plan::bodies::LoweredStatement;
use crate::plan::values::{CaptureBoundary, EvaluationEffect, GoExpression, ValuePlan};
use crate::utils::is_order_sensitive;

struct StructCallContext {
    go_type: String,
    enum_ctx: Option<EnumCallContext>,
}

struct EnumCallContext {
    enum_id: String,
    variant_name: String,
    tag_constant: String,
    /// Fields that need pointer wrapping (recursive types).
    pointer_fields: HashSet<String>,
}

impl Planner<'_> {
    /// Plan a struct/enum-variant construction. Value is the Go struct
    /// literal or update.
    pub(crate) fn plan_struct_call(
        &mut self,
        name: &str,
        field_assignments: &[StructFieldAssignment],
        spread: &StructSpread,
        ty: &Type,
        expression_ctx: ExpressionContext<'_>,
    ) -> ValuePlan {
        let ctx = self.analyze_struct_call(name, ty);

        let tag_field = ctx.enum_ctx.as_ref().map(|e| {
            (
                enum_layout::ENUM_TAG_FIELD.to_string(),
                e.tag_constant.clone(),
            )
        });

        let is_go_struct = self.is_go_abi_type(ty);

        let stages: Vec<ValuePlan> = field_assignments
            .iter()
            .map(|f| self.stage_composite(&f.value, ExpressionContext::value()))
            .collect();
        let mut field_side_effects: Vec<bool> = stages
            .iter()
            .map(|value| value.evaluation.stability.is_observable())
            .collect();
        if ctx.enum_ctx.is_some() {
            field_side_effects.insert(0, false);
        }
        let sequenced = self.sequence_values(stages, CaptureBoundary::SiblingSequence, "_field");
        let mut effect = sequenced.effect;
        let fields_contain_deferred_evaluation = sequenced.contains_deferred_evaluation();
        let (mut setup, emitted_values) = sequenced.into_rendered();

        let mut field_names: Vec<String> = Vec::new();
        let mut field_values: Vec<String> = Vec::new();
        for (slot, f) in field_assignments.iter().enumerate() {
            let field_name = self.resolve_struct_call_field_name(&f.name, ty, &ctx);
            let mut value = emitted_values[slot].clone();
            value = self.wrap_recursive_enum_field(&mut setup, value, f, &ctx);
            let value_ty = f.value.get_type();
            let field_ty = self.lookup_struct_field_ty(ty, &f.name);
            let field_layout =
                self.field_slot_layout(ty, &f.name, field_ty.as_ref().unwrap_or(&value_ty));
            value = self.coerce_struct_field(
                &mut setup,
                value,
                &value_ty,
                field_ty.as_ref(),
                field_layout.as_ref(),
            );
            field_names.push(field_name);
            field_values.push(value);
        }

        let mut field_pairs: Vec<(String, String)> =
            field_names.into_iter().zip(field_values).collect();

        if let Some(tag) = tag_field {
            field_pairs.insert(0, tag);
        }

        let mut base_contains_deferred_evaluation = false;
        let value = match spread {
            StructSpread::From(base) => {
                // Never-typed spread base diverges — emit as statement and
                // return a zero-value struct literal (dead code follows).
                if base.get_type().is_never() {
                    if matches!(base.unwrap_parens(), Expression::Call { .. }) {
                        effect = effect.combine(EvaluationEffect::EffectfulCall);
                    }
                    setup.push(self.lower_statement(base));
                    format!("{}{{}}", ctx.go_type)
                } else {
                    let base_staged = self.stage_operand(base, ExpressionContext::value());
                    effect = effect.combine(base_staged.evaluation.effect);
                    if ctx.enum_ctx.is_some() {
                        let (value, contains_deferred_evaluation) = self.lower_enum_variant_spread(
                            &mut setup,
                            base,
                            base_staged,
                            name,
                            ty,
                            &ctx,
                            field_pairs,
                            &field_side_effects,
                            field_assignments,
                            expression_ctx,
                        );
                        base_contains_deferred_evaluation = contains_deferred_evaluation;
                        value
                    } else {
                        let (value, contains_deferred_evaluation) = self.lower_struct_update(
                            &mut setup,
                            base_staged,
                            &field_pairs,
                            &field_side_effects,
                        );
                        base_contains_deferred_evaluation = contains_deferred_evaluation;
                        value
                    }
                }
            }
            StructSpread::Autofill { .. } if !is_go_struct => {
                self.append_autofills(&mut field_pairs, field_assignments, ty, name, &ctx);
                emit_struct_literal(&ctx.go_type, &field_pairs, expression_ctx)
            }
            StructSpread::Autofill { .. } | StructSpread::None => {
                emit_struct_literal(&ctx.go_type, &field_pairs, expression_ctx)
            }
        };

        ValuePlan::computed(
            setup,
            GoExpression::composite_literal(
                value,
                fields_contain_deferred_evaluation || base_contains_deferred_evaluation,
            ),
            effect,
        )
    }

    /// Apply Go-boundary coercion followed by internal coercion. The Go-boundary
    /// step targets `field_ty` (falling back to `value_ty` when no declared
    /// field exists); the internal step targets `field_ty` only.
    fn coerce_struct_field(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        mut value: String,
        value_ty: &Type,
        field_ty: Option<&Type>,
        field_layout: Option<&ValueLayout>,
    ) -> String {
        if let Some(field_layout) = field_layout {
            let source_layout = self.value_layout(value_ty, SlotOrigin::Lisette);
            let coercion = CoercionPlan::bridge(self, &source_layout, field_layout);
            let (coercion_setup, coerced) = coercion.lower(self, value);
            statements.extend(coercion_setup);
            value = coerced;
        }
        if let Some(field_ty) = field_ty {
            let coercion = CoercionPlan::internal(self, value_ty, field_ty);
            let (coercion_setup, coerced) = coercion.lower(self, value);
            statements.extend(coercion_setup);
            value = coerced;
        }
        value
    }

    /// Append zero-value pairs for fields the user did not assign. Slice fields
    /// are skipped so Go's nil zero-value applies instead of `[]T{}`.
    fn append_autofills(
        &mut self,
        field_pairs: &mut Vec<(String, String)>,
        field_assignments: &[StructFieldAssignment],
        ty: &Type,
        name: &str,
        ctx: &StructCallContext,
    ) {
        let assigned: HashSet<&str> = field_assignments.iter().map(|f| f.name.as_str()).collect();
        let Some(unspecified) =
            self.lookup_unspecified_fields(ty, name, ctx.enum_ctx.as_ref(), &assigned)
        else {
            return;
        };
        for (field_name, field_ty) in unspecified {
            if field_ty.is_slice() {
                continue;
            }
            let go_field_name = self.resolve_struct_call_field_name(&field_name, ty, ctx);
            let zero = self.lisette_zero(&field_ty);
            field_pairs.push((go_field_name, zero));
        }
    }

    /// Look up unspecified fields of a Lisette-defined struct or enum struct variant,
    /// with type substitution applied so generic-typed fields resolve to concrete types.
    /// `name` is needed only for the variant case (to pick the variant within the enum).
    fn lookup_unspecified_fields(
        &self,
        ty: &Type,
        name: &str,
        enum_ctx: Option<&EnumCallContext>,
        assigned: &HashSet<&str>,
    ) -> Option<Vec<(ecow::EcoString, Type)>> {
        let params = match ty.strip_refs() {
            Type::Nominal { params, .. } => params,
            _ => Vec::new(),
        };

        if let Some(enum_ctx) = enum_ctx {
            let Some(Definition {
                body:
                    DefinitionBody::Enum {
                        variants, generics, ..
                    },
                ..
            }) = self.facts.definition(enum_ctx.enum_id.as_str())
            else {
                return None;
            };
            let variant_name = unqualified_name(name);
            let variant = variants.iter().find(|v| v.name == variant_name)?;
            let map = generics_substitution(generics.iter().map(|g| g.name.clone()), &params);
            return Some(unspecified_pairs(
                variant.fields.iter().map(|f| (&f.name, &f.ty)),
                assigned,
                &map,
            ));
        }

        let Type::Nominal { id, .. } = ty.strip_refs() else {
            return None;
        };
        let Some(Definition {
            ty: definition_ty,
            body: DefinitionBody::Struct { fields, .. },
            ..
        }) = self.facts.definition(id.as_str())
        else {
            return None;
        };
        let map = forall_substitution(definition_ty, &params);
        Some(unspecified_pairs(
            fields.iter().map(|f| (&f.name, &f.ty)),
            assigned,
            &map,
        ))
    }
    fn go_imported_zero(&mut self, ty: &Type, id: &str) -> String {
        if self.facts.is_interface(ty) || self.facts.resolve_to_function_type(ty).is_some() {
            return "nil".to_string();
        }
        let go_ty = self.go_type_string(ty);
        // A newtype is a Go named scalar (`type Duration int64`), so a composite
        // literal `Duration{}` is invalid; `*new(Duration)` yields its zero.
        let is_newtype = self.facts.definition(id).is_some_and(|d| d.is_newtype());
        let is_struct_like = !is_newtype
            && (matches!(
                self.facts.definition(id).map(|d| &d.body),
                Some(DefinitionBody::Struct { .. })
            ) || matches!(
                self.facts.definition(id).map(|d| &d.body),
                Some(DefinitionBody::TypeAlias { annotation, .. }) if annotation.is_opaque()
            ));
        if is_struct_like {
            format!("{}{{}}", go_ty)
        } else {
            format!("*new({})", go_ty)
        }
    }

    pub(crate) fn lisette_zero(&mut self, ty: &Type) -> String {
        let layout = self.value_layout(ty, SlotOrigin::Lisette);
        match (ty, &layout) {
            (Type::Simple(kind), _) => match kind {
                SimpleKind::Bool => "false".to_string(),
                SimpleKind::String => "\"\"".to_string(),
                SimpleKind::Unit => "struct{}{}".to_string(),
                _ => "0".to_string(),
            },
            (
                Type::Compound {
                    kind: CompoundKind::Slice,
                    ..
                },
                ValueLayout::Slice { element, .. },
            ) => format!("([]{})(nil)", element.go_type(self)),
            (
                Type::Compound {
                    kind: CompoundKind::Map,
                    ..
                },
                ValueLayout::Map { key, value, .. },
            ) => format!("map[{}]{}{{}}", key.go_type(self), value.go_type(self)),
            (Type::Compound { .. }, _) => format!("{}{{}}", self.go_type_string(ty)),
            (Type::Nominal { id, params, .. }, _) => {
                self.lisette_zero_nominal(ty, id.as_str(), params)
            }
            (Type::Tuple(_), ValueLayout::Tuple { elements, .. }) => {
                let parts: Vec<String> = elements
                    .iter()
                    .map(|element| self.lisette_zero(element.logical_type()))
                    .collect();
                self.require_stdlib();
                format!(
                    "{}.MakeTuple{}({})",
                    go_name::GO_STDLIB_PKG,
                    parts.len(),
                    parts.join(", ")
                )
            }
            (
                Type::Array { .. },
                ValueLayout::Array {
                    length, element, ..
                },
            ) => self.array_zero(*length, element.logical_type()),
            _ => format!("{}{{}}", self.go_type_string(ty)),
        }
    }

    fn lisette_zero_nominal(&mut self, ty: &Type, id: &str, params: &[Type]) -> String {
        if id == "prelude.Option" {
            let inner = params
                .first()
                .map(|a| self.go_type_string(a))
                .unwrap_or_else(|| "any".to_string());
            self.require_stdlib();
            return format!("{}.MakeOptionNone[{}]()", go_name::GO_STDLIB_PKG, inner);
        }
        if go_name::is_go_import(id) {
            return self.go_imported_zero(ty, id);
        }
        if let Some(underlying) = self.get_newtype_underlying(ty) {
            let go_ty = self.go_type_string(ty);
            let inner = self.lisette_zero(&underlying);
            return format!("{}({})", go_ty, inner);
        }
        if let Some(fields) = self.lookup_unspecified_fields(ty, "", None, &HashSet::default()) {
            let go_ty = self.go_type_string(ty);
            let is_tuple = self.is_tuple_struct_type(ty);
            let pairs: Vec<(String, String)> = fields
                .into_iter()
                .enumerate()
                .filter(|(_, (_, field_ty))| !field_ty.is_slice())
                .map(|(index, (name, field_ty))| {
                    let go_name = if is_tuple {
                        format!("F{}", index)
                    } else if self.struct_field_is_exported(ty, &name) {
                        go_name::make_exported(&name)
                    } else {
                        go_name::escape_keyword(&name).into_owned()
                    };
                    (go_name, self.lisette_zero(&field_ty))
                })
                .collect();
            return emit_struct_literal(&go_ty, &pairs, ExpressionContext::value());
        }
        if let Some(underlying) = ty.get_underlying() {
            return self.lisette_zero(underlying);
        }
        format!("{}{{}}", self.go_type_string(ty))
    }

    /// Array zero value, filling per index with `lisette_zero` when Go's own
    /// `[N]E{}` zero differs from Lisette's (e.g. `Option<T>`).
    pub(crate) fn array_zero(&mut self, len: u64, elem: &Type) -> String {
        let elem_go = self.go_type_string(elem);
        if len == 0 || self.element_go_zero_ok(elem) {
            return format!("[{}]{}{{}}", len, elem_go);
        }
        let zero = self.lisette_zero(elem);
        // Go has no syntax to repeat a value across all N slots, so fill each index.
        format!(
            "func() [{len}]{elem_go} {{ var arr [{len}]{elem_go}; for i := range arr {{ arr[i] = {zero} }}; return arr }}()"
        )
    }

    /// True when Go's zero for the element already matches Lisette's.
    pub(crate) fn element_go_zero_ok(&self, ty: &Type) -> bool {
        match ty {
            Type::Simple(_) => true,
            Type::Compound {
                kind: CompoundKind::Slice,
                ..
            } => true,
            Type::Array { element, .. } => self.element_go_zero_ok(element),
            Type::Tuple(elements) => elements.iter().all(|e| self.element_go_zero_ok(e)),
            _ => false,
        }
    }

    /// Address a struct-call field stored behind a compiler-inserted pointer
    /// (recursive-enum cycle breaker). Such a field is by value, so capture it
    /// into a temp Go can take the address of. User `Ref<T>` fields are not
    /// recursive and pass through this untouched.
    fn wrap_recursive_enum_field(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        value: String,
        field: &StructFieldAssignment,
        ctx: &StructCallContext,
    ) -> String {
        let needs_pointer = ctx
            .enum_ctx
            .as_ref()
            .is_some_and(|e| e.pointer_fields.contains(field.name.as_str()));
        if !needs_pointer {
            return value;
        }
        let temp = self.hoist_tmp_value_statement(statements, "ptr", &value);
        format!("&{}", temp)
    }

    /// Analyze a struct call to determine Go type and enum context.
    fn analyze_struct_call(&mut self, name: &str, ty: &Type) -> StructCallContext {
        let is_prelude = is_from_prelude(ty);
        let enum_id = self.as_enum(ty);

        let go_type = self.compute_struct_call_go_type(name, ty, is_prelude, enum_id.is_some());

        if let Some(ref id) = enum_id {
            self.add_enum_imports_if_needed(name, id);
        }

        let enum_ctx = enum_id.map(|id| self.compute_enum_call_context(name, &id));

        StructCallContext { go_type, enum_ctx }
    }

    /// Compute the Go type string for a struct call.
    fn compute_struct_call_go_type(
        &mut self,
        name: &str,
        ty: &Type,
        is_prelude: bool,
        is_enum: bool,
    ) -> String {
        if let Type::Nominal { id, .. } = ty
            && let Some(go) = self.anon_struct_go_type(id)
        {
            self.note_go_type(&go);
            return go.code;
        }

        // For cross-module struct calls (including type aliases), use the original name
        // to preserve the alias. E.g., "api.PublicSecret" should emit as "api.PublicSecret"
        // not as the underlying "internal.Secret".
        if name.contains('.') && !is_prelude {
            let parts: Vec<&str> = name.split('.').collect();
            let emits_qualified = (is_enum && parts.len() == 3) || (!is_enum && parts.len() == 2);
            if emits_qualified && !self.facts.is_current_module(parts[0]) {
                let type_args = if self.is_non_generic_alias_call(parts[1], ty) {
                    String::new()
                } else if let Type::Nominal { params, .. } = ty {
                    self.format_type_args(params)
                } else {
                    String::new()
                };
                let pkg = self.require_module_import(parts[0]);
                return format!("{}.{}{}", pkg, go_name::snake_to_camel(parts[1]), type_args);
            }
        }

        self.go_type_string(ty)
    }

    /// True when `type_name` (e.g., `StringFlag`) is a non-generic alias in the
    /// same module as the underlying struct `ty`.
    fn is_non_generic_alias_call(&self, type_name: &str, ty: &Type) -> bool {
        let Type::Nominal { id: struct_id, .. } = ty else {
            return false;
        };
        let Some(module) = self.facts.module_for_qualified_name(struct_id) else {
            return false;
        };
        let alias_id = format!("{}.{}", module, type_name);
        matches!(
            self.facts.definition(&alias_id).map(|d| &d.body),
            Some(DefinitionBody::TypeAlias { generics, .. }) if generics.is_empty()
        )
    }

    /// Compute the enum-specific context for a struct call.
    fn compute_enum_call_context(&mut self, name: &str, enum_id: &str) -> EnumCallContext {
        let variant_name = unqualified_name(name).to_string();

        let tag_constant = self.resolve_variant(name, enum_id);

        let pointer_fields = if let Some(layout) = self.enum_layout(enum_id) {
            if let Some(variant) = layout.get_variant(&variant_name) {
                variant
                    .fields
                    .iter()
                    .filter(|f| f.is_recursive)
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
        if let Some(enum_module) = self.facts.module_for_qualified_name(enum_id)
            && !self.facts.is_current_module(enum_module)
        {
            let enum_module = enum_module.to_string();
            self.require_module_import(&enum_module);
        }

        let parts: Vec<&str> = name.split('.').collect();
        if parts.len() == 3 {
            let module = self.canonical_module(parts[0]);
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
            self.enum_struct_field_name(&enum_ctx.enum_id, &enum_ctx.variant_name, field_name)
                .unwrap_or_else(|| go_name::make_exported(field_name))
        } else if self.field_is_embedded(ty, field_name) {
            go_name::escape_keyword(field_name).into_owned()
        } else if self.struct_field_is_exported(ty, field_name) {
            go_name::make_exported(field_name)
        } else {
            go_name::escape_keyword(field_name).into_owned()
        }
    }

    fn lower_struct_update(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        base_staged: ValuePlan,
        fields: &[(String, String)],
        field_side_effects: &[bool],
    ) -> (String, bool) {
        if fields.is_empty() {
            let contains_deferred_evaluation =
                base_staged.expression.contains_deferred_evaluation();
            let (setup, value) = base_staged.into_parts();
            statements.extend(setup);
            return (value, contains_deferred_evaluation);
        }

        let fields: Vec<(String, String)> = fields
            .iter()
            .enumerate()
            .map(|(i, (name, value))| {
                if field_side_effects.get(i).copied().unwrap_or(false) {
                    let temp = self.hoist_tmp_value_statement(statements, "field", value);
                    (name.clone(), temp)
                } else {
                    (name.clone(), value.clone())
                }
            })
            .collect();

        let (base_setup, base_value) = base_staged.into_parts();
        statements.extend(base_setup);
        let tmp = self.hoist_tmp_value_statement(statements, "copy", &base_value);

        for (name, value) in &fields {
            statements.push(LoweredStatement::RawGo(format!(
                "{}.{} = {}\n",
                tmp, name, value
            )));
        }

        (tmp, false)
    }

    #[allow(clippy::too_many_arguments)]
    fn lower_enum_variant_spread(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        base: &Expression,
        base_staged: ValuePlan,
        name: &str,
        ty: &Type,
        ctx: &StructCallContext,
        field_pairs: Vec<(String, String)>,
        field_side_effects: &[bool],
        field_assignments: &[StructFieldAssignment],
        expression_ctx: ExpressionContext<'_>,
    ) -> (String, bool) {
        let mut pairs: Vec<(String, String)> = field_pairs
            .into_iter()
            .enumerate()
            .map(|(i, (field_name, value))| {
                if field_side_effects.get(i).copied().unwrap_or(false) {
                    let temp = self.hoist_tmp_value_statement(statements, "field", &value);
                    (field_name, temp)
                } else {
                    (field_name, value)
                }
            })
            .collect();

        let assigned: HashSet<&str> = field_assignments.iter().map(|f| f.name.as_str()).collect();
        let carried = self
            .lookup_unspecified_fields(ty, name, ctx.enum_ctx.as_ref(), &assigned)
            .unwrap_or_default();

        let (base_setup, base_value) = base_staged.into_parts();
        statements.extend(base_setup);

        if carried.is_empty() {
            statements.push(LoweredStatement::RawGo(format!("_ = {}\n", base_value)));
            return (
                emit_struct_literal(&ctx.go_type, &pairs, expression_ctx),
                false,
            );
        }

        let source = if is_order_sensitive(base) {
            self.hoist_tmp_value_statement(statements, "spread", &base_value)
        } else {
            base_value
        };
        for (field_name, _) in carried {
            let slot = self.resolve_struct_call_field_name(&field_name, ty, ctx);
            pairs.push((slot.clone(), format!("{}.{}", source, slot)));
        }
        (
            emit_struct_literal(&ctx.go_type, &pairs, expression_ctx),
            false,
        )
    }
}

fn forall_substitution(definition_ty: &Type, params: &[Type]) -> syntax::types::SubstitutionMap {
    if let Type::Forall { vars, .. } = definition_ty
        && !vars.is_empty()
        && vars.len() == params.len()
    {
        generics_substitution(vars.iter().cloned(), params)
    } else {
        syntax::types::SubstitutionMap::default()
    }
}

fn generics_substitution(
    vars: impl Iterator<Item = ecow::EcoString>,
    params: &[Type],
) -> syntax::types::SubstitutionMap {
    let mut map = syntax::types::SubstitutionMap::default();
    for (var, param) in vars.zip(params.iter()) {
        map.insert(var, param.clone());
    }
    map
}

fn apply_substitution(ty: &Type, map: &syntax::types::SubstitutionMap) -> Type {
    if map.is_empty() {
        ty.clone()
    } else {
        syntax::types::substitute(ty, map)
    }
}

fn unspecified_pairs<'a>(
    fields: impl Iterator<Item = (&'a ecow::EcoString, &'a Type)>,
    assigned: &HashSet<&str>,
    map: &syntax::types::SubstitutionMap,
) -> Vec<(ecow::EcoString, Type)> {
    fields
        .filter(|(name, _)| !assigned.contains(name.as_str()))
        .map(|(name, ty)| (name.clone(), apply_substitution(ty, map)))
        .collect()
}

pub(crate) fn emit_struct_literal(
    ty: &str,
    fields: &[(String, String)],
    ctx: ExpressionContext<'_>,
) -> String {
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
    if ctx.is_condition() && ty.contains('[') {
        format!("({})", raw)
    } else {
        raw
    }
}
