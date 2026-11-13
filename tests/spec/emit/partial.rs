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
fn fused_selective_partial_both_omits_unused_named_value() {
    let input = r#"
import "go:os"

fn write(file: Ref<os.File>) -> Option<error> {
  if let Partial.Both(written, err) = file.Write(['h', 'i']) {
    return Some(err)
  }
  None
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn impossible_selective_partial_err_keeps_call() {
    let input = r#"
import "go:os"

fn write(file: Ref<os.File>) {
  if let Partial.Err(err) = file.Write(['h', 'i']) {
    panic(err.Error())
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn selective_partial_with_empty_arms_keeps_call() {
    let input = r#"
import "go:os"

fn write(file: Ref<os.File>) {
  if let Partial.Both(_, _) = file.Write(['h', 'i']) {}
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn selective_partial_literal_payload_falls_back_to_tagged_match() {
    let input = r#"
import "go:os"

fn write(file: Ref<os.File>) {
  match file.Write(['h', 'i']) {
    Partial.Ok(0) => panic("wrote nothing"),
    _ => {},
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn selective_partial_ok_wildcard_does_not_bind_nilable_value() {
    let input = r#"
import "go:fmt"
import "go:os"

fn main() {
  if let Partial.Ok(_) = os.ReadDir(".") {
    fmt.Println("ok")
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn selective_partial_ok_unused_name_does_not_bind_nilable_value() {
    let input = r#"
import "go:fmt"
import "go:os"

fn main() {
  if let Partial.Ok(entries) = os.ReadDir(".") {
    fmt.Println("ok")
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

#[test]
fn partial_tail_call_in_lowered_return_is_evaluated_once() {
    let input = r#"
fn ident<T>(x: T) -> T {
  x
}

pub fn tail(b: bool, p: Partial<int, error>) -> Partial<int, error> {
  if b {
    ident(p)
  } else {
    p
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn partial_failure_or_pattern_binds_value_into_let_name() {
    let input = r#"
import "go:io"
import "go:strings"

fn read(s: string) -> Result<string, error> {
  let body = match io.ReadAll(strings.NewReader(s)) {
    Partial.Ok(b) => b,
    Partial.Both(_, e) | Partial.Err(e) => return Err(e),
  }
  Ok(body as string)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn partial_failure_arm_first_with_alternatives_reversed() {
    let input = r#"
import "go:io"
import "go:strings"

fn read(s: string) -> Result<string, error> {
  let body = match io.ReadAll(strings.NewReader(s)) {
    Partial.Err(e) | Partial.Both(_, e) => return Err(e),
    Partial.Ok(b) => b,
  }
  Ok(body as string)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn partial_ok_and_wildcard_binds_value_into_let_name() {
    let input = r#"
import "go:io"
import "go:strings"

fn sizes(items: Slice<string>) -> int {
  let mut total = 0
  for s in items {
    let body = match io.ReadAll(strings.NewReader(s)) {
      Partial.Ok(b) => b,
      _ => continue,
    }
    total += body.length()
  }
  total
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn partial_failure_or_pattern_in_statement_match() {
    let input = r#"
import "go:fmt"
import "go:io"
import "go:strings"

fn print_size(s: string) {
  match io.ReadAll(strings.NewReader(s)) {
    Partial.Ok(b) => fmt.Println(b.length()),
    Partial.Both(_, e) | Partial.Err(e) => fmt.Println("error:", e),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn partial_failure_or_pattern_with_unbound_ok_tests_error_only() {
    let input = r#"
import "go:fmt"
import "go:io"
import "go:strings"

fn status(s: string) -> int {
  match io.ReadAll(strings.NewReader(s)) {
    Partial.Ok(_) => 1,
    Partial.Err(e) | Partial.Both(_, e) => {
      fmt.Println(e)
      2
    },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn partial_failure_arm_after_diverging_ok_arm_keeps_its_error_in_scope() {
    let input = r#"
import "go:fmt"
import "go:io"
import "go:strings"

fn report(s: string) {
  match io.ReadAll(strings.NewReader(s)) {
    Partial.Ok(_) => return (),
    Partial.Both(_, e) | Partial.Err(e) => fmt.Println(e),
  }
  fmt.Println("done")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn partial_value_and_error_with_same_name_get_distinct_go_names() {
    let input = r#"
import "go:fmt"
import "go:io"
import "go:strings"

fn size(s: string) -> int {
  match io.ReadAll(strings.NewReader(s)) {
    Partial.Ok(e) => e.length(),
    Partial.Both(_, e) | Partial.Err(e) => {
      fmt.Println(e)
      0
    },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn partial_let_shadowing_a_name_read_by_the_call_gets_fresh_name() {
    let input = r#"
import "go:io"
import "go:strings"

fn read(s: string) -> Result<int, error> {
  let body = s
  let body = match io.ReadAll(strings.NewReader(body)) {
    Partial.Ok(b) => b,
    Partial.Both(_, e) | Partial.Err(e) => return Err(e),
  }
  Ok(body.length())
}
"#;
    assert_emit_snapshot!(input);
}
