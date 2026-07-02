use crate::_harness::lint::apply_lint_fixes;
use crate::{assert_fix_snapshot, assert_no_fix, assert_no_lint_warnings};

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

#[test]
fn fix_manual_is_empty() {
    assert_fix_snapshot!(
        r#"
fn main() {
  let xs = [1, 2, 3]
  let _ = xs.length() == 0
}
"#
    );
}

#[test]
fn fix_unnecessary_first_then_check() {
    assert_fix_snapshot!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4]
  let _ = xs.get(0).is_some()
}
"#
    );
}

#[test]
fn fix_redundant_slice_bounds() {
    assert_fix_snapshot!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4, 5]
  let a = 2
  let _ = xs[a..xs.length()]
}
"#
    );
}

#[test]
fn fix_needless_bool_assign() {
    assert_fix_snapshot!(
        r#"
pub fn f(c: bool) -> bool {
  let mut x = false
  if c {
    x = true
  } else {
    x = false
  }
  x
}
"#
    );
}

#[test]
fn fix_manual_bytes_equal() {
    assert_fix_snapshot!(
        r#"
import "go:bytes"

fn main() {
  let a = "hello" as Slice<byte>
  let b = "world" as Slice<byte>
  let _ = bytes.Compare(a, b) == 0
}
"#
    );
}

#[test]
fn fix_manual_equal_fold() {
    assert_fix_snapshot!(
        r#"
import "go:strings"

fn main() {
  let a = "Hello"
  let b = "hELLO"
  let _ = strings.ToLower(a) == strings.ToLower(b)
}
"#
    );
}

#[test]
fn fix_manual_replace_all() {
    assert_fix_snapshot!(
        r#"
import "go:strings"

fn main() {
  let s = "hello world"
  let _ = strings.Replace(s, "o", "0", -1)
}
"#
    );
}

#[test]
fn fix_manual_time_since() {
    assert_fix_snapshot!(
        r#"
import "go:time"

fn main() {
  let t = time.Now()
  let _ = time.Now().Sub(t)
}
"#
    );
}

#[test]
fn fix_manual_time_until() {
    assert_fix_snapshot!(
        r#"
import "go:time"

fn main() {
  let deadline = time.Now()
  let _ = deadline.Sub(time.Now())
}
"#
    );
}

#[test]
fn fix_unnecessary_raw_string() {
    assert_fix_snapshot!(
        r#"
fn main() {
  let msg = r"hello";
  let _ = msg
}
"#
    );
}

#[test]
fn fix_excess_parens_on_condition() {
    assert_fix_snapshot!(
        r#"
fn main() {
  let x = 5;
  if (x > 0) {
    let _ = x;
  }
}
"#
    );
}

#[test]
fn fix_wildcard_in_or_patterns() {
    assert_fix_snapshot!(
        r#"
pub fn test(n: int) -> int {
  match n {
    0 | _ => 1,
  }
}
"#
    );
}

#[test]
fn fix_needless_question_mark() {
    assert_fix_snapshot!(
        r#"
fn consume() -> Option<int> {
  let x: Option<int> = Some(1)
  Some(x?)
}
"#
    );
}

#[test]
fn fix_neg_multiply() {
    assert_fix_snapshot!(
        r#"
fn negate(x: int) -> int {
  x * -1
}
"#
    );
}

#[test]
fn fix_needless_splitn() {
    assert_fix_snapshot!(
        r#"
import "go:strings"

fn main() {
  let s = "a,b,c"
  let _ = strings.SplitN(s, ",", -1)
}
"#
    );
}

#[test]
fn fix_unnecessary_reference() {
    assert_fix_snapshot!(
        r#"
pub fn foo(x: Ref<int>) {
  let _ = &x;
}
"#
    );
}

#[test]
fn fix_unnecessary_return() {
    assert_fix_snapshot!(
        r#"
fn five() -> int {
  return 5
}
"#
    );
}

#[test]
fn fix_redundant_rebinding() {
    assert_fix_snapshot!(
        r#"
fn main() {
  let a = 5;
  let a = a;
  let _ = a
}
"#
    );
}

#[test]
fn fix_unnecessary_mut() {
    assert_fix_snapshot!(
        r#"
fn main() {
  let mut x = 5
  let _ = x
}
"#
    );
}

#[test]
fn fix_expression_only_fstring() {
    assert_fix_snapshot!(
        r#"
fn main() {
  let name = "world"
  let _ = f"{name}"
}
"#
    );
}

#[test]
fn fix_expression_only_fstring_parenthesizes_binary() {
    assert_fix_snapshot!(
        r#"
fn main() {
  let a = "x"
  let b = "y"
  let _ = f"{a + b}".length()
}
"#
    );
}

#[test]
fn fix_unused_import() {
    assert_fix_snapshot!(
        r#"
import "go:fmt"

fn main() {
  let _ = 5
}
"#
    );
}

#[test]
fn fix_unused_import_with_alias() {
    assert_fix_snapshot!(
        r#"
import f "go:fmt"

fn main() {
  let _ = 5
}
"#
    );
}

#[test]
fn fix_needless_update() {
    assert_fix_snapshot!(
        r#"
struct Config { debug: bool, port: int }

fn read(c: Config) -> int { if c.debug { c.port } else { 0 } }

fn rebuild(base: Config) -> Config {
  Config { debug: true, port: 80, ..base }
}
"#
    );
}

#[test]
fn fix_rest_only_slice_pattern_wildcard() {
    assert_fix_snapshot!(
        r#"
fn main() {
  let xs = [1, 2, 3]
  let [..] = xs
}
"#
    );
}

#[test]
fn fix_rest_only_slice_pattern_bind() {
    assert_fix_snapshot!(
        r#"
fn main() {
  let xs = [1, 2, 3]
  let [..rest] = xs
  let _ = rest
}
"#
    );
}

#[test]
fn fix_unnecessary_bool_positive() {
    assert_fix_snapshot!(
        r#"
pub fn f(c: bool) -> bool {
  if c { true } else { false }
}
"#
    );
}

#[test]
fn fix_unnecessary_bool_negated() {
    assert_fix_snapshot!(
        r#"
pub fn f(c: bool) -> bool {
  if c { false } else { true }
}
"#
    );
}

#[test]
fn fix_unnecessary_bool_negates_comparison() {
    assert_fix_snapshot!(
        r#"
pub fn f(a: int, b: int) -> bool {
  if a == b { false } else { true }
}
"#
    );
}

#[test]
fn fix_unnecessary_bool_parenthesizes_loose_condition() {
    assert_fix_snapshot!(
        r#"
pub fn f(a: int, b: int) -> bool {
  if a > b { true } else { false }
}
"#
    );
}

#[test]
fn unnecessary_bool_newtype_condition_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub struct Flag(bool)

pub fn f(flag: Flag) -> bool {
  if flag { true } else { false }
}
"#
    );
}

#[test]
fn unnecessary_bool_bool_newtype_result_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub struct Flag(bool)

pub fn f(c: bool) -> Flag {
  if c { true } else { false }
}
"#
    );
}

#[test]
fn redundant_pattern_matching_bool_newtype_result_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub struct Flag(bool)

pub fn f(o: Option<int>) -> Flag {
  match o { Some(_) => true, None => false }
}
"#
    );
}

#[test]
fn needless_match_interface_adaptation_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub interface Face { fn tag(self) -> int }
pub struct Impl {}
impl Impl { fn tag(self) -> int { 1 } }

pub fn f(o: Option<Impl>) -> Option<Face> {
  match o { Some(x) => Some(x), None => None }
}
"#
    );
}

#[test]
fn fix_redundant_pattern_matching_is_some() {
    assert_fix_snapshot!(
        r#"
pub fn f(o: Option<int>) -> bool {
  match o { Some(_) => true, None => false }
}
"#
    );
}

#[test]
fn fix_redundant_pattern_matching_is_err() {
    assert_fix_snapshot!(
        r#"
pub fn f(r: Result<int, string>) -> bool {
  match r { Ok(_) => false, Err(_) => true }
}
"#
    );
}

#[test]
fn fix_needless_match_option() {
    assert_fix_snapshot!(
        r#"
pub fn f(o: Option<int>) -> Option<int> {
  match o { Some(x) => Some(x), None => None }
}
"#
    );
}

#[test]
fn fix_needless_match_result() {
    assert_fix_snapshot!(
        r#"
pub fn f(r: Result<int, string>) -> Result<int, string> {
  match r { Ok(x) => Ok(x), Err(e) => Err(e) }
}
"#
    );
}

#[test]
fn fix_redundant_closure() {
    assert_fix_snapshot!(
        r#"
pub fn apply(xs: Slice<int>) -> Slice<int> {
  xs.map(|x| double(x))
}

fn double(n: int) -> int { n * 2 }
"#
    );
}

#[test]
fn redundant_closure_with_return_annotation_has_no_fix() {
    assert_no_fix!(
        r#"
pub fn apply(xs: Slice<int>) -> Slice<int> {
  xs.map(|x| -> int { double(x) })
}

fn double(n: int) -> int { n * 2 }
"#
    );
}

#[test]
fn fix_redundant_closure_call() {
    assert_fix_snapshot!(
        r#"
pub fn f() -> int {
  (|| compute())()
}

fn compute() -> int { 42 }
"#
    );
}

#[test]
fn fix_redundant_closure_call_single_item_block() {
    assert_fix_snapshot!(
        r#"
pub fn f() -> int {
  (|| { compute() })()
}

fn compute() -> int { 42 }
"#
    );
}

#[test]
fn redundant_closure_call_multi_statement_block_has_no_fix() {
    assert_no_fix!(
        r#"
pub fn f() -> int {
  (|| {
    let a = 1
    a + 1
  })()
}
"#
    );
}

#[test]
fn redundant_closure_call_with_return_annotation_has_no_fix() {
    assert_no_fix!(
        r#"
pub fn f() -> int {
  (|| -> int { compute() })()
}

fn compute() -> int { 42 }
"#
    );
}

#[test]
fn fix_double_comparison() {
    assert_fix_snapshot!(
        r#"
pub fn f(a: int, b: int) -> bool {
  a < b || a == b
}
"#
    );
}

#[test]
fn fix_unnecessary_min_or_max() {
    assert_fix_snapshot!(
        r#"
pub fn f(x: int) -> int {
  min(x, x)
}
"#
    );
}

#[test]
fn fix_redundant_fstring_conversion() {
    assert_fix_snapshot!(
        r#"
import "go:strconv"

pub fn f(x: int) -> string {
  f"n={strconv.Itoa(x)}"
}
"#
    );
}

#[test]
fn manual_contains_with_param_annotation_has_no_fix() {
    assert_no_fix!(
        r#"
pub fn f(xs: Slice<int>, v: int) -> bool {
  xs.any(|x: int| x == v)
}
"#
    );
}

#[test]
fn manual_option_zip_with_param_annotation_has_no_fix() {
    assert_no_fix!(
        r#"
pub fn f(a: Option<int>, b: Option<string>) -> Option<(int, string)> {
  a.and_then(|x: int| b.map(|y| (x, y)))
}
"#
    );
}

#[test]
fn redundant_closure_with_param_annotation_has_no_fix() {
    assert_no_fix!(
        r#"
pub fn f(o: Option<int>) -> Option<int> {
  o.and_then(|x: int| Some(x))
}
"#
    );
}

#[test]
fn bind_instead_of_map_with_param_annotation_has_no_fix() {
    assert_no_fix!(
        r#"
pub fn g(o: Option<int>) -> Option<int> {
  o.and_then(|x: int| Some(x + 1))
}
"#
    );
}

#[test]
fn fix_manual_contains() {
    assert_fix_snapshot!(
        r#"
pub fn f(xs: Slice<int>, v: int) -> bool {
  xs.any(|x| x == v)
}
"#
    );
}

#[test]
fn fix_manual_find() {
    assert_fix_snapshot!(
        r#"
pub fn f(xs: Slice<int>) -> Option<int> {
  xs.filter(|x| x > 0).get(0)
}
"#
    );
}

#[test]
fn manual_option_zip_result_adaptation_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub interface Face { fn tag(self) -> int }
pub struct Impl {}
impl Impl { fn tag(self) -> int { 1 } }

pub fn f(a: Option<Impl>, b: Option<Impl>) -> Option<(Face, Face)> {
  a.and_then(|x| b.map(|y| (x, y)))
}
"#
    );
}

#[test]
fn fix_manual_option_zip() {
    assert_fix_snapshot!(
        r#"
pub fn f(a: Option<int>, b: Option<string>) -> Option<(int, string)> {
  a.and_then(|x| b.map(|y| (x, y)))
}
"#
    );
}

#[test]
fn bind_instead_of_map_with_return_annotation_has_no_fix() {
    assert_no_fix!(
        r#"
pub fn f(o: Option<int>) -> Option<int> {
  o.and_then(|x| -> Option<int> { Some(x + 1) })
}
"#
    );
}

#[test]
fn fix_bind_instead_of_map() {
    assert_fix_snapshot!(
        r#"
pub fn f(o: Option<int>) -> Option<int> {
  o.and_then(|x| Some(x + 1))
}
"#
    );
}

#[test]
fn fix_unnecessary_map_on_constructor() {
    assert_fix_snapshot!(
        r#"
pub fn f(x: int) -> Option<int> {
  Some(x).map(triple)
}

fn triple(n: int) -> int { n * 3 }
"#
    );
}

#[test]
fn fix_unnecessary_map_on_constructor_non_nilable_field() {
    assert_fix_snapshot!(
        r#"
struct A { field: int }

pub fn f(a: A) -> Option<int> {
  Some(a.field).map(triple)
}

fn triple(n: int) -> int { n * 3 }
"#
    );
}

#[test]
fn unnecessary_map_on_constructor_lambda_mapper_has_no_fix() {
    assert_no_fix!(
        r#"
pub fn f(x: int) -> Option<int> {
  Some(x).map(|y| y + 1)
}
"#
    );
}

#[test]
fn fix_let_and_return() {
    assert_fix_snapshot!(
        r#"
pub fn f() -> int {
  let x = compute()
  x
}

fn compute() -> int { 42 }
"#
    );
}

#[test]
fn fix_or_fn_call() {
    assert_fix_snapshot!(
        r#"
pub fn f(o: Option<int>) -> int {
  o.unwrap_or(fallback())
}

fn fallback() -> int { 7 }
"#
    );
}

#[test]
fn fix_or_fn_call_result_callback_takes_error() {
    assert_fix_snapshot!(
        r#"
pub fn f(r: Result<int, string>) -> int {
  r.unwrap_or(fallback())
}

fn fallback() -> int { 7 }
"#
    );
}

#[test]
fn fix_unnecessary_lazy_evaluations() {
    assert_fix_snapshot!(
        r#"
pub fn f(o: Option<int>) -> int {
  o.unwrap_or_else(|| 5)
}
"#
    );
}

#[test]
fn fix_redundant_guards() {
    assert_fix_snapshot!(
        r#"
pub fn f(o: Option<int>) -> int {
  match o {
    Some(x) if x == 5 => 1,
    _ => 0,
  }
}
"#
    );
}

#[test]
fn fix_unnecessary_bool_float_ordering_negates_with_not() {
    assert_fix_snapshot!(
        r#"
pub fn f(a: float64, b: float64) -> bool {
  if a < b { false } else { true }
}
"#
    );
}

#[test]
fn fix_unnecessary_bool_parenthesizes_pipeline_condition() {
    assert_fix_snapshot!(
        r#"
pub fn f(x: int) -> bool {
  if x |> pred { false } else { true }
}

fn pred(n: int) -> bool { n > 0 }
"#
    );
}

#[test]
fn fix_redundant_pattern_matching_parenthesizes_pipeline_subject() {
    assert_fix_snapshot!(
        r#"
pub fn f(x: int) -> bool {
  match x |> maybe { Some(_) => true, None => false }
}

fn maybe(n: int) -> Option<int> { Some(n) }
"#
    );
}

#[test]
fn fix_expression_only_fstring_parenthesizes_pipeline() {
    assert_fix_snapshot!(
        r#"
pub fn f(x: int) -> string {
  f"{x |> label}"
}

fn label(n: int) -> string { "n" }
"#
    );
}

#[test]
fn fix_redundant_guards_bare_binding() {
    assert_fix_snapshot!(
        r#"
pub fn f(n: int) -> int {
  match n {
    x if x == 5 => 1,
    _ => 0,
  }
}
"#
    );
}

#[test]
fn fix_match_same_arms() {
    assert_fix_snapshot!(
        r#"
pub fn f(n: int) -> string {
  match n {
    1 => "a",
    2 => "b",
    3 => "a",
    _ => "z",
  }
}
"#
    );
}

#[test]
fn fix_equatable_if_let() {
    assert_fix_snapshot!(
        r#"
enum Sig { A, B }

pub fn f(s: Sig) -> int {
  if let Sig.A = s { 1 } else { 0 }
}
"#
    );
}

#[test]
fn fix_collapsible_else_if() {
    assert_fix_snapshot!(
        r#"
pub fn f(a: int) -> int {
  if a > 0 { 1 } else { if a < 0 { 2 } else { 3 } }
}
"#
    );
}

#[test]
fn fix_collapsible_if() {
    assert_fix_snapshot!(
        r#"
pub fn f(a: bool, b: bool) -> int {
  if a { if b { 1 } }
  0
}
"#
    );
}

#[test]
fn fix_collapsible_if_parenthesizes_or_operand() {
    assert_fix_snapshot!(
        r#"
pub fn f(a: bool, b: bool, c: bool) -> int {
  if a || b { if c { 1 } }
  0
}
"#
    );
}

#[test]
fn fix_replaceable_with_zero_fill() {
    assert_fix_snapshot!(
        r#"
struct Cfg { a: int, b: int, c: int, d: int }

pub fn f() -> Cfg {
  Cfg { a: 5, b: 0, c: 0, d: 0 }
}
"#
    );
}

#[test]
fn fix_redundant_else() {
    assert_fix_snapshot!(
        r#"
fn log(n: int) {}

pub fn f(x: int) {
  if x > 0 {
    return
  } else {
    let y = x + 1
    log(y)
  }
  log(x)
}
"#
    );
}

#[test]
fn fix_redundant_else_single_line() {
    assert_fix_snapshot!(
        r#"
fn log(n: int) {}

pub fn f(x: int) {
  if x > 0 { return } else { log(x) }
  log(0)
}
"#
    );
}

#[test]
fn fix_redundant_else_nested_body() {
    assert_fix_snapshot!(
        r#"
fn log(n: int) {}

pub fn f(x: int) {
  if x > 0 {
    return
  } else {
    if x < -5 {
      log(1)
    }
    log(2)
  }
  log(3)
}
"#
    );
}

#[test]
fn fix_collapsible_if_parenthesizes_pipeline_operand() {
    assert_fix_snapshot!(
        r#"
fn is_even(n: int) -> bool { n % 2 == 0 }
fn work() {}

pub fn f(x: int, b: bool) {
  if x |> is_even { if b { work() } }
}
"#
    );
}

#[test]
fn fix_collapsible_if_keeps_comparison_unparenthesized() {
    assert_fix_snapshot!(
        r#"
fn work() {}

pub fn f(x: int, y: int) {
  if x > 0 { if y > 0 { work() } }
}
"#
    );
}

#[test]
fn collapsible_if_with_dropped_comment_has_no_fix() {
    assert_no_fix!(
        r#"
fn work() {}
fn other() {}

pub fn f(a: bool, b: bool) {
  if a {
    // keep me
    if b { work() }
  }
  other()
}
"#
    );
}

#[test]
fn redundant_else_leaking_binding_has_no_fix() {
    assert_no_fix!(
        r#"
fn log(n: int) {}

pub fn f(cond: bool) -> int {
  let y = 0
  if cond { return 9 } else { let y = 1; log(y) }
  y
}
"#
    );
}

#[test]
fn fix_redundant_else_lifts_binding_used_only_inside() {
    assert_fix_snapshot!(
        r#"
fn log(n: int) {}

pub fn f(cond: bool) {
  if cond { return } else { let y = 1; log(y) }
  log(0)
}
"#
    );
}
