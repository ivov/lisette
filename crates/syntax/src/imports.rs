//! Reads a file's imports from its prologue, without lexing or parsing.

use ecow::EcoString;

use crate::ast::{ImportAlias, Span};
use crate::program::FileImport;

pub fn scan_imports(source: &str, file_id: u32) -> Vec<FileImport> {
    if source.len() > crate::MAX_SOURCE_BYTES {
        return Vec::new();
    }

    let mut scanner = Scanner {
        source,
        offset: 0,
        file_id,
    };
    let mut imports = Vec::new();
    while let Some(import) = scanner.next_import() {
        imports.push(import);
    }
    imports
}

struct Scanner<'source> {
    source: &'source str,
    offset: usize,
    file_id: u32,
}

impl<'source> Scanner<'source> {
    fn byte(&self) -> Option<u8> {
        self.source.as_bytes().get(self.offset).copied()
    }

    fn rest(&self) -> &'source str {
        &self.source[self.offset..]
    }

    fn byte_at(&self, ahead: usize) -> Option<u8> {
        self.source.as_bytes().get(self.offset + ahead).copied()
    }

    fn skip_line(&mut self) {
        while !matches!(self.byte(), None | Some(b'\n')) {
            self.offset += 1;
        }
    }

    fn skip_trivia(&mut self) {
        loop {
            match self.byte() {
                Some(byte) if byte.is_ascii_whitespace() => self.offset += 1,
                Some(b';') => self.offset += 1,
                Some(b'/') if self.byte_at(1) == Some(b'/') => self.skip_line(),
                _ => return,
            }
        }
    }

    /// `Parser::next` skips plain `//` comments but stops at a doc or file comment.
    fn skip_whitespace_and_comments(&mut self) {
        loop {
            match self.byte() {
                Some(byte) if byte.is_ascii_whitespace() => self.offset += 1,
                Some(b'/')
                    if self.byte_at(1) == Some(b'/')
                        && !matches!(self.byte_at(2), Some(b'/' | b'!')) =>
                {
                    self.skip_line()
                }
                _ => return,
            }
        }
    }

    fn skip_horizontal_whitespace(&mut self) {
        while matches!(self.byte(), Some(b' ' | b'\t')) {
            self.offset += 1;
        }
    }

    fn eat_keyword(&mut self, keyword: &str) -> bool {
        let Some(after) = self.rest().strip_prefix(keyword) else {
            return false;
        };
        if after.chars().next().is_some_and(is_identifier_character) {
            return false;
        }
        self.offset += keyword.len();
        true
    }

    fn next_import(&mut self) -> Option<FileImport> {
        self.skip_trivia();

        let start = self.offset;
        if !self.eat_keyword("import") {
            return None;
        }
        self.skip_whitespace_and_comments();

        let alias = if self.byte()? == b'"' {
            None
        } else {
            let (text, span) = self.scan_identifier()?;
            self.skip_horizontal_whitespace();
            Some(match text {
                "_" => ImportAlias::Blank(span),
                _ => ImportAlias::Named(text.into(), span),
            })
        };

        let path_start = self.offset;
        let name = self.scan_path()?;

        Some(FileImport {
            name,
            name_span: self.span_from(path_start),
            alias,
            span: self.span_from(start),
        })
    }

    fn scan_identifier(&mut self) -> Option<(&'source str, Span)> {
        let start = self.offset;
        if !self
            .rest()
            .chars()
            .next()
            .is_some_and(|first| first.is_alphabetic() || first == '_')
        {
            return None;
        }
        for character in self.rest().chars() {
            if !is_identifier_character(character) {
                break;
            }
            self.offset += character.len_utf8();
        }

        let text = &self.source[start..self.offset];
        // `r"` and `f"` open a literal, not an identifier.
        if matches!(text, "r" | "f" | "rf" | "fr") && self.byte() == Some(b'"') {
            return None;
        }
        Some((text, self.span_from(start)))
    }

    /// The bytes between the quotes, left escaped, as the parser takes them.
    fn scan_path(&mut self) -> Option<EcoString> {
        if self.byte()? != b'"' {
            return None;
        }
        self.offset += 1;

        let content_start = self.offset;
        loop {
            match self.byte()? {
                b'\\' => {
                    self.offset += 1;
                    let escaped = self.rest().chars().next().map_or(0, char::len_utf8);
                    self.offset += escaped;
                }
                b'"' => {
                    let content = &self.source[content_start..self.offset];
                    self.offset += 1;
                    return Some(content.into());
                }
                _ => self.offset += 1,
            }
        }
    }

    fn span_from(&self, start: usize) -> Span {
        Span::new(self.file_id, start as u32, (self.offset - start) as u32)
    }
}

fn is_identifier_character(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build_ast;
    use crate::program::File;

    fn scanned(source: &str) -> Vec<FileImport> {
        scan_imports(source, 7)
    }

    fn parsed(source: &str) -> Vec<FileImport> {
        let result = build_ast(source, 7);
        File {
            id: 7,
            module_id: "m".into(),
            name: "m.lis".into(),
            display_path: "m.lis".into(),
            source_path: None,
            source: source.to_string(),
            items: result.ast,
            file_comment: result.file_comment,
        }
        .imports()
    }

    #[track_caller]
    fn assert_matches_parser(source: &str) {
        assert_eq!(
            scanned(source),
            parsed(source),
            "scanner and parser disagree on: {source:?}"
        );
    }

    #[test]
    fn every_import_form_matches_the_parser() {
        for source in [
            "",
            "\n\n",
            "import \"go:fmt\"",
            "import \"go:fmt\"\nimport \"math\"\nfn f() {}",
            "import _ \"go:fmt\"\nfn f() {}",
            "import alias \"go:fmt\"\nfn f() {}",
            "import alias\"go:fmt\"\nfn f() {}",
            "import_alias \"go:fmt\"",
            "//! header\n//! more\n\nimport \"go:fmt\"\nfn f() {}",
            "// note\nimport \"go:fmt\"\n// between\nimport \"go:os\"\n\n/// docs\nfn f() {}",
            "import \"go:fmt\"; import \"go:os\"\nfn f() {}",
            "import\n\"go:fmt\"\nfn f() {}",
            "import // note\n\"go:fmt\"\nfn f() {}",
            "import // note\nalias \"go:fmt\"\nfn f() {}",
            "import /// doc\n\"go:fmt\"\nfn f() {}",
            "import //! file\n\"go:fmt\"\nfn f() {}",
            "import ; \"go:fmt\"\nfn f() {}",
            "import alias // note\n\"go:fmt\"\nfn f() {}",
            "pub import \"go:fmt\"\nfn f() {}",
            "fn f() {}\n",
            "#[test]\nfn f() {}\n",
            "struct S {}\nimport \"go:fmt\"",
            "import \"esc\\\"aped\"\nfn f() {}",
        ] {
            assert_matches_parser(source);
        }
    }

    /// A parse error discards the whole AST, so the parser reports no imports.
    #[test]
    fn a_broken_body_does_not_hide_the_prologue() {
        let source = "import \"go:fmt\"\nfn f() {\n  import \"go:os\"\n}";

        assert_eq!(
            scanned(source)
                .iter()
                .map(|import| import.name.to_string())
                .collect::<Vec<_>>(),
            ["go:fmt"]
        );
        assert!(parsed(source).is_empty());
    }

    #[test]
    fn malformed_imports_produce_none() {
        for source in [
            "import",
            "import alias",
            "import alias\n\"go:fmt\"",
            "import \"unterminated",
            "import r\"go:fmt\"",
            "import f\"go:fmt\"",
            "import 42",
            "import /// doc\n\"go:fmt\"",
        ] {
            assert_matches_parser(source);
            assert!(
                scanned(source).is_empty(),
                "expected no import for: {source:?}"
            );
        }
    }

    #[test]
    fn spans_cover_the_keyword_and_the_quoted_path() {
        let source = "import alias \"go:fmt\"";
        let import = scanned(source).pop().unwrap();
        let text = |span: Span| &source[span.byte_offset as usize..span.end() as usize];

        assert_eq!(text(import.span), source);
        assert_eq!(text(import.name_span), "\"go:fmt\"");
        assert_eq!(import.name, "go:fmt");
    }

    #[test]
    fn a_multibyte_escape_keeps_the_scan_on_a_character_boundary() {
        let scanned = scanned("import \"\\é\"\nfn f() {}");

        assert_eq!(scanned.len(), 1);
        assert_eq!(scanned[0].name, "\\é");
    }

    #[test]
    fn oversized_sources_scan_as_empty() {
        let mut source = String::from("import \"go:fmt\"\n");
        source.push_str(&"\n".repeat(crate::MAX_SOURCE_BYTES));

        assert!(scan_imports(&source, 0).is_empty());
    }
}
