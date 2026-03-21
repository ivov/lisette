pub const TEST_WRAPPER_NAME: &str = "__test__";

pub fn wrap(raw_source: &str) -> String {
    let source = raw_source.trim();

    if !needs_wrapping(source) {
        return raw_source.to_string();
    }

    if source.starts_with('{') && source.ends_with('}') {
        let inner = &source[1..source.len() - 1];
        let (hoisted, rest) = extract_type_definitions(inner);
        if !hoisted.is_empty() {
            return format!("{}\nfn {}() {{ {} }}", hoisted, TEST_WRAPPER_NAME, rest);
        }
    }

    format!("fn {}() {{ {} }}", TEST_WRAPPER_NAME, raw_source)
}

fn extract_type_definitions(source: &str) -> (String, String) {
    let mut hoisted = String::new();
    let mut rest = String::new();
    let mut in_def = false;
    let mut brace_depth: i32 = 0;

    for line in source.lines() {
        let trimmed = line.trim();

        if in_def {
            for ch in line.chars() {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => brace_depth -= 1,
                    _ => {}
                }
            }
            hoisted.push_str(line);
            hoisted.push('\n');
            if brace_depth == 0 {
                in_def = false;
            }
            continue;
        }

        if trimmed.starts_with("enum ") || trimmed.starts_with("struct ") {
            brace_depth = 0;
            for ch in line.chars() {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => brace_depth -= 1,
                    _ => {}
                }
            }
            hoisted.push_str(line);
            hoisted.push('\n');
            if brace_depth != 0 {
                in_def = true;
            }
            continue;
        }

        rest.push_str(line);
        rest.push('\n');
    }

    (hoisted, rest)
}

fn needs_wrapping(source: &str) -> bool {
    let source = skip_doc_comments(source);

    if source.starts_with('#') {
        return false;
    }

    ![
        "fn ",
        "struct ",
        "enum ",
        "const ",
        "impl ",
        "import ",
        "type ",
        "interface",
        "pub ",
    ]
    .iter()
    .any(|kw| source.starts_with(kw))
}

fn skip_doc_comments(source: &str) -> &str {
    let mut s = source;
    loop {
        s = s.trim_start();
        if s.starts_with("//") {
            if let Some(newline_pos) = s.find('\n') {
                s = &s[newline_pos + 1..];
            } else {
                return "";
            }
        } else {
            return s;
        }
    }
}
