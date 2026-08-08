use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use rustc_hash::FxHashSet as HashSet;

use deps::{ImportablePackage, TypedefLocator};
use syntax::ast::Expression;
use syntax::program::{File, FileImport, go_import_default_name};

use crate::position::LineIndex;
use crate::protocol::*;
use crate::snapshot::AnalysisSnapshot;

const UNIMPORTED_SORT_PREFIX: &str = "~";

/// Typedefs also appear with no manifest edit behind them, from `lis sync` or
/// from bindgen filling a cache miss, and no cheap key sees that.
const REFRESH_AFTER: Duration = Duration::from_secs(2);

pub(crate) struct Importable {
    pub(crate) name: String,
    package: ImportablePackage,
}

#[derive(Clone, Copy)]
pub(crate) struct EditTarget {
    pub(crate) insert: Range,
    pub(crate) replace: Range,
    pub(crate) insert_replace_support: bool,
}

pub(crate) struct PackageIndex {
    discovered: Mutex<Option<Discovered>>,
    refresh_after: Duration,
}

impl Default for PackageIndex {
    fn default() -> Self {
        Self {
            discovered: Mutex::default(),
            refresh_after: REFRESH_AFTER,
        }
    }
}

#[cfg(test)]
impl PackageIndex {
    fn refreshing_always() -> Self {
        Self {
            refresh_after: Duration::ZERO,
            ..Default::default()
        }
    }
}

struct Discovered {
    root: Option<PathBuf>,
    stamp: String,
    walked: Instant,
    packages: Arc<Vec<Importable>>,
}

pub(crate) struct PackageResolver {
    locator: TypedefLocator,
    index: Arc<PackageIndex>,
}

impl PackageResolver {
    pub(crate) fn new(locator: TypedefLocator, index: Arc<PackageIndex>) -> Self {
        Self { locator, index }
    }

    pub(crate) fn packages(&self) -> Arc<Vec<Importable>> {
        let root = self.locator.project_root();
        let stamp = self.locator.declared_stamp();

        let mut discovered = self
            .index
            .discovered
            .lock()
            .unwrap_or_else(PoisonError::into_inner);

        if let Some(current) = discovered.as_ref()
            && current.root.as_deref() == root
            && current.stamp == stamp
            && current.walked.elapsed() < self.index.refresh_after
        {
            return Arc::clone(&current.packages);
        }

        let locator = root
            .and_then(|root| TypedefLocator::from_project(root).ok())
            .unwrap_or_else(|| self.locator.with_fresh_local_stamp());

        let packages: Arc<Vec<Importable>> = Arc::new(
            locator
                .importable_packages()
                .into_iter()
                .map(|package| {
                    let name = package
                        .declared_package_name()
                        .unwrap_or_else(|| go_import_default_name(&package.path).to_string());
                    Importable { name, package }
                })
                .collect(),
        );

        *discovered = Some(Discovered {
            root: root.map(std::path::Path::to_path_buf),
            stamp,
            walked: Instant::now(),
            packages: Arc::clone(&packages),
        });

        packages
    }
}

pub(crate) fn not_yet_imported<'a>(
    packages: &'a [Importable],
    file: &File,
    snapshot: &AnalysisSnapshot,
) -> Vec<&'a Importable> {
    let imports = file.imports();
    let go_package_names = &snapshot.analysis.emit_input.go_package_names;

    let bound: HashSet<String> = imports
        .iter()
        .filter_map(|import| import.effective_alias(go_package_names))
        .collect();
    let imported: HashSet<&str> = imports
        .iter()
        .filter_map(|import| import.name.as_str().strip_prefix("go:"))
        .collect();

    packages
        .iter()
        .filter(|importable| {
            !imported.contains(importable.package.path.as_str())
                && !bound.contains(&importable.name)
        })
        .collect()
}

pub(crate) fn member_completions(
    importable: &Importable,
    file: &File,
    line_index: &LineIndex,
    target: Option<EditTarget>,
) -> Vec<CompletionItem> {
    let Some(source) = importable.package.typedef_source() else {
        return Vec::new();
    };
    let edit = ImportSite::new(file, line_index).edit(&importable.package.path);
    let qualifier = format!(" · go:{}", importable.package.path);

    syntax::build_ast(&source, 0)
        .ast
        .iter()
        .filter_map(|item| {
            let (name, kind) = public_member(item)?;
            Some(with_import(
                CompletionItem {
                    label: name,
                    kind: Some(kind),
                    detail: Some(signature(item, &source) + &qualifier),
                    ..Default::default()
                },
                &edit,
                target,
            ))
        })
        .collect()
}

pub(crate) fn package_completions(
    packages: &[&Importable],
    file: &File,
    line_index: &LineIndex,
    target: Option<EditTarget>,
) -> Vec<CompletionItem> {
    let site = ImportSite::new(file, line_index);
    packages
        .iter()
        .map(|importable| {
            with_import(
                CompletionItem {
                    label: importable.name.clone(),
                    kind: Some(CompletionItemKind::MODULE),
                    detail: Some(format!("go:{}", importable.package.path)),
                    sort_text: Some(format!("{UNIMPORTED_SORT_PREFIX}{}", importable.name)),
                    ..Default::default()
                },
                &site.edit(&importable.package.path),
                target,
            )
        })
        .collect()
}

fn with_import(
    item: CompletionItem,
    import: &TextEdit,
    target: Option<EditTarget>,
) -> CompletionItem {
    let Some(target) = target else {
        return CompletionItem {
            additional_text_edits: Some(vec![import.clone()]),
            ..item
        };
    };

    if crate::ranges_overlap(import.range, target.replace) {
        let combined = TextEdit {
            range: target.replace,
            new_text: format!("{}{}", import.new_text, item.label),
        };
        return CompletionItem {
            text_edit: serde_json::to_value(combined).ok(),
            ..item
        };
    }

    let text_edit = if target.insert_replace_support {
        serde_json::to_value(InsertReplaceEdit {
            new_text: item.label.clone(),
            insert: target.insert,
            replace: target.replace,
        })
    } else {
        serde_json::to_value(TextEdit {
            range: target.replace,
            new_text: item.label.clone(),
        })
    };

    CompletionItem {
        text_edit: text_edit.ok(),
        additional_text_edits: Some(vec![import.clone()]),
        ..item
    }
}

struct ImportSite<'a> {
    imports: Vec<FileImport>,
    source: &'a str,
    line_index: &'a LineIndex,
}

impl<'a> ImportSite<'a> {
    fn new(file: &'a File, line_index: &'a LineIndex) -> Self {
        Self {
            imports: file.imports(),
            source: &file.source,
            line_index,
        }
    }

    fn edit(&self, path: &str) -> TextEdit {
        let statement = format!("import \"go:{path}\"");

        let mut go_imports = self
            .imports
            .iter()
            .filter(|import| import.name.starts_with("go:"))
            .peekable();

        if go_imports.peek().is_some() {
            let mut last_offset = 0;
            for import in go_imports {
                let key = format::import_sort_key(&import.name, import.alias.as_ref());
                if key > path {
                    return self.insertion(
                        block_start(self.source, import.span.byte_offset, import_comment),
                        &statement,
                        false,
                    );
                }
                last_offset = import.span.byte_offset;
            }
            return self.insertion(next_line_start(self.source, last_offset), &statement, false);
        }

        match self.imports.first() {
            Some(import) => self.insertion(
                block_start(self.source, import.span.byte_offset, import_comment),
                &statement,
                true,
            ),
            None => self.insertion(header_end(self.source), &statement, true),
        }
    }

    fn insertion(&self, offset: u32, statement: &str, blank_line_after: bool) -> TextEdit {
        let (before, after) = self.source.split_at(offset as usize);
        let mut new_text = String::new();
        if !before.is_empty() && !before.ends_with('\n') {
            new_text.push('\n');
        }
        new_text.push_str(statement);
        new_text.push('\n');
        if blank_line_after && !after.trim().is_empty() {
            new_text.push('\n');
        }

        let position = self.line_index.offset_to_position(offset);
        TextEdit {
            range: Range {
                start: position,
                end: position,
            },
            new_text,
        }
    }
}

/// Below the shebang and any detached comment block, above the first
/// declaration's own.
fn header_end(source: &str) -> u32 {
    let mut offset = 0;
    for line in source.split_inclusive('\n') {
        let trimmed = line.trim();
        if !trimmed.is_empty() && !trimmed.starts_with("//") && !trimmed.starts_with("#!") {
            return block_start(source, offset as u32, declaration_comment).max(table_end(source));
        }
        offset += line.len();
    }
    offset as u32
}

fn table_end(source: &str) -> u32 {
    syntax::dependency_block::scan_dependency_blocks(source, 0)
        .iter()
        .map(|block| block.span.byte_offset + block.span.byte_length)
        .max()
        .unwrap_or(0)
}

/// A blank line ends the block, and a shebang or file comment is never crossed.
fn block_start(source: &str, offset: u32, attached: fn(&str) -> bool) -> u32 {
    let mut start = line_start(source, offset);
    while start > 0 {
        let previous = line_start(source, start - 1);
        if !attached(source[previous as usize..start as usize].trim()) {
            break;
        }
        start = previous;
    }
    start
}

fn import_comment(line: &str) -> bool {
    line.starts_with("//") && !line.starts_with("///") && !line.starts_with("//!")
}

fn declaration_comment(line: &str) -> bool {
    line.starts_with("//") && !line.starts_with("//!")
}

fn line_start(source: &str, offset: u32) -> u32 {
    source[..offset as usize]
        .rfind('\n')
        .map_or(0, |index| index as u32 + 1)
}

fn next_line_start(source: &str, offset: u32) -> u32 {
    source[offset as usize..]
        .find('\n')
        .map_or(source.len() as u32, |index| offset + index as u32 + 1)
}

fn public_member(item: &Expression) -> Option<(String, CompletionItemKind)> {
    let (name, visibility, kind) = match item {
        Expression::Function {
            name, visibility, ..
        } => (name, visibility, CompletionItemKind::FUNCTION),
        Expression::Struct {
            name, visibility, ..
        } => (name, visibility, CompletionItemKind::STRUCT),
        Expression::Enum {
            name, visibility, ..
        } => (name, visibility, CompletionItemKind::ENUM),
        Expression::Interface {
            name, visibility, ..
        } => (name, visibility, CompletionItemKind::INTERFACE),
        Expression::TypeAlias {
            name, visibility, ..
        } => (name, visibility, CompletionItemKind::TYPE_PARAMETER),
        Expression::VariableDeclaration {
            name, visibility, ..
        } => (name, visibility, CompletionItemKind::VARIABLE),
        Expression::Const {
            identifier,
            visibility,
            ..
        } => (identifier, visibility, CompletionItemKind::CONSTANT),
        _ => return None,
    };
    visibility.is_public().then(|| (name.to_string(), kind))
}

fn signature(item: &Expression, source: &str) -> String {
    let span = item.get_span();
    let start = span.byte_offset as usize;
    let text = &source[start..start + span.byte_length as usize];
    let text = text.split_once('{').map_or(text, |(head, _)| head);

    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn applied(path: &str, source: &str) -> String {
        let mut file = File::new_cached("root", "main.lis", "main.lis", source, 0);
        file.items = syntax::build_ast(source, 0).ast;
        let line_index = LineIndex::new(source);

        let edit = ImportSite::new(&file, &line_index).edit(path);
        let offset = line_index
            .position_to_offset(edit.range.start)
            .expect("edit position should be in the source") as usize;

        let mut result = source[..offset].to_string();
        result.push_str(&edit.new_text);
        result.push_str(&source[offset..]);
        result
    }

    fn with_import(path: &str, source: &str) -> String {
        let result = applied(path, source);
        let formatted = format::format_source(&result).expect("result should parse");
        assert_eq!(
            formatted, result,
            "the formatter should leave the inserted import where it is"
        );
        result
    }

    #[test]
    fn a_file_without_imports_takes_the_import_at_the_top() {
        assert_eq!(
            with_import("strings", "fn main() {\n  let name = \"lis\"\n}\n"),
            "import \"go:strings\"\n\nfn main() {\n  let name = \"lis\"\n}\n"
        );
    }

    #[test]
    fn an_empty_file_takes_the_import_without_a_trailing_blank_line() {
        assert_eq!(with_import("strings", ""), "import \"go:strings\"\n");
    }

    #[test]
    fn the_import_goes_below_the_dependency_table() {
        assert_eq!(
            with_import(
                "strings",
                "// [dependencies.go]\n// \"x.y/z\" = \"v1.0.0\"\n\nfn main() {}\n"
            ),
            "// [dependencies.go]\n// \"x.y/z\" = \"v1.0.0\"\n\nimport \"go:strings\"\n\nfn main() {}\n"
        );
    }

    #[test]
    fn the_import_goes_below_a_dependency_table_with_no_blank_line() {
        assert_eq!(
            with_import(
                "strings",
                "// [dependencies.go]\n// \"x.y/z\" = \"v1.0.0\"\nfn main() {}\n"
            ),
            "// [dependencies.go]\n// \"x.y/z\" = \"v1.0.0\"\nimport \"go:strings\"\n\nfn main() {}\n"
        );
    }

    #[test]
    fn the_import_goes_below_the_shebang_and_file_comments() {
        assert_eq!(
            with_import(
                "strings",
                "#!/usr/bin/env lis\n\n//! Tools.\n\nfn main() {}\n"
            ),
            "#!/usr/bin/env lis\n\n//! Tools.\n\nimport \"go:strings\"\n\nfn main() {}\n"
        );
    }

    #[test]
    fn the_import_goes_above_a_doc_comment() {
        assert_eq!(
            with_import("strings", "/// Entry point.\nfn main() {}\n"),
            "import \"go:strings\"\n\n/// Entry point.\nfn main() {}\n"
        );
    }

    #[test]
    fn a_comment_on_the_first_declaration_stays_with_it() {
        assert_eq!(
            with_import("strings", "// What main does.\nfn main() {}\n"),
            "import \"go:strings\"\n\n// What main does.\nfn main() {}\n"
        );
    }

    #[test]
    fn a_comment_the_blank_line_detaches_keeps_the_top_of_the_file() {
        assert_eq!(
            with_import(
                "strings",
                "// Copyright 2026 the authors.\n\nfn main() {}\n"
            ),
            "// Copyright 2026 the authors.\n\nimport \"go:strings\"\n\nfn main() {}\n"
        );
    }

    #[test]
    fn the_import_joins_the_go_group_in_sorted_order() {
        let source = "import \"go:fmt\"\nimport \"go:os\"\n\nfn main() {}\n";
        assert_eq!(
            with_import("errors", source),
            "import \"go:errors\"\nimport \"go:fmt\"\nimport \"go:os\"\n\nfn main() {}\n"
        );
        assert_eq!(
            with_import("io", source),
            "import \"go:fmt\"\nimport \"go:io\"\nimport \"go:os\"\n\nfn main() {}\n"
        );
        assert_eq!(
            with_import("strings", source),
            "import \"go:fmt\"\nimport \"go:os\"\nimport \"go:strings\"\n\nfn main() {}\n"
        );
    }

    #[test]
    fn the_import_does_not_take_over_the_next_import_s_comment() {
        assert_eq!(
            with_import(
                "io",
                "import \"go:fmt\"\n// the OS bits\nimport \"go:os\"\n\nfn main() {}\n"
            ),
            "import \"go:fmt\"\nimport \"go:io\"\n// the OS bits\nimport \"go:os\"\n\nfn main() {}\n"
        );
    }

    #[test]
    fn a_comment_the_blank_line_detaches_stays_above_the_block() {
        assert_eq!(
            with_import(
                "errors",
                "// What this file is.\n\nimport \"go:fmt\"\n\nfn main() {}\n"
            ),
            "// What this file is.\n\nimport \"go:errors\"\nimport \"go:fmt\"\n\nfn main() {}\n"
        );
    }

    #[test]
    fn a_file_comment_is_never_displaced() {
        assert_eq!(
            applied("errors", "//! Tools.\nimport \"go:fmt\"\n\nfn main() {}\n"),
            "//! Tools.\nimport \"go:errors\"\nimport \"go:fmt\"\n\nfn main() {}\n"
        );
    }

    #[test]
    fn an_alias_sorts_the_import_by_the_alias_as_the_formatter_does() {
        assert_eq!(
            with_import("os", "import zzz \"go:fmt\"\n\nfn main() {}\n"),
            "import \"go:os\"\nimport zzz \"go:fmt\"\n\nfn main() {}\n"
        );
    }

    #[test]
    fn the_go_group_opens_above_project_imports() {
        assert_eq!(
            with_import("strings", "import \"handlers\"\n\nfn main() {}\n"),
            "import \"go:strings\"\n\nimport \"handlers\"\n\nfn main() {}\n"
        );
    }

    #[test]
    fn a_file_without_a_final_newline_still_gets_a_line_of_its_own() {
        assert_eq!(
            with_import("os", "import \"go:fmt\""),
            "import \"go:fmt\"\nimport \"go:os\"\n"
        );
    }

    fn declare(root: &std::path::Path, modules: &[&str]) {
        let entries: String = modules
            .iter()
            .map(|module| format!("\"{module}\" = \"v1.0.0\"\n"))
            .collect();
        std::fs::write(
            root.join("lisette.toml"),
            format!(
                "[project]\nname = \"t\"\nversion = \"0.0.1\"\n\n[toolchain]\nlis = \"{}\"\n\n[dependencies.go]\n{entries}",
                env!("CARGO_PKG_VERSION")
            ),
        )
        .unwrap();

        for module in modules {
            let dir = deps::typedef_cache_dir(root)
                .join(deps::Target::host().cache_segment())
                .join(format!("{module}@v1.0.0"));
            std::fs::create_dir_all(&dir).unwrap();
            let name = module.rsplit('/').next().unwrap();
            std::fs::write(dir.join(format!("{name}.d.lis")), "pub fn Do() -> int\n").unwrap();
        }
    }

    fn resolver(root: &std::path::Path, index: &Arc<PackageIndex>) -> PackageResolver {
        PackageResolver::new(
            TypedefLocator::from_project(root).expect("the manifest should parse"),
            Arc::clone(index),
        )
    }

    fn paths(packages: &[Importable]) -> Vec<&str> {
        packages
            .iter()
            .map(|importable| importable.package.path.as_str())
            .filter(|path| path.starts_with("example.com/"))
            .collect()
    }

    #[test]
    fn discovery_is_shared_between_analyses_and_redone_for_another_project() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        declare(a.path(), &["example.com/one"]);
        declare(b.path(), &["example.com/one"]);
        let index = Arc::new(PackageIndex::default());

        let once = resolver(a.path(), &index).packages();
        let twice = resolver(a.path(), &index).packages();
        assert!(
            Arc::ptr_eq(&once, &twice),
            "the next analysis reuses the walk instead of hitting disk again"
        );

        let other = resolver(b.path(), &index).packages();
        assert!(
            !Arc::ptr_eq(&once, &other),
            "another project has its own dependencies to discover"
        );
    }

    #[test]
    fn a_newly_declared_dependency_shows_up_without_waiting_for_the_refresh() {
        let project = tempfile::tempdir().unwrap();
        let index = Arc::new(PackageIndex::default());

        declare(project.path(), &["example.com/one"]);
        let before = resolver(project.path(), &index).packages();
        assert_eq!(paths(&before), ["example.com/one"]);

        declare(project.path(), &["example.com/one", "example.com/two"]);
        let after = resolver(project.path(), &index).packages();
        assert_eq!(
            paths(&after),
            ["example.com/one", "example.com/two"],
            "this analysis starts from different declarations, so it walks again"
        );
    }

    #[test]
    fn a_refresh_rereads_the_manifest_that_no_edit_told_the_server_about() {
        let project = tempfile::tempdir().unwrap();
        let index = Arc::new(PackageIndex::refreshing_always());

        declare(project.path(), &["example.com/one"]);
        let analysis = resolver(project.path(), &index);
        assert_eq!(paths(&analysis.packages()), ["example.com/one"]);

        declare(project.path(), &["example.com/one", "example.com/two"]);

        assert_eq!(
            paths(&analysis.packages()),
            ["example.com/one", "example.com/two"],
            "the walk reads the project off disk, not what this analysis captured"
        );
    }

    #[test]
    fn a_signature_is_one_line_and_stops_before_a_body() {
        let source = "pub fn Verify(\n  hash: Slice<byte>,\n  sig: Slice<byte>,\n) -> bool\n\npub struct Client {\n  pub Timeout: int,\n}\n";
        let items = syntax::build_ast(source, 0).ast;

        assert_eq!(
            signature(&items[0], source),
            "fn Verify( hash: Slice<byte>, sig: Slice<byte>, ) -> bool"
        );
        assert_eq!(signature(&items[1], source), "struct Client");
    }
}
