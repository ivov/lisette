use rustc_hash::FxHashSet as HashSet;

use syntax::ast::{Expression, StructFieldAssignment, StructSpread};
use syntax::program::Definition;
use syntax::types::{CompoundKind, SimpleKind, Type};

use crate::Emitter;
use crate::definitions::enum_layout;
use crate::go_name;
use crate::is_order_sensitive;
use crate::types::coercion::Coercion;
use crate::utils::Staged;
use crate::write_line;

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
    pub(crate) fn emit_struct_call(
        &mut self,
        output: &mut String,
        name: &str,
        field_assignments: &[StructFieldAssignment],
        spread: &StructSpread,
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
            let mut value = emitted_values[fi].clone();
            value = self.wrap_recursive_enum_field(output, value, f, &ctx);
            let value_ty = f.value.get_type();
            let field_ty = self.lookup_struct_field_ty(ty, &f.name);
            if is_go_struct {
                if self.needs_go_pointer_bridge(&value_ty, field_ty.as_ref()) {
                    value = self.emit_option_unwrap_to_go_pointer(output, &value, &value_ty);
                } else {
                    let coercion = Coercion::resolve_unwrap_go_nullable(self, &value_ty);
                    value = coercion.apply(self, output, value);
                }
            }
            if let Some(field_ty) = field_ty.as_ref() {
                let coercion = Coercion::resolve(self, &value_ty, field_ty);
                value = coercion.apply(self, output, value);
            }
            field_names.push(field_name);
            field_values.push(value);
        }

        let mut field_pairs: Vec<(String, String)> =
            field_names.into_iter().zip(field_values).collect();

        if let Some(tag) = tag_field {
            field_pairs.insert(0, tag);
        }

        match spread {
            StructSpread::From(base) => {
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
            }
            StructSpread::ZeroFill { .. } if !is_go_struct => {
                let assigned: HashSet<&str> =
                    field_assignments.iter().map(|f| f.name.as_str()).collect();
                let unspecified =
                    self.lookup_unspecified_fields(ty, name, ctx.enum_ctx.as_ref(), &assigned);
                if let Some(unspecified) = unspecified {
                    for (field_name, field_ty) in unspecified {
                        let go_field_name =
                            self.resolve_struct_call_field_name(&field_name, ty, &ctx);
                        let zero = self.lisette_zero(&field_ty);
                        field_pairs.push((go_field_name, zero));
                    }
                }
                self.emit_struct_literal(&ctx.go_type, &field_pairs)
            }
            StructSpread::ZeroFill { .. } | StructSpread::None => {
                self.emit_struct_literal(&ctx.go_type, &field_pairs)
            }
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
            let Some(Definition::Enum {
                variants, generics, ..
            }) = self.ctx.definitions.get(enum_ctx.enum_id.as_str())
            else {
                return None;
            };
            let variant_name = name.rsplit('.').next()?;
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
        let Some(Definition::Struct {
            fields, ty: def_ty, ..
        }) = self.ctx.definitions.get(id.as_str())
        else {
            return None;
        };
        let map = forall_substitution(def_ty, &params);
        Some(unspecified_pairs(
            fields.iter().map(|f| (&f.name, &f.ty)),
            assigned,
            &map,
        ))
    }
    fn go_imported_zero(&mut self, ty: &Type, id: &str) -> String {
        if self.as_interface(ty).is_some() || self.resolve_to_function_type(ty).is_some() {
            return "nil".to_string();
        }
        let go_ty = self.go_type_as_string(ty);
        let is_struct_like = matches!(
            self.ctx.definitions.get(id),
            Some(Definition::Struct { .. })
        ) || matches!(
            self.ctx.definitions.get(id),
            Some(Definition::TypeAlias { annotation, .. }) if annotation.is_opaque()
        );
        if is_struct_like {
            format!("{}{{}}", go_ty)
        } else {
            format!("*new({})", go_ty)
        }
    }

    pub(crate) fn lisette_zero(&mut self, ty: &Type) -> String {
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
                        .map(|a| self.go_type_as_string(a))
                        .unwrap_or_else(|| "any".to_string());
                    format!("[]{}{{}}", inner)
                }
                CompoundKind::Map => {
                    let key = args
                        .first()
                        .map(|a| self.go_type_as_string(a))
                        .unwrap_or_else(|| "any".to_string());
                    let val = args
                        .get(1)
                        .map(|a| self.go_type_as_string(a))
                        .unwrap_or_else(|| "any".to_string());
                    format!("map[{}]{}{{}}", key, val)
                }
                _ => format!("{}{{}}", self.go_type_as_string(ty)),
            },
            Type::Nominal { id, params, .. } => {
                if id.as_str() == "prelude.Option" {
                    let inner = params
                        .first()
                        .map(|a| self.go_type_as_string(a))
                        .unwrap_or_else(|| "any".to_string());
                    self.flags.needs_stdlib = true;
                    return format!("{}.MakeOptionNone[{}]()", go_name::GO_STDLIB_PKG, inner);
                }
                if go_name::is_go_import(id.as_str()) {
                    return self.go_imported_zero(ty, id.as_str());
                }
                if let Some(fields) =
                    self.lookup_unspecified_fields(ty, "", None, &HashSet::default())
                {
                    let go_ty = self.go_type_as_string(ty);
                    let pairs: Vec<(String, String)> = fields
                        .into_iter()
                        .map(|(name, field_ty)| {
                            let go_name = if self.field_is_public(ty, &name) {
                                go_name::make_exported(&name)
                            } else {
                                go_name::escape_keyword(&name).into_owned()
                            };
                            (go_name, self.lisette_zero(&field_ty))
                        })
                        .collect();
                    return self.emit_struct_literal(&go_ty, &pairs);
                }
                if let Some(underlying) = ty.get_underlying() {
                    return self.lisette_zero(underlying);
                }
                format!("{}{{}}", self.go_type_as_string(ty))
            }
            Type::Tuple(elements) => {
                let go_ty = self.go_type_as_string(ty);
                let parts: Vec<String> = elements.iter().map(|e| self.lisette_zero(e)).collect();
                format!("{}{{{}}}", go_ty, parts.join(", "))
            }
            _ => format!("{}{{}}", self.go_type_as_string(ty)),
        }
    }

    /// Address a struct-call field whose enum variant stores it behind a pointer
    /// (recursive-enum cycle breaker). Fields that are already a reference or a
    /// `Ref<T>` value pass through unchanged; others are captured into a temp so
    /// Go can take their address.
    fn wrap_recursive_enum_field(
        &mut self,
        output: &mut String,
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
        let temp = self.fresh_var(Some("ptr"));
        self.declare(&temp);
        write_line!(output, "{} := {}", temp, value);
        format!("&{}", temp)
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
            let type_args = if let Type::Nominal { params, .. } = ty {
                self.format_type_args(params)
            } else {
                String::new()
            };

            let pkg = self.go_pkg_qualifier(parts[0]);

            if is_enum && parts.len() == 3 {
                // Enum variant via type alias: "module.TypeAlias.Variant"
                // Emit as "module.TypeAlias" (the variant fields are handled separately)
                return format!("{}.{}{}", pkg, go_name::snake_to_camel(parts[1]), type_args);
            } else if !is_enum && parts.len() == 2 {
                // Cross-module struct reference
                return format!("{}.{}{}", pkg, go_name::snake_to_camel(parts[1]), type_args);
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
        let enum_module = go_name::module_of_type_id(enum_id);

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

fn forall_substitution(def_ty: &Type, params: &[Type]) -> syntax::types::SubstitutionMap {
    if let Type::Forall { vars, .. } = def_ty
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
