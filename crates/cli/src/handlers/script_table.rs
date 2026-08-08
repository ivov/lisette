use std::ops::Range;
use std::path::Path;

pub(crate) struct ScriptTable {
    pub(crate) document: deps::DocumentMut,
    range: Range<usize>,
    existed: bool,
    newline: &'static str,
}

impl ScriptTable {
    pub(crate) fn read(source: &str) -> Result<Self, String> {
        let mut blocks = syntax::dependency_block::scan_dependency_blocks(source, 0);
        if blocks.len() > 1 {
            return Err(format!(
                "This script has {} `[dependencies.go]` blocks. Merge them into one",
                blocks.len()
            ));
        }

        let newline = if source.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };

        let Some(block) = blocks.pop() else {
            return Ok(Self {
                document: deps::DocumentMut::new(),
                range: {
                    let at = syntax::dependency_block::insertion_point(source);
                    at..at
                },
                existed: false,
                newline,
            });
        };

        let table = deps::parse_dependency_table(&block.text)
            .map_err(|error| format!("Invalid dependency table: {}", error.message))?;
        for (module_path, dep) in &table.deps {
            deps::validate_script_entry(module_path, dep)?;
        }

        let start = block.span.byte_offset as usize;
        Ok(Self {
            document: table.into_document(),
            range: start..start + block.span.byte_length as usize,
            existed: true,
            newline,
        })
    }

    pub(crate) fn deps(&self) -> std::collections::BTreeMap<String, deps::GoDependency> {
        deps::go_deps_of_document(&self.document).unwrap_or_default()
    }

    pub(crate) fn write(&self, source: &str) -> String {
        let rendered = self.document.to_string();
        let body = if self.deps().is_empty() {
            ""
        } else {
            rendered.trim_end()
        };
        let mut end = self.range.end;
        if body.is_empty() {
            end += line_break_len(&source[end..]);
        }

        let mut out = String::with_capacity(source.len() + body.len());
        let before = &source[..self.range.start];

        if self.existed || body.is_empty() {
            out.push_str(before);
        } else {
            let trimmed = before.trim_end_matches(['\r', '\n']);
            out.push_str(trimmed);
            if !trimmed.is_empty() {
                out.push_str(self.newline);
                if last_line(trimmed).starts_with("#!") {
                    out.push_str(self.newline);
                }
            }
        }

        if !body.is_empty() {
            for line in body.lines() {
                if line.is_empty() {
                    out.push_str("//");
                } else {
                    out.push_str("// ");
                    out.push_str(line);
                }
                out.push_str(self.newline);
            }
            let rest = &source[end..];
            if !self.existed && !rest.is_empty() && line_break_len(rest) == 0 {
                out.push_str(self.newline);
            }
        }
        out.push_str(&source[end..]);
        out
    }

    pub(crate) fn save(&self, file: &Path, source: &str) -> Result<(), String> {
        std::fs::write(file, self.write(source))
            .map_err(|e| format!("Failed to write `{}`: {}", file.display(), e))
    }
}

fn last_line(text: &str) -> &str {
    text.rsplit('\n')
        .next()
        .unwrap_or(text)
        .trim_end_matches('\r')
}

fn line_break_len(rest: &str) -> usize {
    if rest.starts_with("\r\n") {
        2
    } else {
        usize::from(rest.starts_with('\n'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(version: &str) -> deps::GoDependency {
        deps::GoDependency::Remote {
            version: version.to_string(),
            via: None,
        }
    }

    fn upsert(table: &mut ScriptTable, module_path: &str, dep: &deps::GoDependency) {
        deps::upsert_into_document(&mut table.document, module_path, dep).unwrap();
    }

    #[test]
    fn creates_a_block_after_a_shebang() {
        let source = "#!/usr/bin/env lis\n\nimport \"go:fmt\"\n\nfn main() {}\n";
        let mut table = ScriptTable::read(source).unwrap();
        upsert(&mut table, "github.com/google/uuid", &remote("v1.6.0"));

        assert_eq!(
            table.write(source),
            "#!/usr/bin/env lis\n\n// [dependencies.go]\n// \"github.com/google/uuid\" = \"v1.6.0\"\n\nimport \"go:fmt\"\n\nfn main() {}\n"
        );
    }

    #[test]
    fn creates_a_block_in_a_file_with_no_shebang() {
        let source = "import \"go:fmt\"\n";
        let mut table = ScriptTable::read(source).unwrap();
        upsert(&mut table, "x.y/z", &remote("v1.0.0"));

        assert!(
            table
                .write(source)
                .starts_with("// [dependencies.go]\n// \"x.y/z\" = \"v1.0.0\"\n\nimport")
        );
    }

    #[test]
    fn a_new_block_does_not_double_the_blank_line_after_it() {
        let source = "//! A script.\n\nimport \"go:fmt\"\n";
        let mut table = ScriptTable::read(source).unwrap();
        upsert(&mut table, "x.y/z", &remote("v1.0.0"));

        assert_eq!(
            table.write(source),
            "//! A script.\n// [dependencies.go]\n// \"x.y/z\" = \"v1.0.0\"\n\nimport \"go:fmt\"\n"
        );
    }

    #[test]
    fn a_block_goes_below_file_doc_comments() {
        let source = "//! A script.\nimport \"go:fmt\"\n";
        let mut table = ScriptTable::read(source).unwrap();
        upsert(&mut table, "x.y/z", &remote("v1.0.0"));

        assert!(
            table
                .write(source)
                .starts_with("//! A script.\n// [dependencies.go]")
        );
    }

    #[test]
    fn adding_to_an_existing_block_leaves_the_rest_alone() {
        let source = "// [dependencies.go]\n// \"a.b/c\" = \"v1.0.0\"\n\nimport \"go:fmt\"\n";
        let mut table = ScriptTable::read(source).unwrap();
        upsert(&mut table, "d.e/f", &remote("v2.0.0"));
        let written = table.write(source);

        assert!(written.contains("// \"a.b/c\" = \"v1.0.0\""));
        assert!(written.contains("// \"d.e/f\" = \"v2.0.0\""));
        assert!(written.ends_with("\nimport \"go:fmt\"\n"));
    }

    #[test]
    fn removing_the_last_entry_removes_the_block() {
        let source = "// [dependencies.go]\n// \"a.b/c\" = \"v1.0.0\"\n\nimport \"go:fmt\"\n";
        let mut table = ScriptTable::read(source).unwrap();
        deps::remove_from_document(&mut table.document, "a.b/c");

        assert_eq!(table.write(source), "import \"go:fmt\"\n");
    }

    #[test]
    fn removing_a_block_below_a_shebang_leaves_one_blank_line() {
        let source = "#!/usr/bin/env lis\n\n// [dependencies.go]\n// \"a.b/c\" = \"v1.0.0\"\n\nimport \"go:fmt\"\n";
        let mut table = ScriptTable::read(source).unwrap();
        deps::remove_from_document(&mut table.document, "a.b/c");

        assert_eq!(
            table.write(source),
            "#!/usr/bin/env lis\n\nimport \"go:fmt\"\n"
        );
    }

    #[test]
    fn a_via_entry_round_trips() {
        let source =
            "// [dependencies.go]\n// \"a.b/c\" = { version = \"v1.0.0\", via = [\"d.e/f\"] }\n";
        let table = ScriptTable::read(source).unwrap();

        assert_eq!(
            table.deps()["a.b/c"].via(),
            Some(["d.e/f".to_string()].as_slice())
        );
        assert!(table.write(source).contains("via = [\"d.e/f\"]"));
    }

    #[test]
    fn a_prologue_with_no_trailing_newline_still_gets_its_own_line() {
        for (source, expected) in [
            (
                "#!/usr/bin/env lis",
                "#!/usr/bin/env lis\n\n// [dependencies.go]\n// \"x.y/z\" = \"v1.0.0\"\n",
            ),
            (
                "#!/usr/bin/env lis\n",
                "#!/usr/bin/env lis\n\n// [dependencies.go]\n// \"x.y/z\" = \"v1.0.0\"\n",
            ),
            (
                "//! A script.",
                "//! A script.\n// [dependencies.go]\n// \"x.y/z\" = \"v1.0.0\"\n",
            ),
        ] {
            let mut table = ScriptTable::read(source).unwrap();
            upsert(&mut table, "x.y/z", &remote("v1.0.0"));

            assert_eq!(table.write(source), expected, "from {source:?}");
        }
    }

    #[test]
    fn a_crlf_script_keeps_its_line_endings() {
        let source = "#!/usr/bin/env lis\r\n\r\nimport \"go:fmt\"\r\n";
        let mut table = ScriptTable::read(source).unwrap();
        upsert(&mut table, "x.y/z", &remote("v1.0.0"));
        let written = table.write(source);

        assert!(written.contains("\r\n// [dependencies.go]\r\n// \"x.y/z\" = \"v1.0.0\"\r\n"));
        assert!(!written.replace("\r\n", "").contains('\n'), "{written:?}");
    }

    #[test]
    fn removing_a_crlf_block_takes_the_whole_line_break() {
        let source =
            "// [dependencies.go]\r\n// \"a.b/c\" = \"v1.0.0\"\r\n\r\nimport \"go:fmt\"\r\n";
        let mut table = ScriptTable::read(source).unwrap();
        deps::remove_from_document(&mut table.document, "a.b/c");

        assert_eq!(table.write(source), "import \"go:fmt\"\r\n");
    }

    fn rejection(source: &str) -> String {
        ScriptTable::read(source)
            .err()
            .unwrap_or_else(|| panic!("expected `{source}` to be rejected"))
    }

    #[test]
    fn a_table_the_compiler_would_reject_is_not_opened_for_editing() {
        for (source, expected) in [
            (
                "// [dependencies.go]\n// \"a.b/c\" = \"v1.0.0\"\n\n// [dependencies.go]\n// \"d.e/f\" = \"v2.0.0\"\n",
                "2 `[dependencies.go]` blocks",
            ),
            ("// [dependencies.go]\n// \"a.b/c\" = \"latest\"\n", "exact"),
            (
                "// [dependencies.go]\n// \"a.b/c\" = { path = \"../local\" }\n",
                "replace",
            ),
            (
                "// [dependencies.go]\n// \"fmt\" = \"v1.0.0\"\n",
                "module path",
            ),
        ] {
            let error = rejection(source);
            assert!(error.contains(expected), "{source} gave: {error}");
        }
    }

    #[test]
    fn an_edited_row_keeps_its_comment_prefix() {
        let source = "// [dependencies.go]\n// \"a.b/c\" = \"v1.0.0\"\n";
        let mut table = ScriptTable::read(source).unwrap();
        upsert(&mut table, "a.b/c", &remote("v2.0.0"));
        let written = table.write(source);

        assert_eq!(written, "// [dependencies.go]\n// \"a.b/c\" = \"v2.0.0\"\n");
    }
}
