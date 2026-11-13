use emit::{EmitOptions, OutputFile, Planner};
use rustc_hash::FxHashMap as HashMap;
use syntax::program::{EmitInput, File};

use super::pipeline::TestPipeline;

pub fn emit_with_sourcemap(raw_source: &str) -> EmitResult {
    emit_inner(raw_source, Some(raw_source), &[])
}

pub fn assert_sourcemap_only_adds_directives(input: &str, plain: &str) {
    let mapped = emit_with_sourcemap(input).go_code();
    assert_eq!(
        code_lines(&strip_line_directives(&mapped)),
        code_lines(plain),
        "sourcemap output differs beyond its directives:\n{mapped}"
    );
}

/// gofmt separates a directive from the doc comment above it with a `//` line.
fn strip_line_directives(go_code: &str) -> String {
    let lines: Vec<&str> = go_code.lines().collect();
    let mut stripped = String::new();
    let mut in_raw_string = false;
    for (index, line) in lines.iter().enumerate() {
        let directive_follows = lines
            .get(index + 1)
            .is_some_and(|next| next.starts_with("//line "));
        if !in_raw_string
            && (line.starts_with("//line ") || (line.trim() == "//" && directive_follows))
        {
            continue;
        }
        in_raw_string = strip_inline_directives(line, in_raw_string, &mut stripped);
        stripped.push('\n');
    }
    stripped
}

fn strip_inline_directives(line: &str, mut in_raw_string: bool, out: &mut String) -> bool {
    let mut rest = line;
    while let Some(c) = rest.chars().next() {
        if in_raw_string {
            in_raw_string = c != '`';
        } else if rest.starts_with("//") {
            out.push_str(rest);
            return false;
        } else if let Some(directive) = rest.strip_prefix("/*line ") {
            let end = directive.find("*/").expect("a directive closes") + "*/".len();
            rest = &directive[end..];
            rest = rest.strip_prefix(' ').unwrap_or(rest);
            continue;
        } else if c == '`' {
            in_raw_string = true;
        } else if c == '"' || c == '\'' {
            let mut escaped = false;
            let close = rest[1..]
                .find(|quoted| {
                    let closes = !escaped && quoted == c;
                    escaped = !escaped && quoted == '\\';
                    closes
                })
                .map_or(rest.len(), |at| at + 2);
            out.push_str(&rest[..close]);
            rest = &rest[close..];
            continue;
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    in_raw_string
}

/// gofmt spaces comments differently.
fn code_lines(go_code: &str) -> Vec<String> {
    go_code
        .lines()
        .map(|line| line.split_whitespace().collect::<String>())
        .filter(|line| !line.is_empty())
        .collect()
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
        sourcemap_base: None,
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
