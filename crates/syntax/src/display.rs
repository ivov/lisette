use std::borrow::Cow;
use std::fmt;

use crate::ast::Annotation;
use crate::program::is_internal_package_id;
use crate::program::{go_import_default_name, is_go_import, strip_go_prefix};
use crate::types::SimpleKind;
use crate::types::{Symbol, Type};

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
        let body = self.stringify_body(qualifier);
        if self.is_writable() {
            format!("mut {}", body)
        } else {
            body
        }
    }

    fn stringify_body(&self, qualifier: Qualifier) -> String {
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

                if !is_go_import(id) {
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
                    return name.into_owned();
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
                    .map(|param| param.ty.stringify_as(qualifier))
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
                let path = strip_go_prefix(package_id).unwrap_or(package_id);
                path.rsplit('/').next().unwrap_or(package_id).to_string()
            }

            Type::ReceiverPlaceholder => "self".to_string(),

            Type::Simple(kind) => match kind {
                SimpleKind::Unit => "()".to_string(),
                _ => kind.leaf_name().to_string(),
            },

            Type::Compound { kind, args, .. } => {
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

fn qualified_name(id: &Symbol, qualifier: Qualifier) -> Cow<'_, str> {
    if let Some(path) = strip_go_prefix(id) {
        if qualifier == Qualifier::Package
            && let Some(short) = go_package_qualified_name(path, id.last_segment())
        {
            return Cow::Owned(short);
        }
        return Cow::Borrowed(path);
    }

    if qualifier == Qualifier::Package {
        return Cow::Borrowed(id.last_segment());
    }

    match id.as_str().split_once('.') {
        Some((package, _))
            if package != crate::ENTRY_PACKAGE_ID && !is_internal_package_id(package) =>
        {
            Cow::Borrowed(id.as_str())
        }
        _ => Cow::Borrowed(id.last_segment()),
    }
}

pub fn type_display_name(id: &Symbol) -> String {
    qualified_name(id, Qualifier::Package).into_owned()
}

fn go_package_qualified_name(path: &str, leaf: &str) -> Option<String> {
    let package_path = path.strip_suffix(leaf)?.strip_suffix('.')?;
    let package = go_import_default_name(package_path);
    let is_identifier = package
        .chars()
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && package
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_');
    is_identifier.then(|| format!("{package}.{leaf}"))
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let (types, _generics) = Self::remove_vars(&[self]);
        write!(f, "{}", types[0].stringify())
    }
}

pub fn annotation_to_string(ann: &Annotation) -> String {
    match ann {
        Annotation::Constructor {
            name,
            params,
            writable,
            ..
        } => {
            let rendered = if params.is_empty() {
                name.to_string()
            } else {
                format!(
                    "{}<{}>",
                    name,
                    params
                        .iter()
                        .map(annotation_to_string)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            if *writable {
                format!("mut {}", rendered)
            } else {
                rendered
            }
        }
        Annotation::Function {
            params,
            return_type,
            ..
        } => {
            let params_str = params
                .iter()
                .map(annotation_to_string)
                .collect::<Vec<_>>()
                .join(", ");
            if matches!(return_type.as_ref(), Annotation::Unknown) {
                return format!("fn({})", params_str);
            }
            let ret = annotation_to_string(return_type);
            if ret == "Unit" || ret == "()" {
                format!("fn({})", params_str)
            } else {
                format!("fn({}) -> {}", params_str, ret)
            }
        }
        Annotation::Tuple { elements, .. } => {
            let inner = elements
                .iter()
                .map(annotation_to_string)
                .collect::<Vec<_>>()
                .join(", ");
            format!("({})", inner)
        }
        Annotation::Constant { value, text, .. } => {
            text.clone().unwrap_or_else(|| value.to_string())
        }
        Annotation::Unknown => "Unknown".to_string(),
        Annotation::Opaque { .. } => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::go_package_qualified_name;

    #[test]
    fn a_go_type_takes_the_name_of_an_unaliased_import() {
        assert_eq!(
            go_package_qualified_name("net/http.Request", "Request").as_deref(),
            Some("http.Request")
        );
        assert_eq!(
            go_package_qualified_name("github.com/jackc/pgx/v5.Conn", "Conn").as_deref(),
            Some("pgx.Conn")
        );
    }

    #[test]
    fn a_package_name_that_is_not_an_identifier_keeps_the_path() {
        assert_eq!(
            go_package_qualified_name("gopkg.in/yaml.v3.Node", "Node"),
            None
        );
        assert_eq!(
            go_package_qualified_name("github.com/google/go-github.Client", "Client"),
            None
        );
    }
}
