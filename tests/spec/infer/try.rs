use crate::spec::infer::*;

#[test]
fn propagate_result_basic() {
    infer(
        r#"
    fn get_value() -> Result<int, string> {
      let result: Result<int, string> = Result.Ok(42);
      let value = result?;
      Result.Ok(value)
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn propagate_result_propagates_error() {
    infer(
        r#"
    fn divide(x: int, y: int) -> Result<int, string> {
      let checked: Result<int, string> = Result.Ok(x);
      let value = checked?;
      Result.Ok(value / y)
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn propagate_result_same_error_type() {
    infer(
        r#"
    fn process() -> Result<string, string> {
      let r1: Result<int, string> = Result.Ok(42);
      let v1 = r1?;
      let r2: Result<bool, string> = Result.Ok(true);
      let v2 = r2?;
      Result.Ok("done")
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn propagate_result_unwraps_to_correct_type() {
    infer(
        r#"
    fn get_string() -> Result<string, int> {
      let result: Result<string, int> = Result.Ok("hello");
      let s = result?;
      s
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn propagate_result_in_expression() {
    infer(
        r#"
    fn add_results() -> Result<int, string> {
      let r1: Result<int, string> = Result.Ok(10);
      let r2: Result<int, string> = Result.Ok(20);
      Result.Ok(r1? + r2?)
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn propagate_option_basic() {
    infer(
        r#"
    fn get_value() -> Option<int> {
      let opt: Option<int> = Option.Some(42);
      let value = opt?;
      Option.Some(value)
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn propagate_option_propagates_none() {
    infer(
        r#"
    fn process(x: Option<int>) -> Option<string> {
      let value = x?;
      Option.Some("found")
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn propagate_option_unwraps_to_correct_type() {
    infer(
        r#"
    fn get_number() -> Option<int> {
      let opt: Option<string> = Option.Some("hello");
      let s = opt?;
      s
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn propagate_option_in_expression() {
    infer(
        r#"
    fn add_options() -> Option<int> {
      let o1: Option<int> = Option.Some(10);
      let o2: Option<int> = Option.Some(20);
      Option.Some(o1? + o2?)
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn propagate_option_chained() {
    infer(
        r#"
    fn get_first(list: Slice<int>) -> Option<int> {
      let opt: Option<int> = Option.Some(42);
      let value = opt?;
      Option.Some(value * 2)
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn propagate_on_non_result_or_option() {
    infer(
        r#"
    fn test() -> int {
      let x = 42;
      x?
    }
        "#,
    )
    .assert_infer_code("try_requires_result_or_option");
}

#[test]
fn propagate_result_type_mismatch_error() {
    infer(
        r#"
    fn process() -> Result<int, bool> {
      let r: Result<int, string> = Result.Ok(42);
      r?
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn propagate_option_requires_option_return() {
    infer(
        r#"
    fn process() -> int {
      let opt: Option<int> = Option.Some(42);
      opt?
    }
        "#,
    )
    .assert_infer_code("try_return_type_mismatch");
}

#[test]
fn propagate_result_requires_result_return() {
    infer(
        r#"
    fn process() -> int {
      let r: Result<int, string> = Result.Ok(42);
      r?
    }
        "#,
    )
    .assert_infer_code("try_return_type_mismatch");
}

#[test]
fn propagate_option_different_inner_types() {
    infer(
        r#"
    fn convert() -> Option<string> {
      let opt_int: Option<int> = Option.Some(42);
      let num = opt_int?;
      let opt_bool: Option<bool> = Option.Some(true);
      let flag = opt_bool?;
      Option.Some("result")
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn propagate_nested_results() {
    infer(
        r#"
    fn nested() -> Result<int, string> {
      let r1: Result<Result<int, string>, string> = Result.Ok(Result.Ok(42));
      let r2 = r1?;
      r2
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn try_block_result_type() {
    infer(
        r#"{
    let risky = || -> Result<int, string> { Result.Ok(42) };
    let result = try {
      risky()?
    };
    result
    }"#,
    )
    .assert_type_struct_generic("Result", vec![int_type(), string_type()]);
}

#[test]
fn try_block_option_type() {
    infer(
        r#"{
    let maybe = || -> Option<int> { Option.Some(42) };
    let opt = try {
      maybe()?
    };
    opt
    }"#,
    )
    .assert_type_struct_generic("Option", vec![int_type()]);
}

#[test]
fn try_block_multiple_question_marks() {
    infer(
        r#"{
    let get_a = || -> Result<int, string> { Result.Ok(10) };
    let get_b = || -> Result<int, string> { Result.Ok(20) };
    let result = try {
      let a = get_a()?;
      let b = get_b()?;
      a + b
    };
    result
    }"#,
    )
    .assert_type_struct_generic("Result", vec![int_type(), string_type()]);
}

#[test]
fn try_block_with_semicolon_returns_last_expression_type() {
    infer(
        r#"{
    let risky = || -> Result<int, string> { Result.Ok(42) };
    let result = try {
      risky()?
    };
    result
    }"#,
    )
    .assert_type_struct_generic("Result", vec![int_type(), string_type()]);
}

#[test]
fn try_block_nested() {
    infer(
        r#"{
    let inner_risky = || -> Result<int, string> { Result.Ok(1) };
    let outer_risky = || -> Result<int, string> { Result.Ok(2) };
    let outer = try {
      let inner = try {
        inner_risky()?
      };
      let inner_val = match inner {
        Ok(x) => x,
        Err(_) => 0,
      };
      inner_val + outer_risky()?
    };
    outer
    }"#,
    )
    .assert_type_struct_generic("Result", vec![int_type(), string_type()]);
}

#[test]
fn try_block_with_loop_inside() {
    infer(
        r#"{
    let get = || -> Option<int> { Option.Some(42) };
    let result = try {
      let mut sum = 0;
      for i in 0..3 {
        if i > 1 {
          break;
        }
        sum = sum + i;
      };
      let x = get()?;
      sum + x
    };
    result
    }"#,
    )
    .assert_type_struct_generic("Option", vec![int_type()]);
}

#[test]
fn try_block_in_if_condition() {
    infer(
        r#"{
    let maybe = || -> Option<int> { Option.Some(42) };
    let opt = try {
      if true {
        maybe()?
      } else {
        0
      }
    };
    opt
    }"#,
    )
    .assert_type_struct_generic("Option", vec![int_type()]);
}

#[test]
fn try_block_question_mark_in_nested_if() {
    infer(
        r#"{
    let risky = || -> Result<int, string> { Result.Ok(42) };
    let result = try {
      if true {
        if false {
          risky()?
        } else {
          0
        }
      } else {
        1
      }
    };
    result
    }"#,
    )
    .assert_type_struct_generic("Result", vec![int_type(), string_type()]);
}

#[test]
fn try_block_with_matching_annotation() {
    infer(
        r#"{
    let risky = || -> Result<int, string> { Result.Ok(42) };
    let result: Result<int, string> = try {
      risky()?
    };
    result
    }"#,
    )
    .assert_type_struct_generic("Result", vec![int_type(), string_type()]);
}

#[test]
fn try_block_option_with_matching_annotation() {
    infer(
        r#"{
    let maybe = || -> Option<int> { Option.Some(42) };
    let opt: Option<int> = try {
      maybe()?
    };
    opt
    }"#,
    )
    .assert_type_struct_generic("Option", vec![int_type()]);
}

#[test]
fn try_block_with_lambda_containing_return() {
    infer(
        r#"{
    let risky = || -> Result<int, string> { Result.Ok(42) };
    let result = try {
      let f = |x: int| -> int { return x + 1 };
      f(risky()?)
    };
    result
    }"#,
    )
    .assert_type_struct_generic("Result", vec![int_type(), string_type()]);
}

#[test]
fn try_block_with_nested_function_containing_return() {
    infer(
        r#"{
    let risky = || -> Result<int, string> { Result.Ok(42) };
    let result = try {
      let helper = |x: int| -> int { x + 1 };
      helper(risky()?)
    };
    result
    }"#,
    )
    .assert_type_struct_generic("Result", vec![int_type(), string_type()]);
}

#[test]
fn try_block_question_mark_in_try_block_not_in_lambda() {
    infer(
        r#"{
    let risky = || -> Result<int, string> { Result.Ok(42) };
    let result = try {
      let x = risky()?;
      x + 1
    };
    result
    }"#,
    )
    .assert_type_struct_generic("Result", vec![int_type(), string_type()]);
}

#[test]
fn try_block_question_mark_inside_lambda_propagates_to_lambda() {
    infer(
        r#"{
    let risky = || -> Result<int, string> { Result.Ok(42) };
    let result = try {
      let f = || -> Result<int, string> {
        let x = risky()?;
        Result.Ok(x + 1)
      };
      f()?
    };
    result
    }"#,
    )
    .assert_type_struct_generic("Result", vec![int_type(), string_type()]);
}

#[test]
fn failure_propagation_in_call_arg() {
    infer(
        r#"
    fn f(x: int) -> int { x }
    fn test() -> Result<int, string> {
      Ok(f(Err("e")?))
    }
        "#,
    )
    .assert_infer_code("failure_propagation_in_expression");
}

#[test]
fn failure_propagation_in_binary() {
    infer(
        r#"
    fn test() -> Result<int, string> {
      Ok(1 + Err("e")?)
    }
        "#,
    )
    .assert_infer_code("failure_propagation_in_expression");
}

#[test]
fn failure_propagation_none_in_unary() {
    infer(
        r#"
    fn test() -> Option<bool> {
      Some(!None?)
    }
        "#,
    )
    .assert_infer_code("failure_propagation_in_expression");
}

#[test]
fn failure_propagation_allowed_in_statement() {
    infer(
        r#"
    fn test() -> Result<int, string> {
      Err("e")?;
      Ok(1)
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn failure_propagation_allowed_in_try_block_statement() {
    infer(
        r#"
    fn test() -> int {
      let result = try {
        Err("e")?;
        42
      };
      match result {
        Ok(v) => v,
        Err(_) => 0,
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn failure_propagation_allowed_as_block_tail() {
    infer(
        r#"
    fn test() -> Result<int, string> {
      Err("e")?
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn failure_propagation_allowed_in_if_branch() {
    infer(
        r#"
    fn test(cond: bool) -> Result<int, string> {
      if cond {
        Err("e")?
      };
      Ok(1)
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn panic_in_let_binding_rejected() {
    infer(
        r#"
    fn main() {
      let _ = panic("boom")
    }
        "#,
    )
    .assert_infer_code("never_call_in_expression");
}

#[test]
fn panic_in_if_condition_rejected() {
    infer(
        r#"
    fn main() {
      if panic("boom") {}
    }
        "#,
    )
    .assert_infer_code("never_call_in_expression");
}

#[test]
fn panic_in_match_arm_valid() {
    infer(
        r#"
    fn test(x: int) -> int {
      match x {
        1 => panic("unexpected"),
        _ => 42,
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn panic_in_if_branch_valid() {
    infer(
        r#"
    fn test(cond: bool) -> int {
      if cond {
        panic("boom")
      } else {
        42
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn panic_as_statement_valid() {
    infer(
        r#"
    fn main() {
      panic("boom")
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn return_in_let_binding_rejected() {
    infer(
        r#"
    fn f() -> int {
      let _ = return 1
      2
    }
        "#,
    )
    .assert_infer_code("control_flow_in_expression");
}

#[test]
fn break_in_let_binding_rejected() {
    infer(
        r#"
    fn f() {
      loop {
        let _ = break
      }
    }
        "#,
    )
    .assert_infer_code("control_flow_in_expression");
}

#[test]
fn continue_in_match_subject_rejected() {
    infer(
        r#"
    fn f() {
      loop {
        match continue {
          _ => 1,
        }
        break
      }
    }
        "#,
    )
    .assert_infer_code("control_flow_in_expression");
}

#[test]
fn task_as_match_subject_rejected() {
    infer(
        r#"
    fn work() {}
    fn f() {
      match task work() {
        _ => 1,
      }
    }
        "#,
    )
    .assert_infer_code("control_flow_in_expression");
}

#[test]
fn break_in_match_arm_valid() {
    infer(
        r#"
    fn f() {
      loop {
        match 1 {
          1 => break,
          _ => continue,
        }
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn return_in_if_branch_valid() {
    infer(
        r#"
    fn f(x: int) -> int {
      if x > 0 {
        return x
      }
      0
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn task_defer_as_statement_valid() {
    infer(
        r#"
    fn work() {}
    fn f() {
      task work()
      defer work()
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn return_break_rejected() {
    infer(
        r#"
    fn f() -> int {
      loop {
        return break
      }
    }
        "#,
    )
    .assert_infer_code("control_flow_in_expression");
}

#[test]
fn return_continue_rejected() {
    infer(
        r#"
    fn f() -> int {
      loop {
        return continue
      }
    }
        "#,
    )
    .assert_infer_code("control_flow_in_expression");
}

#[test]
fn return_return_rejected() {
    infer(
        r#"
    fn f() -> int {
      return return 1
    }
        "#,
    )
    .assert_infer_code("control_flow_in_expression");
}

#[test]
fn return_paren_return_rejected() {
    infer(
        r#"
    fn f() -> int {
      return (return 1)
    }
        "#,
    )
    .assert_infer_code("control_flow_in_expression");
}

#[test]
fn paren_continue_as_statement_rejected() {
    infer(
        r#"
    fn main() {
      loop {
        (continue)
        break
      }
    }
        "#,
    )
    .assert_infer_code("control_flow_in_expression");
}

#[test]
fn paren_break_as_statement_rejected() {
    infer(
        r#"
    fn main() {
      let _ = loop {
        (break 1)
        2
      }
    }
        "#,
    )
    .assert_infer_code("control_flow_in_expression");
}

#[test]
fn paren_return_as_statement_rejected() {
    infer(
        r#"
    fn f() -> int {
      (return 1)
      2
    }
        "#,
    )
    .assert_infer_code("control_flow_in_expression");
}

#[test]
fn paren_return_in_match_arm_rejected() {
    infer(
        r#"
    fn f() -> int {
      let _ = match 1 {
        1 => (return 1),
        _ => 2,
      }
      3
    }
        "#,
    )
    .assert_infer_code("control_flow_in_expression");
}

#[test]
fn paren_continue_in_if_body_rejected() {
    infer(
        r#"
    fn main() {
      loop {
        let _ = if true {
          (continue)
        } else {
          1
        }
      }
    }
        "#,
    )
    .assert_infer_code("control_flow_in_expression");
}

#[test]
fn paren_break_in_match_arm_rejected() {
    infer(
        r#"
    fn main() {
      let _ = loop {
        let _ = match 1 {
          1 => (break 1),
          _ => 2,
        }
        3
      }
    }
        "#,
    )
    .assert_infer_code("control_flow_in_expression");
}
