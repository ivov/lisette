use crate::spec::infer::*;

#[test]
fn recover_block_basic() {
    infer(
        r#"{
    let may_panic = || -> int { 42 };
    let result = recover { may_panic() };
    result
    }"#,
    )
    .assert_type_struct_generic("Result", vec![int_type(), con_type("PanicValue", vec![])]);
}

#[test]
fn recover_block_infers_inner_type_string() {
    infer(
        r#"{
    let result = recover { "hello" };
    result
    }"#,
    )
    .assert_type_struct_generic(
        "Result",
        vec![string_type(), con_type("PanicValue", vec![])],
    );
}

#[test]
fn recover_block_with_multiple_statements() {
    infer(
        r#"{
    let result = recover {
      let x = 1;
      let y = 2;
      x + y
    };
    result
    }"#,
    )
    .assert_type_struct_generic("Result", vec![int_type(), con_type("PanicValue", vec![])]);
}

#[test]
fn recover_block_unit_result() {
    infer(
        r#"{
    let side_effect = || {};
    let result = recover {
      side_effect();
      ()
    };
    result
    }"#,
    )
    .assert_type_struct_generic("Result", vec![unit_type(), con_type("PanicValue", vec![])]);
}

#[test]
fn recover_cannot_use_question_mark() {
    infer(
        r#"{
    fn fallible() -> Result<int, string> { Result.Ok(42) }
    recover { fallible()? }
    }"#,
    )
    .assert_infer_code("recover_cannot_use_question_mark");
}

#[test]
fn recover_cannot_return() {
    infer(
        r#"{
    fn foo() -> int {
      recover {
        return 42
      };
      0
    }
    foo()
    }"#,
    )
    .assert_infer_code("recover_block_return");
}

#[test]
fn recover_cannot_break_outside() {
    infer(
        r#"{
    for i in [1, 2, 3] {
      recover { break };
    }
    }"#,
    )
    .assert_infer_code("recover_block_break");
}

#[test]
fn recover_cannot_continue_outside() {
    infer(
        r#"{
    for i in [1, 2, 3] {
      recover { continue };
    }
    }"#,
    )
    .assert_infer_code("recover_block_continue");
}

#[test]
fn recover_break_inside_loop_is_ok() {
    infer(
        r#"{
    recover {
      for i in [1, 2, 3] {
        break
      }
      42
    }
    }"#,
    )
    .assert_no_errors();
}

#[test]
fn recover_continue_inside_loop_is_ok() {
    infer(
        r#"{
    recover {
      for i in [1, 2, 3] {
        continue
      }
      42
    }
    }"#,
    )
    .assert_no_errors();
}

#[test]
fn recover_nested_in_try() {
    infer(
        r#"{
    let may_panic = || -> int { 42 };
    let result = try {
      let inner = recover { may_panic() };
      inner?
    };
    result
    }"#,
    )
    .assert_type_struct_generic("Result", vec![int_type(), con_type("PanicValue", vec![])]);
}

#[test]
fn try_nested_in_recover() {
    infer(
        r#"{
    let fallible = || -> Result<int, string> { Result.Ok(42) };
    let result = recover {
      try { fallible()? }
    };
    result
    }"#,
    )
    .assert_type_struct_generic(
        "Result",
        vec![
            con_type("Result", vec![int_type(), string_type()]),
            con_type("PanicValue", vec![]),
        ],
    );
}

#[test]
fn recover_question_mark_in_lambda_propagates_to_lambda() {
    infer(
        r#"{
    let fallible = || -> Result<int, string> { Result.Ok(42) };
    recover {
      let f = || -> Result<int, string> {
        let x = fallible()?;
        Result.Ok(x)
      };
      f()
    }
    }"#,
    )
    .assert_no_errors();
}

#[test]
fn recover_return_in_nested_function_is_ok() {
    infer(
        r#"{
    recover {
      let inner = || -> int { return 42 };
      inner()
    }
    }"#,
    )
    .assert_no_errors();
}

#[test]
fn recover_block_empty_warning() {
    infer(
        r#"{
    recover {}
    }"#,
    )
    .assert_infer_code("recover_block_empty");
}
