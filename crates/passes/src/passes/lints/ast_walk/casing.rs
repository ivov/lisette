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
    if s.is_empty() {
        return String::new();
    }

    let chars: Vec<char> = s.chars().collect();
    let mut result = String::with_capacity(s.len() + 4);

    for (i, &c) in chars.iter().enumerate() {
        if c == '_' {
            result.push('_');
            continue;
        }

        if c.is_uppercase() {
            let prev_upper = i > 0 && chars[i - 1].is_uppercase();
            let next_lower = i + 1 < chars.len() && chars[i + 1].is_lowercase();

            if (i > 0 && !prev_upper) || (i > 1 && prev_upper && next_lower) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn to_snake_case_handles_acronyms() {
        assert_eq!(to_snake_case("ID"), "id");
        assert_eq!(to_snake_case("URL"), "url");
        assert_eq!(to_snake_case("HTTP"), "http");

        assert_eq!(to_snake_case("HTMLParser"), "html_parser");
        assert_eq!(to_snake_case("XMLHttpRequest"), "xml_http_request");

        assert_eq!(to_snake_case("UserID"), "user_id");
        assert_eq!(to_snake_case("APIKey"), "api_key");

        assert_eq!(to_snake_case("oddsAndEnds"), "odds_and_ends");
        assert_eq!(to_snake_case("FooBar"), "foo_bar");

        assert_eq!(to_snake_case("user_id"), "user_id");
        assert_eq!(to_snake_case("hello"), "hello");
        assert_eq!(to_snake_case(""), "");
    }
}
