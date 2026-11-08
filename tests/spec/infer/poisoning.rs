//! Every pairing must report only the errors of the source alone.

use crate::spec::infer::*;

const PRELUDE: &str = r#"
interface Show { fn show() -> string }
struct P { a: int }
enum Shape { Dot, Line(int) }
fn take<T: Show>(_x: T) {}
fn want_int(_n: int) {}
fn want_fn(_f: fn(int) -> int) {}
"#;

const SOURCES: &[&str] = &[
    "undefined_fn(1)",
    "undefined_name",
    "p.missing_field",
    "p.missing_method()",
    "Missing.new()",
    "Missing { a: 1 }",
    "Shape.Missing",
];

const OPERATIONS: &[&str] = &[
    "let _ = x",
    "let _ = x.field",
    "let _ = x.method()",
    "let _ = x.method(1, 2)",
    "let _ = x(1)",
    "let _ = x[0]",
    "let _ = x + 1",
    "let _ = 1 + x",
    "let _ = x == 1",
    "let _ = x < 1",
    "let _ = -x",
    "let _ = !x",
    "let _ = x as int",
    "let _ = &x",
    "let _ = x.*",
    "let _ = if x { 1 } else { 2 }",
    "while x { break }",
    "for a in x { let _ = a }",
    "match x { Some(v) => { let _ = v }, None => {} }",
    "match x { Ok(v) => { let _ = v }, Err(e) => { let _ = e } }",
    "match x { 1 => {}, _ => {} }",
    "match x { P { a } => { let _ = a } }",
    "match x { Shape.Dot => {}, Shape.Line(n) => { let _ = n } }",
    "match x { (a, b) => { let _ = a; let _ = b } }",
    "if let Some(v) = x { let _ = v }",
    "let Some(v) = x else { return }\n  let _ = v",
    "let (a, b) = x\n  let _ = a\n  let _ = b",
    "match x { [a, ..rest] => { let _ = a; let _ = rest }, _ => {} }",
    "match x { Some(1) | None => {}, _ => {} }",
    "match x { Some(v) if v > 0 => {}, _ => {} }",
    "match x { Some(_) as s => { let _ = s }, _ => {} }",
    "take(x)",
    "want_int(x)",
    "want_fn(x)",
    "let _: int = x",
    "let _: string = x",
    "let _ = P { a: x }",
    "let _ = [x, 1]",
    "let _ = (x, 1)",
    "let _ = f\"{x}\"",
    "let _ = x |> want_int",
    "let _ = x.map(|v| v)",
    "let _ = x.unwrap_or(0)",
    "let _ = x.length()",
    "let mut m = Map.new<int, int>()\n  m[x] = 1",
    "let mut m = Map.new<int, int>()\n  m[1] = x",
    "let mut s = [1, 2]\n  s[x] = 1",
];

fn program(source: &str, operation: &str) -> String {
    format!(
        "{PRELUDE}\nfn main() {{\n  let p = P {{ a: 1 }}\n  let _ = p\n  let x = {source}\n  {operation}\n}}\n"
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
fn a_poisoned_value_reports_no_further_errors() {
    let mut failures = Vec::new();
    for source in SOURCES {
        let baseline = error_codes(&program(source, "let _ = 0"));
        assert!(
            !baseline.is_empty(),
            "the source `{source}` must fail to infer"
        );
        for operation in OPERATIONS {
            let codes = error_codes(&program(source, operation));
            if codes != baseline {
                failures.push(format!(
                    "source `{source}`, operation `{}`\n    expected {baseline:?}\n    found    {codes:?}",
                    operation.replace('\n', "; ")
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} pairing(s) report extra errors:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn patterns_on_a_poisoned_value_still_report_unknown_names() {
    let source = r#"
struct Point {
  x: int,
}

fn main() {
  let v = undefined_value
  match v {
    Pointt { x } => {
      let _ = x
    },
    _ => {},
  }
  match v {
    Sme(y) => {
      let _ = y
    },
    _ => {},
  }
  let _ = Point { x: 1 }
}
"#;
    assert_eq!(
        error_codes(source),
        [
            "resolve.name_not_found",
            "resolve.struct_not_found",
            "resolve.variant_not_found"
        ]
    );
}

#[test]
fn patterns_on_a_poisoned_value_report_no_coverage_errors() {
    let source = r#"
fn main() {
  let v = undefined_value
  match v {
    Some(y) => {
      let _ = y
    },
    None => (),
  }
  if let Some(a) = v {
    let _ = a
  } else {
    let _ = 1
  }
  let Some(b) = v else {
    return
  }
  let _ = b
  while let Some(c) = v {
    let _ = c
  }
}
"#;
    assert_eq!(error_codes(source), ["resolve.name_not_found"]);
}

#[test]
fn an_unknown_type_argument_does_not_hide_a_wrong_count() {
    let source = r#"
fn main() {
  let m: Map<Undefined> = Map.new()
  let _ = m
}
"#;
    assert_eq!(
        error_codes(source),
        ["infer.type_arg_count_mismatch", "resolve.type_not_found"]
    );
}

#[test]
fn a_reserved_import_alias_reports_once() {
    let source = r#"
import Map "go:maps"

fn main() {
  let m = Map.new<string, int>()
  let n: string = m.length()
  let _ = n
}
"#;
    assert_eq!(error_codes(source), ["resolve.reserved_import_alias"]);
}

#[test]
fn a_panic_in_an_operand_still_reports_the_never_call() {
    let source = r#"
fn main() {
  let x = 1 + panic(undefined_msg)
  let _ = x
}
"#;
    assert_eq!(
        error_codes(source),
        ["infer.never_call_in_expression", "resolve.name_not_found"]
    );
}

#[test]
fn a_function_reference_with_an_unknown_bounded_type_still_reports() {
    let source = r#"
fn identity<T: Ordered>(x: T) -> T { x }
fn wrap<T>(f: fn(T) -> T) -> fn(T) -> T { f }
fn main() { let _f = wrap(identity) }
"#;
    assert_eq!(
        error_codes(source),
        ["infer.cannot_infer_bounded_function_reference"]
    );
}

#[test]
fn embedding_a_pointer_to_an_interface_reports_once_in_each_form() {
    let source = r#"
pub interface Named { fn name() -> string }
pub type Pointer<T> = Ref<T>
pub type NamedRef = Ref<Named>
pub struct Wrapper { embed Pointer<Named> }
pub struct Direct { embed Ref<Named> }
pub struct Plain { embed NamedRef }
fn main() {}
"#;
    assert_eq!(
        error_codes(source),
        [
            "infer.embed_pointer_to_interface",
            "infer.embed_pointer_to_interface",
            "infer.ref_of_interface"
        ]
    );
}

#[test]
fn a_rejected_arm_does_not_hide_other_unreachable_arms() {
    let source = r#"
enum E { A, B }
fn main() {
  match E.B {
    E.A => (),
    E.A => (),
    E.Missing => (),
  }
  match E.B {
    E.Missing => (),
    E.A => (),
    E.A => (),
  }
}
"#;
    assert_eq!(
        error_codes(source),
        [
            "infer.redundant_arm",
            "infer.redundant_arm",
            "resolve.variant_not_found",
            "resolve.variant_not_found"
        ]
    );
}

#[test]
fn slice_patterns_on_a_poisoned_value_still_report_unknown_names() {
    let source = r#"
fn main() {
  match missing_value {
    [Missing(_)] => (),
    [a, ..rest] => {
      let _ = a
      let _ = rest
    },
    _ => (),
  }
}
"#;
    assert_eq!(
        error_codes(source),
        ["resolve.name_not_found", "resolve.variant_not_found"]
    );
}

#[test]
fn a_wrong_type_argument_count_reports_once() {
    let source = r#"
fn choose<T: Comparable>() -> Option<T> { None }
fn main() { let _x = choose<int, string>() }
"#;
    assert_eq!(error_codes(source), ["infer.type_arg_count_mismatch"]);
}

#[test]
fn a_rejected_alternative_does_not_hide_its_valid_siblings() {
    let source = r#"
enum E { A, B }
fn main() {
  match E.B {
    E.A => (),
    E.A | E.Missing => (),
  }
  match E.B {
    E.A => (),
    E.B | E.Missing => (),
  }
  match E.B {
    E.A | E.Missing => (),
    E.A => (),
  }
}
"#;
    assert_eq!(
        error_codes(source),
        [
            "infer.redundant_arm",
            "infer.redundant_arm",
            "resolve.variant_not_found",
            "resolve.variant_not_found",
            "resolve.variant_not_found"
        ]
    );
}
