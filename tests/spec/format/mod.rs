use crate::assert_format_snapshot;

#[test]
fn assignment_simple() {
    assert_format_snapshot!("fn test() { x = 42 }");
}

#[test]
fn binary_addition() {
    assert_format_snapshot!("fn test() { 1 + 2 }");
}

#[test]
fn binary_remainder() {
    assert_format_snapshot!("fn test() { 10 % 3 }");
}

#[test]
fn binary_less_than_or_equal() {
    assert_format_snapshot!("fn test() { a <= b }");
}

#[test]
fn binary_greater_than_or_equal() {
    assert_format_snapshot!("fn test() { a >= b }");
}

#[test]
fn binary_not_equal() {
    assert_format_snapshot!("fn test() { a != b }");
}

#[test]
fn binary_subtraction() {
    assert_format_snapshot!("fn test() { 5 - 3 }");
}

#[test]
fn binary_division() {
    assert_format_snapshot!("fn test() { 10 / 2 }");
}

#[test]
fn binary_less_than() {
    assert_format_snapshot!("fn test() { a < b }");
}

#[test]
fn binary_greater_than() {
    assert_format_snapshot!("fn test() { a > b }");
}

#[test]
fn binary_equal() {
    assert_format_snapshot!("fn test() { a == b }");
}

#[test]
fn binary_and() {
    assert_format_snapshot!("fn test() { a && b }");
}

#[test]
fn binary_or() {
    assert_format_snapshot!("fn test() { a || b }");
}

#[test]
fn binary_no_spaces() {
    assert_format_snapshot!("fn test() { 1+2*3 }");
}

#[test]
fn binary_pipeline() {
    assert_format_snapshot!("fn test() { x |> foo() |> bar() }");
}

#[test]
fn binary_with_comment_before_right() {
    assert_format_snapshot!(
        r#"fn test() {
  1 +
  // comment before right operand
  2
}"#
    );
}

#[test]
fn binary_pipeline_with_comment() {
    assert_format_snapshot!(
        r#"fn test() {
  x
  |> foo()
  // comment before bar
  |> bar()
}"#
    );
}

#[test]
fn call_no_args() {
    assert_format_snapshot!("fn test() { foo() }");
}

#[test]
fn chaining_dot_access() {
    assert_format_snapshot!("fn test() { obj.field.nested.value }");
}

#[test]
fn chaining_method_calls() {
    assert_format_snapshot!("fn test() { x.foo().bar().baz() }");
}

#[test]
fn chaining_mixed() {
    assert_format_snapshot!("fn test() { obj.field.method().another.final_call() }");
}

#[test]
fn call_with_args() {
    assert_format_snapshot!("fn test() { add(1, 2, 3) }");
}

#[test]
fn call_with_comment_between_args() {
    assert_format_snapshot!(
        r#"fn test() {
  foo(
    a,
    // comment before b
    b,
    c,
  );
}"#
    );
}

#[test]
fn call_with_type_args() {
    assert_format_snapshot!("fn test() { func<int>(arg) }");
}

#[test]
fn call_with_multiple_type_args() {
    assert_format_snapshot!("fn test() { func<A, B, C>(x, y) }");
}

#[test]
fn call_with_single_closure_arg() {
    assert_format_snapshot!("fn test() { map(|x| x + 1) }");
}

#[test]
fn method_call_with_type_args() {
    assert_format_snapshot!("fn test() { obj.method<int>(arg) }");
}

#[test]
fn comment_before_match_arm() {
    assert_format_snapshot!(
        "fn test() {\n    match x {\n        // comment before arm\n        Some(a) => a,\n        None => 0,\n    }\n}"
    );
}

#[test]
fn comment_between_statements() {
    assert_format_snapshot!(
        r#"fn foo() {
  let x = 1;
  // comment between
  let y = 2;
}"#
    );
}

#[test]
fn comment_leading() {
    assert_format_snapshot!("// leading comment\nfn foo() {}");
}

#[test]
fn comment_leading_before_second_def() {
    assert_format_snapshot!(
        r#"fn first() {}

// comment before second
fn second() {}"#
    );
}

#[test]
fn comment_groups_with_blank_line() {
    assert_format_snapshot!(
        r#"// first group

// second group
fn foo() {}"#
    );
}

#[test]
fn comment_on_struct_field() {
    assert_format_snapshot!(
        "struct Point {\n    x: int, // the x coordinate\n    y: int, // the y coordinate\n}"
    );
}

#[test]
fn comment_trailing() {
    assert_format_snapshot!(
        r#"fn test() {
  let x = 1 // trailing comment
}"#
    );
}

#[test]
fn const_simple() {
    assert_format_snapshot!("const MAX_SIZE = 100");
}

#[test]
fn const_with_type() {
    assert_format_snapshot!("const PI: float = 3.14159");
}

#[test]
fn const_public() {
    assert_format_snapshot!("pub const MAX: int = 100");
}

#[test]
fn var_simple() {
    assert_format_snapshot!("var ErrNotFound: error");
}

#[test]
fn var_public() {
    assert_format_snapshot!("pub var Stdin: Ref<File>");
}

#[test]
fn continue_simple() {
    assert_format_snapshot!("fn test() { loop { continue } }");
}

#[test]
fn docline_on_function() {
    assert_format_snapshot!("/// This is a docline\nfn foo() {}");
}

#[test]
fn docline_multiline() {
    assert_format_snapshot!("/// First line\n/// Second line\nfn foo() {}");
}

#[test]
fn enum_simple() {
    assert_format_snapshot!("enum Status { Pending, Complete }");
}

#[test]
fn enum_with_data() {
    assert_format_snapshot!("enum Option<T> { Some(T), None }");
}

#[test]
fn enum_empty() {
    assert_format_snapshot!("enum Empty {}");
}

#[test]
fn value_enum_with_underlying_type() {
    assert_format_snapshot!(
        "pub enum ParameterSizes: int { L1024N160 = 0, L2048N224 = 1, L2048N256 = 2, L3072N256 = 3 }"
    );
}

#[test]
fn for_loop() {
    assert_format_snapshot!("fn test() { for item in items { process(item) } }");
}

#[test]
fn format_string() {
    assert_format_snapshot!("fn test() { f\"hello {name}\" }");
}

#[test]
fn function_empty() {
    assert_format_snapshot!("fn foo() {}");
}

#[test]
fn function_multiline() {
    assert_format_snapshot!(
        r#"fn foo() {
  let x = 1;
  let y = 2;
  x + y
}"#
    );
}

#[test]
fn block_with_blank_line() {
    assert_format_snapshot!(
        r#"fn foo() {
  let x = 1;

  let y = 2;
  x + y
}"#
    );
}

#[test]
fn block_empty_with_comment() {
    assert_format_snapshot!(
        r#"fn foo() {
  // nothing here
}"#
    );
}

#[test]
fn function_public() {
    assert_format_snapshot!("pub fn greet() { \"hello\" }");
}

#[test]
fn function_public_with_attribute() {
    assert_format_snapshot!("#[go(comma_ok)]\npub fn ReadBuildInfo() -> Option<Ref<BuildInfo>>");
}

#[test]
fn function_with_body() {
    assert_format_snapshot!("fn add(a: int, b: int) -> int { a + b }");
}

#[test]
fn function_generic() {
    assert_format_snapshot!("fn identity<T>(x: T) -> T { x }");
}

#[test]
fn function_generic_multiple() {
    assert_format_snapshot!("fn pair<A, B>(a: A, b: B) -> (A, B) { (a, b) }");
}

#[test]
fn function_generic_bounds() {
    assert_format_snapshot!("fn print<T: Display>(x: T) { x.to_string() }");
}

#[test]
fn function_multiple_params() {
    assert_format_snapshot!(
        "fn calculate(a: int, b: int, c: int, d: int) -> int { a + b + c + d }"
    );
}

#[test]
fn if_no_spaces() {
    assert_format_snapshot!("fn test() { if true{1}else{2} }");
}

#[test]
fn if_simple() {
    assert_format_snapshot!("fn test() { if true { 1 } else { 2 } }");
}

#[test]
fn if_else_if_chain() {
    assert_format_snapshot!("fn test() { if a { 1 } else if b { 2 } else if c { 3 } else { 4 } }");
}

#[test]
fn if_else_symmetric_breaking() {
    assert_format_snapshot!(
        "fn medium_branch(items: Slice<int>, id: int) -> Slice<int> { let mut result: Slice<int> = []; for item in items { if item == id { result = result.append(item * 100) } else { result = result.append(item) } }; result }"
    );
}

#[test]
fn if_without_else() {
    assert_format_snapshot!("fn test() { if condition { do_something(); } }");
}

#[test]
fn if_let_simple() {
    assert_format_snapshot!("fn test(opt: Option<int>) { if let Some(x) = opt { use(x); } }");
}

#[test]
fn if_let_with_else() {
    assert_format_snapshot!(
        "fn test(opt: Option<int>) -> int { if let Some(x) = opt { x } else { 0 } }"
    );
}

#[test]
fn if_let_else_if_let() {
    assert_format_snapshot!(
        "fn test(a: Option<int>, b: Option<int>) -> int { if let Some(x) = a { x } else if let Some(y) = b { y } else { 0 } }"
    );
}

#[test]
fn let_else_simple() {
    assert_format_snapshot!(
        "fn test(opt: Option<int>) -> int { let Some(x) = opt else { return 0; }; x }"
    );
}

#[test]
fn let_else_with_result() {
    assert_format_snapshot!(
        "fn test(res: Result<int, string>) -> int { let Ok(x) = res else { return 0; }; x }"
    );
}

#[test]
fn impl_simple() {
    assert_format_snapshot!("impl Point { fn origin() -> Point { Point { x: 0, y: 0 } } }");
}

#[test]
fn impl_empty() {
    assert_format_snapshot!("impl Point {}");
}

#[test]
fn impl_comment_before_method() {
    assert_format_snapshot!("struct Foo {}\n\nimpl Foo {\n  // test\n  fn foo() {}\n}");
}

#[test]
fn impl_comment_after_method() {
    assert_format_snapshot!("struct Foo {}\n\nimpl Foo {\n  fn foo() {}\n  // test\n}");
}

#[test]
fn try_block_comment_after() {
    assert_format_snapshot!("fn foo() {\n  try {}\n  // comment\n}");
}

#[test]
fn recover_block_comment_after() {
    assert_format_snapshot!("fn foo() {\n  recover {}\n  // comment\n}");
}

#[test]
fn import_single() {
    assert_format_snapshot!("import \"go:fmt\"");
}

#[test]
fn import_multiple_sorted() {
    assert_format_snapshot!("import \"go:os\"\nimport \"go:fmt\"\nimport \"go:io\"");
}

#[test]
fn import_sort_with_prefix() {
    assert_format_snapshot!(
        "import \"go:crypto\"\nimport \"go:crypto/ecdh\"\nimport \"go:io\"\nimport \"go:math/big\""
    );
}

#[test]
fn import_with_leading_comments() {
    assert_format_snapshot!(
        "// Generated by bindgen\n// Source: bytes\n\nimport \"go:io\"\nimport \"go:fmt\""
    );
}

#[test]
fn import_go_and_local_grouped() {
    assert_format_snapshot!(
        "import \"commands\"\nimport \"go:fmt\"\nimport \"display\"\nimport \"go:os\""
    );
}

#[test]
fn import_user_grouping_overridden() {
    assert_format_snapshot!(
        "import \"go:fmt\"\n\nimport \"commands\"\n\nimport \"go:os\"\n\nimport \"display\""
    );
}

#[test]
fn import_only_local() {
    assert_format_snapshot!("import \"display\"\nimport \"commands\"\nimport \"store\"");
}

#[test]
fn import_only_go() {
    assert_format_snapshot!("import \"go:strings\"\nimport \"go:fmt\"\nimport \"go:os\"");
}

#[test]
fn index_access() {
    assert_format_snapshot!("fn test() { arr[0] }");
}

#[test]
fn interface_simple() {
    assert_format_snapshot!(
        r#"interface Display {
  fn fmt() -> string;
}"#
    );
}

#[test]
fn interface_empty() {
    assert_format_snapshot!("interface Empty {}");
}

#[test]
fn interface_method_with_attribute() {
    assert_format_snapshot!(
        r#"interface Cache {
  #[go(comma_ok)]
  fn Get(key: string) -> Option<Ref<Value>>
  fn Put(key: string, val: Ref<Value>)
}"#
    );
}

#[test]
fn interface_with_parent() {
    assert_format_snapshot!(
        r#"interface Reader {
  impl Closable;
  fn read() -> string;
}"#
    );
}

#[test]
fn let_binding() {
    assert_format_snapshot!("fn test() { let x = 42 }");
}

#[test]
fn let_mut_binding() {
    assert_format_snapshot!("fn test() { let mut counter = 0 }");
}

#[test]
fn let_with_type() {
    assert_format_snapshot!("fn test() { let x: int = 42 }");
}

#[test]
fn let_with_inferred_type() {
    assert_format_snapshot!("fn test() { let x: _ = 42 }");
}

#[test]
fn line_breaking_long_function_signature() {
    assert_format_snapshot!(
        "fn process(first_argument: string, second_argument: int, third_argument: bool, fourth_argument: float) -> string { first_argument }"
    );
}

#[test]
fn line_breaking_long_call_args() {
    assert_format_snapshot!(
        "fn test() { some_function(first_argument, second_argument, third_argument, fourth_argument, fifth_argument) }"
    );
}

#[test]
fn line_breaking_long_binary_chain() {
    assert_format_snapshot!(
        "fn test() { first_value + second_value + third_value + fourth_value + fifth_value + sixth_value }"
    );
}

#[test]
fn line_breaking_long_slice() {
    assert_format_snapshot!(
        "fn test() { [first_element, second_element, third_element, fourth_element, fifth_element, sixth_element] }"
    );
}

#[test]
fn line_breaking_long_tuple() {
    assert_format_snapshot!(
        "fn test() { (first_long_element, second_long_element, third_long_element, fourth_long_element, fifth_long_element) }"
    );
}

#[test]
fn literal_bool() {
    assert_format_snapshot!("fn test() { true }");
}

#[test]
fn literal_float() {
    assert_format_snapshot!("fn test() { 3.14 }");
}

#[test]
fn literal_float_trailing_zero() {
    assert_format_snapshot!("fn test() { 0.0 }");
}

#[test]
fn literal_float_whole_number() {
    assert_format_snapshot!("fn test() { 6.0 }");
}

#[test]
fn literal_float_scientific_notation() {
    assert_format_snapshot!("const MaxFloat64 = 1.7976931348623157e+308");
}

#[test]
fn literal_int() {
    assert_format_snapshot!("fn test() { 42 }");
}

#[test]
fn literal_string() {
    assert_format_snapshot!("fn test() { \"hello world\" }");
}

#[test]
fn literal_string_with_escapes() {
    assert_format_snapshot!("fn test() { \"hello\\nworld\\t!\" }");
}

#[test]
fn literal_char() {
    assert_format_snapshot!("fn test() { 'a' }");
}

#[test]
fn lambda_simple() {
    assert_format_snapshot!("fn test() { let f = |x| x + 1; }");
}

#[test]
fn lambda_no_params() {
    assert_format_snapshot!("fn test() { let f = || 42; }");
}

#[test]
fn lambda_typed() {
    assert_format_snapshot!("fn test() { let f = |x: int| -> int { x + 1 }; }");
}

#[test]
fn lambda_nested() {
    assert_format_snapshot!("fn test() { let f = |x| |y| x + y; }");
}

#[test]
fn lambda_multi_param() {
    assert_format_snapshot!("fn test() { let f = |a, b, c| a + b + c; }");
}

#[test]
fn lambda_as_last_arg() {
    assert_format_snapshot!("fn test() { map(items, |x| x + 1) }");
}

#[test]
fn lambda_with_block() {
    assert_format_snapshot!("fn test() { let f = |x| { let y = x + 1; y * 2 }; }");
}

#[test]
fn loop_break() {
    assert_format_snapshot!("fn test() { loop { break } }");
}

#[test]
fn match_multiline() {
    assert_format_snapshot!(
        "fn test() {\n    match x {\n        Some(a) => a,\n        None => 0,\n    }\n}"
    );
}

#[test]
fn match_simple() {
    assert_format_snapshot!("fn test() { match x { Some(a) => a, None => 0 } }");
}

#[test]
fn match_nested() {
    assert_format_snapshot!(
        "fn test() { match x { Some(a) => match a { 1 => \"one\", _ => \"other\" }, None => \"none\" } }"
    );
}

#[test]
fn match_with_pattern_comment() {
    assert_format_snapshot!(
        r#"fn test() {
  match x {
    Pair(
      a,
      // comment before b
      b,
    ) => a + b,
    _ => 0,
  }
}"#
    );
}

#[test]
fn match_pattern_literal() {
    assert_format_snapshot!("fn test() { match x { 1 => \"one\", 2 => \"two\", _ => \"other\" } }");
}

#[test]
fn match_pattern_struct() {
    assert_format_snapshot!("fn test() { match p { Point { x, y } => x + y } }");
}

#[test]
fn match_pattern_struct_empty() {
    assert_format_snapshot!("fn test() { match p { Empty {} => \"empty\" } }");
}

#[test]
fn match_pattern_struct_rest() {
    assert_format_snapshot!(
        "fn test() { match shape { Shape.Circle { radius, .. } => radius, _ => 0 } }"
    );
}

#[test]
fn match_pattern_struct_renamed() {
    assert_format_snapshot!("fn test() { match p { Point { x: a, y: b } => a + b } }");
}

#[test]
fn match_pattern_tuple() {
    assert_format_snapshot!("fn test() { match pair { (a, b) => a + b } }");
}

#[test]
fn match_pattern_unit() {
    assert_format_snapshot!("fn test() { match u { () => \"unit\" } }");
}

#[test]
fn match_pattern_wildcard() {
    assert_format_snapshot!("fn test() { match x { _ => \"anything\" } }");
}

#[test]
fn match_pattern_slice_empty() {
    assert_format_snapshot!("fn test() { match items { [] => \"empty\", _ => \"not empty\" } }");
}

#[test]
fn match_pattern_slice_fixed() {
    assert_format_snapshot!("fn test() { match items { [a, b, c] => a + b + c, _ => 0 } }");
}

#[test]
fn match_pattern_slice_rest() {
    assert_format_snapshot!("fn test() { match items { [first, ..rest] => first, [] => 0 } }");
}

#[test]
fn match_pattern_slice_rest_discard() {
    assert_format_snapshot!("fn test() { match items { [first, ..] => first, [] => 0 } }");
}

#[test]
fn method_call() {
    assert_format_snapshot!("fn test() { obj.method() }");
}

#[test]
fn module_multiple_definitions() {
    assert_format_snapshot!(
        "struct Point { x: int, y: int }\n\nfn origin() -> Point { Point { x: 0, y: 0 } }\n\nfn add(a: Point, b: Point) -> Point { Point { x: a.x + b.x, y: a.y + b.y } }"
    );
}

#[test]
fn module_imports_and_definitions() {
    assert_format_snapshot!(
        "import \"go:os\"\nimport \"go:fmt\"\n\nfn main() { fmt.Println(\"hello\") }"
    );
}

#[test]
fn module_only_comments() {
    assert_format_snapshot!("// Generated by bindgen\n// Source: crypto/hkdf");
}

#[test]
fn module_trailing_comment() {
    assert_format_snapshot!(
        r#"fn main() {}

// end of file"#
    );
}

#[test]
fn paren_expression() {
    assert_format_snapshot!("fn test() { (1 + 2) * 3 }");
}

#[test]
fn reference() {
    assert_format_snapshot!("fn test() { &value }");
}

#[test]
fn return_simple() {
    assert_format_snapshot!("fn test() { return 42 }");
}

#[test]
fn return_unit() {
    assert_format_snapshot!("fn test() { return }");
}

#[test]
fn rawgo_simple() {
    assert_format_snapshot!("fn test() { @rawgo(\"fmt.Println()\") }");
}

#[test]
fn select_simple() {
    assert_format_snapshot!(
        "fn test() {\n    select {\n        let x = rx.Receive() => handle(x),\n        _ => default(),\n    }\n}"
    );
}

#[test]
fn select_send() {
    assert_format_snapshot!(
        r#"fn test() {
  select {
    tx.Send(42) => done(),
    _ => timeout(),
  }
}"#
    );
}

#[test]
fn select_match_receive() {
    assert_format_snapshot!(
        r#"fn test() {
  select {
    match ch.receive() {
      Some(v) => process(v),
      None => handle_close(),
    },
    _ => default(),
  }
}"#
    );
}

#[test]
fn select_match_receive_multiline_arms() {
    assert_format_snapshot!(
        r#"fn test() {
  select {
    match ch.receive() {
      Some(msg) => {
        log(msg);
        process(msg);
      },
      None => cleanup(),
    },
    _ => {},
  }
}"#
    );
}

#[test]
fn select_multiple_with_match() {
    assert_format_snapshot!(
        r#"fn test() {
  select {
    let Some(v) = ch1.receive() => handle(v),
    match ch2.receive() {
      Some(x) => process(x),
      None => close(),
    },
    _ => timeout(),
  }
}"#
    );
}

#[test]
fn slice_empty() {
    assert_format_snapshot!("fn test() { [] }");
}

#[test]
fn slice_simple() {
    assert_format_snapshot!("fn test() { [1, 2, 3] }");
}

#[test]
fn slice_nested() {
    assert_format_snapshot!("fn test() { [[1, 2], [3, 4], [5, 6]] }");
}

#[test]
fn slice_with_comment() {
    assert_format_snapshot!(
        r#"fn test() {
  [
    a,
    // comment before b
    b,
    c,
  ]
}"#
    );
}

#[test]
fn attribute_on_struct() {
    assert_format_snapshot!("#[json]\nstruct Person { name: string }");
}

#[test]
fn attribute_multiple_on_struct() {
    assert_format_snapshot!("#[json]\n#[xml]\nstruct Config { value: int }");
}

#[test]
fn attribute_on_field() {
    assert_format_snapshot!(
        r#"struct Person {
  #[json("firstName")]
  first_name: string,
}"#
    );
}

#[test]
fn attribute_with_flag() {
    assert_format_snapshot!(
        r#"struct Item {
  #[json(omitempty)]
  value: int,
}"#
    );
}

#[test]
fn attribute_with_negated_flag() {
    assert_format_snapshot!(
        r#"struct Item {
  #[json(!omitempty)]
  value: int,
}"#
    );
}

#[test]
fn attribute_with_multiple_args() {
    assert_format_snapshot!(
        r#"struct Item {
  #[json(snake_case, omitempty)]
  value: int,
}"#
    );
}

#[test]
fn attribute_with_raw_string() {
    assert_format_snapshot!(
        r#"struct User {
  #[tag(`validate:"required,email"`)]
  email: string,
}"#
    );
}

#[test]
fn attribute_struct_and_fields() {
    assert_format_snapshot!(
        r#"#[json]
struct Person {
  #[json("firstName")]
  first_name: string,
  #[json(omitempty)]
  age: int,
}"#
    );
}

#[test]
fn struct_empty() {
    assert_format_snapshot!("struct Empty {}");
}

#[test]
fn struct_empty_with_comment() {
    assert_format_snapshot!(
        r#"struct Empty {
  // no fields yet
}"#
    );
}

#[test]
fn struct_call_empty() {
    assert_format_snapshot!("fn test() { Empty {} }");
}

#[test]
fn struct_instantiation() {
    assert_format_snapshot!("fn test() { Point { x: 1, y: 2 } }");
}

#[test]
fn struct_instantiation_shorthand() {
    assert_format_snapshot!("fn test() { let x = 1; let y = 2; Point { x, y } }");
}

#[test]
fn struct_instantiation_spread() {
    assert_format_snapshot!("fn test() { Point { x: 1, ..other } }");
}

#[test]
fn struct_instantiation_spread_with_comment() {
    assert_format_snapshot!(
        r#"fn test() {
  Point {
    x: 1,
    // inherit the rest
    ..other
  }
}"#
    );
}

#[test]
fn struct_public() {
    assert_format_snapshot!("pub struct Point { x: int, y: int }");
}

#[test]
fn struct_with_fields() {
    assert_format_snapshot!("struct Point { x: int, y: int }");
}

#[test]
fn struct_with_generics() {
    assert_format_snapshot!("struct Container<T> { value: T }");
}

#[test]
fn struct_pub_fields() {
    assert_format_snapshot!("struct Point { pub x: int, pub y: int }");
}

#[test]
fn struct_mixed_visibility_fields() {
    assert_format_snapshot!(
        "struct Config { pub name: string, secret: string, pub enabled: bool }"
    );
}

#[test]
fn struct_tuple() {
    assert_format_snapshot!("pub struct Format(int)");
}

#[test]
fn struct_multiline_no_trailing_space() {
    assert_format_snapshot!(
        "struct Params {\n  pub alpha: Option<Ref<big.Int>>,\n  pub beta: Option<Ref<big.Int>>,\n  pub gamma: Option<Ref<big.Int>>,\n  pub size: int,\n}"
    );
}

#[test]
fn task_launch() {
    assert_format_snapshot!("fn test() { task do_work() }");
}

#[test]
fn defer_simple() {
    assert_format_snapshot!("fn test() { defer cleanup(); }");
}

#[test]
fn defer_block() {
    assert_format_snapshot!(
        r#"fn test() {
  defer {
    cleanup();
  };
}"#
    );
}

#[test]
fn defer_multiple() {
    assert_format_snapshot!(
        r#"fn test() {
  defer {
    first();
  };
  defer {
    second();
  };
  work();
}"#
    );
}

#[test]
fn range_exclusive() {
    assert_format_snapshot!("fn test() { 0..10 }");
}

#[test]
fn range_inclusive() {
    assert_format_snapshot!("fn test() { 0..=10 }");
}

#[test]
fn range_in_for() {
    assert_format_snapshot!("fn test() { for i in 0..3 { print(i) } }");
}

#[test]
fn cast_simple() {
    assert_format_snapshot!("fn test() { x as int }");
}

#[test]
fn cast_with_generic() {
    assert_format_snapshot!("fn test() { value as Option<int> }");
}

#[test]
fn propagate_expression() {
    assert_format_snapshot!("fn test() { fallible()? }");
}

#[test]
fn tuple_simple() {
    assert_format_snapshot!("fn test() { (1, 2, 3) }");
}

#[test]
fn tuple_with_comment() {
    assert_format_snapshot!(
        r#"fn test() {
  (
    a,
    // comment before b
    b,
    c,
  )
}"#
    );
}

#[test]
fn type_alias_generic() {
    assert_format_snapshot!("type Result<T> = Option<T>");
}

#[test]
fn type_alias_simple() {
    assert_format_snapshot!("type UserId = int");
}

#[test]
fn type_alias_function() {
    assert_format_snapshot!("type Handler = fn(int, string) -> bool");
}

#[test]
fn type_alias_opaque() {
    assert_format_snapshot!("type   Point");
}

#[test]
fn type_alias_opaque_generic() {
    assert_format_snapshot!("type  Slice<  T  >");
}

#[test]
fn unary_negation() {
    assert_format_snapshot!("fn test() { -42 }");
}

#[test]
fn unary_not() {
    assert_format_snapshot!("fn test() { !flag }");
}

#[test]
fn unary_deref() {
    assert_format_snapshot!("fn test() { ptr.* }");
}

#[test]
fn unit_literal() {
    assert_format_snapshot!("fn test() { () }");
}

#[test]
fn while_loop() {
    assert_format_snapshot!("fn test() { while condition { do_something() } }");
}

#[test]
fn while_let_simple() {
    assert_format_snapshot!("fn test(opt: Option<int>) { while let Some(x) = opt { use(x); } }");
}

#[test]
fn while_let_tuple_pattern() {
    assert_format_snapshot!(
        "fn test(opt: Option<(int, int)>) { while let Some((a, b)) = opt { use(a + b); } }"
    );
}

#[test]
fn or_pattern_simple() {
    assert_format_snapshot!("fn test() { match x { 1 | 2 | 3 => \"small\", _ => \"other\" } }");
}

#[test]
fn or_pattern_enum() {
    assert_format_snapshot!(
        "fn test() { match color { Red | Green => \"warm\", Blue => \"cool\" } }"
    );
}

#[test]
fn or_pattern_with_binding() {
    assert_format_snapshot!("fn test() { match opt { Some(x) | Some(x) => x, None => 0 } }");
}

#[test]
fn or_pattern_multiline() {
    assert_format_snapshot!(
        r#"fn test() {
  match x {
    1 | 2 | 3 => "small",
    4 | 5 | 6 => "medium",
    _ => "large",
  }
}"#
    );
}

#[test]
fn or_pattern_strings() {
    assert_format_snapshot!(
        "fn test() { match s { \"yes\" | \"y\" => true, \"no\" | \"n\" => false, _ => false } }"
    );
}

#[test]
fn compound_assignment_add() {
    assert_format_snapshot!("fn test() { x += 5 }");
}

#[test]
fn compound_assignment_sub() {
    assert_format_snapshot!("fn test() { x -= 3 }");
}

#[test]
fn compound_assignment_mul() {
    assert_format_snapshot!("fn test() { x *= 2 }");
}

#[test]
fn compound_assignment_div() {
    assert_format_snapshot!("fn test() { x /= 4 }");
}

#[test]
fn compound_assignment_rem() {
    assert_format_snapshot!("fn test() { x %= 3 }");
}

#[test]
fn method_chain_two_calls() {
    assert_format_snapshot!(
        "fn test() { let result = Some(42).map(|x: int| -> int { x * 2 }).unwrap_or(0) }"
    );
}

#[test]
fn method_chain_short_stays_inline() {
    assert_format_snapshot!("fn test() { foo.bar().baz() }");
}

#[test]
fn unit_return_type_annotation() {
    assert_format_snapshot!("fn do_nothing() -> () { () }");
}

#[test]
fn unit_in_result_type_param() {
    assert_format_snapshot!("fn fallible() -> Result<(), string> { Ok(()) }");
}

#[test]
fn unit_in_option_type_param() {
    assert_format_snapshot!("fn maybe() -> Option<()> { Some(()) }");
}

#[test]
fn function_with_mut_parameter() {
    assert_format_snapshot!(
        "fn process(mut items: Slice<int>, count: int) -> Slice<int> { items }"
    );
}

#[test]
fn call_with_spread_arg() {
    assert_format_snapshot!("fn test() { foo(..args) }");
}

#[test]
fn call_with_leading_args_and_spread_arg() {
    assert_format_snapshot!("fn test() { foo(a, b, ..args) }");
}
