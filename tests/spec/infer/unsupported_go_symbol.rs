use crate::spec::infer::*;

const TYPEDEF: &str = r#"
#[go(skipped, "Use Find instead")]
pub type Box

#[go(skipped, "Use Find instead")]
pub var Lookup: Unknown

#[go(skipped, "Use Find instead")]
pub var Limit: Unknown
"#;

#[test]
fn call_is_rejected_with_reason() {
    infer(
        r#"import "go:errors"
import "go:os"
fn check(err: error) -> bool {
  let mut target: Option<Ref<os.PathError>> = None
  errors.As(err, &target)
}"#,
    )
    .assert_resolve_code_once("unsupported_go_symbol")
    .assert_error_contains("errors.AsType<E>(err)");
}

#[test]
fn function_value_is_rejected() {
    infer(
        r#"import "go:errors"
fn check(err: error) -> bool {
  let mut target = 0
  let as_error = errors.As
  as_error(err, &target)
}"#,
    )
    .assert_resolve_code_once("unsupported_go_symbol");
}

#[test]
fn use_in_generic_function_is_rejected() {
    infer(
        r#"import "go:errors"
fn find<T>(err: error, target: mut Ref<T>) -> bool {
  errors.As(err, target)
}"#,
    )
    .assert_resolve_code_once("unsupported_go_symbol");
}

#[test]
fn builtin_skipped_by_bindgen_is_rejected_with_reason() {
    infer(
        r#"import "go:unsafe"
fn size() -> uintptr {
  let n = 0
  unsafe.Sizeof(n)
}"#,
    )
    .assert_resolve_code_once("unsupported_go_symbol")
    .assert_error_contains("Go expands this builtin at compile time");
}

#[test]
fn skipped_type_in_annotation_is_rejected() {
    let input = r#"
import "go:example.com/store"

fn run(_b: store.Box) {}
"#;
    infer_with_go_typedefs(input, &[("go:example.com/store", TYPEDEF)])
        .assert_resolve_code_once("unsupported_go_symbol")
        .assert_error_contains("Use Find instead");
}

#[test]
fn label_names_the_package_not_the_alias() {
    let input = r#"
import s "go:example.com/store"

fn run(_b: s.Box) {
  let _ = s.Lookup
}
"#;
    infer_with_go_typedefs(input, &[("go:example.com/store", TYPEDEF)])
        .assert_resolve_code_count("unsupported_go_symbol", 2)
        .assert_error_contains("Lisette cannot use `store.Box`")
        .assert_error_contains("Lisette cannot use `store.Lookup`");
}

#[test]
fn skipped_type_in_struct_literal_is_rejected() {
    let input = r#"
import "go:example.com/store"

fn run() {
  let _ = store.Box {}
}
"#;
    infer_with_go_typedefs(input, &[("go:example.com/store", TYPEDEF)])
        .assert_resolve_code_once("unsupported_go_symbol")
        .assert_error_contains("Use Find instead");
}

#[test]
fn skipped_type_in_struct_pattern_is_rejected() {
    let input = r#"
import "go:example.com/store"

fn run(value: int) {
  match value {
    store.Box {} => {},
    _ => {},
  }
}
"#;
    infer_with_go_typedefs(input, &[("go:example.com/store", TYPEDEF)])
        .assert_resolve_code_once("unsupported_go_symbol")
        .assert_error_contains("Use Find instead");
}

#[test]
fn skipped_value_in_pattern_is_rejected() {
    let input = r#"
import "go:example.com/store"

fn run(value: int) {
  match value {
    store.Lookup => {},
    _ => {},
  }
}
"#;
    infer_with_go_typedefs(input, &[("go:example.com/store", TYPEDEF)])
        .assert_resolve_code_once("unsupported_go_symbol")
        .assert_error_contains("Use Find instead");
}

#[test]
fn skipped_constant_as_array_size_is_rejected() {
    let input = r#"
import "go:example.com/store"

fn run(_buffer: Array<byte, store.Limit>) {}
"#;
    infer_with_go_typedefs(input, &[("go:example.com/store", TYPEDEF)])
        .assert_resolve_code_once("unsupported_go_symbol")
        .assert_error_contains("Use Find instead");
}
