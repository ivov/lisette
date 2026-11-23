use crate::assert_emit_snapshot;

#[test]
fn recover_block_with_panic_never_type() {
    let input = r#"
fn panicky() -> int {
  let arr: Slice<int> = [];
  arr[0]
}

fn test() {
  let result = recover { panicky() };
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn recover_block_with_direct_panic() {
    let input = r#"
fn test() {
  let result = recover { panic("oh no!") };
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn recover_block_basic() {
    let input = r#"
fn may_panic() -> int { 42 }

fn test() {
  let result = recover { may_panic() };
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn recover_block_with_statements() {
    let input = r#"
fn test() {
  let result = recover {
    let x = 1;
    let y = 2;
    x + y
  };
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn recover_block_string_result() {
    let input = r#"
fn test() {
  let result = recover { "hello" };
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn recover_block_in_function_returning_result() {
    let input = r#"
fn may_panic() -> int { 42 }

fn test() -> Result<int, PanicValue> {
  recover { may_panic() }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn recover_block_nested() {
    let input = r#"
fn test() {
  let outer = recover {
    let inner = recover { 1 };
    2
  };
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn recover_block_void_last_expression() {
    let input = r#"
fn risky() {
  panic("oops");
}

fn test() {
  let result = recover { risky() };
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn recover_block_with_match() {
    let input = r#"
fn may_panic() -> int { 42 }

fn test() -> int {
  let result = recover { may_panic() };
  match result {
    Ok(x) => x,
    Err(pv) => 0,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn recover_block_never_returning_user_function() {
    let input = r#"
fn fail(msg: string) -> Never { panic(msg) }

fn test() -> string {
  let result = recover { fail("intentional") };
  match result {
    Ok(_) => "ok",
    Err(_) => "caught",
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn recover_block_unit_ok_binding() {
    let input = r#"
fn test() {
  let result = recover { panic("boom") };
  match result {
    Ok(v) => v,
    Err(_) => {},
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn recover_block_panic_pattern_match() {
    let input = r#"
fn test() -> string {
  let result = recover { panic("oh no!") };
  match result {
    Ok(_) => "ok",
    Err(pv) => "caught panic",
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn recover_block_result_temp_var_no_collision() {
    let input = r#"
fn main() {
  let result = recover { 1 };
  let recoverResult_1 = 7;
  let _ = recoverResult_1;
  let _ = result;
}
"#;
    assert_emit_snapshot!(input);
}
