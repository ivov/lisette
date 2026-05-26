use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use deps::TypedefLocator;
use semantics::analyze::{AnalyzeInput, CompilePhase, SemanticConfig};
use semantics::loader::{Loader, MemoryLoader};
use syntax::ast::Expression;

pub fn single_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join("single")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read single fixture {name}: {e}"))
}

/// Builds a synthetic multi-module Lisette project entirely in memory.
///
/// Returns the populated [`MemoryLoader`] and the entry file source. Each
/// generated module contains `n_funcs` trivial int-arithmetic functions; the
/// entry module imports every generated module but calls none.
///
/// Deterministic: same arguments produce byte-identical output across runs.
pub fn stress_project(n_modules: usize, n_funcs: usize) -> (MemoryLoader, String) {
    let mut loader = MemoryLoader::new();

    let mut entry_source = String::new();
    for i in 0..n_modules {
        writeln!(entry_source, "import \"mod{i:03}\"").unwrap();
    }
    entry_source.push_str("\nfn main() {}\n");
    loader.add_file("_entry_", "main.lis", &entry_source);

    for i in 0..n_modules {
        let mod_name = format!("mod{i:03}");
        let mut module = String::new();
        for j in 0..n_funcs {
            writeln!(
                module,
                "fn f{j:03}(x: int, y: int) -> int {{\n  let a = x + y;\n  let b = a * 2;\n  if b > 100 {{\n    return b - 1;\n  }}\n  return b + x;\n}}\n"
            )
            .unwrap();
        }
        let filename = format!("{mod_name}.lis");
        loader.add_file(&mod_name, &filename, &module);
    }

    (loader, entry_source)
}

pub fn analyze_input<'a>(
    source: String,
    filename: String,
    ast: Vec<Expression>,
    loader: &'a dyn Loader,
    compile_phase: CompilePhase,
) -> AnalyzeInput<'a> {
    AnalyzeInput {
        config: SemanticConfig {
            run_lints: true,
            standalone_mode: false,
            load_siblings: false,
        },
        loader,
        source,
        display_path: filename.clone(),
        filename,
        ast,
        project_root: None,
        compile_phase,
        locator: TypedefLocator::default(),
        go_module: "bench".to_string(),
        disable_cache: true,
    }
}
