use crate::EmitEffects;
use crate::Planner;
use crate::context::expression::ExpressionContext;
use crate::names::go_name;
use syntax::ast::Expression;
use syntax::program::DefinitionBody;
use syntax::types::{Type, unqualified_name};

impl Planner<'_> {
    /// Emit a value enum variant as a Go constant (e.g., `reflect.String`).
    pub(crate) fn emit_value_enum_variant(
        &mut self,
        expression: &Expression,
        member: &str,
        fx: &mut EmitEffects,
    ) -> Option<String> {
        let expression_ty = expression.get_type();
        let enum_id = match expression_ty.unwrap_forall() {
            Type::Nominal { id, .. } => id.clone(),
            Type::Function(f) => {
                if let Type::Nominal { id, .. } = f.return_type.as_ref() {
                    id.clone()
                } else {
                    return None;
                }
            }
            _ => return None,
        };

        let module_key = self.facts.module_for_qualified_name(&enum_id)?;

        let qualifier = self.require_module_import_fx(module_key, fx);

        Some(format!("{}.{}", qualifier, member))
    }

    /// ADT enum variant dot access (constructor or unit variant).
    pub(crate) fn emit_enum_variant_dot(
        &mut self,
        member: &str,
        result_ty: &Type,
        fx: &mut EmitEffects,
    ) -> Option<String> {
        if let Some(s) = self.emit_enum_variant_constructor(member, result_ty, fx) {
            return Some(s);
        }
        self.emit_unit_variant_constructor(member, result_ty, fx)
    }

    /// Static method dot access (cross-module, alias, or instance-as-value).
    pub(crate) fn emit_static_method_dot(
        &mut self,
        expression: &Expression,
        member: &str,
        result_ty: &Type,
        ctx: ExpressionContext<'_>,
        fx: &mut EmitEffects,
    ) -> Option<String> {
        if let Some(s) =
            self.emit_cross_module_static_method(expression, member, result_ty, ctx, fx)
        {
            return Some(s);
        }
        if let Some(s) = self.emit_alias_static_method(expression, member, result_ty, fx) {
            return Some(s);
        }
        None
    }

    /// Enum variant constructor reference (e.g.
    /// `shapes.ShapeKind.CircleKind` → `shapes.makeShapeKindCircleKind`).
    fn emit_enum_variant_constructor(
        &mut self,
        variant_name: &str,
        result_ty: &Type,
        fx: &mut EmitEffects,
    ) -> Option<String> {
        let Type::Function(f) = result_ty else {
            return None;
        };
        let fn_params = &f.params;

        let Type::Nominal {
            id: enum_id,
            params: ret_params,
            ..
        } = f.return_type.as_ref()
        else {
            return None;
        };

        let enum_name = unqualified_name(enum_id);
        let constructor_key = format!("{}.{}", enum_name, variant_name);

        let make_fn_name = self.facts.make_function_name(&constructor_key)?.to_string();

        let enum_module = self.facts.module_for_qualified_name(enum_id)?;
        let needs_qualifier = !self.facts.is_current_module(enum_module);

        let needs_type_args = ret_params.len() > fn_params.len();
        let type_args = if needs_type_args {
            self.format_type_args(ret_params, fx)
        } else {
            String::new()
        };

        let make_fn = if needs_qualifier {
            if make_fn_name.starts_with(go_name::PRELUDE_PREFIX) {
                let resolved = go_name::resolve(&make_fn_name);
                if resolved.needs_stdlib {
                    fx.require_stdlib();
                }
                format!("{}{}", resolved.name, type_args)
            } else {
                let pkg = self.require_module_import_fx(enum_module, fx);
                format!("{}.{}{}", pkg, make_fn_name, type_args)
            }
        } else {
            format!("{}{}", make_fn_name, type_args)
        };
        Some(make_fn)
    }

    fn emit_unit_variant_constructor(
        &mut self,
        variant_name: &str,
        result_ty: &Type,
        fx: &mut EmitEffects,
    ) -> Option<String> {
        let Type::Nominal {
            id: enum_id,
            params,
            ..
        } = result_ty
        else {
            return None;
        };

        let enum_module = self.facts.module_for_qualified_name(enum_id)?;
        let is_prelude = enum_module == go_name::PRELUDE_MODULE;
        let is_cross_module = !self.facts.is_current_module(enum_module) && !is_prelude;

        let definition = self.facts.definition(enum_id.as_str())?;
        let DefinitionBody::Enum { variants, .. } = &definition.body else {
            return None;
        };

        let variant = variants.iter().find(|v| v.name == variant_name)?;
        if !variant.fields.is_empty() {
            return None;
        }

        let enum_name = unqualified_name(enum_id);
        let key = format!("{}.{}", enum_name, variant_name);
        let make_fn = self.facts.make_function_name(&key)?.to_string();
        let type_args = self.format_type_args(params, fx);

        if is_prelude {
            let resolved = go_name::resolve(&make_fn);
            if resolved.needs_stdlib {
                fx.require_stdlib();
            }
            Some(format!("{}{}()", resolved.name, type_args))
        } else if is_cross_module {
            let pkg = self.require_module_import_fx(enum_module, fx);
            Some(format!("{}.{}{}()", pkg, make_fn, type_args))
        } else {
            Some(format!("{}{}()", make_fn, type_args))
        }
    }

    /// Handles `Alias.new(1)` where `type Alias = Box` → emit as `Box_new(1)`.
    /// The DotAccess is on a type alias identifier whose underlying type has the method.
    fn emit_alias_static_method(
        &mut self,
        expression: &Expression,
        member: &str,
        result_ty: &Type,
        fx: &mut EmitEffects,
    ) -> Option<String> {
        let func_ty = result_ty.unwrap_forall();
        if !matches!(func_ty, Type::Function(_)) {
            return None;
        }

        let Expression::Identifier { value, .. } = expression else {
            return None;
        };

        let real_type = self.resolve_alias_type_name(value)?;

        let resolved_name = format!("{}.{}", real_type, member);

        let capitalized = self.capitalize_static_method_if_public(&resolved_name);
        let go_name = self.resolve_go_name(&capitalized, fx);

        Some(go_name)
    }

    /// Instance method used as a value (e.g. `lib.Point.area` callback →
    /// `lib.Point.Area` Go method expression).
    pub(crate) fn emit_instance_method_value_dot(
        &mut self,
        expression: &Expression,
        member: &str,
        result_ty: &Type,
        is_exported: bool,
        is_pointer_receiver: bool,
        fx: &mut EmitEffects,
    ) -> Option<String> {
        let Expression::DotAccess {
            expression: inner_expression,
            member: type_name,
            ..
        } = expression
        else {
            return None;
        };

        let inner_ty = inner_expression.get_type();

        let module_name = if let Some(synthetic_module) = inner_ty.as_import_namespace() {
            synthetic_module.to_string()
        } else if matches!(&inner_ty, Type::Nominal { .. })
            && let Expression::Identifier { value, .. } = inner_expression.as_ref()
        {
            value.to_string()
        } else {
            return None;
        };
        let module_name = module_name.as_str();

        let go_method = if is_exported {
            go_name::snake_to_camel(member)
        } else {
            go_name::escape_keyword(member).into_owned()
        };

        let pkg = self.require_module_import_fx(module_name, fx);
        let go_type_name = go_name::snake_to_camel(type_name);

        // Extract type args from the receiver parameter
        let type_args = if let Type::Function(f) = result_ty.unwrap_forall()
            && let Some(first_param) = f.params.first()
        {
            let receiver_ty = first_param.strip_refs();
            if let Type::Nominal {
                params: receiver_params,
                ..
            } = receiver_ty
            {
                if receiver_params.is_empty() {
                    String::new()
                } else {
                    self.format_type_args(&receiver_params, fx)
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        let method_expression = if is_pointer_receiver {
            format!("(*{}.{}{}).{}", pkg, go_type_name, type_args, go_method)
        } else {
            format!("{}.{}{}.{}", pkg, go_type_name, type_args, go_method)
        };

        Some(method_expression)
    }

    /// Cross-module static method access (`shapes.Point.new` →
    /// `shapes.Point_new`).
    fn emit_cross_module_static_method(
        &mut self,
        expression: &Expression,
        member: &str,
        result_ty: &Type,
        ctx: ExpressionContext<'_>,
        fx: &mut EmitEffects,
    ) -> Option<String> {
        if !matches!(result_ty.unwrap_forall(), Type::Function(_)) {
            return None;
        }

        let Expression::DotAccess {
            expression: inner_expression,
            member: type_name,
            ..
        } = expression
        else {
            return None;
        };

        let inner_ty = inner_expression.get_type();

        let module_name = if let Some(synthetic_module) = inner_ty.as_import_namespace() {
            synthetic_module.to_string()
        } else if matches!(inner_ty, Type::Nominal { .. }) {
            if let Expression::Identifier { value, .. } = inner_expression.as_ref() {
                value.to_string()
            } else {
                return None;
            }
        } else {
            return None;
        };
        let module_name = module_name.as_str();

        let qualified_type = format!("{}.{}", module_name, type_name);
        let definition = self.facts.definition(qualified_type.as_str())?;

        let is_go_type = go_name::is_go_import(module_name);
        if !is_go_type
            && !matches!(
                definition.body,
                DefinitionBody::Struct { .. }
                    | DefinitionBody::Enum { .. }
                    | DefinitionBody::TypeAlias { .. }
            )
        {
            return None;
        }

        let (qualified_type, _type_name) =
            if matches!(definition.body, DefinitionBody::TypeAlias { .. }) {
                let id = self.peel_alias_id(&qualified_type);
                let resolved_name = unqualified_name(&id).to_string();
                (id, resolved_name)
            } else {
                (qualified_type, type_name.to_string())
            };

        let qualified_method = format!("{}.{}", qualified_type, member);

        let is_public = definition.visibility().is_public() || self.method_needs_export(member);
        let qualified_name = self.qualify_method_call(&qualified_type, member, is_public, fx);

        let type_args = if !ctx.is_callee() {
            self.format_cross_module_type_args(&qualified_method, result_ty, fx)
                .unwrap_or_default()
        } else {
            String::new()
        };

        Some(format!("{}{}", qualified_name, type_args))
    }
}
