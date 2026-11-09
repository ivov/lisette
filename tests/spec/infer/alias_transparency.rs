//! Every pairing must be accepted or rejected both with and without the alias.

use crate::spec::infer::*;

const PRELUDE: &str = r#"
interface Show { fn show() -> string }
struct P { a: int }
impl P { fn show(self) -> string { "p" } }
enum Shape { Dot, Line(int) }
fn take_show<T: Show>(_x: T) {}
fn take_comparable<T: Comparable>(_x: T) {}
fn take_ordered<T: Ordered>(_x: T) {}
"#;

const TYPES: &[&str] = &[
    "int",
    "string",
    "float64",
    "bool",
    "P",
    "Shape",
    "Slice<int>",
    "Map<string, int>",
    "Option<int>",
    "Result<int, string>",
    "fn(int) -> int",
    "Ref<P>",
    "(int, string)",
    "Channel<int>",
];

const WRAPPERS: &[&str] = &["T", "Slice<T>", "Option<T>", "Ref<T>"];

const OPERATIONS: &[&str] = &[
    "let _ = x",
    "let _ = x.a",
    "let _ = x.show()",
    "let _ = x.length()",
    "let _ = x(1)",
    "let _ = x[0]",
    "let _ = x + x",
    "let _ = x == x",
    "let _ = x < x",
    "let _ = -x",
    "let _ = !x",
    "let _ = x as int",
    "let _ = &x",
    "let _ = x.*",
    "let _ = x.0",
    "let _ = if x { 1 } else { 2 }",
    "for a in x { let _ = a }",
    "match x { Some(v) => { let _ = v }, None => {} }",
    "match x { Ok(v) => { let _ = v }, Err(e) => { let _ = e } }",
    "match x { Shape.Dot => {}, Shape.Line(n) => { let _ = n } }",
    "match x { P { a } => { let _ = a } }",
    "let (a, b) = x\n  let _ = a\n  let _ = b",
    "take_show(x)",
    "take_comparable(x)",
    "take_ordered(x)",
    "let _ = x.unwrap_or(x.unwrap())",
    "let _ = x.map(|v| v)",
    "let _ = f\"{x}\"",
    "let _ = x.receive()",
    "let m = Map.new<int, int>()\n  let _ = m[x]",
    "let _ = [x, x]",
    "let _: int = x",
];

fn program(declaration: &str, parameter_ty: &str, operation: &str) -> String {
    format!("{PRELUDE}\n{declaration}\nfn test(x: {parameter_ty}) {{\n  {operation}\n}}\n")
}

fn error_codes(source_text: &str) -> Vec<String> {
    let mut codes: Vec<String> = infer(source_text)
        .errors
        .iter()
        .filter(|error| error.is_error())
        .map(|error| error.code_str().unwrap_or("<no code>").to_string())
        .collect();
    codes.sort();
    codes
}

#[test]
fn an_alias_checks_like_the_type_it_names() {
    let mut failures = Vec::new();
    for ty in TYPES {
        for wrapper in WRAPPERS {
            let direct = wrapper.replace('T', ty);
            let aliased = wrapper.replace('T', "Alias");
            let declaration = format!("type Alias = {ty}");
            for operation in OPERATIONS {
                let expected = error_codes(&program(&declaration, &direct, operation));
                let found = error_codes(&program(&declaration, &aliased, operation));
                if expected.is_empty() != found.is_empty() {
                    failures.push(format!(
                        "type `{direct}` as `{aliased}`, operation `{}`\n    direct  {expected:?}\n    aliased {found:?}",
                        operation.replace('\n', "; ")
                    ));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} pairing(s) are accepted only one way:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
