use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use crate::Emitter;
use syntax::EcoString;
use syntax::ast::{Expression, Generic};
use syntax::program::Definition;
use syntax::types::Type;

fn build_type_map(generics: &[Generic], type_args: &[Type]) -> HashMap<EcoString, Type> {
    generics
        .iter()
        .map(|g| g.name.clone())
        .zip(type_args.iter().cloned())
        .collect()
}

pub(crate) use syntax::types::substitute;

/// Substitute a field's type using generics and their concrete type arguments.
///
/// Convenience wrapper around `build_type_map` + `substitute` for the common
/// pattern of resolving a field's declared type with concrete type args.
pub(crate) fn resolve_field_type(
    generics: &[Generic],
    type_args: &[Type],
    field_ty: &Type,
) -> Type {
    let type_map = build_type_map(generics, type_args);
    substitute(field_ty, &type_map)
}

impl Emitter<'_> {
    pub(crate) fn merge_impl_bounds(&self, type_name: &str, generics: &[Generic]) -> Vec<Generic> {
        let Some(impl_generics) = self.module.impl_bounds.get(type_name) else {
            return generics.to_vec();
        };

        if self.module.unconstrained_impl_receivers.contains(type_name) {
            return generics.to_vec();
        }

        generics
            .iter()
            .map(|g| {
                if !g.bounds.is_empty() {
                    return g.clone();
                }
                if let Some(impl_g) = impl_generics.iter().find(|ig| ig.name == g.name) {
                    Generic {
                        bounds: impl_g.bounds.clone(),
                        ..g.clone()
                    }
                } else {
                    g.clone()
                }
            })
            .collect()
    }

    fn is_map_key_type(ty: &Type, generic_name: &str) -> bool {
        match ty {
            Type::Constructor { id, params, .. } => {
                if (id.as_ref() == "Map" || id.as_ref().ends_with(".Map"))
                    && !params.is_empty()
                    && let Type::Parameter(name) = &params[0]
                    && name.as_ref() == generic_name
                {
                    return true;
                }
                params
                    .iter()
                    .any(|p| Self::is_map_key_type(p, generic_name))
            }
            Type::Function {
                params,
                return_type,
                ..
            } => {
                params
                    .iter()
                    .any(|p| Self::is_map_key_type(p, generic_name))
                    || Self::is_map_key_type(return_type, generic_name)
            }
            Type::Tuple(elements) => elements
                .iter()
                .any(|e| Self::is_map_key_type(e, generic_name)),
            _ => false,
        }
    }

    pub(crate) fn collect_map_key_generics<'a>(
        types: impl Iterator<Item = &'a Type>,
        generic_names: &[&str],
    ) -> HashSet<String> {
        let mut result = HashSet::default();
        for ty in types {
            for generic_name in generic_names {
                if !result.contains(*generic_name) && Self::is_map_key_type(ty, generic_name) {
                    result.insert(generic_name.to_string());
                }
            }
        }
        result
    }

    pub(crate) fn map_key_positions(
        &self,
        id: &str,
        visited: &mut HashSet<String>,
    ) -> HashSet<usize> {
        if !visited.insert(id.to_string()) {
            return HashSet::default();
        }
        let peeled = self.peel_alias_id(id);
        let Some(Definition::Interface { definition, .. }) =
            self.ctx.definitions.get(peeled.as_str())
        else {
            return HashSet::default();
        };
        let names: Vec<&str> = definition
            .generics
            .iter()
            .map(|g| g.name.as_ref())
            .collect();
        let keys = Self::collect_map_key_generics(definition.methods.values(), &names);
        let mut positions: HashSet<usize> = definition
            .generics
            .iter()
            .enumerate()
            .filter(|(_, g)| keys.contains(g.name.as_ref()))
            .map(|(i, _)| i)
            .collect();
        for p in &definition.parents {
            if let Type::Constructor {
                id: pid, params, ..
            } = p
            {
                for position in self.map_key_positions(pid, visited) {
                    if let Some(Type::Parameter(name)) = params.get(position)
                        && let Some(idx) = definition.generics.iter().position(|g| g.name == *name)
                    {
                        positions.insert(idx);
                    }
                }
            }
        }
        positions
    }

    pub(crate) fn enum_map_key_generics(
        &self,
        enum_id: &str,
        generic_names: &[&str],
    ) -> HashSet<String> {
        if let Some(Definition::Enum { variants, .. }) = self.ctx.definitions.get(enum_id) {
            let types = variants.iter().flat_map(|v| v.fields.iter().map(|f| &f.ty));
            Self::collect_map_key_generics(types, generic_names)
        } else {
            HashSet::default()
        }
    }

    pub(crate) fn body_has_map_key_generic(expression: &Expression, generic_name: &str) -> bool {
        match expression {
            Expression::Block { items, .. }
            | Expression::TryBlock { items, .. }
            | Expression::RecoverBlock { items, .. } => items
                .iter()
                .any(|item| Self::body_has_map_key_generic(item, generic_name)),
            Expression::Let {
                binding,
                else_block,
                ..
            } => {
                Self::is_map_key_type(&binding.ty, generic_name)
                    || else_block
                        .as_ref()
                        .is_some_and(|eb| Self::body_has_map_key_generic(eb, generic_name))
            }
            Expression::If {
                consequence,
                alternative,
                ..
            }
            | Expression::IfLet {
                consequence,
                alternative,
                ..
            } => {
                Self::body_has_map_key_generic(consequence, generic_name)
                    || Self::body_has_map_key_generic(alternative, generic_name)
            }
            Expression::Match { arms, .. } => arms
                .iter()
                .any(|arm| Self::body_has_map_key_generic(&arm.expression, generic_name)),
            Expression::Loop { body, .. }
            | Expression::While { body, .. }
            | Expression::WhileLet { body, .. }
            | Expression::For { body, .. } => Self::body_has_map_key_generic(body, generic_name),
            Expression::Task { expression, .. } | Expression::Defer { expression, .. } => {
                Self::body_has_map_key_generic(expression, generic_name)
            }
            Expression::Select { arms, .. } => arms.iter().any(|arm| {
                use syntax::ast::SelectArmPattern;
                match &arm.pattern {
                    SelectArmPattern::Receive { body, .. }
                    | SelectArmPattern::Send { body, .. }
                    | SelectArmPattern::WildCard { body, .. } => {
                        Self::body_has_map_key_generic(body, generic_name)
                    }
                    SelectArmPattern::MatchReceive { arms, .. } => arms
                        .iter()
                        .any(|a| Self::body_has_map_key_generic(&a.expression, generic_name)),
                }
            }),
            Expression::Lambda { body, .. } | Expression::Function { body, .. } => {
                Self::body_has_map_key_generic(body, generic_name)
            }
            _ => {
                Self::is_map_key_type(&expression.get_type(), generic_name)
                    || Self::expression_tree_has_map_key(expression, generic_name)
            }
        }
    }

    fn expression_tree_has_map_key(expression: &Expression, generic_name: &str) -> bool {
        match expression {
            Expression::Call {
                expression,
                args,
                spread,
                ..
            } => {
                Self::is_map_key_type(&expression.get_type(), generic_name)
                    || args
                        .iter()
                        .any(|a| Self::is_map_key_type(&a.get_type(), generic_name))
                    || args
                        .iter()
                        .any(|a| Self::expression_tree_has_map_key(a, generic_name))
                    || spread.as_ref().as_ref().is_some_and(|s| {
                        Self::is_map_key_type(&s.get_type(), generic_name)
                            || Self::expression_tree_has_map_key(s, generic_name)
                    })
            }
            _ => false,
        }
    }

    pub(crate) fn generics_to_string_with_map_keys(
        &mut self,
        generics: &[Generic],
        map_key_generics: &HashSet<String>,
    ) -> String {
        if generics.is_empty() {
            return String::new();
        }

        let generics_string = generics
            .iter()
            .map(|g| {
                let bounds: Vec<_> = g
                    .bounds
                    .iter()
                    .map(|ann| self.annotation_to_go_type(ann))
                    .collect();

                let constraint = match bounds.as_slice() {
                    [] => {
                        if map_key_generics.contains(g.name.as_ref()) {
                            "comparable".to_string()
                        } else {
                            "any".to_string()
                        }
                    }
                    [single] => single.clone(),
                    multiple => {
                        let ifaces = multiple.join("; ");
                        return format!("{} interface {{ {} }}", g.name, ifaces);
                    }
                };

                format!("{} {}", g.name, constraint)
            })
            .collect::<Vec<_>>()
            .join(", ");

        format!("[{}]", generics_string)
    }
}

pub(crate) fn extract_type_mapping(
    generic: &Type,
    concrete: &Type,
    mapping: &mut HashMap<String, Type>,
) {
    match (generic, concrete) {
        (Type::Parameter(name), concrete) => {
            mapping
                .entry(name.to_string())
                .or_insert_with(|| concrete.clone());
        }
        (
            Type::Constructor {
                params: gen_params, ..
            },
            Type::Constructor {
                params: conc_params,
                ..
            },
        ) => {
            for (g, c) in gen_params.iter().zip(conc_params.iter()) {
                extract_type_mapping(g, c, mapping);
            }
        }
        (
            Type::Function {
                params: gen_params,
                return_type: gen_ret,
                ..
            },
            Type::Function {
                params: conc_params,
                return_type: conc_ret,
                ..
            },
        ) => {
            for (g, c) in gen_params.iter().zip(conc_params.iter()) {
                extract_type_mapping(g, c, mapping);
            }
            extract_type_mapping(gen_ret, conc_ret, mapping);
        }
        (Type::Tuple(generic_elems), Type::Tuple(conc)) => {
            for (g, c) in generic_elems.iter().zip(conc.iter()) {
                extract_type_mapping(g, c, mapping);
            }
        }
        _ => {}
    }
}
