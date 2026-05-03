use syntax::program::DefinitionBody;

use crate::Emitter;
use crate::names::generics::extract_type_mapping;
use crate::names::go_name;
use syntax::types::{Type, unqualified_name};

pub(crate) enum IdentifierKind {
    /// `Unit` used as expression value → `struct{}{}`
    UnitValue,
    /// Value enum variant (e.g., `Color.Red`) → module-qualified constant
    ValueEnumVariant { go_constant: String },
    /// Public function needing Go capitalization
    PublicFunction { capitalized: String },
    /// Enum variant unit constructor → `MakeName[Types]()`
    UnitConstructor { name: String, type_args: String },
    /// Enum variant constructor as function value → `MakeName[Types]`
    ConstructorFunction { name: String, type_args: String },
    /// Regular identifier (may need static method capitalization or cross-module resolution)
    Regular { name: String },
}

impl Emitter<'_> {
    pub(crate) fn emit_identifier(&mut self, value: &str, ty: &Type) -> String {
        match self.classify_identifier(value, ty) {
            IdentifierKind::UnitValue => "struct{}{}".to_string(),
            IdentifierKind::ValueEnumVariant { go_constant } => go_constant,
            IdentifierKind::PublicFunction { capitalized } => capitalized,
            IdentifierKind::UnitConstructor { name, type_args } => {
                format!("{}{}()", self.resolve_go_name(&name), type_args)
            }
            IdentifierKind::ConstructorFunction { name, type_args } => {
                format!("{}{}", self.resolve_go_name(&name), type_args)
            }
            IdentifierKind::Regular { name } => {
                if let Some(expression) = self.try_emit_method_expression(&name, ty) {
                    return expression;
                }
                let resolved = self.capitalize_static_method_if_public(&name);
                let go_name = self.resolve_go_name(&resolved);
                if !self.emitting_call_callee
                    && let Some(type_args) = self.format_generic_value_type_args(&name, ty)
                {
                    return format!("{}{}", go_name, type_args);
                }
                go_name
            }
        }
    }

    fn classify_identifier(&mut self, value: &str, ty: &Type) -> IdentifierKind {
        if value == "Unit" && ty.is_unit() {
            return IdentifierKind::UnitValue;
        }

        let name = self
            .scope
            .bindings
            .get(value)
            .map(|s| s.to_string())
            .unwrap_or_else(|| value.to_string());

        if let Some(go_constant) = self.try_classify_value_enum_variant(&name, ty) {
            return IdentifierKind::ValueEnumVariant { go_constant };
        }

        if let Some(capitalized) = self.try_capitalize_public_function(&name, ty) {
            return IdentifierKind::PublicFunction { capitalized };
        }

        let mut make_fn = self.module.make_functions.get(&name);

        if make_fn.is_none() {
            let enum_id = match ty {
                Type::Function { return_type, .. } => {
                    if let Type::Nominal { id, .. } = return_type.as_ref() {
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
                make_fn = self.module.make_functions.get(&qualified);
            }
        }

        if let Some(make_fn_value) = make_fn {
            let name = make_fn_value.clone();

            match ty {
                Type::Nominal { params, .. } => {
                    let slot_ty = self.current_slot_expected_ty.clone();
                    let type_args = slot_ty
                        .as_ref()
                        .and_then(|t| self.prelude_container_type_args(t))
                        .unwrap_or_else(|| self.format_type_args(params));
                    return IdentifierKind::UnitConstructor { name, type_args };
                }

                Type::Function {
                    params: fn_params,
                    return_type,
                    ..
                } => {
                    if let Type::Nominal {
                        params: ret_params, ..
                    } = return_type.as_ref()
                    {
                        let type_args = self.constructor_fn_type_args(fn_params, ret_params);
                        return IdentifierKind::ConstructorFunction { name, type_args };
                    }
                }

                _ => unreachable!("make_fn set for unexpected type: {:?}", ty),
            }
        }

        let resolved = make_fn.cloned().unwrap_or(name);
        IdentifierKind::Regular { name: resolved }
    }

    /// Type args for a constructor function reference (e.g. `MakeFoo[T]` used as a value).
    /// Skips type args when the callee position already supplies them or when they can be
    /// inferred from the parameter types.
    fn constructor_fn_type_args(&mut self, fn_params: &[Type], ret_params: &[Type]) -> String {
        let needs_type_args = !self.emitting_call_callee
            || ret_params.len() > fn_params.len()
            || !ret_params
                .iter()
                .all(|rp| fn_params.iter().any(|fp| fp.contains_type(rp)))
            || fn_params
                .iter()
                .any(|t| self.needs_explicit_args_for_go_inference(t));
        if needs_type_args {
            self.format_type_args(ret_params)
        } else {
            String::new()
        }
    }

    /// The identifier's type is already instantiated (Forall stripped by the type checker).
    /// We look up the definition to find the generic signature, then extract concrete
    /// types by matching the generic body against the instantiated type.
    fn format_generic_value_type_args(
        &mut self,
        name: &str,
        instantiated_ty: &Type,
    ) -> Option<String> {
        let qualified_name = format!("{}.{}", self.current_module, name);
        let definition_ty = self
            .ctx
            .definitions
            .get(qualified_name.as_str())
            .or_else(|| {
                let prelude_name = format!("{}.{}", go_name::PRELUDE_MODULE, name);
                self.ctx.definitions.get(prelude_name.as_str())
            })?
            .ty();

        let Type::Forall { vars, body } = definition_ty else {
            return None;
        };

        let mut mapping = rustc_hash::FxHashMap::default();
        extract_type_mapping(body, instantiated_ty, &mut mapping);

        let args: Vec<String> = vars
            .iter()
            .filter_map(|var| {
                let concrete = mapping.get(var.as_str())?;
                Some(self.go_type_as_string(concrete))
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

    /// Like `format_generic_value_type_args` but takes a pre-qualified definition name
    /// instead of constructing one from the current module.
    pub(crate) fn format_cross_module_type_args(
        &mut self,
        qualified_name: &str,
        instantiated_ty: &Type,
    ) -> Option<String> {
        let definition_ty = self.ctx.definitions.get(qualified_name)?.ty().clone();

        let Type::Forall { vars, body } = &definition_ty else {
            return None;
        };

        let mut mapping = rustc_hash::FxHashMap::default();
        extract_type_mapping(body, instantiated_ty, &mut mapping);

        let args: Vec<String> = vars
            .iter()
            .filter_map(|var| {
                let concrete = mapping.get(var.as_str())?;
                Some(self.go_type_as_string(concrete))
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

    /// Check if a dotted name like "Type.method" refers to a receiver method,
    /// and if so return Go method expression syntax instead of free function name.
    ///
    /// Uses the identifier's type (from the AST) to determine whether the first
    /// parameter is `self` — this is reliable because the type checker includes
    /// the receiver as the first param for instance methods but not static ones.
    fn try_emit_method_expression(&mut self, name: &str, id_ty: &Type) -> Option<String> {
        let (type_part, method_part) = name.split_once('.')?;

        if method_part.contains('.') {
            return None;
        }

        let fn_params = match id_ty {
            Type::Function { params, .. } => params,
            Type::Forall { body, .. } => match body.as_ref() {
                Type::Function { params, .. } => params,
                _ => return None,
            },
            _ => return None,
        };

        let real_type_part = self
            .resolve_alias_type_name(type_part)
            .unwrap_or_else(|| type_part.to_string());
        let qualified_name = format!("{}.{}", self.current_module, real_type_part);
        let first = fn_params.first()?;
        let stripped = first.strip_refs();
        let is_self =
            matches!(stripped, Type::Nominal { ref id, .. } if id.as_str() == qualified_name);
        if !is_self {
            return None;
        }
        let type_part = &real_type_part;

        let is_pointer = first.is_ref();

        if self
            .ctx
            .ufcs_methods
            .contains(&(qualified_name.to_string(), method_part.to_string()))
        {
            return None;
        }

        let method_key = format!("{}.{}.{}", self.current_module, type_part, method_part);
        let should_export = self
            .ctx
            .definitions
            .get(method_key.as_str())
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
                self.format_type_args(params)
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

    /// Attempt to resolve a cross-module static method call name using qualify_method.
    /// Returns Some if name matches pattern "module.Type.method", None to fall through.
    pub(crate) fn try_resolve_cross_module_static_method(&mut self, name: &str) -> Option<String> {
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

        if module_name == self.current_module {
            return None;
        }

        let type_id = format!("{}.{}", module_name, type_name);

        let method_key = format!("{}.{}", type_id, method_name);
        let is_public = self
            .ctx
            .definitions
            .get(method_key.as_str())
            .map(|d| d.visibility().is_public())
            .unwrap_or(true)
            || self.method_needs_export(method_name);

        Some(self.qualify_method_call(&type_id, method_name, is_public))
    }

    fn try_classify_value_enum_variant(&self, name: &str, ty: &Type) -> Option<String> {
        if !name.contains('.') {
            return None;
        }

        let Type::Nominal { id: enum_id, .. } = ty else {
            return None;
        };

        let definition = self.ctx.definitions.get(enum_id.as_str())?;
        if !matches!(definition.body, DefinitionBody::ValueEnum { .. }) {
            return None;
        }

        let variant_name = go_name::unqualified_name(name);
        let module = go_name::module_of_type_id(enum_id.as_str());
        let qualifier = self.go_pkg_qualifier(module);

        Some(format!("{}.{}", qualifier, variant_name))
    }

    /// Check if an identifier refers to a public function in the current module.
    /// If so, return its capitalized Go name.
    fn try_capitalize_public_function(&self, name: &str, ty: &Type) -> Option<String> {
        let is_function = matches!(ty, Type::Function { .. })
            || matches!(ty, Type::Forall { body, .. } if matches!(body.as_ref(), Type::Function { .. }));
        if !is_function {
            return None;
        }

        if self.scope.bindings.get(name).is_some() {
            return None;
        }

        if name.contains('.') {
            return None;
        }

        let qualified_name = format!("{}.{}", self.current_module, name);
        let definition = self.ctx.definitions.get(qualified_name.as_str())?;

        if !definition.visibility().is_public() {
            return None;
        }

        Some(go_name::snake_to_camel(name))
    }
}
