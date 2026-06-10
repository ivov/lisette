use crate::EmitEffects;
use crate::Planner;
use crate::context::expression::ExpressionContext;
use crate::names::generics::extract_type_mapping;
use crate::names::go_name;
use crate::state::bindings::BindingValue;
use syntax::types::{Type, unqualified_name};

pub(crate) enum IdentifierKind {
    /// `Unit` used as expression value → `struct{}{}`
    UnitValue,
    /// Public function needing Go capitalization
    PublicFunction { capitalized: String },
    /// Enum variant unit constructor → `MakeName[Types]()`
    UnitConstructor { name: String, type_args: String },
    /// Enum variant constructor as function value → `MakeName[Types]`
    ConstructorFunction { name: String, type_args: String },
    /// Regular identifier (may need static method capitalization or cross-module resolution)
    Regular { name: String },
}

impl Planner<'_> {
    pub(crate) fn emit_identifier(
        &mut self,
        value: &str,
        ty: &Type,
        ctx: ExpressionContext<'_>,
        fx: &mut EmitEffects,
    ) -> String {
        if let Some(BindingValue::InlineExpr(expr)) = self.scope.resolve_identifier_binding(value) {
            let text = expr.as_str().to_string();
            let refs = expr.refs().to_vec();
            for go_name in &refs {
                self.scope.record_go_use(go_name);
            }
            return text;
        }
        let bound_go_name = self
            .scope
            .resolve_binding_go_name(value)
            .map(str::to_string);
        if let Some(go_name) = &bound_go_name {
            self.scope.record_go_use(go_name);
        }
        match self.classify_identifier(value, ty, ctx, fx) {
            IdentifierKind::UnitValue => "struct{}{}".to_string(),
            IdentifierKind::PublicFunction { capitalized } => capitalized,
            IdentifierKind::UnitConstructor { name, type_args } => {
                format!("{}{}()", self.resolve_go_name(&name, fx), type_args)
            }
            IdentifierKind::ConstructorFunction { name, type_args } => {
                format!("{}{}", self.resolve_go_name(&name, fx), type_args)
            }
            IdentifierKind::Regular { name } => {
                if let Some(expression) = self.try_emit_method_expression(&name, ty, fx) {
                    return expression;
                }
                let resolved = self.capitalize_static_method_if_public(&name);
                let go_name = self.resolve_go_name(&resolved, fx);
                if !ctx.is_callee()
                    && let Some(type_args) = self.format_generic_value_type_args(&name, ty, fx)
                {
                    return format!("{}{}", go_name, type_args);
                }
                go_name
            }
        }
    }

    fn classify_identifier(
        &mut self,
        value: &str,
        ty: &Type,
        ctx: ExpressionContext<'_>,
        fx: &mut EmitEffects,
    ) -> IdentifierKind {
        if value == "Unit" && ty.is_unit() {
            return IdentifierKind::UnitValue;
        }

        let name = self
            .scope
            .resolve_binding_go_name(value)
            .unwrap_or(value)
            .to_string();

        if let Some(capitalized) = self.try_capitalize_public_function(&name, ty) {
            return IdentifierKind::PublicFunction { capitalized };
        }

        let mut make_fn = self.facts.make_function_name(&name);

        if make_fn.is_none() {
            let enum_id = match ty {
                Type::Function(f) => {
                    if let Type::Nominal { id, .. } = f.return_type.as_ref() {
                        Some(id.as_str())
                    } else {
                        None
                    }
                }
                Type::Nominal { id, .. } => Some(id.as_str()),
                _ => None,
            };

            if let Some(id) = enum_id {
                let enum_name = unqualified_name(id);
                let qualified = format!("{}.{}", enum_name, value);
                make_fn = self.facts.make_function_name(&qualified);
            }
        }

        if let Some(make_fn_value) = make_fn {
            let name = make_fn_value.to_string();

            match ty {
                Type::Nominal { params, .. } => {
                    let type_args = match ctx.expected_slot_type() {
                        Some(t) => self
                            .prelude_container_type_args(t, fx)
                            .unwrap_or_else(|| self.format_type_args(params, fx)),
                        None => self.format_type_args(params, fx),
                    };
                    return IdentifierKind::UnitConstructor { name, type_args };
                }

                Type::Function(f) => {
                    if let Type::Nominal {
                        params: ret_params, ..
                    } = f.return_type.as_ref()
                    {
                        let type_args =
                            self.constructor_fn_type_args(&f.params, ret_params, ctx, fx);
                        return IdentifierKind::ConstructorFunction { name, type_args };
                    }
                }

                _ => unreachable!("make_fn set for unexpected type: {:?}", ty),
            }
        }

        let resolved = make_fn.map(str::to_string).unwrap_or(name);
        IdentifierKind::Regular { name: resolved }
    }

    /// Type args for a constructor function reference (e.g. `MakeFoo[T]` used as a value).
    /// Skips type args when the callee position already supplies them or when they can be
    /// inferred from the parameter types.
    fn constructor_fn_type_args(
        &mut self,
        fn_params: &[Type],
        ret_params: &[Type],
        ctx: ExpressionContext<'_>,
        fx: &mut EmitEffects,
    ) -> String {
        let needs_type_args = !ctx.is_callee()
            || ret_params.len() > fn_params.len()
            || !ret_params
                .iter()
                .all(|rp| fn_params.iter().any(|fp| fp.contains_type(rp)))
            || fn_params
                .iter()
                .any(|t| self.needs_explicit_args_for_go_inference(t));
        if needs_type_args {
            self.format_type_args(ret_params, fx)
        } else {
            String::new()
        }
    }

    /// Match a generic definition's `Forall` body against an instantiated type
    /// and render the Go type-argument list `[T1, T2]`. `None` when the
    /// definition is not generic, a var is unresolved, or any arg is `interface{}`.
    fn format_type_args_from_forall(
        &mut self,
        definition_ty: &Type,
        instantiated_ty: &Type,
        fx: &mut EmitEffects,
    ) -> Option<String> {
        let Type::Forall { vars, body } = definition_ty else {
            return None;
        };

        let mut mapping = rustc_hash::FxHashMap::default();
        extract_type_mapping(body, instantiated_ty, &mut mapping);

        let args: Vec<String> = vars
            .iter()
            .filter_map(|var| {
                let concrete = mapping.get(var.as_str())?;
                Some(self.go_type_string(concrete, fx))
            })
            .collect();

        if args.len() != vars.len()
            || args.is_empty()
            || args.iter().any(|a| a.contains("interface{}"))
        {
            return None;
        }

        Some(format!("[{}]", args.join(", ")))
    }

    /// Recover the type-arg list from the identifier's instantiated type by
    /// matching against the definition's generic signature.
    fn format_generic_value_type_args(
        &mut self,
        name: &str,
        instantiated_ty: &Type,
        fx: &mut EmitEffects,
    ) -> Option<String> {
        let qualified_name = self.facts.qualified_current(name);
        let definition_ty = self
            .facts
            .definition(qualified_name.as_str())
            .or_else(|| {
                let prelude_name = format!("{}.{}", go_name::PRELUDE_MODULE, name);
                self.facts.definition(prelude_name.as_str())
            })?
            .ty();

        self.format_type_args_from_forall(definition_ty, instantiated_ty, fx)
    }

    /// Like `format_generic_value_type_args` but takes a pre-qualified definition name
    /// instead of constructing one from the current module.
    pub(crate) fn format_cross_module_type_args(
        &mut self,
        qualified_name: &str,
        instantiated_ty: &Type,
        fx: &mut EmitEffects,
    ) -> Option<String> {
        let definition_ty = self.facts.definition(qualified_name)?.ty().clone();

        self.format_type_args_from_forall(&definition_ty, instantiated_ty, fx)
    }

    /// Return Go method-expression syntax for a `Type.method` referring to an
    /// instance method (first param is `self`); `None` for static methods.
    fn try_emit_method_expression(
        &mut self,
        name: &str,
        id_ty: &Type,
        fx: &mut EmitEffects,
    ) -> Option<String> {
        let (type_part, method_part) = name.split_once('.')?;

        if method_part.contains('.') {
            return None;
        }

        let fn_params = match id_ty {
            Type::Function(f) => &f.params,
            Type::Forall { body, .. } => match body.as_ref() {
                Type::Function(f) => &f.params,
                _ => return None,
            },
            _ => return None,
        };

        let real_type_part = self
            .resolve_alias_type_name(type_part)
            .unwrap_or_else(|| type_part.to_string());
        let qualified_name = self.facts.qualified_current(&real_type_part);
        let first = fn_params.first()?;
        let stripped = first.strip_refs();
        let is_self =
            matches!(stripped, Type::Nominal { ref id, .. } if id.as_str() == qualified_name);
        if !is_self {
            return None;
        }
        let type_part = &real_type_part;

        let is_pointer = first.is_ref();

        if self.facts.is_ufcs_method(&qualified_name, method_part) {
            return None;
        }

        let method_key = self.facts.qualified_current_member(type_part, method_part);
        let should_export = self
            .facts
            .definition(method_key.as_str())
            .map(|d| d.visibility().is_public())
            .unwrap_or(false)
            || self.method_needs_export(method_part);
        let go_method = if should_export {
            go_name::snake_to_camel(method_part)
        } else {
            go_name::escape_keyword(method_part).into_owned()
        };

        let type_args = if let Type::Nominal { ref params, .. } = stripped {
            if params.is_empty() {
                String::new()
            } else {
                self.format_type_args(params, fx)
            }
        } else {
            String::new()
        };

        if is_pointer {
            Some(format!("(*{}{}).{}", type_part, type_args, go_method))
        } else {
            Some(format!("{}{}.{}", type_part, type_args, go_method))
        }
    }

    /// Resolve `module.Type.method` as a cross-module static method call.
    pub(crate) fn try_resolve_cross_module_static_method(
        &mut self,
        name: &str,
        fx: &mut EmitEffects,
    ) -> Option<String> {
        if !name.contains('.') {
            return None;
        }

        let last_dot = name.rfind('.')?;
        let method_name = &name[last_dot + 1..];
        let type_and_module = &name[..last_dot];

        let type_name = if let Some(dot_position) = type_and_module.rfind('.') {
            &type_and_module[dot_position + 1..]
        } else if let Some(slash_position) = type_and_module.rfind('/') {
            &type_and_module[slash_position + 1..]
        } else {
            return None;
        };

        let module_name = if let Some(dot_position) = type_and_module.rfind('.') {
            type_and_module[..dot_position].to_string()
        } else if let Some(slash_position) = type_and_module.rfind('/') {
            type_and_module[..slash_position].to_string()
        } else {
            return None;
        };

        if self.facts.is_current_module(&module_name) {
            return None;
        }

        let type_id = format!("{}.{}", module_name, type_name);

        let method_key = format!("{}.{}", type_id, method_name);
        let is_public = self
            .facts
            .definition(method_key.as_str())
            .map(|d| d.visibility().is_public())
            .unwrap_or(true)
            || self.method_needs_export(method_name);

        Some(self.qualify_method_call(&type_id, method_name, is_public, fx))
    }

    /// `Some(capitalized)` when the identifier names a public function in
    /// the current module.
    fn try_capitalize_public_function(&self, name: &str, ty: &Type) -> Option<String> {
        let is_function = matches!(ty, Type::Function(_))
            || matches!(ty, Type::Forall { body, .. } if matches!(body.as_ref(), Type::Function(_)));
        if !is_function {
            return None;
        }

        if self.scope.resolve_identifier_binding(name).is_some() {
            return None;
        }

        if name.contains('.') {
            return None;
        }

        let qualified_name = self.facts.qualified_current(name);
        let definition = self.facts.definition(qualified_name.as_str())?;

        if !definition.visibility().is_public() {
            return None;
        }

        Some(go_name::snake_to_camel(name))
    }
}
