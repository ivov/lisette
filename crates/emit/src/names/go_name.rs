use std::borrow::Cow;

use crate::definitions::enum_layout::ENUM_TAG_FIELD;
use syntax::types::Type;

pub(crate) const GO_IMPORT_PREFIX: &str = "go:";

pub(crate) fn is_go_import(id: &str) -> bool {
    id.starts_with(GO_IMPORT_PREFIX)
}

pub(crate) const PRELUDE_MODULE: &str = "prelude";

pub(crate) const PRELUDE_PREFIX: &str = "prelude.";

pub(crate) const PRELUDE_ERROR_ID: &str = "prelude.error";

pub(crate) const GO_STDLIB_PKG: &str = "lisette";

pub const PRELUDE_IMPORT_PATH: &str = "github.com/ivov/lisette/prelude";

pub(crate) use syntax::types::unqualified_name;

pub(crate) fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

/// Convert a Lisette identifier to its exported Go form: snake_case becomes
/// PascalCase (`user_id` → `UserId`), already-Pascal names pass through, and
/// the result is escaped if it collides with a Go keyword.
pub(crate) fn make_exported(name: &str) -> String {
    escape_keyword(&snake_to_camel(name)).into_owned()
}

pub(crate) fn snake_to_camel(s: &str) -> String {
    s.split('_').map(capitalize_first).collect()
}

pub(crate) fn go_package_name(module: &str) -> &str {
    module.rsplit('/').next().unwrap_or(module)
}

pub(crate) fn module_of_type_id(id: &str) -> &str {
    syntax::types::module_part(id)
}

pub(crate) fn sanitize_package_name(name: &str) -> Cow<'_, str> {
    let has_bad_chars = name.chars().any(|c| !c.is_ascii_alphanumeric() && c != '_');
    let starts_with_digit = name.starts_with(|c: char| c.is_ascii_digit());

    if !has_bad_chars && !starts_with_digit && !is_reserved_package_name(name) {
        return Cow::Borrowed(name);
    }

    let mut result: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();

    if result.starts_with(|c: char| c.is_ascii_digit()) {
        result.insert(0, '_');
    }

    if is_reserved_package_name(&result) {
        result.push('_');
    }

    Cow::Owned(result)
}

/// Go reserved keywords that cannot be used as identifiers.
/// See: https://go.dev/ref/spec#Keywords
const GO_KEYWORDS: &[&str] = &[
    "break",
    "case",
    "chan",
    "const",
    "continue",
    "default",
    "defer",
    "else",
    "fallthrough",
    "for",
    "func",
    "go",
    "goto",
    "if",
    "import",
    "interface",
    "map",
    "package",
    "range",
    "return",
    "select",
    "struct",
    "switch",
    "type",
    "var",
];

pub(crate) struct ResolvedName {
    pub(crate) name: String,
    pub(crate) needs_stdlib: bool,
}

impl ResolvedName {
    fn stdlib(name: String) -> Self {
        Self {
            name,
            needs_stdlib: true,
        }
    }

    fn local(name: String) -> Self {
        Self {
            name,
            needs_stdlib: false,
        }
    }
}

/// Convert a qualified Lisette name to its Go equivalent.
///
/// # Examples
/// - `"prelude.Option"` → `"lisette.Option"` (needs_stdlib: true)
/// - `"prelude.Slice.filter"` → `"lisette.SliceFilter"` (needs_stdlib: true)
/// - `"mymodule.foo"` → `"mymodule_foo"` (needs_stdlib: false)
/// - `"range"` → `"range_"` (Go keyword escaped)
pub(crate) fn resolve(name: &str) -> ResolvedName {
    if let Some(rest) = name.strip_prefix(PRELUDE_PREFIX) {
        let go_name: String = rest.split('.').map(snake_to_camel).collect();
        ResolvedName::stdlib(format!("{}.{}", GO_STDLIB_PKG, go_name))
    } else {
        ResolvedName::local(escape_reserved(&name.replace('.', "_")).into_owned())
    }
}

pub(crate) fn variant(
    identifier: &str,
    ty: &Type,
    current_module: &str,
    module_alias: Option<&str>,
) -> ResolvedName {
    let Type::Nominal { id, .. } = ty else {
        return ResolvedName::local(identifier.replace('.', "_"));
    };

    variant_by_id(identifier, id, current_module, module_alias)
}

pub(crate) fn variant_by_id(
    identifier: &str,
    enum_id: &str,
    current_module: &str,
    module_alias: Option<&str>,
) -> ResolvedName {
    let is_prelude = enum_id.starts_with(PRELUDE_PREFIX);
    let enum_module = module_of_type_id(enum_id);
    let enum_name = unqualified_name(enum_id);
    let variant_name = unqualified_name(identifier);

    let needs_qualifier = !is_prelude && enum_module != current_module;

    if is_prelude {
        ResolvedName::stdlib(format!("{}.{enum_name}{variant_name}", GO_STDLIB_PKG))
    } else if variant_name == ENUM_TAG_FIELD {
        let base = format!("{enum_name}Tag_");
        if needs_qualifier {
            let pkg = module_alias.unwrap_or_else(|| go_package_name(enum_module));
            ResolvedName::local(format!("{pkg}.{base}"))
        } else {
            ResolvedName::local(base)
        }
    } else {
        let base = format!("{enum_name}{variant_name}");
        if needs_qualifier {
            let pkg = module_alias.unwrap_or_else(|| go_package_name(enum_module));
            ResolvedName::local(format!("{pkg}.{base}"))
        } else {
            ResolvedName::local(base)
        }
    }
}

/// Go predeclared identifiers (builtin functions, types, constants).
/// See: https://go.dev/ref/spec#Predeclared_identifiers
const GO_BUILTINS: &[&str] = &[
    // Builtin functions
    "any",
    "append",
    "cap",
    "clear",
    "close",
    "complex",
    "copy",
    "delete",
    "imag",
    "init",
    "len",
    "make",
    "max",
    "min",
    "new",
    "panic",
    "print",
    "println",
    "real",
    "recover",
    // Predeclared types
    "bool",
    "byte",
    "comparable",
    "complex64",
    "complex128",
    "error",
    "float32",
    "float64",
    "int",
    "int8",
    "int16",
    "int32",
    "int64",
    "rune",
    "string",
    "uint",
    "uint8",
    "uint16",
    "uint32",
    "uint64",
    "uintptr",
    // Predeclared constants
    "false",
    "iota",
    "nil",
    "true",
];

pub(crate) fn escape_keyword(name: &str) -> Cow<'_, str> {
    if GO_KEYWORDS.contains(&name) {
        Cow::Owned(format!("{}_", name))
    } else {
        Cow::Borrowed(name)
    }
}

/// Go builtins re-exposed by the Lisette prelude under the same name. The
/// `prelude_function_shadowed` diagnostic forbids user code from declaring
/// identifiers with these names, so the emitter need not escape them.
const PRELUDE_BUILTIN_NAMES: &[&str] = &["complex", "max", "min", "panic", "real"];

fn is_reserved_package_name(name: &str) -> bool {
    GO_KEYWORDS.contains(&name) || GO_BUILTINS.contains(&name)
}

fn is_reserved_identifier(name: &str) -> bool {
    GO_KEYWORDS.contains(&name)
        || (GO_BUILTINS.contains(&name) && !PRELUDE_BUILTIN_NAMES.contains(&name))
}

pub(crate) fn escape_reserved(name: &str) -> Cow<'_, str> {
    if is_reserved_identifier(name) {
        Cow::Owned(format!("{}_", name))
    } else {
        Cow::Borrowed(name)
    }
}

pub(crate) fn qualify_method(
    type_id: &str,
    method: &str,
    current_module: &str,
    is_public: bool,
    module_alias: Option<&str>,
) -> ResolvedName {
    let Some((module, type_name)) = type_id.split_once('.') else {
        let method_name = if is_public {
            snake_to_camel(method)
        } else {
            method.to_string()
        };
        return ResolvedName::local(format!("{}_{}", type_id, method_name));
    };

    if module == PRELUDE_MODULE {
        ResolvedName::stdlib(format!(
            "{}.{}{}",
            GO_STDLIB_PKG,
            type_name,
            snake_to_camel(method)
        ))
    } else if module == current_module {
        let method_name = if is_public {
            snake_to_camel(method)
        } else {
            method.to_string()
        };
        ResolvedName::local(format!("{}_{}", type_name, method_name))
    } else {
        let pkg = module_alias.unwrap_or_else(|| go_package_name(module));
        ResolvedName::local(format!("{}.{}_{}", pkg, type_name, snake_to_camel(method)))
    }
}
