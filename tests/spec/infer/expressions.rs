use crate::spec::infer::*;

#[test]
fn addition() {
    infer("1 + 2").assert_type_int();
}

#[test]
fn subtraction() {
    infer("10 - 5").assert_type_int();
}

#[test]
fn multiplication() {
    infer("3 * 4").assert_type_int();
}

#[test]
fn division() {
    infer("10 / 2").assert_type_int();
}

#[test]
fn remainder() {
    infer("10 % 3").assert_type_int();
}

#[test]
fn equal() {
    infer("1 == 2").assert_type_bool();
}

#[test]
fn not_equal() {
    infer("1 != 2").assert_type_bool();
}

#[test]
fn less_than() {
    infer("1 < 2").assert_type_bool();
}

#[test]
fn less_than_or_equal() {
    infer("1 <= 2").assert_type_bool();
}

#[test]
fn greater_than() {
    infer("2 > 1").assert_type_bool();
}

#[test]
fn greater_than_or_equal() {
    infer("2 >= 1").assert_type_bool();
}

#[test]
fn logical_and() {
    infer("true && false").assert_type_bool();
}

#[test]
fn logical_or() {
    infer("true || false").assert_type_bool();
}

#[test]
fn nested_arithmetic() {
    infer("(1 + 2) * 3").assert_type_int();
}

#[test]
fn complex_comparison() {
    infer("(1 + 2) == (5 - 2)").assert_type_bool();
}

#[test]
fn chained_arithmetic() {
    infer("1 + 2 + 3 + 4").assert_type_int();
}

#[test]
fn string_concat_string() {
    infer(r#""hello" + "world""#).assert_type_string();
}

#[test]
fn string_concat_with_variables() {
    infer(
        r#"{
    let a = "hello";
    let b = "world";
    a + b
    }"#,
    )
    .assert_type_string();
}

#[test]
fn string_concat_empty_string() {
    infer(r#""" + "hello""#).assert_type_string();
}

#[test]
fn string_concat_chained() {
    infer(r#""a" + "b" + "c""#).assert_type_string();
}

#[test]
fn float_addition() {
    infer("3.14 + 2.71").assert_type_float();
}

#[test]
fn float_subtraction() {
    infer("10.5 - 3.2").assert_type_float();
}

#[test]
fn float_multiplication() {
    infer("2.5 * 4.0").assert_type_float();
}

#[test]
fn float_division() {
    infer("10.0 / 2.5").assert_type_float();
}

#[test]
fn string_equality() {
    infer(r#""hello" == "hello""#).assert_type_bool();
}

#[test]
fn string_inequality() {
    infer(r#""hello" != "world""#).assert_type_bool();
}

#[test]
fn string_less_than() {
    infer(r#""abc" < "def""#).assert_type_bool();
}

#[test]
fn string_less_than_or_equal() {
    infer(r#""abc" <= "abc""#).assert_type_bool();
}

#[test]
fn string_greater_than() {
    infer(r#""xyz" > "abc""#).assert_type_bool();
}

#[test]
fn string_greater_than_or_equal() {
    infer(r#""xyz" >= "xyz""#).assert_type_bool();
}

#[test]
fn bool_equality() {
    infer("true == false").assert_type_bool();
}

#[test]
fn bool_inequality() {
    infer("true != false").assert_type_bool();
}

#[test]
fn bool_less_than() {
    infer("false < true").assert_type_bool();
}

#[test]
fn bool_greater_than() {
    infer("true > false").assert_type_bool();
}

#[test]
fn float_equality() {
    infer("3.14 == 3.14").assert_type_bool();
}

#[test]
fn float_less_than() {
    infer("2.5 < 3.5").assert_type_bool();
}

#[test]
fn float_greater_than() {
    infer("5.0 > 2.0").assert_type_bool();
}

#[test]
fn int_comparison_with_variables() {
    infer(
        r#"{
    let x = 5;
    let y = 10;
    x < y
    }"#,
    )
    .assert_type_bool();
}

#[test]
fn string_comparison_with_variables() {
    infer(
        r#"{
    let a = "hello";
    let b = "world";
    a == b
    }"#,
    )
    .assert_type_bool();
}

#[test]
fn string_concat_int_error() {
    infer(r#""count: " + 42"#).assert_type_mismatch();
}

#[test]
fn int_concat_string_error() {
    infer(r#"42 + " items""#).assert_type_mismatch();
}

#[test]
fn string_concat_bool_error() {
    infer(r#""value: " + true"#).assert_type_mismatch();
}

#[test]
fn string_concat_float_error() {
    infer(r#""pi: " + 3.14"#).assert_type_mismatch();
}

#[test]
fn int_minus_string_error() {
    infer(r#"42 - "hello""#).assert_type_mismatch();
}

#[test]
fn string_multiply_int_error() {
    infer(r#""hello" * 3"#).assert_type_mismatch();
}

#[test]
fn bool_plus_int_error() {
    infer(r#"true + 42"#).assert_type_mismatch();
}

#[test]
fn int_divide_string_error() {
    infer(r#"100 / "ten""#).assert_type_mismatch();
}

#[test]
fn int_compare_string_error() {
    infer(r#"42 < "hello""#).assert_type_mismatch();
}

#[test]
fn string_compare_bool_error() {
    infer(r#""hello" > true"#).assert_type_mismatch();
}

#[test]
fn int_compare_bool_error() {
    infer(r#"42 == true"#).assert_type_mismatch();
}

#[test]
fn and_with_int_error() {
    infer(r#"42 && true"#).assert_type_mismatch();
}

#[test]
fn or_with_string_error() {
    infer(r#"true || "false""#).assert_type_mismatch();
}

#[test]
fn and_with_both_int_error() {
    infer(r#"1 && 0"#).assert_type_mismatch();
}

#[test]
fn addition_type_inference() {
    infer(
        r#"{
    let add = |x: int, y: int| -> int { x + y };
    add(5, 10)
    }"#,
    )
    .assert_type_int();
}

#[test]
fn comparison_in_if() {
    infer(
        r#"{
    let x = 10;
    if x > 5 {
      1
    } else {
      0
    }
    }"#,
    )
    .assert_type_int();
}

#[test]
fn unary_minus_int_literal() {
    infer("-42").assert_type_int();
}

#[test]
fn unary_minus_float_literal() {
    infer("-3.14").assert_type_float();
}

#[test]
fn unary_minus_int_variable() {
    infer("{ let x = 5; -x }").assert_type_int();
}

#[test]
fn unary_minus_expression() {
    infer("-(1 + 2)").assert_type_int();
}

#[test]
fn double_negative() {
    infer("--42").assert_type_int();
}

#[test]
fn unary_minus_in_arithmetic() {
    infer("-5 + 10").assert_type_int();
}

#[test]
fn unary_minus_with_multiplication() {
    infer("-2 * 3").assert_type_int();
}

#[test]
fn unary_not_true() {
    infer("!true").assert_type_bool();
}

#[test]
fn unary_not_false() {
    infer("!false").assert_type_bool();
}

#[test]
fn unary_not_bool_variable() {
    infer("{ let b = true; !b }").assert_type_bool();
}

#[test]
fn unary_not_comparison() {
    infer("!(1 > 2)").assert_type_bool();
}

#[test]
fn double_negation() {
    infer("!!true").assert_type_bool();
}

#[test]
fn unary_not_in_condition() {
    infer("{ let x = false; if !x { 1 } else { 2 } }").assert_type_int();
}

#[test]
fn unary_not_with_equality() {
    infer("!(1 == 2)").assert_type_bool();
}

#[test]
fn negative_in_comparison() {
    infer("-5 < 0").assert_type_bool();
}

#[test]
fn not_with_negative() {
    infer("{ let x = -5; !(x > 0) }").assert_type_bool();
}

#[test]
fn unary_in_block() {
    infer("{ let x = 10; let y = -x; y }").assert_type_int();
}

#[test]
fn unary_not_in_block() {
    infer("{ let x = true; let y = !x; y }").assert_type_bool();
}

#[test]
fn nested_unary_expressions() {
    infer("{ let a = 5; let b = -a; let c = -b; c }").assert_type_int();
}

#[test]
fn unary_minus_on_bool_should_error() {
    infer("-true").assert_infer_code("type_mismatch");
}

#[test]
fn unary_minus_on_string_should_error() {
    infer(r#"-"hello""#).assert_infer_code("type_mismatch");
}

#[test]
fn pipeline_simple() {
    infer(
        r#"{
    let double = |x: int| -> int { x * 2 };
    5 |> double
  }"#,
    )
    .assert_type_int();
}

#[test]
fn pipeline_chained() {
    infer(
        r#"{
    let double = |x: int| -> int { x * 2 };
    let add_ten = |x: int| -> int { x + 10 };
    5 |> double |> add_ten
  }"#,
    )
    .assert_type_int();
}

#[test]
fn pipeline_multiple_chains() {
    infer(
        r#"{
    let double = |x: int| -> int { x * 2 };
    let triple = |x: int| -> int { x * 3 };
    let add = |x: int| -> int { x + 1 };
    5 |> double |> triple |> add
  }"#,
    )
    .assert_type_int();
}

#[test]
fn pipeline_with_partial_application() {
    infer(
        r#"{
    let add = |x: int, y: int| -> int { x + y };
    5 |> add(3)
  }"#,
    )
    .assert_type_int();
}

#[test]
fn pipeline_chained_with_partial_application() {
    infer(
        r#"{
    let add = |x: int, y: int| -> int { x + y };
    let multiply = |x: int, y: int| -> int { x * y };
    5 |> add(3) |> multiply(2)
  }"#,
    )
    .assert_type_int();
}

#[test]
fn pipeline_partial_application_multiple_args() {
    infer(
        r#"{
    let sum_three = |x: int, y: int, z: int| -> int { x + y + z };
    10 |> sum_three(20, 30)
  }"#,
    )
    .assert_type_int();
}

#[test]
fn pipeline_with_strings() {
    infer(
        r#"{
    let greet = |name: string| -> string { "Hello, " + name };
    "World" |> greet
  }"#,
    )
    .assert_type_string();
}

#[test]
fn pipeline_type_transformation() {
    infer(
        r#"{
    let length = |s: string| -> int { 5 };
    "hello" |> length
  }"#,
    )
    .assert_type_int();
}

#[test]
fn pipeline_bool_result() {
    infer(
        r#"{
    let is_positive = |x: int| -> bool { x > 0 };
    5 |> is_positive
  }"#,
    )
    .assert_type_bool();
}

#[test]
fn pipeline_with_let_binding() {
    infer(
        r#"{
    let double = |x: int| -> int { x * 2 };
    let value = 5;
    value |> double
  }"#,
    )
    .assert_type_int();
}

#[test]
fn pipeline_result_in_let() {
    infer(
        r#"{
    let double = |x: int| -> int { x * 2 };
    let result = 5 |> double;
    result
  }"#,
    )
    .assert_type_int();
}

#[test]
fn pipeline_chained_result_in_let() {
    infer(
        r#"{
    let double = |x: int| -> int { x * 2 };
    let add = |x: int, y: int| -> int { x + y };
    let result = 5 |> double |> add(10);
    result
  }"#,
    )
    .assert_type_int();
}

#[test]
fn pipeline_with_arithmetic() {
    infer(
        r#"{
    let double = |x: int| -> int { x * 2 };
    (2 + 3) |> double
  }"#,
    )
    .assert_type_int();
}

#[test]
fn pipeline_complex_expression() {
    infer(
        r#"{
    let add = |x: int, y: int| -> int { x + y };
    let multiply = |x: int, y: int| -> int { x * y };
    (10 + 5) |> multiply(2) |> add(100)
  }"#,
    )
    .assert_type_int();
}

#[test]
fn pipeline_precedence_with_addition() {
    infer(
        r#"{
    let double = |x: int| -> int { x * 2 };
    2 + 3 |> double
  }"#,
    )
    .assert_type_int();
}

#[test]
fn pipeline_in_function() {
    infer(
        r#"{
    let double = |x: int| -> int { x * 2 };
    let add_ten = |x: int| -> int { x + 10 };
    let process = |value: int| -> int { value |> double |> add_ten };
    process(5)
  }"#,
    )
    .assert_type_int();
}

#[test]
fn simple_chained_field_access() {
    infer(
        r#"{
    struct C { value: int }
    struct B { c: C }
    struct A { b: B }
    let a = A { b: B { c: C { value: 42 } } };
    a.b.c.value
    }"#,
    )
    .assert_type_int();
}

#[test]
fn four_level_field_access() {
    infer(
        r#"{
    struct D { value: int }
    struct C { d: D }
    struct B { c: C }
    struct A { b: B }
    let a = A { b: B { c: C { d: D { value: 42 } } } };
    a.b.c.d.value
    }"#,
    )
    .assert_type_int();
}

#[test]
fn generic_container_chain() {
    infer(
        r#"{
    struct Container<T> { value: T }
    let outer = Container { value: Container { value: 42 } };
    outer.value.value
    }"#,
    )
    .assert_type_int();
}

#[test]
fn triple_generic_container_chain() {
    infer(
        r#"{
    struct Container<T> { value: T }
    let c = Container { value: Container { value: Container { value: 42 } } };
    c.value.value.value
    }"#,
    )
    .assert_type_int();
}

#[test]
fn generic_chain_with_type_change() {
    infer(
        r#"{
    struct Container<T> { value: T }
    let c1 = Container { value: 42 };
    let c2 = Container { value: c1 };
    c2.value.value
    }"#,
    )
    .assert_type_int();
}

#[test]
fn generic_containing_non_generic() {
    infer(
        r#"{
    struct Point { x: int, y: int }
    struct Container<T> { value: T }
    let c = Container { value: Point { x: 1, y: 2 } };
    c.value.x
    }"#,
    )
    .assert_type_int();
}

#[test]
fn non_generic_containing_generic() {
    infer(
        r#"{
    struct Container<T> { value: T }
    struct Wrapper { container: Container<int> }
    let w = Wrapper { container: Container { value: 42 } };
    w.container.value
    }"#,
    )
    .assert_type_int();
}

#[test]
fn alternating_generic_non_generic() {
    infer(
        r#"{
    struct Container<T> { value: T }
    struct Point { x: int, y: int }
    struct Outer<T> { inner: T }
    let o = Outer { inner: Point { x: 1, y: 2 } };
    let c = Container { value: o };
    c.value.inner.x
    }"#,
    )
    .assert_type_int();
}

#[test]
fn two_param_generic_chain() {
    infer(
        r#"{
    struct Pair<K, V> { key: K, value: V }
    let outer = Pair { key: "id", value: Pair { key: 1, value: 42 } };
    outer.value.value
    }"#,
    )
    .assert_type_int();
}

#[test]
fn two_param_generic_access_both() {
    infer(
        r#"{
    struct Pair<K, V> { key: K, value: V }
    let p = Pair { key: "name", value: 42 };
    let result = p.key;
    result
    }"#,
    )
    .assert_type_string();
}

#[test]
fn nested_two_param_generics() {
    infer(
        r#"{
    struct Pair<K, V> { key: K, value: V }
    let outer = Pair {
      key: Pair { key: "outer", value: 1 },
      value: Pair { key: "inner", value: 42 }
    };
    outer.value.value
    }"#,
    )
    .assert_type_int();
}

#[test]
fn generic_field_different_instantiation() {
    infer(
        r#"{
    struct Container<T> { value: T }
    let int_container = Container { value: 42 };
    let str_container = Container { value: "hello" };
    int_container.value
    }"#,
    )
    .assert_type_int();
}

#[test]
fn generic_chain_multiple_types() {
    infer(
        r#"{
    struct Container<T> { value: T }
    struct Pair<K, V> { key: K, value: V }
    let c = Container { value: Pair { key: "id", value: 42 } };
    c.value.value
    }"#,
    )
    .assert_type_int();
}

#[test]
fn deeply_nested_same_generic() {
    infer(
        r#"{
    struct Box<T> { inner: T }
    let b = Box {
      inner: Box {
        inner: Box {
          inner: Box {
            inner: 42
          }
        }
      }
    };
    b.inner.inner.inner.inner
    }"#,
    )
    .assert_type_int();
}

#[test]
fn generic_chain_with_enum() {
    infer(
        r#"{
    struct Container<T> { value: T }
    let c = Container { value: Some(42) };
    c.value
    }"#,
    )
    .assert_type_struct_generic("Option", vec![int_type()]);
}

#[test]
fn inference_through_generic_chain() {
    infer(
        r#"{
    struct Container<T> { value: T }
    let get_value = |c: Container<Container<int>>| -> int { c.value.value };
    get_value
    }"#,
    )
    .assert_function_type(
        vec![con_type(
            "Container",
            vec![con_type("Container", vec![int_type()])],
        )],
        int_type(),
    );
}

#[test]
fn generic_chain_in_function_call() {
    infer(
        r#"{
    struct Container<T> { value: T }
    let process = |x: int| -> int { x + 1 };
    let c = Container { value: Container { value: 42 } };
    process(c.value.value)
    }"#,
    )
    .assert_type_int();
}

#[test]
fn wrong_field_in_generic_chain() {
    infer(
        r#"{
    struct Container<T> { value: T }
    let c = Container { value: Container { value: 42 } };
    c.value.wrong_field
    }"#,
    )
    .assert_infer_code("member_not_found");
}

#[test]
fn field_access_on_non_struct() {
    infer(
        r#"{
    let x = 42;
    x.field
    }"#,
    )
    .assert_infer_code("member_not_found");
}

#[test]
fn generic_chain_with_function_return() {
    infer(
        r#"{
    struct Container<T> { value: T }
    let make_nested = || -> Container<Container<int>> { Container { value: Container { value: 42 } } };
    let result = make_nested();
    result.value.value
    }"#,
    )
    .assert_type_int();
}

#[test]
fn nested_generic_with_mixed_params() {
    infer(
        r#"{
    struct Container<T> { value: T }
    struct Pair<K, V> { key: K, value: V }
    let nested = Container {
      value: Pair {
        key: Container { value: "id" },
        value: Container { value: 42 }
      }
    };
    nested.value.value.value
    }"#,
    )
    .assert_type_int();
}

#[test]
fn triple_nested_different_generics() {
    infer(
        r#"{
    struct Container<T> { value: T }
    struct Wrapper<T> { data: T }
    struct Holder<T> { item: T }
    let nested = Container {
      value: Wrapper {
        data: Holder {
          item: 42
        }
      }
    };
    nested.value.data.item
    }"#,
    )
    .assert_type_int();
}

#[test]
fn generic_first_field_non_generic_second() {
    infer(
        r#"{
    struct Point { x: int, y: int }
    struct Container<T> { value: T, metadata: string }
    let c = Container { value: Point { x: 1, y: 2 }, metadata: "data" };
    c.value.x
    }"#,
    )
    .assert_type_int();
}

#[test]
fn multiple_fields_generic_access() {
    infer(
        r#"{
    struct Pair<K, V> { first: K, second: V }
    let p = Pair { first: Container { value: 1 }, second: Container { value: "hello" } };
    struct Container<T> { value: T }
    p.first.value
    }"#,
    )
    .assert_type_int();
}

#[test]
fn empty_block() {
    infer("{}").assert_type_unit();
}

#[test]
fn block_single_expression() {
    infer("{ 42 }").assert_type_int();
}

#[test]
fn block_with_binding() {
    infer("{ let x = 5; x }").assert_type_int();
}

#[test]
fn block_with_multiple_bindings() {
    infer("{ let x = 10; let y = 20; x + y }").assert_type_int();
}

#[test]
fn block_with_only_let_statement() {
    infer("{ let x = 5 }").assert_type_unit();
}

#[test]
fn block_with_multiple_let_statements() {
    infer("{ let x = 10; let y = 20 }").assert_type_unit();
}

#[test]
fn block_returns_bool() {
    infer("{ let x = 5; x > 0 }").assert_type_bool();
}

#[test]
fn nested_blocks() {
    infer(
        r#"
    {
      let x = {
        let y = 5;
        y + 1
      };
      x * 2
    }
        "#,
    )
    .assert_type_int();
}

#[test]
fn infer_bool() {
    infer("{ let flag = true; flag }").assert_type_bool();
}

#[test]
fn infer_string() {
    infer("{ let greeting = \"hello\"; greeting }").assert_type_string();
}

#[test]
fn annotation_int() {
    infer("{ let x: int = 42; x }").assert_type_int();
}

#[test]
fn annotation_bool() {
    infer("{ let flag: bool = true; flag }").assert_type_bool();
}

#[test]
fn annotation_mismatch() {
    infer("{ let x: bool = 42; x }").assert_type_mismatch();
}

#[test]
fn mixed_annotations() {
    infer("{ let x = 10; let y: int = 20; x + y }").assert_type_int();
}

#[test]
fn variable_in_arithmetic() {
    infer("{ let x = 10; x + 5 }").assert_type_int();
}

#[test]
fn chained_operations() {
    infer("{ let x = 10; let y = x + 5; y }").assert_type_int();
}

#[test]
fn shadowing_same_type() {
    infer("{ let x = 1; let x = 2; x }").assert_type_int();
}

#[test]
fn shadowing_different_types() {
    infer("{ let x = 1; let x = true; x }").assert_type_bool();
}

#[test]
fn undefined_variable() {
    infer("undefined_var").assert_not_found();
}

#[test]
fn use_before_definition() {
    infer("{ let x = y; let y = 42; x }").assert_not_found();
}

#[test]
fn block_with_local_function_declaration() {
    infer(
        r#"
    {
      let helper = |x: int| -> int { x + 1 };

      let result = helper(5);
      result
    }
        "#,
    )
    .assert_type_int();
}

#[test]
fn block_with_local_struct_declaration() {
    infer(
        r#"
    {
      struct Point {
        x: int,
        y: int,
      }

      let p = Point { x: 10, y: 20 };
      p.x
    }
        "#,
    )
    .assert_type_int();
}

#[test]
fn reference_of_variable() {
    infer("{ let x = 42; &x }").assert_type(ref_type(int_type()));
}

#[test]
fn reference_of_struct_literal() {
    infer(
        r#"{
    struct Foo { value: int }
    &Foo { value: 42 }
  }"#,
    )
    .assert_no_errors();
}

#[test]
fn can_take_address_of_function_call() {
    infer(
        r#"{
    let make_int = || -> int { 42 };
    &make_int()
  }"#,
    )
    .assert_type(ref_type(int_type()));
}

#[test]
fn cannot_take_address_of_field_on_function_call() {
    infer(
        r#"{
    struct Foo { value: int }
    let make_foo = || -> Foo { Foo { value: 42 } };
    &make_foo().value
  }"#,
    )
    .assert_infer_code("non_addressable_expression");
}

#[test]
fn cannot_take_address_of_field_on_parenthesized_function_call() {
    infer(
        r#"{
    struct Foo { value: int }
    let make_foo = || -> Foo { Foo { value: 42 } };
    &(make_foo()).value
  }"#,
    )
    .assert_infer_code("non_addressable_expression");
}

#[test]
fn cannot_take_address_of_literal() {
    infer("&42").assert_infer_code("non_addressable_expression");
}

#[test]
fn cannot_take_address_of_binary_expression() {
    infer("&(1 + 2)").assert_infer_code("non_addressable_expression");
}

#[test]
fn cannot_take_address_of_conditional() {
    infer("&(if true { 1 } else { 2 })").assert_infer_code("non_addressable_expression");
}

#[test]
fn can_take_address_of_field_access() {
    infer(
        r#"{
    struct Foo { value: int }
    let f = Foo { value: 42 };
    &f.value
  }"#,
    )
    .assert_type(ref_type(int_type()));
}

#[test]
fn cannot_take_address_of_index_on_map_call() {
    infer(
        r#"{
    let make_map = || -> Map<string, int> { Map.new() };
    &make_map()["k"]
  }"#,
    )
    .assert_infer_code("non_addressable_expression");
}

#[test]
fn can_take_address_of_slice_call_index() {
    infer(
        r#"{
    let make_slice = || -> Slice<int> { [1, 2, 3] };
    &make_slice()[0]
  }"#,
    )
    .assert_no_errors();
}

#[test]
fn cannot_take_address_of_field_on_struct_literal() {
    infer(
        r#"{
    struct Point { x: int }
    &Point { x: 1 }.x
  }"#,
    )
    .assert_infer_code("non_addressable_expression");
}

#[test]
fn cannot_assign_to_field_on_function_call() {
    infer(
        r#"{
    struct Box { x: int }
    fn make_box() -> Box { Box { x: 1 } }
    make_box().x = 2
  }"#,
    )
    .assert_infer_code("non_addressable_assignment");
}

#[test]
fn cannot_assign_to_field_on_struct_literal() {
    infer(
        r#"{
    struct Point { x: int }
    Point { x: 1 }.x = 2
  }"#,
    )
    .assert_infer_code("non_addressable_assignment");
}

#[test]
fn cannot_assign_to_tuple_literal_field() {
    infer(
        r#"{
    (1, 2).0 = 3
  }"#,
    )
    .assert_infer_code("non_addressable_assignment");
}

#[test]
fn can_assign_to_field_on_ref_returning_call() {
    infer(
        r#"{
    struct Point { x: int }
    let make = || -> Ref<Point> { &Point { x: 1 } };
    make().x = 2
  }"#,
    )
    .assert_no_errors();
}

#[test]
fn can_take_address_of_field_on_ref_returning_call() {
    infer(
        r#"{
    struct Point { x: int }
    let make = || -> Ref<Point> { &Point { x: 1 } };
    &make().x
  }"#,
    )
    .assert_no_errors();
}

#[test]
fn cannot_append_to_immutable_slice() {
    infer(
        r#"{
    let s = [1, 2, 3];
    s.append(4)
  }"#,
    )
    .assert_infer_code("immutable");
}

#[test]
fn append_on_mutable_slice_is_allowed() {
    infer(
        r#"{
    let mut s = [1, 2, 3];
    s.append(4)
  }"#,
    )
    .assert_no_errors();
}

#[test]
fn append_in_expression_position_no_mut_required() {
    infer(
        r#"{
    let s = [1, 2, 3];
    let s2 = s.append(4);
    s2
  }"#,
    )
    .assert_no_errors();
}

#[test]
fn append_as_return_value_no_mut_required() {
    infer(
        r#"
fn test(s: Slice<int>) -> Slice<int> {
  s.append(4)
}
"#,
    )
    .assert_no_errors();
}

#[test]
fn cannot_append_to_immutable_slice_mid_block() {
    infer(
        r#"{
    let s = [1, 2, 3];
    s.append(4);
    s
  }"#,
    )
    .assert_infer_code("immutable");
}

#[test]
fn append_in_if_branch_no_mut_required() {
    infer(
        r#"
fn test(s: Slice<int>, flag: bool) -> Slice<int> {
  if flag { s.append(4) } else { s }
}
"#,
    )
    .assert_no_errors();
}

#[test]
fn append_in_if_branch_let_binding_no_mut_required() {
    infer(
        r#"{
    let s = [1, 2, 3];
    let flag = true;
    let result = if flag { s.append(4) } else { s };
    result
  }"#,
    )
    .assert_no_errors();
}

#[test]
fn append_in_match_arm_no_mut_required() {
    infer(
        r#"
fn test(s: Slice<int>, x: int) -> Slice<int> {
  match x {
    1 => s.append(4),
    _ => s,
  }
}
"#,
    )
    .assert_no_errors();
}

#[test]
fn cannot_delete_from_immutable_map() {
    infer(
        r#"{
    let m = Map.from([("a", 1)]);
    m.delete("a")
  }"#,
    )
    .assert_infer_code("immutable");
}

#[test]
fn delete_on_mutable_map_is_allowed() {
    infer(
        r#"{
    let mut m = Map.from([("a", 1)]);
    m.delete("a")
  }"#,
    )
    .assert_no_errors();
}

#[test]
fn auto_address_value_to_ref_receiver() {
    infer(
        r#"
    struct Foo { value: int }

    impl Foo {
      fn increment(self: Ref<Foo>) { self.value = self.value + 1 }
    }

    fn main() {
      let mut foo = Foo { value: 42 };
      foo.increment()
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn auto_address_struct_literal_receiver() {
    infer(
        r#"
    struct Foo { value: int }

    impl Foo {
      fn increment(self: Ref<Foo>) { self.value = self.value + 1 }
    }

    fn main() {
      Foo { value: 42 }.increment()
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn auto_address_field_access_receiver() {
    infer(
        r#"
    struct Inner { value: int }

    impl Inner {
      fn increment(self: Ref<Inner>) { self.value = self.value + 1 }
    }

    struct Outer { inner: Inner }

    fn main() {
      let mut outer = Outer { inner: Inner { value: 42 } };
      outer.inner.increment()
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn auto_address_slice_index_receiver() {
    infer(
        r#"
    struct Foo { value: int }

    impl Foo {
      fn increment(self: Ref<Foo>) { self.value = self.value + 1 }
    }

    fn main() {
      let mut items: Slice<Foo> = [Foo { value: 42 }];
      items[0].increment()
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn auto_deref_ref_to_value_receiver() {
    infer(
        r#"
    struct Foo { value: int }

    impl Foo {
      fn get_value(self: Foo) -> int { self.value }
    }

    fn main() -> int {
      let foo = Foo { value: 42 };
      let foo_ref: Ref<Foo> = &foo;
      foo_ref.get_value()
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn auto_deref_nested_field() {
    infer(
        r#"
    struct Inner { value: int }

    impl Inner {
      fn get_value(self: Inner) -> int { self.value }
    }

    struct Outer { inner: Inner }

    fn main() -> int {
      let outer = Outer { inner: Inner { value: 42 } };
      let outer_ref: Ref<Outer> = &outer;
      outer_ref.inner.get_value()
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn auto_address_function_call_receiver() {
    infer(
        r#"
    struct Foo { value: int }

    impl Foo {
      fn increment(self: Ref<Foo>) { self.value = self.value + 1 }
    }

    fn make_foo() -> Foo { Foo { value: 42 } }

    fn main() {
      make_foo().increment()
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn auto_address_function_call_receiver_with_return() {
    infer(
        r#"
    struct Counter(int)

    impl Counter {
      fn get(self: Ref<Counter>) -> int { self.0 }
    }

    fn make_counter(n: int) -> Counter { Counter(n) }

    fn main() -> int {
      make_counter(1).get()
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn no_coercion_needed_when_types_match() {
    infer(
        r#"
    struct Foo { value: int }

    impl Foo {
      fn get_value(self: Foo) -> int { self.value }
    }

    fn main() -> int {
      let foo = Foo { value: 42 };
      foo.get_value()
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn no_coercion_needed_ref_matches() {
    infer(
        r#"
    struct Foo { value: int }

    impl Foo {
      fn increment(self: Ref<Foo>) { self.value = self.value + 1 }
    }

    fn main() {
      let foo = Foo { value: 42 };
      let foo_ref: Ref<Foo> = &foo;
      foo_ref.increment()
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn explicit_ref_still_works() {
    infer(
        r#"
    struct Foo { value: int }

    impl Foo {
      fn increment(self: Ref<Foo>) { self.value = self.value + 1 }
    }

    fn main() {
      let foo = Foo { value: 42 };
      (&foo).increment()
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn auto_address_generic_method() {
    infer(
        r#"
    struct Container<T> { value: T }

    impl<T> Container<T> {
      fn set(self: Ref<Container<T>>, v: T) { self.value = v }
    }

    fn main() {
      let mut c = Container { value: 42 };
      c.set(100)
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn auto_address_generic_method_with_explicit_type_args() {
    infer(
        r#"
    struct Foo { value: int }

    impl Foo {
      fn convert<T>(self: Ref<Foo>, v: T) -> T { v }
    }

    fn main() -> string {
      let mut foo = Foo { value: 42 };
      foo.convert<string>("hello")
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn equality_on_unbounded_generic_rejected() {
    infer(
        r#"
    fn make_predicate<T>(threshold: T) -> fn(T) -> bool {
      |val| val == threshold
    }

    fn main() {}
        "#,
    )
    .assert_infer_code("type_mismatch");
}

#[test]
fn not_equal_on_unbounded_generic_rejected() {
    infer(
        r#"
    fn dedup<T>(items: Slice<T>) -> Slice<T> {
      let mut result: Slice<T> = [];
      for item in items {
        if result.length() == 0 || result[result.length() - 1] != item {
          result.append(item);
        }
      }
      result
    }

    fn main() {}
        "#,
    )
    .assert_infer_code("type_mismatch");
}

#[test]
fn arithmetic_on_unbounded_generic_rejected() {
    infer(
        r#"
    fn add_generic<T>(a: T, b: T) -> T {
      a + b
    }

    fn main() {}
        "#,
    )
    .assert_infer_code("type_mismatch");
}

#[test]
fn float_modulo_rejected() {
    infer(
        r#"
    fn main() {
      let x: float64 = 10.0;
      let y: float64 = 3.0;
      let _ = x % y;
    }
        "#,
    )
    .assert_infer_code("float_modulo");
}

#[test]
fn int_modulo_still_works() {
    infer(
        r#"
    fn main() {
      let x = 10;
      let y = 3;
      let _ = x % y;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn constant_float_division_by_zero_rejected() {
    infer(
        r#"
    fn main() {
      let inf = 1.0 / 0.0;
    }
        "#,
    )
    .assert_infer_code("division_by_zero");
}

#[test]
fn constant_float_zero_division_by_zero_rejected() {
    infer(
        r#"
    fn main() {
      let nan = 0.0 / 0.0;
    }
        "#,
    )
    .assert_infer_code("division_by_zero");
}

#[test]
fn try_block_two_question_marks_different_t_custom_error() {
    infer(
        r#"
    struct MyError { msg: string }
    impl MyError { fn Error(self) -> string { self.msg } }

    fn get_name() -> Result<string, MyError> { Ok("Alice") }
    fn get_age() -> Result<int, MyError> { Ok(30) }

    fn combine() -> Result<(string, int), MyError> {
      let nr = get_name();
      let ar = get_age();
      let result = try {
        let n = nr?;
        let a = ar?;
        (n, a)
      };
      result
    }

    fn main() {}
        "#,
    )
    .assert_no_errors();
}

#[test]
fn try_block_question_marks_inline_custom_error() {
    infer(
        r#"
    struct MyError { msg: string }
    impl MyError { fn Error(self) -> string { self.msg } }

    fn get_name() -> Result<string, MyError> { Ok("Alice") }
    fn get_age() -> Result<int, MyError> { Ok(30) }

    fn combine() -> Result<(string, int), MyError> {
      try {
        let n = get_name()?;
        let a = get_age()?;
        (n, a)
      }
    }

    fn main() {}
        "#,
    )
    .assert_no_errors();
}

#[test]
fn generic_function_reference_assigned_to_concrete_fn_type() {
    infer(
        r#"
    interface HasName {
      fn name(self) -> string
    }

    struct Person { full_name: string }
    impl Person { fn name(self) -> string { self.full_name } }

    fn get_name<T: HasName>(t: T) -> string { t.name() }

    fn main() {
      let p = Person { full_name: "Alice" };
      let name_getter: fn(Person) -> string = get_name;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn ref_param_field_write_in_free_function() {
    infer(
        r#"
    struct Point { x: int, y: int }

    fn set_x(p: Ref<Point>, val: int) {
      p.*.x = val
    }

    fn main() {
      let mut pt = Point { x: 0, y: 0 };
      set_x(&pt, 42)
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn pointer_deref_assignment_no_ice() {
    infer(
        r#"
    fn main() {
      let mut count = 0;
      (&count).* = (&count).* + 1
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn ref_binding_field_mutation_without_mut() {
    infer(
        r#"
    struct Point { x: int, y: int }

    fn test() {
      let mut p = Point { x: 1, y: 2 }
      let r = &p
      r.x = 50
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn range_variable_slice_index() {
    infer(
        r#"
    fn main() {
      let items = [10, 20, 30, 40, 50];
      let r = 2..5;
      let sliced: Slice<int> = items[r]
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn defer_in_closure_inside_loop() {
    infer(
        r#"
    fn cleanup() {}

    fn main() {
      for i in 0..3 {
        let f = || {
          defer cleanup()
          42
        };
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn break_value_in_while_rejected() {
    infer(
        r#"
    fn main() {
      let mut n = 0;
      while n < 100 {
        n = n + 1;
        if n > 5 { break n }
      }
    }
        "#,
    )
    .assert_infer_code("break_value_in_non_loop");
}

#[test]
fn break_value_in_for_rejected() {
    infer(
        r#"
    fn main() {
      for i in 0..10 {
        if i == 5 { break i }
      }
    }
        "#,
    )
    .assert_infer_code("break_value_in_non_loop");
}

#[test]
fn const_shadow_with_let_clear_error() {
    infer(
        r#"
    const MY_CONST = 42

    fn main() {
      let MY_CONST = "shadowed"
    }
        "#,
    )
    .assert_infer_code("uppercase_binding");
}

#[test]
fn closure_after_let_on_new_line() {
    infer(
        r#"
    fn make_counter() -> fn() -> int {
      let mut count = 0
      || {
        count = count + 1
        count
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn option_enum_equality_comparable() {
    infer(
        r#"
    enum Color { Red, Green, Blue }

    fn test() -> bool {
      let o1 = Some(Color.Red)
      let o2 = Some(Color.Red)
      o1 == o2
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn slice_index_wrong_type_single_diagnostic() {
    let result = infer(r#"{ let _ = [1,2,3][true] }"#);
    assert_eq!(result.errors.len(), 1);
}

#[test]
fn numeric_binary_wrong_type_single_diagnostic() {
    let result = infer(r#"{ let _ = 1 + true }"#);
    assert_eq!(result.errors.len(), 1);
}

#[test]
fn assignment_type_mismatch_single_diagnostic() {
    let result = infer(r#"{ let mut x = 0; x = true; let _ = x }"#);
    assert_eq!(result.errors.len(), 1);
}
