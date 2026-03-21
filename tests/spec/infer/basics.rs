use crate::spec::infer::*;

#[test]
fn integer_literal() {
    infer("1").assert_type_int();
}

#[test]
fn boolean_true() {
    infer("true").assert_type_bool();
}

#[test]
fn boolean_false() {
    infer("false").assert_type_bool();
}

#[test]
fn string_literal() {
    infer(r#""hello""#).assert_type_string();
}

#[test]
fn float_literal() {
    infer("3.14").assert_type_float();
}

#[test]
fn char_literal() {
    infer("'a'").assert_type_char();
}

#[test]
fn char_literal_unicode() {
    infer("'🔥'").assert_type_char();
}

#[test]
fn char_literal_as_uint8() {
    infer("let x: uint8 = 'f'; x").assert_type(con_type("uint8", vec![]));
}

#[test]
fn const_used_in_function() {
    infer(
        r#"{
    const MAX = 100;

    let get_max = || -> int { MAX };

    get_max()
    }"#,
    )
    .assert_last_type(int_type());
}

#[test]
fn const_in_arithmetic() {
    infer(
        r#"{
    const BASE = 50;

    let calculate = || -> int { BASE * 2 + 10 };

    calculate()
    }"#,
    )
    .assert_last_type(int_type());
}

#[test]
fn multiple_constants() {
    infer(
        r#"{
    const A = 10;
    const B = 20;

    let add = || -> int { A + B };

    add()
    }"#,
    )
    .assert_last_type(int_type());
}

#[test]
fn const_bool() {
    infer(
        r#"{
    const IS_DEBUG = true;

    let get_debug = || -> bool { IS_DEBUG };

    get_debug()
    }"#,
    )
    .assert_last_type(bool_type());
}

#[test]
fn const_string() {
    infer(
        r#"{
    const NAME = "lisette";

    let get_name = || -> string { NAME };

    get_name()
    }"#,
    )
    .assert_last_type(string_type());
}

#[test]
fn const_float() {
    infer(
        r#"{
    const PI = 3.14;

    let get_pi = || -> float64 { PI };

    get_pi()
    }"#,
    )
    .assert_last_type(float_type());
}

#[test]
fn const_with_explicit_annotation() {
    infer(
        r#"{
    const MAX: int = 100;

    let get_max = || -> int { MAX };

    get_max()
    }"#,
    )
    .assert_last_type(int_type());
}

#[test]
fn const_with_expression() {
    infer(
        r#"{
    const RESULT = 10 + 20;
    RESULT
    }"#,
    )
    .assert_type_int();
}

#[test]
fn const_negative_integer() {
    infer(
        r#"{
    const MIN = -128;
    MIN
    }"#,
    )
    .assert_type_int();
}

#[test]
fn const_rejects_function_call() {
    infer(
        r#"{
    fn noop() {}
    const U = noop();
    U
    }"#,
    )
    .assert_infer_code("const_requires_simple_expression");
}

#[test]
fn format_string_simple() {
    infer(r#"f"hello world""#).assert_type_string();
}

#[test]
fn format_string_with_variable() {
    let input = r#"
let name = "Alice";
f"hello {name}"
"#;
    infer(input).assert_type_string();
}

#[test]
fn format_string_multiple_interpolations() {
    let input = r#"
let first = "John";
let last = "Doe";
f"hello {first} {last}"
"#;
    infer(input).assert_type_string();
}

#[test]
fn format_string_empty() {
    infer(r#"f"""#).assert_type_string();
}

#[test]
fn format_string_mixed_types() {
    let input = r#"
let name = "Alice";
let age = 30;
let height = 5.7;
f"{name} is {age} years old and {height} feet tall"
"#;
    infer(input).assert_type_string();
}

#[test]
fn tuple_two_elements() {
    infer("(1, 2)").assert_type_tuple(int_type(), int_type());
}

#[test]
fn tuple_mixed_types() {
    infer("(1, \"hello\")").assert_type_tuple(int_type(), string_type());
}

#[test]
fn empty_tuple() {
    infer("()").assert_type_unit();
}

#[test]
fn nested_tuple() {
    infer("((1, 2), 3)").assert_type_tuple(tuple_type(vec![int_type(), int_type()]), int_type());
}

#[test]
fn tuple_in_block() {
    infer("{ let pair = (1, 2); pair }").assert_type_tuple(int_type(), int_type());
}

#[test]
fn tuple_with_expressions() {
    infer("(1 + 2, 3 * 4)").assert_type_tuple(int_type(), int_type());
}

#[test]
fn tuple_pattern_let_destructure() {
    infer("{ let (a, b) = (1, 2); a }").assert_type_int();
}

#[test]
fn tuple_pattern_size_mismatch() {
    infer("{ let (a, b, c) = (1, 2); a }").assert_infer_code("tuple_element_count_mismatch");
}

#[test]
fn tuple_pattern_let_destructure_second_element() {
    infer("{ let (a, b) = (1, 2); b }").assert_type_int();
}

#[test]
fn tuple_pattern_mixed_types() {
    infer("{ let (x, y) = (42, \"hello\"); x }").assert_type_int();
}

#[test]
fn tuple_pattern_mixed_types_second() {
    infer("{ let (x, y) = (42, \"hello\"); y }").assert_type_string();
}

#[test]
fn tuple_pattern_nested() {
    infer("{ let ((a, b), c) = ((1, 2), 3); a }").assert_type_int();
}

#[test]
fn tuple_pattern_nested_inner() {
    infer("{ let ((a, b), c) = ((1, 2), 3); b }").assert_type_int();
}

#[test]
fn tuple_pattern_nested_outer() {
    infer("{ let ((a, b), c) = ((1, 2), 3); c }").assert_type_int();
}

#[test]
fn tuple_pattern_in_match() {
    infer(
        r#"
    {
      let pair = (1, 2);
      match pair {
        (x, y) => x
      }
    }
        "#,
    )
    .assert_type_int();
}

#[test]
fn tuple_pattern_in_match_second_element() {
    infer(
        r#"
    {
      let pair = (1, 2);
      match pair {
        (x, y) => y
      }
    }
        "#,
    )
    .assert_type_int();
}

#[test]
fn tuple_pattern_match_literal() {
    infer(
        r#"
    {
      let pair = (1, 2);
      match pair {
        (1, 2) => true,
        _ => false
      }
    }
        "#,
    )
    .assert_type_bool();
}

#[test]
fn empty_slice() {
    infer("[]").assert_type_empty_slice();
}

#[test]
fn slice_homogeneous_int() {
    infer("[1, 2, 3]").assert_type_slice_of_ints();
}

#[test]
fn slice_homogeneous_string() {
    infer(r#"["hello", "world"]"#).assert_type_slice_of_strings();
}

#[test]
fn slice_homogeneous_bool() {
    infer("[true, false, true]").assert_type_slice_of_booleans();
}

#[test]
fn slice_single_element() {
    infer("[42]").assert_type_slice_of_ints();
}

#[test]
fn slice_with_expressions() {
    infer("[1 + 2, 3 * 4, 5 - 1]").assert_type_slice_of_ints();
}

#[test]
fn slice_in_block() {
    infer("{ let nums = [1, 2, 3]; nums }").assert_type_slice_of_ints();
}

#[test]
fn nested_slice() {
    infer("[[1, 2], [3, 4]]").assert_type_slice_of(slice_type(int_type()));
}

#[test]
fn slice_mixed_types_should_error() {
    infer(r#"[1, "hello"]"#).assert_type_mismatch();
}

#[test]
fn slice_with_boolean_expressions() {
    infer("[1 > 2, true, false]").assert_type_slice_of_booleans();
}

#[test]
fn deeply_nested_slice() {
    infer("[[[1, 2]], [[3, 4]]]").assert_type_slice_of(slice_type(slice_type(int_type())));
}

#[test]
fn slice_literal_adapts_int8_element_type() {
    infer("{ let arr: Slice<int8> = [1, 2, 3]; arr }").assert_type_slice_of(int8_type());
}

#[test]
fn slice_literal_adapts_int16_element_type() {
    infer("{ let arr: Slice<int16> = [100, 200, 300]; arr }").assert_type_slice_of(int16_type());
}

#[test]
fn slice_literal_adapts_float32_element_type() {
    infer("{ let arr: Slice<float32> = [1.5, 2.5, 3.5]; arr }")
        .assert_type_slice_of(float32_type());
}

#[test]
fn slice_literal_adapts_negative_int8_elements() {
    infer("{ let arr: Slice<int8> = [-1, -2, -3]; arr }").assert_type_slice_of(int8_type());
}

#[test]
fn enum_variant_unqualified() {
    infer(
        r#"{
    enum Color { Red, Green, Blue }
    Red
    }"#,
    )
    .assert_no_errors();
}

#[test]
fn enum_variant_qualified() {
    infer(
        r#"{
    enum Color { Red, Green, Blue }
    Color.Red
    }"#,
    )
    .assert_no_errors();
}

#[test]
fn enum_variant_with_payload_unqualified() {
    infer(
        r#"{
    Some(42)
    }"#,
    )
    .assert_no_errors();
}

#[test]
fn enum_variant_with_payload_qualified() {
    infer(
        r#"{
    Option.Some(42)
    }"#,
    )
    .assert_no_errors();
}

#[test]
fn enum_pattern_match_unqualified() {
    infer(
        r#"{
    let opt = Some(42);
    match opt {
      Some(x) => x,
      None => 0,
    }
    }"#,
    )
    .assert_type_int();
}

#[test]
fn enum_pattern_match_qualified() {
    infer(
        r#"{
    let opt = Option.Some(42);
    match opt {
      Option.Some(x) => x,
      Option.None => 0,
    }
    }"#,
    )
    .assert_type_int();
}

#[test]
fn enum_pattern_match_mixed_qualified_unqualified() {
    infer(
        r#"{
    let opt = Some(42);
    match opt {
      Option.Some(x) => x,
      None => 0,
    }
    }"#,
    )
    .assert_type_int();
}

#[test]
fn nested_enum_variants() {
    infer(
        r#"{
    Some(Ok(42))
    }"#,
    )
    .assert_no_errors();
}

#[test]
fn enum_variant_in_function_call() {
    infer(
        r#"{
    let unwrap_or = |opt: Option<int>, fallback: int| -> int {
      match opt {
        Some(x) => x,
        None => fallback,
      }
    };
    unwrap_or(Some(42), 0)
    }"#,
    )
    .assert_type_int();
}

#[test]
fn enum_variant_constructor_type_inference() {
    infer(
        r#"{
    let x = Some(42);
    x
    }"#,
    )
    .assert_no_errors();
}

#[test]
fn enum_multiple_variants_same_name_different_enums() {
    infer(
        r#"{
    let r = Ok(42);
    let o = Some(true);
    (r, o)
    }"#,
    )
    .assert_no_errors();
}

#[test]
fn enum_pattern_destructuring() {
    infer(
        r#"{
    let result = Ok(42);
    match result {
      Ok(value) => value,
      Err(msg) => 0,
    }
    }"#,
    )
    .assert_type_int();
}

#[test]
fn enum_unit_variant() {
    infer(
        r#"{
    enum Status { Running, Stopped }
    let s = Running;
    match s {
      Running => "active",
      Stopped => "inactive",
    }
    }"#,
    )
    .assert_type_string();
}

#[test]
fn enum_variant_in_let_binding() {
    infer(
        r#"{
    enum Color { Red, Green, Blue }
    let c = Red;
    c
    }"#,
    )
    .assert_no_errors();
}

#[test]
fn enum_generic_inference() {
    infer(
        r#"{
    let id = |x: Option<int>| -> Option<int> { x };
    id(Some(42))
    }"#,
    )
    .assert_no_errors();
}

#[test]
fn enum_nested_pattern_match() {
    infer(
        r#"{
    let nested = Some(Ok(42));
    match nested {
      Some(Ok(x)) => x,
      Some(Err(_)) => 0,
      None => 0,
    }
    }"#,
    )
    .assert_type_int();
}

#[test]
fn enum_in_if_expression() {
    infer(
        r#"{
    let opt = Some(42);
    if true {
      Some(1)
    } else {
      None
    }
    }"#,
    )
    .assert_no_errors();
}

#[test]
fn enum_tuple_variant() {
    infer(
        r#"{
    enum IpAddress { V4(int, int, int, int), V6(string) }
    V4(192, 168, 1, 1)
    }"#,
    )
    .assert_no_errors();
}

#[test]
fn enum_variant_as_expression() {
    infer(
        r#"{
    let get_default = || -> Option<int> { None };
    get_default()
    }"#,
    )
    .assert_no_errors();
}

#[test]
fn enum_variant_wrong_qualified_name() {
    infer(
        r#"{
    Option.Ok(42)
    }"#,
    )
    .assert_not_found();
}

#[test]
fn enum_variant_undefined() {
    infer(
        r#"{
    enum Color { Red, Green }
    Blue
    }"#,
    )
    .assert_not_found();
}

#[test]
fn enum_pattern_undefined_variant() {
    infer(
        r#"{
    enum Color { Red, Green }
    let c = Red;
    match c {
      Red => 1,
      Blue => 2,
    }
    }"#,
    )
    .assert_not_found();
}

#[test]
fn enum_qualified_and_unqualified_in_pattern_arms() {
    infer(
        r#"{
    let r = Ok(42);
    match r {
      Result.Ok(x) => x,
      Err(_) => 0,
    }
    }"#,
    )
    .assert_type_int();
}

#[test]
fn enum_unqualified_construction_qualified_pattern() {
    infer(
        r#"{
    let opt = Some(42);
    match opt {
      Option.Some(x) => x,
      Option.None => 0,
    }
    }"#,
    )
    .assert_type_int();
}

#[test]
fn enum_qualified_construction_unqualified_pattern() {
    infer(
        r#"{
    let opt = Option.Some(42);
    match opt {
      Some(x) => x,
      None => 0,
    }
    }"#,
    )
    .assert_type_int();
}

#[test]
fn enum_with_numeric_like_variant() {
    infer(
        r#"{
    enum HttpStatus { Status200, Status404, Status500 }
    Status200
    }"#,
    )
    .assert_no_errors();
}

#[test]
fn simple_type_alias() {
    infer(
        r#"
    type UserId = int

    fn test() -> UserId {
      let x: UserId = 42;
      return x;
    }
        "#,
    )
    .assert_last_function_type(vec![], int_type());
}

#[test]
fn type_alias_in_function_param() {
    infer(
        r#"
    type UserId = int

    fn get_user(id: UserId) -> UserId {
      return id;
    }
        "#,
    )
    .assert_last_function_type(vec![int_type()], int_type());
}

#[test]
fn generic_type_alias_with_string_param() {
    infer(
        r#"
    type StringMap<V> = Map<string, V>

    fn test() -> StringMap<int> {
      return Map.new();
    }
        "#,
    )
    .assert_last_function_type(vec![], con_type("Map", vec![string_type(), int_type()]));
}

#[test]
fn generic_type_alias_foo_to_map() {
    infer(
        r#"
    type Foo<V> = Map<int, V>

    fn test() -> Foo<string> {
      return Map.new();
    }
        "#,
    )
    .assert_last_function_type(vec![], con_type("Map", vec![int_type(), string_type()]));
}

#[test]
fn nested_generic_type_alias() {
    infer(
        r#"
    type Foo<V> = Map<int, V>

    fn test() -> Foo<Foo<string>> {
      return Map.new();
    }
        "#,
    )
    .assert_last_function_type(
        vec![],
        con_type(
            "Map",
            vec![int_type(), con_type("Map", vec![int_type(), string_type()])],
        ),
    );
}

#[test]
fn type_alias_of_alias() {
    infer(
        r#"
    type ID = int
    type UserId = ID

    fn test() -> UserId {
      return 42;
    }
        "#,
    )
    .assert_last_function_type(vec![], int_type());
}

#[test]
fn multiple_generic_params_in_alias() {
    infer(
        r#"
    type Pair<A, B> = Map<A, B>

    fn test() -> Pair<int, string> {
      return Map.new();
    }
        "#,
    )
    .assert_last_function_type(vec![], con_type("Map", vec![int_type(), string_type()]));
}

#[test]
fn generic_alias_in_function_return() {
    infer(
        r#"
    type StringMap<V> = Map<string, V>

    fn create_map() -> StringMap<int> {
      return Map.new();
    }
        "#,
    )
    .assert_last_function_type(vec![], con_type("Map", vec![string_type(), int_type()]));
}

#[test]
fn type_alias_used_in_parameter() {
    infer(
        r#"
    type StringMap<V> = Map<string, V>

    fn process(m: StringMap<int>) -> int {
      return 42;
    }
        "#,
    )
    .assert_last_function_type(
        vec![con_type("Map", vec![string_type(), int_type()])],
        int_type(),
    );
}

#[test]
fn unused_generic_parameter() {
    infer(
        r#"
    type Ignore<T> = int

    fn test() -> Ignore<string> {
      return 42;
    }
        "#,
    )
    .assert_last_function_type(vec![], int_type());
}

#[test]
fn parameter_reuse() {
    infer(
        r#"
    type Mirror<T> = Map<T, T>

    fn test() -> Mirror<int> {
      return Map.new();
    }
        "#,
    )
    .assert_last_function_type(vec![], con_type("Map", vec![int_type(), int_type()]));
}

#[test]
fn parameter_order_swapping() {
    infer(
        r#"
    type Swapped<A, B> = Map<B, A>

    fn test() -> Swapped<int, string> {
      return Map.new();
    }
        "#,
    )
    .assert_last_function_type(vec![], con_type("Map", vec![string_type(), int_type()]));
}

#[test]
fn circular_type_alias_self_referential() {
    infer(
        r#"
    type T = T

    fn test() -> T {
      return 42;
    }
        "#,
    )
    .assert_circular_type();
}

#[test]
fn circular_type_alias_mutual() {
    infer(
        r#"
    type A = B
    type B = A

    fn test() -> A {
      return 42;
    }
        "#,
    )
    .assert_circular_type();
}

#[test]
fn reference_to_int() {
    infer(
        r#"
    {
      let x = 5;
      &x
    }
        "#,
    )
    .assert_type(ref_type(int_type()));
}

#[test]
fn reference_to_bool() {
    infer(
        r#"
    {
      let x = true;
      &x
    }
        "#,
    )
    .assert_type(ref_type(bool_type()));
}

#[test]
fn reference_to_string() {
    infer(
        r#"
    {
      let x = "hello";
      &x
    }
        "#,
    )
    .assert_type(ref_type(string_type()));
}

#[test]
fn reference_to_struct() {
    infer(
        r#"{
    struct Point {
      x: int,
      y: int,
    }

    let p = Point { x: 1, y: 2 };
    &p
    }"#,
    )
    .assert_last_type(ref_type(con_type("Point", vec![])));
}

#[test]
fn reference_to_tuple() {
    infer(
        r#"
    {
      let t = (1, true);
      &t
    }
        "#,
    )
    .assert_type(ref_type(tuple_type(vec![int_type(), bool_type()])));
}

#[test]
fn nested_references() {
    infer(
        r#"
    {
      let x = 5;
      let r1 = &x;
      &r1
    }
        "#,
    )
    .assert_type(ref_type(int_type()));
}

#[test]
fn both_references_unify_in_if() {
    infer(
        r#"
    {
      let a = 1;
      let b = 2;
      let result = if true {
        &a
      } else {
        &b
      };
      result
    }
        "#,
    )
    .assert_type(ref_type(int_type()));
}

#[test]
fn nested_refs_unify_in_if() {
    infer(
        r#"
    {
      let a = 1;
      let b = 2;
      let r1 = &a;
      let r2 = &b;
      if true {
        &r1
      } else {
        &r2
      }
    }
        "#,
    )
    .assert_type(ref_type(int_type()));
}

#[test]
fn reference_type_mismatch_in_if() {
    infer(
        r#"
    fn test() {
      let n = 42;
      let s = "hello";
      let x = if true {
        &n
      } else {
        &s
      };
      x
    }
        "#,
    )
    .assert_infer_code("type_mismatch");
}

#[test]
fn reference_vs_value_unifies() {
    infer(
        r#"
    {
      let n = 42;
      if true {
        &n
      } else {
        n
      }
    }
        "#,
    )
    .assert_type(ref_type(int_type()));
}

#[test]
fn user_defined_type_named_unit() {
    infer(
        r#"{
    enum Unit { Value }
    let x: Unit = Unit.Value
    x
    }"#,
    )
    .assert_type(con_type("Unit", vec![]));
}
