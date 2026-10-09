use emit::{EmitOptions, OutputFile, Planner};
use rustc_hash::FxHashMap as HashMap;
use syntax::program::{EmitInput, File};

use super::pipeline::TestPipeline;

pub fn emit_with_sourcemap(raw_source: &str) -> EmitResult {
    emit_inner(raw_source, Some(raw_source), &[])
}

pub fn emit(raw_source: &str) -> EmitResult {
    emit_inner(raw_source, None, &[])
}

pub fn emit_with_go_typedefs(raw_source: &str, typedefs: &[(&str, &str)]) -> EmitResult {
    emit_inner(raw_source, None, typedefs)
}

fn emit_inner(
    raw_source: &str,
    source_for_sourcemap: Option<&str>,
    extra_go_typedefs: &[(&str, &str)],
) -> EmitResult {
    let mut pipeline = TestPipeline::new(raw_source).wrapped();
    for (name, source) in extra_go_typedefs {
        pipeline = pipeline.with_go_typedef(name, source);
    }
    let compiled = pipeline.compile();

    let result = compiled.run_inference();

    if !result.errors.is_empty() {
        panic!("Type inference failed in emit test: {:?}", result.errors);
    }

    let file = File {
        id: 0,
        package_id: result.package_id.clone(),
        parse_status: syntax::FileParseStatus::Clean,
        name: "test.lis".to_string(),
        display_path: "src/test.lis".to_string(),
        source_path: None,
        source: source_for_sourcemap.unwrap_or("").to_string(),
        items: result.ast,
        file_comment: None,
    };

    let input = EmitInput {
        files: HashMap::from_iter([(0, file)]),
        definitions: result.definitions,
        entry_package_id: result.package_id,
        unused: result.unused,
        bindings: result.bindings,
        equality_index: result.equality_index,
        go_package_names: result.go_package_names,
        go_package_ids: result.go_package_ids,
        ..Default::default()
    };
    let options = EmitOptions {
        sourcemap: source_for_sourcemap.is_some(),
        emit_tests: false,
    };
    let emitted_files = Planner::emit(&input, "myproject", "main", options)
        .unwrap_or_else(|diagnostics| panic!("Emission failed: {diagnostics:?}"));

    EmitResult {
        files: emitted_files,
    }
}

pub struct EmitResult {
    pub files: Vec<OutputFile>,
}

impl EmitResult {
    pub fn go_code(&self) -> String {
        if self.files.is_empty() {
            panic!("No files emitted");
        }
        self.files[0].to_go()
    }
}
