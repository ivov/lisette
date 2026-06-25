use crate::assert_emit_snapshot;

#[test]
fn partial_ok_construction() {
    let input = r#"
fn test() -> Partial<int, string> {
  Partial.Ok(42)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn partial_err_construction() {
    let input = r#"
fn test() -> Partial<int, string> {
  Partial.Err("fail")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn partial_both_construction() {
    let input = r#"
fn test() -> Partial<int, string> {
  Partial.Both(42, "eof")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn partial_match_all_variants() {
    let input = r#"
fn test(p: Partial<int, string>) -> int {
  match p {
    Partial.Ok(n) => n,
    Partial.Err(_) => 0,
    Partial.Both(n, _) => n,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn partial_match_both_field_access() {
    let input = r#"
fn test(p: Partial<int, string>) -> string {
  match p {
    Partial.Ok(_) => "ok",
    Partial.Err(e) => e,
    Partial.Both(_, e) => e,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn partial_unwrap_or() {
    let input = r#"
fn test(p: Partial<int, string>) -> int {
  p.unwrap_or(0)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn partial_map() {
    let input = r#"
fn test(p: Partial<int, string>) -> Partial<int, string> {
  p.map(|n| n * 2)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn partial_is_ok() {
    let input = r#"
fn test(p: Partial<int, string>) -> bool {
  p.is_ok()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn partial_is_both() {
    let input = r#"
fn test(p: Partial<int, string>) -> bool {
  p.is_both()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn fused_partial_match_on_go_write_call() {
    let input = r#"
import "go:os"
import "go:fmt"

fn write_all(file: Ref<os.File>) {
  match file.Write(['h', 'i']) {
    Ok(n) => fmt.Println(n),
    Both(n, _) => fmt.Println(n),
    Err(e) => {
      fmt.Println(e)
      return
    },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn fused_partial_match_on_lowered_call() {
    let input = r#"
import "go:errors"

fn attempt(ok: bool) -> Partial<int, error> {
  if ok { Partial.Ok(1) } else { Partial.Err(errors.New("nope")) }
}

fn test() -> int {
  match attempt(true) {
    Partial.Ok(n) => n,
    Partial.Both(n, _) => n,
    Partial.Err(_) => 0,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn lisette_function_returning_partial_tuple_uses_packed_abi() {
    let input = r#"
fn pair<A, B>(a: A, b: B) -> Partial<(A, B), error> {
  Partial.Ok((a, b))
}

fn test() {
  match pair<int, string>(1, "x") {
    Partial.Ok((first, second)) => {
      let _ = first
      let _ = second
    },
    Partial.Err(e) => {
      let _ = e
    },
    Partial.Both((first, second), e) => {
      let _ = first
      let _ = second
      let _ = e
    },
  }
}
"#;
    assert_emit_snapshot!(input);
}
