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
        .assert_infer_code("param_needs_comparable_bound");
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
    infer_package("main", fs).assert_resolve_code("reserved_import_alias");
}

#[test]
fn array_new_distinguishes_same_named_cross_package_types() {
    let mut fs = MockFileSystem::new();
    fs.add_file("b", "b.lis", "pub struct Box<T> { pub r: Ref<T> }\n");
    fs.add_file(
        "main",
        "main.lis",
        "import \"b\"\nstruct Box<T> { inner: b.Box<T> }\nfn f() { let _ = Array.new<Box<int>, 1>() }\n",
    );
    infer_package("main", fs).assert_infer_code("array_new_no_zero");
}

#[test]
fn array_new_zero_length_zeroless_element_from_annotation_is_ok() {
    infer("let x: Array<Ref<int>, 0> = Array.new(); x").assert_no_errors();
}

#[test]
fn array_from_with_turbofish() {
    infer("Array.from<int, 3>([1, 2, 3])")
        .assert_type_struct_generic("Option", vec![array_type(3, int_type())]);
}

#[test]
fn array_from_infers_size_from_annotation() {
    infer("let a: Option<Array<int, 3>> = Array.from([1, 2, 3]); a")
        .assert_last_type(con_type("Option", vec![array_type(3, int_type())]));
}

#[test]
fn array_from_without_size_errors() {
    infer("Array.from([1, 2, 3])").assert_infer_code("array_from_cannot_infer_size");
}

#[test]
fn array_from_bare_array_annotation_does_not_infer_size() {
    infer("let a: Array<int, 3> = Array.from([1, 2, 3]); a")
        .assert_infer_code("array_from_cannot_infer_size");
}

#[test]
fn array_from_wrong_arity_errors() {
    infer("Array.from<int>([1, 2, 3])").assert_infer_code("array_type_arity");
}

#[test]
fn array_from_non_literal_size_errors() {
    infer("Array.from<int, int>([1, 2, 3])").assert_infer_code("array_size_not_literal");
}

#[test]
fn array_from_without_arguments_errors() {
    infer("Array.from<int, 3>()").assert_infer_code("array_from_takes_one_argument");
}

#[test]
fn array_from_with_extra_argument_errors() {
    infer("Array.from<int, 3>([1, 2, 3], [4])").assert_infer_code("array_from_takes_one_argument");
}

#[test]
fn array_from_rejects_spread() {
    infer("Array.from<int, 3>([1, 2, 3]...)").assert_infer_code("spread_on_non_variadic");
}

#[test]
fn array_from_spread_does_not_also_count_arguments() {
    infer("Array.from<int, 3>([1, 2, 3]...)")
        .assert_infer_code_count("array_from_takes_one_argument", 0);
}

#[test]
fn array_from_still_resolves_names_inside_a_rejected_spread() {
    infer("Array.from<int, 3>(missing...)").assert_resolve_code("name_not_found");
}

#[test]
fn array_from_rejects_non_slice_argument() {
    infer("Array.from<int, 3>(\"nope\")").assert_infer_code("type_mismatch");
}

#[test]
fn array_from_element_without_zero_is_allowed() {
    infer("fn f(xs: Slice<Ref<int>>) -> Option<Array<Ref<int>, 2>> { Array.from(xs) }")
        .assert_no_errors();
}

#[test]
fn array_from_zero_length_is_ok() {
    infer("fn f(xs: Slice<int>) -> Option<Array<int, 0>> { Array.from(xs) }").assert_no_errors();
}

#[test]
fn array_from_named_constant_size() {
    infer("const SIZE = 3\nfn f(xs: Slice<int>) -> Option<Array<int, SIZE>> { Array.from(xs) }")
        .assert_no_errors();
}

#[test]
fn slice_to_array_mismatch_suggests_array_from() {
    infer("let _a: Array<int, 3> = [1, 2, 3][0..3]").assert_error_contains("`Array.from(...)`");
}

#[test]
fn unknown_array_static_is_not_an_alias_error() {
    infer("Array.zzz([1, 2, 3])").assert_infer_code("unknown_native_static");
}

#[test]
fn unknown_slice_static_is_not_an_alias_error() {
    infer("Slice.zzz([1, 2, 3])").assert_infer_code("unknown_native_static");
}

#[test]
fn array_from_as_a_value_is_a_constructor_error() {
    infer("let f = Array.from; f").assert_infer_code("native_constructor_value");
}

#[test]
fn array_new_as_a_value_is_a_constructor_error() {
    infer("let f = Array.new; f").assert_infer_code("native_constructor_value");
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
    infer_package("main", fs).assert_resolve_code("reserved_import_alias");
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

#[test]
fn array_size_from_constant() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "main",
        "main.lis",
        "const SIZE = 3\nfn f(xs: Array<int, SIZE>) -> int { xs.length() }\n",
    );
    infer_package("main", fs).assert_no_errors();
}

#[test]
fn array_size_from_constant_declared_later() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "main",
        "main.lis",
        "fn f(xs: Array<int, SIZE>) -> int { xs.length() }\nconst SIZE = 3\n",
    );
    infer_package("main", fs).assert_no_errors();
}

#[test]
fn array_size_constant_reaches_struct_field_and_alias() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "main",
        "main.lis",
        "const SIZE = 3\nstruct Holder { pub data: Array<int, SIZE> }\ntype Buf = Array<int, SIZE>\nfn f(b: Buf) -> Holder { Holder { data: b } }\n",
    );
    infer_package("main", fs).assert_no_errors();
}

#[test]
fn array_size_from_constant_in_another_package() {
    let mut fs = MockFileSystem::new();
    fs.add_file("sizes", "sizes.lis", "pub const WIDTH = 4\n");
    fs.add_file(
        "main",
        "main.lis",
        "import \"sizes\"\nfn f(xs: Array<int, sizes.WIDTH>) -> int { xs.length() }\n",
    );
    infer_package("main", fs).assert_no_errors();
}

#[test]
fn array_size_constant_rejects_wrong_literal_length() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "main",
        "main.lis",
        "const SIZE = 3\nfn f() -> Array<int, SIZE> { [1, 2] }\n",
    );
    infer_package("main", fs).assert_infer_code("array_literal_length_mismatch");
}

#[test]
fn array_new_turbofish_takes_a_constant() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "main",
        "main.lis",
        "const SIZE = 3\nfn f() -> Array<int, 3> { Array.new<int, SIZE>() }\n",
    );
    infer_package("main", fs).assert_no_errors();
}

#[test]
fn array_size_constant_not_found() {
    infer("let xs: Array<int, NOPE> = [1]; xs").assert_infer_code("array_size_unknown_constant");
}

#[test]
fn array_size_constant_from_computed_initializer() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "main",
        "main.lis",
        "const SIZE = 2 + 1\nfn f(xs: Array<int, SIZE>) -> int { xs.length() }\n",
    );
    infer_package("main", fs).assert_infer_code("array_size_computed_constant");
}

#[test]
fn array_size_constant_must_be_an_integer() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "main",
        "main.lis",
        "const SIZE = \"three\"\nfn f(xs: Array<int, SIZE>) -> int { xs.length() }\n",
    );
    infer_package("main", fs).assert_infer_code("array_size_not_integer_constant");
}

#[test]
fn array_size_constant_must_not_be_negative() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "main",
        "main.lis",
        "const SIZE = -2\nfn f(xs: Array<int, SIZE>) -> int { xs.length() }\n",
    );
    infer_package("main", fs).assert_infer_code("array_size_negative_constant");
}

#[test]
fn array_size_constant_must_not_be_function_local() {
    infer("fn f() -> int { const SIZE = 3; let xs: Array<int, SIZE> = [1, 2, 3]; xs.length() }")
        .assert_infer_code("array_size_local_constant");
}

#[test]
fn array_size_rejects_a_runtime_value() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "main",
        "main.lis",
        "fn size() -> int { 3 }\nfn f(xs: Array<int, size>) -> int { xs.length() }\n",
    );
    infer_package("main", fs).assert_infer_code("array_size_not_constant");
}

#[test]
fn array_size_constant_above_int_max_is_rejected() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "main",
        "main.lis",
        "const SIZE: uint = 18000000000000000000\nfn f(xs: Array<int, SIZE>) -> int { xs.length() }\n",
    );
    infer_package("main", fs).assert_infer_code("array_size_too_large");
}

#[test]
fn array_size_constant_with_a_float_type_is_rejected() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "main",
        "main.lis",
        "const SIZE: float64 = 3\nfn f(xs: Array<int, SIZE>) -> int { xs.length() }\n",
    );
    infer_package("main", fs).assert_infer_code("array_size_not_integer_constant");
}

#[test]
fn array_size_constant_typed_by_a_numeric_newtype() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "main",
        "main.lis",
        "struct Width(int)\nconst SIZE: Width = 3\nfn f(xs: Array<int, SIZE>) -> int { xs.length() }\n",
    );
    infer_package("main", fs).assert_no_errors();
}

#[test]
fn array_size_constant_typed_by_a_newtype_declared_later() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "main",
        "main.lis",
        "type Buf = Array<int, SIZE>\nstruct Width(int)\nconst SIZE: Width = 3\nfn f(b: Buf) -> int { b.length() }\n",
    );
    infer_package("main", fs).assert_no_errors();
}

#[test]
fn array_size_constant_typed_by_a_nested_numeric_newtype() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "main",
        "main.lis",
        "struct Inner(int)\nstruct Outer(Inner)\nconst SIZE: Outer = 3\nfn f(xs: Array<int, SIZE>) -> int { xs.length() }\n",
    );
    infer_package("main", fs).assert_no_errors();
}

#[test]
fn array_size_constant_typed_by_a_uintptr_newtype() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "main",
        "main.lis",
        "struct Handle(uintptr)\nconst SIZE: Handle = 3\nfn f(xs: Array<int, SIZE>) -> int { xs.length() }\n",
    );
    infer_package("main", fs).assert_no_errors();
}

#[test]
fn array_size_constant_typed_by_a_uintptr_alias() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "main",
        "main.lis",
        "type Slot = uintptr\nconst SIZE: Slot = 3\nfn f(xs: Array<int, SIZE>) -> int { xs.length() }\n",
    );
    infer_package("main", fs).assert_no_errors();
}

#[test]
fn array_size_constant_typed_by_a_rune() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "main",
        "main.lis",
        "const SIZE: rune = 3\nfn f(xs: Array<int, SIZE>) -> int { xs.length() }\n",
    );
    infer_package("main", fs).assert_no_errors();
}

#[test]
fn array_size_rejects_a_function_parameter() {
    infer("fn f(n: int) -> int { let xs: Array<int, n> = [1]; xs.length() }")
        .assert_infer_code("array_size_not_constant");
}

#[test]
fn array_size_rejects_a_local_binding() {
    infer("fn f() -> int { let m = 2; let xs: Array<int, m> = [1]; xs.length() }")
        .assert_infer_code("array_size_not_constant");
}

#[test]
fn array_size_constant_typed_by_a_float_newtype_is_rejected() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "main",
        "main.lis",
        "struct Weird(float64)\nconst SIZE: Weird = 3\nfn f(xs: Array<int, SIZE>) -> int { xs.length() }\n",
    );
    infer_package("main", fs).assert_infer_code("array_size_not_integer_constant");
}

#[test]
fn array_size_constant_typed_by_an_alias_declared_later() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "main",
        "main.lis",
        "type Buf = Array<int, SIZE>\ntype Width = int\nconst SIZE: Width = 3\nfn f(b: Buf) -> int { b.length() }\n",
    );
    infer_package("main", fs).assert_no_errors();
}

#[test]
fn array_size_constant_typed_by_an_alias_declared_first() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "main",
        "main.lis",
        "type Width = int\ntype Buf = Array<int, SIZE>\nconst SIZE: Width = 3\nfn f(b: Buf) -> int { b.length() }\n",
    );
    infer_package("main", fs).assert_no_errors();
}

#[test]
fn array_size_constant_typed_by_a_later_float_alias_is_rejected() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "main",
        "main.lis",
        "type Buf = Array<int, SIZE>\ntype Weird = float64\nconst SIZE: Weird = 3\nfn f(b: Buf) -> int { b.length() }\n",
    );
    infer_package("main", fs).assert_infer_code("array_size_not_integer_constant");
}

#[test]
fn array_size_constant_typed_by_an_alias_in_another_file() {
    let mut fs = MockFileSystem::new();
    fs.add_file("main", "width.lis", "pub type Width = int\n");
    fs.add_file("main", "size.lis", "pub const SIZE: Width = 3\n");
    fs.add_file(
        "main",
        "main.lis",
        "type Buf = Array<int, SIZE>\nfn f(b: Buf) -> int { b.length() }\n",
    );
    infer_package("main", fs).assert_no_errors();
}

#[test]
fn array_size_constant_through_an_integer_alias() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "main",
        "main.lis",
        "type Width = int\nconst SIZE: Width = 3\nfn f(xs: Array<int, SIZE>) -> int { xs.length() }\n",
    );
    infer_package("main", fs).assert_no_errors();
}

#[test]
fn array_size_constant_with_a_narrow_integer_type() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "main",
        "main.lis",
        "const SIZE: int8 = 3\nfn f(xs: Array<int, SIZE>) -> int { xs.length() }\n",
    );
    infer_package("main", fs).assert_no_errors();
}

#[test]
fn array_size_rejects_a_private_constant_from_another_package() {
    let mut fs = MockFileSystem::new();
    fs.add_file("sizes", "sizes.lis", "const WIDTH = 4\n");
    fs.add_file(
        "main",
        "main.lis",
        "import \"sizes\"\nfn f(xs: Array<int, sizes.WIDTH>) -> int { xs.length() }\n",
    );
    infer_package("main", fs).assert_infer_code("array_size_unknown_constant");
}

#[test]
fn array_size_rejects_a_test_only_constant_from_a_production_file() {
    let mut fs = MockFileSystem::new();
    fs.add_file("main", "main.test.lis", "const TEST_SIZE = 4\n");
    fs.add_file(
        "main",
        "main.lis",
        "fn f(xs: Array<int, TEST_SIZE>) -> int { xs.length() }\n",
    );
    infer_package("main", fs).assert_infer_code("array_size_unknown_constant");
}

#[test]
fn array_new_default_variant_enum_is_zeroable() {
    infer("enum Colour { #[default] Red, Green }\nfn f() { let _ = Array.new<Colour, 3>() }")
        .assert_no_errors();
}

#[test]
fn array_new_enum_without_default_still_errors() {
    infer("enum Colour { Red, Green }\nfn f() { let _ = Array.new<Colour, 3>() }")
        .assert_infer_code("array_new_no_zero");
}

#[test]
fn slice_make_default_variant_enum_is_zeroable() {
    infer("enum Colour { #[default] Red, Green }\nfn f() { let _ = Slice.make<Colour>(3) }")
        .assert_no_errors();
}

#[test]
fn struct_spread_fills_default_variant_field() {
    infer(
        "enum Colour { #[default] Red, Green }\nstruct Paint { name: string, shade: Colour }\nfn f() { let _ = Paint { name: \"a\", .. } }",
    )
    .assert_no_errors();
}

#[test]
fn map_read_of_default_variant_enum_is_allowed() {
    infer(
        "enum Colour { #[default] Red, Green }\nfn f(m: Map<string, Colour>) -> Colour { m[\"a\"] }",
    )
    .assert_no_errors();
}

#[test]
fn default_variant_reaches_through_a_struct_field() {
    infer(
        "enum Colour { #[default] Red, Green }\nstruct Paint { shade: Colour }\nfn f() { let _ = Array.new<Paint, 2>() }",
    )
    .assert_no_errors();
}

#[test]
fn generic_enum_may_mark_a_payload_less_variant() {
    infer("enum Cached<T> { #[default] Miss, Hit(T) }\nfn f() { let _ = Array.new<Cached<int>, 2>() }")
        .assert_no_errors();
}

#[test]
fn default_variant_in_typedef_is_rejected() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "gen",
        "gen.d.lis",
        "pub enum Mode { #[default] Fast, Slow }\n",
    );
    fs.add_file(
        "main",
        "main.lis",
        "import \"gen\"\nfn f() { let _ = gen.Mode.Fast }\n",
    );
    infer_package("main", fs).assert_error_contains("#[default]` in a typedef");
}
