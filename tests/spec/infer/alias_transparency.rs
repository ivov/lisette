//! Every pairing must be accepted or rejected both with and without the alias.
use crate::_harness::infer::for_each_expression;

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
    "uint8",
    "int8",
    "uintptr",
    "Array<int, 2>",
    "Range<int>",
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
    "let _ = &x[0]",
    "let _ = &x[\"a\"]",
    "let _ = x & x",
    "let _ = x << 1",
    "let _ = ^x",
    "let s = [1, 2]\n  let _ = s[x]",
    "select { let Some(v) = x.receive() => { let _ = v }, }",
    "let _y: {ty} = -1",
    "let _y: {ty} = -200",
    "let _ = &make()[0]",
    "make()[0] = 1",
];

fn program(declaration: &str, parameter_ty: &str, operation: &str) -> String {
    let operation = operation.replace("{ty}", parameter_ty);
    format!(
        "{PRELUDE}\n{declaration}\nfn make() -> {parameter_ty} {{ panic(\"make\") }}\nfn test(x: {parameter_ty}) {{\n  {operation}\n}}\n"
    )
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

#[test]
fn a_newtype_of_an_unsigned_integer_rejects_a_negative_constant() {
    infer("struct Mode(uint8)\nfn test() { let _m: Mode = -1 }")
        .assert_infer_code("cannot_negate_unsigned");
}

#[test]
fn integer_operators_reject_a_reference_to_an_integer() {
    for operation in ["x & x", "x << 1", "^x"] {
        infer(&format!("fn test(x: Ref<int>) {{ let _ = {operation} }}"))
            .assert_infer_code("type_mismatch");
    }
}

#[test]
fn reference_alias_array_elements_resolve_to_ref() {
    use crate::_harness::infer::infer;
    use syntax::ast::Expression;
    use syntax::types::Type;
    let result =
        infer("type IntRef = Ref<int>; fn refs(xs: Array<IntRef, 2>) -> Array<IntRef, 2> { xs }")
            .assert_no_errors();

    let mut seen = false;
    for_each_expression(&result.ast, |expression| {
        if let Expression::Identifier {
            value,
            ty: Type::Array { element, .. },
            ..
        } = expression
            && value == "xs"
        {
            assert!(
                element.is_ref(),
                "array element was not normalized: {element:?}"
            );
            seen = true;
        }
    });
    assert!(seen);
}

#[test]
fn go_function_alias_preserves_its_declared_method_owner() {
    infer_with_go_typedefs(
        r#"import "go:example.com/handler"
fn accepts(h: handler.Handler) { h.handle() }
fn main() { let h = handler.HandlerFunc(|| {}); accepts(h) }
"#,
        &[(
            "go:example.com/handler",
            r#"
pub interface Handler { fn handle() }
pub type HandlerFunc = fn()
impl HandlerFunc { pub fn handle(self) }
"#,
        )],
    )
    .assert_no_errors();
}
