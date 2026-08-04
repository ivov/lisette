use std::fmt;

use crate::program::is_internal_package_id;
use crate::types::{GO_IMPORT_PREFIX, Symbol, Type};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Qualifier {
    Package,
    Path,
}

impl Type {
    pub fn stringify(&self) -> String {
        self.stringify_as(Qualifier::Package)
    }

    pub fn stringify_pair(first: &Self, second: &Self) -> (String, String) {
        let (named, _) = Self::remove_vars(&[first, second]);
        let (first, second) = (&named[0], &named[1]);

        let (short_first, short_second) = (first.stringify(), second.stringify());
        if short_first != short_second {
            return (short_first, short_second);
        }

        let wide_first = first.stringify_as(Qualifier::Path);
        let wide_second = second.stringify_as(Qualifier::Path);
        if wide_first == wide_second {
            return (short_first, short_second);
        }

        (wide_first, wide_second)
    }

    fn stringify_as(&self, qualifier: Qualifier) -> String {
        match self {
            Type::Nominal {
                id, params: args, ..
            } => {
                let args_formatted = args
                    .iter()
                    .map(|a| a.stringify_as(qualifier))
                    .collect::<Vec<_>>()
                    .join(", ");

                let leaf = id.last_segment();

                if !id.as_str().starts_with(GO_IMPORT_PREFIX) {
                    if leaf == "Unit" {
                        return "()".to_string();
                    }

                    if leaf == "bool" {
                        return "bool".to_string();
                    }

                    if leaf.starts_with("Tuple") {
                        return format!("({})", args_formatted);
                    }

                    if leaf == "Ref" {
                        return format!("Ref<{}>", args_formatted);
                    }
                }

                let name = qualified_name(id, qualifier);

                if args.is_empty() {
                    return name.to_string();
                }

                format!("{}<{}>", name, args_formatted)
            }

            Type::Var { id, hint } => match hint {
                Some(name) => format!("?{}", name),
                None => format!("?{}", id.index()),
            },

            Type::Uninferred => "?uninferred".to_string(),

            Type::Ignored => "?ignored".to_string(),

            Type::Function(f) => {
                let args_formatted = f
                    .params
                    .iter()
                    .map(|param| {
                        if param.mutable {
                            format!("mut {}", param.ty.stringify_as(qualifier))
                        } else {
                            param.ty.stringify_as(qualifier)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");

                let ret_formatted = f.return_type.stringify_as(qualifier);

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
                    .map(|e| e.stringify_as(qualifier))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("({})", formatted)
            }

            Type::Array { length, element } => {
                format!("Array<{}, {}>", element.stringify_as(qualifier), length)
            }

            Type::Error => "<error>".to_string(),

            Type::ImportNamespace(package_id) if package_id.as_str() == crate::ENTRY_PACKAGE_ID => {
                crate::ROOT_IMPORT.to_string()
            }

            Type::ImportNamespace(package_id) => {
                let path = package_id.strip_prefix("go:").unwrap_or(package_id);
                path.rsplit('/').next().unwrap_or(package_id).to_string()
            }

            Type::ReceiverPlaceholder => "self".to_string(),

            Type::Simple(kind) => match kind {
                crate::types::SimpleKind::Unit => "()".to_string(),
                _ => kind.leaf_name().to_string(),
            },

            Type::Compound { kind, args } => {
                let args_formatted = args
                    .iter()
                    .map(|a| a.stringify_as(qualifier))
                    .collect::<Vec<_>>()
                    .join(", ");
                if args.is_empty() {
                    kind.leaf_name().to_string()
                } else {
                    format!("{}<{}>", kind.leaf_name(), args_formatted)
                }
            }
        }
    }
}

fn qualified_name(id: &Symbol, qualifier: Qualifier) -> &str {
    if let Some(path) = id.as_str().strip_prefix(GO_IMPORT_PREFIX) {
        return path;
    }

    if qualifier == Qualifier::Package {
        return id.last_segment();
    }

    match id.as_str().split_once('.') {
        Some((package, _))
            if package != crate::ENTRY_PACKAGE_ID && !is_internal_package_id(package) =>
        {
            id.as_str()
        }
        _ => id.last_segment(),
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let (types, _generics) = Self::remove_vars(&[self]);
        write!(f, "{}", types[0].stringify())
    }
}
