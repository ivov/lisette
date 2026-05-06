use crate::spec::infer::*;

#[test]
fn if_else_returning_int() {
    infer("if true { 1 } else { 2 }").assert_type_int();
}

#[test]
fn if_else_returning_bool() {
    infer("if true { true } else { false }").assert_type_bool();
}

#[test]
fn if_else_returning_string() {
    infer(r#"if false { "yes" } else { "no" }"#).assert_type_string();
}

#[test]
fn if_else_with_condition_expression() {
    infer("if 5 > 3 { 10 } else { 20 }").assert_type_int();
}

#[test]
fn if_else_with_expressions_in_branches() {
    infer("if true { 1 + 2 } else { 3 * 4 }").assert_type_int();
}

#[test]
fn nested_if_in_then_branch() {
    infer(
        r#"
    if true {
      if false { 1 } else { 2 }
    } else {
      3
    }
        "#,
    )
    .assert_type_int();
}

#[test]
fn nested_if_in_else_branch() {
    infer(
        r#"
    if false {
      1
    } else {
      if true { 2 } else { 3 }
    }
        "#,
    )
    .assert_type_int();
}

#[test]
fn deeply_nested_if() {
    infer(
        r#"
    if true {
      if true {
        if true { 1 } else { 2 }
      } else {
        3
      }
    } else {
      4
    }
        "#,
    )
    .assert_type_int();
}

#[test]
fn if_else_in_let_binding() {
    infer(
        r#"{
    let x = if true { 5 } else { 10 };
    x
    }"#,
    )
    .assert_type_int();
}

#[test]
fn if_else_in_arithmetic() {
    infer("{ let x = if true { 5 } else { 10 }; x + 3 }").assert_type_int();
}

#[test]
fn if_else_in_comparison() {
    infer("{ let x = if true { 5 } else { 10 }; x > 7 }").assert_type_bool();
}

#[test]
fn if_else_as_function_arg() {
    infer(
        r#"{
    let double = |x: int| -> int { x * 2 };
    let arg = if true { 5 } else { 10 };
    double(arg)
    }"#,
    )
    .assert_type_int();
}

#[test]
fn if_else_with_variable_condition() {
    infer("{ let cond = true; if cond { 1 } else { 2 } }").assert_type_int();
}

#[test]
fn if_else_returning_tuple() {
    infer("if true { (1, 2) } else { (3, 4) }").assert_type_tuple(int_type(), int_type());
}

#[test]
fn if_else_returning_slice() {
    infer("if true { [1, 2, 3] } else { [4, 5, 6] }").assert_type_slice_of_ints();
}

#[test]
fn if_without_else_in_function() {
    infer(
        r#"
    fn test() -> int {
      if true { return 1; }
      return 2;
    }
        "#,
    )
    .assert_function_type(vec![], int_type());
}

#[test]
fn if_condition_not_bool() {
    infer("if 42 { 1 } else { 2 }").assert_type_mismatch();
}

#[test]
fn if_condition_string() {
    infer(r#"if "yes" { 1 } else { 2 }"#).assert_type_mismatch();
}

#[test]
fn else_if_chain() {
    infer(
        r#"
    if false {
      1
    } else {
      if true { 2 } else { 3 }
    }
        "#,
    )
    .assert_type_int();
}

#[test]
fn multiple_else_if() {
    infer(
        r#"
    if false {
      1
    } else {
      if false {
        2
      } else {
        if true { 3 } else { 4 }
      }
    }
        "#,
    )
    .assert_type_int();
}

#[test]
fn if_else_with_block_branches() {
    infer(
        r#"{
    if true {
      let x = 5;
      x + 1
    } else {
      let y = 10;
      y - 1
    }
    }"#,
    )
    .assert_type_int();
}

#[test]
fn if_else_with_multiple_statements() {
    infer(
        r#"{
    if true {
      let a = 1;
      let b = 2;
      a + b
    } else {
      let c = 3;
      let d = 4;
      c + d
    }
    }"#,
    )
    .assert_type_int();
}

#[test]
fn match_int_literals() {
    infer(
        r#"
    match 1 {
      1 => "one",
      2 => "two",
      _ => "other",
    }
        "#,
    )
    .assert_type_string();
}

#[test]
fn match_bool_literals() {
    infer(
        r#"
    match true {
      true => 1,
      false => 0,
    }
        "#,
    )
    .assert_type_int();
}

#[test]
fn match_with_wildcard() {
    infer(
        r#"
    match 5 {
      _ => 42,
    }
        "#,
    )
    .assert_type_int();
}

#[test]
fn match_variable_subject() {
    infer(
        r#"{
    let x = 5;
    match x {
      1 => "one",
      _ => "other",
    }
    }"#,
    )
    .assert_type_string();
}

#[test]
fn match_with_expressions_in_arms() {
    infer(
        r#"
    match 1 {
      1 => 10 + 5,
      _ => 20 * 2,
    }
        "#,
    )
    .assert_type_int();
}

#[test]
fn match_in_let_binding() {
    infer(
        r#"{
    let result = match 1 {
      1 => "one",
      _ => "other",
    };
    result
    }"#,
    )
    .assert_type_string();
}

#[test]
fn match_in_arithmetic() {
    infer(
        r#"{
    let x = match 1 {
      1 => 10,
      _ => 20,
    };
    x + 5
    }"#,
    )
    .assert_type_int();
}

#[test]
fn match_as_function_argument() {
    infer(
        r#"{
    let double = |x: int| -> int { x * 2 };
    let arg = match 1 {
      1 => 5,
      _ => 10,
    };
    double(arg)
    }"#,
    )
    .assert_type_int();
}

#[test]
fn nested_match_in_arm() {
    infer(
        r#"
    match 1 {
      1 => match true {
        true => "yes",
        false => "no",
      },
      _ => "other",
    }
        "#,
    )
    .assert_type_string();
}

#[test]
fn match_with_match_subject() {
    infer(
        r#"
    match (match 1 {
      1 => 10,
      _ => 20,
    }) {
      10 => "ten",
      _ => "other",
    }
        "#,
    )
    .assert_type_string();
}

#[test]
fn match_with_block_arms() {
    infer(
        r#"
    match 1 {
      1 => {
        let x = 10;
        x + 5
      },
      _ => {
        let y = 20;
        y - 5
      },
    }
        "#,
    )
    .assert_type_int();
}

#[test]
fn match_returning_tuple() {
    infer(
        r#"
    match 1 {
      1 => (1, 2),
      _ => (3, 4),
    }
        "#,
    )
    .assert_type_tuple(int_type(), int_type());
}

#[test]
fn match_returning_slice() {
    infer(
        r#"
    match 1 {
      1 => [1, 2, 3],
      _ => [4, 5, 6],
    }
        "#,
    )
    .assert_type_slice_of_ints();
}

#[test]
fn match_arms_with_mismatched_types_in_void_context() {
    infer(
        r#"
    match 1 {
      1 => 42,
      _ => true,
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn match_pattern_type_mismatch() {
    infer(
        r#"
    match true {
      1 => "number",
      _ => "other",
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn match_on_function_type_fails() {
    infer(
        r#"{
    fn my_function() -> int { 42 }
    match my_function {
      _ => "matched",
    }
  }"#,
    )
    .assert_infer_code("invalid_pattern");
}

#[test]
fn for_loop_infers_binding_type() {
    infer(
        r#"{
    let xs = [1, 2, 3];
    for x in xs {
      let y = x + 10;
    }
  }"#,
    )
    .assert_no_errors();
}

#[test]
fn for_loop_with_annotation_matches_inferred() {
    infer(
        r#"{
    let xs = [1, 2, 3];
    for x: int in xs {
      let y = x + 10;
    }
  }"#,
    )
    .assert_no_errors();
}

#[test]
fn for_loop_with_wrong_annotation_fails() {
    infer(
        r#"{
    let xs = [1, 2, 3];
    for x: string in xs {
      let y = x;
    }
  }"#,
    )
    .assert_type_mismatch();
}

#[test]
fn for_loop_over_string_runes() {
    infer(
        r#"{
    let s = "hello";
    for ch in s.runes() {
      let c = ch;
    }
  }"#,
    )
    .assert_no_errors();
}

#[test]
fn for_loop_over_string_rejected() {
    infer(
        r#"{
    let s = "hello";
    for _ch in s {}
  }"#,
    )
    .assert_infer_code("string_not_iterable");
}

#[test]
fn for_loop_over_range_exclusive() {
    infer(
        r#"{
    for i in 0..5 {
      let x = i + 1;
    }
  }"#,
    )
    .assert_no_errors();
}

#[test]
fn for_loop_over_range_inclusive() {
    infer(
        r#"{
    for i in 0..=5 {
      let x = i + 1;
    }
  }"#,
    )
    .assert_no_errors();
}

#[test]
fn for_loop_over_range_from() {
    infer(
        r#"{
    for i in 0.. {
      if i > 10 { break; }
    }
  }"#,
    )
    .assert_no_errors();
}

#[test]
fn for_loop_range_binding_is_int() {
    infer(
        r#"{
    for i in 0..5 {
      let x: int = i;
    }
  }"#,
    )
    .assert_no_errors();
}

#[test]
fn for_loop_range_binding_wrong_type_fails() {
    infer(
        r#"{
    for i in 0..5 {
      let x: string = i;
    }
  }"#,
    )
    .assert_type_mismatch();
}

#[test]
fn tight_float_range_parses_correctly() {
    infer(
        r#"
    fn test() {
      let r: Range<float64> = 0.0..1.5
      let _ = r
    }
  "#,
    )
    .assert_no_errors();
}

#[test]
fn loop_basic() {
    infer(
        r#"
    fn test() {
      loop {
        let x = 42;
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn while_loop_basic() {
    infer(
        r#"
    fn test() {
      while true {
        let x = 42;
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn while_loop_with_condition() {
    infer(
        r#"{
    let mut counter = 0;
    while counter < 10 {
      counter = counter + 1;
    }
  }"#,
    )
    .assert_no_errors();
}

#[test]
fn for_loop_over_map_with_tuple_destructuring() {
    infer(
        r#"
    fn test(map: Map<int, string>) {
      for (k, v) in map {
        let key = k;
        let val = v;
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn for_loop_over_map_without_tuple_fails() {
    infer(
        r#"
    fn test(map: Map<int, string>) {
      for x in map {
        let item = x;
      }
    }
        "#,
    )
    .assert_infer_code("invalid_pattern");
}

#[test]
fn for_loop_over_non_iterable() {
    infer(
        r#"{
    let x = 42;
    for i in x {
      let item = i;
    }
  }"#,
    )
    .assert_infer_code("not_iterable");
}

#[test]
fn for_loop_over_slice_of_tuples_with_identifier() {
    infer(
        r#"
    fn test() {
      let items: Slice<(string, int)> = [("a", 1), ("b", 2)];
      for row in items {
        let (name, value) = row;
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn while_body_allows_non_unit_tail() {
    infer(
        r#"
    fn test() {
      while true {
        1
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn for_body_allows_non_unit_tail() {
    infer(
        r#"
    fn test() {
      for x in [1, 2, 3] {
        x * 2
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn while_body_accepts_unit() {
    infer(
        r#"
    fn test() {
      while true {
        let x = 1;
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn for_body_accepts_unit() {
    infer(
        r#"
    fn test() {
      for x in [1, 2, 3] {
        let y = x;
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn if_let_without_else_allows_non_unit_body() {
    infer(
        r#"{
    let a: Option<int> = Some(10);
    if let Some(x) = a {
      x + 1
    }
  }"#,
    )
    .assert_no_errors();
}

#[test]
fn match_in_for_loop_body_allows_non_unit_result() {
    infer(
        r#"
    enum Status { Active, Inactive }

    fn returns_result(s: Status) -> Result<int, string> {
      match s {
        Status.Active => Ok(1),
        Status.Inactive => Err("inactive"),
      }
    }

    fn test() {
      let items = [Status.Active, Status.Inactive];
      for item in items {
        match item {
          Status.Active => returns_result(item),
          Status.Inactive => returns_result(item),
        }
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn match_in_while_loop_body_allows_non_unit_result() {
    infer(
        r#"
    fn test() {
      let mut x = 0;
      while x < 3 {
        match x {
          0 => "zero",
          1 => "one",
          _ => "other",
        };
        x = x + 1;
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn match_in_statement_position_allows_non_unit_result() {
    infer(
        r#"
    fn test() -> int {
      let x = 1;
      match x {
        1 => "one",
        _ => "other",
      };
      42
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn match_in_statement_position_arms_still_unify() {
    infer(
        r#"
    fn test() {
      let x = 1;
      match x {
        1 => "one",
        2 => "two",
        _ => "other",
      };
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn match_in_statement_position_allows_mismatched_arms() {
    infer(
        r#"
    fn test() {
      let x = 1;
      match x {
        1 => "one",
        _ => 42,
      };
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn match_in_statement_position_void_arms() {
    infer(
        r#"
    enum Action { Print(string), Skip }

    fn test(action: Action) {
      match action {
        Action.Print(s) => s,
        Action.Skip => (),
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn match_in_expression_position_requires_arm_unification() {
    infer(
        r#"
    fn test() -> string {
      let x = 1;
      match x {
        1 => "one",
        _ => 42,
      }
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn match_in_expression_position_unifies_with_expected() {
    infer(
        r#"
    fn test() -> int {
      let x = 1;
      match x {
        1 => "one",
        _ => "other",
      }
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn task_returns_unit() {
    infer(
        r#"
    fn compute() -> int { 42 }

    fn test() {
      task compute();
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn task_body_can_be_any_type() {
    infer(
        r#"
    fn returns_int() -> int { 42 }
    fn returns_string() -> string { "hello" }
    fn returns_bool() -> bool { true }

    fn test() {
      task returns_int();
      task returns_string();
      task returns_bool();
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn select_receive_extracts_element_type() {
    infer(
        r#"
    fn test() -> int {
      let ch = Channel.new<int>();
      let result = select {
        let Some(x) = ch.receive() => x + 10,
        _ => 0,
      };
      result
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn select_receive_with_tuple_pattern() {
    infer(
        r#"
    type Pair = (int, int)

    fn test() -> int {
      let ch: Channel<Pair> = Channel.new();
      select {
        let Some((a, b)) = ch.receive() => a + b,
        _ => 0,
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn select_send_arm_with_expression() {
    infer(
        r#"{
    select {
      1 + 2 => 10,
      _ => 20,
    }
  }"#,
    )
    .assert_infer_code("expected_channel_send");
}

#[test]
fn select_non_channel_receive_method() {
    infer(
        r#"
    struct Foo { x: int }

    impl Foo {
      fn receive(self) -> Option<int> { None }
    }

    fn test() -> int {
      let f = Foo { x: 1 }
      select {
        let Some(v) = f.receive() => v,
        _ => 0,
      }
    }
        "#,
    )
    .assert_infer_code("expected_channel_receive");
}

#[test]
fn select_non_channel_send_method() {
    infer(
        r#"
    struct Foo { x: int }

    impl Foo {
      fn send(self, v: int) -> bool { true }
    }

    fn test() {
      let f = Foo { x: 1 }
      select {
        f.send(1) => { let _ = 0 },
        _ => { let _ = 1 },
      }
    }
        "#,
    )
    .assert_infer_code("expected_channel_send");
}

#[test]
fn select_multiple_receivers() {
    infer(
        r#"
    fn test() -> int {
      let ch1 = Channel.new<int>();
      let ch2 = Channel.new<string>();
      let result = select {
        match ch1.receive() {
          Some(x) => 1,
          None => 3,
        },
        match ch2.receive() {
          Some(y) => 2,
          None => 3,
        },
        _ => 3,
      };
      result
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn select_wildcard_only() {
    infer(
        r#"{
    let x = select {
      _ => 42,
    };
    x
  }"#,
    )
    .assert_type_int();
}

#[test]
fn select_arm_type_mismatch() {
    infer(
        r#"
    fn test() -> int {
      let ch = Channel.new<int>();
      select {
        let Some(x) = ch.receive() => x,
        _ => "wrong",
      }
    }
  "#,
    )
    .assert_type_mismatch();
}

#[test]
fn task_in_select() {
    infer(
        r#"
    fn process(n: int) {}

    fn test() -> int {
      let ch = Channel.new<int>();
      select {
        let Some(x) = ch.receive() => {
          task process(x)
          x + 1
        },
        _ => 0,
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn consecutive_tasks_without_semicolons() {
    infer(
        r#"
    fn compute() {}
    fn process() {}

    fn test() {
      task compute()
      task process()
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn consecutive_select_without_semicolons() {
    infer(
        r#"
    fn test() {
      select { _ => 1 }
      select { _ => 2 }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn task_does_not_break_greater_than_operator() {
    infer(
        r#"
    fn compute() -> int { 42 }

    fn test() -> bool {
      task compute()
      let x = 5 > 3;
      x
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn undefined_type_in_annotation() {
    infer(
        r#"{
    let x: UndefinedType = 42;
  }"#,
    )
    .assert_resolve_code("type_not_found");
}

#[test]
fn undefined_type_in_function_param() {
    infer(
        r#"{
    fn foo(x: UndefinedType) {
    }
  }"#,
    )
    .assert_resolve_code("type_not_found");
}

#[test]
fn undefined_type_in_function_return() {
    infer(
        r#"{
    fn foo() -> UndefinedType {
      return 42;
    }
  }"#,
    )
    .assert_resolve_code("type_not_found");
}

#[test]
fn undefined_type_in_struct_field() {
    infer(
        r#"{
    struct Point {
      x: UndefinedType,
    }
  }"#,
    )
    .assert_resolve_code("type_not_found");
}

#[test]
fn undefined_type_in_enum_variant() {
    infer(
        r#"{
    enum Outcome {
      Ok(UndefinedType),
    }
  }"#,
    )
    .assert_resolve_code("type_not_found");
}

#[test]
fn undefined_enum_variant() {
    infer(
        r#"{
    let x = Maybe(42);
  }"#,
    )
    .assert_resolve_code("name_not_found");
}

#[test]
fn enum_variant_arity_too_few_args() {
    infer(
        r#"{
    let x = Ok();
  }"#,
    )
    .assert_infer_code("arg_count_mismatch");
}

#[test]
fn enum_variant_arity_too_many_args() {
    infer(
        r#"{
    let x = Some(42, 43);
  }"#,
    )
    .assert_infer_code("arg_count_mismatch");
}

#[test]
fn missing_required_struct_field() {
    infer(
        r#"{
    struct Point { x: int, y: int }
    let p = Point { x: 1 };
  }"#,
    )
    .assert_infer_code("missing_struct_fields");
}

#[test]
fn extra_struct_field() {
    infer(
        r#"{
    struct Point { x: int, y: int }
    let p = Point { x: 1, y: 2, z: 3 };
  }"#,
    )
    .assert_infer_code("member_not_found");
}

#[test]
fn accessing_nonexistent_field() {
    infer(
        r#"{
    struct Point { x: int, y: int }
    let p = Point { x: 1, y: 2 };
    p.z
  }"#,
    )
    .assert_infer_code("member_not_found");
}

#[test]
fn updating_nonexistent_field() {
    infer(
        r#"{
    struct Point { x: int, y: int }
    let mut p = Point { x: 1, y: 2 };
    p.z = 3;
  }"#,
    )
    .assert_infer_code("member_not_found");
}

#[test]
fn method_not_found_on_int() {
    infer(
        r#"{
    let x = 42;
    x.nonexistent()
  }"#,
    )
    .assert_infer_code("member_not_found");
}

#[test]
fn method_not_found_on_string() {
    infer(
        r#"{
    let s = "hello";
    s.nonexistent()
  }"#,
    )
    .assert_infer_code("member_not_found");
}

#[test]
fn method_not_found_on_struct() {
    infer(
        r#"{
    struct Point { x: int, y: int }
    let p = Point { x: 1, y: 2 };
    p.distance()
  }"#,
    )
    .assert_infer_code("member_not_found");
}

#[test]
fn method_not_found_on_generic_type() {
    infer(
        r#"{
    let opt = Some(42);
    opt.nonexistent()
  }"#,
    )
    .assert_infer_code("member_not_found");
}

#[test]
fn function_called_with_too_few_args() {
    infer(
        r#"{
    fn add(x: int, y: int) -> int {
      return x + y;
    }
    add(5)
  }"#,
    )
    .assert_infer_code("arg_count_mismatch");
}

#[test]
fn function_called_with_too_many_args() {
    infer(
        r#"{
    fn add(x: int, y: int) -> int {
      return x + y;
    }
    add(5, 10, 15)
  }"#,
    )
    .assert_infer_code("arg_count_mismatch");
}

#[test]
fn builtin_method_not_found() {
    infer(
        r#"{
    let xs = [1, 2, 3];
    xs.nonexistent()
  }"#,
    )
    .assert_infer_code("member_not_found");
}

#[test]
fn return_in_top_level_block_type_error() {
    infer(
        r#"
    let x = {
      return 42;
    };
    "#,
    )
    .assert_type_mismatch();
}

#[test]
fn negation_on_string() {
    infer(r#"-"hello""#).assert_infer_code("type_mismatch");
}

#[test]
fn negation_on_bool() {
    infer(r#"-true"#).assert_infer_code("type_mismatch");
}

#[test]
fn not_on_int() {
    infer(r#"!42"#).assert_infer_code("type_mismatch");
}

#[test]
fn not_on_string() {
    infer(r#"!"hello""#).assert_infer_code("type_mismatch");
}

#[test]
fn index_non_slice_with_int() {
    infer(
        r#"{
    let x = 42;
    x[0]
  }"#,
    )
    .assert_infer_code("not_indexable");
}

#[test]
fn index_slice_with_string() {
    infer(
        r#"{
    let xs = [1, 2, 3];
    xs["index"]
  }"#,
    )
    .assert_type_mismatch();
}

#[test]
fn undefined_variable() {
    infer(
        r#"{
    let x = undefined_var;
  }"#,
    )
    .assert_resolve_code("name_not_found");
}

#[test]
fn undefined_variable_in_function() {
    infer(
        r#"{
    fn foo() {
      return undefined_var;
    }
  }"#,
    )
    .assert_resolve_code("name_not_found");
}

#[test]
fn variable_used_before_declaration() {
    infer(
        r#"{
    let x = y;
    let y = 42;
  }"#,
    )
    .assert_resolve_code("name_not_found");
}

#[test]
fn undefined_function() {
    infer(
        r#"{
    undefined_function()
  }"#,
    )
    .assert_resolve_code("name_not_found");
}

#[test]
fn match_on_undefined_enum_variant() {
    infer(
        r#"{
    match Some(42) {
      Maybe(x) => x,
    }
  }"#,
    )
    .assert_resolve_code("name_not_found");
}

#[test]
fn match_pattern_wrong_arity() {
    infer(
        r#"{
    match Some(42) {
      Some(x, y) => x,
    }
  }"#,
    )
    .assert_infer_code("arg_count_mismatch");
}

#[test]
fn match_struct_pattern_missing_field() {
    infer(
        r#"{
    struct Point { x: int, y: int }
    let p = Point { x: 1, y: 2 };
    match p {
      Point { z } => z,
    }
  }"#,
    )
    .assert_infer_code("member_not_found");
}

#[test]
fn tuple_access_out_of_bounds() {
    infer(
        r#"{
    let t = (1, 2);
    t.2
  }"#,
    )
    .assert_infer_code("member_not_found");
}

#[test]
fn if_branches_different_types() {
    infer(
        r#"
    fn test() -> int {
      if true {
        42
      } else {
        "hello"
      }
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn match_arms_different_types_in_void_context() {
    infer(
        r#"{
    match Some(42) {
      Some(x) => x,
      None => "default",
    }
  }"#,
    )
    .assert_no_errors();
}

#[test]
fn generic_type_wrong_param_count() {
    infer(
        r#"{
    let x: Option<int, string> = Some(42);
  }"#,
    )
    .assert_infer_code("type_arg_count_mismatch");
}

#[test]
fn non_generic_type_with_params() {
    infer(
        r#"{
    struct Point { x: int, y: int }
    let p: Point<int> = Point { x: 1, y: 2 };
  }"#,
    )
    .assert_infer_code("type_arg_count_mismatch");
}

#[test]
fn select_receive_from_non_channel_should_fail() {
    infer(
        r#"
    fn test() -> int {
      let x = 42;
      select {
        let y = x => y,
      }
    }
        "#,
    )
    .assert_infer_code("expected_channel_receive");
}

#[test]
fn rawgo_directive() {
    infer(
        r#"
    fn test() {
      @rawgo("fmt.Println(1)");
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn match_slice_pattern_empty() {
    infer(
        r#"{
    let test = |items: Slice<int>| -> string {
      match items {
        [] => "empty",
        _ => "not empty",
      }
    };
    test([1, 2, 3])
    }"#,
    )
    .assert_no_errors()
    .assert_type_string();
}

#[test]
fn match_slice_pattern_with_rest() {
    infer(
        r#"{
    let test = |items: Slice<int>| -> int {
      match items {
        [] => 0,
        [first, ..rest] => first,
      }
    };
    test([1, 2, 3])
    }"#,
    )
    .assert_no_errors()
    .assert_type_int();
}

#[test]
fn match_slice_pattern_rest_binding_type() {
    infer(
        r#"{
    let test = |items: Slice<int>| -> Slice<int> {
      match items {
        [] => items,
        [first, ..rest] => rest,
      }
    };
    test([1, 2, 3])
    }"#,
    )
    .assert_no_errors()
    .assert_type_slice_of_ints();
}

#[test]
fn match_slice_pattern_fixed_length() {
    infer(
        r#"{
    let test = |items: Slice<int>| -> int {
      match items {
        [a, b, c] => a + b + c,
        _ => 0,
      }
    };
    test([1, 2, 3])
    }"#,
    )
    .assert_no_errors()
    .assert_type_int();
}

#[test]
fn match_slice_pattern_nested() {
    infer(
        r#"{
    let test = |items: Slice<Slice<int>>| -> int {
      match items {
        [] => 0,
        [[first, ..], ..] => first,
        [[], ..] => 0,
      }
    };
    test([[1, 2], [3, 4]])
    }"#,
    )
    .assert_no_errors()
    .assert_type_int();
}

#[test]
fn match_struct_pattern_partial_with_rest() {
    infer(
        r#"{
    struct Point { x: int, y: int }
    let p = Point { x: 10, y: 20 };
    match p {
      Point { x, .. } => x,
    }
    }"#,
    )
    .assert_no_errors()
    .assert_type_int();
}

#[test]
fn match_struct_pattern_partial_ignores_remaining_fields() {
    infer(
        r#"{
    struct Rectangle { x: int, y: int, width: int, height: int }
    let r = Rectangle { x: 0, y: 0, width: 100, height: 50 };
    match r {
      Rectangle { width, height, .. } => width * height,
    }
    }"#,
    )
    .assert_no_errors()
    .assert_type_int();
}

#[test]
fn match_struct_pattern_partial_with_literal() {
    infer(
        r#"{
    struct Point { x: int, y: int }
    let p = Point { x: 0, y: 42 };
    match p {
      Point { x: 0, .. } => true,
      Point { .. } => false,
    }
    }"#,
    )
    .assert_no_errors()
    .assert_type_bool();
}

#[test]
fn let_struct_destructure_basic() {
    infer(
        r#"{
    struct Point { x: int, y: int }
    let p = Point { x: 3, y: 4 };
    let Point { x, y } = p;
    x + y
    }"#,
    )
    .assert_no_errors()
    .assert_type_int();
}

#[test]
fn let_struct_destructure_with_rename() {
    infer(
        r#"{
    struct Point { x: int, y: int }
    let p = Point { x: 10, y: 20 };
    let Point { x: a, y: b } = p;
    a * b
    }"#,
    )
    .assert_no_errors()
    .assert_type_int();
}

#[test]
fn let_struct_destructure_with_rest() {
    infer(
        r#"{
    struct Rectangle { x: int, y: int, width: int, height: int }
    let r = Rectangle { x: 0, y: 0, width: 100, height: 50 };
    let Rectangle { width, height, .. } = r;
    width * height
    }"#,
    )
    .assert_no_errors()
    .assert_type_int();
}

#[test]
fn let_struct_destructure_nested() {
    infer(
        r#"{
    struct Point { x: int, y: int }
    struct Line { start: Point, end: Point }
    let line = Line {
      start: Point { x: 0, y: 0 },
      end: Point { x: 10, y: 10 }
    };
    let Line { start: Point { x: x1, y: y1 }, end: Point { x: x2, y: y2 } } = line;
    x1 + y1 + x2 + y2
    }"#,
    )
    .assert_no_errors()
    .assert_type_int();
}

#[test]
fn let_struct_destructure_from_function_return() {
    infer(
        r#"{
    struct Point { x: int, y: int }

    let get_point = || -> Point { Point { x: 5, y: 10 } };

    let Point { x, y } = get_point();
    x + y
    }"#,
    )
    .assert_no_errors()
    .assert_type_int();
}

#[test]
fn let_struct_destructure_missing_field_without_rest_is_error() {
    infer(
        r#"{
    struct Point { x: int, y: int }
    let p = Point { x: 1, y: 2 };
    let Point { x } = p;
    x
    }"#,
    )
    .assert_infer_code("pattern_missing_fields");
}

#[test]
fn match_guard_basic() {
    infer(
        r#"{
    match Some(42) {
      Some(x) if x > 0 => "positive",
      Some(_) => "non-positive",
      None => "none",
    }
    }"#,
    )
    .assert_type_string();
}

#[test]
fn match_guard_uses_binding() {
    infer(
        r#"{
    match Some(42) {
      Some(n) if n > 100 => n * 2,
      Some(n) => n,
      None => 0,
    }
    }"#,
    )
    .assert_type_int();
}

#[test]
fn match_guard_on_struct_pattern() {
    infer(
        r#"{
    struct Point { x: int, y: int }
    let test = |p: Point| -> string {
      match p {
        Point { x, y } if x == y => "diagonal",
        Point { x, y } if x > y => "above",
        _ => "below",
      }
    };
    test(Point { x: 1, y: 2 })
    }"#,
    )
    .assert_type_string();
}

#[test]
fn match_guard_non_bool_is_type_mismatch() {
    infer(
        r#"{
    match Some(42) {
      Some(x) if x => "matched",
      _ => "other",
    }
    }"#,
    )
    .assert_type_mismatch();
}

#[test]
fn match_guard_multiple_arms() {
    infer(
        r#"{
    match (1, 2) {
      (x, y) if x == y => "equal",
      (x, y) if x > y => "greater",
      (x, y) if x < y => "less",
      _ => "unreachable",
    }
    }"#,
    )
    .assert_type_string();
}

#[test]
fn recursive_struct_option_pattern_matching() {
    infer(
        r#"
    struct Node {
      value: int,
      next: Option<Ref<Node>>,
    }

    fn sum_list(list: Option<Ref<Node>>) -> int {
      match list {
        None => 0,
        Some(node) => node.value,
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn recursive_enum_pattern_matching() {
    infer(
        r#"
    enum Tree {
      Leaf(int),
      Branch(Tree, Tree),
    }

    fn sum_tree(tree: Tree) -> int {
      match tree {
        Tree.Leaf(n) => n,
        Tree.Branch(left, right) => sum_tree(left) + sum_tree(right),
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn for_loop_over_channel() {
    infer(
        r#"
    fn test() {
      let ch = Channel.new<int>();
      for v in ch {
        let x: int = v;
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn for_loop_over_receiver() {
    infer(
        r#"
    fn test(rx: Receiver<string>) {
      for msg in rx {
        let s: string = msg;
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn channel_split_returns_tuple() {
    infer(
        r#"
    fn test() {
      let ch = Channel.new<int>();
      let (tx, rx) = ch.split();
      tx.send(42);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn channel_does_not_coerce_to_sender() {
    infer(
        r#"
    fn producer(tx: Sender<int>) {
      tx.send(42);
    }

    fn test() {
      let ch = Channel.new<int>();
      producer(ch);
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn channel_does_not_coerce_to_receiver() {
    infer(
        r#"
    fn consumer(rx: Receiver<int>) -> Option<int> {
      rx.receive()
    }

    fn test() -> Option<int> {
      let ch = Channel.new<int>();
      consumer(ch)
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn sender_does_not_coerce_to_channel() {
    infer(
        r#"
    fn needs_channel(ch: Channel<int>) {}

    fn test(tx: Sender<int>) {
      needs_channel(tx);
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn receiver_does_not_coerce_to_channel() {
    infer(
        r#"
    fn needs_channel(ch: Channel<int>) {}

    fn test(rx: Receiver<int>) {
      needs_channel(rx);
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn sender_does_not_coerce_to_receiver() {
    infer(
        r#"
    fn consumer(rx: Receiver<int>) {}

    fn test(tx: Sender<int>) {
      consumer(tx);
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn receiver_does_not_coerce_to_sender() {
    infer(
        r#"
    fn producer(tx: Sender<int>) {}

    fn test(rx: Receiver<int>) {
      producer(rx);
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn channel_receive_returns_option() {
    infer(
        r#"
    fn test() {
      let ch = Channel.new<int>();
      let result: Option<int> = ch.receive();
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn receiver_receive_returns_option() {
    infer(
        r#"
    fn test(rx: Receiver<string>) {
      let result: Option<string> = rx.receive();
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn buffered_channel_type() {
    infer(
        r#"
    fn test() {
      let ch: Channel<int> = Channel.buffered(10);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn select_receive_rejects_bare_identifier() {
    infer(
        r#"
    fn test() {
      let ch = Channel.new<int>();
      select {
        let v = ch.receive() => v,
        _ => 0,
      }
    }
        "#,
    )
    .assert_infer_code("bare_identifier_in_select_receive");
}

#[test]
fn select_receive_rejects_none_pattern() {
    infer(
        r#"
    fn test() {
      let ch = Channel.new<int>();
      select {
        let None = ch.receive() => 0,
        _ => 1,
      }
    }
        "#,
    )
    .assert_infer_code("none_pattern_in_select_receive");
}

#[test]
fn select_receive_accepts_some_pattern() {
    infer(
        r#"
    fn test() {
      let ch = Channel.new<int>();
      select {
        let Some(v) = ch.receive() => v,
        _ => 0,
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn select_match_receive_pattern() {
    infer(
        r#"
    fn test() {
      let ch = Channel.new<int>();
      select {
        match ch.receive() {
          Some(v) => v,
          None => 0,
        },
        _ => 0,
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn task_channel_close_captured() {
    infer(
        r#"
    fn test() {
      let ch = Channel.new<int>();
      task {
        ch.send(42)
        ch.close()
      }
      for v in ch {
        let _ = v
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn task_channel_close_only() {
    infer(
        r#"
    fn test() {
      let ch = Channel.new<int>();
      task {
        ch.close()
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn if_without_else_in_if_let_body() {
    infer(
        r#"
    fn side_effect() {}

    fn test() {
      let a: Option<int> = Some(10);
      if let Some(x) = a {
        if x > 5 {
          side_effect();
        }
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn if_without_else_in_match_arm() {
    infer(
        r#"
    fn side_effect() {}

    fn test() {
      let x = 5;
      match x {
        5 => {
          if true {
            side_effect();
          }
        },
        _ => {},
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn if_without_else_nested() {
    infer(
        r#"
    fn side_effect() {}

    fn test() {
      if true {
        if true {
          side_effect();
        }
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn if_without_else_in_closure_body() {
    infer(
        r#"
    fn side_effect() {}

    fn test() {
      let f = || {
        if true {
          side_effect();
        }
      };
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn nested_if_without_else_is_unit() {
    infer(
        r#"
    fn side_effect() -> int { 42 }

    fn test() {
      if true {
        if true {
          side_effect();
        }
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn nested_if_let_without_else_is_unit() {
    infer(
        r#"
    fn side_effect() -> int { 42 }

    fn test() {
      let a: Option<Option<int>> = Some(Some(99))
      if let Some(inner) = a {
        if let Some(val) = inner {
          side_effect();
        }
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn if_without_else_always_unit() {
    infer("if true { 42 }").assert_type_unit();
}

#[test]
fn break_outside_loop_is_error() {
    infer(
        r#"
    fn test() {
      break;
    }
        "#,
    )
    .assert_infer_code("break_outside_loop");
}

#[test]
fn continue_outside_loop_is_error() {
    infer(
        r#"
    fn test() {
      continue;
    }
        "#,
    )
    .assert_infer_code("continue_outside_loop");
}

#[test]
fn break_with_value_outside_loop_is_error() {
    infer(
        r#"
    fn test() {
      break 42;
    }
        "#,
    )
    .assert_infer_code("break_outside_loop");
}

#[test]
fn break_in_loop_is_allowed() {
    infer(
        r#"
    fn test() {
      loop {
        break;
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn continue_in_loop_is_allowed() {
    infer(
        r#"
    fn test() {
      loop {
        continue;
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn break_in_for_loop_is_allowed() {
    infer(
        r#"
    fn test() {
      for i in [1, 2, 3] {
        break;
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn continue_in_while_loop_is_allowed() {
    infer(
        r#"
    fn test() {
      while true {
        continue;
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn lambda_as_value() {
    infer(
        r#"
    fn test() -> int {
      let f = |x: int| -> int { x + 1 };
      f(5)
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn if_branches_coerce_to_interface_return_type() {
    infer(
        r#"
    interface Printable {
      fn to_string(self) -> string
    }

    struct Box { label: string }
    impl Box {
      fn to_string(self) -> string { self.label }
    }

    struct Circle { radius: int }
    impl Circle {
      fn to_string(self) -> string { "circle" }
    }

    fn make_printable(kind: string) -> Printable {
      if kind == "box" {
        Box { label: "dynamic" }
      } else {
        Circle { radius: 5 }
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn infer_non_exhaustive_select_expression() {
    infer(
        r#"
    fn test() -> int {
      let ch = Channel.new<int>()
      select {
        let Some(v) = ch.receive() => v,
      }
    }
        "#,
    )
    .assert_infer_code("non_exhaustive_select_expression");
}

#[test]
fn infer_select_expression_with_match_receive_ok() {
    infer(
        r#"
    fn test() -> int {
      let ch = Channel.new<int>()
      select {
        match ch.receive() {
          Some(v) => v,
          None => 0,
        },
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn infer_select_expression_with_default_ok() {
    infer(
        r#"
    fn test() -> int {
      let ch = Channel.new<int>()
      select {
        let Some(v) = ch.receive() => v,
        _ => 0,
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn infer_select_statement_no_default_ok() {
    infer(
        r#"
    fn test() {
      let ch = Channel.new<int>()
      select {
        let Some(v) = ch.receive() => {
          let _ = v
        },
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn select_duplicate_default_arms() {
    infer(
        r#"
    fn test() {
      let ch = Channel.new<int>()
      select {
        let Some(v) = ch.receive() => {
          let _ = v
        },
        _ => { let _ = 1 },
        _ => { let _ = 2 },
      }
    }
        "#,
    )
    .assert_infer_code("duplicate_select_default");
}

#[test]
fn infer_empty_select_rejected() {
    infer(
        r#"
fn test() {
  select {}
}
        "#,
    )
    .assert_infer_code("empty_select");
}

#[test]
fn infer_select_bare_channel_rejected() {
    infer(
        r#"
fn test() {
  let ch = Channel.new<int>()
  select {
    ch => { let _ = 1 },
    _ => {},
  }
}
        "#,
    )
    .assert_infer_code("expected_channel_send");
}

#[test]
fn infer_select_ufcs_non_channel_rejected() {
    infer(
        r#"
fn send(ch: Channel<int>, v: int) -> Channel<int> { ch }

fn test() {
  let ch = Channel.buffered<int>(1)
  select {
    send(ch, 7) => { let _ = 1 },
    _ => {},
  }
}
        "#,
    )
    .assert_infer_code("expected_channel_send");
}

#[test]
fn infer_select_non_channel_send_method_rejected() {
    infer(
        r#"
struct Foo {}

impl Foo {
  fn send(self) -> Channel<int> { Channel.new<int>() }
}

fn test() {
  let f = Foo {}
  select {
    f.send() => { let _ = 1 },
    _ => {},
  }
}
        "#,
    )
    .assert_infer_code("expected_channel_send");
}

#[test]
fn panic_in_assignment_expression() {
    infer(r#"{ let x: int = panic("boom") }"#).assert_infer_code("panic_in_expression_position");
}
