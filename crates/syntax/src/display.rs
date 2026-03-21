use std::fmt;

use crate::types::{Type, TypeVariableState, unqualified_name};

impl Type {
    pub fn stringify(&self) -> String {
        match self {
            Type::Constructor {
                id, params: args, ..
            } => {
                let args_formatted = args
                    .iter()
                    .map(|a| a.stringify())
                    .collect::<Vec<_>>()
                    .join(", ");

                let name = unqualified_name(id);

                if name == "Unit" {
                    return "()".to_string();
                }

                if name == "bool" {
                    return "bool".to_string();
                }

                if name.starts_with("Tuple") {
                    return format!("({})", args_formatted);
                }

                if name == "Ref" {
                    return format!("Ref<{}>", args_formatted);
                }

                if args.is_empty() {
                    return name.to_string();
                }

                format!("{}<{}>", name, args_formatted)
            }

            Type::Variable(var) => match &*var.borrow() {
                TypeVariableState::Unbound { id, hint } => match hint {
                    Some(name) => format!("?{}", name),
                    None => format!("?{}", id),
                },
                TypeVariableState::Link(ty) => format!("{}", ty),
            },

            Type::Function {
                params: args,
                param_mutability,
                return_type,
                ..
            } => {
                let args_formatted = args
                    .iter()
                    .enumerate()
                    .map(|(i, a)| {
                        let is_mut = param_mutability.get(i).copied().unwrap_or(false);
                        if is_mut {
                            format!("mut {}", a.stringify())
                        } else {
                            a.stringify()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");

                let ret_formatted = (*return_type).stringify();

                format!("fn ({}) -> {}", args_formatted, ret_formatted)
            }

            Type::Forall { .. } => {
                unreachable!("Forall types are always instantiated before display")
            }

            Type::Parameter(name) => name.to_string(),

            Type::Never => "Never".to_string(),

            Type::Tuple(elements) => {
                let formatted = elements
                    .iter()
                    .map(|e| e.stringify())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("({})", formatted)
            }

            Type::Error => "<error>".to_string(),
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let (types, _generics) = Self::remove_vars(&[self]);
        write!(f, "{}", types[0].stringify())
    }
}
