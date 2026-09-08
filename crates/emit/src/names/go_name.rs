use std::borrow::Cow;

use syntax::go_names::ENUM_TAG_FIELD;
use syntax::types::Type;

use crate::names::packages::PackageUse;
use crate::plan::values::GoExpression;

pub(crate) const GO_IMPORT_PREFIX: &str = "go:";

pub(crate) fn is_go_import(id: &str) -> bool {
    id.starts_with(GO_IMPORT_PREFIX)
}

pub(crate) const PRELUDE_PACKAGE: &str = "prelude";

pub(crate) const PRELUDE_PREFIX: &str = "prelude.";

pub(crate) const PRELUDE_ERROR_ID: &str = "prelude.error";

pub(crate) const GO_STDLIB_PKG: &str = "lisette";

pub(crate) const ADAPTER_TYPE_PREFIX: &str = "_lisAdapter_";

const TEST_HANDLE_PREFIX: &str = "_lisTest_";

pub(crate) const TEST_T_PARAM: &str = "_lisTest_t";

pub(crate) const TEST_CTX_PARAM: &str = "_lisTest_ctx";

const RESERVED_GO_PREFIXES: &[&str] = &[ADAPTER_TYPE_PREFIX, TEST_HANDLE_PREFIX];

pub(crate) fn reserved_prefix_of(go: &str) -> Option<&'static str> {
    RESERVED_GO_PREFIXES
        .iter()
        .find(|&&prefix| go.starts_with(prefix))
        .copied()
}

pub const PRELUDE_IMPORT_PATH: &str = "github.com/ivov/lisette/prelude";

pub(crate) const TEST_PRELUDE_PACKAGE: &str = "**test_prelude";
pub const TESTKIT_IMPORT_PATH: &str = "github.com/ivov/lisette/prelude/testkit";
const TESTKIT_PKG: &str = "testkit";

pub(crate) use syntax::go_names::{
    GO_BUILTINS, GO_KEYWORDS, escape_keyword, escape_type_name, is_go_reserved_word,
    screaming_snake_to_camel, snake_to_camel, snake_to_lower_camel, unexported_method_go_name,
};
pub(crate) use syntax::types::unqualified_name;

/// Convert a Lisette identifier to its exported Go form: snake_case becomes
/// PascalCase (`user_id` → `UserId`), already-Pascal names pass through, and
/// the result is escaped if it collides with a Go keyword.
pub(crate) fn make_exported(name: &str) -> String {
    escape_keyword(&snake_to_camel(name)).into_owned()
}

pub(crate) fn exported_member(owner: &Type, member: &str) -> String {
    let owner = owner.strip_refs();
    if owner.get_qualified_id().is_some_and(is_go_import) && member.starts_with(char::is_uppercase)
    {
        return member.to_string();
    }
    make_exported(member)
}

pub fn go_test_function_name(fn_name: &str) -> String {
    format!("Test{}", snake_to_camel(fn_name))
}

pub(crate) fn iterate_variants_fn_name(enum_name: &str, exported: bool) -> String {
    let method_segment = if exported {
        snake_to_camel("variants")
    } else {
        "variants".to_string()
    };
    format!("{}_{}", enum_name, method_segment)
}

pub(crate) fn go_package_name(package: &str) -> &str {
    package.rsplit('/').next().unwrap_or(package)
}

pub(crate) fn free_method_part(method: &str, exported: bool) -> String {
    if exported {
        snake_to_camel(method)
    } else {
        snake_to_lower_camel(method)
    }
}

pub(crate) fn fresh_suffixed(base: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base}_{n}"))
        .find(|candidate| !taken(candidate))
        .expect("freshening counter is unbounded")
}

pub(crate) fn is_plain_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(c) if c.is_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_alphanumeric() || c == '_')
}

pub(crate) fn sanitize_package_name(name: &str) -> Cow<'_, str> {
    if (name.is_empty() || is_plain_identifier(name)) && !is_reserved_package_name(name) {
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

pub(crate) struct ResolvedName {
    name: String,
    package: Option<PackageUse>,
}

impl ResolvedName {
    fn stdlib(name: String) -> Self {
        Self {
            name,
            package: Some(PackageUse::generated(GeneratedPackage::Prelude)),
        }
    }

    fn local(name: String) -> Self {
        Self {
            name,
            package: None,
        }
    }

    fn foreign(name: String, package: PackageUse) -> Self {
        Self {
            name,
            package: Some(package),
        }
    }

    pub(crate) fn into_expression(self) -> GoExpression {
        match self.package {
            Some(package) => GoExpression::qualified(package, self.name),
            None if self.name.is_empty() => GoExpression::empty(),
            None => GoExpression::name(self.name),
        }
    }
}

/// Convert a qualified Lisette name to its Go equivalent.
///
/// # Examples
/// - `"prelude.Option"` → `"lisette.Option"` (Prelude package)
/// - `"prelude.Slice.filter"` → `"lisette.SliceFilter"` (Prelude package)
/// - `"mypackage.foo"` → `"mypackage_foo"` (no package)
/// - `"range"` → `"range_"` (Go keyword escaped)
pub(crate) fn resolve(name: &str) -> ResolvedName {
    if let Some(rest) = name.strip_prefix(PRELUDE_PREFIX) {
        let go_name: String = rest.split('.').map(snake_to_camel).collect();
        ResolvedName::stdlib(go_name)
    } else {
        ResolvedName::local(escape_reserved(&name.replace('.', "_")).into_owned())
    }
}

pub(crate) fn variant(
    identifier: &str,
    ty: &Type,
    enum_package: &str,
    current_package: &str,
    package: Option<PackageUse>,
) -> ResolvedName {
    let Type::Nominal { id, .. } = ty else {
        return ResolvedName::local(identifier.replace('.', "_"));
    };

    variant_by_id(identifier, id, enum_package, current_package, package)
}

pub(crate) fn variant_by_id(
    identifier: &str,
    enum_id: &str,
    enum_package: &str,
    current_package: &str,
    package: Option<PackageUse>,
) -> ResolvedName {
    let is_prelude = enum_id.starts_with(PRELUDE_PREFIX);
    let enum_name = unqualified_name(enum_id);
    let variant_name = unqualified_name(identifier);

    if is_prelude {
        ResolvedName::stdlib(format!("{enum_name}{variant_name}"))
    } else {
        let base = enum_tag_constant(enum_name, variant_name);
        if enum_package != current_package {
            match package {
                Some(package) => ResolvedName::foreign(base, package),
                None => ResolvedName::local(format!("{}.{base}", go_package_name(enum_package))),
            }
        } else {
            ResolvedName::local(base)
        }
    }
}

pub(crate) fn enum_tag_constant(enum_name: &str, variant_name: &str) -> String {
    if variant_name == ENUM_TAG_FIELD {
        return format!("{enum_name}Tag_");
    }
    let constant = format!("{enum_name}{variant_name}");
    if is_go_reserved_word(&constant) {
        format!("{constant}_")
    } else {
        constant
    }
}

pub(crate) fn enum_make_function(enum_name: &str, variant_name: &str) -> String {
    format!("Make{}{}", escape_type_name(enum_name), variant_name)
}

/// Go builtins re-exposed by the Lisette prelude under the same name. The
/// `prelude_function_shadowed` diagnostic forbids user code from declaring
/// identifiers with these names, so the emitter need not escape them.
const PRELUDE_BUILTIN_NAMES: &[&str] = &["complex", "max", "min", "panic", "real"];

pub(crate) fn is_prelude_go_builtin(qualified_name: &str) -> bool {
    qualified_name
        .strip_prefix("prelude.")
        .is_some_and(|name| PRELUDE_BUILTIN_NAMES.contains(&name))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum GeneratedPackage {
    Prelude,
    Fmt,
    Errors,
    Slices,
    Strings,
    Maps,
    Json,
    Cmp,
    TestKit,
    Testing,
}

impl GeneratedPackage {
    const ALL: &'static [GeneratedPackage] = &[
        GeneratedPackage::Prelude,
        GeneratedPackage::Fmt,
        GeneratedPackage::Errors,
        GeneratedPackage::Slices,
        GeneratedPackage::Strings,
        GeneratedPackage::Maps,
        GeneratedPackage::Json,
        GeneratedPackage::Cmp,
        GeneratedPackage::TestKit,
        GeneratedPackage::Testing,
    ];

    pub(crate) fn path(self) -> &'static str {
        match self {
            GeneratedPackage::Prelude => PRELUDE_IMPORT_PATH,
            GeneratedPackage::Fmt => "fmt",
            GeneratedPackage::Errors => "errors",
            GeneratedPackage::Slices => "slices",
            GeneratedPackage::Strings => "strings",
            GeneratedPackage::Maps => "maps",
            GeneratedPackage::Json => "encoding/json",
            GeneratedPackage::Cmp => "cmp",
            GeneratedPackage::TestKit => TESTKIT_IMPORT_PATH,
            GeneratedPackage::Testing => "testing",
        }
    }

    pub(crate) fn qualifier(self) -> &'static str {
        match self {
            GeneratedPackage::Prelude => GO_STDLIB_PKG,
            GeneratedPackage::Fmt => "fmt",
            GeneratedPackage::Errors => "errors",
            GeneratedPackage::Slices => "slices",
            GeneratedPackage::Strings => "strings",
            GeneratedPackage::Maps => "maps",
            GeneratedPackage::Json => "json",
            GeneratedPackage::Cmp => "cmp",
            GeneratedPackage::TestKit => TESTKIT_PKG,
            GeneratedPackage::Testing => "testing",
        }
    }
}

pub(crate) fn prelude_qualifier() -> &'static str {
    GeneratedPackage::Prelude.qualifier()
}

pub(crate) fn testkit_qualifier() -> &'static str {
    GeneratedPackage::TestKit.qualifier()
}

pub(crate) fn testing_qualifier() -> &'static str {
    GeneratedPackage::Testing.qualifier()
}

pub(crate) fn is_generated_import_qualifier(name: &str) -> bool {
    GeneratedPackage::ALL
        .iter()
        .any(|package| package.qualifier() == name)
}

fn is_reserved_package_name(name: &str) -> bool {
    name == "main" || name == "documentation" || is_go_reserved_word(name)
}

fn is_reserved_identifier(name: &str) -> bool {
    GO_KEYWORDS.contains(&name)
        || (GO_BUILTINS.contains(&name) && !PRELUDE_BUILTIN_NAMES.contains(&name))
        || is_generated_import_qualifier(name)
}

pub(crate) fn escape_reserved(name: &str) -> Cow<'_, str> {
    if is_reserved_identifier(name) {
        Cow::Owned(format!("{}_", name))
    } else {
        Cow::Borrowed(name)
    }
}

pub(crate) fn qualify_method(
    package: Option<&str>,
    type_name: &str,
    method: &str,
    current_package: &str,
    is_public: bool,
    package_use: Option<PackageUse>,
) -> ResolvedName {
    let Some(package) = package else {
        let method_name = free_method_part(method, is_public);
        return ResolvedName::local(format!("{}_{}", type_name, method_name));
    };

    if package == PRELUDE_PACKAGE {
        ResolvedName::stdlib(format!("{}{}", type_name, snake_to_camel(method)))
    } else if package == current_package {
        let method_name = free_method_part(method, is_public);
        ResolvedName::local(format!("{}_{}", type_name, method_name))
    } else {
        let name = format!("{}_{}", type_name, snake_to_camel(method));
        match package_use {
            Some(package_use) => ResolvedName::foreign(name, package_use),
            None => ResolvedName::local(format!("{}.{name}", go_package_name(package))),
        }
    }
}
