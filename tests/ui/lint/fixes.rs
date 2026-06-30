use crate::_harness::lint::apply_lint_fixes;
use crate::{assert_fix_snapshot, assert_no_fix};

#[test]
fn fix_bool_literal_comparison_eq_true() {
    assert_fix_snapshot!(
        r#"
fn main() {
  let x = true;
  let _ = x == true
}
"#
    );
}

#[test]
fn fix_bool_literal_comparison_ne_true() {
    assert_fix_snapshot!(
        r#"
fn main() {
  let x = true;
  let _ = x != true
}
"#
    );
}

#[test]
fn fix_redundant_operation_identity() {
    assert_fix_snapshot!(
        r#"
fn main() {
  let x = 5
  let _ = 0 + x
}
"#
    );
}

#[test]
fn fix_redundant_operation_constant() {
    assert_fix_snapshot!(
        r#"
fn main() {
  let x = 5
  let _ = x * 0
}
"#
    );
}

#[test]
fn fix_redundant_operation_keeps_operand_parens() {
    assert_fix_snapshot!(
        r#"
fn main() {
  let a = 1
  let b = 2
  let c = 3
  let _ = (a + b) * 1 * c
}
"#
    );
}

#[test]
fn fix_double_bool_negation() {
    assert_fix_snapshot!(
        r#"
fn main() {
  let x = true;
  let _ = !!x
}
"#
    );
}

#[test]
fn fix_double_bool_negation_with_parens() {
    assert_fix_snapshot!(
        r#"
fn main() {
  let x = true;
  let _ = !(!x)
}
"#
    );
}

#[test]
fn fix_double_int_negation() {
    assert_fix_snapshot!(
        r#"
fn main() {
  let x = 5;
  let _ = --x
}
"#
    );
}

#[test]
fn fix_negated_equality_equal() {
    assert_fix_snapshot!(
        r#"
fn main() {
  let a = true;
  let b = false;
  let _ = !(a == b)
}
"#
    );
}

#[test]
fn fix_negated_equality_not_equal() {
    assert_fix_snapshot!(
        r#"
fn main() {
  let a = true;
  let b = false;
  let _ = !(a != b)
}
"#
    );
}

#[test]
fn fix_redundant_sprintf() {
    assert_fix_snapshot!(
        r#"
import "go:fmt"

fn main() {
  let s = "hello"
  let _ = fmt.Sprintf("%s", s)
}
"#
    );
}

#[test]
fn fix_map_identity_option() {
    assert_fix_snapshot!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = o.map(|x| x)
}
"#
    );
}

#[test]
fn fix_manual_compound_assignment() {
    assert_fix_snapshot!(
        r#"
fn main() {
  let mut x = 5;
  x = x + 1;
  let _ = x
}
"#
    );
}

#[test]
fn fix_redundant_field_names() {
    assert_fix_snapshot!(
        r#"
struct Point { x: int, y: int }

fn read(p: Point) -> int { p.x + p.y }

fn make(x: int) -> Point {
  Point { x: x, y: 0 }
}
"#
    );
}

#[test]
fn fix_redundant_field_names_multiple() {
    assert_fix_snapshot!(
        r#"
struct Point { x: int, y: int }

fn read(p: Point) -> int { p.x + p.y }

fn make(x: int, y: int) -> Point {
  Point { x: x, y: y }
}
"#
    );
}

#[test]
fn fix_uninterpolated_fstring() {
    assert_fix_snapshot!(
        r#"
fn main() {
  let msg = f"hello world";
  let _ = msg
}
"#
    );
}

#[test]
fn fix_uninterpolated_fstring_collapses_brace_escapes() {
    assert_fix_snapshot!(
        r#"
fn main() {
  let msg = f"a {{ b }}";
  let _ = msg
}
"#
    );
}

#[test]
fn self_comparison_has_no_fix() {
    assert_no_fix!(
        r#"
fn main() {
  let x = 5;
  let _ = x == x
}
"#
    );
}

#[test]
fn fix_is_idempotent() {
    let source = r#"
fn main() {
  let x = true;
  let _ = x == true
}
"#;
    let once = apply_lint_fixes(source);
    let twice = apply_lint_fixes(&once);
    assert_eq!(once, twice);
}
