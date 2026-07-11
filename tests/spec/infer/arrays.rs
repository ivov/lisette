use crate::spec::infer::*;

#[test]
fn array_literal_with_annotation() {
    infer("let xs: Array<int, 3> = [1, 2, 3]; xs").assert_last_type(array_type(3, int_type()));
}

#[test]
fn array_element_adapts_to_annotation() {
    infer("let xs: Array<int8, 2> = [1, 2]; xs").assert_last_type(array_type(2, int8_type()));
}

#[test]
fn array_index_returns_element() {
    infer("let xs: Array<int, 3> = [1, 2, 3]; xs[0]").assert_last_type(int_type());
}

#[test]
fn cannot_assign_to_array_element_through_map_base() {
    infer(
        r#"{
    let mut m: Map<string, Array<int, 3>> = Map.new()
    m["a"] = Array.new()
    m["a"][0] = 9
  }"#,
    )
    .assert_infer_code("non_addressable_assignment");
}

#[test]
fn cannot_assign_to_array_element_of_function_result() {
    infer(
        r#"{
    fn make() -> Array<int, 3> { [1, 2, 3] }
    make()[0] = 9
  }"#,
    )
    .assert_infer_code("non_addressable_assignment");
}

#[test]
fn can_assign_to_element_of_addressable_array() {
    infer(
        r#"{
    let mut xs: Array<int, 3> = [1, 2, 3]
    xs[0] = 9
  }"#,
    )
    .assert_no_errors();
}

#[test]
fn array_length_returns_int() {
    infer("let xs: Array<int, 3> = [1, 2, 3]; xs.length()").assert_last_type(int_type());
}

#[test]
fn array_equality_is_bool() {
    infer("let xs: Array<int, 2> = [1, 2]; let ys: Array<int, 2> = [3, 4]; xs == ys")
        .assert_last_type(bool_type());
}

#[test]
fn array_inequality_is_bool() {
    infer("let xs: Array<int, 2> = [1, 2]; let ys: Array<int, 2> = [3, 4]; xs != ys")
        .assert_last_type(bool_type());
}

#[test]
fn nested_array_type() {
    infer("let xs: Array<Array<int, 3>, 2> = [[1, 2, 3], [4, 5, 6]]; xs")
        .assert_last_type(array_type(2, array_type(3, int_type())));
}

#[test]
fn array_literal_too_few_elements() {
    infer("let xs: Array<int, 3> = [1, 2]; xs").assert_infer_code("array_literal_length_mismatch");
}

#[test]
fn array_literal_too_many_elements() {
    infer("let xs: Array<int, 2> = [1, 2, 3]; xs")
        .assert_infer_code("array_literal_length_mismatch");
}

#[test]
fn arrays_of_different_lengths_do_not_unify() {
    infer("let xs: Array<int, 3> = [1, 2, 3]; let ys: Array<int, 4> = xs; ys")
        .assert_infer_code("array_length_mismatch");
}

#[test]
fn array_element_type_mismatch() {
    infer(r#"let xs: Array<int, 2> = ["a", "b"]; xs"#).assert_infer_code("type_mismatch");
}

#[test]
fn array_size_must_be_literal() {
    infer("let xs: Array<int, int> = [1]; xs").assert_infer_code("array_size_not_literal");
}

#[test]
fn array_size_above_int_max_is_rejected() {
    infer("fn f(x: Array<int, 18000000000000000000>) {}").assert_infer_code("array_size_too_large");
}

#[test]
fn array_of_slices_is_not_comparable() {
    infer(
        "let xs: Array<Slice<int>, 2> = [[1], [2]]; let ys: Array<Slice<int>, 2> = [[3], [4]]; xs == ys",
    )
    .assert_infer_code("type_mismatch");
}

#[test]
fn bounded_generic_array_equality_is_allowed() {
    infer("fn eq<T: Comparable>(a: Array<T, 2>, b: Array<T, 2>) -> bool { a == b }")
        .assert_no_errors();
}

#[test]
fn unbounded_generic_array_equality_is_rejected() {
    infer("fn eq<T>(a: Array<T, 2>, b: Array<T, 2>) -> bool { a == b }")
        .assert_infer_code("type_mismatch");
}

#[test]
fn array_new_with_turbofish() {
    infer("Array.new<int, 5>()").assert_type(array_type(5, int_type()));
}

#[test]
fn array_new_infers_size_from_annotation() {
    infer("let a: Array<int, 3> = Array.new(); a").assert_last_type(array_type(3, int_type()));
}

#[test]
fn array_new_without_size_errors() {
    infer("Array.new()").assert_infer_code("array_new_cannot_infer_size");
}

#[test]
fn array_new_non_literal_size_errors() {
    infer("Array.new<int, int>()").assert_infer_code("array_size_not_literal");
}

#[test]
fn array_new_size_above_int_max_is_rejected() {
    infer("Array.new<int, 18000000000000000000>()").assert_infer_code("array_size_too_large");
}

#[test]
fn array_new_wrong_arity_errors() {
    infer("Array.new<int>()").assert_infer_code("array_type_arity");
}

#[test]
fn array_new_rejects_value_arguments() {
    infer("Array.new<int, 3>(5)").assert_infer_code("array_new_takes_no_arguments");
}

#[test]
fn array_new_element_without_zero_errors() {
    infer("Array.new<Channel<int>, 2>()").assert_infer_code("array_new_no_zero");
}

#[test]
fn array_new_ref_element_without_zero_errors() {
    // `Ref<T>` has no zero; only an `Option<Ref<T>>` (zero = None) is fillable.
    infer("Array.new<Ref<int>, 2>()").assert_infer_code("array_new_no_zero");
}

#[test]
fn zero_length_array_of_zeroless_element_is_zeroable() {
    infer("struct S { a: Array<Ref<int>, 0>, b: int }\nfn f() { let _ = S { b: 1, .. } }")
        .assert_no_errors();
}

#[test]
fn array_new_zero_length_zeroless_element_is_ok() {
    infer("Array.new<Ref<int>, 0>()").assert_no_errors();
}

#[test]
fn array_new_checks_distinct_instantiations_of_one_generic() {
    infer(
        "struct Box<T> { value: T }\nstruct Pair { a: Box<int>, b: Box<Ref<int>> }\nfn f() { let _ = Array.new<Pair, 2>() }",
    )
    .assert_infer_code("array_new_no_zero");
}

#[test]
fn array_new_checks_nested_same_generic_tail() {
    infer("struct Box<T> { value: T }\nfn f() { let _ = Array.new<Box<Box<Ref<int>>>, 1>() }")
        .assert_infer_code("array_new_no_zero");
}

#[test]
fn array_is_reserved_as_import_alias() {
    let mut fs = MockFileSystem::new();
    fs.add_file("arr", "arr.lis", "pub fn new() -> int { 0 }\n");
    fs.add_file(
        "main",
        "main.lis",
        "import Array \"arr\"\nfn f() -> int { Array.new() }\n",
    );
    infer_module("main", fs).assert_resolve_code("reserved_import_alias");
}

#[test]
fn array_new_distinguishes_same_named_cross_module_types() {
    let mut fs = MockFileSystem::new();
    fs.add_file("b", "b.lis", "pub struct Box<T> { pub r: Ref<T> }\n");
    fs.add_file(
        "main",
        "main.lis",
        "import \"b\"\nstruct Box<T> { inner: b.Box<T> }\nfn f() { let _ = Array.new<Box<int>, 1>() }\n",
    );
    infer_module("main", fs).assert_infer_code("array_new_no_zero");
}

#[test]
fn array_new_zero_length_zeroless_element_from_annotation_is_ok() {
    infer("let x: Array<Ref<int>, 0> = Array.new(); x").assert_no_errors();
}

#[test]
fn array_for_loop_binds_element_type() {
    // `_y: int = x` only type-checks if the loop variable is inferred as `int`.
    infer("let arr: Array<int, 3> = [1, 2, 3]; for x in arr { let _y: int = x }")
        .assert_no_errors();
}

#[test]
fn array_for_loop_element_type_mismatch() {
    infer("let arr: Array<int, 3> = [1, 2, 3]; for x in arr { let _y: string = x }")
        .assert_infer_code("type_mismatch");
}

#[test]
fn zero_length_array() {
    infer("let xs: Array<int, 0> = []; xs").assert_last_type(array_type(0, int_type()));
}

#[test]
fn integer_in_type_position_errors() {
    infer("let xs: Slice<3> = []; xs").assert_infer_code("integer_in_type_position");
}

#[test]
fn index_through_ref_deref() {
    infer("let arr: Array<int, 3> = [1, 2, 3]; let r = &arr; r.*[0]").assert_last_type(int_type());
}

#[test]
fn slice_of_arrays_element_is_array() {
    infer("let xs: Slice<Array<int, 3>> = [[1, 2, 3]]; xs[0]")
        .assert_last_type(array_type(3, int_type()));
}

#[test]
fn map_value_array_index_is_array() {
    infer("let m: Map<string, Array<int, 3>> = Map.new(); m[\"k\"]")
        .assert_last_type(array_type(3, int_type()));
}

#[test]
fn comparable_array_map_key_indexing() {
    infer("let m: Map<Array<int, 2>, string> = Map.new(); m[[1, 2]]")
        .assert_last_type(string_type());
}

#[test]
fn array_deep_alias_cast_peels_element() {
    infer("type MyInt = int\nfn g(a: Array<int, 2>) {}\nfn f(a: Array<MyInt, 2>) { g(a as Array<int, 2>) }")
        .assert_no_errors();
}

#[test]
fn generic_array_map_key_requires_comparable_bound() {
    infer("fn f<T>(m: Map<Array<T, 2>, int>) {}").assert_infer_code_once("missing_map_key_bound");
}

#[test]
fn bounded_generic_array_map_key_is_allowed() {
    infer("fn f<T: Comparable>(m: Map<Array<T, 2>, int>) -> int { m.length() }").assert_no_errors();
}

#[test]
fn inferred_generic_map_key_requires_comparable_bound() {
    infer("fn f<T>() { Map.new<T, int>() }").assert_infer_code_once("missing_map_key_bound");
}

#[test]
fn late_inferred_generic_map_key_requires_comparable_bound() {
    infer(
        r#"
fn f<T>(key: T) {
  let mut values = Map.new()
  values[key] = 1
}
        "#,
    )
    .assert_infer_code_once("missing_map_key_bound");
}

#[test]
fn late_inferred_bounded_map_key_is_allowed() {
    infer(
        r#"
fn f<T: Comparable>(key: T) {
  let mut values = Map.new()
  values[key] = 1
}
        "#,
    )
    .assert_no_errors();
}

#[test]
fn interface_like_map_key_component_does_not_hide_missing_bound() {
    infer("fn f<T>(m: Map<(Unknown, T), int>) {}").assert_infer_code_once("missing_map_key_bound");
}

#[test]
fn default_import_named_array_is_reserved() {
    let mut fs = MockFileSystem::new();
    fs.add_file("Array", "Array.lis", "pub fn new() -> int { 0 }\n");
    fs.add_file(
        "main",
        "main.lis",
        "import \"Array\"\nfn f() -> int { Array.new() }\n",
    );
    infer_module("main", fs).assert_resolve_code("reserved_import_alias");
}

#[test]
fn non_comparable_array_alias_rejected_as_map_key() {
    infer("type BadKey = Array<Slice<int>, 2>\nfn f(m: Map<BadKey, int>) {}")
        .assert_infer_code("non_comparable_map_key");
}

#[test]
fn array_to_slice_returns_slice_of_element() {
    infer("let xs: Array<int, 3> = [1, 2, 3]; xs.to_slice()")
        .assert_last_type(slice_type(int_type()));
}

#[test]
fn array_get_returns_option_of_element() {
    infer("let xs: Array<int, 3> = [1, 2, 3]; xs.get(0)")
        .assert_type_struct_generic("Option", vec![int_type()]);
}

// A generic-param size is valid only in the prelude `Array` impl; in user code it
// must error cleanly, not mint the nominal that leaks into emit and crashes.
#[test]
fn user_generic_param_array_size_errors_not_ice() {
    infer("fn first<SIZE>(a: Array<int, SIZE>) -> int { 0 }")
        .assert_infer_code("array_size_not_literal");
}

#[test]
fn user_generic_param_array_size_in_struct_field_errors() {
    infer("struct Buf<SIZE> { data: Array<int, SIZE> }")
        .assert_infer_code("array_size_not_literal");
}

#[test]
fn array_full_length_destructure_is_irrefutable() {
    infer("fn f(arr: Array<int, 3>) -> int { let [a, b, c] = arr; a + b + c }").assert_no_errors();
}

#[test]
fn array_match_full_length_is_exhaustive() {
    infer("fn f(arr: Array<int, 3>) -> int { match arr { [a, b, c] => a + b + c } }")
        .assert_no_errors();
}

#[test]
fn array_pattern_too_few_elements_errors() {
    infer("fn f(arr: Array<int, 3>) -> int { let [a, b] = arr; a }")
        .assert_infer_code("array_pattern_length_mismatch");
}

#[test]
fn array_match_literal_element_is_not_exhaustive() {
    infer("fn f(arr: Array<int, 2>) -> int { match arr { [0, y] => y } }")
        .assert_infer_code("non_exhaustive");
}

#[test]
fn array_rest_pattern_binds_sub_array() {
    infer("{ let arr: Array<int, 3> = [1, 2, 3]; let [_first, ..rest] = arr; rest }")
        .assert_last_type(array_type(2, int_type()));
}

#[test]
fn nested_array_destructure() {
    infer("fn f(m: Array<Array<int, 2>, 2>) -> int { let [[a, b], [c, d]] = m; a + b + c + d }")
        .assert_no_errors();
}

#[test]
fn array_alias_destructure_peels_to_array() {
    infer("type Vec3 = Array<int, 3>\nfn f(v: Vec3) -> int { let [a, b, c] = v; a + b + c }")
        .assert_no_errors();
}

#[test]
fn huge_array_rest_pattern_terminates() {
    infer("fn f(arr: Array<int, 2000000000>) -> int { let [head, ..rest] = arr; head }")
        .assert_no_errors();
}

#[test]
fn array_rest_arm_makes_full_arm_redundant() {
    infer("fn f(arr: Array<int, 3>) -> int { match arr { [_, ..] => 1, [a, b, c] => 2 } }")
        .assert_infer_code("redundant_arm");
}

#[test]
fn large_array_literal_arm_is_not_exhaustive() {
    infer("fn f(arr: Array<int, 1000>) -> int { match arr { [0, ..] => 1 } }")
        .assert_infer_code("non_exhaustive");
}
