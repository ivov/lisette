use crate::spec::infer::*;

#[test]
fn channel_new_without_type_arg_errors() {
    infer(
        r#"
    fn test() {
      let ch = Channel.new();
    }
        "#,
    )
    .assert_infer_code("missing_type_argument");
}

#[test]
fn slice_new_without_type_arg_errors() {
    infer(
        r#"
    fn test() {
      let s = Slice.new();
    }
        "#,
    )
    .assert_infer_code("missing_type_argument");
}

#[test]
fn map_new_without_type_args_errors() {
    infer(
        r#"
    fn test() {
      let m = Map.new();
    }
        "#,
    )
    .assert_infer_code("missing_type_argument");
}

#[test]
fn ok_variant_without_type_arg_succeeds() {
    infer(
        r#"
    fn test() {
      let r = Ok(42);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn err_variant_without_type_arg_succeeds() {
    infer(
        r#"
    fn test() {
      let r = Err("failed");
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn some_variant_without_type_arg_succeeds() {
    infer(
        r#"
    fn test() {
      let o = Some(42);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn none_variant_succeeds() {
    infer(
        r#"
    fn test() -> Option<int> {
      return None;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn channel_new_with_type_arg_succeeds() {
    infer(
        r#"
    fn test() {
      let ch = Channel.new<int>();
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn slice_new_with_type_arg_succeeds() {
    infer(
        r#"
    fn test() {
      let s = Slice.new<string>();
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn map_new_with_type_args_succeeds() {
    infer(
        r#"
    fn test() {
      let m = Map.new<string, int>();
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn channel_new_type_from_annotation_succeeds() {
    infer(
        r#"
    fn test() {
      let ch: Channel<int> = Channel.new();
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn slice_new_type_from_annotation_succeeds() {
    infer(
        r#"
    fn test() {
      let s: Slice<bool> = Slice.new();
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn map_new_type_from_annotation_succeeds() {
    infer(
        r#"
    fn test() {
      let m: Map<string, int> = Map.new();
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn channel_new_type_from_return_type_succeeds() {
    infer(
        r#"
    fn make_channel() -> Channel<int> {
      return Channel.new();
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn empty_literal_in_generic_call_errors() {
    infer(
        r#"
    fn count<T>(items: Slice<T>) -> int {
      items.length()
    }

    fn test() -> int {
      count([])
    }
        "#,
    )
    .assert_infer_code("empty_slice_no_element_type");
}

#[test]
fn empty_literal_unresolved_binding_reports_once() {
    let result = crate::_harness::infer::infer("fn test() { let xs = []; let _ = xs }");
    let errors: Vec<_> = result
        .errors
        .iter()
        .filter(|e| e.is_error())
        .filter_map(|e| e.code_str())
        .collect();
    assert_eq!(
        errors,
        vec!["infer.type_not_inferred"],
        "an unresolved empty-literal binding must report only the binding diagnostic"
    );
}

#[test]
fn empty_literal_concrete_param_succeeds() {
    infer(
        r#"
    fn take(xs: Slice<int>) -> int {
      xs.length()
    }

    fn test() -> int {
      take([])
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn empty_literal_annotation_succeeds() {
    infer(
        r#"
    fn test() {
      let xs: Slice<string> = [];
      let _ = xs;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn empty_literal_resolved_by_later_append_succeeds() {
    infer(
        r#"
    fn test() {
      let mut xs = [];
      xs = xs.append(1);
      let _ = xs;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn empty_literal_in_generic_return_position_succeeds() {
    infer(
        r#"
    fn empty_of<T>() -> Slice<T> {
      []
    }

    fn test() {
      let xs: Slice<string> = empty_of();
      let _ = xs;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn empty_literal_against_non_slice_param_reports_only_mismatch() {
    let result = crate::_harness::infer::infer(
        r#"
    fn take(x: int) -> int {
      x
    }

    fn test() -> int {
      take([])
    }
        "#,
    );
    let errors: Vec<_> = result
        .errors
        .iter()
        .filter(|e| e.is_error())
        .filter_map(|e| e.code_str())
        .collect();
    assert_eq!(
        errors,
        vec!["infer.type_mismatch"],
        "a context mismatch must not cascade into an empty-literal report"
    );
}

#[test]
fn empty_literal_in_uninferred_generic_call_reports_only_call() {
    let result = crate::_harness::infer::infer(
        r#"
    fn keep<T>(items: Slice<T>) -> Slice<T> {
      items
    }

    fn test() {
      let _ = keep([]);
    }
        "#,
    );
    let errors: Vec<_> = result
        .errors
        .iter()
        .filter(|e| e.is_error())
        .filter_map(|e| e.code_str())
        .collect();
    assert_eq!(
        errors,
        vec!["infer.missing_type_argument"],
        "an uninferred generic call must not cascade into an empty-literal report"
    );
}

#[test]
fn empty_literal_with_unrelated_var_reports_inside_uninferred_call() {
    let result = crate::_harness::infer::infer(
        r#"
    fn count<U>(items: Slice<U>) -> int {
      items.length()
    }

    fn outer<T>(n: int) -> Slice<T> {
      []
    }

    fn test() {
      let _ = outer(count([]));
    }
        "#,
    );
    let mut errors: Vec<_> = result
        .errors
        .iter()
        .filter(|e| e.is_error())
        .filter_map(|e| e.code_str())
        .collect();
    errors.sort_unstable();
    assert_eq!(
        errors,
        vec![
            "infer.empty_slice_no_element_type",
            "infer.missing_type_argument"
        ],
        "the literal's unresolved element is independent of the outer call's type argument, so both must report"
    );
}

#[test]
fn empty_literal_in_tuple_destructuring_errors() {
    infer(
        r#"
    fn test() {
      let (a, b) = ([], [1]);
      let _ = (a, b);
    }
        "#,
    )
    .assert_infer_code("empty_slice_no_element_type");
}
