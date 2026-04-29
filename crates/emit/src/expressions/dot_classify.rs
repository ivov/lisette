use crate::Emitter;
use crate::names::go_name;
use syntax::ast::Expression;
use syntax::program::Definition;
use syntax::types::Type;

impl Emitter<'_> {
    /// Emit a value enum variant as a Go constant (e.g., `reflect.String`).
    pub(crate) fn emit_value_enum_variant(
        &self,
        expression: &Expression,
        member: &str,
    ) -> Option<String> {
        let expression_ty = expression.get_type();
        let enum_id = match expression_ty.unwrap_forall() {
            Type::Nominal { id, .. } => id.clone(),
            Type::Function { return_type, .. } => {
                if let Type::Nominal { id, .. } = return_type.as_ref() {
                    id.clone()
                } else {
                    return None;
                }
            }
            _ => return None,
        };

        let module_key = go_name::module_of_type_id(&enum_id);

        let qualifier = self.go_pkg_qualifier(module_key);

        Some(format!("{}.{}", qualifier, member))
    }

    /// Emit an ADT enum variant dot access (constructor or unit variant).
    ///
    /// Consolidates enum variant constructor, unit variant via alias, and
    /// type alias unit variant sub-cases.
    pub(crate) fn emit_enum_variant_dot(
        &mut self,
        expression: &Expression,
        member: &str,
        result_ty: &Type,
    ) -> Option<String> {
        if let Some(s) = self.emit_enum_variant_constructor(member, result_ty) {
            return Some(s);
        }
        if let Some(s) = self.emit_unit_variant_via_alias(expression, member, result_ty) {
            return Some(s);
        }
        if let Some(s) = self.emit_type_alias_unit_variant(expression, member, result_ty) {
            return Some(s);
        }
        None
    }

    /// Emit a static method dot access (cross-module or alias).
    ///
    /// Consolidates cross-module static methods, alias static methods,
    /// and instance method value references.
    pub(crate) fn emit_static_method_dot(
        &mut self,
        expression: &Expression,
        member: &str,
        result_ty: &Type,
    ) -> Option<String> {
        if let Some(s) = self.emit_cross_module_static_method(expression, member, result_ty) {
            return Some(s);
        }
        if let Some(s) = self.emit_alias_static_method(expression, member, result_ty) {
            return Some(s);
        }
        None
    }

    /// Emit an enum variant constructor reference.
    ///
    /// Handles cross-module enum variant access like `shapes.ShapeKind.CircleKind`
    /// which should emit the make function `shapes.makeShapeKindCircleKind`.
    fn emit_enum_variant_constructor(
        &mut self,
        variant_name: &str,
        result_ty: &Type,
    ) -> Option<String> {
        let Type::Function {
            return_type,
            params: fn_params,
            ..
        } = result_ty
        else {
            return None;
        };

        let Type::Nominal {
            id: enum_id,
            params: ret_params,
            ..
        } = return_type.as_ref()
        else {
            return None;
        };

        let enum_name = enum_id.split('.').next_back().unwrap_or(enum_id);
        let constructor_key = format!("{}.{}", enum_name, variant_name);

        let make_fn_name = self.module.make_functions.get(&constructor_key)?.clone();

        let enum_module = go_name::module_of_type_id(enum_id);
        let needs_qualifier = enum_module != self.current_module;

        let needs_type_args = ret_params.len() > fn_params.len();
        let type_args = if needs_type_args {
            self.format_type_args(ret_params)
        } else {
            String::new()
        };

        let make_fn = if needs_qualifier {
            if make_fn_name.starts_with(go_name::PRELUDE_PREFIX) {
                let resolved = go_name::resolve(&make_fn_name);
                if resolved.needs_stdlib {
                    self.flags.needs_stdlib = true;
                }
                format!("{}{}", resolved.name, type_args)
            } else {
                let pkg = self.go_pkg_qualifier(enum_module);
                format!("{}.{}{}", pkg, make_fn_name, type_args)
            }
        } else {
            format!("{}{}", make_fn_name, type_args)
        };
        Some(make_fn)
    }

    fn emit_unit_variant_via_alias(
        &mut self,
        expression: &Expression,
        variant_name: &str,
        result_ty: &Type,
    ) -> Option<String> {
        let Type::Nominal {
            id: enum_id,
            params,
            ..
        } = result_ty
        else {
            return None;
        };

        let enum_module = enum_id.split('.').next()?;
        let is_prelude = enum_module == go_name::PRELUDE_MODULE;
        let is_cross_module = enum_module != self.current_module && !is_prelude;

        if is_cross_module && !matches!(expression, Expression::Identifier { .. }) {
            return None;
        }

        let definition = self.ctx.definitions.get(enum_id.as_str())?;
        let Definition::Enum { variants, .. } = definition else {
            return None;
        };

        let variant = variants.iter().find(|v| v.name == variant_name)?;
        if !variant.fields.is_empty() {
            return None;
        }

        let enum_name = enum_id.split('.').next_back()?;
        let key = format!("{}.{}", enum_name, variant_name);
        let make_fn = self.module.make_functions.get(&key)?.clone();
        let type_args = self.format_type_args(params);

        if is_prelude {
            let resolved = go_name::resolve(&make_fn);
            if resolved.needs_stdlib {
                self.flags.needs_stdlib = true;
            }
            Some(format!("{}{}()", resolved.name, type_args))
        } else if is_cross_module {
            let pkg = self.require_module_import(enum_module);
            Some(format!("{}.{}{}()", pkg, make_fn, type_args))
        } else {
            Some(format!("{}{}()", make_fn, type_args))
        }
    }

    /// Emit a unit variant access through a type alias.
    ///
    /// Handles cases like `api.UIEvent.Close` where `UIEvent` is a type alias to an enum
    /// and `Close` is a unit variant. Should emit `api.UIEvent{Tag: events.EventClose}`.
    fn emit_type_alias_unit_variant(
        &mut self,
        expression: &Expression,
        variant_name: &str,
        result_ty: &Type,
    ) -> Option<String> {
        let Type::Nominal {
            id: enum_id,
            params,
            ..
        } = result_ty
        else {
            return None;
        };

        let definition = self.ctx.definitions.get(enum_id.as_str())?;
        let Definition::Enum { variants, .. } = definition else {
            return None;
        };

        let variant = variants.iter().find(|v| v.name == variant_name)?;
        if !variant.fields.is_empty() {
            return None;
        }

        let Expression::DotAccess {
            expression: inner_expression,
            member: type_alias_name,
            ..
        } = expression
        else {
            return None;
        };

        let inner_ty = inner_expression.get_type();
        let alias_module = inner_ty.as_import_namespace()?.to_string();
        let alias_module = alias_module.as_str();

        let enum_module = enum_id.split('.').next()?;

        self.require_module_import(enum_module);

        let type_args = self.format_type_args(params);

        let alias_pkg = self.require_module_import(alias_module);
        let tag_value = self.resolve_variant(variant_name, enum_id);
        let literal = format!(
            "{}.{}{}{{ Tag: {} }}",
            alias_pkg,
            go_name::snake_to_camel(type_alias_name),
            type_args,
            tag_value
        );
        // Wrap generic composite literals in parens so gofmt doesn't
        // produce invalid Go in comparison/selector contexts.
        if type_args.is_empty() {
            Some(literal)
        } else {
            Some(format!("({})", literal))
        }
    }

    /// Handles `Alias.new(1)` where `type Alias = Box` → emit as `Box_new(1)`.
    /// The DotAccess is on a type alias identifier whose underlying type has the method.
    fn emit_alias_static_method(
        &mut self,
        expression: &Expression,
        member: &str,
        result_ty: &Type,
    ) -> Option<String> {
        let func_ty = result_ty.unwrap_forall();
        if !matches!(func_ty, Type::Function { .. }) {
            return None;
        }

        let Expression::Identifier { value, .. } = expression else {
            return None;
        };

        let real_type = self.resolve_alias_type_name(value)?;

        let resolved_name = format!("{}.{}", real_type, member);

        let capitalized = self.capitalize_static_method_if_public(&resolved_name);
        let go_name = self.resolve_go_name(&capitalized);

        Some(go_name)
    }

    /// Emit an instance method used as a first-class value (not called).
    ///
    /// Handles cases like `lib.Point.area` used as a callback, emitting Go method
    /// expression syntax like `lib.Point.Area` or `(*lib.Point).Area`.
    ///
    /// Pre-classified by semantics as `InstanceMethodValue`, so no need to re-derive
    /// static vs instance or pointer receiver status.
    pub(crate) fn emit_instance_method_value_dot(
        &mut self,
        expression: &Expression,
        member: &str,
        result_ty: &Type,
        is_exported: bool,
        is_pointer_receiver: bool,
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

        let pkg = self.go_pkg_qualifier(module_name);
        let go_type_name = go_name::snake_to_camel(type_name);

        // Extract type args from the receiver parameter
        let type_args = if let Type::Function { params, .. } = result_ty.unwrap_forall()
            && let Some(first_param) = params.first()
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
                    self.format_type_args(&receiver_params)
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

    /// Emit a cross-module static method access.
    ///
    /// Handles cases like `shapes.Point.new` which should become `shapes.Point_new`.
    /// The expression is a cross-module type reference (e.g., `shapes.Point`) and
    /// the member is a static method (no self parameter).
    fn emit_cross_module_static_method(
        &mut self,
        expression: &Expression,
        member: &str,
        result_ty: &Type,
    ) -> Option<String> {
        if !matches!(result_ty.unwrap_forall(), Type::Function { .. }) {
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
        let definition = self.ctx.definitions.get(qualified_type.as_str())?;

        let is_go_type = go_name::is_go_import(module_name);
        if !is_go_type
            && !matches!(
                definition,
                Definition::Struct { .. } | Definition::Enum { .. } | Definition::TypeAlias { .. }
            )
        {
            return None;
        }

        let (qualified_type, _type_name) = if matches!(definition, Definition::TypeAlias { .. }) {
            let id = self.peel_alias_id(&qualified_type);
            let resolved_name = id.rsplit('.').next().unwrap_or(&id).to_string();
            (id, resolved_name)
        } else {
            (qualified_type, type_name.to_string())
        };

        let qualified_method = format!("{}.{}", qualified_type, member);

        let is_public = definition.visibility().is_public() || self.method_needs_export(member);
        let qualified_name = self.qualify_method_call(&qualified_type, member, is_public);

        let type_args = if !self.emitting_call_callee {
            self.format_cross_module_type_args(&qualified_method, result_ty)
                .unwrap_or_default()
        } else {
            String::new()
        };

        Some(format!("{}{}", qualified_name, type_args))
    }
}
