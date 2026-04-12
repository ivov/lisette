use emit::{Emitter, OutputFile, TestEmitConfig};
use syntax::program::File;

use super::pipeline::TestPipeline;

pub fn emit_with_debug_info(raw_source: &str) -> EmitResult {
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
    source_for_debug: Option<&str>,
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
        module_id: result.module_id.clone(),
        name: "test.lis".to_string(),
        source: source_for_debug.unwrap_or("").to_string(),
        items: result.ast,
    };

    let config = TestEmitConfig {
        definitions: &result.definitions,
        module_id: &result.module_id,
        go_module: "myproject",
        unused: &result.unused,
        mutations: &result.mutations,
        coercions: &result.coercions,
        resolutions: &result.resolutions,
        ufcs_methods: &result.ufcs_methods,
    };
    let mut emitter = Emitter::new_for_tests(&config, source_for_debug);
    let emitted_files = emitter.emit_files(&[&file], &result.module_id);

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
