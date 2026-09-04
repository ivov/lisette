pub fn is_callout_line(line: &str) -> bool {
    line.trim_start()
        .strip_prefix("//")
        .is_some_and(|body| body.trim_start().starts_with("!callout"))
}

pub fn drop_callout_lines(text: &str) -> String {
    text.lines()
        .filter(|line| !is_callout_line(line))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn split_example(doc: &str) -> (&str, Option<&str>) {
    match doc.find("\nExample:\n") {
        Some(position) => {
            let prose = doc[..position].trim_end();
            let example = doc[position + "\nExample:\n".len()..].trim_end();
            (prose, Some(example))
        }
        None => (doc, None),
    }
}

pub fn dedent(text: &str) -> String {
    let indent = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);
    text.lines()
        .map(|line| line.get(indent..).unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Callout lines dropped, the example fenced.
pub fn to_markdown(doc: &str) -> String {
    let text = drop_callout_lines(doc);
    let (prose, example) = split_example(&text);
    match example {
        Some(example) => format!(
            "{prose}\n\nExample:\n\n```lisette\n{}\n```",
            dedent(example)
        ),
        None => prose.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BUFFERED: &str = "Creates a `Channel` with room for `capacity` values.\n\nExample:\n  // !callout-right room for `4` values before a send waits\n  let jobs = Channel.buffered<int>(4)";

    #[test]
    fn markdown_drops_callouts_and_fences_the_example() {
        assert_eq!(
            to_markdown(BUFFERED),
            "Creates a `Channel` with room for `capacity` values.\n\nExample:\n\n```lisette\nlet jobs = Channel.buffered<int>(4)\n```"
        );
    }

    #[test]
    fn markdown_without_example_is_the_prose() {
        assert_eq!(to_markdown("Returns the length."), "Returns the length.");
    }

    #[test]
    fn split_example_keeps_prose_and_example_apart() {
        let (prose, example) = split_example(BUFFERED);
        assert_eq!(
            prose,
            "Creates a `Channel` with room for `capacity` values."
        );
        assert_eq!(
            example,
            Some(
                "  // !callout-right room for `4` values before a send waits\n  let jobs = Channel.buffered<int>(4)"
            )
        );
    }

    #[test]
    fn dedent_removes_the_shared_indent_only() {
        assert_eq!(dedent("  a\n\n    b"), "a\n\n  b");
    }
}
