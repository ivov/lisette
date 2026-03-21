/// All keywords, including contextual ones (`var`, `self`).
const KEYWORDS: &[&str] = &[
    "fn",
    "let",
    "if",
    "else",
    "match",
    "enum",
    "struct",
    "type",
    "interface",
    "impl",
    "const",
    "var",
    "return",
    "defer",
    "import",
    "mut",
    "pub",
    "for",
    "in",
    "while",
    "loop",
    "break",
    "continue",
    "select",
    "task",
    "try",
    "recover",
    "as",
    "true",
    "false",
    "self",
];

pub(crate) fn validate_rename(new_name: &str) -> Result<(), String> {
    if new_name.is_empty() {
        return Err("Identifier cannot be empty".to_string());
    }

    let first = new_name.chars().next().unwrap();
    if !first.is_alphabetic() && first != '_' {
        return Err(format!(
            "Identifier must start with a letter or underscore, not '{}'",
            first
        ));
    }

    if let Some(invalid) = new_name.chars().find(|c| !c.is_alphanumeric() && *c != '_') {
        return Err(format!(
            "Identifier cannot contain '{}' - only letters, digits, and underscores allowed",
            invalid
        ));
    }

    if KEYWORDS.contains(&new_name) {
        return Err(format!("'{}' is a reserved keyword", new_name));
    }

    Ok(())
}

fn is_prelude_symbol(qualified_name: &str) -> bool {
    qualified_name.starts_with("prelude.")
}

fn is_go_import(qualified_name: &str) -> bool {
    qualified_name.starts_with("go:")
}

pub(crate) fn rename_error(
    message: impl Into<std::borrow::Cow<'static, str>>,
) -> tower_lsp::jsonrpc::Error {
    tower_lsp::jsonrpc::Error {
        code: tower_lsp::jsonrpc::ErrorCode::InvalidParams,
        message: message.into(),
        data: None,
    }
}

pub(crate) fn check_rename_guards(qualified_name: &str) -> Result<(), tower_lsp::jsonrpc::Error> {
    if is_prelude_symbol(qualified_name) {
        return Err(rename_error("Cannot rename prelude symbol"));
    }
    if is_go_import(qualified_name) {
        return Err(rename_error("Cannot rename Go import"));
    }
    Ok(())
}
