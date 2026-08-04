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
    imports: Vec<ScannedImport>,
}

struct ScannedImport {
    package: String,
    usage: ImportUsage,
}

enum ImportUsage {
    Named,
    Blank,
}

impl ScannedImports {
    /// All third-party `go:` imports (blank imports keep modules referenced).
    pub(crate) fn all(&self) -> impl Iterator<Item = &str> {
        self.imports.iter().map(|import| import.package.as_str())
    }

    /// Third-party `go:` imports that require typedefs.
    pub(crate) fn non_blank(&self) -> impl Iterator<Item = &str> {
        self.imports
            .iter()
            .filter(|import| matches!(import.usage, ImportUsage::Named))
            .map(|import| import.package.as_str())
    }
}

/// Collect every third-party `go:` import across `src/**/*.lis`.
pub(crate) fn scan_source_imports(src_dir: &Path) -> Result<ScannedImports, SourceScanError> {
    use rayon::prelude::*;

    if !src_dir.is_dir() {
        return Ok(ScannedImports {
            imports: Vec::new(),
        });
    }

    let scanned: Vec<Result<Vec<ScannedImport>, SourceScanError>> =
        collect_lis_filepaths_recursive(src_dir)
            .into_par_iter()
            .map(scan_file_imports)
            .collect();

    let mut imports = Vec::new();
    for file_imports in scanned {
        imports.extend(file_imports?);
    }

    Ok(ScannedImports { imports })
}

fn scan_file_imports(path: PathBuf) -> Result<Vec<ScannedImport>, SourceScanError> {
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

    let mut imports = Vec::new();
    for expr in &parse_result.ast {
        if let Expression::PackageImport { name, alias, .. } = expr
            && let Some(pkg) = name.strip_prefix("go:")
            && deps::is_third_party(pkg)
        {
            imports.push(ScannedImport {
                package: pkg.to_string(),
                usage: if matches!(alias, Some(ImportAlias::Blank(_))) {
                    ImportUsage::Blank
                } else {
                    ImportUsage::Named
                },
            });
        }
    }
    Ok(imports)
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

        assert_eq!(
            scanned.non_blank().collect::<Vec<_>>(),
            ["github.com/gorilla/mux"]
        );
        let mut all: Vec<_> = scanned.all().collect();
        all.sort();
        assert_eq!(
            all,
            vec!["github.com/gorilla/context", "github.com/gorilla/mux"]
        );
    }
}
