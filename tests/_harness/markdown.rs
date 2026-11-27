pub fn backticks_around(text: &str, minimum: usize) -> String {
    let longest = text
        .split(|character| character != '`')
        .map(str::len)
        .max()
        .unwrap_or(0);
    "`".repeat(minimum.max(longest + 1))
}

pub fn code_span(text: &str) -> String {
    let fence = backticks_around(text, 1);
    if text.starts_with('`') || text.ends_with('`') || fence.len() > 1 {
        format!("{fence} {text} {fence}")
    } else {
        format!("{fence}{text}{fence}")
    }
}
