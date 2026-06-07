use crate::expressions::access::dot_access::is_from_prelude;
use rustc_hash::FxHashSet as HashSet;

use syntax::ast::{Expression, StructFieldAssignment, StructSpread};
use syntax::program::{Definition, DefinitionBody};
use syntax::types::{CompoundKind, SimpleKind, Type, unqualified_name};

use crate::EmitEffects;
use crate::Planner;
use crate::abi::coercion::{Coercion, CoercionDirection};
use crate::context::expression::ExpressionContext;
use crate::definitions::enum_layout;
use crate::expressions::emission::StagedExpression;
use crate::go_name;
use crate::plan::bodies::LoweredStatement;
use crate::plan::values::{ValuePlan, value_plan_from_statements};
use crate::utils::observable_after_mutation;

struct StructCallContext {
    go_type: String,
    enum_ctx: Option<EnumCallContext>,
    is_prelude: bool,
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
        fx: &mut EmitEffects,
    ) -> ValuePlan {
        let ctx = self.analyze_struct_call(name, ty, fx);

        let tag_field = ctx.enum_ctx.as_ref().map(|e| {
            (
                enum_layout::ENUM_TAG_FIELD.to_string(),
                e.tag_constant.clone(),
            )
        });

        let is_go_struct = self.go_imported_shape(ty).is_some();
        let kept = self.kept_struct_call_fields(field_assignments, spread, ty, is_go_struct);

        let stages: Vec<StagedExpression> = kept
            .iter()
            .map(|f| self.stage_composite(&f.value, ExpressionContext::value(), fx))
            .collect();
        let (mut setup, emitted_values) = self.sequence_structured(stages, "_field");

        let mut field_names: Vec<String> = Vec::new();
        let mut field_values: Vec<String> = Vec::new();
        for (slot, f) in kept.iter().enumerate() {
            let field_name = self.resolve_struct_call_field_name(&f.name, ty, &ctx);
            let mut value = emitted_values[slot].clone();
            value = self.wrap_recursive_enum_field(&mut setup, value, f, &ctx);
            let value_ty = f.value.get_type();
            let field_ty = self.lookup_struct_field_ty(ty, &f.name);
            value = self.coerce_struct_field(
                &mut setup,
                value,
                &f.value,
                &value_ty,
                field_ty.as_ref(),
                is_go_struct,
                fx,
            );
            field_names.push(field_name);
            field_values.push(value);
        }

        let mut field_pairs: Vec<(String, String)> =
            field_names.into_iter().zip(field_values).collect();

        if let Some(tag) = tag_field {
            field_pairs.insert(0, tag);
        }

        let value = match spread {
            StructSpread::From(base) => {
                // Never-typed spread base diverges — emit as statement and
                // return a zero-value struct literal (dead code follows).
                if base.get_type().is_never() {
                    setup.push(self.lower_statement(base, fx));
                    format!("{}{{}}", ctx.go_type)
                } else {
                    let mut field_side_effects: Vec<bool> = Vec::new();
                    if ctx.enum_ctx.is_some() {
                        field_side_effects.push(false); // tag field is a constant
                    }
                    field_side_effects.extend(
                        field_assignments
                            .iter()
                            .map(|f| observable_after_mutation(&f.value)),
                    );
                    self.lower_struct_update(
                        &mut setup,
                        base,
                        &field_pairs,
                        &field_side_effects,
                        fx,
                    )
                }
            }
            StructSpread::ZeroFill { .. } if !is_go_struct => {
                self.append_zero_fills(&mut field_pairs, field_assignments, ty, name, &ctx, fx);
                emit_struct_literal(&ctx.go_type, &field_pairs, expression_ctx)
            }
            StructSpread::ZeroFill { .. } | StructSpread::None => {
                emit_struct_literal(&ctx.go_type, &field_pairs, expression_ctx)
            }
        };

        value_plan_from_statements(setup, value)
    }

    /// Apply Go-boundary coercion followed by internal coercion. The Go-boundary
    /// step targets `field_ty` (falling back to `value_ty` when no declared
    /// field exists); the internal step targets `field_ty` only.
    #[allow(clippy::too_many_arguments)]
    fn coerce_struct_field(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        mut value: String,
        value_expr: &Expression,
        value_ty: &Type,
        field_ty: Option<&Type>,
        is_go_struct: bool,
        fx: &mut EmitEffects,
    ) -> String {
        if is_go_struct {
            let target_ty = field_ty.unwrap_or(value_ty);
            if value_expr.is_none_literal() && target_ty.resolves_to_unknown() {
                return "nil".to_string();
            }
            let coercion =
                Coercion::resolve(self, value_ty, target_ty, CoercionDirection::ToGoBoundary);
            let (coercion_setup, coerced) = coercion.lower(self, value, fx);
            statements.extend(coercion_setup);
            value = coerced;
        }
        if let Some(field_ty) = field_ty {
            let coercion = Coercion::resolve(self, value_ty, field_ty, CoercionDirection::Internal);
            let (coercion_setup, coerced) = coercion.lower(self, value, fx);
            statements.extend(coercion_setup);
            value = coerced;
        }
        value
    }

    /// Append zero-value pairs for fields the user did not assign. Slice fields
    /// are skipped so Go's nil zero-value applies instead of `[]T{}`.
    fn append_zero_fills(
        &mut self,
        field_pairs: &mut Vec<(String, String)>,
        field_assignments: &[StructFieldAssignment],
        ty: &Type,
        name: &str,
        ctx: &StructCallContext,
        fx: &mut EmitEffects,
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
            let zero = self.lisette_zero(&field_ty, fx);
            field_pairs.push((go_field_name, zero));
        }
    }

    /// Drop empty `Slice<T>` field assignments so Go's nil zero-value applies
    /// instead of an `[]T{}` allocation. Skipped for Go-imported structs (the
    /// Go API may distinguish nil from empty) and `From` spreads (the override
    /// must still fire to clear the inherited value).
    fn kept_struct_call_fields<'a>(
        &self,
        field_assignments: &'a [StructFieldAssignment],
        spread: &StructSpread,
        ty: &Type,
        is_go_struct: bool,
    ) -> Vec<&'a StructFieldAssignment> {
        let can_omit_slices = !is_go_struct
            && !matches!(spread, StructSpread::From(_))
            && field_assignments
                .iter()
                .any(|f| f.value.is_empty_collection());
        if !can_omit_slices {
            return field_assignments.iter().collect();
        }
        field_assignments
            .iter()
            .filter(|f| {
                !(f.value.is_empty_collection()
                    && self
                        .lookup_struct_field_ty(ty, &f.name)
                        .as_ref()
                        .is_some_and(Type::is_slice))
            })
            .collect()
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
    fn go_imported_zero(&mut self, ty: &Type, id: &str, fx: &mut EmitEffects) -> String {
        if self.facts.is_interface(ty) || self.facts.resolve_to_function_type(ty).is_some() {
            return "nil".to_string();
        }
        let go_ty = self.go_type_string(ty, fx);
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

    pub(crate) fn lisette_zero(&mut self, ty: &Type, fx: &mut EmitEffects) -> String {
        match ty {
            Type::Simple(kind) => match kind {
                SimpleKind::Bool => "false".to_string(),
                SimpleKind::String => "\"\"".to_string(),
                SimpleKind::Unit => "struct{}{}".to_string(),
                _ => "0".to_string(),
            },
            Type::Compound { kind, args } => match kind {
                CompoundKind::Slice => {
                    let inner = args
                        .first()
                        .map(|a| self.go_type_string(a, fx))
                        .unwrap_or_else(|| "any".to_string());
                    format!("([]{})(nil)", inner)
                }
                CompoundKind::Map => {
                    let key = args
                        .first()
                        .map(|a| self.go_type_string(a, fx))
                        .unwrap_or_else(|| "any".to_string());
                    let val = args
                        .get(1)
                        .map(|a| self.go_type_string(a, fx))
                        .unwrap_or_else(|| "any".to_string());
                    format!("map[{}]{}{{}}", key, val)
                }
                _ => format!("{}{{}}", self.go_type_string(ty, fx)),
            },
            Type::Nominal { id, params, .. } => {
                if id.as_str() == "prelude.Option" {
                    let inner = params
                        .first()
                        .map(|a| self.go_type_string(a, fx))
                        .unwrap_or_else(|| "any".to_string());
                    fx.require_stdlib();
                    return format!("{}.MakeOptionNone[{}]()", go_name::GO_STDLIB_PKG, inner);
                }
                if go_name::is_go_import(id.as_str()) {
                    return self.go_imported_zero(ty, id.as_str(), fx);
                }

                if let Some(underlying) = self.get_newtype_underlying(ty) {
                    let go_ty = self.go_type_string(ty, fx);
                    let inner = self.lisette_zero(&underlying, fx);
                    return format!("{}({})", go_ty, inner);
                }

                if let Some(fields) =
                    self.lookup_unspecified_fields(ty, "", None, &HashSet::default())
                {
                    let go_ty = self.go_type_string(ty, fx);
                    let is_tuple = self.is_tuple_struct_type(ty);
                    let pairs: Vec<(String, String)> = fields
                        .into_iter()
                        .enumerate()
                        .filter(|(_, (_, field_ty))| !field_ty.is_slice())
                        .map(|(index, (name, field_ty))| {
                            let go_name = if is_tuple {
                                format!("F{}", index)
                            } else if self.field_is_public(ty, &name) {
                                go_name::make_exported(&name)
                            } else {
                                go_name::escape_keyword(&name).into_owned()
                            };
                            (go_name, self.lisette_zero(&field_ty, fx))
                        })
                        .collect();
                    return emit_struct_literal(&go_ty, &pairs, ExpressionContext::value());
                }
                if let Some(underlying) = ty.get_underlying() {
                    return self.lisette_zero(underlying, fx);
                }
                format!("{}{{}}", self.go_type_string(ty, fx))
            }
            Type::Tuple(elements) => {
                let parts: Vec<String> =
                    elements.iter().map(|e| self.lisette_zero(e, fx)).collect();
                fx.require_stdlib();
                format!(
                    "{}.MakeTuple{}({})",
                    go_name::GO_STDLIB_PKG,
                    parts.len(),
                    parts.join(", ")
                )
            }
            _ => format!("{}{{}}", self.go_type_string(ty, fx)),
        }
    }

    /// Address a struct-call field whose enum variant stores it behind a pointer
    /// (recursive-enum cycle breaker). Fields that are already a reference or a
    /// `Ref<T>` value pass through unchanged; others are captured into a temp so
    /// Go can take their address.
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
        if matches!(*field.value, Expression::Reference { .. }) || field.value.get_type().is_ref() {
            return value;
        }
        let temp = self.hoist_tmp_value_statement(statements, "ptr", &value);
        format!("&{}", temp)
    }

    /// Analyze a struct call to determine Go type and enum context.
    fn analyze_struct_call(
        &mut self,
        name: &str,
        ty: &Type,
        fx: &mut EmitEffects,
    ) -> StructCallContext {
        let is_prelude = is_from_prelude(ty);
        let enum_id = self.as_enum(ty);

        let go_type = self.compute_struct_call_go_type(name, ty, is_prelude, enum_id.is_some(), fx);

        if let Some(ref id) = enum_id {
            self.add_enum_imports_if_needed(name, id, fx);
        }

        let enum_ctx = enum_id.map(|id| self.compute_enum_call_context(name, &id, fx));

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
        fx: &mut EmitEffects,
    ) -> String {
        if let Type::Nominal { id, .. } = ty
            && let Some(go) = self.anon_struct_go_type(id)
        {
            fx.merge_from_go_type(&go);
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
                    self.format_type_args(params, fx)
                } else {
                    String::new()
                };
                let pkg = self.require_module_import_fx(parts[0], fx);
                return format!("{}.{}{}", pkg, go_name::snake_to_camel(parts[1]), type_args);
            }
        }

        self.go_type_string(ty, fx)
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
    fn compute_enum_call_context(
        &mut self,
        name: &str,
        enum_id: &str,
        fx: &mut EmitEffects,
    ) -> EnumCallContext {
        let variant_name = unqualified_name(name).to_string();

        let tag_constant = self.resolve_variant(name, enum_id, fx);

        let pointer_fields = if let Some(layout) = self.module.enum_layout(enum_id) {
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

    fn add_enum_imports_if_needed(&mut self, name: &str, enum_id: &str, fx: &mut EmitEffects) {
        if let Some(enum_module) = self.facts.module_for_qualified_name(enum_id)
            && !self.facts.is_current_module(enum_module)
        {
            let enum_module = enum_module.to_string();
            self.require_module_import_fx(&enum_module, fx);
        }

        let parts: Vec<&str> = name.split('.').collect();
        if parts.len() == 3 {
            let module = self
                .module
                .module_for_alias(parts[0])
                .unwrap_or(parts[0])
                .to_string();
            self.require_module_import_fx(&module, fx);
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
        } else if ctx.is_prelude || self.field_is_public(ty, field_name) {
            go_name::make_exported(field_name)
        } else {
            go_name::escape_keyword(field_name).into_owned()
        }
    }

    fn lower_struct_update(
        &mut self,
        statements: &mut Vec<LoweredStatement>,
        base: &Expression,
        fields: &[(String, String)],
        field_side_effects: &[bool],
        fx: &mut EmitEffects,
    ) -> String {
        if fields.is_empty() {
            let staged = self.stage_operand(base, ExpressionContext::value(), fx);
            statements.extend(staged.setup);
            return staged.value;
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

        let base_staged = self.stage_operand(base, ExpressionContext::value(), fx);
        statements.extend(base_staged.setup);
        let tmp = self.hoist_tmp_value_statement(statements, "copy", &base_staged.value);

        for (name, value) in &fields {
            statements.push(LoweredStatement::RawGo(format!(
                "{}.{} = {}\n",
                tmp, name, value
            )));
        }

        tmp
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
