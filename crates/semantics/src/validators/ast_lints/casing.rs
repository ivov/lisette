pub fn to_pascal_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = true;
    for c in s.chars() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(c.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }
    result
}

pub fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            result.push(c.to_ascii_lowercase());
        } else {
            result.push(c);
        }
    }
    result
}

pub fn to_screaming_snake_case(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut result = String::with_capacity(s.len() + 4);
    for (i, &b) in bytes.iter().enumerate() {
        if b.is_ascii_uppercase() && i > 0 && bytes[i - 1].is_ascii_lowercase() {
            result.push('_');
        }
        result.push(b.to_ascii_uppercase() as char);
    }
    result
}

pub fn is_snake_case(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }
    let s = s.strip_prefix('_').unwrap_or(s);
    s.chars()
        .all(|c| c.is_lowercase() || c.is_ascii_digit() || c == '_')
}

pub fn is_screaming_snake_case(s: &str) -> bool {
    if s.is_empty() {
        return true;
    }
    let s = s.strip_prefix('_').unwrap_or(s);
    s.chars()
        .all(|c| c.is_uppercase() || c.is_ascii_digit() || c == '_')
}
