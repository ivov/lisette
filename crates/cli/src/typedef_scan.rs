use std::path::{Path, PathBuf};

use syntax::ast::{Expression, ImportAlias};
use syntax::parse::Parser;

use lisette::fs::collect_lis_filepaths_recursive;

pub(crate) enum SourceScanError {
    Parse {
        path: PathBuf,
        message: String,
    },
    Read {
        path: PathBuf,
        error: std::io::Error,
    },
}

pub(crate) struct ScannedImports {
    /// All third-party `go:` imports (blank-imports keep modules referenced).
    pub(crate) all: Vec<String>,
    /// Third-party `go:` imports excluding `_`-aliased blank ones.
    pub(crate) non_blank: Vec<String>,
}

/// Collect every third-party `go:` import across `src/**/*.lis`.
pub(crate) fn scan_source_imports(src_dir: &Path) -> Result<ScannedImports, SourceScanError> {
    let mut all = Vec::new();
    let mut non_blank = Vec::new();
    if !src_dir.is_dir() {
        return Ok(ScannedImports { all, non_blank });
    }

    for path in collect_lis_filepaths_recursive(src_dir) {
        let source = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => return Err(SourceScanError::Read { path, error: e }),
        };
        let parse_result = Parser::lex_and_parse_file(&source, 0);
        if parse_result.failed() {
            return Err(SourceScanError::Parse {
                path,
                message: parse_result.errors[0].message.clone(),
            });
        }
        for expr in &parse_result.ast {
            if let Expression::ModuleImport { name, alias, .. } = expr
                && let Some(pkg) = name.strip_prefix("go:")
                && deps::is_third_party(pkg)
            {
                all.push(pkg.to_string());
                if !matches!(alias, Some(ImportAlias::Blank(_))) {
                    non_blank.push(pkg.to_string());
                }
            }
        }
    }

    Ok(ScannedImports { all, non_blank })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project_src(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        for (name, body) in files {
            std::fs::write(src.join(name), body).unwrap();
        }
        dir
    }

    #[test]
    fn scan_reports_parse_error_naming_the_file() {
        let project = project_src(&[("main.lis", "fn broken( {\n")]);

        let Err(SourceScanError::Parse { path, .. }) =
            scan_source_imports(&project.path().join("src"))
        else {
            panic!("expected a parse error");
        };
        assert!(
            path.ends_with("main.lis"),
            "error must name the failing file, got {}",
            path.display()
        );
    }

    #[test]
    fn scan_collects_third_party_imports_and_separates_blank_and_stdlib() {
        let source = r#"import "go:github.com/gorilla/mux"
import _ "go:github.com/gorilla/context"
import "go:fmt"

fn main() {}
"#;
        let project = project_src(&[("main.lis", source)]);

        let Ok(scanned) = scan_source_imports(&project.path().join("src")) else {
            panic!("scan must succeed on valid sources");
        };

        assert_eq!(scanned.non_blank, vec!["github.com/gorilla/mux"]);
        let mut all = scanned.all;
        all.sort();
        assert_eq!(
            all,
            vec!["github.com/gorilla/context", "github.com/gorilla/mux"]
        );
    }
}
