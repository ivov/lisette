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
fn slice_make_without_type_arg_errors() {
    infer(
        r#"
    fn test() {
      let s = Slice.make(4);
    }
        "#,
    )
    .assert_infer_code("missing_type_argument");
}

#[test]
fn slice_make_with_type_arg_succeeds() {
    infer(
        r#"
    fn test() {
      let s = Slice.make<string>(4);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn slice_make_type_from_annotation_succeeds() {
    infer(
        r#"
    fn test() {
      let s: Slice<bool> = Slice.make(2);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn slice_make_zero_length_succeeds() {
    infer(
        r#"
    fn test() {
      let s = Slice.make<int>(0);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn slice_make_option_ref_element_succeeds() {
    infer(
        r#"
    fn test() {
      let s = Slice.make<Option<Ref<int>>>(3);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn slice_make_ref_element_errors() {
    infer(
        r#"
    fn test() {
      let refs = Slice.make<Ref<int>>(4);
    }
        "#,
    )
    .assert_infer_code("slice_make_no_zero");
}

#[test]
fn slice_make_annotation_ref_element_errors() {
    infer(
        r#"
    fn test() {
      let refs: Slice<Ref<int>> = Slice.make(4);
    }
        "#,
    )
    .assert_infer_code("slice_make_no_zero");
}

#[test]
fn slice_make_element_resolved_later_errors() {
    infer(
        r#"
    fn test() {
      let mut chans = Slice.make(1);
      chans = chans.append(Channel.new<int>());
    }
        "#,
    )
    .assert_infer_code("slice_make_no_zero");
}

#[test]
fn slice_make_struct_with_ref_field_errors() {
    infer(
        r#"
    struct Holder { r: Ref<int> }

    fn test() {
      let holders = Slice.make<Holder>(1);
    }
        "#,
    )
    .assert_infer_code("slice_make_no_zero");
}

#[test]
fn slice_make_type_parameter_element_errors() {
    infer(
        r#"
    fn make<T>(n: int) -> Slice<T> {
      Slice.make<T>(n)
    }
        "#,
    )
    .assert_infer_code("slice_make_no_zero");
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
fn negative_literal_buffered_errors() {
    infer("fn test() { let c = Channel.buffered<int>(-2); let _ = c }")
        .assert_infer_code("negative_size_literal");
}

#[test]
fn negative_literal_reserve_errors() {
    infer("fn test() { let mut s = [1]; s = s.reserve(-3); let _ = s }")
        .assert_infer_code("negative_size_literal");
}

#[test]
fn negative_literal_reserve_ufcs_errors() {
    infer("fn test() { let mut s = [1]; s = Slice.reserve(s, -4); let _ = s }")
        .assert_infer_code("negative_size_literal");
}

#[test]
fn negative_zero_size_literal_is_legal() {
    infer("fn test() { let a = Slice.make<byte>(-0); let _ = a }").assert_no_errors();
}

#[test]
fn computed_negative_size_stays_runtime() {
    infer("fn test() { let a = Slice.make<byte>(0 - 1); let _ = a }").assert_no_errors();
}

#[test]
fn slice_make_guard_sees_through_parens() {
    infer(
        r#"
    fn test() {
      let refs: Slice<Ref<int>> = (Slice.make)(4);
      let _ = refs;
    }
        "#,
    )
    .assert_infer_code("slice_make_no_zero");
}

#[test]
fn user_static_named_make_is_a_legal_value() {
    infer(
        r#"
    struct Builder {}

    impl Builder {
      fn make() -> Slice<int> {
        [1, 2]
      }
    }

    fn test() {
      let factory = Builder.make;
      let made = factory();
      let _ = made;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn map_make_recovery_infers_spread() {
    let result = crate::_harness::infer::infer(
        r#"
    fn test(xs: Slice<int>) {
      let m = Map.make<string, int>(xs...);
      let _ = m;
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
        vec!["infer.no_make_constructor"],
        "the recovery must infer the spread without cascading"
    );
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
