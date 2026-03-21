pub fn use_color() -> bool {
    std::env::var("NO_COLOR").is_err()
}

pub fn format_elapsed(elapsed: std::time::Duration) -> String {
    let time_str = if elapsed.as_secs() >= 1 {
        format!("{:.2}s", elapsed.as_secs_f64())
    } else if elapsed.as_millis() > 0 {
        format!("{}ms", elapsed.as_millis())
    } else {
        format!("{}μs", elapsed.as_micros())
    };

    if use_color() {
        use owo_colors::OwoColorize;
        format!("{}", format!("({})", time_str).dimmed())
    } else {
        format!("({})", time_str)
    }
}

pub fn format_backticks(text: &str, use_color: bool) -> String {
    if !use_color {
        return text.to_string();
    }

    use owo_colors::OwoColorize;

    let mut result = String::new();
    let mut chars = text.char_indices().peekable();
    let mut segment_start = 0;

    while let Some((i, ch)) = chars.next() {
        if ch == '`' {
            if i > segment_start {
                result.push_str(&text[segment_start..i]);
            }

            let mut found_closing = false;
            for (j, inner_ch) in chars.by_ref() {
                if inner_ch == '`' {
                    let quoted = &text[i + 1..j];
                    result.push_str(&format!("{}", quoted.bright_magenta()));
                    segment_start = j + 1;
                    found_closing = true;
                    break;
                }
            }

            if !found_closing {
                result.push_str(&text[i..]);
                segment_start = text.len();
            }
        }
    }

    if segment_start < text.len() {
        result.push_str(&text[segment_start..]);
    }

    result
}

fn format_help_text(text: &str, use_color: bool) -> String {
    if !use_color {
        let mut result = text.to_string();
        result = result.replace(":g]", "]");
        result = result.replace(":b]", "]");
        let mut out = String::new();
        let mut chars = result.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '{' {
                let mut content = String::new();
                for inner in chars.by_ref() {
                    if inner == '}' {
                        break;
                    }
                    content.push(inner);
                }
                let clean = content
                    .strip_suffix(":g")
                    .or_else(|| content.strip_suffix(":b"))
                    .unwrap_or(&content);
                out.push_str(clean);
            } else if ch == '`' {
                for inner in chars.by_ref() {
                    if inner == '`' {
                        break;
                    }
                    out.push(inner);
                }
            } else {
                out.push(ch);
            }
        }
        return out;
    }

    use owo_colors::OwoColorize;

    let mut result = String::new();
    let mut chars = text.char_indices().peekable();
    let mut segment_start = 0;

    while let Some((i, ch)) = chars.next() {
        let close = match ch {
            '`' => '`',
            '[' => ']',
            '<' => '>',
            '{' => '}',
            _ => continue,
        };

        if i > segment_start {
            result.push_str(&text[segment_start..i]);
        }

        let mut found_closing = false;
        for (j, inner_ch) in chars.by_ref() {
            if inner_ch == close {
                let content = &text[i + 1..j];
                let formatted = match ch {
                    '`' => format!("{}", content.bright_magenta()),
                    '[' => {
                        if let Some(name) = content.strip_suffix(":g") {
                            format!("{}", format!("[{}]", name).green())
                        } else if let Some(name) = content.strip_suffix(":b") {
                            format!("{}", format!("[{}]", name).blue())
                        } else {
                            format!("{}", format!("[{}]", content).blue())
                        }
                    }
                    '<' => format!("{}", format!("<{}>", content).green()),
                    '{' => {
                        if let Some(name) = content.strip_suffix(":g") {
                            format!("{}", name.green())
                        } else if let Some(name) = content.strip_suffix(":b") {
                            format!("{}", name.blue())
                        } else {
                            format!("{}", content.blue())
                        }
                    }
                    _ => unreachable!(),
                };
                result.push_str(&formatted);
                segment_start = j + 1;
                found_closing = true;
                break;
            }
            if inner_ch == '\n' || inner_ch == ch {
                break;
            }
        }

        if !found_closing {
            result.push_str(&text[i..i + 1]);
            segment_start = i + 1;
        }
    }

    if segment_start < text.len() {
        result.push_str(&text[segment_start..]);
    }

    result
}

pub fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

pub fn print_help(text: &str) {
    println!();
    println!("{}", format_help_text(text, use_color()));
}

#[macro_export]
macro_rules! error {
    ($msg:literal, $reason:expr) => {{
        let msg = $crate::output::capitalize_first($msg);
        let reason = $reason;
        if $crate::output::use_color() {
            use owo_colors::OwoColorize;
            let formatted_msg = $crate::output::format_backticks(&msg, true);
            let formatted_reason = $crate::output::format_backticks(&reason, true);
            eprintln!();
            eprintln!("{} {}", " ERROR ".black().on_red().bold(), formatted_msg);
            eprintln!(" · reason: {}", formatted_reason);
        } else {
            eprintln!();
            eprintln!("ERROR: {}", msg);
            eprintln!(" · reason: {}", reason);
        }
    }};
}

#[macro_export]
macro_rules! cli_error {
    ($msg:literal, $reason:literal, $hint:literal) => {{
        let msg = $crate::output::capitalize_first($msg);
        if $crate::output::use_color() {
            use owo_colors::OwoColorize;
            let formatted_msg = $crate::output::format_backticks(&msg, true);
            let formatted_reason = $crate::output::format_backticks($reason, true);
            let formatted_hint = $crate::output::format_backticks($hint, true);
            eprintln!();
            eprintln!("{} {}", " ERROR ".black().on_red().bold(), formatted_msg);
            eprintln!(" · reason: {}", formatted_reason);
            eprintln!(" · help: {}", formatted_hint);
        } else {
            eprintln!();
            eprintln!("ERROR: {}", msg);
            eprintln!(" · reason: {}", $reason);
            eprintln!(" · help: {}", $hint);
        }
    }};
    ($msg:expr, $reason:expr, $hint:literal) => {{
        let msg = $crate::output::capitalize_first(&$msg);
        let reason = $reason;
        if $crate::output::use_color() {
            use owo_colors::OwoColorize;
            let formatted_msg = $crate::output::format_backticks(&msg, true);
            let formatted_reason = $crate::output::format_backticks(&reason, true);
            let formatted_hint = $crate::output::format_backticks($hint, true);
            eprintln!();
            eprintln!("{} {}", " ERROR ".black().on_red().bold(), formatted_msg);
            eprintln!(" · reason: {}", formatted_reason);
            eprintln!(" · help: {}", formatted_hint);
        } else {
            eprintln!();
            eprintln!("ERROR: {}", msg);
            eprintln!(" · reason: {}", reason);
            eprintln!(" · help: {}", $hint);
        }
    }};
    ($msg:expr, $reason:expr, $hint:expr) => {{
        let msg = $crate::output::capitalize_first(&$msg);
        let reason = $reason;
        let hint = $hint;
        if $crate::output::use_color() {
            use owo_colors::OwoColorize;
            let formatted_msg = $crate::output::format_backticks(&msg, true);
            let formatted_reason = $crate::output::format_backticks(&reason, true);
            let formatted_hint = $crate::output::format_backticks(&hint, true);
            eprintln!();
            eprintln!("{} {}", " ERROR ".black().on_red().bold(), formatted_msg);
            eprintln!(" · reason: {}", formatted_reason);
            eprintln!(" · help: {}", formatted_hint);
        } else {
            eprintln!();
            eprintln!("ERROR: {}", msg);
            eprintln!(" · reason: {}", reason);
            eprintln!(" · help: {}", hint);
        }
    }};
}
