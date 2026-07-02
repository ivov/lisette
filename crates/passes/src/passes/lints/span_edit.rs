use syntax::ast::Span;

pub(crate) fn statement_deletion(source: &str, stmt: Span) -> Span {
    line_aware_deletion(source, stmt, b';')
}

pub(crate) fn match_arm_deletion(source: &str, arm: Span) -> Span {
    line_aware_deletion(source, arm, b',')
}

/// Deletes `item` and its trailing `separator`, widening to the whole physical
/// line when `item` is alone on it so no indented blank line is left behind.
fn line_aware_deletion(source: &str, item: Span, separator: u8) -> Span {
    let bytes = source.as_bytes();
    let start = item.byte_offset as usize;
    let end = item.end() as usize;

    let mut after = end;
    while after < bytes.len() && matches!(bytes[after], b' ' | b'\t') {
        after += 1;
    }
    let has_separator = after < bytes.len() && bytes[after] == separator;
    if has_separator {
        after += 1;
    }

    let mut line_tail = after;
    while line_tail < bytes.len() && matches!(bytes[line_tail], b' ' | b'\t') {
        line_tail += 1;
    }
    let trailing_blank = line_tail >= bytes.len() || bytes[line_tail] == b'\n';

    let line_start = source[..start].rfind('\n').map_or(0, |i| i + 1);
    let leading_blank = source[line_start..start]
        .bytes()
        .all(|b| matches!(b, b' ' | b'\t'));

    if leading_blank && trailing_blank {
        let line_end = if line_tail < bytes.len() && bytes[line_tail] == b'\n' {
            line_tail + 1
        } else {
            line_tail
        };
        Span::new(
            item.file_id,
            line_start as u32,
            (line_end - line_start) as u32,
        )
    } else if has_separator {
        Span::new(item.file_id, start as u32, (after - start) as u32)
    } else {
        item
    }
}

/// Deletes `item` with its separating comma: trailing (`item, rest`) if present,
/// else leading (`rest, item`), else the item alone.
pub(crate) fn list_item_deletion(source: &str, item: Span) -> Span {
    let bytes = source.as_bytes();
    let start = item.byte_offset as usize;
    let end = item.end() as usize;

    let mut after = end;
    while after < bytes.len() && matches!(bytes[after], b' ' | b'\t' | b'\n') {
        after += 1;
    }
    if after < bytes.len() && bytes[after] == b',' {
        let mut trail = after + 1;
        while trail < bytes.len() && matches!(bytes[trail], b' ' | b'\t') {
            trail += 1;
        }
        return Span::new(item.file_id, start as u32, (trail - start) as u32);
    }

    let mut before = start;
    while before > 0 && matches!(bytes[before - 1], b' ' | b'\t' | b'\n') {
        before -= 1;
    }
    if before > 0 && bytes[before - 1] == b',' {
        let new_start = before - 1;
        return Span::new(item.file_id, new_start as u32, (end - new_start) as u32);
    }

    item
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(source: &str, needle: &str) -> Span {
        let offset = source.find(needle).unwrap();
        Span::new(0, offset as u32, needle.len() as u32)
    }

    fn deleted(source: &str, span: Span) -> String {
        let mut out = source.to_string();
        out.replace_range(span.byte_offset as usize..span.end() as usize, "");
        out
    }

    #[test]
    fn statement_alone_on_line_removes_the_line() {
        let src = "a\n  let x = x;\n  b\n";
        let span = at(src, "let x = x");
        assert_eq!(deleted(src, statement_deletion(src, span)), "a\n  b\n");
    }

    #[test]
    fn statement_sharing_a_line_keeps_the_rest() {
        let src = "let x = x; foo()\n";
        let span = at(src, "let x = x");
        assert_eq!(deleted(src, statement_deletion(src, span)), " foo()\n");
    }

    #[test]
    fn list_item_consumes_trailing_comma() {
        let src = "f(a, b, c)";
        let span = at(src, "a");
        assert_eq!(deleted(src, list_item_deletion(src, span)), "f(b, c)");
    }

    #[test]
    fn list_item_consumes_leading_comma_for_last() {
        let src = "f(a, b, c)";
        let span = at(src, "c");
        assert_eq!(deleted(src, list_item_deletion(src, span)), "f(a, b)");
    }

    #[test]
    fn match_arm_alone_on_line_removes_the_line() {
        let src = "match n {\n  1 => a,\n  2 => b,\n  3 => c,\n}\n";
        let span = at(src, "2 => b");
        assert_eq!(
            deleted(src, match_arm_deletion(src, span)),
            "match n {\n  1 => a,\n  3 => c,\n}\n"
        );
    }
}
