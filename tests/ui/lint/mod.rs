use crate::_harness::build::compile_check;
use crate::_harness::filesystem::MockFileSystem;
use crate::{assert_lint_snapshot, assert_no_lint_warnings};
use semantics::store::ENTRY_MODULE_ID;

#[test]
fn unused_variable() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5;
  ()
}
"#
    );
}

#[test]
fn unused_as_alias_in_match_arm() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let opt: Option<int> = Some(1)
  match opt {
    Some(n) as unused => n,
    None => 0,
  };
}
"#
    );
}

#[test]
fn unused_variable_suppressed_by_underscore() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let _x = 5;
  ()
}
"#
    );
}

#[test]
fn unused_variable_struct_field_shorthand() {
    assert_lint_snapshot!(
        r#"
struct Point { x: int }

fn main() {
  let p = Point { x: 1 };
  let Point { x } = p;
  ()
}
"#
    );
}

#[test]
fn unused_variable_struct_field_explicit() {
    assert_lint_snapshot!(
        r#"
struct Point { x: int }

fn main() {
  let p = Point { x: 1 };
  let Point { x: foo } = p;
  ()
}
"#
    );
}

#[test]
fn used_variable_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x = 5;
  let _ = x
}
"#
    );
}

#[test]
fn or_pattern_binding_no_spurious_unused_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let t = (42, 1);
  let _ = match t {
    (x, 1) | (x, 2) => x,
    _ => 0,
  };
}
"#
    );
}

#[test]
fn unused_mut() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let mut x = 5;
  x
}
"#
    );
}

#[test]
fn used_mut_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let mut x = 5;
  x = 10;
  let _ = x
}
"#
    );
}

#[test]
fn mut_param_no_unnecessary_mut_warning() {
    assert_no_lint_warnings!(
        r#"
fn process(mut items: Slice<int>) -> Slice<int> {
  items = items.append(42);
  items
}

fn main() {
  let mut x = [3, 1, 2];
  let _ = process(x)
}
"#
    );
}

#[test]
fn referenced_mut_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn mutate(r: Ref<int>) {
  r.* = 99
}

fn main() {
  let mut x = 5;
  mutate(&x);
  let _ = x
}
"#
    );
}

#[test]
fn ref_method_mut_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Counter {
  value: int,
}

impl Counter {
  fn increment(self: Ref<Counter>) {
    self.value += 1;
  }

  fn get(self: Counter) -> int {
    self.value
  }
}

fn main() {
  let mut c = Counter { value: 0 };
  c.increment();
  let _ = c.get()
}
"#
    );
}

#[test]
fn written_but_not_read_simple() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let mut x = 0
  x = 42
  ()
}
"#
    );
}

#[test]
fn written_but_not_read_simple_reassignment() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let mut a = 0
  a = 42
  ()
}
"#
    );
}

#[test]
fn written_but_not_read_in_match() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let mut status = "init"
  let opt: Option<int> = None
  match opt {
    Some(_) => { status = "found" },
    None => { status = "missing" },
  }
  ()
}
"#
    );
}

#[test]
fn written_but_not_read_suppressed_by_underscore() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let mut _flag = false
  _flag = true
  ()
}
"#
    );
}

#[test]
fn written_and_read_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let mut x = 0
  x = 42
  let _ = x
}
"#
    );
}

#[test]
fn unused_value() {
    assert_lint_snapshot!(
        r#"
fn main() {
  1 + 2;
  ()
}
"#
    );
}

#[test]
fn unused_literal() {
    assert_lint_snapshot!(
        r#"
fn main() {
  42;
  ()
}
"#
    );
}

#[test]
fn unused_result() {
    assert_lint_snapshot!(
        r#"
fn get_result() -> Result<int, string> {
  Ok(42)
}

fn main() {
  get_result();
  ()
}
"#
    );
}

#[test]
fn unused_result_handled_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn get_result() -> Result<int, string> {
  Ok(42)
}

fn main() {
  let _ = get_result();
  ()
}
"#
    );
}

#[test]
fn allow_unused_result_suppresses_lint() {
    assert_no_lint_warnings!(
        r#"
#[allow(unused_result)]
fn get_result() -> Result<int, string> {
  Ok(42)
}

fn main() {
  get_result();
  ()
}
"#
    );
}

#[test]
fn allow_unused_result_scoped_to_annotated_function() {
    assert_lint_snapshot!(
        r#"
#[allow(unused_result)]
fn safe_call() -> Result<int, string> {
  Ok(1)
}

fn unsafe_call() -> Result<int, string> {
  Ok(2)
}

fn main() {
  safe_call();
  unsafe_call();
  ()
}
"#
    );
}

#[test]
fn allow_unused_result_does_not_suppress_option() {
    assert_lint_snapshot!(
        r#"
#[allow(unused_result)]
fn get_option() -> Option<int> {
  Some(42)
}

fn main() {
  get_option();
  ()
}
"#
    );
}

#[test]
fn unused_option() {
    assert_lint_snapshot!(
        r#"
fn get_option() -> Option<int> {
  Some(42)
}

fn main() {
  get_option();
  ()
}
"#
    );
}

#[test]
fn unused_option_handled_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn get_option() -> Option<int> {
  Some(42)
}

fn main() {
  let _ = get_option();
  ()
}
"#
    );
}

#[test]
fn match_in_statement_position_with_unit_arms_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"
fn get_result() -> Result<int, string> {
  Ok(42)
}

fn main() {
  let r = get_result();
  match r {
    Ok(_) => fmt.Println("ok"),
    Err(_) => fmt.Println("err"),
  }
  ()
}
"#
    );
}

#[test]
fn match_in_statement_position_with_unit_arms_no_value_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"
fn main() {
  let x = 1;
  match x {
    1 => fmt.Println("one"),
    _ => fmt.Println("other"),
  }
  ()
}
"#
    );
}

#[test]
fn if_in_statement_position_with_unit_branches_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"
fn main() {
  let x = 1;
  if x > 0 {
    fmt.Println("positive")
  } else {
    fmt.Println("negative")
  }
  ()
}
"#
    );
}

#[test]
fn unused_param() {
    assert_lint_snapshot!(
        r#"
pub fn foo(x: int) -> int {
  42
}
"#
    );
}

#[test]
fn unused_param_suppressed_by_underscore() {
    assert_no_lint_warnings!(
        r#"
pub fn foo(_x: int) -> int {
  42
}
"#
    );
}

#[test]
fn used_param_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn foo(x: int) -> int {
  x
}
"#
    );
}

#[test]
fn self_assignment() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let mut x = 5;
  x = x;
  let _ = x
}
"#
    );
}

#[test]
fn self_assignment_with_parens() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let mut x = 5;
  x = (x);
  let _ = x
}
"#
    );
}

#[test]
fn manual_compound_assignment_addition() {
    assert_lint_snapshot!(
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
fn manual_compound_assignment_shift() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let mut x = 5;
  x = x << 2;
  let _ = x
}
"#
    );
}

#[test]
fn manual_compound_assignment_parenthesized_value() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let mut x = 5;
  x = (x * 2);
  let _ = x
}
"#
    );
}

#[test]
fn manual_compound_assignment_field() {
    assert_lint_snapshot!(
        r#"
struct Counter {
  count: int
}

fn main() {
  let mut c = Counter { count: 0 };
  c.count = c.count + 1;
  let _ = c.count
}
"#
    );
}

#[test]
fn manual_compound_assignment_already_compound_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let mut x = 5;
  x += 1;
  let _ = x
}
"#
    );
}

#[test]
fn manual_compound_assignment_target_on_right_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let mut x = 5;
  x = 1 - x;
  let _ = x
}
"#
    );
}

#[test]
fn manual_compound_assignment_other_operand_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let y = 3;
  let mut x = 5;
  x = y + 1;
  let _ = x
}
"#
    );
}

#[test]
fn misrefactored_assign_op_addition() {
    assert_lint_snapshot!(
        r#"
fn add(b: int) {
  let mut a = 1
  a += a + b
  let _ = a
}

fn main() {
  add(2)
}
"#
    );
}

#[test]
fn misrefactored_assign_op_multiplication() {
    assert_lint_snapshot!(
        r#"
fn scale(b: int) {
  let mut a = 2
  a *= a * b
  let _ = a
}

fn main() {
  scale(2)
}
"#
    );
}

#[test]
fn misrefactored_assign_op_target_on_right() {
    assert_lint_snapshot!(
        r#"
fn add(b: int) {
  let mut a = 1
  a += b + a
  let _ = a
}

fn main() {
  add(2)
}
"#
    );
}

#[test]
fn misrefactored_assign_op_field() {
    assert_lint_snapshot!(
        r#"
struct Counter { count: int }

fn main() {
  let mut c = Counter { count: 0 }
  c.count += c.count + 1
  let _ = c.count
}
"#
    );
}

#[test]
fn misrefactored_assign_op_simple_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn add(b: int) {
  let mut a = 1
  a += b
  let _ = a
}

fn main() {
  add(2)
}
"#
    );
}

#[test]
fn misrefactored_assign_op_mixed_operator_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn add(b: int) {
  let mut a = 1
  a += b - a
  let _ = a
}

fn main() {
  add(2)
}
"#
    );
}

#[test]
fn misrefactored_assign_op_distinct_operands_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn add(b: int, c: int) {
  let mut a = 1
  a += b + c
  let _ = a
}

fn main() {
  add(2, 3)
}
"#
    );
}

#[test]
fn misrefactored_assign_op_noncommutative_target_on_right_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn sub(b: int) {
  let mut a = 1
  a -= b - a
  let _ = a
}

fn main() {
  sub(2)
}
"#
    );
}

#[test]
fn neg_multiply_literal_right() {
    assert_lint_snapshot!(
        r#"
fn negate(x: int) -> int {
  x * -1
}

fn main() {
  let _ = negate(3)
}
"#
    );
}

#[test]
fn neg_multiply_literal_left() {
    assert_lint_snapshot!(
        r#"
fn negate(x: int) -> int {
  -1 * x
}

fn main() {
  let _ = negate(3)
}
"#
    );
}

#[test]
fn neg_multiply_float() {
    assert_lint_snapshot!(
        r#"
fn negate(f: float64) -> float64 {
  f * -1
}

fn main() {
  let _ = negate(3.0)
}
"#
    );
}

#[test]
fn neg_multiply_parenthesized() {
    assert_lint_snapshot!(
        r#"
fn negate(x: int, y: int) -> int {
  (x + y) * -1
}

fn main() {
  let _ = negate(3, 4)
}
"#
    );
}

#[test]
fn neg_multiply_compound_assignment_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let mut a = 5
  a *= -1
  let _ = a
}
"#
    );
}

#[test]
fn neg_multiply_other_factor_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn scale(x: int) -> int {
  x * -2
}

fn main() {
  let _ = scale(3)
}
"#
    );
}

#[test]
fn manual_is_empty_equals_zero() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let xs = [1, 2, 3]
  let _ = xs.length() == 0
}
"#
    );
}

#[test]
fn manual_is_empty_not_equals_zero_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3]
  let _ = xs.length() != 0
}
"#
    );
}

#[test]
fn manual_is_empty_greater_than_zero_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3]
  let _ = xs.length() > 0
}
"#
    );
}

#[test]
fn manual_is_empty_zero_on_left() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let xs = [1, 2, 3]
  let _ = 0 == xs.length()
}
"#
    );
}

#[test]
fn manual_is_empty_string_receiver() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let s = "hello"
  let _ = s.length() == 0
}
"#
    );
}

#[test]
fn manual_is_empty_field_receiver() {
    assert_lint_snapshot!(
        r#"
struct Bag {
  items: Slice<int>
}

fn main() {
  let b = Bag { items: [1, 2, 3] }
  let _ = b.items.length() == 0
}
"#
    );
}

#[test]
fn manual_is_empty_less_or_equal_zero() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let xs = [1, 2, 3]
  let _ = xs.length() <= 0
}
"#
    );
}

#[test]
fn manual_is_empty_zero_greater_or_equal() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let xs = [1, 2, 3]
  let _ = 0 >= xs.length()
}
"#
    );
}

#[test]
fn manual_is_empty_zero_less_than_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3]
  let _ = 0 < xs.length()
}
"#
    );
}

#[test]
fn manual_is_empty_zero_not_equals_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3]
  let _ = 0 != xs.length()
}
"#
    );
}

#[test]
fn manual_is_empty_compare_one_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3]
  let _ = xs.length() == 1
}
"#
    );
}

#[test]
fn manual_is_empty_user_type_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Stack {
  depth: int
}

impl Stack {
  fn length(self) -> int {
    self.depth
  }
}

fn main() {
  let st = Stack { depth: 0 }
  let _ = st.length() == 0
}
"#
    );
}

#[test]
fn manual_is_empty_already_is_empty_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3]
  let _ = xs.is_empty()
}
"#
    );
}

#[test]
fn manual_bytes_equal() {
    assert_lint_snapshot!(
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
fn manual_bytes_equal_not_equal() {
    assert_lint_snapshot!(
        r#"
import "go:bytes"

fn main() {
  let a = "hello" as Slice<byte>
  let b = "world" as Slice<byte>
  let _ = bytes.Compare(a, b) != 0
}
"#
    );
}

#[test]
fn manual_bytes_equal_yoda() {
    assert_lint_snapshot!(
        r#"
import "go:bytes"

fn main() {
  let a = "hello" as Slice<byte>
  let b = "world" as Slice<byte>
  let _ = 0 == bytes.Compare(a, b)
}
"#
    );
}

#[test]
fn manual_bytes_equal_parenthesized() {
    assert_lint_snapshot!(
        r#"
import "go:bytes"

fn main() {
  let a = "hello" as Slice<byte>
  let b = "world" as Slice<byte>
  let _ = (bytes.Compare(a, b)) == 0
}
"#
    );
}

#[test]
fn manual_bytes_equal_aliased_import() {
    assert_lint_snapshot!(
        r#"
import mybytes "go:bytes"

fn main() {
  let a = "hello" as Slice<byte>
  let b = "world" as Slice<byte>
  let _ = mybytes.Compare(a, b) == 0
}
"#
    );
}

#[test]
fn manual_bytes_equal_ordering_comparison_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:bytes"

fn main() {
  let a = "hello" as Slice<byte>
  let b = "world" as Slice<byte>
  let _ = bytes.Compare(a, b) < 0
}
"#
    );
}

#[test]
fn manual_bytes_equal_nonzero_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:bytes"

fn main() {
  let a = "hello" as Slice<byte>
  let b = "world" as Slice<byte>
  let _ = bytes.Compare(a, b) == 1
}
"#
    );
}

#[test]
fn manual_bytes_equal_strings_compare_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:strings"

fn main() {
  let _ = strings.Compare("a", "b") == 0
}
"#
    );
}

#[test]
fn redundant_sprintf() {
    assert_lint_snapshot!(
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
fn redundant_sprintf_aliased_import() {
    assert_lint_snapshot!(
        r#"
import myfmt "go:fmt"

fn main() {
  let s = "hello"
  let _ = myfmt.Sprintf("%s", s)
}
"#
    );
}

#[test]
fn redundant_sprintf_call_argument() {
    assert_lint_snapshot!(
        r#"
import "go:fmt"

fn greet() -> string {
  "hi"
}

fn main() {
  let _ = fmt.Sprintf("%s", greet())
}
"#
    );
}

#[test]
fn redundant_sprintf_non_string_argument_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"

fn main() {
  let n = 42
  let _ = fmt.Sprintf("%s", n)
}
"#
    );
}

#[test]
fn redundant_sprintf_byte_slice_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"

fn main() {
  let b = "hi" as Slice<byte>
  let _ = fmt.Sprintf("%s", b)
}
"#
    );
}

#[test]
fn redundant_sprintf_quote_verb_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"

fn main() {
  let s = "hello"
  let _ = fmt.Sprintf("%q", s)
}
"#
    );
}

#[test]
fn redundant_sprintf_prefixed_format_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"

fn main() {
  let s = "hello"
  let _ = fmt.Sprintf("value: %s", s)
}
"#
    );
}

#[test]
fn redundant_sprintf_multiple_arguments_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"

fn main() {
  let s = "hello"
  let _ = fmt.Sprintf("%s %s", s, s)
}
"#
    );
}

#[test]
fn redundant_sprintf_sprint_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"

fn main() {
  let s = "hello"
  let _ = fmt.Sprint(s)
}
"#
    );
}

#[test]
fn manual_replace_all() {
    assert_lint_snapshot!(
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
fn manual_replace_all_aliased_import() {
    assert_lint_snapshot!(
        r#"
import mystr "go:strings"

fn main() {
  let s = "hello world"
  let _ = mystr.Replace(s, "o", "0", -1)
}
"#
    );
}

#[test]
fn manual_replace_all_positive_count_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:strings"

fn main() {
  let s = "hello world"
  let _ = strings.Replace(s, "o", "0", 2)
}
"#
    );
}

#[test]
fn manual_replace_all_zero_count_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:strings"

fn main() {
  let s = "hello world"
  let _ = strings.Replace(s, "o", "0", 0)
}
"#
    );
}

#[test]
fn manual_replace_all_already_replace_all_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:strings"

fn main() {
  let s = "hello world"
  let _ = strings.ReplaceAll(s, "o", "0")
}
"#
    );
}

#[test]
fn manual_replace_all_bytes_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:bytes"

fn main() {
  let bs = "hi" as Slice<byte>
  let _ = bytes.Replace(bs, bs, bs, -1)
}
"#
    );
}

#[test]
fn manual_replace_all_user_type_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Editor {}

impl Editor {
  fn Replace(self, s: string, _old: string, _new: string, _n: int) -> string {
    s
  }
}

fn main() {
  let e = Editor {}
  let _ = e.Replace("a", "b", "c", -1)
}
"#
    );
}

#[test]
fn needless_splitn() {
    assert_lint_snapshot!(
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
fn needless_split_after_n() {
    assert_lint_snapshot!(
        r#"
import "go:strings"

fn main() {
  let s = "a,b,c"
  let _ = strings.SplitAfterN(s, ",", -1)
}
"#
    );
}

#[test]
fn needless_splitn_aliased_import() {
    assert_lint_snapshot!(
        r#"
import mystr "go:strings"

fn main() {
  let s = "a,b,c"
  let _ = mystr.SplitN(s, ",", -1)
}
"#
    );
}

#[test]
fn needless_splitn_positive_count_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:strings"

fn main() {
  let s = "a,b,c"
  let _ = strings.SplitN(s, ",", 2)
}
"#
    );
}

#[test]
fn needless_splitn_zero_count_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:strings"

fn main() {
  let s = "a,b,c"
  let _ = strings.SplitN(s, ",", 0)
}
"#
    );
}

#[test]
fn needless_splitn_user_type_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Splitter {}

impl Splitter {
  fn SplitN(self, s: string, _sep: string, _n: int) -> Slice<string> {
    s.split(_sep)
  }
}

fn main() {
  let sp = Splitter {}
  let _ = sp.SplitN("a,b", ",", -1)
}
"#
    );
}

#[test]
fn manual_rotate() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x: uint32 = 7
  let _ = (x << 5) | (x >> 27)
}
"#
    );
}

#[test]
fn manual_rotate_right_shift_first() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x: uint16 = 7
  let _ = (x >> 4) | (x << 12)
}
"#
    );
}

#[test]
fn manual_rotate_byte() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let b: byte = 3
  let _ = (b << 1) | (b >> 7)
}
"#
    );
}

#[test]
fn manual_rotate_signed_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x: int32 = 7
  let _ = (x << 5) | (x >> 27)
}
"#
    );
}

#[test]
fn manual_rotate_wrong_sum_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x: uint32 = 7
  let _ = (x << 5) | (x >> 20)
}
"#
    );
}

#[test]
fn manual_rotate_different_operands_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x: uint32 = 7
  let y: uint32 = 9
  let _ = (x << 5) | (y >> 27)
}
"#
    );
}

#[test]
fn manual_rotate_same_direction_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x: uint32 = 7
  let _ = (x << 5) | (x << 27)
}
"#
    );
}

#[test]
fn manual_rotate_platform_uint_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x: uint = 7
  let _ = (x << 5) | (x >> 59)
}
"#
    );
}

#[test]
fn manual_rotate_named_type_no_warning() {
    assert_no_lint_warnings!(
        r#"
type Word = uint32

fn main() {
  let x: Word = 7
  let _ = (x << 5) | (x >> 27)
}
"#
    );
}

#[test]
fn manual_rotate_addition_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x: uint32 = 7
  let _ = (x << 5) + (x >> 27)
}
"#
    );
}

#[test]
fn manual_rotate_effectful_operand_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn gen() -> uint32 {
  7
}

fn main() {
  let _ = (gen() << 5) | (gen() >> 27)
}
"#
    );
}

#[test]
fn manual_equal_fold_to_lower() {
    assert_lint_snapshot!(
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
fn manual_equal_fold_to_upper() {
    assert_lint_snapshot!(
        r#"
import "go:strings"

fn main() {
  let a = "Hello"
  let b = "hELLO"
  let _ = strings.ToUpper(a) == strings.ToUpper(b)
}
"#
    );
}

#[test]
fn manual_equal_fold_not_equal() {
    assert_lint_snapshot!(
        r#"
import "go:strings"

fn main() {
  let a = "Hello"
  let b = "hELLO"
  let _ = strings.ToLower(a) != strings.ToLower(b)
}
"#
    );
}

#[test]
fn manual_equal_fold_parenthesized() {
    assert_lint_snapshot!(
        r#"
import "go:strings"

fn main() {
  let a = "Hello"
  let b = "hELLO"
  let _ = (strings.ToLower(a)) == strings.ToLower(b)
}
"#
    );
}

#[test]
fn manual_equal_fold_aliased_import() {
    assert_lint_snapshot!(
        r#"
import mystrings "go:strings"

fn main() {
  let a = "Hello"
  let b = "hELLO"
  let _ = mystrings.ToLower(a) == mystrings.ToLower(b)
}
"#
    );
}

#[test]
fn manual_equal_fold_mixed_case_functions_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:strings"

fn main() {
  let a = "Hello"
  let b = "hELLO"
  let _ = strings.ToLower(a) == strings.ToUpper(b)
}
"#
    );
}

#[test]
fn manual_equal_fold_one_sided_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:strings"

fn main() {
  let a = "Hello"
  let b = "hELLO"
  let _ = strings.ToLower(a) == b
}
"#
    );
}

#[test]
fn manual_equal_fold_other_function_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:strings"

fn main() {
  let a = "Hello"
  let b = "hELLO"
  let _ = strings.TrimSpace(a) == strings.TrimSpace(b)
}
"#
    );
}

#[test]
fn manual_equal_fold_ordering_comparison_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:strings"

fn main() {
  let a = "Hello"
  let b = "hELLO"
  let _ = strings.ToLower(a) < strings.ToLower(b)
}
"#
    );
}

#[test]
fn manual_time_since() {
    assert_lint_snapshot!(
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
fn manual_time_since_aliased_import() {
    assert_lint_snapshot!(
        r#"
import clock "go:time"

fn main() {
  let t = clock.Now()
  let _ = clock.Now().Sub(t)
}
"#
    );
}

#[test]
fn manual_time_since_parenthesized() {
    assert_lint_snapshot!(
        r#"
import "go:time"

fn main() {
  let t = time.Now()
  let _ = (time.Now()).Sub(t)
}
"#
    );
}

#[test]
fn manual_time_since_variable_receiver_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:time"

fn main() {
  let t = time.Now()
  let u = time.Now()
  let _ = u.Sub(t)
}
"#
    );
}

#[test]
fn manual_time_since_add_method_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:time"

fn main() {
  let _ = time.Now().Add(time.Second)
}
"#
    );
}

#[test]
fn manual_time_since_user_type_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Clock {
  ticks: int
}

impl Clock {
  fn Now(self) -> Clock {
    self
  }
  fn Sub(self, other: Clock) -> int {
    self.ticks - other.ticks
  }
}

fn main() {
  let c = Clock { ticks: 0 }
  let _ = c.Now().Sub(c)
}
"#
    );
}

#[test]
fn manual_time_since_field_argument() {
    assert_lint_snapshot!(
        r#"
import "go:time"

struct Timer {
  start: time.Time
}

fn elapsed(timer: Timer) -> time.Duration {
  time.Now().Sub(timer.start)
}
"#
    );
}

#[test]
fn manual_time_since_effectful_argument_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:time"

fn main() {
  let _ = time.Now().Sub(time.Now())
}
"#
    );
}

#[test]
fn manual_time_until() {
    assert_lint_snapshot!(
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
fn manual_time_until_aliased_import() {
    assert_lint_snapshot!(
        r#"
import clock "go:time"

fn main() {
  let deadline = clock.Now()
  let _ = deadline.Sub(clock.Now())
}
"#
    );
}

#[test]
fn manual_time_until_parenthesized() {
    assert_lint_snapshot!(
        r#"
import "go:time"

fn main() {
  let deadline = time.Now()
  let _ = deadline.Sub((time.Now()))
}
"#
    );
}

#[test]
fn manual_time_until_field_receiver() {
    assert_lint_snapshot!(
        r#"
import "go:time"

struct Timer {
  deadline: time.Time
}

fn remaining(timer: Timer) -> time.Duration {
  timer.deadline.Sub(time.Now())
}
"#
    );
}

#[test]
fn manual_time_until_variable_argument_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:time"

fn main() {
  let deadline = time.Now()
  let now = time.Now()
  let _ = deadline.Sub(now)
}
"#
    );
}

#[test]
fn manual_time_until_effectful_receiver_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:time"

fn deadline() -> time.Time {
  time.Now()
}

fn main() {
  let _ = deadline().Sub(time.Now())
}
"#
    );
}

#[test]
fn manual_time_until_user_type_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Clock {
  ticks: int
}

impl Clock {
  fn Now(self) -> Clock {
    self
  }
  fn Sub(self, other: Clock) -> int {
    self.ticks - other.ticks
  }
}

fn main() {
  let c = Clock { ticks: 0 }
  let _ = c.Sub(c.Now())
}
"#
    );
}

#[test]
fn manual_time_until_non_time_receiver_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:time"

struct Marker {
  id: int
}

impl Marker {
  fn Sub(self, _t: time.Time) -> int {
    self.id
  }
}

fn main() {
  let m = Marker { id: 1 }
  let _ = m.Sub(time.Now())
}
"#
    );
}

#[test]
fn manual_time_until_ref_receiver_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:time"

fn main() {
  let deadline = time.Now()
  let r = &deadline
  let _ = r.Sub(time.Now())
}
"#
    );
}

#[test]
fn self_comparison_equal() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5;
  let _ = x == x
}
"#
    );
}

#[test]
fn self_comparison_less_than() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5;
  let _ = x < x
}
"#
    );
}

#[test]
fn self_comparison_with_parens() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5;
  let _ = (x) == x
}
"#
    );
}

#[test]
fn self_comparison_float_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x: float64 = 0.0;
  let _ = x == x
}
"#
    );
}

#[test]
fn self_comparison_float_alias_no_warning() {
    assert_no_lint_warnings!(
        r#"
type Score = float64
fn main() {
  let s: Score = 0.0
  let _ = s != s
}
"#
    );
}

#[test]
fn self_comparison_float_newtype_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Distance(float64)
fn main() {
  let d = Distance(0.0)
  let _ = d != d
}
"#
    );
}

#[test]
fn self_comparison_int_alias_warns() {
    assert_lint_snapshot!(
        r#"
type Id = int
fn main() {
  let n: Id = 3
  let _ = n == n
}
"#
    );
}

#[test]
fn unsigned_comparison_less_than_zero() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let n: uint = 5;
  let _ = n < 0
}
"#
    );
}

#[test]
fn unsigned_comparison_greater_than_or_equal_zero() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let n: uint = 5;
  let _ = n >= 0
}
"#
    );
}

#[test]
fn unsigned_comparison_zero_on_left() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let n: uint = 5;
  let _ = 0 > n
}
"#
    );
}

#[test]
fn unsigned_comparison_byte() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let b: byte = 5;
  let _ = b < 0
}
"#
    );
}

#[test]
fn unsigned_comparison_with_parens() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let n: uint = 5;
  let _ = (n) >= (0)
}
"#
    );
}

#[test]
fn unsigned_equality_with_zero_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let n: uint = 5;
  let _ = n == 0
}
"#
    );
}

#[test]
fn unsigned_less_than_or_equal_zero_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let n: uint = 5;
  let _ = n <= 0
}
"#
    );
}

#[test]
fn signed_comparison_with_zero_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let n: int = 5;
  let _ = n < 0
}
"#
    );
}

#[test]
fn unsigned_comparison_with_nonzero_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let n: uint = 5;
  let _ = n < 10
}
"#
    );
}

#[test]
fn unsigned_comparison_named_newtype() {
    assert_lint_snapshot!(
        r#"
struct Counter(uint8)

fn main() {
  let c = Counter(5)
  let _ = c < 0
}
"#
    );
}

#[test]
fn non_negative_comparison_less_than_zero() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let xs = [1, 2, 3]
  let _ = xs.length() < 0
}
"#
    );
}

#[test]
fn non_negative_comparison_greater_or_equal_zero() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let xs = [1, 2, 3]
  let _ = xs.length() >= 0
}
"#
    );
}

#[test]
fn non_negative_comparison_zero_greater() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let xs = [1, 2, 3]
  let _ = 0 > xs.length()
}
"#
    );
}

#[test]
fn non_negative_comparison_zero_less_or_equal() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let xs = [1, 2, 3]
  let _ = 0 <= xs.length()
}
"#
    );
}

#[test]
fn non_negative_comparison_string_receiver() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let s = "hello"
  let _ = s.length() < 0
}
"#
    );
}

#[test]
fn non_negative_comparison_ref_receiver() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let xs = [1, 2, 3]
  let r = &xs
  let _ = r.length() < 0
}
"#
    );
}

#[test]
fn non_negative_comparison_native_identifier_form() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let xs = [1, 2, 3]
  let _ = Slice.length(xs) < 0
}
"#
    );
}

#[test]
fn non_negative_comparison_pipeline_form() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let xs = [1, 2, 3]
  let _ = (xs |> Slice.length) >= 0
}
"#
    );
}

#[test]
fn non_negative_comparison_with_parens() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let xs = [1, 2, 3]
  let _ = (xs.length()) >= (0)
}
"#
    );
}

#[test]
fn non_negative_comparison_user_type_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Stack {
  depth: int
}

impl Stack {
  fn length(self) -> int {
    self.depth
  }
}

fn main() {
  let st = Stack { depth: 0 }
  let _ = st.length() < 0
}
"#
    );
}

#[test]
fn non_negative_comparison_user_type_ufcs_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Stack {
  depth: int
}

impl Stack {
  fn length(self) -> int {
    self.depth - 100
  }
}

fn main() {
  let st = Stack { depth: 0 }
  let _ = Stack.length(st) < 0
}
"#
    );
}

#[test]
fn non_negative_comparison_alias_identifier_form() {
    assert_lint_snapshot!(
        r#"
type MyString = string

fn main() {
  let s: MyString = "hi"
  let _ = MyString.length(s) < 0
}
"#
    );
}

#[test]
fn non_negative_comparison_nonzero_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3]
  let _ = xs.length() < 5
}
"#
    );
}

#[test]
fn type_limit_comparison_uint8_le_max() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let a: uint8 = 5
  let _ = a <= 255
}
"#
    );
}

#[test]
fn type_limit_comparison_uint8_gt_max() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let a: uint8 = 5
  let _ = a > 255
}
"#
    );
}

#[test]
fn type_limit_comparison_int32_gt_max() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let b: int32 = 5
  let _ = b > 2147483647
}
"#
    );
}

#[test]
fn type_limit_comparison_int8_lt_min() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let c: int8 = 5
  let _ = c < -128
}
"#
    );
}

#[test]
fn type_limit_comparison_int8_ge_min() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let c: int8 = 5
  let _ = c >= -128
}
"#
    );
}

#[test]
fn type_limit_comparison_literal_on_left() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let a: uint8 = 5
  let _ = 255 < a
}
"#
    );
}

#[test]
fn type_limit_comparison_byte_max() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let b: byte = 5
  let _ = b > 255
}
"#
    );
}

#[test]
fn type_limit_comparison_below_max_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let a: uint8 = 5
  let _ = a <= 254
}
"#
    );
}

#[test]
fn type_limit_comparison_lt_max_not_constant_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let a: uint8 = 5
  let _ = a < 255
}
"#
    );
}

#[test]
fn type_limit_comparison_ge_max_not_constant_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let a: uint8 = 5
  let _ = a >= 255
}
"#
    );
}

#[test]
fn type_limit_comparison_equality_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let a: uint8 = 5
  let _ = a == 255
}
"#
    );
}

#[test]
fn type_limit_comparison_platform_width_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let n: int = 5
  let _ = n <= 100
  let m: uint = 5
  let _ = m <= 100
}
"#
    );
}

#[test]
fn type_limit_comparison_rune_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let r: rune = 'x'
  let _ = r <= 2147483647
}
"#
    );
}

#[test]
fn type_limit_comparison_named_newtype() {
    assert_lint_snapshot!(
        r#"
struct Small(uint8)

fn main() {
  let s = Small(5)
  let _ = s <= 255
}
"#
    );
}

#[test]
fn type_limit_comparison_alias() {
    assert_lint_snapshot!(
        r#"
type Big = uint16

fn main() {
  let b: Big = 5
  let _ = b > 65535
}
"#
    );
}

#[test]
fn redundant_operation_add_zero() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5
  let _ = x + 0
}
"#
    );
}

#[test]
fn redundant_operation_zero_add() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5
  let _ = 0 + x
}
"#
    );
}

#[test]
fn redundant_operation_sub_zero() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5
  let _ = x - 0
}
"#
    );
}

#[test]
fn redundant_operation_mul_one() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5
  let _ = x * 1
}
"#
    );
}

#[test]
fn redundant_operation_div_one() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5
  let _ = x / 1
}
"#
    );
}

#[test]
fn redundant_operation_mul_zero() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5
  let _ = x * 0
}
"#
    );
}

#[test]
fn redundant_operation_bitwise_or_zero() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5
  let _ = x | 0
}
"#
    );
}

#[test]
fn redundant_operation_bitwise_and_zero() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5
  let _ = x & 0
}
"#
    );
}

#[test]
fn redundant_operation_bitwise_and_not_zero() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5
  let _ = x &^ 0
}
"#
    );
}

#[test]
fn redundant_operation_shift_left_zero() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5
  let _ = x << 0
}
"#
    );
}

#[test]
fn redundant_operation_shift_right_zero() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5
  let _ = x >> 0
}
"#
    );
}

#[test]
fn redundant_operation_unsigned_add_zero() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let u: uint = 5
  let _ = u + 0
}
"#
    );
}

#[test]
fn redundant_operation_identity_keeps_call() {
    assert_lint_snapshot!(
        r#"
fn ident(n: int) -> int { n }

fn main() {
  let x = 5
  let _ = ident(x) + 0
}
"#
    );
}

#[test]
fn redundant_operation_and_true() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let b = true
  let _ = b && true
}
"#
    );
}

#[test]
fn redundant_operation_or_false() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let b = true
  let _ = b || false
}
"#
    );
}

#[test]
fn redundant_operation_and_false() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let b = true
  let _ = b && false
}
"#
    );
}

#[test]
fn redundant_operation_or_true() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let b = true
  let _ = b || true
}
"#
    );
}

#[test]
fn redundant_operation_float_add_zero_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let f: float64 = 1.5
  let _ = f + 0
}
"#
    );
}

#[test]
fn redundant_operation_string_concat_empty_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let s = "hi"
  let _ = s + ""
}
"#
    );
}

#[test]
fn redundant_operation_side_effecting_constant_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn ident(n: int) -> int { n }

fn main() {
  let x = 5
  let _ = ident(x) * 0
}
"#
    );
}

#[test]
fn redundant_operation_non_trivial_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x = 5
  let _ = x + 2
}
"#
    );
}

#[test]
fn redundant_operation_zero_shift_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let n: int = 3
  let _ = 0 << n
  let _ = 0 >> n
}
"#
    );
}

#[test]
fn redundant_operation_modulo_one() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5
  let _ = x % 1
}
"#
    );
}

#[test]
fn redundant_operation_modulo_reversed_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x = 5
  let _ = 1 % x
}
"#
    );
}

#[test]
fn integer_division_to_zero_basic() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let _ = 1 / 2
}
"#
    );
}

#[test]
fn integer_division_to_zero_larger_denominator() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let _ = 5 / 10
}
"#
    );
}

#[test]
fn integer_division_to_zero_parenthesized() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let _ = (1) / (2)
}
"#
    );
}

#[test]
fn integer_division_to_zero_numerator_zero_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let _ = 0 / 5
}
"#
    );
}

#[test]
fn integer_division_to_zero_exact_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let _ = 4 / 2
}
"#
    );
}

#[test]
fn integer_division_to_zero_equal_operands_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let _ = 2 / 2
}
"#
    );
}

#[test]
fn integer_division_to_zero_float_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let _ = 1.0 / 2.0
}
"#
    );
}

#[test]
fn integer_division_to_zero_variable_operand_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x = 5
  let _ = x / 10
}
"#
    );
}

#[test]
fn integer_division_to_zero_negative_numerator() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let _ = -1 / 2
}
"#
    );
}

#[test]
fn integer_division_to_zero_negative_denominator() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let _ = 1 / -2
}
"#
    );
}

#[test]
fn integer_division_to_zero_negative_non_truncating_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let _ = -3 / 2
}
"#
    );
}

#[test]
fn negated_equality_equal() {
    assert_lint_snapshot!(
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
fn negated_equality_not_equal() {
    assert_lint_snapshot!(
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
fn negated_relational_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x = 1;
  let y = 2;
  let _ = !(x < y)
}
"#
    );
}

#[test]
fn negation_of_identifier_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let a = true;
  let b = false;
  let _ = !a == b
}
"#
    );
}

#[test]
fn negated_conjunction_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let a = true;
  let b = false;
  let _ = !(a && b)
}
"#
    );
}

#[test]
fn unnecessary_return_simple() {
    assert_lint_snapshot!(
        r#"
fn five() -> int {
  return 5
}

fn main() {
  let _ = five()
}
"#
    );
}

#[test]
fn unnecessary_return_after_statements() {
    assert_lint_snapshot!(
        r#"
fn doubled(n: int) -> int {
  let x = n * 2
  return x
}

fn main() {
  let _ = doubled(3)
}
"#
    );
}

#[test]
fn unnecessary_return_if_else_branches() {
    assert_lint_snapshot!(
        r#"
fn sign(n: int) -> int {
  if n > 0 {
    return 1
  } else {
    return 2
  }
}

fn main() {
  let _ = sign(3)
}
"#
    );
}

#[test]
fn unnecessary_return_match_arms() {
    assert_lint_snapshot!(
        r#"
fn label(n: int) -> string {
  match n {
    0 => return "zero",
    _ => return "other",
  }
}

fn main() {
  let _ = label(0)
}
"#
    );
}

#[test]
fn unnecessary_return_early_return_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn clamp(n: int) -> int {
  if n > 0 {
    return 1
  }
  n + 1
}

fn main() {
  let _ = clamp(1)
}
"#
    );
}

#[test]
fn unnecessary_return_in_loop_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn first_positive(xs: Slice<int>) -> int {
  for x in xs {
    if x > 0 {
      return x
    }
  }
  0
}

fn main() {
  let _ = first_positive([1])
}
"#
    );
}

#[test]
fn unnecessary_return_bare_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn greet() {
  return
}

fn main() {
  greet()
}
"#
    );
}

#[test]
fn unnecessary_return_let_else_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn unwrap_or_zero(x: Option<int>) -> int {
  let Some(v) = x else {
    return 0
  }
  v
}

fn main() {
  let _ = unwrap_or_zero(Some(3))
}
"#
    );
}

#[test]
fn unnecessary_return_in_lambda_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn apply() -> int {
  let f = || {
    return 1
  }
  f()
}

fn main() {
  let _ = apply()
}
"#
    );
}

#[test]
fn nan_comparison_equal() {
    assert_lint_snapshot!(
        r#"
import "go:math"
fn main() {
  let x: float64 = 1.0
  let _ = x == math.NaN()
}
"#
    );
}

#[test]
fn nan_comparison_not_equal() {
    assert_lint_snapshot!(
        r#"
import "go:math"
fn main() {
  let x: float64 = 1.0
  let _ = x != math.NaN()
}
"#
    );
}

#[test]
fn nan_comparison_less_than() {
    assert_lint_snapshot!(
        r#"
import "go:math"
fn main() {
  let x: float64 = 1.0
  let _ = x < math.NaN()
}
"#
    );
}

#[test]
fn nan_comparison_nan_on_left() {
    assert_lint_snapshot!(
        r#"
import "go:math"
fn main() {
  let x: float64 = 1.0
  let _ = math.NaN() >= x
}
"#
    );
}

#[test]
fn nan_comparison_with_parens() {
    assert_lint_snapshot!(
        r#"
import "go:math"
fn main() {
  let x: float64 = 1.0
  let _ = (x) == (math.NaN())
}
"#
    );
}

#[test]
fn is_nan_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:math"
fn main() {
  let x: float64 = 1.0
  let _ = math.IsNaN(x)
}
"#
    );
}

#[test]
fn nan_comparison_other_math_function_no_warning() {
    let diagnostics = crate::_harness::lint::lint(
        r#"
import "go:math"
fn main() {
  let x: float64 = 1.0
  let _ = x == math.Pi
}
"#,
    );
    let codes: Vec<&str> = diagnostics.iter().filter_map(|d| d.code_str()).collect();
    assert!(
        !codes.contains(&"infer.nan_comparison"),
        "nan_comparison must fire only on `math.NaN()`, not other math values: {codes:?}"
    );
}

#[test]
fn nan_comparison_user_defined_function_no_warning() {
    let diagnostics = crate::_harness::lint::lint(
        r#"
fn nan() -> float64 {
  0.0
}

fn main() {
  let x: float64 = 1.0
  let _ = x == nan()
}
"#,
    );
    let codes: Vec<&str> = diagnostics.iter().filter_map(|d| d.code_str()).collect();
    assert!(
        !codes.contains(&"infer.nan_comparison"),
        "nan_comparison must fire only on `go:math.NaN`, not a user `nan`: {codes:?}"
    );
}

#[test]
fn impossible_comparison_contradictory_range() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5;
  let _ = x < 5 && x > 10
}
"#
    );
}

#[test]
fn impossible_comparison_literal_on_left() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5;
  let _ = 5 > x && 10 < x
}
"#
    );
}

#[test]
fn impossible_comparison_exclusive_boundary() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5;
  let _ = x < 5 && x >= 5
}
"#
    );
}

#[test]
fn impossible_comparison_two_equalities() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 3;
  let _ = x == 3 && x == 4
}
"#
    );
}

#[test]
fn impossible_comparison_negative_bounds() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 0;
  let _ = x <= -3 && x >= 0
}
"#
    );
}

#[test]
fn impossible_comparison_float_operand_integer_literals() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let f: float64 = 1.0
  let _ = f < 5 && f > 10
}
"#
    );
}

#[test]
fn impossible_comparison_disjunction_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x = 5;
  let _ = x < 5 || x > 10
}
"#
    );
}

#[test]
fn impossible_comparison_equality_inequality() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5;
  let _ = x == 5 && x != 5
}
"#
    );
}

#[test]
fn impossible_comparison_overlapping_range_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x = 5;
  let _ = x < 10 && x > 5
}
"#
    );
}

#[test]
fn impossible_comparison_chain_with_unrelated_conjunct() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5;
  let flag = true;
  let _ = x < 5 && flag && x > 10
}
"#
    );
}

#[test]
fn impossible_comparison_chain_with_negated_conjunct() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5;
  let flag = true;
  let _ = x < 5 && !flag && x > 10
}
"#
    );
}

#[test]
fn impossible_comparison_distinct_operands_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let a = 1;
  let b = 2;
  let _ = a < 5 && b > 10
}
"#
    );
}

#[test]
fn impossible_comparison_side_effecting_operand_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn pick() -> int { 7 }

fn main() {
  let _ = pick() < 5 && pick() > 10
}
"#
    );
}

#[test]
fn impossible_comparison_side_effecting_conjunct_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn step() -> bool { true }

fn main() {
  let x = 0;
  let _ = x < 5 && step() && x > 10
}
"#
    );
}

#[test]
fn redundant_comparison_disjunction_narrower() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5;
  let _ = x < 5 || x < 10
}
"#
    );
}

#[test]
fn redundant_comparison_conjunction_wider() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5;
  let _ = x < 5 && x < 10
}
"#
    );
}

#[test]
fn redundant_comparison_greater_than() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5;
  let _ = x > 5 || x > 3
}
"#
    );
}

#[test]
fn redundant_comparison_float_operand_integer_literals() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let f: float64 = 1.0
  let _ = f < 5 || f < 10
}
"#
    );
}

#[test]
fn redundant_comparison_proper_overlap_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x = 5;
  let _ = x < 10 && x > 5
}
"#
    );
}

#[test]
fn redundant_comparison_side_effecting_operand_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn pick() -> int { 7 }

fn main() {
  let _ = pick() < 5 || pick() < 10
}
"#
    );
}

#[test]
fn double_comparison_equal_or_less() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5;
  let y = 3;
  let _ = x == y || x < y
}
"#
    );
}

#[test]
fn double_comparison_less_or_greater() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5;
  let y = 3;
  let _ = x < y || x > y
}
"#
    );
}

#[test]
fn double_comparison_inclusive_meet() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5;
  let _ = x <= 5 && x >= 5
}
"#
    );
}

#[test]
fn double_comparison_distinct_bounds_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x = 5;
  let _ = x < 5 || x == 6
}
"#
    );
}

#[test]
fn double_comparison_float_inequality_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x: float64 = 1.0
  let y: float64 = 2.0
  let _ = x < y || x > y
}
"#
    );
}

#[test]
fn double_comparison_float_equal_or_less() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x: float64 = 1.0
  let y: float64 = 2.0
  let _ = x == y || x < y
}
"#
    );
}

#[test]
fn double_comparison_float_inclusive_meet() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x: float64 = 1.0
  let y: float64 = 2.0
  let _ = x <= y && x >= y
}
"#
    );
}

#[test]
fn double_comparison_generic_ordered_inequality_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:cmp"

fn different<T: cmp.Ordered>(x: T, y: T) -> bool {
  x < y || x > y
}

fn main() {
  let _ = different(1, 2)
}
"#
    );
}

#[test]
fn impossible_comparison_skips_type_invalid_operand() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
fn main() {
  let x = "a";
  let _ = x < 5 && x > 10
}
"#,
    );
    let result = compile_check(fs);
    assert!(
        result
            .errors
            .iter()
            .any(|d| d.plain_message().contains("Type mismatch")),
        "expected the type mismatch to still be reported: {:?}",
        result.errors
    );
    assert!(
        !result
            .errors
            .iter()
            .any(|d| d.plain_message() == "Impossible comparison"),
        "must not flag a type-invalid comparison as impossible: {:?}",
        result.errors
    );
}

#[test]
fn impossible_comparison_non_boolean_conjunct_no_diagnostic() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
fn main() {
  let x = 0
  let _ = x < 5 && 3 && x > 10
}
"#,
    );
    let result = compile_check(fs);
    assert!(
        result
            .errors
            .iter()
            .any(|d| d.plain_message().contains("Type mismatch")),
        "expected the non-boolean conjunct type error to still be reported: {:?}",
        result.errors
    );
    assert!(
        !result
            .errors
            .iter()
            .any(|d| d.plain_message() == "Impossible comparison"),
        "must not reason across a non-boolean conjunct in a type-invalid chain: {:?}",
        result.errors
    );
}

#[test]
fn impossible_comparison_invalid_comparison_conjunct_no_diagnostic() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
fn main() {
  let x = 0
  let s = "a"
  let _ = x < 5 && s < 5 && x > 10
}
"#,
    );
    let result = compile_check(fs);
    assert!(
        result
            .errors
            .iter()
            .any(|d| d.plain_message().contains("Type mismatch")),
        "expected the invalid-comparison type error to still be reported: {:?}",
        result.errors
    );
    assert!(
        !result
            .errors
            .iter()
            .any(|d| d.plain_message() == "Impossible comparison"),
        "must not reason across an out-of-scope or invalid comparison conjunct: {:?}",
        result.errors
    );
}

#[test]
fn impossible_comparison_invalid_comparison_inside_or_no_diagnostic() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
fn main() {
  let x = 0
  let flag = true
  let s = "a"
  let _ = x < 5 && (s < 5 || flag) && x > 10
}
"#,
    );
    let result = compile_check(fs);
    assert!(
        result
            .errors
            .iter()
            .any(|d| d.plain_message().contains("Type mismatch")),
        "expected the invalid comparison inside the disjunction to still be reported: {:?}",
        result.errors
    );
    assert!(
        !result
            .errors
            .iter()
            .any(|d| d.plain_message() == "Impossible comparison"),
        "must not reason across a disjunction hiding an invalid comparison: {:?}",
        result.errors
    );
}

#[test]
fn redundant_comparison_skips_type_invalid_operand() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
fn main() {
  let x = "a";
  let _ = x < 5 || x < 10
}
"#,
    );
    let result = compile_check(fs);
    assert!(
        result
            .errors
            .iter()
            .any(|d| d.plain_message().contains("Type mismatch")),
        "expected the type mismatch to still be reported: {:?}",
        result.errors
    );
    assert!(
        !result
            .lints
            .iter()
            .any(|d| d.plain_message() == "Redundant comparison"),
        "must not flag a type-invalid comparison as redundant: {:?}",
        result.lints
    );
}

#[test]
fn double_comparison_skips_type_invalid_operand() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
fn main() {
  let x = "a";
  let _ = x < 5 || x > 5
}
"#,
    );
    let result = compile_check(fs);
    assert!(
        result
            .errors
            .iter()
            .any(|d| d.plain_message().contains("Type mismatch")),
        "expected the type mismatch to still be reported: {:?}",
        result.errors
    );
    assert!(
        !result
            .lints
            .iter()
            .any(|d| d.plain_message() == "Comparisons can be combined"),
        "must not flag a type-invalid comparison as combinable: {:?}",
        result.lints
    );
}

// Cases the checker accepts but the comparison lints deliberately leave out of scope.

#[test]
fn comparison_named_integer_type_out_of_scope() {
    assert_no_lint_warnings!(
        r#"
struct Celsius(int)

fn main() {
  let t = Celsius(5)
  let _ = t < 5 && t > 10
}
"#
    );
}

#[test]
fn comparison_named_uintptr_out_of_scope() {
    assert_no_lint_warnings!(
        r#"
struct Handle(uintptr)

fn main() {
  let h: Handle = 5
  let _ = h == 5 && h != 5
}
"#
    );
}

#[test]
fn comparison_named_string_out_of_scope() {
    assert_no_lint_warnings!(
        r#"
struct Name(string)

fn main() {
  let x: Name = "b"
  let _ = x == "a" || x < "a"
}
"#
    );
}

#[test]
fn comparison_complex_out_of_scope() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let c: complex64 = 1
  let _ = c == 1 && c != 1
}
"#
    );
}

#[test]
fn comparison_float_literal_out_of_scope() {
    assert_no_chain_comparison_lints(
        r#"
fn main() {
  let x: float64 = 1.0
  let _ = x == 1.0 && x != 1.0
}
"#,
    );
}

#[test]
fn comparison_float_ordering_and_mixing_out_of_scope() {
    assert_no_chain_comparison_lints(
        r#"
fn main() {
  let x: float64 = 1.0
  let _ = x < 1.0 && x > 2.0
  let _ = x == 1 && x != 1.0
  let _ = x < 1.0 || x < 2.0
}
"#,
    );
}

/// Float operands are out of scope for the comparison-chain lints (PR 1), even
/// though `float_cmp` legitimately flags the `==`/`!=` operands on its own.
fn assert_no_chain_comparison_lints(source: &str) {
    let diagnostics = crate::_harness::lint::lint(source);
    let codes: Vec<&str> = diagnostics.iter().filter_map(|d| d.code_str()).collect();
    for code in [
        "infer.impossible_comparison",
        "lint.redundant_comparison",
        "lint.double_comparison",
    ] {
        assert!(
            !codes.contains(&code),
            "chain lints must not analyze float operands, found {code}: {codes:?}"
        );
    }
}

#[test]
fn comparison_string_out_of_scope() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let s = "a"
  let _ = s == "a" && s != "a"
}
"#
    );
}

#[test]
fn impossible_comparison_string_satisfiable_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let s = "a"
  let _ = s == "a" && s != "b"
}
"#
    );
}

#[test]
fn comparison_boolean_operands_out_of_scope() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x = true
  let y = false
  let _ = x == y || x < y
}
"#
    );
}

#[test]
fn double_comparison_distinct_named_string_types_no_diagnostic() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
struct A(string)
struct B(string)

fn main() {
  let a = A("a")
  let b = B("b")
  let _ = a < b || a > b
}
"#,
    );
    let result = compile_check(fs);
    assert!(
        result
            .errors
            .iter()
            .any(|d| d.plain_message().contains("Type mismatch")),
        "expected the named-type boundary error to still be reported: {:?}",
        result.errors
    );
    assert!(
        !result
            .lints
            .iter()
            .any(|d| d.plain_message() == "Comparisons can be combined"),
        "must not combine comparisons across a named-type boundary: {:?}",
        result.lints
    );
}

#[test]
fn double_comparison_named_vs_plain_string_no_diagnostic() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
struct A(string)

fn main() {
  let a = A("a")
  let s = "x"
  let _ = a < s || a > s
}
"#,
    );
    let result = compile_check(fs);
    assert!(
        result
            .errors
            .iter()
            .any(|d| d.plain_message().contains("Type mismatch")),
        "expected the named-vs-plain-string error to still be reported: {:?}",
        result.errors
    );
    assert!(
        !result
            .lints
            .iter()
            .any(|d| d.plain_message() == "Comparisons can be combined"),
        "must not combine comparisons of a named type against a plain primitive: {:?}",
        result.lints
    );
}

#[test]
fn impossible_comparison_integer_range_not_flagged() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x = 0
  let _ = x > 0 && x < 1
}
"#
    );
}

#[test]
fn double_comparison_generic_ordered_le_not_flagged() {
    assert_no_lint_warnings!(
        r#"
import "go:cmp"

fn at_most<T: cmp.Ordered>(x: T, y: T) -> bool {
  x == y || x < y
}

fn main() {
  let _ = at_most(1, 2)
}
"#
    );
}

#[test]
fn impossible_comparison_unsigned_negative_literal_no_diagnostic() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
fn main() {
  let x: uint = 1
  let _ = x > -1 && x < -2
}
"#,
    );
    let result = compile_check(fs);
    assert!(
        result
            .errors
            .iter()
            .any(|d| d.plain_message().contains("negate")),
        "expected the unsigned-negation error to still be reported: {:?}",
        result.errors
    );
    assert!(
        !result
            .errors
            .iter()
            .any(|d| d.plain_message() == "Impossible comparison"),
        "must not flag a comparison against an invalid unsigned negation: {:?}",
        result.errors
    );
}

#[test]
fn redundant_comparison_unsigned_negative_literal_no_diagnostic() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
fn main() {
  let x: uint = 1
  let _ = x < -1 || x < -2
}
"#,
    );
    let result = compile_check(fs);
    assert!(
        result
            .errors
            .iter()
            .any(|d| d.plain_message().contains("negate")),
        "expected the unsigned-negation error to still be reported: {:?}",
        result.errors
    );
    assert!(
        !result
            .lints
            .iter()
            .any(|d| d.plain_message() == "Redundant comparison"),
        "must not flag a comparison against an invalid unsigned negation: {:?}",
        result.lints
    );
}

#[test]
fn double_comparison_unsigned_negative_literal_no_diagnostic() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
fn main() {
  let x: uint = 1
  let _ = x == -1 || x < -1
}
"#,
    );
    let result = compile_check(fs);
    assert!(
        result
            .errors
            .iter()
            .any(|d| d.plain_message().contains("negate")),
        "expected the unsigned-negation error to still be reported: {:?}",
        result.errors
    );
    assert!(
        !result
            .lints
            .iter()
            .any(|d| d.plain_message() == "Comparisons can be combined"),
        "must not flag a comparison against an invalid unsigned negation: {:?}",
        result.lints
    );
}

#[test]
fn impossible_comparison_overflow_literal_no_diagnostic() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
fn main() {
  let x: uint8 = 1
  let _ = x > 300 && x < 200
}
"#,
    );
    let result = compile_check(fs);
    assert!(
        result
            .errors
            .iter()
            .any(|d| d.plain_message().contains("overflow")),
        "expected the literal-overflow error to still be reported: {:?}",
        result.errors
    );
    assert!(
        !result
            .errors
            .iter()
            .any(|d| d.plain_message() == "Impossible comparison"),
        "must not flag a comparison against an overflowing literal: {:?}",
        result.errors
    );
}

#[test]
fn redundant_comparison_overflow_literal_no_diagnostic() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
fn main() {
  let x: uint8 = 1
  let _ = x < 300 || x < 400
}
"#,
    );
    let result = compile_check(fs);
    assert!(
        result
            .errors
            .iter()
            .any(|d| d.plain_message().contains("overflow")),
        "expected the literal-overflow error to still be reported: {:?}",
        result.errors
    );
    assert!(
        !result
            .lints
            .iter()
            .any(|d| d.plain_message() == "Redundant comparison"),
        "must not flag a comparison against an overflowing literal: {:?}",
        result.lints
    );
}

#[test]
fn double_comparison_int_overflow_literal_no_diagnostic() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
fn main() {
  let x: int8 = 1
  let _ = x == -200 || x < -200
}
"#,
    );
    let result = compile_check(fs);
    assert!(
        result
            .errors
            .iter()
            .any(|d| d.plain_message().contains("overflow")),
        "expected the literal-overflow error to still be reported: {:?}",
        result.errors
    );
    assert!(
        !result
            .lints
            .iter()
            .any(|d| d.plain_message() == "Comparisons can be combined"),
        "must not flag a comparison against an overflowing literal: {:?}",
        result.lints
    );
}

#[test]
fn double_comparison_float_overflow_literal_no_diagnostic() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
fn main() {
  let x: float32 = 1.0
  let _ = x == 1e100 || x < 1e100
}
"#,
    );
    let result = compile_check(fs);
    assert!(
        result
            .errors
            .iter()
            .any(|d| d.plain_message().contains("overflow")),
        "expected the float-overflow error to still be reported: {:?}",
        result.errors
    );
    assert!(
        !result
            .lints
            .iter()
            .any(|d| d.plain_message() == "Comparisons can be combined"),
        "must not flag a comparison against an overflowing float literal: {:?}",
        result.lints
    );
}

#[test]
fn impossible_comparison_uintptr_operand_no_diagnostic() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
fn get() -> uintptr { panic("x") }

fn main() {
  let x = get()
  let _ = x < 5 && x > 10
}
"#,
    );
    let result = compile_check(fs);
    assert!(
        result
            .errors
            .iter()
            .any(|d| d.plain_message().contains("Type mismatch")),
        "expected the orderability type error to still be reported: {:?}",
        result.errors
    );
    assert!(
        !result
            .errors
            .iter()
            .any(|d| d.plain_message() == "Impossible comparison"),
        "must not flag a comparison on a non-orderable `uintptr`: {:?}",
        result.errors
    );
}

#[test]
fn redundant_comparison_uintptr_operand_no_diagnostic() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
fn get() -> uintptr { panic("x") }

fn main() {
  let x = get()
  let _ = x < 5 || x < 10
}
"#,
    );
    let result = compile_check(fs);
    assert!(
        result
            .errors
            .iter()
            .any(|d| d.plain_message().contains("Type mismatch")),
        "expected the orderability type error to still be reported: {:?}",
        result.errors
    );
    assert!(
        !result
            .lints
            .iter()
            .any(|d| d.plain_message() == "Redundant comparison"),
        "must not flag a comparison on a non-orderable `uintptr`: {:?}",
        result.lints
    );
}

#[test]
fn double_comparison_uintptr_operand_no_diagnostic() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
fn get() -> uintptr { panic("x") }

fn main() {
  let x = get()
  let _ = x == 5 || x < 5
}
"#,
    );
    let result = compile_check(fs);
    assert!(
        result
            .errors
            .iter()
            .any(|d| d.plain_message().contains("Type mismatch")),
        "expected the orderability type error to still be reported: {:?}",
        result.errors
    );
    assert!(
        !result
            .lints
            .iter()
            .any(|d| d.plain_message() == "Comparisons can be combined"),
        "must not flag a comparison on a non-orderable `uintptr`: {:?}",
        result.lints
    );
}

#[test]
fn bad_bit_mask_and_equality_impossible() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5
  let _ = x & 1 == 2
}
"#
    );
}

#[test]
fn bad_bit_mask_or_equality_impossible() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5
  let _ = x | 1 == 0
}
"#
    );
}

#[test]
fn bad_bit_mask_literal_on_left() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5
  let _ = 2 == x & 1
}
"#
    );
}

#[test]
fn bad_bit_mask_or_relational_always_true() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x: uint = 5
  let _ = x | 1 > 0
}
"#
    );
}

#[test]
fn bad_bit_mask_and_relational_always_false() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5
  let _ = x & 7 > 100
}
"#
    );
}

#[test]
fn bad_bit_mask_satisfiable_equality_no_diagnostic() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x = 5
  let _ = x & 1 == 1
}
"#
    );
}

#[test]
fn bad_bit_mask_mask_does_not_constrain_no_diagnostic() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x = 5
  let _ = x & 7 < 4
}
"#
    );
}

#[test]
fn bad_bit_mask_signed_or_relational_no_diagnostic() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x = 5
  let _ = x | 1 > 0
}
"#
    );
}

#[test]
fn bad_bit_mask_defers_relational_to_unsigned_comparison() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
fn main() {
  let x: uint = 5
  let _ = x | 1 >= 0
}
"#,
    );
    let result = compile_check(fs);
    assert!(
        !result
            .lints
            .iter()
            .any(|d| d.plain_message() == "Incompatible bit mask"),
        "must leave the type-bound case to unsigned_comparison: {:?}",
        result.lints
    );
    assert!(
        result
            .lints
            .iter()
            .any(|d| d.plain_message() == "Comparison is always true"),
        "expected unsigned_comparison to own this comparison: {:?}",
        result.lints
    );
}

#[test]
fn ineffective_bit_mask_less_than() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5
  let _ = x | 1 < 8
}
"#
    );
}

#[test]
fn ineffective_bit_mask_greater_than() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5
  let _ = x | 3 > 7
}
"#
    );
}

#[test]
fn ineffective_bit_mask_non_power_of_two_no_diagnostic() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x = 5
  let _ = x | 1 < 7
}
"#
    );
}

#[test]
fn equal_operands_bitand() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5
  let _ = x & x
}
"#
    );
}

#[test]
fn equal_operands_xor() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5
  let _ = x ^ x
}
"#
    );
}

#[test]
fn equal_operands_subtraction() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5
  let _ = x - x
}
"#
    );
}

#[test]
fn equal_operands_division() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5
  let _ = x / x
}
"#
    );
}

#[test]
fn equal_operands_float_no_diagnostic() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x = 1.5
  let _ = x - x
}
"#
    );
}

#[test]
fn equal_operands_distinct_no_diagnostic() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let a = 5
  let b = 6
  let _ = a - b
}
"#
    );
}

#[test]
fn equal_operands_side_effecting_no_diagnostic() {
    assert_no_lint_warnings!(
        r#"
fn f() -> int { 1 }

fn main() {
  let _ = f() - f()
}
"#
    );
}

#[test]
fn bad_bit_mask_inequality_always_true() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5
  let _ = x & 1 != 2
}
"#
    );
}

#[test]
fn bad_bit_mask_alias() {
    assert_lint_snapshot!(
        r#"
type Flags = int

fn main() {
  let a: Flags = 5
  let _ = a & 1 == 2
}
"#
    );
}

#[test]
fn bad_bit_mask_named_newtype() {
    assert_lint_snapshot!(
        r#"
struct Mask(int)

fn main() {
  let m = Mask(5)
  let _ = m & 1 == 2
}
"#
    );
}

#[test]
fn bad_bit_mask_negative_mask() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5
  let _ = x & -2 == 1
}
"#
    );
}

#[test]
fn bad_bit_mask_negative_mask_relational_no_diagnostic() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x = 5
  let _ = x & -2 < 5
}
"#
    );
}

#[test]
fn bad_bit_mask_suppressed_by_allow() {
    assert_no_lint_warnings!(
        r#"
#[allow(bad_bit_mask)]
fn main() {
  let x = 5
  let _ = x & 1 == 2
}
"#
    );
}

#[test]
fn bad_bit_mask_uintptr_operand_no_diagnostic() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
fn main() {
  let p: uintptr = 5 as uintptr
  let _ = p & 7 > 100
}
"#,
    );
    let result = compile_check(fs);
    assert!(
        result
            .errors
            .iter()
            .any(|d| d.plain_message().contains("Type mismatch")),
        "expected the uintptr bitwise/orderability type error to still be reported: {:?}",
        result.errors
    );
    assert!(
        !result
            .lints
            .iter()
            .any(|d| d.plain_message() == "Incompatible bit mask"),
        "must not flag a masked comparison on a non-orderable `uintptr`: {:?}",
        result.lints
    );
}

#[test]
fn ineffective_bit_mask_less_than_or_equal() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5
  let _ = x | 3 <= 7
}
"#
    );
}

#[test]
fn ineffective_bit_mask_greater_than_or_equal() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5
  let _ = x | 1 >= 8
}
"#
    );
}

#[test]
fn ineffective_bit_mask_literal_on_left() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5
  let _ = 8 > x | 1
}
"#
    );
}

#[test]
fn equal_operands_bitor() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5
  let _ = x | x
}
"#
    );
}

#[test]
fn equal_operands_bitand_not() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5
  let _ = x &^ x
}
"#
    );
}

#[test]
fn equal_operands_remainder() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5
  let _ = x % x
}
"#
    );
}

#[test]
fn equal_operands_alias() {
    assert_lint_snapshot!(
        r#"
type MyInt = int

fn main() {
  let a: MyInt = 5
  let _ = a - a
}
"#
    );
}

#[test]
fn equal_operands_uintptr_bitand() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let p: uintptr = 5 as uintptr
  let _ = p & p
}
"#
    );
}

#[test]
fn equal_operands_uintptr_xor() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let p: uintptr = 5 as uintptr
  let _ = p ^ p
}
"#
    );
}

#[test]
fn equal_operands_uintptr_arithmetic_no_diagnostic() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
fn main() {
  let p: uintptr = 5 as uintptr
  let _ = p - p
}
"#,
    );
    let result = compile_check(fs);
    assert!(
        result
            .errors
            .iter()
            .any(|d| d.plain_message().contains("Type mismatch")),
        "expected the uintptr arithmetic type error to still be reported: {:?}",
        result.errors
    );
    assert!(
        !result
            .lints
            .iter()
            .any(|d| d.plain_message() == "Equal operands"),
        "must not flag arithmetic equal operands on a non-arithmetic `uintptr`: {:?}",
        result.lints
    );
}

#[test]
fn equal_operands_uintptr_alias_bitwise_no_diagnostic() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
type Handle = uintptr

fn main() {
  let h: Handle = 5 as Handle
  let _ = h & h
}
"#,
    );
    let result = compile_check(fs);
    assert!(
        result
            .errors
            .iter()
            .any(|d| d.plain_message().contains("Type mismatch")),
        "expected the uintptr-alias bitwise type error to still be reported: {:?}",
        result.errors
    );
    assert!(
        !result
            .lints
            .iter()
            .any(|d| d.plain_message() == "Equal operands"),
        "must not flag bitwise equal operands on a `uintptr`-backed alias: {:?}",
        result.lints
    );
}

#[test]
fn equal_operands_uintptr_newtype_bitwise_no_diagnostic() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
struct Wrapped(uintptr)

fn main() {
  let w = Wrapped(5 as uintptr)
  let _ = w & w
}
"#,
    );
    let result = compile_check(fs);
    assert!(
        result
            .errors
            .iter()
            .any(|d| d.plain_message().contains("Type mismatch")),
        "expected the uintptr-newtype bitwise type error to still be reported: {:?}",
        result.errors
    );
    assert!(
        !result
            .lints
            .iter()
            .any(|d| d.plain_message() == "Equal operands"),
        "must not flag bitwise equal operands on a `uintptr`-backed newtype: {:?}",
        result.lints
    );
}

#[test]
fn goos_comparison_invalid_equal() {
    assert_lint_snapshot!(
        r#"
import "go:runtime"
fn main() {
  let _ = runtime.GOOS == "windows10"
}
"#
    );
}

#[test]
fn goos_comparison_invalid_not_equal() {
    assert_lint_snapshot!(
        r#"
import "go:runtime"
fn main() {
  let _ = runtime.GOOS != "frobnix"
}
"#
    );
}

#[test]
fn goarch_comparison_invalid_equal() {
    assert_lint_snapshot!(
        r#"
import "go:runtime"
fn main() {
  let _ = runtime.GOARCH == "x86"
}
"#
    );
}

#[test]
fn goos_comparison_literal_on_left() {
    assert_lint_snapshot!(
        r#"
import "go:runtime"
fn main() {
  let _ = "windows10" == runtime.GOOS
}
"#
    );
}

#[test]
fn goos_comparison_alias_import() {
    assert_lint_snapshot!(
        r#"
import r "go:runtime"
fn main() {
  let _ = r.GOOS == "win"
}
"#
    );
}

#[test]
fn goos_comparison_valid_value_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:runtime"
fn main() {
  let _ = runtime.GOOS == "linux"
}
"#
    );
}

#[test]
fn goarch_comparison_valid_value_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:runtime"
fn main() {
  let _ = runtime.GOARCH == "amd64"
}
"#
    );
}

#[test]
fn goos_comparison_ordering_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:runtime"
fn main() {
  let _ = runtime.GOOS < "frobnix"
}
"#
    );
}

#[test]
fn double_bool_negation() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = true;
  let _ = !!x
}
"#
    );
}

#[test]
fn double_int_negation() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5;
  let _ = --x
}
"#
    );
}

#[test]
fn double_bool_negation_with_parens() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = true;
  let _ = !(!x)
}
"#
    );
}

#[test]
fn duplicate_logical_operand_and() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let a = 5;
  let b = 10;
  let _ = a > b && a > b
}
"#
    );
}

#[test]
fn duplicate_logical_operand_or() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let a = 5;
  let b = 10;
  let _ = a == b || a == b
}
"#
    );
}

#[test]
fn duplicate_logical_operand_with_side_effects_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn side_effect() -> bool { true }

fn main() {
  let _ = side_effect() && side_effect()
}
"#
    );
}

#[test]
fn negated_logical_operand_and() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let a = true;
  let _ = a && !a
}
"#
    );
}

#[test]
fn negated_logical_operand_or() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let a = true;
  let _ = a || !a
}
"#
    );
}

#[test]
fn negated_logical_operand_field_access() {
    assert_lint_snapshot!(
        r#"
struct Flag { ready: bool }

fn main() {
  let f = Flag { ready: true };
  let _ = f.ready && !f.ready
}
"#
    );
}

#[test]
fn negated_logical_operand_distinct_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let a = true;
  let b = false;
  let _ = a && !b
}
"#
    );
}

#[test]
fn negated_logical_operand_with_side_effects_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn side_effect() -> bool { true }

fn main() {
  let _ = side_effect() && !side_effect()
}
"#
    );
}

#[test]
fn bool_literal_comparison_eq_true() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = true;
  let _ = x == true
}
"#
    );
}

#[test]
fn bool_literal_comparison_eq_false() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = true;
  let _ = x == false
}
"#
    );
}

#[test]
fn bool_literal_comparison_ne_true() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = true;
  let _ = x != true
}
"#
    );
}

#[test]
fn identical_if_branches() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let a = 5;
  let b = 10;
  let x = if a > b { 42 } else { 42 };
  let _ = x
}
"#
    );
}

#[test]
fn identical_if_branches_else_if_chain_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let a = 5;
  let b = 10;
  let x = if a > b { 1 } else if a < b { 2 } else { 3 };
  let _ = x
}
"#
    );
}

#[test]
fn collapsible_if() {
    assert_lint_snapshot!(
        r#"
pub fn check(a: bool, b: bool) {
  let mut count = 0
  if a {
    if b {
      count += 1
    }
  }
  let _ = count
}
"#
    );
}

#[test]
fn collapsible_if_outer_else_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn check(a: bool, b: bool) {
  let mut count = 0
  if a {
    if b {
      count += 1
    }
  } else {
    count += 2
  }
  let _ = count
}
"#
    );
}

#[test]
fn collapsible_if_inner_else_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn check(a: bool, b: bool) {
  let mut count = 0
  if a {
    if b {
      count += 1
    } else {
      count += 2
    }
  }
  let _ = count
}
"#
    );
}

#[test]
fn collapsible_if_extra_statement_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn check(a: bool, b: bool) {
  let mut count = 0
  if a {
    count += 5
    if b {
      count += 1
    }
  }
  let _ = count
}
"#
    );
}

#[test]
fn collapsible_if_inner_if_let_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn check(a: bool, x: Option<int>) {
  let mut count = 0
  if a {
    if let Some(v) = x {
      count += v
    }
  }
  let _ = count
}
"#
    );
}

#[test]
fn collapsible_if_named_bool_condition_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub struct Flag(bool)

pub fn check(a: Flag, b: bool) {
  let mut count = 0
  if a {
    if b {
      count += 1
    }
  }
  let _ = count
}
"#
    );
}

#[test]
fn redundant_else() {
    assert_lint_snapshot!(
        r#"
pub fn check(c: bool) -> int {
  let mut count = 0
  if c {
    return 0
  } else {
    count += 1
  }
  count
}
"#
    );
}

#[test]
fn redundant_else_chain() {
    assert_lint_snapshot!(
        r#"
pub fn classify(c: bool, d: bool) -> int {
  let mut count = 0
  if c {
    return 0
  } else if d {
    count += 1
  }
  count
}
"#
    );
}

#[test]
fn redundant_else_continue() {
    assert_lint_snapshot!(
        r#"
pub fn scan(xs: Slice<int>) -> int {
  let mut total = 0
  for x in xs {
    if x < 0 {
      continue
    } else {
      total += x
    }
    total += 1
  }
  total
}
"#
    );
}

#[test]
fn redundant_else_no_else_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn check(c: bool) -> int {
  let mut count = 0
  if c {
    return 0
  }
  count += 1
  count
}
"#
    );
}

#[test]
fn redundant_else_non_diverging_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn check(c: bool) -> int {
  let mut count = 0
  if c {
    count += 1
  } else {
    count += 2
  }
  count
}
"#
    );
}

#[test]
fn redundant_else_value_position_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn check(c: bool) -> int {
  let x = if c { return 0 } else { 5 }
  x + 1
}
"#
    );
}

#[test]
fn redundant_else_empty_else_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn check(c: bool) -> int {
  let mut count = 0
  if c {
    return 0
  } else {
  }
  count += 1
  count
}
"#
    );
}

#[test]
fn redundant_else_tail_position_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn check(xs: Slice<int>) -> int {
  let mut total = 0
  for x in xs {
    if x < 0 {
      break
    } else {
      total += x
    }
  }
  total
}
"#
    );
}

#[test]
fn redundant_else_if_let_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn check(opt: Option<int>) -> int {
  let mut count = 0
  if let Some(v) = opt {
    return v
  } else {
    count += 1
  }
  count
}
"#
    );
}

#[test]
fn collapsible_else_if() {
    assert_lint_snapshot!(
        r#"
pub fn check(c: bool, d: bool) -> int {
  if c {
    1
  } else {
    if d {
      2
    } else {
      3
    }
  }
}
"#
    );
}

#[test]
fn collapsible_else_if_no_inner_else() {
    assert_lint_snapshot!(
        r#"
pub fn check(c: bool, d: bool) {
  let mut count = 0
  if c {
    count += 1
  } else {
    if d {
      count += 2
    }
  }
  let _ = count
}
"#
    );
}

#[test]
fn collapsible_else_if_already_chained_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn check(c: bool, d: bool) -> int {
  if c {
    1
  } else if d {
    2
  } else {
    3
  }
}
"#
    );
}

#[test]
fn collapsible_else_if_extra_statement_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn check(c: bool, d: bool) {
  let mut count = 0
  if c {
    count += 1
  } else {
    count += 9
    if d {
      count += 2
    }
  }
  let _ = count
}
"#
    );
}

#[test]
fn collapsible_else_if_diverging_consequence_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn check(xs: Slice<int>, d: bool) {
  for x in xs {
    if x < 0 {
      continue
    } else {
      if d {
        let _ = x
      }
    }
  }
}
"#
    );
}

#[test]
fn collapsible_else_if_inner_if_let_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn check(c: bool, o: Option<int>) {
  let mut count = 0
  if c {
    count += 1
  } else {
    if let Some(v) = o {
      count += v
    }
  }
  let _ = count
}
"#
    );
}

#[test]
fn identical_match_arms_literals() {
    assert_lint_snapshot!(
        r#"
pub fn pick(n: int) -> int {
  let a = 1;
  let b = 2;
  match n {
    0 => a + b,
    1 => a + b,
    _ => a + b,
  }
}
"#
    );
}

#[test]
fn identical_match_arms_enum_variants() {
    assert_lint_snapshot!(
        r#"
pub enum Signal { Red, Yellow, Green }

pub fn stop(s: Signal) -> int {
  let halt = 1;
  match s {
    Signal.Red => halt,
    Signal.Yellow => halt,
    Signal.Green => halt,
  }
}
"#
    );
}

#[test]
fn differing_match_arms_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn pick(n: int) -> int {
  let a = 1;
  let b = 2;
  match n {
    0 => a + b,
    _ => a - b,
  }
}
"#
    );
}

#[test]
fn identical_match_arms_with_binding_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn pick(opt: Option<int>) -> int {
  match opt {
    Some(v) => 42,
    None => 42,
  }
}
"#
    );
}

#[test]
fn identical_match_arms_with_guard_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn pick(n: int) -> int {
  let a = 1;
  let b = 2;
  match n {
    0 if a > 0 => a + b,
    _ => a + b,
  }
}
"#
    );
}

#[test]
fn identical_match_arms_dividing_subject_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn pick(n: int) -> int {
  match 10 / n {
    0 => 42,
    _ => 42,
  }
}
"#
    );
}

#[test]
fn identical_match_arms_shifting_subject_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn pick(n: int) -> int {
  match 1 << n {
    0 => 42,
    _ => 42,
  }
}
"#
    );
}

#[test]
fn identical_match_arms_interpolated_subject_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn side() -> string { "x" }

pub fn pick() -> int {
  match f"a{side()}" {
    "ax" => 42,
    _ => 42,
  }
}
"#
    );
}

#[test]
fn unnecessary_bool_true_false() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let a = 5;
  let b = 10;
  let x = if a > b { true } else { false };
  let _ = x
}
"#
    );
}

#[test]
fn unnecessary_bool_false_true() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let a = 5;
  let b = 10;
  let x = if a > b { false } else { true };
  let _ = x
}
"#
    );
}

#[test]
fn unnecessary_bool_non_bool_branches_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let a = 5;
  let b = 10;
  let x = if a > b { 1 } else { 0 };
  let _ = x
}
"#
    );
}

#[test]
fn unnecessary_bool_branch_not_literal_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let a = 5;
  let b = 10;
  let x = if a > b { true } else { a < b };
  let _ = x
}
"#
    );
}

#[test]
fn repeated_if_condition_simple() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5;
  let _ = if x > 0 { 1 } else if x > 0 { 2 } else { 3 }
}
"#
    );
}

#[test]
fn repeated_if_condition_with_parens() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5;
  let _ = if x > (0) { 1 } else if x > 0 { 2 } else { 3 }
}
"#
    );
}

#[test]
fn repeated_if_condition_identifier() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let flag = true;
  let _ = if flag { 1 } else if flag { 2 } else { 3 }
}
"#
    );
}

#[test]
fn repeated_if_condition_dot_access() {
    assert_lint_snapshot!(
        r#"
struct Config { enabled: bool }

fn main() {
  let c = Config { enabled: true };
  let _ = if c.enabled { 1 } else if c.enabled { 2 } else { 3 }
}
"#
    );
}

#[test]
fn repeated_if_condition_distinct_conditions_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x = 5;
  let _ = if x > 0 { 1 } else if x < 0 { 2 } else { 3 }
}
"#
    );
}

#[test]
fn repeated_if_condition_with_side_effects_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn side_effect() -> bool { true }

fn main() {
  let _ = if side_effect() { 1 } else if side_effect() { 2 } else { 3 }
}
"#
    );
}

#[test]
fn repeated_if_condition_simple_if_else_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x = 5;
  let _ = if x > 0 { 1 } else { 2 }
}
"#
    );
}

#[test]
fn unchanging_loop_condition_comparison() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let n = 5;
  while n < 10 {
    let _ = n
  }
}
"#
    );
}

#[test]
fn unchanging_loop_condition_negated_flag() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let done = false;
  while !done {
    let _ = done
  }
}
"#
    );
}

#[test]
fn unchanging_loop_condition_mutated_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let mut i = 0;
  while i < 10 {
    i += 1
  }
}
"#
    );
}

#[test]
fn unchanging_loop_condition_partial_mutation_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let mut a = 0;
  let b = 5;
  while a < b {
    a += 1
  }
}
"#
    );
}

#[test]
fn unchanging_loop_condition_break_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let n = 5;
  while n < 10 {
    if n > 7 {
      break
    }
    let _ = n
  }
}
"#
    );
}

#[test]
fn unchanging_loop_condition_return_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let n = 5;
  while n < 10 {
    if n > 7 {
      return
    }
    let _ = n
  }
}
"#
    );
}

#[test]
fn unchanging_loop_condition_call_in_condition_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn check() -> bool { true }

fn main() {
  while check() {
    let _ = 1
  }
}
"#
    );
}

#[test]
fn unchanging_loop_condition_literal_condition_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  while true {
    let _ = 1
  }
}
"#
    );
}

#[test]
fn unchanging_loop_condition_try_scoped_propagate() {
    assert_lint_snapshot!(
        r#"
fn may_fail() -> Result<int, string> { Ok(1) }

fn main() {
  let n = 5;
  while n < 10 {
    let _ = try { may_fail()? }
  }
}
"#
    );
}

#[test]
fn unchanging_loop_condition_function_propagate_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn may_fail() -> Result<int, string> { Ok(1) }

fn run() -> Result<int, string> {
  let n = 5;
  while n < 10 {
    let _ = may_fail()?
  }
  Ok(0)
}

fn main() {
  let _ = run()
}
"#
    );
}

#[test]
fn unchanging_loop_condition_diverging_call_in_try_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn may_fail() -> Result<int, string> { Ok(1) }
fn diverge() -> Never { panic("x") }

fn run() {
  let n = 5;
  while n < 10 {
    let _ = try {
      let _ = may_fail()?
      diverge()
    }
  }
}

fn main() {
  run()
}
"#
    );
}

#[test]
fn unchanging_loop_condition_deref_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let mut x = 0;
  let r = &x;
  while r.* < 10 {
    x += 1
  }
}
"#
    );
}

#[test]
fn unchanging_loop_condition_task_return() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let n = 5;
  while n < 10 {
    task { return }
  }
}
"#
    );
}

#[test]
fn loop_runs_once_loop_break() {
    assert_lint_snapshot!(
        r#"
fn main() {
  loop {
    let _ = 1
    break
  }
}
"#
    );
}

#[test]
fn loop_runs_once_for_return() {
    assert_lint_snapshot!(
        r#"
fn main() {
  for _ in 0..10 {
    return
  }
}
"#
    );
}

#[test]
fn loop_runs_once_while_return() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let flag = true
  while flag {
    return
  }
}
"#
    );
}

#[test]
fn regexp_in_loop_for() {
    assert_lint_snapshot!(
        r#"
import "go:regexp"

fn main() {
  for s in ["a", "b"] {
    let _ = regexp.MatchString("[0-9]+", s)
  }
}
"#
    );
}

#[test]
fn regexp_in_loop_nested_in_if() {
    assert_lint_snapshot!(
        r#"
import "go:regexp"

fn main() {
  for s in ["a", "b"] {
    if s != "" {
      let _ = regexp.MatchString("a", s)
    }
  }
}
"#
    );
}

#[test]
fn regexp_in_loop_while_condition() {
    assert_lint_snapshot!(
        r#"
import "go:regexp"

fn main() {
  let s = "abc"
  while regexp.MatchString("[0-9]+", s).unwrap_or(false) {
    let _ = 1
  }
}
"#
    );
}

#[test]
fn regexp_in_loop_nested_for_iterable() {
    let warnings = crate::_harness::lint::lint(
        r#"
import "go:regexp"

fn pick(b: bool) -> Slice<string> {
  if b { ["x"] } else { [] }
}

fn main() {
  for o in ["a"] {
    for y in pick(regexp.MatchString("[0-9]+", o).unwrap_or(false)) {
      let _ = y
    }
  }
}
"#,
    );
    let fires = warnings
        .iter()
        .any(|w| w.code_str() == Some("lint.regexp_in_loop"));
    assert!(
        fires,
        "expected regexp_in_loop on a pattern compiled in a nested for iterable, got: {:?}",
        warnings
    );
}

#[test]
fn regexp_in_loop_top_level_for_iterable_no_warning() {
    let warnings = crate::_harness::lint::lint(
        r#"
import "go:regexp"

fn pick(b: bool) -> Slice<string> {
  if b { ["x"] } else { [] }
}

fn main() {
  for y in pick(regexp.MatchString("[0-9]+", "abc").unwrap_or(false)) {
    let _ = y
  }
}
"#,
    );
    let fires = warnings
        .iter()
        .any(|w| w.code_str() == Some("lint.regexp_in_loop"));
    assert!(
        !fires,
        "a for iterable runs once on entry, so a top-level for iterable must not warn, got: {:?}",
        warnings
    );
}

#[test]
fn regexp_in_loop_non_literal_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:regexp"

fn main() {
  let pat = "[0-9]+"
  for s in ["a", "b"] {
    let _ = regexp.MatchString(pat, s)
  }
}
"#
    );
}

#[test]
fn regexp_in_loop_hoisted_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:regexp"

fn main() {
  let _ = regexp.MatchString("[0-9]+", "seed")
  for s in ["a", "b"] {
    let _ = s
  }
}
"#
    );
}

#[test]
fn regexp_in_loop_allow_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:regexp"

#[allow(regexp_in_loop)]
fn main() {
  for s in ["a", "b"] {
    let _ = regexp.MatchString("[0-9]+", s)
  }
}
"#
    );
}

#[test]
fn loop_runs_once_if_else_both_exit() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let flag = true
  loop {
    if flag {
      break
    } else {
      return
    }
  }
}
"#
    );
}

#[test]
fn loop_runs_once_diverging_call() {
    assert_lint_snapshot!(
        r#"
fn main() {
  loop {
    panic("stop")
  }
}
"#
    );
}

#[test]
fn loop_runs_once_conditional_break_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  for i in 0..10 {
    if i > 5 {
      break
    }
  }
}
"#
    );
}

#[test]
fn loop_runs_once_continue_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  for i in 0..10 {
    if i > 5 {
      continue
    }
    let _ = i
  }
}
"#
    );
}

#[test]
fn loop_runs_once_continue_then_break_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let mut i = 0
  loop {
    i += 1
    if i < 10 {
      continue
    }
    break
  }
}
"#
    );
}

#[test]
fn loop_runs_once_continue_in_return_value_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn pick(n: int) -> int {
  let mut i = 0
  loop {
    i += 1
    return if i < n { continue } else { 42 }
  }
}

fn main() {
  let _ = pick(5)
}
"#
    );
}

#[test]
fn loop_runs_once_continue_in_break_value_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let mut i = 0
  let _x = loop {
    i += 1
    break if i < 10 { continue } else { 42 }
  }
}
"#
    );
}

#[test]
fn loop_runs_once_continue_in_nested_for_iterable_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let mut i = 0
  loop {
    i += 1
    for _ in if i < 10 { continue } else { 0..1 } {
      let _ = i
    }
    break
  }
}
"#
    );
}

#[test]
fn loop_runs_once_normal_loop_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  for i in 0..10 {
    let _ = i
  }
}
"#
    );
}

#[test]
fn loop_runs_once_return_in_lambda_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  for i in 0..10 {
    let _f = || { return i }
    let _ = i
  }
}
"#
    );
}

#[test]
fn unused_function() {
    assert_lint_snapshot!(
        r#"
fn unused_helper() -> int {
  42
}

fn main() {
  ()
}
"#
    );
}

#[test]
fn used_function_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn helper() -> int {
  42
}

fn main() {
  let _ = helper()
}
"#
    );
}

#[test]
fn unused_struct() {
    assert_lint_snapshot!(
        r#"
struct UnusedPoint {
  x: int,
  y: int,
}

fn main() {
  ()
}
"#
    );
}

#[test]
fn used_struct_all_fields_read_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Point {
  x: int,
  y: int,
}

fn main() {
  let p = Point { x: 1, y: 2 };
  let _ = p.x + p.y
}
"#
    );
}

#[test]
fn zero_fill_through_alias_does_not_warn_unused_fields() {
    assert_no_lint_warnings!(
        r#"
struct Inner { x: int, y: int }
type Alias = Inner

fn main() {
  let a = Alias { .. }
  let _ = a.x + a.y
}
"#
    );
}

#[test]
fn unused_enum() {
    assert_lint_snapshot!(
        r#"
enum UnusedColor {
  Red,
  Green,
  Blue,
}

fn main() {
  ()
}
"#
    );
}

#[test]
fn used_enum_all_variants_used_no_warning() {
    assert_no_lint_warnings!(
        r#"
enum Color {
  Red,
  Green,
}

fn main() {
  let c = Color.Red;
  let _ = match c {
    Color.Red => 1,
    Color.Green => 2,
  }
}
"#
    );
}

#[test]
fn unused_constant() {
    assert_lint_snapshot!(
        r#"
const UNUSED_VALUE = 42

fn main() {
  ()
}
"#
    );
}

#[test]
fn used_constant_no_warning() {
    assert_no_lint_warnings!(
        r#"
const VALUE = 42

fn main() {
  let _ = VALUE
}
"#
    );
}

#[test]
fn public_function_not_unused() {
    assert_no_lint_warnings!(
        r#"
pub fn public_helper() -> int {
  42
}

fn main() {
  ()
}
"#
    );
}

#[test]
fn public_struct_not_unused() {
    assert_no_lint_warnings!(
        r#"
pub struct PublicPoint {
  x: int,
  y: int,
}

fn main() {
  ()
}
"#
    );
}

#[test]
fn function_reachable_through_chain() {
    assert_no_lint_warnings!(
        r#"
fn helper1() -> int {
  42
}

fn helper2() -> int {
  helper1()
}

fn main() {
  let _ = helper2()
}
"#
    );
}

#[test]
fn struct_used_in_signature() {
    assert_no_lint_warnings!(
        r#"
pub struct Point {
  x: int,
  y: int,
}

fn create_point() -> Point {
  Point { x: 1, y: 2 }
}

fn main() {
  let _ = create_point()
}
"#
    );
}

#[test]
fn struct_used_in_parameter() {
    assert_no_lint_warnings!(
        r#"
pub struct Point {
  x: int,
  y: int,
}

fn get_x(p: Point) -> int {
  p.x
}

fn main() {
  let _ = get_x(Point { x: 1, y: 2 })
}
"#
    );
}

#[test]
fn internal_type_leak() {
    assert_lint_snapshot!(
        r#"
struct PrivateData {
  _value: int,
}

pub fn leaky_function() -> PrivateData {
  PrivateData { _value: 42 }
}

fn main() {
  ()
}
"#
    );
}

#[test]
fn public_type_in_public_signature_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub struct PublicData {
  value: int,
}

pub fn public_function() -> PublicData {
  PublicData { value: 42 }
}

fn main() {
  ()
}
"#
    );
}

#[test]
fn internal_type_leak_in_tuple_return() {
    assert_lint_snapshot!(
        r#"
struct PrivateA {
  _a: int,
}

struct PrivateB {
  _b: int,
}

pub fn get_pair() -> (PrivateA, PrivateB) {
  (PrivateA { _a: 1 }, PrivateB { _b: 2 })
}

fn main() {
  ()
}
"#
    );
}

#[test]
fn internal_type_leak_in_higher_order_function() {
    assert_lint_snapshot!(
        r#"
struct PrivateOutput {
  _result: int,
}

pub fn make_handler(seed: int) -> fn() -> PrivateOutput {
  || PrivateOutput { _result: seed }
}

fn main() {
  ()
}
"#
    );
}

#[test]
fn unused_import() {
    assert_lint_snapshot!(
        r#"
import "some/module"

fn main() {
  ()
}
"#
    );
}

#[test]
fn enum_struct_variant_constructor_no_warning() {
    assert_no_lint_warnings!(
        r#"
enum Shape {
  Circle { radius: float64 },
}

fn make_circle() -> Shape {
    Shape.Circle { radius: 5.0 }
}

fn main() {
    let _s = make_circle()
}
"#
    );
}

#[test]
fn unused_struct_field() {
    assert_lint_snapshot!(
        r#"
struct Data {
  used_field: int,
  unused_field: int,
}

fn make_data() -> Data {
  Data { used_field: 1, unused_field: 2 }
}

fn main() {
  let d = make_data();
  let _ = d.used_field
}
"#
    );
}

#[test]
fn used_struct_field_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Point {
  x: int,
  y: int,
}

fn main() {
  let p = Point { x: 1, y: 2 };
  let _ = p.x + p.y
}
"#
    );
}

#[test]
fn public_struct_fields_not_unused() {
    assert_no_lint_warnings!(
        r#"
pub struct Point {
  x: int,
  y: int,
}

fn main() {
  ()
}
"#
    );
}

#[test]
fn public_field_on_private_struct_not_unused() {
    assert_no_lint_warnings!(
        r#"
struct Person {
  pub name: string,
  pub age: int,
}

fn main() {
  let _p = Person { name: "", age: 0 }
}
"#
    );
}

#[test]
fn serialization_struct_fields_not_unused() {
    assert_no_lint_warnings!(
        r#"
#[json]
struct Response {
  status: string,
  code: int,
}

fn main() {
  let _r = Response { status: "ok", code: 200 }
}
"#
    );
}

#[test]
fn display_struct_fields_not_unused() {
    assert_no_lint_warnings!(
        r#"
#[display]
struct Point {
  x: int,
  y: int,
}

fn main() {
  let _p = Point { x: 1, y: 2 }
}
"#
    );
}

#[test]
fn field_with_tag_attribute_not_unused() {
    assert_no_lint_warnings!(
        r#"
struct User {
  #[tag(`validate:"required,email"`)]
  email: string,
  #[tag(`validate:"gte=0"`)]
  age: int,
}

fn main() {
  let _u = User { email: "a@b.c", age: 1 }
}
"#
    );
}

#[test]
fn struct_field_used_in_pattern() {
    assert_no_lint_warnings!(
        r#"
struct Point {
  x: int,
  y: int,
}

fn main() {
  let p = Point { x: 1, y: 2 };
  let _ = match p {
    Point { x, y } => x + y,
  }
}
"#
    );
}

#[test]
fn struct_field_used_in_match_subject() {
    assert_no_lint_warnings!(
        r#"
struct Container {
  value: int,
}

fn main() {
  let c = Container { value: 42 };
  let _ = match c.value {
    _ => 0,
  }
}
"#
    );
}

#[test]
fn struct_field_with_option_used_in_match_subject() {
    assert_no_lint_warnings!(
        r#"
struct Container {
  value: Option<int>,
}

fn main() {
  let c = Container { value: Some(42) };
  match c.value {
    Some(n) => { let _ = n + 1; },
    None => { let _ = 0; },
  }
}
"#
    );
}

#[test]
fn struct_field_suppressed_by_underscore() {
    assert_no_lint_warnings!(
        r#"
struct Data {
  used: int,
  _unused: int,
}

fn main() {
  let d = Data { used: 1, _unused: 2 };
  let _ = d.used
}
"#
    );
}

#[test]
fn struct_field_used_via_spread_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Config {
  debug: bool,
  verbose: bool,
  port: int,
}

fn main() {
  let base = Config { debug: true, verbose: true, port: 8080 };
  let dev = Config { debug: true, ..base };
  let _ = if dev.debug { dev.port } else { 0 }
}
"#
    );
}

#[test]
fn struct_field_used_via_type_alias_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Point {
  x: int,
  y: int,
}

type MyPoint = Point

fn read(p: MyPoint) -> int {
  p.x + p.y
}

fn main() {
  let _ = read(Point { x: 1, y: 2 })
}
"#
    );
}

#[test]
fn unused_enum_variant() {
    assert_lint_snapshot!(
        r#"
enum Color {
  Red,
  Unused,
}

fn main() {
  let _ = Color.Red
}
"#
    );
}

#[test]
fn used_enum_variant_no_warning() {
    assert_no_lint_warnings!(
        r#"
enum Color {
  Red,
  Green,
}

fn main() {
  let c = Color.Red;
  let _ = match c {
    Color.Red => 1,
    Color.Green => 2,
  }
}
"#
    );
}

#[test]
fn public_enum_variants_not_unused() {
    assert_no_lint_warnings!(
        r#"
pub enum Color {
  Red,
  Green,
  Blue,
}

fn main() {
  ()
}
"#
    );
}

#[test]
fn enum_variant_used_in_pattern() {
    assert_no_lint_warnings!(
        r#"
enum Status {
  Active(int),
  Inactive,
}

fn main() {
  let s = Status.Active(42);
  let _ = match s {
    Status.Active(x) => x,
    Status.Inactive => 0,
  }
}
"#
    );
}

#[test]
fn enum_variant_used_in_pattern_unqualified() {
    assert_no_lint_warnings!(
        r#"
enum Color {
  Red,
  Green,
  Blue,
}

fn main() {
  let c = Color.Blue;
  let _ = match c {
    Red => 1,
    Green => 2,
    Blue => 3,
  }
}
"#
    );
}

#[test]
fn match_on_literal_slice() {
    assert_lint_snapshot!(
        r#"
fn main() {
  match [1, 2, 3] {
    _ => (),
  }
}
"#
    );
}

#[test]
fn match_on_literal_tuple() {
    assert_lint_snapshot!(
        r#"
fn main() {
  match (1, 2) {
    _ => (),
  }
}
"#
    );
}

#[test]
fn match_on_nested_paren_literal_tuple() {
    assert_lint_snapshot!(
        r#"
fn main() {
  match (((1, 2))) {
    _ => (),
  }
}
"#
    );
}

#[test]
fn match_on_variable_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3];
  match xs {
    _ => (),
  }
}
"#
    );
}

#[test]
fn match_on_tuple_of_variables_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let a = Some(1);
  let b = Some(2);
  let _ = match (a, b) {
    (Some(x), Some(y)) => x + y,
    _ => 0,
  }
}
"#
    );
}

#[test]
fn dead_code_after_return() {
    assert_lint_snapshot!(
        r#"
pub fn foo() -> int {
  return 42;
  let x = 1;
  x
}
"#
    );
}

#[test]
fn no_dead_code_when_return_is_last() {
    assert_no_lint_warnings!(
        r#"
pub fn foo() {
  return
}
"#
    );
}

#[test]
fn dead_code_after_break() {
    assert_lint_snapshot!(
        r#"
fn main() {
  loop {
    break;
    let x = 1;
    x
  }
}
"#
    );
}

#[test]
fn no_dead_code_when_break_is_last() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let cond = true
  loop {
    if cond {
      break
    }
  }
}
"#
    );
}

#[test]
fn dead_code_after_continue() {
    assert_lint_snapshot!(
        r#"
fn main() {
  loop {
    continue;
    let x = 1;
    x
  }
}
"#
    );
}

#[test]
fn no_dead_code_when_continue_is_last() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  loop {
    continue
  }
}
"#
    );
}

#[test]
fn dead_code_after_diverging_if_else() {
    assert_lint_snapshot!(
        r#"
pub fn foo() -> int {
  if true {
    return 1
  } else {
    return 2
  };
  let x = 3;
  x
}
"#
    );
}

#[test]
fn no_dead_code_when_only_one_branch_returns() {
    assert_no_lint_warnings!(
        r#"
pub fn foo() {
  if true {
    return
  } else {
    ()
  }
}
"#
    );
}

#[test]
fn dead_code_after_diverging_match() {
    assert_lint_snapshot!(
        r#"
pub fn foo(x: int) -> int {
  match x {
    0 => return 0,
    _ => return 1,
  };
  let y = 2;
  y
}
"#
    );
}

#[test]
fn no_dead_code_when_not_all_match_arms_diverge() {
    assert_no_lint_warnings!(
        r#"
pub fn foo(x: int) {
  match x {
    0 => { return },
    _ => (),
  }
}
"#
    );
}

#[test]
fn dead_code_after_diverging_nested_block() {
    assert_lint_snapshot!(
        r#"
pub fn foo() -> int {
  { return 1 };
  let x = 2;
  x
}
"#
    );
}

#[test]
fn no_dead_code_after_loop_with_break() {
    assert_no_lint_warnings!(
        r#"
pub fn foo(cond: bool) -> int {
  loop { if cond { break } };
  42
}
"#
    );
}

#[test]
fn no_dead_code_after_while_with_break() {
    assert_no_lint_warnings!(
        r#"
pub fn foo(cond: bool) -> int {
  while true { if cond { break } };
  42
}
"#
    );
}

#[test]
fn no_dead_code_after_closure_with_return() {
    assert_no_lint_warnings!(
        r#"
pub fn foo() -> int {
  let _f = || { return 1 };
  42
}
"#
    );
}

#[test]
fn no_dead_code_when_if_has_no_else() {
    assert_no_lint_warnings!(
        r#"
pub fn foo(cond: bool) -> int {
  if cond { return 1 };
  42
}
"#
    );
}

#[test]
fn dead_code_after_infinite_loop() {
    assert_lint_snapshot!(
        r#"
fn main() {
  loop {
    ()
  };
  let x = 1;
  x
}
"#
    );
}

#[test]
fn no_dead_code_after_loop_with_conditional_break() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  loop {
    if true {
      break
    } else {
      ()
    }
  };
  ()
}
"#
    );
}

#[test]
fn no_dead_code_after_loop_with_nested_break() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  loop {
    match 1 {
      1 => break,
      _ => (),
    }
  };
  ()
}
"#
    );
}

#[test]
fn dead_code_after_diverging_call() {
    assert_lint_snapshot!(
        r#"
fn diverge() -> Never {
  loop { () }
}

fn main() {
  diverge();
  let x = 1;
  x
}
"#
    );
}

#[test]
fn no_dead_code_after_normal_call() {
    assert_no_lint_warnings!(
        r#"
pub fn normal() {
  ()
}

fn main() {
  normal();
  ()
}
"#
    );
}

#[test]
fn interface_references_used_type() {
    assert_no_lint_warnings!(
        r#"
pub struct Data {
  value: int,
}

pub interface Container {
  fn get() -> Data;
  fn set(_d: Data);
}

fn main() {
  ()
}
"#
    );
}

#[test]
fn unused_interface_warning() {
    assert_lint_snapshot!(
        r#"
interface Processor {
  fn process(_x: int);
}

fn main() {
  ()
}
"#
    );
}

#[test]
fn interface_used_in_embedding_not_unused() {
    assert_no_lint_warnings!(
        r#"
interface HasName {
  fn name(self) -> string
}

pub interface Person {
  embed HasName
}

fn main() {
  ()
}
"#
    );
}

#[test]
fn interface_self_param_no_unused_warning() {
    assert_no_lint_warnings!(
        r#"
pub interface Greetable {
  fn greet(self) -> string;
  fn update(self, value: int);
}

fn main() {
  ()
}
"#
    );
}

#[test]
fn for_loop_uses_function() {
    assert_no_lint_warnings!(
        r#"
fn get_items() -> Slice<int> {
  [1, 2, 3]
}

fn main() {
  for item in get_items() {
    let _x = item + 1;
  }
}
"#
    );
}

#[test]
fn for_loop_uses_struct() {
    assert_no_lint_warnings!(
        r#"
struct Item {
  value: int,
}

fn main() {
  let items = [Item { value: 1 }, Item { value: 2 }];
  for item in items {
    let _x = item.value;
  }
}
"#
    );
}

#[test]
fn select_uses_channel_type() {
    assert_no_lint_warnings!(
        r#"
pub struct Data {
  value: int,
}

fn main() {
  let ch = Channel.new<Data>();
  let _ = select {
    let Some(d) = ch.receive() => d.value,
    _ => 0,
  }
}
"#
    );
}

#[test]
fn type_alias_uses_struct() {
    assert_no_lint_warnings!(
        r#"
pub struct Point {
  x: int,
  y: int,
}

pub type Location = Point

fn main() {
  let loc: Location = Point { x: 1, y: 2 };
  let _ = loc.x
}
"#
    );
}

#[test]
fn unused_type_alias_warning() {
    assert_lint_snapshot!(
        r#"
type Unused = int

fn main() {
  ()
}
"#
    );
}

#[test]
fn type_alias_used_in_parameter_no_warning() {
    assert_no_lint_warnings!(
        r#"
type Ints = Slice<int>

fn sum(nums: Ints) -> int {
  let mut total = 0;
  for n in nums {
    total += n;
  }
  total
}

fn main() {
  let _ = sum([1, 2, 3])
}
"#
    );
}

#[test]
fn type_alias_used_in_let_binding_no_warning() {
    assert_no_lint_warnings!(
        r#"
type Ints = Slice<int>

fn main() {
  let nums: Ints = [1, 2, 3];
  let _ = nums[0]
}
"#
    );
}

#[test]
fn type_alias_used_in_another_type_alias_no_warning() {
    assert_no_lint_warnings!(
        r#"
type Inner = Option<int>
type Outer = Option<Inner>

fn unwrap_nested(o: Outer) -> int {
  match o {
    Some(Some(x)) => x,
    Some(None) => -1,
    None => -2,
  }
}

fn main() {
  let x: Outer = Some(Some(42));
  let _ = unwrap_nested(x)
}
"#
    );
}

#[test]
fn type_alias_used_in_struct_field_no_warning() {
    assert_no_lint_warnings!(
        r#"
type UserId = int

struct User {
  id: UserId,
  name: string,
}

fn main() {
  let u = User { id: 1, name: "Alice" }
  u.id + u.name.len() as int
}
"#
    );
}

#[test]
fn type_alias_used_in_const_annotation_no_warning() {
    assert_no_lint_warnings!(
        r#"
type Limit = int

const MAX: Limit = 100

fn main() {
  let _ = MAX + 1
}
"#
    );
}

#[test]
fn type_alias_used_in_cast_expression_no_warning() {
    assert_no_lint_warnings!(
        r#"
type Score = float64

fn main() {
  let x = 42 as Score
  let _ = x + 1.0
}
"#
    );
}

#[test]
fn type_used_via_static_method_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Vec2 { x: int, y: int }

impl Vec2 {
  fn new(x: int, y: int) -> Vec2 {
    Vec2 { x, y }
  }

  fn length_squared(self: Vec2) -> int {
    self.x * self.x + self.y * self.y
  }
}

fn main() {
  let v1 = Vec2.new(3, 4);
  let _ = v1.length_squared()
}
"#
    );
}

#[test]
fn format_string_uses_variable() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let name = "world";
  let msg = f"Hello, {name}!";
  let _ = msg
}
"#
    );
}

#[test]
fn uninterpolated_fstring() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let msg = f"hello world";
  let _ = msg
}
"#
    );
}

#[test]
fn expression_only_fstring() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let name = "world";
  let msg = f"{name}";
  msg
}
"#
    );
}

#[test]
fn fstring_with_text_and_interpolation_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let name = "world";
  let msg = f"hello {name}";
  let _ = msg
}
"#
    );
}

#[test]
fn fstring_with_non_string_expression_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let num = 42;
  let msg = f"{num}";
  let _ = msg
}
"#
    );
}

#[test]
fn slice_literal_uses_function() {
    assert_no_lint_warnings!(
        r#"
fn one() -> int { 1 }
fn two() -> int { 2 }

fn main() {
  let xs = [one(), two(), 3];
  let _ = xs[0]
}
"#
    );
}

#[test]
fn propagate_expression_uses_result_type() {
    assert_no_lint_warnings!(
        r#"
pub struct Error {
  message: string,
}

fn might_fail() -> Result<int, Error> {
  Ok(42)
}

fn run() -> Result<int, Error> {
  let value = might_fail()?;
  Ok(value)
}

fn main() { let _ = run() }
"#
    );
}

#[test]
fn tuple_uses_struct_types() {
    assert_no_lint_warnings!(
        r#"
pub struct First { a: int }
pub struct Second { b: string }

fn main() {
  let pair = (First { a: 1 }, Second { b: "x" });
  let (first, _second) = pair;
  let _ = first.a + 1
}
"#
    );
}

#[test]
fn paren_expression_uses_function() {
    assert_no_lint_warnings!(
        r#"
fn compute() -> int { 42 }

fn main() {
  let result = (compute()) + 1;
  let _ = result
}
"#
    );
}

#[test]
fn reference_expression_uses_variable() {
    assert_no_lint_warnings!(
        r#"
pub struct Data { value: int }

fn main() {
  let d = Data { value: 42 };
  let ptr = &d;
  let _ = ptr.value
}
"#
    );
}

#[test]
fn division_by_non_zero_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn test() {
  let x = 10 / 2;
  let _ = x
}
"#
    );
}

#[test]
fn empty_match_arm() {
    assert_lint_snapshot!(
        r#"
pub fn test() {
  let opt: Option<int> = None;
  match opt {
    Some(_x) => {},
    None => (),
  }
}
"#
    );
}

#[test]
fn match_arm_with_unit_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn test() {
  let opt: Option<int> = None;
  match opt {
    Some(_) => (),
    None => (),
  }
}
"#
    );
}

#[test]
fn unnecessary_reference() {
    assert_lint_snapshot!(
        r#"
pub fn foo(x: Ref<int>) {
  let _ = &x;
}
"#
    );
}

#[test]
fn necessary_reference_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x = 42;
  let _ = &x;
}
"#
    );
}

#[test]
fn unused_type_parameter() {
    assert_lint_snapshot!(
        r#"
pub fn process<T>(x: int) -> int {
  x
}
"#
    );
}

#[test]
fn used_type_parameter_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn identity<T>(x: T) -> T {
  x
}

fn main() {
  let _ = identity(42);
}
"#
    );
}

#[test]
fn type_param_only_in_bound_warns() {
    assert_lint_snapshot!(
        r#"
pub interface Cloner<T: Cloner<T>> {
  fn clone(self) -> T
}

pub fn squiggle<A: Cloner<B>, B>(_: A) {}
"#
    );
}

#[test]
fn type_param_in_bound_and_used_as_parameter_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub interface Cloner<T: Cloner<T>> {
  fn clone(self) -> T
}

struct Foo{}

impl Foo {
  fn clone(self) -> Foo { Foo{} }
}

pub fn squiggle<A: Cloner<B>, B>(_: A, _: B) {}

fn main() {
  squiggle(Foo{}, Foo{})
}
"#
    );
}

#[test]
fn type_param_in_bound_and_used_as_return_type_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub interface Cloner<T: Cloner<T>> {
  fn clone(self) -> T
}

struct Foo{}

impl Foo {
  fn clone(self) -> Foo { Foo{} }
}

pub fn squiggle<A: Cloner<B>, B>(a: A) -> B {
  a.clone()
}

fn main() {
  let _ = squiggle(Foo{})
}
"#
    );
}

#[test]
fn type_param_only_in_bound_underscore_prefix_suppressed() {
    assert_no_lint_warnings!(
        r#"
pub interface Cloner<T: Cloner<T>> {
  fn clone(self) -> T
}

pub fn squiggle<A: Cloner<_B>, _B>(_: A) {}
"#
    );
}

#[test]
fn interface_used_as_struct_type_parameter_constraint_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"

interface Showable {
  fn show(self) -> string
}

struct Wrapper<T: Showable> {
  inner: T,
}

impl<T: Showable> Wrapper<T> {
  fn display(self) -> string {
    self.inner.show()
  }
}

struct Name {
  value: string,
}

impl Name {
  fn show(self) -> string {
    self.value
  }
}

fn main() {
  let w = Wrapper { inner: Name { value: "test" } }
  fmt.Println(w.display())
}
"#
    );
}

#[test]
fn rest_only_slice_pattern_discard() {
    assert_lint_snapshot!(
        r#"
pub fn test(slice: Slice<int>) {
  let [..] = slice;
}
"#
    );
}

#[test]
fn rest_only_slice_pattern_bind() {
    assert_lint_snapshot!(
        r#"
pub fn test(slice: Slice<int>) {
  let [..rest] = slice;
  let _ = rest
}
"#
    );
}

#[test]
fn non_pascal_case_struct() {
    assert_lint_snapshot!(
        r#"
struct point { x: int, y: int }

fn main() {
  let _ = point { x: 1, y: 2 };
}
"#
    );
}

#[test]
fn non_pascal_case_enum() {
    assert_lint_snapshot!(
        r#"
enum color { Red, Green, Blue }

fn main() {}
"#
    );
}

#[test]
fn pascal_case_type_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"

struct Point { x: int, y: int }

fn main() {
  let p = Point { x: 1, y: 2 };
  fmt.Print(p.x + p.y);
}
"#
    );
}

#[test]
fn non_snake_case_function() {
    assert_lint_snapshot!(
        r#"
fn getUserId() -> int { 42 }

fn main() {
  let _ = getUserId();
}
"#
    );
}

#[test]
fn snake_case_function_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn get_user_id() -> int { 42 }

fn main() {
  let _ = get_user_id();
}
"#
    );
}

#[test]
fn non_snake_case_variable() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let userId = 42;
  let _ = userId;
}
"#
    );
}

#[test]
fn snake_case_variable_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let user_id = 42;
  let _ = user_id;
}
"#
    );
}

#[test]
fn non_snake_case_parameter() {
    assert_lint_snapshot!(
        r#"
fn greet(userId: int) {
  let _ = userId;
}

fn main() {
  greet(42);
}
"#
    );
}

#[test]
fn snake_case_parameter_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn greet(user_id: int) {
  let _ = user_id;
}

fn main() {
  greet(42);
}
"#
    );
}

#[test]
fn non_snake_case_struct_field() {
    assert_lint_snapshot!(
        r#"
struct User { oddsAndEnds: int }

fn main() {
  let u = User { oddsAndEnds: 42 };
  let _ = u.oddsAndEnds;
}
"#
    );
}

#[test]
fn snake_case_struct_field_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct User { odds_and_ends: int }

fn main() {
  let u = User { odds_and_ends: 42 };
  let _ = u.odds_and_ends;
}
"#
    );
}

#[test]
fn non_snake_case_struct_field_initialism() {
    assert_lint_snapshot!(
        r#"
struct Resource { ID: uint }

fn main() {
  let r = Resource { ID: 1 };
  let _ = r.ID;
}
"#
    );
}

#[test]
fn non_snake_case_struct_field_trailing_initialism() {
    assert_lint_snapshot!(
        r#"
struct User { UserID: int }

fn main() {
  let u = User { UserID: 1 };
  let _ = u.UserID;
}
"#
    );
}

#[test]
fn screaming_snake_case_constant_no_warning() {
    assert_no_lint_warnings!(
        r#"
const MAX_RETRIES = 3;

fn main() {
  let _ = MAX_RETRIES;
}
"#
    );
}

#[test]
fn underscore_prefix_suppresses_casing_warnings() {
    assert_no_lint_warnings!(
        r#"
fn _getUserId() -> int { 42 }

fn main() {
  let _ = _getUserId();
}
"#
    );
}

#[test]
fn non_pascal_case_type_parameter() {
    assert_lint_snapshot!(
        r#"
fn identity<t>(x: t) -> t { x }

fn main() {
  let _ = identity(42);
}
"#
    );
}

#[test]
fn pascal_case_type_parameter_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn identity<T>(x: T) -> T { x }

fn main() {
  let _ = identity(42);
}
"#
    );
}

#[test]
fn non_pascal_case_enum_variant() {
    assert_lint_snapshot!(
        r#"
pub enum Status { pending, completed }

fn main() {
  let _ = Status.pending;
}
"#
    );
}

#[test]
fn pascal_case_enum_variant_no_warning() {
    assert_no_lint_warnings!(
        r#"
enum Status { Pending, Completed }

fn main() {
  let _ = Status.Pending;
  let _ = Status.Completed;
}
"#
    );
}

#[test]
fn irrefutable_if_let_identifier() {
    assert_lint_snapshot!(
        r#"
pub fn test(x: int) {
  if let y = x {
    let _ = y;
  }
}
"#
    );
}

#[test]
fn irrefutable_if_let_tuple() {
    assert_lint_snapshot!(
        r#"
pub fn test(pair: (int, int)) {
  if let (a, b) = pair {
    let _ = a + b;
  }
}
"#
    );
}

#[test]
fn refutable_if_let_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn test(opt: Option<int>) {
  if let Some(x) = opt {
    let _ = x;
  }
}

fn main() { test(Some(1)); }
"#
    );
}

#[test]
fn match_as_if_let_option() {
    assert_lint_snapshot!(
        r#"
pub fn test(opt: Option<int>) {
  match opt {
    Some(x) => { let _ = x; },
    _ => (),
  }
}
"#
    );
}

#[test]
fn match_as_if_let_result() {
    assert_lint_snapshot!(
        r#"
pub fn test(res: Result<int, string>) {
  match res {
    Ok(x) => { let _ = x; },
    _ => (),
  }
}
"#
    );
}

#[test]
fn match_as_if_let_option_none() {
    assert_lint_snapshot!(
        r#"
pub fn test(opt: Option<int>) {
  match opt {
    Some(x) => { let _ = x; },
    None => (),
  }
}
"#
    );
}

#[test]
fn match_as_if_let_result_err() {
    assert_lint_snapshot!(
        r#"
pub fn test(res: Result<int, string>) {
  match res {
    Ok(x) => { let _ = x; },
    Err(_) => (),
  }
}
"#
    );
}

#[test]
fn multi_arm_match_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn test(opt: Option<int>) {
  match opt {
    Some(x) => { let _ = x; },
    None => { let _ = 0; },
  }
}

fn main() { test(Some(1)); }
"#
    );
}

#[test]
fn match_as_if_let_meaningful_second_arm() {
    assert_lint_snapshot!(
        r#"
pub fn test(opt: Option<int>) {
  match opt {
    None => (),
    Some(x) => { let _ = x; },
  }
}
"#
    );
}

#[test]
fn match_as_if_let_reversed_none_meaningful() {
    assert_lint_snapshot!(
        r#"
pub fn test(opt: Option<int>) {
  match opt {
    Some(_) => (),
    None => { let _ = 0; },
  }
}
"#
    );
}

#[test]
fn match_as_if_let_preserves_literal_payload() {
    assert_lint_snapshot!(
        r#"
pub fn test(opt: Option<int>) {
  match opt {
    Some(0) => { let _ = 1 },
    _ => (),
  }
}
"#
    );
}

#[test]
fn match_as_if_let_preserves_binding_name() {
    assert_lint_snapshot!(
        r#"
pub fn test(opt: Option<int>) {
  match opt {
    Some(inner) => { let _ = inner },
    None => (),
  }
}
"#
    );
}

#[test]
fn match_as_if_let_two_variant_enum() {
    assert_lint_snapshot!(
        r#"
pub enum Switch { On, Off }

pub fn test(s: Switch) {
  match s {
    Switch.On => { let _ = 1 },
    Switch.Off => (),
  }
}
"#
    );
}

#[test]
fn match_as_if_let_multi_variant_wildcard() {
    assert_lint_snapshot!(
        r#"
pub enum Signal { Red, Yellow, Green }

pub fn test(s: Signal) {
  match s {
    Signal.Red => { let _ = 1 },
    _ => (),
  }
}
"#
    );
}

#[test]
fn equatable_if_let_unit_variant() {
    assert_lint_snapshot!(
        r#"
pub enum Sig { A, B }

pub fn other() -> Sig { Sig.B }

pub fn test(g: Sig) {
  if let Sig.A = g { let _ = 1 }
}
"#
    );
}

#[test]
fn equatable_if_let_field_access_subject() {
    assert_lint_snapshot!(
        r#"
pub enum Color { Red, Green }

pub struct Holder { color: Color }

pub fn other() -> Color { Color.Green }

pub fn test(h: Holder) {
  if let Color.Red = h.color { let _ = 1 }
}
"#
    );
}

#[test]
fn equatable_if_let_binding_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn test(x: Option<int>) { if let Some(n) = x { let _ = n } }

fn main() { test(Some(1)); }
"#
    );
}

#[test]
fn equatable_if_let_wildcard_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn test(x: Option<int>) { if let Some(_) = x { let _ = 1 } }

fn main() { test(Some(1)); }
"#
    );
}

#[test]
fn equatable_if_let_fielded_variant_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn test(x: Option<int>) { if let Some(5) = x { let _ = 1 } }

fn main() { test(Some(1)); }
"#
    );
}

#[test]
fn equatable_if_let_literal_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn test(s: string) { if let "foo" = s { let _ = 1 } }

fn main() { test("foo"); }
"#
    );
}

#[test]
fn equatable_if_let_non_comparable_no_warning() {
    assert_no_lint_warnings!(
        r#"
enum Bag { A, B(Slice<int>) }

fn make() -> Bag { Bag.B([1]) }

fn test(e: Bag) { if let Bag.A = e { let _ = 1 } }

fn main() { let _ = make(); test(Bag.A); }
"#
    );
}

#[test]
fn equatable_if_let_call_subject_no_warning() {
    assert_no_lint_warnings!(
        r#"
enum Color { Red, Green }

fn current() -> Color { Color.Green }

fn test() { if let Color.Red = current() { let _ = 1 } }

fn main() { test(); }
"#
    );
}

#[test]
fn collapsible_match() {
    assert_lint_snapshot!(
        r#"
pub fn test(o: Option<Result<int, string>>) -> int {
  match o {
    Some(n) => match n {
      Ok(x) => x,
      _ => 0,
    },
    _ => 0,
  }
}
"#
    );
}

#[test]
fn collapsible_match_nested_if_let() {
    assert_lint_snapshot!(
        r#"
pub fn use_int(x: int) -> int { x }

pub fn test(opt: Option<Result<int, string>>) {
  if let Some(n) = opt {
    if let Ok(x) = n {
      let _ = use_int(x)
    }
  }
}
"#
    );
}

#[test]
fn collapsible_match_differing_dismissals_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn test(o: Option<Option<int>>) -> int {
  match o {
    Some(n) => match n {
      Some(x) => x,
      _ => 1,
    },
    _ => 0,
  }
}
"#
    );
}

#[test]
fn collapsible_match_outer_binding_used_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn depth(o: Option<int>) -> int { o.unwrap_or(0) }

pub fn test(o: Option<Option<int>>) -> int {
  match o {
    Some(n) => match n {
      Some(x) => depth(n) + x,
      _ => 0,
    },
    _ => 0,
  }
}
"#
    );
}

#[test]
fn collapsible_match_inner_identifier_dismissal_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn test(o: Option<Option<int>>, fallback: Option<int>) -> Option<int> {
  match o {
    Some(n) => match n {
      Some(v) => Some(v),
      fallback => fallback,
    },
    _ => fallback,
  }
}
"#
    );
}

#[test]
fn collapsible_match_inner_specific_dismissal_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn test(o: Option<Result<int, string>>) -> int {
  match o {
    Some(n) => match n {
      Ok(x) => x,
      Err(e) => 0,
    },
    _ => 0,
  }
}
"#
    );
}

#[test]
fn match_single_binding_identifier() {
    assert_lint_snapshot!(
        r#"
pub fn test(x: int) -> int {
  match x {
    y => y + 1,
  }
}
"#
    );
}

#[test]
fn match_single_binding_wildcard_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn test(x: int) -> int {
  match x {
    _ => 42,
  }
}
"#
    );
}

#[test]
fn match_single_binding_tuple_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn test(pair: (int, int)) -> int {
  match pair {
    (a, b) => a + b,
  }
}
"#
    );
}

#[test]
fn match_on_bool_true_false() {
    assert_lint_snapshot!(
        r#"
pub fn test(b: bool) -> int {
  match b {
    true => 1,
    false => 0,
  }
}
"#
    );
}

#[test]
fn match_on_bool_false_true() {
    assert_lint_snapshot!(
        r#"
pub fn test(b: bool) -> int {
  match b {
    false => 0,
    true => 1,
  }
}
"#
    );
}

#[test]
fn match_on_bool_guard_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn test(b: bool, n: int) -> int {
  match b {
    true if n > 0 => 1,
    _ => 0,
  }
}
"#
    );
}

#[test]
fn match_on_bool_wildcard_arm_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn test(b: bool) -> int {
  match b {
    true => 1,
    _ => 0,
  }
}
"#
    );
}

#[test]
fn match_on_bool_non_bool_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn test(x: int) -> int {
  match x {
    0 => 1,
    _ => 2,
  }
}
"#
    );
}

#[test]
fn match_on_bool_duplicate_true_no_suggestion() {
    let warnings = crate::_harness::lint::lint(
        r#"
pub fn test(b: bool) -> int {
  match b {
    true => 1,
    true => 2,
  }
}
"#,
    );
    let suggests_if = warnings
        .iter()
        .any(|w| w.code_str() == Some("lint.match_on_bool"));
    assert!(
        !suggests_if,
        "expected no match_on_bool suggestion on duplicate `true` arms, got: {:?}",
        warnings
    );
}

#[test]
fn match_on_bool_duplicate_false_no_suggestion() {
    let warnings = crate::_harness::lint::lint(
        r#"
pub fn test(b: bool) -> int {
  match b {
    false => 1,
    false => 2,
  }
}
"#,
    );
    let suggests_if = warnings
        .iter()
        .any(|w| w.code_str() == Some("lint.match_on_bool"));
    assert!(
        !suggests_if,
        "expected no match_on_bool suggestion on duplicate `false` arms, got: {:?}",
        warnings
    );
}

#[test]
fn redundant_pattern_matching_option_is_some() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = match o {
    Some(_) => true,
    None => false,
  }
}
"#
    );
}

#[test]
fn redundant_pattern_matching_option_is_none() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = match o {
    Some(_) => false,
    None => true,
  }
}
"#
    );
}

#[test]
fn redundant_pattern_matching_result_is_ok() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let r: Result<int, string> = Ok(1)
  let _ = match r {
    Ok(_) => true,
    Err(_) => false,
  }
}
"#
    );
}

#[test]
fn redundant_pattern_matching_result_is_err() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let r: Result<int, string> = Ok(1)
  let _ = match r {
    Ok(_) => false,
    Err(_) => true,
  }
}
"#
    );
}

#[test]
fn redundant_pattern_matching_reversed_arms() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = match o {
    None => false,
    Some(_) => true,
  }
}
"#
    );
}

#[test]
fn redundant_pattern_matching_non_bool_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = match o {
    Some(_) => 1,
    None => 0,
  }
}
"#
    );
}

#[test]
fn identical_match_arms_same_bool() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = match o {
    Some(_) => true,
    None => true,
  }
}
"#
    );
}

#[test]
fn redundant_pattern_matching_same_bool_no_suggestion() {
    let warnings = crate::_harness::lint::lint(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = match o {
    Some(_) => true,
    None => true,
  }
}
"#,
    );
    let suggests_predicate = warnings
        .iter()
        .any(|w| w.code_str() == Some("lint.redundant_pattern_matching"));
    assert!(
        !suggests_predicate,
        "expected no redundant_pattern_matching suggestion on same-bool arms, got: {:?}",
        warnings
    );
}

#[test]
fn redundant_pattern_matching_bound_payload_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = match o {
    Some(v) => true,
    None => false,
  }
}
"#
    );
}

#[test]
fn redundant_pattern_matching_non_option_enum_no_warning() {
    assert_no_lint_warnings!(
        r#"
enum Toggle { On, Off }

fn main() {
  let t = Toggle.On
  let _ = match t {
    Toggle.On => true,
    Toggle.Off => false,
  }
}
"#
    );
}

#[test]
fn manual_unwrap_or_option() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = match o {
    Some(v) => v,
    None => 0,
  }
}
"#
    );
}

#[test]
fn manual_unwrap_or_result() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let r: Result<int, string> = Ok(1)
  let _ = match r {
    Ok(v) => v,
    Err(_) => 0,
  }
}
"#
    );
}

#[test]
fn manual_unwrap_or_reversed_arms() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = match o {
    None => 0,
    Some(v) => v,
  }
}
"#
    );
}

#[test]
fn manual_unwrap_or_else_effectful_default() {
    assert_lint_snapshot!(
        r#"
fn fallback() -> int {
  0
}

fn main() {
  let o: Option<int> = Some(1)
  let _ = match o {
    Some(v) => v,
    None => fallback(),
  }
}
"#
    );
}

#[test]
fn manual_unwrap_or_panicking_default() {
    assert_lint_snapshot!(
        r#"
fn run(denom: int) -> int {
  let o: Option<int> = Some(1)
  match o {
    Some(v) => v,
    None => 1 / denom,
  }
}

fn main() {
  let _ = run(2)
}
"#
    );
}

#[test]
fn manual_unwrap_or_composite_literal_default() {
    assert_lint_snapshot!(
        r#"
fn fallback() -> int {
  0
}

fn run() -> Slice<int> {
  let o: Option<Slice<int>> = Some([1])
  match o {
    Some(v) => v,
    None => [fallback()],
  }
}

fn main() {
  let _ = run()
}
"#
    );
}

#[test]
fn manual_unwrap_or_transformed_body_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  match o {
    Some(v) => { let _ = v + 1; },
    None => { let _ = 0; },
  }
}
"#
    );
}

#[test]
fn manual_unwrap_or_bound_error_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let r: Result<int, string> = Ok(1)
  let _ = match r {
    Ok(v) => v,
    Err(e) => e.length(),
  }
}
"#
    );
}

#[test]
fn manual_unwrap_or_propagating_default_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn fallback() -> Result<int, string> {
  Ok(0)
}

fn run(r: Result<int, string>) -> Result<int, string> {
  let v = match r {
    Ok(v) => v,
    Err(_) => fallback()?,
  }
  Ok(v)
}

fn main() {
  let _ = run(Ok(1))
}
"#
    );
}

#[test]
fn manual_unwrap_or_conditional_return_default_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn run(o: Option<int>, flag: bool) -> int {
  let v = match o {
    Some(v) => v,
    None => if flag { return 0 } else { 5 },
  }
  v + 1
}

fn main() {
  let _ = run(Some(1), true)
}
"#
    );
}

#[test]
fn manual_unwrap_or_diverging_default_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = match o {
    Some(v) => v,
    None => return (),
  }
}
"#
    );
}

#[test]
fn manual_unwrap_or_custom_enum_no_warning() {
    assert_no_lint_warnings!(
        r#"
enum Maybe { Yes(int), Nope }

fn main() {
  let m = Maybe.Yes(1)
  let _ = match m {
    Maybe.Yes(v) => v,
    Maybe.Nope => 0,
  }
}
"#
    );
}

#[test]
fn manual_unwrap_or_mismatched_binding_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let other = 5
  let _ = match o {
    Some(v) => other,
    None => 0,
  }
}
"#
    );
}

#[test]
fn manual_map_option() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = match o {
    Some(v) => Some(v + 1),
    None => None,
  }
}
"#
    );
}

#[test]
fn manual_map_result() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let r: Result<int, string> = Ok(1)
  let _ = match r {
    Ok(v) => Ok(v + 1),
    Err(e) => Err(e),
  }
}
"#
    );
}

#[test]
fn manual_map_reversed_arms() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = match o {
    None => None,
    Some(v) => Some(v + 1),
  }
}
"#
    );
}

#[test]
fn manual_map_non_bare_none_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = match o {
    Some(v) => Some(v + 1),
    None => Some(0),
  }
}
"#
    );
}

#[test]
fn manual_map_propagating_body_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn run(o: Option<Option<int>>) -> Option<int> {
  match o {
    Some(v) => Some(v? + 1),
    None => None,
  }
}

fn main() {
  let _ = run(Some(Some(1)))
}
"#
    );
}

#[test]
fn manual_map_transformed_error_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let r: Result<int, int> = Ok(1)
  let _ = match r {
    Ok(v) => Ok(v + 1),
    Err(e) => Err(e + 1),
  }
}
"#
    );
}

#[test]
fn manual_map_custom_enum_no_warning() {
    assert_no_lint_warnings!(
        r#"
enum Maybe { Yes(int), Nope }

fn main() {
  let m = Maybe.Yes(1)
  let _ = match m {
    Maybe.Yes(v) => Maybe.Yes(v + 1),
    Maybe.Nope => Maybe.Nope,
  }
}
"#
    );
}

#[test]
fn manual_map_rewraps_lookalike_enum_no_warning() {
    assert_no_lint_warnings!(
        r#"
enum MyResult<T, E> { Ok(T), Err(E) }

fn main() {
  let r: Result<int, string> = Ok(1)
  let _: MyResult<int, string> = match r {
    Ok(v) => MyResult.Ok(v + 1),
    Err(e) => MyResult.Err(e),
  }
}
"#
    );
}

#[test]
fn manual_map_or_option() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = match o {
    Some(v) => v + 1,
    None => 0,
  }
}
"#
    );
}

#[test]
fn manual_map_or_result() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let r: Result<int, string> = Ok(1)
  let _ = match r {
    Ok(v) => v * 2,
    Err(_) => 0,
  }
}
"#
    );
}

#[test]
fn manual_map_or_reversed_arms() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = match o {
    None => 0,
    Some(v) => v + 1,
  }
}
"#
    );
}

#[test]
fn manual_map_or_effectful_default_option() {
    assert_lint_snapshot!(
        r#"
fn fallback() -> int {
  0
}

fn main() {
  let o: Option<int> = Some(1)
  let _ = match o {
    Some(v) => v + 1,
    None => fallback(),
  }
}
"#
    );
}

#[test]
fn manual_map_or_effectful_default_result() {
    assert_lint_snapshot!(
        r#"
fn fallback() -> int {
  0
}

fn main() {
  let r: Result<int, string> = Ok(1)
  let _ = match r {
    Ok(v) => v + 1,
    Err(_) => fallback(),
  }
}
"#
    );
}

#[test]
fn manual_map_or_panicking_default_lazy() {
    assert_lint_snapshot!(
        r#"
fn run(denom: int) -> int {
  let o: Option<int> = Some(1)
  match o {
    Some(v) => v + 1,
    None => 1 / denom,
  }
}

fn main() {
  let _ = run(2)
}
"#
    );
}

#[test]
fn manual_map_or_composite_literal_default_lazy() {
    assert_lint_snapshot!(
        r#"
fn fallback() -> int {
  0
}

fn run() -> Slice<int> {
  let o: Option<int> = Some(1)
  match o {
    Some(v) => [v],
    None => [fallback()],
  }
}

fn main() {
  let _ = run()
}
"#
    );
}

#[test]
fn manual_map_or_interpolated_string_default_lazy() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5
  let o: Option<string> = Some("a")
  let _ = match o {
    Some(v) => f"got {v}",
    None => f"x is {x}",
  }
}
"#
    );
}

#[test]
fn manual_map_or_unused_binding_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = match o {
    Some(v) => 42,
    None => 0,
  }
}
"#
    );
}

#[test]
fn manual_map_or_block_identity_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = match o {
    Some(v) => { v },
    None => 0,
  }
}
"#
    );
}

#[test]
fn manual_map_or_shadowed_binding_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn compute() -> int {
  7
}

fn main() {
  let o: Option<int> = Some(1)
  let _ = match o {
    Some(v) => {
      let v = compute()
      v + 1
    },
    None => 0,
  }
}
"#
    );
}

#[test]
fn manual_map_or_bound_error_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let r: Result<int, string> = Ok(1)
  let _ = match r {
    Ok(v) => v + 1,
    Err(e) => e.length(),
  }
}
"#
    );
}

#[test]
fn manual_map_or_diverging_default_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = match o {
    Some(v) => v + 1,
    None => return (),
  }
}
"#
    );
}

#[test]
fn manual_map_or_propagating_body_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn run(o: Option<Option<int>>) -> Option<int> {
  let r = match o {
    Some(v) => v?,
    None => 0,
  }
  Some(r)
}

fn main() {
  let _ = run(Some(Some(1)))
}
"#
    );
}

#[test]
fn manual_map_or_cross_wrapper_result_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn run(o: Option<int>) -> Result<int, string> {
  match o {
    Some(v) => Ok(v + 1),
    None => Ok(0),
  }
}

fn main() {
  let _ = run(Some(1))
}
"#
    );
}

#[test]
fn manual_map_or_custom_enum_no_warning() {
    assert_no_lint_warnings!(
        r#"
enum Maybe { Yes(int), Nope }

fn main() {
  let m = Maybe.Yes(1)
  let _ = match m {
    Maybe.Yes(v) => v + 1,
    Maybe.Nope => 0,
  }
}
"#
    );
}

#[test]
fn manual_map_or_side_effecting_arms_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let mut counts: Map<string, int> = Map.new<string, int>()
  match counts.get("a") {
    Some(existing) => counts["a"] = existing + 1,
    None => counts["a"] = 1,
  }
}
"#
    );
}

#[test]
fn redundant_if_let_else() {
    assert_lint_snapshot!(
        r#"
pub fn test(opt: Option<int>) {
  if let Some(x) = opt {
    let _ = x;
  } else {
  }
}
"#
    );
}

#[test]
fn redundant_if_let_else_unit() {
    assert_lint_snapshot!(
        r#"
pub fn test(opt: Option<int>) {
  if let Some(x) = opt {
    let _ = x;
  } else {
    ()
  }
}
"#
    );
}

#[test]
fn if_let_with_meaningful_else_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn test(opt: Option<int>) {
  if let Some(x) = opt {
    let _ = x;
  } else {
    let _ = 0;
  }
}

fn main() { test(Some(1)); }
"#
    );
}

#[test]
fn irrefutable_if_let_struct() {
    assert_lint_snapshot!(
        r#"
pub struct Point { x: int, y: int }

pub fn test(p: Point) {
  if let Point { x, y } = p {
    let _ = x + y;
  }
}
"#
    );
}

#[test]
fn redundant_let_else() {
    assert_lint_snapshot!(
        r#"
pub fn test(opt: Option<int>) {
  let x = opt else { return; };
  let _ = x;
}
"#
    );
}

#[test]
fn enum_variant_used_in_while_let_pattern() {
    assert_no_lint_warnings!(
        r#"
enum Status {
  Active(int),
  Done,
}

fn main() {
  let mut s = Status.Active(3);
  while let Status.Active(x) = s {
    if x <= 1 {
      s = Status.Done
    } else {
      s = Status.Active(x - 1)
    };
    s
  }
}
"#
    );
}

#[test]
fn refutable_or_pattern_if_let_no_warning() {
    assert_no_lint_warnings!(
        r#"
enum E { A, B, C }

fn test(e: E) {
  if let A | B = e {
    ();
  }
}

fn main() {
  test(E.A);
  test(E.B);
  test(E.C);
}
"#
    );
}

#[test]
fn irrefutable_or_pattern_if_let_warning() {
    assert_lint_snapshot!(
        r#"
fn test(opt: Option<int>) {
  if let Some(_) | None = opt {
    ();
  }
}

fn main() { test(Some(1)); }
"#
    );
}

#[test]
fn try_block_no_success_path_err() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let result: Result<int, string> = try {
    Err("fail")?
  };
  let _ = result;
}
"#
    );
}

#[test]
fn try_block_no_success_path_none() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let result: Option<int> = try {
    None?
  };
  let _ = result;
}
"#
    );
}

#[test]
fn excess_parens_on_condition_if() {
    assert_lint_snapshot!(
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
fn excess_parens_on_condition_while() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let mut i = 0;
  while (i < 10) {
    i = i + 1;
  }
  let _ = i;
}
"#
    );
}

#[test]
fn excess_parens_on_condition_match() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5;
  let _ = match (x) {
    0 => 0,
    _ => 1,
  };
}
"#
    );
}

#[test]
fn unknown_attribute() {
    assert_lint_snapshot!(
        r#"
#[foo]
pub struct User {
  name: string,
}
"#
    );
}

#[test]
fn unknown_attribute_on_enum() {
    assert_lint_snapshot!(
        r#"
#[foo]
enum Color {
  Red,
  Green,
}
"#
    );
}

#[test]
fn field_attribute_without_struct_attribute() {
    assert_lint_snapshot!(
        r#"
pub struct User {
  #[json(omitempty)]
  name: string,
}
"#
    );
}

#[test]
fn display_attribute_is_known() {
    assert_no_lint_warnings!(
        r#"
#[display]
struct Point { x: int, y: int }

#[display]
enum Color {
  Red,
  Green,
}

fn main() {
  let p = Point { x: 1, y: 2 }
  let c = Color.Red
  let _ = match c {
    Red => p.x,
    Green => p.y,
  }
}
"#
    );
}

#[test]
fn duplicate_tag_key() {
    assert_lint_snapshot!(
        r#"
#[json]
pub struct User {
  #[json(omitempty)]
  #[json(skip)]
  name: string,
}
"#
    );
}

#[test]
fn conflicting_case_transforms() {
    assert_lint_snapshot!(
        r#"
#[json(snake_case, camel_case)]
pub struct User {
  first_name: string,
}
"#
    );
}

#[test]
fn raw_tags_different_keys_no_duplicate() {
    assert_no_lint_warnings!(
        r#"
#[tag("validate")]
#[tag("custom")]
pub struct User {
  #[tag(`validate:"required"`)]
  #[tag(`custom:"foo"`)]
  name: string,
}
"#
    );
}

#[test]
fn raw_tag_plus_alias_same_key_duplicate() {
    assert_lint_snapshot!(
        r#"
#[tag("validate")]
pub struct User {
  #[tag(`validate:"required"`)]
  #[tag("validate", "email")]
  name: string,
}
"#
    );
}

#[test]
fn raw_tag_should_use_alias() {
    assert_lint_snapshot!(
        r#"
#[json]
pub struct User {
  #[tag(`json:"user_name"`)]
  name: string,
}
"#
    );
}

#[test]
fn struct_tag_satisfies_field_alias() {
    assert_no_lint_warnings!(
        r#"
#[tag("json")]
pub struct User {
  #[json("name")]
  name: string,
}
"#
    );
}

#[test]
fn field_tag_requires_struct_opt_in() {
    assert_lint_snapshot!(
        r#"
pub struct User {
  #[bson("custom_name")]
  name: string,
}
"#
    );
}

#[test]
fn unknown_tag_option_warns() {
    assert_lint_snapshot!(
        r#"
#[json]
pub struct User {
  #[json(unknown_flag)]
  name: string,
}
"#
    );
}

#[test]
fn known_tag_options_no_warning() {
    assert_no_lint_warnings!(
        r#"
#[json(snake_case, omitempty)]
pub struct User {
  #[json("user_name", omitempty, skip)]
  name: string,
  #[json(camel_case, string)]
  age: int,
  #[json(!omitempty)]
  active: bool,
}
"#
    );
}

#[test]
fn struct_fields_accessed_through_ref_not_unused() {
    assert_no_lint_warnings!(
        r#"
struct Node {
  value: int,
  next: Option<Ref<Node>>,
}

fn sum_list(node: Option<Ref<Node>>) -> int {
  if let Some(n) = node {
    n.value + sum_list(n.next)
  } else {
    0
  }
}

fn main() -> int {
  let c = Node { value: 3, next: None }
  let b = Node { value: 2, next: Some(&c) }
  let a = Node { value: 1, next: Some(&b) }
  sum_list(Some(&a))
}
"#
    );
}

#[test]
fn interface_used_as_generic_bound_not_unused() {
    assert_no_lint_warnings!(
        r#"
interface Describable {
  fn describe(self) -> string
}

struct Dog {
  name: string,
}

impl Dog {
  fn describe(self: Dog) -> string {
    self.name
  }
}

fn print_thing<T: Describable>(thing: T) -> string {
  thing.describe()
}

fn main() -> string {
  print_thing(Dog { name: "Rex" })
}
"#
    );
}

#[test]
fn interface_method_via_structural_typing_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub interface Describable {
  fn describe(self) -> string
}

pub fn print_desc(d: Describable) -> string {
  d.describe()
}

struct Dog {
  name: string,
}

impl Dog {
  fn describe(self) -> string {
    f"Dog: {self.name}"
  }
}

fn main() {
  let d = Dog { name: "Rex" }
  let _ = print_desc(d)
}
"#
    );
}

#[test]
fn interface_method_unused_still_warns() {
    assert_lint_snapshot!(
        r#"
pub interface Describable {
  fn describe(self) -> string
}

struct Dog {
  name: string,
}

impl Dog {
  fn describe(self) -> string {
    f"Dog: {self.name}"
  }
}

fn main() {
  ()
}
"#
    );
}

#[test]
fn interface_method_multiple_implementers_no_warning() {
    assert_no_lint_warnings!(
        r#"
interface Animal {
  fn speak(self) -> string
}

fn make_sound(a: Animal) -> string {
  a.speak()
}

struct Dog {
  name: string,
}

impl Dog {
  fn speak(self) -> string {
    self.name
  }
}

struct Cat {
  name: string,
}

impl Cat {
  fn speak(self) -> string {
    self.name
  }
}

fn main() {
  let dog = Dog { name: "Rex" }
  let cat = Cat { name: "Whiskers" }
  let _ = make_sound(dog)
  let _ = make_sound(cat)
}
"#
    );
}

#[test]
fn impl_method_pascal_case_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct MyError { message: string }

impl MyError {
  fn Error(self) -> string {
    self.message
  }
}

fn main() {
  let e = MyError { message: "fail" };
  let _ = e.Error();
}
"#
    );
}

#[test]
fn standalone_function_pascal_case_still_warns() {
    assert_lint_snapshot!(
        r#"
fn GetUserId() -> int { 42 }

fn main() {
  let _ = GetUserId();
}
"#
    );
}

#[test]
fn unused_self_in_method_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub struct Circle { radius: float64 }

impl Circle {
  fn name(self) -> string {
    "circle"
  }
}

fn main() {
  let c = Circle { radius: 1.0 };
  let _ = c.name();
}
"#
    );
}

#[test]
fn interface_used_as_struct_field_type_no_warning() {
    assert_no_lint_warnings!(
        r#"
interface Greeter {
  fn greet(self) -> string
}

struct Person { name: string }
impl Person {
  fn greet(self) -> string { self.name }
}

struct App {
  greeter: Greeter,
}

fn main() {
  let app = App { greeter: Person { name: "Alice" } };
  let _ = app.greeter.greet();
}
"#
    );
}

#[test]
fn interface_used_as_enum_variant_field_type_no_warning() {
    assert_no_lint_warnings!(
        r#"
interface Handler {
  fn handle(self) -> string
}

struct MyHandler {}
impl MyHandler {
  fn handle(self) -> string { "ok" }
}

enum Action {
  Run { handler: Handler },
  Skip,
}

fn main() -> string {
  let action = Action.Run { handler: MyHandler {} };
  match action {
    Action.Run { handler } => handler.handle(),
    Action.Skip => "skipped",
  }
}
"#
    );
}

#[test]
fn type_used_in_turbofish_no_warning() {
    assert_no_lint_warnings!(
        r#"
interface Worker {
  fn work(self) -> string;
}

struct Greeter { name: string }

impl Greeter {
  fn work(self) -> string { self.name }
}

fn main() {
  let ch = Channel.new<Worker>();
  ch.send(Greeter { name: "test" });
  ch.close()
}
"#
    );
}

#[test]
fn unused_result_in_tail_position() {
    assert_lint_snapshot!(
        r#"
fn get_result() -> Result<int, string> {
  Ok(42)
}

fn do_work() {
  get_result()
}

fn main() {
  do_work()
}
"#
    );
}

#[test]
fn unused_option_in_tail_position() {
    assert_lint_snapshot!(
        r#"
fn find_item() -> Option<int> {
  Some(42)
}

fn do_search() {
  find_item()
}

fn main() {
  do_search()
}
"#
    );
}

#[test]
fn result_in_tail_position_of_result_fn_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn get_result() -> Result<int, string> {
  Ok(42)
}

fn wrapper() -> Result<int, string> {
  get_result()
}

fn main() {
  let _ = wrapper()
}
"#
    );
}

#[test]
fn unused_partial() {
    assert_lint_snapshot!(
        r#"
fn get_partial() -> Partial<int, string> {
  Partial.Ok(42)
}

fn main() {
  get_partial()
  ()
}
"#
    );
}

#[test]
fn unused_partial_in_tail_position() {
    assert_lint_snapshot!(
        r#"
fn get_partial() -> Partial<int, string> {
  Partial.Ok(42)
}

fn main() {
  get_partial()
}
"#
    );
}

#[test]
fn unused_partial_handled_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn get_partial() -> Partial<int, string> {
  Partial.Ok(42)
}

fn main() {
  let _ = get_partial()
  ()
}
"#
    );
}

#[test]
fn unnecessary_raw_string() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let msg = r"hello";
  let _ = msg
}
"#
    );
}

#[test]
fn unnecessary_raw_string_empty() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let msg = r"";
  let _ = msg
}
"#
    );
}

#[test]
fn unnecessary_raw_string_in_pattern() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let s = "hello"
  let _ = match s { r"hello" => 1, _ => 0 }
}
"#
    );
}

#[test]
fn replaceable_with_zero_fill_lisette_struct() {
    assert_lint_snapshot!(
        r#"
struct Conf { name: string, count: int, on: bool, retries: int }

fn main() -> int {
  let c = Conf { name: "x", count: 0, on: false, retries: 0 };
  let on_n = if c.on { 1 } else { 0 }
  c.name.length() + c.count + on_n + c.retries
}
"#
    );
}

#[test]
fn replaceable_with_zero_fill_enum_variant() {
    assert_lint_snapshot!(
        r#"
enum Action {
  Move { x: int, y: int, z: int, dist: int },
  Stop,
}

fn main() -> int {
  let m = Action.Move { x: 5, y: 0, z: 0, dist: 0 };
  let _ = Action.Stop
  match m {
    Action.Move { x, y, z, dist } => x + y + z + dist,
    Action.Stop => 0,
  }
}
"#
    );
}

#[test]
fn replaceable_with_zero_fill_all_fields_zero() {
    assert_lint_snapshot!(
        r#"
struct Point3 { x: int, y: int, z: int }

fn main() -> int {
  let p = Point3 { x: 0, y: 0, z: 0 };
  p.x + p.y + p.z
}
"#
    );
}

#[test]
fn replaceable_with_zero_fill_multiline_literal() {
    assert_lint_snapshot!(
        r#"
struct Conf { name: string, count: int, on: bool, retries: int }

fn main() -> int {
  let c = Conf {
    name: "x",
    count: 0,
    on: false,
    retries: 0,
  }
  let on_n = if c.on { 1 } else { 0 }
  c.name.length() + c.count + on_n + c.retries
}
"#
    );
}

#[test]
fn replaceable_with_zero_fill_below_threshold_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Conf { name: string, count: int, on: bool }

fn main() -> int {
  let c = Conf { name: "x", count: 0, on: false };
  let on_n = if c.on { 1 } else { 0 }
  c.name.length() + c.count + on_n
}
"#
    );
}

#[test]
fn replaceable_with_zero_fill_already_uses_spread_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Conf { name: string, count: int, on: bool }

fn main() -> int {
  let c = Conf { count: 0, on: false, .. };
  let on_n = if c.on { 1 } else { 0 }
  c.name.length() + c.count + on_n
}
"#
    );
}

#[test]
fn replaceable_with_zero_fill_binding_zero_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Conf { count: int, more: int, name: string }

fn main() -> int {
  let zero = 0;
  let c = Conf { count: zero, more: zero, name: "x" };
  c.count + c.more + c.name.length()
}
"#
    );
}

#[test]
fn replaceable_with_zero_fill_incomplete_literal_no_warning() {
    let warnings = crate::_harness::lint::lint(
        r#"
struct Conf {
  title: string,
  count: int,
  on: bool,
  retries: int,
  ch: Channel<int>,
}

fn main() -> string {
  let c = Conf { title: "x", count: 0, on: false, retries: 0 };
  c.title
}
"#,
    );
    let zero_fill = warnings
        .iter()
        .any(|w| w.code_str() == Some("lint.replaceable_with_zero_fill"));
    assert!(
        !zero_fill,
        "expected no replaceable_with_zero_fill warning on incomplete literal, got: {:?}",
        warnings
    );
}

#[test]
fn replaceable_with_zero_fill_constructor_call_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Conf { name: string, items: Slice<int>, lookup: Map<string, int> }

fn main() -> int {
  let c = Conf { name: "x", items: Slice.new<int>(), lookup: Map.new<string, int>() };
  c.name.length() + c.items.len() + c.lookup.len()
}
"#
    );
}

#[test]
fn discarded_lambda_value_bare_literal() {
    assert_lint_snapshot!(
        r#"
fn take(f: fn() -> ()) { f() }
fn main() {
  take(|| { 42 })
}
"#
    );
}

#[test]
fn discarded_lambda_value_silent_on_call() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"
fn take(f: fn() -> ()) { f() }
fn main() {
  take(|| { fmt.Println("hi") })
}
"#
    );
}

#[test]
fn discarded_function_value_bare_literal() {
    assert_lint_snapshot!(
        r#"
fn test() {
  42
}
fn main() {
  test()
}
"#
    );
}

#[test]
fn discarded_function_value_native_call_errors() {
    assert_lint_snapshot!(
        r#"
fn test() {
  "test".length()
}
fn main() {
  test()
}
"#
    );
}

#[test]
fn discarded_function_value_user_call_errors() {
    assert_lint_snapshot!(
        r#"
fn make_int() -> int { 42 }
fn test() {
  make_int()
}
fn main() {
  test()
}
"#
    );
}

#[test]
fn discarded_function_value_silent_on_result_tail() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"
fn test(cond: bool) {
  if cond { fmt.Print("yes") } else { fmt.Print("no") }
}
fn main() {
  test(true)
}
"#
    );
}

#[test]
fn discarded_function_value_silent_with_allow_attr() {
    assert_no_lint_warnings!(
        r#"
#[allow(unused_value)]
fn advance_rng() -> int { 42 }
fn test() {
  advance_rng()
}
fn main() {
  test()
}
"#
    );
}

#[test]
fn discarded_lambda_value_expression_body_errors() {
    assert_lint_snapshot!(
        r#"
fn take(f: fn() -> ()) { f() }
fn main() {
  take(|| 42)
}
"#
    );
}

#[test]
fn discarded_paren_call_returning_result_errors_as_unused_result() {
    assert_lint_snapshot!(
        r#"
fn might_fail() -> Result<int, string> { Ok(1) }
fn test() {
  (might_fail())
}
fn main() {
  test()
}
"#
    );
}

#[test]
fn discarded_paren_call_silent_with_allow_attr() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"
fn test() {
  (fmt.Println("hi"))
}
fn main() {
  test()
}
"#
    );
}

#[test]
fn discarded_loop_with_break_value_errors() {
    assert_lint_snapshot!(
        r#"
fn test() {
  loop { break 1 }
}
fn main() {
  test()
}
"#
    );
}

#[test]
fn discarded_infinite_loop_silent() {
    assert_no_lint_warnings!(
        r#"
fn test() {
  loop { let _ = 1 }
}
fn main() {
  test()
}
"#
    );
}

#[test]
fn discarded_non_call_result_tail_errors() {
    assert_lint_snapshot!(
        r#"
fn get_result() -> Result<int, string> { Ok(1) }
fn test() {
  let r = get_result()
  r
}
fn main() {
  test()
}
"#
    );
}

#[test]
fn discarded_loop_break_value_silent_with_allow_attr() {
    assert_no_lint_warnings!(
        r#"
#[allow(unused_value)]
fn allowed() -> int { 42 }
fn test() {
  let cond = true
  loop {
    if cond {
      break allowed()
    }
  }
}
fn main() {
  test()
}
"#
    );
}

#[test]
fn discarded_loop_two_branches_two_diagnostics() {
    let warnings = crate::_harness::lint::lint(
        r#"
fn test() {
  loop {
    if true { break 1 } else { break 2 }
  }
}
fn main() {
  test()
}
"#,
    );
    let count = warnings
        .iter()
        .filter(|w| w.code_str() == Some("infer.mismatched_return_value"))
        .count();
    assert_eq!(count, 2, "expected one diagnostic per break value");
}

#[test]
fn discarded_non_call_option_in_if_branches_errors() {
    assert_lint_snapshot!(
        r#"
fn get_option() -> Option<int> { Some(1) }
fn test(c: bool) {
  let a = get_option()
  let b = get_option()
  if c { a } else { b }
}
fn main() {
  test(true)
}
"#
    );
}

#[test]
fn interface_method_allow_unused_value_suppresses_lint() {
    assert_no_lint_warnings!(
        r#"
pub interface Router {
  #[allow(unused_value)]
  fn Get(self, path: string) -> Router
}

pub fn register(r: Router) {
  r.Get("/ping")
}

fn main() {
  ()
}
"#
    );
}

#[test]
fn interface_method_without_allow_warns_on_discard() {
    assert_lint_snapshot!(
        r#"
pub interface Router {
  fn Get(self, path: string) -> Router
}

pub fn register(r: Router) {
  r.Get("/ping")
}

fn main() {
  ()
}
"#
    );
}

#[test]
fn invisible_in_string_zero_width_space() {
    assert_lint_snapshot!(
        "
fn main() {
  let _ = \"hello\u{200B}world\"
}
"
    );
}

#[test]
fn invisible_in_string_right_to_left_override() {
    assert_lint_snapshot!(
        "
fn main() {
  let _ = \"admin\u{202E}gnp.exe\"
}
"
    );
}

#[test]
fn invisible_in_string_no_break_space() {
    assert_lint_snapshot!(
        "
fn main() {
  let _ = \"foo\u{00A0}bar\"
}
"
    );
}

#[test]
fn invisible_in_string_byte_order_mark() {
    assert_lint_snapshot!(
        "
fn main() {
  let _ = \"\u{FEFF}leading\"
}
"
    );
}

#[test]
fn invisible_in_fstring_text_part() {
    assert_lint_snapshot!(
        "
fn main() {
  let name = \"world\"
  let _ = f\"hi\u{200B}{name}!\"
}
"
    );
}

#[test]
fn invisible_in_string_pattern() {
    assert_lint_snapshot!(
        "
fn main() {
  let s = \"x\"
  let _ = match s {
    \"foo\u{200B}\" => 1,
    _ => 0,
  };
}
"
    );
}

#[test]
fn invisible_in_string_ascii_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let _ = "hello world"
  let _ = "tab\there"
  let _ = "newline\nhere"
}
"#
    );
}

#[test]
fn invisible_in_string_unicode_letters_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let _ = "πϊθον"
  let _ = "日本語"
  let _ = "emoji: 🦀"
}
"#
    );
}

#[test]
fn verbose_failure_propagation_option_match() {
    assert_lint_snapshot!(
        r#"
fn first(x: Option<int>) -> Option<int> {
  let v = match x {
    Some(v) => v,
    None => return None,
  }
  Some(v + 1)
}

fn main() {
  let _ = first(Some(1))
}
"#
    );
}

#[test]
fn verbose_failure_propagation_option_match_reversed_arms() {
    assert_lint_snapshot!(
        r#"
fn first(x: Option<int>) -> Option<int> {
  let v = match x {
    None => return None,
    Some(v) => v,
  }
  Some(v + 1)
}

fn main() {
  let _ = first(Some(1))
}
"#
    );
}

#[test]
fn verbose_failure_propagation_option_match_wildcard_arm() {
    assert_lint_snapshot!(
        r#"
fn first(x: Option<int>) -> Option<int> {
  let v = match x {
    Some(v) => v,
    _ => return None,
  }
  Some(v + 1)
}

fn main() {
  let _ = first(Some(1))
}
"#
    );
}

#[test]
fn verbose_failure_propagation_result_match() {
    assert_lint_snapshot!(
        r#"
fn first(x: Result<int, string>) -> Result<int, string> {
  let v = match x {
    Ok(v) => v,
    Err(e) => return Err(e),
  }
  Ok(v + 1)
}

fn main() {
  let _ = first(Ok(1))
}
"#
    );
}

#[test]
fn verbose_failure_propagation_if_let_option() {
    assert_lint_snapshot!(
        r#"
fn first(x: Option<int>) -> Option<int> {
  let v = if let Some(v) = x {
    v
  } else {
    return None
  }
  Some(v + 1)
}

fn main() {
  let _ = first(Some(1))
}
"#
    );
}

#[test]
fn verbose_failure_propagation_option_value_default_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x: Option<int> = Some(1)
  let _ = match x {
    Some(_) => 99,
    None => 0,
  }
}
"#
    );
}

#[test]
fn verbose_failure_propagation_option_fallback_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn first(x: Option<int>) -> Option<int> {
  let v = match x {
    Some(v) => v,
    None => return Some(99),
  }
  Some(v + 1)
}

fn main() {
  let _ = first(Some(1))
}
"#
    );
}

#[test]
fn verbose_failure_propagation_option_transform_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn first(x: Option<int>) -> Option<int> {
  let v = match x {
    Some(v) => v + 1,
    None => return None,
  }
  Some(v)
}

fn main() {
  let _ = first(Some(1))
}
"#
    );
}

#[test]
fn verbose_failure_propagation_result_replaced_error_no_warning() {
    assert_no_lint_warnings!(
        r#"
const OTHER_ERROR: string = "oops"

fn first(x: Result<int, string>) -> Result<int, string> {
  let v = match x {
    Ok(v) => v,
    Err(e) => return Err(OTHER_ERROR),
  }
  Ok(v + 1)
}

fn main() {
  let _ = first(Ok(1))
}
"#
    );
}

#[test]
fn verbose_failure_propagation_result_via_wildcard_no_warning() {
    assert_no_lint_warnings!(
        r#"
const OTHER_ERROR: string = "oops"

fn first(x: Result<int, string>) -> Result<int, string> {
  let v = match x {
    Ok(v) => v,
    _ => return Err(OTHER_ERROR),
  }
  Ok(v + 1)
}

fn main() {
  let _ = first(Ok(1))
}
"#
    );
}

#[test]
fn verbose_failure_propagation_guard_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn first(x: Option<int>) -> Option<int> {
  let v = match x {
    Some(v) if v > 0 => v,
    _ => return None,
  }
  Some(v + 1)
}

fn main() {
  let _ = first(Some(1))
}
"#
    );
}

#[test]
fn empty_range_in_for() {
    assert_lint_snapshot!(
        r#"
import "go:fmt"
fn main() {
  for i in 10..0 {
    fmt.Println(i)
  }
}
"#
    );
}

#[test]
fn empty_range_inclusive() {
    assert_lint_snapshot!(
        r#"
import "go:fmt"
fn main() {
  for i in 10..=5 {
    fmt.Println(i)
  }
}
"#
    );
}

#[test]
fn empty_range_with_negative_end() {
    assert_lint_snapshot!(
        r#"
import "go:fmt"
fn main() {
  for i in 0..-5 {
    fmt.Println(i)
  }
}
"#
    );
}

#[test]
fn empty_range_in_slice() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4, 5]
  let _ = xs[3..1]
}
"#
    );
}

#[test]
fn forward_range_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"
fn main() {
  for i in 0..10 {
    fmt.Println(i)
  }
}
"#
    );
}

#[test]
fn equal_bounds_exclusive_range_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"
fn main() {
  for i in 5..5 {
    fmt.Println(i)
  }
}
"#
    );
}

#[test]
fn equal_bounds_inclusive_range_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"
fn main() {
  for i in 5..=5 {
    fmt.Println(i)
  }
}
"#
    );
}

#[test]
fn variable_bounds_range_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"
fn main() {
  let a = 10
  let b = 0
  for i in a..b {
    fmt.Println(i)
  }
}
"#
    );
}

#[test]
fn open_ended_range_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4, 5]
  let _ = xs[2..]
}
"#
    );
}

#[test]
fn index_out_of_bounds_past_length() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let _ = [1, 2, 3][5]
}
"#
    );
}

#[test]
fn index_out_of_bounds_equal_to_length() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let _ = [10, 20, 30][3]
}
"#
    );
}

#[test]
fn index_out_of_bounds_single_element() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let _ = [42][1]
}
"#
    );
}

#[test]
fn index_out_of_bounds_empty_slice() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let _: int = [][0]
}
"#
    );
}

#[test]
fn index_out_of_bounds_length_call() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let xs = [10, 20]
  let _ = xs[xs.length()]
}
"#
    );
}

#[test]
fn index_in_bounds_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let _ = [1, 2, 3][2]
}
"#
    );
}

#[test]
fn index_length_minus_one_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [10, 20]
  let _ = xs[xs.length() - 1]
}
"#
    );
}

#[test]
fn index_dynamic_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3]
  let i = 0
  let _ = xs[i]
}
"#
    );
}

#[test]
fn index_map_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let m = Map.new<int, string>()
  let _ = m[5]
}
"#
    );
}

#[test]
fn oversized_shift_uint32_left() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x: uint32 = 1
  let _ = x << 40
}
"#
    );
}

#[test]
fn oversized_shift_int8_at_width() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x: int8 = 1
  let _ = x << 8
}
"#
    );
}

#[test]
fn oversized_shift_int64_right() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x: int64 = 1
  let _ = x >> 64
}
"#
    );
}

#[test]
fn oversized_shift_byte() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x: byte = 1
  let _ = x << 8
}
"#
    );
}

#[test]
fn oversized_shift_in_bounds_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x: uint16 = 1
  let _ = x << 15
}
"#
    );
}

#[test]
fn oversized_shift_platform_int_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x: uint = 1
  let _ = x << 64
}
"#
    );
}

#[test]
fn oversized_shift_uintptr_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x: uintptr = 1
  let _ = x << 64
}
"#
    );
}

#[test]
fn oversized_shift_non_literal_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x: uint32 = 1
  let n: uint = 40
  let _ = x << n
}
"#
    );
}

#[test]
fn empty_infinite_loop_fires() {
    assert_lint_snapshot!(
        r#"
fn main() {
  loop {}
}
"#
    );
}

#[test]
fn empty_infinite_loop_with_break_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let cond = true
  loop {
    if cond {
      break
    }
  }
}
"#
    );
}

#[test]
fn empty_infinite_loop_non_empty_body_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let mut i = 0
  loop {
    i += 1
    if i > 5 {
      break
    }
  }
}
"#
    );
}

#[test]
fn empty_infinite_loop_while_false_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  while false {}
}
"#
    );
}

#[test]
fn empty_select_default_fires_in_loop() {
    assert_lint_snapshot!(
        r#"
import "go:fmt"

fn main() {
  let ch = Channel.new<int>()
  loop {
    select {
      let Some(v) = ch.receive() => fmt.Println(v),
      _ => {},
    }
  }
}
"#
    );
}

#[test]
fn empty_select_default_fires_in_while() {
    assert_lint_snapshot!(
        r#"
import "go:fmt"

fn main() {
  let ch = Channel.new<int>()
  let mut done = false
  while !done {
    select {
      let Some(v) = ch.receive() => { fmt.Println(v); done = true },
      _ => {},
    }
  }
}
"#
    );
}

#[test]
fn empty_select_default_outside_loop_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"

fn main() {
  let ch = Channel.new<int>()
  select {
    let Some(v) = ch.receive() => fmt.Println(v),
    _ => {},
  }
}
"#
    );
}

#[test]
fn empty_select_default_non_empty_body_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"
import "go:time"

fn main() {
  let ch = Channel.new<int>()
  loop {
    select {
      let Some(v) = ch.receive() => fmt.Println(v),
      _ => time.Sleep(time.Millisecond),
    }
  }
}
"#
    );
}

#[test]
fn empty_select_default_inside_lambda_in_loop_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"

fn main() {
  let ch = Channel.new<int>()
  loop {
    let _ = || {
      select {
        let Some(v) = ch.receive() => fmt.Println(v),
        _ => {},
      }
    }
  }
}
"#
    );
}

#[test]
fn empty_select_default_unit_body_fires() {
    assert_lint_snapshot!(
        r#"
import "go:fmt"

fn main() {
  let ch = Channel.new<int>()
  loop {
    select {
      let Some(v) = ch.receive() => fmt.Println(v),
      _ => (),
    }
  }
}
"#
    );
}

#[test]
fn empty_select_default_inside_task_in_loop_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"

fn main() {
  let ch = Channel.new<int>()
  loop {
    task {
      select {
        let Some(v) = ch.receive() => fmt.Println(v),
        _ => {},
      }
    }
  }
}
"#
    );
}

#[test]
fn single_arm_select_value_position() {
    assert_lint_snapshot!(
        r#"
import "go:fmt"

fn main() {
  let ch = Channel.new<int>()
  let result = select {
    match ch.receive() {
      Some(v) => v,
      None => 0,
    },
  }
  fmt.Println(result)
}
"#
    );
}

#[test]
fn single_arm_select_statement_position() {
    assert_lint_snapshot!(
        r#"
import "go:fmt"

fn main() {
  let ch = Channel.new<int>()
  select {
    match ch.receive() {
      Some(v) => fmt.Println(v),
      None => {},
    },
  }
}
"#
    );
}

#[test]
fn single_arm_select_two_arms_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"

fn main() {
  let a = Channel.new<int>()
  let b = Channel.new<int>()
  let result = select {
    match a.receive() {
      Some(v) => v,
      None => 0,
    },
    match b.receive() {
      Some(v) => v * 2,
      None => 1,
    },
  }
  fmt.Println(result)
}
"#
    );
}

#[test]
fn single_arm_select_with_default_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"

fn main() {
  let ch = Channel.new<int>()
  let result = select {
    match ch.receive() {
      Some(v) => v,
      None => 0,
    },
    _ => -1,
  }
  fmt.Println(result)
}
"#
    );
}

#[test]
fn single_arm_select_send_arm_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"

fn main() {
  let ch = Channel.new<int>()
  select {
    ch.send(42) => fmt.Println("sent"),
  }
}
"#
    );
}

#[test]
fn single_arm_select_shorthand_receive_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"

fn main() {
  let ch = Channel.new<int>()
  select {
    let Some(v) = ch.receive() => fmt.Println(v),
  }
}
"#
    );
}

#[test]
fn decimal_file_mode_chmod() {
    assert_lint_snapshot!(
        r#"
import "go:os"

fn main() {
  let _ = os.Chmod("/tmp/foo", 644)
}
"#
    );
}

#[test]
fn decimal_file_mode_mkdir_all() {
    assert_lint_snapshot!(
        r#"
import "go:os"

fn main() {
  let _ = os.MkdirAll("/tmp/foo", 1000)
}
"#
    );
}

#[test]
fn decimal_file_mode_octal_prefix_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:os"

fn main() {
  let _ = os.Chmod("/tmp/foo", 0o755)
}
"#
    );
}

#[test]
fn decimal_file_mode_below_perm_mask_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:os"

fn main() {
  let _ = os.Chmod("/tmp/foo", 420)
}
"#
    );
}

#[test]
fn decimal_file_mode_non_file_mode_position_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let _ = 1000
}
"#
    );
}

#[test]
fn dup_arg_math_max() {
    assert_lint_snapshot!(
        r#"
import "go:math"

fn main() {
  let x: float64 = 1.0
  let _ = math.Max(x, x)
}
"#
    );
}

#[test]
fn dup_arg_math_min() {
    assert_lint_snapshot!(
        r#"
import "go:math"

fn main() {
  let x: float64 = 1.0
  let _ = math.Min(x, x)
}
"#
    );
}

#[test]
fn dup_arg_reflect_deep_equal() {
    assert_lint_snapshot!(
        r#"
import "go:reflect"

fn main() {
  let s = "abc"
  let _ = reflect.DeepEqual(s, s)
}
"#
    );
}

#[test]
fn dup_arg_strings_replace() {
    assert_lint_snapshot!(
        r#"
import "go:strings"

fn main() {
  let s = "abc"
  let _ = strings.Replace(s, s, s, 1)
}
"#
    );
}

#[test]
fn dup_arg_strings_replace_all() {
    assert_lint_snapshot!(
        r#"
import "go:strings"

fn main() {
  let s = "abc"
  let _ = strings.ReplaceAll(s, s, s)
}
"#
    );
}

#[test]
fn dup_arg_strings_compare() {
    assert_lint_snapshot!(
        r#"
import "go:strings"

fn main() {
  let s = "abc"
  let _ = strings.Compare(s, s)
}
"#
    );
}

#[test]
fn dup_arg_strings_equal_fold() {
    assert_lint_snapshot!(
        r#"
import "go:strings"

fn main() {
  let s = "abc"
  let _ = strings.EqualFold(s, s)
}
"#
    );
}

#[test]
fn dup_arg_bytes_equal() {
    assert_lint_snapshot!(
        r#"
import "go:bytes"

fn main() {
  let bs: Slice<byte> = Slice.new<byte>()
  let _ = bytes.Equal(bs, bs)
}
"#
    );
}

#[test]
fn dup_arg_bytes_compare() {
    assert_lint_snapshot!(
        r#"
import "go:bytes"

fn main() {
  let bs: Slice<byte> = Slice.new<byte>()
  let _ = bytes.Compare(bs, bs)
}
"#
    );
}

#[test]
fn dup_arg_bytes_equal_fold() {
    assert_lint_snapshot!(
        r#"
import "go:bytes"

fn main() {
  let bs: Slice<byte> = Slice.new<byte>()
  let _ = bytes.EqualFold(bs, bs)
}
"#
    );
}

#[test]
fn dup_arg_parenthesized_args() {
    assert_lint_snapshot!(
        r#"
import "go:math"

fn main() {
  let x: float64 = 1.0
  let _ = math.Max((x), x)
}
"#
    );
}

#[test]
fn dup_arg_strings_replace_with_search_replace_dup() {
    assert_lint_snapshot!(
        r#"
import "go:strings"

fn main() {
  let s = "abc"
  let pat = "x"
  let _ = strings.Replace(s, pat, pat, 1)
}
"#
    );
}

#[test]
fn dup_arg_distinct_identifiers_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:math"

fn main() {
  let x: float64 = 1.0
  let y: float64 = 2.0
  let _ = math.Max(x, y)
}
"#
    );
}

#[test]
fn dup_arg_distinct_literals_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:math"

fn main() {
  let _ = math.Max(1.0, 2.0)
}
"#
    );
}

#[test]
fn dup_arg_calls_with_side_effects_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:math"

fn next() -> float64 {
  1.0
}

fn main() {
  let _ = math.Max(next(), next())
}
"#
    );
}

#[test]
fn dup_arg_strings_replace_different_search_replace_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:strings"

fn main() {
  let s = "abc"
  let _ = strings.Replace(s, "a", "b", 1)
}
"#
    );
}

#[test]
fn dup_arg_unrelated_function_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:strings"

fn main() {
  let s = "abc"
  let _ = strings.Contains(s, s)
}
"#
    );
}

#[test]
fn duplicate_cutset_trim() {
    assert_lint_snapshot!(
        r#"
import "go:strings"

fn main() {
  let url = "https://example.com"
  let _ = strings.Trim(url, "https://")
}
"#
    );
}

#[test]
fn duplicate_cutset_trim_left() {
    assert_lint_snapshot!(
        r#"
import "go:strings"

fn main() {
  let s = "//path"
  let _ = strings.TrimLeft(s, "//")
}
"#
    );
}

#[test]
fn duplicate_cutset_trim_right() {
    assert_lint_snapshot!(
        r#"
import "go:strings"

fn main() {
  let s = "value.."
  let _ = strings.TrimRight(s, "..")
}
"#
    );
}

#[test]
fn duplicate_cutset_no_duplicate_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:strings"

fn main() {
  let s = "abc"
  let _ = strings.Trim(s, "abc")
}
"#
    );
}

#[test]
fn duplicate_cutset_non_literal_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:strings"

fn main() {
  let s = "abc"
  let cutset = "aa"
  let _ = strings.Trim(s, cutset)
}
"#
    );
}

#[test]
fn duplicate_cutset_trim_prefix_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:strings"

fn main() {
  let url = "https://example.com"
  let _ = strings.TrimPrefix(url, "https://")
}
"#
    );
}

#[test]
fn duplicate_cutset_trim_space_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:strings"

fn main() {
  let s = "  hi  "
  let _ = strings.TrimSpace(s)
}
"#
    );
}

#[test]
fn json_non_serializable_channel_field() {
    assert_lint_snapshot!(
        r#"
#[json]
pub struct Job {
  id: int,
  ch: Channel<int>,
}
"#
    );
}

#[test]
fn json_non_serializable_function_field() {
    assert_lint_snapshot!(
        r#"
#[json]
pub struct Task {
  handler: fn(int) -> int,
}
"#
    );
}

#[test]
fn json_non_serializable_sender_field() {
    assert_lint_snapshot!(
        r#"
#[json]
pub struct Pipe {
  tx: Sender<int>,
}
"#
    );
}

#[test]
fn json_non_serializable_receiver_field() {
    assert_lint_snapshot!(
        r#"
#[json]
pub struct Pipe {
  rx: Receiver<int>,
}
"#
    );
}

#[test]
fn json_non_serializable_enum_tuple_payload() {
    assert_lint_snapshot!(
        r#"
#[json]
pub enum Event {
  Tick,
  Data(Channel<int>),
}
"#
    );
}

#[test]
fn json_non_serializable_enum_struct_field() {
    assert_lint_snapshot!(
        r#"
#[json]
pub enum Event {
  Run { cb: fn() -> int },
}
"#
    );
}

#[test]
fn json_skipped_channel_field_no_warning() {
    assert_no_lint_warnings!(
        r#"
#[json]
pub struct Job {
  #[json(skip)]
  ch: Channel<int>,
  id: int,
}
"#
    );
}

#[test]
fn json_serializable_struct_no_warning() {
    assert_no_lint_warnings!(
        r#"
#[json]
pub struct Job {
  id: int,
  name: string,
}
"#
    );
}

#[test]
fn non_json_channel_field_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub struct Job {
  ch: Channel<int>,
}
"#
    );
}

#[test]
fn waitgroup_add_in_task() {
    assert_lint_snapshot!(
        r#"
import "go:sync"

fn main() {
  let mut wg = sync.WaitGroup {}
  task {
    wg.Add(1)
    wg.Done()
  }
  wg.Wait()
}
"#
    );
}

#[test]
fn waitgroup_add_before_task_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:sync"

fn main() {
  let mut wg = sync.WaitGroup {}
  wg.Add(1)
  task {
    wg.Done()
  }
  wg.Wait()
}
"#
    );
}

#[test]
fn waitgroup_add_in_task_not_waited_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:sync"

fn main() {
  let mut wg = sync.WaitGroup {}
  task {
    wg.Add(1)
    wg.Done()
  }
}
"#
    );
}

#[test]
fn waitgroup_negative_add_in_task_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:sync"

fn main() {
  let mut wg = sync.WaitGroup {}
  wg.Add(1)
  task {
    wg.Add(-1)
  }
  wg.Wait()
}
"#
    );
}

#[test]
fn waitgroup_distinct_groups_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:sync"

fn main() {
  let mut wg1 = sync.WaitGroup {}
  let mut wg2 = sync.WaitGroup {}
  task {
    wg1.Add(1)
  }
  wg2.Wait()
}
"#
    );
}

#[test]
fn deprecated_function() {
    assert_lint_snapshot!(
        r#"
/// Deprecated: use the new API instead.
fn legacy() -> int {
  42
}

fn main() {
  let _ = legacy()
}
"#
    );
}

#[test]
fn deprecated_method() {
    assert_lint_snapshot!(
        r#"
struct Cache {}

impl Cache {
  /// Deprecated: use the new method instead.
  fn warm(self) -> int {
    1
  }
}

fn main() {
  let c = Cache {}
  let _ = c.warm()
}
"#
    );
}

#[test]
fn deprecated_function_value() {
    assert_lint_snapshot!(
        r#"
/// Deprecated: use the new API instead.
fn legacy() -> int {
  42
}

fn main() {
  let f = legacy
  let _ = f()
}
"#
    );
}

#[test]
fn deprecated_promoted_method() {
    assert_lint_snapshot!(
        r#"
pub struct Logger {
  pub prefix: string,
}

impl Logger {
  /// Deprecated: use the new method instead.
  pub fn old_log(self) -> string {
    self.prefix
  }
}

struct Server {
  embed Logger,
  pub port: int,
}

fn main() {
  let s = Server { Logger: Logger { prefix: "[api]" }, port: 8080 }
  let _ = s.old_log()
}
"#
    );
}

#[test]
fn deprecated_interface_method() {
    assert_lint_snapshot!(
        r#"
interface Store {
  /// Deprecated: use `fetch` instead.
  fn old_get(self) -> int
}

struct Db {}

impl Db {
  fn old_get(self) -> int {
    1
  }
}

fn read(s: Store) -> int {
  s.old_get()
}

fn main() {
  let _ = read(Db {})
}
"#
    );
}

#[test]
fn deprecated_go_stdlib_function() {
    assert_lint_snapshot!(
        r#"
import "go:strings"

fn main() {
  let _ = strings.Title("hello world")
}
"#
    );
}

#[test]
fn deprecated_type_use_no_warning() {
    assert_no_lint_warnings!(
        r#"
/// Deprecated: use NewType instead.
pub struct OldType {
  pub x: int,
}

fn make() -> OldType {
  OldType { x: 1 }
}

fn main() {
  let _ = make()
}
"#
    );
}

#[test]
fn deprecated_non_deprecated_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:strings"

fn main() {
  let _ = strings.ToLower("HELLO")
}
"#
    );
}

#[test]
fn deprecated_allow_no_warning() {
    assert_no_lint_warnings!(
        r#"
/// Deprecated: use the new API instead.
fn legacy() -> int {
  42
}

#[allow(deprecated)]
fn main() {
  let _ = legacy()
}
"#
    );
}

#[test]
fn lost_cancel_discarded() {
    assert_lint_snapshot!(
        r#"
import "go:context"

fn main() {
  let (ctx, _) = context.WithCancel(context.Background())
  let _ = ctx
}
"#
    );
}

#[test]
fn lost_cancel_whole_result_discarded() {
    assert_lint_snapshot!(
        r#"
import "go:context"

fn main() {
  let _ = context.WithCancel(context.Background())
}
"#
    );
}

#[test]
fn lost_cancel_through_type_alias() {
    assert_lint_snapshot!(
        r#"
import "go:context"

type MyCancel = context.CancelFunc

fn make_cancel() -> MyCancel {
  let (ctx, cancel) = context.WithCancel(context.Background())
  let _ = ctx
  cancel
}

fn main() {
  let _ = make_cancel()
}
"#
    );
}

#[test]
fn lost_cancel_named_unused() {
    let warnings = crate::_harness::lint::lint(
        r#"
import "go:context"

fn main() {
  let (ctx, cancel) = context.WithCancel(context.Background())
  let _ = ctx
}
"#,
    );
    let flags_leak = warnings
        .iter()
        .any(|w| w.code_str() == Some("lint.lost_cancel"));
    let flags_unused = warnings
        .iter()
        .any(|w| w.code_str() == Some("lint.unused_variable"));
    assert!(
        flags_leak && flags_unused,
        "expected both lost_cancel and unused_variable on an unused named cancel, got: {:?}",
        warnings
    );
}

#[test]
fn lost_cancel_aliased_copy_no_warning() {
    let warnings = crate::_harness::lint::lint(
        r#"
import "go:context"

fn main() {
  let (ctx, cancel) = context.WithCancel(context.Background())
  let cancel2 = cancel
  defer cancel()
  let _ = ctx
}
"#,
    );
    let flags_leak = warnings
        .iter()
        .any(|w| w.code_str() == Some("lint.lost_cancel"));
    assert!(
        !flags_leak,
        "expected no lost_cancel on a copy of a called cancel, got: {:?}",
        warnings
    );
}

#[test]
fn lost_cancel_cause_func_discarded() {
    assert_lint_snapshot!(
        r#"
import "go:context"

fn main() {
  let (ctx, _) = context.WithCancelCause(context.Background())
  let _ = ctx
}
"#
    );
}

#[test]
fn lost_cancel_tuple_projection() {
    let warnings = crate::_harness::lint::lint(
        r#"
import "go:context"

fn main() {
  let cancel = context.WithCancel(context.Background()).1
}
"#,
    );
    let flags_leak = warnings
        .iter()
        .any(|w| w.code_str() == Some("lint.lost_cancel"));
    assert!(
        flags_leak,
        "expected lost_cancel on a cancel projected out of a fresh call, got: {:?}",
        warnings
    );
}

#[test]
fn lost_cancel_projection_of_binding_no_warning() {
    let warnings = crate::_harness::lint::lint(
        r#"
import "go:context"

fn main() {
  let pair = context.WithCancel(context.Background())
  let cancel = pair.1
  defer cancel()
  let _ = pair
}
"#,
    );
    let flags_leak = warnings
        .iter()
        .any(|w| w.code_str() == Some("lint.lost_cancel"));
    assert!(
        !flags_leak,
        "expected no lost_cancel on a projection off an existing binding, got: {:?}",
        warnings
    );
}

#[test]
fn lost_cancel_called_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:context"

fn main() {
  let (ctx, cancel) = context.WithCancel(context.Background())
  defer cancel()
  let _ = ctx
}
"#
    );
}

#[test]
fn lost_cancel_discarded_copy_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:context"

fn main() {
  let (ctx, cancel) = context.WithCancel(context.Background())
  let _ = cancel
  defer cancel()
  let _ = ctx
}
"#
    );
}

#[test]
fn lost_cancel_with_timeout_discarded() {
    assert_lint_snapshot!(
        r#"
import "go:context"
import "go:time"

fn main() {
  let (ctx, _) = context.WithTimeout(context.Background(), time.Second)
  let _ = ctx
}
"#
    );
}

#[test]
fn lost_cancel_allow_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:context"

#[allow(lost_cancel)]
fn main() {
  let (ctx, _) = context.WithCancel(context.Background())
  let _ = ctx
}
"#
    );
}

#[test]
fn exit_after_defer() {
    assert_lint_snapshot!(
        r#"
import "go:os"

fn cleanup() {}

fn main() {
  defer cleanup()
  os.Exit(1)
}
"#
    );
}

#[test]
fn exit_after_defer_nested_in_branch() {
    assert_lint_snapshot!(
        r#"
import "go:os"

fn cleanup() {}

fn main() {
  defer cleanup()
  if os.Getpid() > 0 {
    os.Exit(1)
  }
}
"#
    );
}

#[test]
fn exit_after_defer_no_defer_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:os"

fn main() {
  os.Exit(1)
}
"#
    );
}

#[test]
fn exit_after_defer_without_exit_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:os"

fn cleanup() {}

fn main() {
  defer cleanup()
  let _ = os.Getpid()
}
"#
    );
}

#[test]
fn exit_after_defer_in_returning_guard_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:os"

fn cleanup() {}

fn run(ok: bool) {
  if ok {
    defer cleanup()
    return
  }
  os.Exit(1)
}

fn main() {
  run(true)
}
"#
    );
}

#[test]
fn exit_after_defer_exit_before_defer_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:os"

fn cleanup() {}

fn main() {
  if os.Getpid() > 0 {
    os.Exit(0)
  }
  defer cleanup()
  let _ = os.Getpid()
}
"#
    );
}

#[test]
fn exit_after_defer_allow_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:os"

fn cleanup() {}

#[allow(exit_after_defer)]
fn main() {
  defer cleanup()
  os.Exit(1)
}
"#
    );
}

#[test]
fn exit_after_defer_in_lambda() {
    assert_lint_snapshot!(
        r#"
import "go:os"

fn cleanup() {}

fn main() {
  let f = || {
    defer cleanup()
    os.Exit(1)
  }
  f()
}
"#
    );
}

#[test]
fn exit_after_defer_in_lambda_allow_on_enclosing_function_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:os"

fn cleanup() {}

#[allow(exit_after_defer)]
fn main() {
  let f = || {
    defer cleanup()
    os.Exit(1)
  }
  f()
}
"#
    );
}

#[test]
fn allow_suppresses_ast_walk_lint() {
    assert_no_lint_warnings!(
        r#"
#[allow(self_comparison)]
fn main() {
  let x = 1
  let _ = x == x
}
"#
    );
}

#[test]
fn allow_suppresses_nested_ast_walk_lint() {
    assert_no_lint_warnings!(
        r#"
#[allow(self_comparison)]
fn main() {
  let x = 1
  if x > 0 {
    let _ = x == x
  }
}
"#
    );
}

#[test]
fn allow_is_lint_specific() {
    assert_lint_snapshot!(
        r#"
#[allow(self_comparison)]
fn main() {
  let flag = true
  let _ = !!flag
}
"#
    );
}

#[test]
fn unnecessary_range_loop_read() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let xs = [10, 20, 30]
  for i in 0..xs.length() {
    let _ = xs[i]
  }
}
"#
    );
}

#[test]
fn unnecessary_range_loop_accumulator() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let xs = [1, 2, 3]
  let mut total = 0
  for i in 0..xs.length() {
    total += xs[i]
  }
  let _ = total
}
"#
    );
}

#[test]
fn unnecessary_range_loop_index_used_directly_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3]
  for i in 0..xs.length() {
    let _ = i
  }
}
"#
    );
}

#[test]
fn unnecessary_range_loop_two_collections_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3]
  let ys = [4, 5, 6]
  for i in 0..xs.length() {
    let _ = xs[i]
    let _ = ys[i]
  }
}
"#
    );
}

#[test]
fn unnecessary_range_loop_writes_through_index_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let mut xs = [1, 2, 3]
  for i in 0..xs.length() {
    xs[i] = 0
  }
}
"#
    );
}

#[test]
fn unnecessary_range_loop_nonzero_start_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3]
  for i in 1..xs.length() {
    let _ = xs[i]
  }
}
"#
    );
}

#[test]
fn unnecessary_range_loop_inclusive_range_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3]
  for i in 0..=xs.length() {
    let _ = xs[i]
  }
}
"#
    );
}

#[test]
fn unnecessary_range_loop_discarded_index_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3]
  for _ in 0..xs.length() {
    let _ = 1
  }
}
"#
    );
}

#[test]
fn unnecessary_range_loop_shadowed_collection_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3]
  for i in 0..xs.length() {
    let xs = [9, 9, 9]
    let _ = xs[i]
  }
}
"#
    );
}

#[test]
fn unnecessary_range_loop_field_write_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Point {
  x: int,
}

fn main() {
  let mut xs = [Point { x: 0 }]
  for i in 0..xs.length() {
    xs[i].x = 1
  }
}
"#
    );
}

#[test]
fn unnecessary_range_loop_method_receiver_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [[1, 2], [3, 4]]
  for i in 0..xs.length() {
    let _ = xs[i].length()
  }
}
"#
    );
}

#[test]
fn unnecessary_range_loop_other_index_write_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let mut xs = [1, 2, 3]
  for i in 0..xs.length() {
    xs[0] = 99
    let _ = xs[i]
  }
}
"#
    );
}

#[test]
fn unnecessary_range_loop_map_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let mut m = Map.new<int, int>()
  m[0] = 10
  m[1] = 20
  for i in 0..m.length() {
    let _ = m[i]
  }
}
"#
    );
}

#[test]
fn unnecessary_range_loop_shadowed_inner_index() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let xs = [1, 2, 3]
  let flag = true
  for i in 0..xs.length() {
    let _ = xs[i]
    if flag {
      let i = 0
      let _ = xs[i]
    }
  }
}
"#
    );
}

#[test]
fn unnecessary_range_loop_alias_write_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3]
  let mut ys = xs
  for i in 0..xs.length() {
    ys[0] = 99
    let _ = xs[i]
  }
}
"#
    );
}

#[test]
fn unnecessary_range_loop_collection_passed_to_call_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn sum_all(s: Slice<int>) -> int {
  let mut total = 0
  for x in s {
    total += x
  }
  total
}

fn main() {
  let xs = [1, 2, 3]
  for i in 0..xs.length() {
    let _ = sum_all(xs)
    let _ = xs[i]
  }
}
"#
    );
}

#[test]
fn unnecessary_range_loop_alias_passed_to_call_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn touch(mut s: Slice<int>) {
  s[0] = 99
}

fn main() {
  let xs = [1, 2, 3]
  let mut ys = xs
  for i in 0..xs.length() {
    touch(ys)
    let _ = xs[i]
  }
}
"#
    );
}

#[test]
fn unnecessary_range_loop_wrapper_passed_to_call_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Box {
  items: Slice<int>,
}

fn touch(mut b: Box) {
  b.items[0] = 99
}

fn main() {
  let xs = [1, 2, 3]
  let mut b = Box { items: xs }
  for i in 0..xs.length() {
    touch(b)
    let _ = xs[i]
  }
}
"#
    );
}

#[test]
fn unnecessary_range_loop_lambda_call_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let mut xs = [1, 2, 3]
  let touch = || {
    xs[0] = 99
  }
  for i in 0..xs.length() {
    touch()
    let _ = xs[i]
  }
}
"#
    );
}

#[test]
fn unnecessary_range_loop_task_body_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3]
  for i in 0..xs.length() {
    task {
      let _ = xs[i]
    }
  }
}
"#
    );
}

#[test]
fn unnecessary_range_loop_lambda_body_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3]
  for i in 0..xs.length() {
    let _ = || xs[i]
  }
}
"#
    );
}

#[test]
fn unnecessary_range_loop_select_arm_binds_index_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3]
  let ch = Channel.buffered<int>(1)
  ch.send(0)
  for i in 0..xs.length() {
    let _ = xs[i]
    let _ = select {
      let Some(i) = ch.receive() => xs[i],
      _ => 0,
    }
  }
}
"#
    );
}

#[test]
fn redundant_closure_single_param() {
    assert_lint_snapshot!(
        r#"
fn double(x: int) -> int { x * 2 }

fn main() {
  let xs = [1, 2, 3]
  let _ = xs.map(|x| double(x))
}
"#
    );
}

#[test]
fn redundant_closure_multi_param() {
    assert_lint_snapshot!(
        r#"
fn add(a: int, b: int) -> int { a + b }

fn main() {
  let xs = [1, 2, 3]
  let _ = xs.fold(0, |acc, x| add(acc, x))
}
"#
    );
}

#[test]
fn redundant_closure_module_member() {
    assert_lint_snapshot!(
        r#"
import "go:strconv"

fn main() {
  let xs = ["1", "2"]
  let _ = xs.map(|s| strconv.Atoi(s))
}
"#
    );
}

#[test]
fn redundant_closure_block_body() {
    assert_lint_snapshot!(
        r#"
fn double(x: int) -> int { x * 2 }

fn main() {
  let xs = [1, 2, 3]
  let _ = xs.map(|x| { double(x) })
}
"#
    );
}

#[test]
fn redundant_closure_immutable_local_callee() {
    assert_lint_snapshot!(
        r#"
fn double(x: int) -> int { x * 2 }

fn main() {
  let f = double
  let xs = [1, 2, 3]
  let _ = xs.map(|x| f(x))
}
"#
    );
}

#[test]
fn redundant_closure_partial_application_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn add(a: int, b: int) -> int { a + b }

fn main() {
  let xs = [1, 2, 3]
  let _ = xs.map(|x| add(10, x))
}
"#
    );
}

#[test]
fn redundant_closure_reordered_args_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn add(a: int, b: int) -> int { a + b }

fn main() {
  let xs = [1, 2, 3]
  let _ = xs.fold(0, |acc, x| add(x, acc))
}
"#
    );
}

#[test]
fn redundant_closure_repeated_arg_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn add(a: int, b: int) -> int { a + b }

fn main() {
  let xs = [1, 2, 3]
  let _ = xs.map(|x| add(x, x))
}
"#
    );
}

#[test]
fn redundant_closure_transformed_body_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn double(x: int) -> int { x * 2 }

fn main() {
  let xs = [1, 2, 3]
  let _ = xs.map(|x| double(x) + 1)
}
"#
    );
}

#[test]
fn redundant_closure_mutable_capture_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn double(x: int) -> int { x * 2 }
fn triple(x: int) -> int { x * 3 }

fn main() {
  let mut f = double
  let xs = [1, 2, 3]
  let _ = xs.map(|x| f(x))
  f = triple
}
"#
    );
}

#[test]
fn redundant_closure_method_receiver_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = ["a", "bb"]
  let _ = xs.map(|s| s.length())
}
"#
    );
}

#[test]
fn redundant_closure_mut_param_callee_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn dec(mut x: int) -> int { x -= 1; x }
fn apply(f: fn(int) -> int, n: int) -> int { f(n) }

fn main() {
  let n = 5
  let _ = apply(|x| dec(x), n)
}
"#
    );
}

#[test]
fn redundant_assert_type_primitive() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = 5
  let _ = assert_type<int>(x)
}
"#
    );
}

#[test]
fn redundant_assert_type_named() {
    assert_lint_snapshot!(
        r#"
struct Point { x: int }

fn main() {
  let p = Point { x: 1 }
  let _ = assert_type<Point>(p)
}
"#
    );
}

#[test]
fn assert_type_on_unknown_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let value: Unknown = 7
  let _ = assert_type<int>(value)
}
"#
    );
}

#[test]
fn assert_type_different_type_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x = 5
  let _ = assert_type<int64>(x)
}
"#
    );
}

#[test]
fn lost_query_mutation_set() {
    assert_lint_snapshot!(
        r#"
import "go:net/url"

fn main() {
  let Ok(u) = url.Parse("http://example.com?a=1") else { return }
  u.Query().Set("b", "2")
}
"#
    );
}

#[test]
fn lost_query_mutation_add() {
    assert_lint_snapshot!(
        r#"
import "go:net/url"

fn main() {
  let Ok(u) = url.Parse("http://example.com?a=1") else { return }
  u.Query().Add("b", "2")
}
"#
    );
}

#[test]
fn lost_query_mutation_del() {
    assert_lint_snapshot!(
        r#"
import "go:net/url"

fn main() {
  let Ok(u) = url.Parse("http://example.com?a=1") else { return }
  u.Query().Del("a")
}
"#
    );
}

#[test]
fn lost_query_mutation_alias_receiver() {
    assert_lint_snapshot!(
        r#"
import "go:net/url"

type MyURL = url.URL

fn main() {
  let mut u: MyURL = url.URL { Scheme: "https", Host: "example.com", .. }
  u.Query().Set("b", "2")
}
"#
    );
}

#[test]
fn lost_query_mutation_bound_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:net/url"
import "go:fmt"

fn main() {
  let Ok(u) = url.Parse("http://example.com?a=1") else { return }
  let q = u.Query()
  q.Set("b", "2")
  fmt.Println(q.Encode())
}
"#
    );
}

#[test]
fn lost_query_mutation_read_method_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:net/url"
import "go:fmt"

fn main() {
  let Ok(u) = url.Parse("http://example.com?a=1") else { return }
  fmt.Println(u.Query().Get("a"))
}
"#
    );
}

#[test]
fn lost_query_mutation_encode_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:net/url"
import "go:fmt"

fn main() {
  let Ok(u) = url.Parse("http://example.com?a=1") else { return }
  fmt.Println(u.Query().Encode())
}
"#
    );
}

#[test]
fn lost_query_mutation_user_type_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Bag {
  data: int,
}

impl Bag {
  fn Query(self) -> Bag { self }
  fn Set(self, k: string, v: string) -> int { self.data + k.length() + v.length() }
}

fn main() {
  let b = Bag { data: 0 }
  let _ = b.Query().Set("a", "b")
}
"#
    );
}

#[test]
fn let_and_return_simple() {
    assert_lint_snapshot!(
        r#"
pub fn doubled(n: int) -> int {
  let x = n * 2
  x
}
"#
    );
}

#[test]
fn let_and_return_after_statements() {
    assert_lint_snapshot!(
        r#"
pub fn process(n: int) -> int {
  let doubled = n * 2
  let result = doubled + 1
  result
}
"#
    );
}

#[test]
fn let_and_return_nested_block() {
    assert_lint_snapshot!(
        r#"
pub fn compute() -> int {
  let total = {
    let inner = 5 + 1
    inner
  }
  total + 1
}
"#
    );
}

#[test]
fn let_and_return_annotated_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn annotated() -> int {
  let x: int = 7
  x
}
"#
    );
}

#[test]
fn let_and_return_tail_expression_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn tail_expr(n: int) -> int {
  let x = n
  x + 1
}
"#
    );
}

#[test]
fn let_and_return_destructure_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn destructure() -> int {
  let (a, _) = (1, 2)
  a
}
"#
    );
}

#[test]
fn let_and_return_name_mismatch_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn name_mismatch(a: int, b: int) -> int {
  let _ignored = a + b
  a
}
"#
    );
}

#[test]
fn out_of_domain_call_argument() {
    assert_lint_snapshot!(
        r#"
import "go:time"

fn payday(_d: time.Weekday) -> int { 5 }

fn main() {
  let _ = payday(7)
}
"#
    );
}

#[test]
fn out_of_domain_annotated_binding() {
    assert_lint_snapshot!(
        r#"
import "go:time"

fn main() {
  let w: time.Weekday = 99
  let _ = w
}
"#
    );
}

#[test]
fn out_of_domain_month() {
    assert_lint_snapshot!(
        r#"
import "go:time"

fn main() {
  let m: time.Month = 13
  let _ = m
}
"#
    );
}

#[test]
fn out_of_domain_closed_string_domain() {
    assert_lint_snapshot!(
        r#"
#[go(closed_domain)]
pub struct Level(string)

pub const LOW: Level = "low"
pub const HIGH: Level = "high"

pub fn at(l: Level) -> Level { l }

fn main() {
  let _ = at("medium")
}
"#
    );
}

#[test]
fn out_of_domain_in_domain_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:time"

fn payday(_d: time.Weekday) -> int { 5 }

fn main() {
  let _ = payday(5)
}
"#
    );
}

#[test]
fn out_of_domain_explicit_cast_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:time"

fn payday(_d: time.Weekday) -> int { 5 }

fn main() {
  let _ = payday(7 as time.Weekday)
}
"#
    );
}

#[test]
fn out_of_domain_constructor() {
    assert_lint_snapshot!(
        r#"
import "go:time"

fn main() {
  let _ = time.Weekday(7)
}
"#
    );
}

#[test]
fn out_of_domain_constructor_in_domain_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:time"

fn main() {
  let _ = time.Weekday(6)
}
"#
    );
}

#[test]
fn out_of_domain_non_closed_type_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:time"

fn main() {
  let d: time.Duration = 99
  let _ = d
}
"#
    );
}

#[test]
fn out_of_domain_bit_flag_set_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:io/fs"

fn main() {
  let mode: fs.FileMode = 99
  let _ = mode
}
"#
    );
}

#[test]
fn out_of_domain_negative_literal() {
    assert_lint_snapshot!(
        r#"
import "go:time"

fn main() {
  let w: time.Weekday = -1
  let _ = w
}
"#
    );
}

#[test]
fn out_of_domain_constructor_negative() {
    assert_lint_snapshot!(
        r#"
import "go:time"

fn main() {
  let _ = time.Weekday(-1)
}
"#
    );
}

#[test]
fn out_of_domain_sparse_integer_domain() {
    assert_lint_snapshot!(
        r#"
#[go(closed_domain)]
pub struct Lvl(int)

pub const LOW: Lvl = 1
pub const HIGH: Lvl = 3

pub fn at(l: Lvl) -> Lvl { l }

fn main() {
  let _ = at(2)
}
"#
    );
}

#[test]
fn out_of_domain_negative_member_no_false_positive() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"

#[go(closed_domain)]
pub struct Sign(int)

pub const NEG: Sign = -1
pub const ZERO: Sign = 0
pub const POS: Sign = 1

pub fn at(s: Sign) -> Sign { s }

fn main() {
  let _ = at(-1)
  fmt.Println(NEG, ZERO, POS)
}
"#
    );
}

#[test]
fn out_of_domain_negative_member_domain() {
    assert_lint_snapshot!(
        r#"
import "go:fmt"

#[go(closed_domain)]
pub struct Sign(int)

pub const NEG: Sign = -1
pub const ZERO: Sign = 0
pub const POS: Sign = 1

pub fn at(s: Sign) -> Sign { s }

fn main() {
  let _ = at(2)
  fmt.Println(NEG, ZERO, POS)
}
"#
    );
}

#[test]
fn out_of_domain_float_domain_not_linted() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"

#[go(closed_domain)]
pub struct F(float64)

pub const ZERO: F = 0.0
pub const ONE: F = 1.0

pub fn at(x: F) -> F { x }

fn main() {
  let _ = at(2.5)
  fmt.Println(ZERO, ONE)
}
"#
    );
}

#[test]
fn out_of_domain_uintptr_domain() {
    assert_lint_snapshot!(
        r#"
import "go:fmt"

#[go(closed_domain)]
pub struct P(uintptr)

pub const A: P = 1
pub const B: P = 2

pub fn at(p: P) -> P { p }

fn main() {
  let _ = at(3)
  fmt.Println(A, B)
}
"#
    );
}

#[test]
fn out_of_domain_rune_escape_member_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"

#[go(closed_domain)]
pub struct R(rune)

pub const BEL: R = '\a'
pub const TAB: R = '\t'

pub fn at(r: R) -> R { r }

fn main() {
  let _ = at(7)
  let _ = at('\a')
  fmt.Println(BEL, TAB)
}
"#
    );
}

#[test]
fn out_of_domain_rune_escape_domain() {
    assert_lint_snapshot!(
        r#"
import "go:fmt"

#[go(closed_domain)]
pub struct R(rune)

pub const BEL: R = '\a'
pub const TAB: R = '\t'

pub fn at(r: R) -> R { r }

fn main() {
  let _ = at('\b')
  fmt.Println(BEL, TAB)
}
"#
    );
}

#[test]
fn redundant_slice_bounds_upper() {
    assert_lint_snapshot!(
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
fn redundant_slice_bounds_lower() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4, 5]
  let b = 3
  let _ = xs[0..b]
}
"#
    );
}

#[test]
fn redundant_slice_bounds_lower_inclusive() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4, 5]
  let b = 3
  let _ = xs[0..=b]
}
"#
    );
}

#[test]
fn redundant_slice_bounds_full_reslice_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4, 5]
  let _ = xs[..]
}
"#
    );
}

#[test]
fn redundant_slice_bounds_open_lower_zero_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4, 5]
  let _ = xs[0..]
}
"#
    );
}

#[test]
fn redundant_slice_bounds_open_upper_length_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4, 5]
  let _ = xs[..xs.length()]
}
"#
    );
}

#[test]
fn redundant_slice_bounds_both_default_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4, 5]
  let _ = xs[0..xs.length()]
}
"#
    );
}

#[test]
fn redundant_slice_bounds_meaningful_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4, 5]
  let a = 2
  let b = 3
  let _ = xs[a..b]
}
"#
    );
}

#[test]
fn redundant_slice_bounds_different_receiver_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4, 5]
  let ys = [9, 8, 7]
  let a = 2
  let _ = xs[a..ys.length()]
}
"#
    );
}

#[test]
fn redundant_slice_bounds_inclusive_length_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4, 5]
  let a = 2
  let _ = xs[a..=xs.length()]
}
"#
    );
}

#[test]
fn redundant_slice_bounds_side_effecting_receiver_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn get_slice() -> Slice<int> {
  [1, 2, 3]
}

fn main() {
  let a = 1
  let _ = get_slice()[a..get_slice().length()]
}
"#
    );
}

#[test]
fn redundant_slice_bounds_side_effecting_start_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"

fn noisy() -> int {
  fmt.Println("start")
  1
}

fn main() {
  let xs = [1, 2, 3, 4, 5]
  let _ = xs[noisy()..xs.length()]
}
"#
    );
}

#[test]
fn manual_find() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4]
  let _ = xs.filter(|x| x > 2).get(0)
}
"#
    );
}

#[test]
fn manual_find_equality_predicate() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4]
  let _ = xs.filter(|x| x == 2).get(0)
}
"#
    );
}

#[test]
fn manual_find_field_access_predicate() {
    assert_lint_snapshot!(
        r#"
struct User {
  age: int
}

fn main() {
  let users = [User { age: 17 }, User { age: 21 }]
  let _ = users.filter(|u| u.age > 18).get(0)
}
"#
    );
}

#[test]
fn manual_find_bare_function_predicate_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn is_pos(x: int) -> bool {
  x > 0
}

fn main() {
  let xs = [1, 2, 3, 4]
  let _ = xs.filter(is_pos).get(0)
}
"#
    );
}

#[test]
fn manual_find_index_not_zero_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4]
  let _ = xs.filter(|x| x > 2).get(1)
}
"#
    );
}

#[test]
fn manual_find_plain_get_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4]
  let _ = xs.get(0)
}
"#
    );
}

#[test]
fn manual_find_map_not_filter_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4]
  let _ = xs.map(|x| x * 2).get(0)
}
"#
    );
}

#[test]
fn manual_find_user_type_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Bag {
  items: Slice<int>
}

impl Bag {
  fn filter(self, p: fn(int) -> bool) -> Bag {
    Bag { items: self.items.filter(p) }
  }

  fn get(self, index: int) -> Option<int> {
    self.items.get(index)
  }
}

fn main() {
  let b = Bag { items: [1, 2, 3] }
  let _ = b.filter(|x| x > 1).get(0)
}
"#
    );
}

#[test]
fn manual_find_already_using_find_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4]
  let _ = xs.find(|x| x > 2)
}
"#
    );
}

#[test]
fn manual_find_effectful_predicate_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"

fn main() {
  let xs = [1, 2, 3, 4]
  let _ = xs.filter(|x| {
    fmt.Println(x)
    x > 2
  }).get(0)
}
"#
    );
}

#[test]
fn manual_find_dividing_predicate_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4]
  let _ = xs.filter(|x| 10 % x == 0).get(0)
}
"#
    );
}

#[test]
fn manual_find_interpolated_fstring_predicate_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4]
  let _ = xs.filter(|x| f"{x}" == "1").get(0)
}
"#
    );
}

#[test]
fn manual_find_shifting_predicate_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4]
  let _ = xs.filter(|x| (1 << x) > 0).get(0)
}
"#
    );
}

#[test]
fn manual_find_ref_element_predicate_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct User {
  age: int
}

fn main() {
  let a = User { age: 17 }
  let b = User { age: 21 }
  let refs = [&a, &b]
  let _ = refs.filter(|u| u.age > 18).get(0)
}
"#
    );
}

#[test]
fn manual_find_interface_equality_predicate_no_warning() {
    assert_no_lint_warnings!(
        r#"
interface Animal {
  fn sound(self) -> string
}

struct Dog {}

impl Dog {
  fn sound(self) -> string {
    "woof"
  }
}

fn main() {
  let d1 = Dog {}
  let animals: Slice<Animal> = [d1]
  let target: Animal = d1
  let _ = animals.filter(|x| x == target).get(0)
}
"#
    );
}

#[test]
fn manual_contains() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4]
  let _ = xs.any(|x| x == 3)
}
"#
    );
}

#[test]
fn manual_contains_value_on_left() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4]
  let _ = xs.any(|x| 3 == x)
}
"#
    );
}

#[test]
fn manual_contains_string_slice() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let names = ["a", "b"]
  let _ = names.any(|s| s == "b")
}
"#
    );
}

#[test]
fn manual_contains_enum_slice() {
    assert_lint_snapshot!(
        r#"
enum Color {
  Red,
  Green,
  Blue,
}

fn main() {
  let cs = [Color.Red, Color.Green]
  let _ = cs.any(|c| c == Color.Blue)
}
"#
    );
}

#[test]
fn manual_contains_not_equal_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4]
  let _ = xs.any(|x| x != 3)
}
"#
    );
}

#[test]
fn manual_contains_relational_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4]
  let _ = xs.any(|x| x > 3)
}
"#
    );
}

#[test]
fn manual_contains_field_predicate_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct User {
  age: int
}

fn main() {
  let users = [User { age: 17 }, User { age: 21 }]
  let _ = users.any(|u| u.age == 18)
}
"#
    );
}

#[test]
fn manual_contains_value_uses_element_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4]
  let _ = xs.any(|x| x == x + 1)
}
"#
    );
}

#[test]
fn manual_contains_effectful_value_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn cost() -> int {
  7
}

fn main() {
  let xs = [1, 2, 3, 4]
  let _ = xs.any(|x| x == cost())
}
"#
    );
}

#[test]
fn manual_contains_field_value() {
    assert_lint_snapshot!(
        r#"
struct Config {
  target: int
}

fn main() {
  let cfg = Config { target: 3 }
  let xs = [1, 2, 3, 4]
  let _ = xs.any(|x| x == cfg.target)
}
"#
    );
}

#[test]
fn manual_contains_dividing_value_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4]
  let denom = 2
  let _ = xs.any(|x| x == 10 / denom)
}
"#
    );
}

#[test]
fn manual_contains_interpolated_fstring_value_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let names = ["a", "b"]
  let n = 1
  let _ = names.any(|s| s == f"{n}")
}
"#
    );
}

#[test]
fn manual_contains_mismatched_int_type_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4]
  let small: int8 = 2
  let _ = xs.any(|x| x == small)
}
"#
    );
}

#[test]
fn manual_contains_interface_equality_no_warning() {
    assert_no_lint_warnings!(
        r#"
interface Animal {
  fn sound(self) -> string
}

struct Dog {}

impl Dog {
  fn sound(self) -> string {
    "woof"
  }
}

fn main() {
  let d = Dog {}
  let animals: Slice<Animal> = [d]
  let target: Animal = d
  let _ = animals.any(|x| x == target)
}
"#
    );
}

#[test]
fn manual_contains_user_type_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Bag {
  items: Slice<int>
}

impl Bag {
  fn any(self, p: fn(int) -> bool) -> bool {
    self.items.any(p)
  }
}

fn main() {
  let b = Bag { items: [1, 2, 3] }
  let _ = b.any(|x| x == 1)
}
"#
    );
}

#[test]
fn manual_contains_all_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4]
  let _ = xs.all(|x| x == 3)
}
"#
    );
}

#[test]
fn manual_contains_already_contains_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4]
  let _ = xs.contains(3)
}
"#
    );
}

#[test]
fn unnecessary_first_then_check_is_some() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4]
  let _ = xs.get(0).is_some()
}
"#
    );
}

#[test]
fn unnecessary_first_then_check_is_none() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4]
  let _ = xs.get(0).is_none()
}
"#
    );
}

#[test]
fn unnecessary_first_then_check_field_receiver() {
    assert_lint_snapshot!(
        r#"
struct Holder {
  items: Slice<int>
}

fn main() {
  let h = Holder { items: [1, 2] }
  let _ = h.items.get(0).is_some()
}
"#
    );
}

#[test]
fn unnecessary_first_then_check_index_not_zero_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4]
  let _ = xs.get(1).is_some()
}
"#
    );
}

#[test]
fn unnecessary_first_then_check_map_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let m = Map.new<int, string>()
  let _ = m.get(0).is_some()
}
"#
    );
}

#[test]
fn unnecessary_first_then_check_user_type_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Bag {
  items: Slice<int>
}

impl Bag {
  fn get(self, i: int) -> Option<int> {
    self.items.get(i)
  }
}

fn main() {
  let b = Bag { items: [1, 2, 3] }
  let _ = b.get(0).is_some()
}
"#
    );
}

#[test]
fn unnecessary_first_then_check_plain_get_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4]
  let _ = xs.get(0)
}
"#
    );
}

#[test]
fn unnecessary_first_then_check_already_is_empty_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let xs = [1, 2, 3, 4]
  let _ = xs.is_empty()
}
"#
    );
}

#[test]
fn qualified_tuple_variant_pattern_marks_import_used() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "players",
        "lib.lis",
        r#"
pub enum LoadDecision {
  Created(int),
  Existing(int),
}
"#,
    );
    fs.add_file(
        "service",
        "lib.lis",
        r#"
import "players"

pub fn load() -> players.LoadDecision {
  players.LoadDecision.Created(1)
}
"#,
    );
    let source = r#"
import "players"
import "service"

fn main() {
  match service.load() {
    players.LoadDecision.Created(n) => { let _ = n },
    players.LoadDecision.Existing(n) => { let _ = n },
  }
}
"#;
    fs.add_file(ENTRY_MODULE_ID, "main.lis", source);

    let result = compile_check(fs);
    assert!(
        result.errors.is_empty(),
        "unexpected errors: {:?}",
        result.errors
    );
    assert!(
        result.lints.is_empty(),
        "import used only in a qualified tuple-variant pattern should produce no lints: {:?}",
        result.lints
    );
}

#[test]
fn qualified_struct_variant_pattern_marks_import_used() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "players",
        "lib.lis",
        r#"
pub enum LoadDecision {
  Created { player: int },
  Existing { id: int },
}
"#,
    );
    fs.add_file(
        "service",
        "lib.lis",
        r#"
import "players"

pub fn load() -> players.LoadDecision {
  players.LoadDecision.Created { player: 1 }
}
"#,
    );
    let source = r#"
import "players"
import "service"

fn main() {
  match service.load() {
    players.LoadDecision.Created { player } => { let _ = player },
    players.LoadDecision.Existing { id } => { let _ = id },
  }
}
"#;
    fs.add_file(ENTRY_MODULE_ID, "main.lis", source);

    let result = compile_check(fs);
    assert!(
        result.errors.is_empty(),
        "unexpected errors: {:?}",
        result.errors
    );
    assert!(
        result.lints.is_empty(),
        "import used only in a qualified struct-variant pattern should produce no lints: {:?}",
        result.lints
    );
}

#[test]
fn qualified_struct_pattern_marks_import_used() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "models",
        "lib.lis",
        r#"
pub struct Point {
  pub x: int,
  pub y: int,
}
"#,
    );
    fs.add_file(
        "service",
        "lib.lis",
        r#"
import "models"

pub fn origin() -> models.Point {
  models.Point { x: 0, y: 0 }
}
"#,
    );
    let source = r#"
import "models"
import "service"

fn main() {
  match service.origin() {
    models.Point { x, y } => { let _ = x + y },
  }
}
"#;
    fs.add_file(ENTRY_MODULE_ID, "main.lis", source);

    let result = compile_check(fs);
    assert!(
        result.errors.is_empty(),
        "unexpected errors: {:?}",
        result.errors
    );
    assert!(
        result.lints.is_empty(),
        "import used only in a qualified struct pattern should produce no lints: {:?}",
        result.lints
    );
}

#[test]
fn qualified_imported_pattern_does_not_suppress_local_collisions() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "models",
        "lib.lis",
        r#"
pub struct Model {
  pub value: int,
}
"#,
    );
    let source = r#"
import "models"

enum Model {
  Model,
  Other,
}

fn describe(m: models.Model) {
  match m {
    models.Model { value } => { let _ = value },
  }
}

fn main() {
  describe(models.Model { value: 1 })
}
"#;
    fs.add_file(ENTRY_MODULE_ID, "main.lis", source);

    let result = compile_check(fs);
    assert!(
        result.errors.is_empty(),
        "unexpected errors: {:?}",
        result.errors
    );
    let codes: Vec<&str> = result.lints.iter().filter_map(|l| l.code_str()).collect();
    assert!(
        !codes.contains(&"lint.unused_import"),
        "models import is used in the pattern and must not be flagged: {codes:?}"
    );
    assert!(
        codes.contains(&"lint.unused_type"),
        "local enum Model is unused and must still be flagged: {codes:?}"
    );
    assert_eq!(
        codes
            .iter()
            .filter(|c| **c == "lint.unused_enum_variant")
            .count(),
        2,
        "both local variants Model and Other are unused and must still be flagged: {codes:?}"
    );
}

#[test]
fn imported_enum_construction_does_not_suppress_same_named_local_type() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "tvcall",
        "lib.lis",
        r#"
pub enum Tv {
  A(int),
  B,
}
"#,
    );
    let source = r#"
import "tvcall"

enum Tv {
  A(int),
  B,
}

fn main() {
  let _a = tvcall.Tv.A(1)
}
"#;
    fs.add_file(ENTRY_MODULE_ID, "main.lis", source);

    let result = compile_check(fs);
    assert!(
        result.errors.is_empty(),
        "unexpected errors: {:?}",
        result.errors
    );
    let codes: Vec<&str> = result.lints.iter().filter_map(|l| l.code_str()).collect();
    assert!(
        !codes.contains(&"lint.unused_import"),
        "tvcall import is used and must not be flagged: {codes:?}"
    );
    assert!(
        codes.contains(&"lint.unused_type"),
        "local enum Tv is unused and must still be flagged: {codes:?}"
    );
    assert_eq!(
        codes
            .iter()
            .filter(|c| **c == "lint.unused_enum_variant")
            .count(),
        2,
        "both local variants A and B are unused and must still be flagged: {codes:?}"
    );
}

#[test]
fn imported_enum_variant_construction_does_not_suppress_same_named_local_const() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "tvcall",
        "lib.lis",
        r#"
pub enum Tv {
  A(int),
  B,
}
"#,
    );
    let source = r#"
import "tvcall"

const A: int = 1

fn main() {
  let _a = tvcall.Tv.A(1)
}
"#;
    fs.add_file(ENTRY_MODULE_ID, "main.lis", source);

    let result = compile_check(fs);
    assert!(
        result.errors.is_empty(),
        "unexpected errors: {:?}",
        result.errors
    );
    let codes: Vec<&str> = result.lints.iter().filter_map(|l| l.code_str()).collect();
    assert!(
        !codes.contains(&"lint.unused_import"),
        "tvcall import is used and must not be flagged: {codes:?}"
    );
    assert!(
        codes.contains(&"lint.unused_constant"),
        "local const A is unused and must still be flagged: {codes:?}"
    );
}

#[test]
fn imported_const_access_does_not_suppress_local_collision() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "limits",
        "lib.lis",
        r#"
pub const MAX: int = 100
"#,
    );
    let source = r#"
import "limits"

const MAX: int = 1

fn main() {
  let _m = limits.MAX
}
"#;
    fs.add_file(ENTRY_MODULE_ID, "main.lis", source);

    let result = compile_check(fs);
    assert!(
        result.errors.is_empty(),
        "unexpected errors: {:?}",
        result.errors
    );
    let codes: Vec<&str> = result.lints.iter().filter_map(|l| l.code_str()).collect();
    assert!(
        !codes.contains(&"lint.unused_import"),
        "limits import is used and must not be flagged: {codes:?}"
    );
    assert!(
        codes.contains(&"lint.unused_constant"),
        "local const MAX is unused and must still be flagged: {codes:?}"
    );
}

#[test]
fn imported_method_call_does_not_suppress_same_named_local_function() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "foreign",
        "lib.lis",
        r#"
pub struct T {
  pub n: int,
}

impl T {
  pub fn m(self: T) -> int {
    self.n
  }
  pub fn mk() -> T {
    T { n: 1 }
  }
}

pub fn make() -> T {
  T { n: 2 }
}
"#,
    );
    let source = r#"
import "foreign"

fn m() -> int {
  0
}

fn mk() -> int {
  0
}

fn main() {
  let _a = foreign.make().m()
  let _b = foreign.T.mk()
}
"#;
    fs.add_file(ENTRY_MODULE_ID, "main.lis", source);

    let result = compile_check(fs);
    assert!(
        result.errors.is_empty(),
        "unexpected errors: {:?}",
        result.errors
    );
    let codes: Vec<&str> = result.lints.iter().filter_map(|l| l.code_str()).collect();
    assert!(
        !codes.contains(&"lint.unused_import"),
        "foreign import is used and must not be flagged: {codes:?}"
    );
    assert_eq!(
        codes
            .iter()
            .filter(|c| **c == "lint.unused_function")
            .count(),
        2,
        "local fns m and mk are unused and must still be flagged: {codes:?}"
    );
}

#[test]
fn local_method_call_is_still_credited() {
    let source = r#"
struct Counter {
  value: int,
}

impl Counter {
  fn get(self: Counter) -> int {
    self.value
  }
}

fn main() {
  let c = Counter { value: 1 };
  let _ = c.get()
}
"#;
    assert_no_lint_warnings!(source);
}

#[test]
fn imported_tuple_struct_static_method_does_not_suppress_same_named_local_function() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "foreign",
        "lib.lis",
        r#"
pub struct T(int)

impl T {
  pub fn mk() -> T {
    T(1)
  }
}
"#,
    );
    let source = r#"
import "foreign"

fn mk() -> int {
  2
}

fn main() {
  let _b = foreign.T.mk()
}
"#;
    fs.add_file(ENTRY_MODULE_ID, "main.lis", source);

    let result = compile_check(fs);
    assert!(
        result.errors.is_empty(),
        "unexpected errors: {:?}",
        result.errors
    );
    let codes: Vec<&str> = result.lints.iter().filter_map(|l| l.code_str()).collect();
    assert!(
        !codes.contains(&"lint.unused_import"),
        "foreign import is used and must not be flagged: {codes:?}"
    );
    assert!(
        codes.contains(&"lint.unused_function"),
        "local fn mk is unused and must still be flagged: {codes:?}"
    );
}

#[test]
fn local_tuple_struct_static_method_is_still_credited() {
    let source = r#"
struct Wrap(int)

impl Wrap {
  fn make() -> Wrap {
    Wrap(1)
  }
}

fn main() {
  let _w = Wrap.make()
}
"#;
    assert_no_lint_warnings!(source);
}

#[test]
fn builtin_method_call_does_not_suppress_same_named_local_function() {
    let source = r#"
fn contains() -> int {
  0
}

fn main() {
  let s = "hello";
  let _ = s.contains("e")
}
"#;
    let warnings = crate::_harness::lint::lint(source);
    let codes: Vec<&str> = warnings.iter().filter_map(|w| w.code_str()).collect();
    assert!(
        codes.contains(&"lint.unused_function"),
        "local fn contains is unused and must still be flagged: {codes:?}"
    );
}

#[test]
fn generic_call_with_imported_interface_bound_does_not_suppress_local_function() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "shapes",
        "lib.lis",
        r#"
pub interface Area {
  fn area(self) -> int
}
"#,
    );
    let source = r#"
import "shapes"

fn area() -> int {
  0
}

pub fn total<T: shapes.Area>(x: T) -> int {
  x.area()
}

fn main() {
  ()
}
"#;
    fs.add_file(ENTRY_MODULE_ID, "main.lis", source);

    let result = compile_check(fs);
    assert!(
        result.errors.is_empty(),
        "unexpected errors: {:?}",
        result.errors
    );
    let codes: Vec<&str> = result.lints.iter().filter_map(|l| l.code_str()).collect();
    assert!(
        !codes.contains(&"lint.unused_import"),
        "shapes import is used in the bound and must not be flagged: {codes:?}"
    );
    assert!(
        codes.contains(&"lint.unused_function"),
        "local fn area is unused and must still be flagged: {codes:?}"
    );
}

#[test]
fn imported_bound_with_same_named_local_interface_does_not_suppress_local_function() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "shapes",
        "lib.lis",
        r#"
pub interface Area {
  fn area(self) -> int
}
"#,
    );
    let source = r#"
import "shapes"

interface LocalArea {
  fn area(self) -> int
}

fn area() -> int {
  0
}

pub fn total<T: shapes.Area>(x: T) -> int {
  x.area()
}

fn main() {
  ()
}
"#;
    fs.add_file(ENTRY_MODULE_ID, "main.lis", source);

    let result = compile_check(fs);
    assert!(
        result.errors.is_empty(),
        "unexpected errors: {:?}",
        result.errors
    );
    let codes: Vec<&str> = result.lints.iter().filter_map(|l| l.code_str()).collect();
    assert!(
        !codes.contains(&"lint.unused_import"),
        "shapes import is used in the bound and must not be flagged: {codes:?}"
    );
    assert!(
        codes.contains(&"lint.unused_function"),
        "local fn area is unused and must still be flagged even though local LocalArea declares area: {codes:?}"
    );
    assert!(
        codes.contains(&"lint.unused_type"),
        "local interface LocalArea is unused and must still be flagged: {codes:?}"
    );
}

#[test]
fn float_cmp_equal() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let f: float64 = 1.0
  let g: float64 = 2.0
  let _ = f == g
}
"#
    );
}

#[test]
fn float_cmp_not_equal() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let f: float64 = 1.0
  let g: float64 = 2.0
  let _ = f != g
}
"#
    );
}

#[test]
fn float_cmp_literal() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let f: float64 = 1.0
  let _ = f == 1.5
}
"#
    );
}

#[test]
fn float_cmp_float32() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let f: float32 = 1.0
  let g: float32 = 2.0
  let _ = f == g
}
"#
    );
}

#[test]
fn float_cmp_zero_no_diagnostic() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let f: float64 = 1.0
  let _ = f == 0.0
}
"#
    );
}

#[test]
fn float_cmp_integer_zero_no_diagnostic() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let f: float64 = 1.0
  let _ = f == 0
}
"#
    );
}

#[test]
fn float_cmp_infinity_no_diagnostic() {
    assert_no_lint_warnings!(
        r#"
import "go:math"
fn main() {
  let f: float64 = 1.0
  let _ = f == math.Inf(1)
}
"#
    );
}

#[test]
fn float_cmp_self_no_diagnostic() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let f: float64 = 1.0
  let _ = f != f
}
"#
    );
}

#[test]
fn float_cmp_integer_no_diagnostic() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let n: int = 3
  let m: int = 4
  let _ = n == m
}
"#
    );
}

#[test]
fn float_cmp_defers_nan_to_nan_comparison() {
    let diagnostics = crate::_harness::lint::lint(
        r#"
import "go:math"
fn main() {
  let f: float64 = 1.0
  let _ = f == math.NaN()
}
"#,
    );
    let codes: Vec<&str> = diagnostics.iter().filter_map(|d| d.code_str()).collect();
    assert!(
        codes.contains(&"infer.nan_comparison"),
        "nan_comparison must own the NaN comparison: {codes:?}"
    );
    assert!(
        !codes.contains(&"lint.float_cmp"),
        "float_cmp must not double-report on a NaN comparison: {codes:?}"
    );
}

#[test]
fn float_equality_without_abs_less_than() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let f: float64 = 1.0
  let g: float64 = 2.0
  let _ = (f - g) < 0.0001
}
"#
    );
}

#[test]
fn float_equality_without_abs_margin_on_left() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let f: float64 = 1.0
  let g: float64 = 2.0
  let _ = 0.5 > (f - g)
}
"#
    );
}

#[test]
fn float_equality_without_abs_difference_large_no_diagnostic() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let f: float64 = 1.0
  let g: float64 = 2.0
  let _ = (f - g) > 0.0001
}
"#
    );
}

#[test]
fn float_equality_without_abs_variable_margin_no_diagnostic() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let f: float64 = 1.0
  let g: float64 = 2.0
  let _ = (f - g) < g
}
"#
    );
}

#[test]
fn float_equality_without_abs_integer_no_diagnostic() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let n: int = 3
  let m: int = 4
  let _ = (n - m) < 5
}
"#
    );
}

#[test]
fn cast_nan_to_int_signed() {
    assert_lint_snapshot!(
        r#"
import "go:math"
fn main() {
  let _ = math.NaN() as int
}
"#
    );
}

#[test]
fn cast_nan_to_int_unsigned() {
    assert_lint_snapshot!(
        r#"
import "go:math"
fn main() {
  let _ = math.NaN() as uint8
}
"#
    );
}

#[test]
fn cast_nan_to_int_float_target_no_diagnostic() {
    assert_no_lint_warnings!(
        r#"
import "go:math"
fn main() {
  let _ = math.NaN() as float32
}
"#
    );
}

#[test]
fn cast_nan_to_int_other_value_no_diagnostic() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let f: float64 = 1.5
  let _ = f as int
}
"#
    );
}

#[test]
fn float_cmp_repeated_call() {
    assert_lint_snapshot!(
        r#"
fn value() -> float64 { 1.0 }
fn main() {
  let _ = value() == value()
}
"#
    );
}

#[test]
fn float_cmp_alias() {
    assert_lint_snapshot!(
        r#"
type Score = float64
fn main() {
  let a: Score = 1.0
  let b: Score = 2.0
  let _ = a == b
}
"#
    );
}

#[test]
fn float_cmp_newtype() {
    assert_lint_snapshot!(
        r#"
struct Distance(float64)
fn main() {
  let x = Distance(1.0)
  let y = Distance(2.0)
  let _ = x == y
}
"#
    );
}

#[test]
fn float_cmp_negative_zero_no_diagnostic() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let f: float64 = 1.0
  let _ = f == -0.0
  let _ = f == (-0)
}
"#
    );
}

#[test]
fn float_cmp_distinct_newtypes_no_diagnostic() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
struct Distance(float64)
struct Speed(float64)
fn main() {
  let d = Distance(1.0)
  let s = Speed(2.0)
  let _ = d == s
}
"#,
    );
    let result = compile_check(fs);
    assert!(
        !result
            .lints
            .iter()
            .any(|d| d.plain_message() == "Exact float comparison"),
        "float_cmp must not fire on a checker-rejected newtype comparison: {:?}",
        result.lints
    );
}

#[test]
fn float_equality_without_abs_less_than_or_equal() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let f: float64 = 1.0
  let g: float64 = 2.0
  let _ = (f - g) <= 0.0001
}
"#
    );
}

#[test]
fn float_equality_without_abs_greater_than_or_equal() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let f: float64 = 1.0
  let g: float64 = 2.0
  let _ = 0.0001 >= (f - g)
}
"#
    );
}

#[test]
fn float_equality_without_abs_integer_margin() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let f: float64 = 1.0
  let g: float64 = 2.0
  let _ = (f - g) < 1
}
"#
    );
}

#[test]
fn float_equality_without_abs_alias() {
    assert_lint_snapshot!(
        r#"
type Score = float64
fn main() {
  let a: Score = 1.0
  let b: Score = 2.0
  let _ = (a - b) < 0.001
}
"#
    );
}

#[test]
fn cast_nan_to_int_alias() {
    assert_lint_snapshot!(
        r#"
import "go:math"
type Count = int
fn main() {
  let _ = math.NaN() as Count
}
"#
    );
}

#[test]
fn cast_nan_to_int_uintptr() {
    assert_lint_snapshot!(
        r#"
import "go:math"
fn main() {
  let _ = math.NaN() as uintptr
}
"#
    );
}

#[test]
fn cast_nan_to_int_newtype() {
    assert_lint_snapshot!(
        r#"
import "go:math"
struct CountNew(int)
fn main() {
  let _ = math.NaN() as CountNew
}
"#
    );
}

#[test]
fn float_cmp_distinct_aliases() {
    assert_lint_snapshot!(
        r#"
type A = float64
type B = float64
fn main() {
  let a: A = 1.0
  let b: B = 2.0
  let _ = a == b
}
"#
    );
}

#[test]
fn float_cmp_alias_chain() {
    assert_lint_snapshot!(
        r#"
type A = float64
type B = A
fn main() {
  let a: A = 1.0
  let b: B = 2.0
  let _ = a == b
}
"#
    );
}

#[test]
fn float_equality_without_abs_distinct_aliases() {
    assert_lint_snapshot!(
        r#"
type A = float64
type B = float64
fn main() {
  let a: A = 1.0
  let b: B = 2.0
  let _ = (a - b) < 0.01
}
"#
    );
}

#[test]
fn float_equality_without_abs_alias_chain() {
    assert_lint_snapshot!(
        r#"
type A = float64
type B = A
fn main() {
  let a: A = 1.0
  let b: B = 2.0
  let _ = (a - b) < 0.01
}
"#
    );
}

#[test]
fn float_equality_without_abs_distinct_newtypes_no_diagnostic() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
struct Distance(float64)
struct Speed(float64)
fn main() {
  let d = Distance(1.0)
  let s = Speed(2.0)
  let _ = (d - s) < 0.01
}
"#,
    );
    let result = compile_check(fs);
    assert!(
        !result
            .lints
            .iter()
            .any(|d| d.plain_message() == "Float equality without `abs`"),
        "must not fire on a checker-rejected newtype subtraction: {:?}",
        result.lints
    );
}

#[test]
fn float_cmp_alias_to_newtype() {
    assert_lint_snapshot!(
        r#"
type A = float64
struct Distance(float64)
fn main() {
  let a: A = 1.0
  let d = Distance(2.0)
  let _ = a == d
}
"#
    );
}

#[test]
fn float_cmp_newtype_alias_symmetric() {
    let diagnostics = crate::_harness::lint::lint(
        r#"
struct Distance(float64)
type DA = Distance
fn main() {
  let d = Distance(1.0)
  let da: DA = Distance(2.0)
  let _ = d == da
  let _ = da == d
}
"#,
    );
    let count = diagnostics
        .iter()
        .filter(|d| d.code_str() == Some("lint.float_cmp"))
        .count();
    assert_eq!(
        count, 2,
        "both newtype/alias orderings are accepted by the checker and must fire: {diagnostics:?}"
    );
}

#[test]
fn float_cmp_plain_vs_newtype_no_diagnostic() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
struct Distance(float64)
fn main() {
  let f: float64 = 1.0
  let d = Distance(2.0)
  let _ = f == d
}
"#,
    );
    let result = compile_check(fs);
    assert!(
        !result
            .lints
            .iter()
            .any(|d| d.plain_message() == "Exact float comparison"),
        "must not fire on a checker-rejected plain-vs-newtype comparison: {:?}",
        result.lints
    );
}

#[test]
fn float_equality_without_abs_rejected_newtype_subtraction_no_diagnostic() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
struct Distance(float64)
type DA = Distance
fn main() {
  let d = Distance(1.0)
  let da: DA = Distance(2.0)
  let _ = (d - da) < 0.01
}
"#,
    );
    let result = compile_check(fs);
    assert!(
        !result
            .lints
            .iter()
            .any(|d| d.plain_message() == "Float equality without `abs`"),
        "must not fire when the checker rejected the subtraction: {:?}",
        result.lints
    );
}

#[test]
fn min_max_min_over_max() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x: int = 3
  let _ = min(max(x, 5), 1)
}
"#
    );
}

#[test]
fn min_max_max_over_min() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x: int = 3
  let _ = max(min(x, 1), 5)
}
"#
    );
}

#[test]
fn min_max_equal_bounds() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x: int = 3
  let _ = min(max(x, 5), 5)
}
"#
    );
}

#[test]
fn min_max_float_integer_literals_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let g: float64 = 1.5
  let _ = min(max(g, 5), 1)
}
"#
    );
}

#[test]
fn min_max_literal_on_left() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x: int = 3
  let _ = min(1, max(5, x))
}
"#
    );
}

#[test]
fn min_max_valid_clamp_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x: int = 3
  let _ = max(min(x, 5), 1)
}
"#
    );
}

#[test]
fn min_max_inner_clamp_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x: int = 3
  let _ = min(max(x, 1), 5)
}
"#
    );
}

#[test]
fn min_max_same_op_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let x: int = 3
  let _ = min(min(x, 5), 1)
}
"#
    );
}

#[test]
fn min_max_float_literal_bounds_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let g: float64 = 1.5
  let _ = min(max(g, 5.0), 1.0)
}
"#
    );
}

#[test]
fn min_max_shadowed_min_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let min = |a: int, b: int| a + b
  let x: int = 3
  let _ = min(max(x, 5), 1)
}
"#
    );
}

#[test]
fn almost_swapped() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let mut a = 1
  let mut b = 2
  a = b
  b = a
  let _ = a + b
}
"#
    );
}

#[test]
fn almost_swapped_mutable_params() {
    assert_lint_snapshot!(
        r#"
fn swap(mut a: int, mut b: int) -> int {
  a = b
  b = a
  a + b
}

fn main() {
  let _ = swap(1, 2)
}
"#
    );
}

#[test]
fn almost_swapped_suppressed_by_allow() {
    assert_no_lint_warnings!(
        r#"
#[allow(almost_swapped)]
fn main() {
  let mut a = 1
  let mut b = 2
  a = b
  b = a
  let _ = a + b
}
"#
    );
}

#[test]
fn almost_swapped_in_try_block() {
    assert_lint_snapshot!(
        r#"
fn risky() -> Result<int, string> {
  Ok(1)
}

fn compute() -> Result<int, string> {
  try {
    let mut a = risky()?
    let mut b = risky()?
    a = b
    b = a
    a + b
  }
}

fn main() {
  let _ = compute()
}
"#
    );
}

#[test]
fn almost_swapped_third_variable_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let mut a = 1
  let mut b = 2
  let c = 3
  a = b
  b = c
  let _ = a + b
}
"#
    );
}

#[test]
fn almost_swapped_repeated_copy_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let mut a = 1
  let b = 2
  a = b
  a = b
  let _ = a + b
}
"#
    );
}

#[test]
fn almost_swapped_compound_assignment_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let mut a = 1
  let mut b = 2
  a += b
  b += a
  let _ = a + b
}
"#
    );
}

#[test]
fn almost_swapped_non_adjacent_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let mut a = 1
  let mut b = 2
  a = b
  let _ = a
  b = a
  let _ = b
}
"#
    );
}

#[test]
fn almost_swapped_field_access_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Point { x: int, y: int }

fn main() {
  let mut p = Point { x: 1, y: 2 }
  p.x = p.y
  p.y = p.x
  let _ = p.x + p.y
}
"#
    );
}

#[test]
fn unnecessary_min_or_max_identical_min() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x: int = 3
  let _ = min(x, x)
}
"#
    );
}

#[test]
fn unnecessary_min_or_max_identical_max() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x: int = 3
  let _ = max(x, x)
}
"#
    );
}

#[test]
fn unnecessary_min_or_max_uint8_floor() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let b: uint8 = 5
  let _ = max(b, 0)
}
"#
    );
}

#[test]
fn unnecessary_min_or_max_uint8_ceil() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let b: uint8 = 5
  let _ = min(b, 255)
}
"#
    );
}

#[test]
fn unnecessary_min_or_max_int8_floor() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let c: int8 = 5
  let _ = max(c, -128)
}
"#
    );
}

#[test]
fn unnecessary_min_or_max_identical_string() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let s: string = "a"
  let _ = min(s, s)
}
"#
    );
}

#[test]
fn unnecessary_min_or_max_side_effect_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn f() -> int { 3 }
fn main() {
  let _ = min(f(), f())
}
"#
    );
}

#[test]
fn unnecessary_min_or_max_distinct_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let a: int = 1
  let b: int = 2
  let _ = min(a, b)
}
"#
    );
}

#[test]
fn unnecessary_min_or_max_signed_floor_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let a: int = 1
  let _ = max(a, 0)
}
"#
    );
}

#[test]
fn unnecessary_min_or_max_below_ceil_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let b: uint8 = 5
  let _ = min(b, 254)
}
"#
    );
}

#[test]
fn unnecessary_min_or_max_shadowed_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let min = |a: int, b: int| a + b
  let _ = min(7, 7)
}
"#
    );
}

#[test]
fn manual_filter_option() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = match o {
    Some(x) if x > 0 => Some(x),
    _ => None,
  }
}
"#
    );
}

#[test]
fn manual_filter_with_map_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = match o {
    Some(x) if x > 0 => Some(x * 2),
    _ => None,
  }
}
"#
    );
}

#[test]
fn manual_filter_non_none_default_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = match o {
    Some(x) if x > 0 => Some(x),
    _ => Some(0),
  }
}
"#
    );
}

#[test]
fn manual_ok_or_option() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = match o {
    Some(x) => Ok(x),
    None => Err("missing"),
  }
}
"#
    );
}

#[test]
fn manual_ok_or_else_lazy() {
    assert_lint_snapshot!(
        r#"
fn make_err() -> string {
  "boom"
}

fn main() {
  let o: Option<int> = Some(1)
  let _ = match o {
    Some(x) => Ok(x),
    None => Err(make_err()),
  }
}
"#
    );
}

#[test]
fn manual_ok_or_reversed_arms() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = match o {
    None => Err("missing"),
    Some(x) => Ok(x),
  }
}
"#
    );
}

#[test]
fn manual_ok_or_mapped_ok_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = match o {
    Some(x) => Ok(x * 2),
    None => Err("e"),
  }
}
"#
    );
}

#[test]
fn manual_ok_err_ok() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let r: Result<int, string> = Ok(1)
  let _ = match r {
    Ok(x) => Some(x),
    Err(_) => None,
  }
}
"#
    );
}

#[test]
fn manual_ok_err_err() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let r: Result<int, string> = Ok(1)
  let _ = match r {
    Ok(_) => None,
    Err(e) => Some(e),
  }
}
"#
    );
}

#[test]
fn manual_ok_err_mapped_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let r: Result<int, string> = Ok(1)
  let _ = match r {
    Ok(x) => Some(x * 2),
    Err(_) => None,
  }
}
"#
    );
}

#[test]
fn needless_match_option() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = match o {
    Some(x) => Some(x),
    None => None,
  }
}
"#
    );
}

#[test]
fn needless_match_result() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let r: Result<int, string> = Ok(1)
  let _ = match r {
    Ok(x) => Ok(x),
    Err(e) => Err(e),
  }
}
"#
    );
}

#[test]
fn needless_match_reversed_arms() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = match o {
    None => None,
    Some(x) => Some(x),
  }
}
"#
    );
}

#[test]
fn needless_match_custom_enum_no_warning() {
    assert_no_lint_warnings!(
        r#"
enum Maybe { Yes(int), Nope }

fn main() {
  let m = Maybe.Yes(1)
  let _ = match m {
    Maybe.Yes(v) => Maybe.Yes(v),
    Maybe.Nope => Maybe.Nope,
  }
}
"#
    );
}

#[test]
fn needless_match_partial_reconstruction_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = match o {
    Some(x) => Some(x),
    None => Some(0),
  }
}
"#
    );
}

#[test]
fn map_unwrap_or_option() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = o.map(|x| x * 2).unwrap_or(0)
}
"#
    );
}

#[test]
fn map_unwrap_or_result() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let r: Result<int, string> = Ok(1)
  let _ = r.map(|x| x * 2).unwrap_or(0)
}
"#
    );
}

#[test]
fn map_unwrap_or_partial_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let p: Partial<int, string> = Ok(1)
  let _ = p.map(|x| x * 2).unwrap_or(0)
}
"#
    );
}

#[test]
fn map_unwrap_or_side_effecting_default_no_map_unwrap_or() {
    let mut fs = MockFileSystem::new();
    let source = r#"
fn make_default() -> int {
  0
}

fn main() {
  let o: Option<int> = Some(1)
  let _ = o.map(|x| x * 2).unwrap_or(make_default())
}
"#;
    fs.add_file(ENTRY_MODULE_ID, "main.lis", source);

    let result = compile_check(fs);
    let map_unwrap_or = result
        .lints
        .iter()
        .filter(|l| l.code_str() == Some("lint.map_unwrap_or"))
        .count();
    assert_eq!(
        map_unwrap_or,
        0,
        "`map_or` reorders the default before the mapper, unsound when the default does work: {:?}",
        result
            .lints
            .iter()
            .filter_map(|l| l.code_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn map_unwrap_or_effectful_mapper_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn make_mapper() -> fn(int) -> int {
  |x| x * 2
}

fn main() {
  let o: Option<int> = Some(1)
  let _ = o.map(make_mapper()).unwrap_or(0)
}
"#
    );
}

#[test]
fn bind_instead_of_map_option() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = o.and_then(|x| Some(x + 1))
}
"#
    );
}

#[test]
fn bind_instead_of_map_result() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let r: Result<int, string> = Ok(1)
  let _ = r.and_then(|x| Ok(x + 1))
}
"#
    );
}

#[test]
fn bind_instead_of_map_conditional_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = o.and_then(|x| if x > 0 { Some(x) } else { None })
}
"#
    );
}

#[test]
fn bind_instead_of_map_propagating_wrapped_value_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn fallible(x: int) -> Option<int> {
  Some(x)
}

fn main() {
  let o: Option<int> = Some(1)
  let _ = o.and_then(|x| Some(fallible(x)?))
}
"#
    );
}

#[test]
fn map_flatten_option() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = o.map(|x| Some(x + 1)).flatten()
}
"#
    );
}

#[test]
fn map_flatten_without_map_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let oo: Option<Option<int>> = Some(Some(1))
  let _ = oo.flatten()
}
"#
    );
}

#[test]
fn map_identity_option() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = o.map(|x| x)
}
"#
    );
}

#[test]
fn map_identity_result() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let r: Result<int, string> = Ok(1)
  let _ = r.map(|x| x)
}
"#
    );
}

#[test]
fn map_identity_slice_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let s: Slice<int> = [1, 2, 3]
  let _ = s.map(|x| x)
}
"#
    );
}

#[test]
fn map_identity_option_upcast_no_warning() {
    assert_no_lint_warnings!(
        r#"
interface Printable {
  fn print(self) -> string
}

struct Text {
  content: string
}

impl Text {
  fn print(self) -> string {
    self.content
  }
}

fn main() {
  let t = Some(Text { content: "hello" })
  let _: Option<Printable> = t.map(|x| x)
}
"#
    );
}

#[test]
fn map_identity_result_upcast_no_warning() {
    assert_no_lint_warnings!(
        r#"
interface Printable {
  fn print(self) -> string
}

struct Text {
  content: string
}

impl Text {
  fn print(self) -> string {
    self.content
  }
}

fn main() {
  let r: Result<Text, string> = Ok(Text { content: "hello" })
  let _: Result<Printable, string> = r.map(|x| x)
}
"#
    );
}

#[test]
fn unnecessary_map_on_some() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let _ = Some(1).map(|x| x + 1)
}
"#
    );
}

#[test]
fn unnecessary_map_on_ok() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let _: Result<int, string> = Ok(1).map(|x| x + 1)
}
"#
    );
}

#[test]
fn unnecessary_map_err_on_err() {
    assert_lint_snapshot!(
        r#"
fn f() -> Result<int, string> {
  Err("oops").map_err(|e| e + "!")
}

fn main() {
  let _ = f()
}
"#
    );
}

#[test]
fn unnecessary_map_on_variable_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = o.map(|x| x + 1)
}
"#
    );
}

#[test]
fn unnecessary_map_on_constructor_effectful_mapper_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn pick() -> fn(int) -> int {
  |x| x + 1
}

fn main() {
  let _ = Some(1).map(pick())
}
"#
    );
}

#[test]
fn unnecessary_map_on_constructor_effectful_payload_lambda_warns() {
    assert_lint_snapshot!(
        r#"
fn compute() -> int {
  5
}

fn main() {
  let _ = Some(compute()).map(|x| x + 1)
}
"#
    );
}

#[test]
fn map_or_none_option() {
    assert_lint_snapshot!(
        r#"
fn maybe(x: int) -> Option<int> {
  Some(x + 1)
}

fn main() {
  let o: Option<int> = Some(1)
  let _ = o.map_or(None, maybe)
}
"#
    );
}

#[test]
fn map_or_into_option_result() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let r: Result<int, string> = Ok(1)
  let _ = r.map_or(None, Some)
}
"#
    );
}

#[test]
fn map_or_some_default_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = o.map_or(0, |x| x + 1)
}
"#
    );
}

#[test]
fn map_or_into_option_upcast_no_map_or_none() {
    let mut fs = MockFileSystem::new();
    let source = r#"
interface Printable {
  fn print(self) -> string
}

struct Text {
  content: string
}

impl Text {
  fn print(self) -> string {
    self.content
  }
}

fn main() {
  let r: Result<Text, string> = Ok(Text { content: "hello" })
  let _: Option<Printable> = r.map_or(None, |x| Some(x))
}
"#;
    fs.add_file(ENTRY_MODULE_ID, "main.lis", source);

    let result = compile_check(fs);
    let map_or_none = result
        .lints
        .iter()
        .filter(|l| l.code_str() == Some("lint.map_or_none"))
        .count();
    assert_eq!(
        map_or_none,
        0,
        "`.ok()` would discard the upcast to `Option<Printable>`: {:?}",
        result
            .lints
            .iter()
            .filter_map(|l| l.code_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn or_fn_call_unwrap_or_option() {
    assert_lint_snapshot!(
        r#"
fn make_default() -> int {
  42
}

fn main() {
  let o: Option<int> = Some(1)
  let _ = o.unwrap_or(make_default())
}
"#
    );
}

#[test]
fn or_fn_call_unwrap_or_result() {
    assert_lint_snapshot!(
        r#"
fn make_default() -> int {
  42
}

fn main() {
  let r: Result<int, string> = Ok(1)
  let _ = r.unwrap_or(make_default())
}
"#
    );
}

#[test]
fn or_fn_call_unwrap_or_partial() {
    assert_lint_snapshot!(
        r#"
fn make_default() -> int {
  42
}

fn get_partial() -> Partial<int, string> {
  Partial.Ok(1)
}

fn main() {
  let p: Partial<int, string> = get_partial()
  let _ = p.unwrap_or(make_default())
}
"#
    );
}

#[test]
fn or_fn_call_map_or() {
    assert_lint_snapshot!(
        r#"
fn make_default() -> int {
  42
}

fn main() {
  let o: Option<int> = Some(1)
  let _ = o.map_or(make_default(), |x| x + 1)
}
"#
    );
}

#[test]
fn or_fn_call_ok_or() {
    assert_lint_snapshot!(
        r#"
fn make_err() -> string {
  "boom"
}

fn main() {
  let o: Option<int> = Some(1)
  let _ = o.ok_or(make_err())
}
"#
    );
}

#[test]
fn or_fn_call_literal_default_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = o.unwrap_or(0)
}
"#
    );
}

#[test]
fn or_fn_call_arithmetic_default_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let n = 5
  let o: Option<int> = Some(1)
  let _ = o.unwrap_or(n + 1)
}
"#
    );
}

#[test]
fn or_fn_call_closure_value_default_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn make_default() -> int {
  42
}

fn main() {
  let o: Option<fn() -> int> = Some(|| 1)
  let _ = o.unwrap_or(|| make_default() + 1)
}
"#
    );
}

#[test]
fn or_fn_call_cheap_constructor_default_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let o: Option<Option<int>> = Some(None)
  let _ = o.unwrap_or(Some(1))
}
"#
    );
}

#[test]
fn or_fn_call_constructor_wrapping_work() {
    assert_lint_snapshot!(
        r#"
fn make_default() -> int {
  42
}

fn main() {
  let o: Option<Option<int>> = Some(None)
  let _ = o.unwrap_or(Some(make_default()))
}
"#
    );
}

#[test]
fn or_fn_call_go_stdlib_call() {
    assert_lint_snapshot!(
        r#"
import "go:errors"

fn main() {
  let o: Option<int> = Some(1)
  let _ = o.ok_or(errors.New("boom"))
}
"#
    );
}

#[test]
fn or_fn_call_constructor_wrapping_buried_work() {
    assert_lint_snapshot!(
        r#"
fn make_default() -> int {
  42
}

fn main() {
  let o: Option<Option<int>> = Some(None)
  let _ = o.unwrap_or(Some(make_default() + 1))
}
"#
    );
}

#[test]
fn or_fn_call_native_constructor() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let o: Option<Map<string, int>> = Some(Map.new())
  let _ = o.unwrap_or(Map.new())
}
"#
    );
}

#[test]
fn or_fn_call_native_method_identifier() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let s: Slice<int> = [1, 2, 3]
  let o: Option<bool> = Some(true)
  let _ = o.unwrap_or(Slice.contains(s, 2))
}
"#
    );
}

#[test]
fn or_fn_call_propagating_default_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn fallback() -> Option<int> {
  Some(7)
}

fn produce() -> Option<int> {
  let o: Option<int> = Some(1)
  let _ = o.unwrap_or(fallback()?)
  Some(3)
}

fn main() {
  let _ = produce()
}
"#
    );
}

#[test]
fn unnecessary_lazy_evaluations_unwrap_or_else() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = o.unwrap_or_else(|| 0)
}
"#
    );
}

#[test]
fn unnecessary_lazy_evaluations_unwrap_or_else_result() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let r: Result<int, string> = Ok(1)
  let _ = r.unwrap_or_else(|_| 0)
}
"#
    );
}

#[test]
fn unnecessary_lazy_evaluations_map_or_else() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = o.map_or_else(|| 0, |x| x + 1)
}
"#
    );
}

#[test]
fn unnecessary_lazy_evaluations_ok_or_else() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let o: Option<int> = Some(1)
  let _ = o.ok_or_else(|| "x")
}
"#
    );
}

#[test]
fn unnecessary_lazy_evaluations_call_body_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn make_default() -> int {
  42
}

fn main() {
  let o: Option<int> = Some(1)
  let _ = o.unwrap_or_else(|| make_default() + 1)
}
"#
    );
}

#[test]
fn unnecessary_lazy_evaluations_uses_param_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let r: Result<int, int> = Ok(1)
  let _ = r.unwrap_or_else(|e| e)
}
"#
    );
}

#[test]
fn unnecessary_lazy_evaluations_slice_literal_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let o: Option<Slice<int>> = Some([0])
  let _ = o.unwrap_or_else(|| [1, 2, 3])
}
"#
    );
}

#[test]
fn unnecessary_lazy_evaluations_function_ref_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn make_thunk() -> fn() -> int {
  || 42
}

fn main() {
  let o: Option<int> = Some(1)
  let _ = o.unwrap_or_else(make_thunk())
}
"#
    );
}

#[test]
fn needless_question_mark_option() {
    assert_lint_snapshot!(
        r#"
fn consume() -> Option<int> {
  let x: Option<int> = Some(1)
  Some(x?)
}

fn main() {
  let _ = consume()
}
"#
    );
}

#[test]
fn needless_question_mark_result() {
    assert_lint_snapshot!(
        r#"
fn consume() -> Result<int, string> {
  let r: Result<int, string> = Ok(1)
  Ok(r?)
}

fn main() {
  let _ = consume()
}
"#
    );
}

#[test]
fn needless_question_mark_return() {
    assert_lint_snapshot!(
        r#"
fn consume(flag: bool) -> Option<int> {
  let x: Option<int> = Some(1)
  if flag {
    return Some(x?)
  }
  None
}

fn main() {
  let _ = consume(true)
}
"#
    );
}

#[test]
fn needless_question_mark_not_in_tail_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn consume() -> Option<int> {
  let x: Option<int> = Some(1)
  let y: Option<int> = Some(x?)
  y.map(|v| v + 1)
}

fn main() {
  let _ = consume()
}
"#
    );
}

#[test]
fn needless_question_mark_transformed_operand_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn consume() -> Option<int> {
  let x: Option<int> = Some(1)
  Some(x? + 1)
}

fn main() {
  let _ = consume()
}
"#
    );
}

#[test]
fn manual_option_zip_fires() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let a: Option<int> = Some(1)
  let b: Option<int> = Some(2)
  let _ = a.and_then(|a| b.map(|b| (a, b)))
}
"#
    );
}

#[test]
fn manual_option_zip_reversed_arguments_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let a: Option<int> = Some(1)
  let b: Option<int> = Some(2)
  let _ = a.and_then(|a| b.map(|b| (b, a)))
}
"#
    );
}

#[test]
fn manual_option_zip_effectful_second_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn makeb() -> Option<int> {
  Some(2)
}

fn main() {
  let a: Option<int> = Some(1)
  let _ = a.and_then(|a| makeb().map(|b| (a, b)))
}
"#
    );
}

#[test]
fn manual_option_zip_captured_second_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let a: Option<Option<int>> = Some(Some(1))
  let _ = a.and_then(|a| a.map(|b| (a, b)))
}
"#
    );
}

#[test]
fn equals_methods_are_tracked_per_receiver_type() {
    let mut fs = MockFileSystem::new();
    let source = r#"
struct Inner { v: int }

impl Inner {
  fn equals(self, other: Inner) -> bool {
    self.v == other.v
  }
}

struct Other { v: int }

impl Other {
  fn equals(self, other: Other) -> bool {
    self.v == other.v
  }
}

fn main() {
  let a = Inner { v: 1 }
  let b = Inner { v: 2 }
  let _ = a.equals(b)
  let _o = Other { v: 3 }
}
"#;
    fs.add_file(ENTRY_MODULE_ID, "main.lis", source);

    let result = compile_check(fs);
    assert!(
        result.errors.is_empty(),
        "unexpected errors: {:?}",
        result.errors
    );
    let unused_fns = result
        .lints
        .iter()
        .filter(|l| l.code_str() == Some("lint.unused_function"))
        .count();
    assert_eq!(
        unused_fns,
        1,
        "only the uncalled Other.equals should be flagged: {:?}",
        result
            .lints
            .iter()
            .filter_map(|l| l.code_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn equals_method_satisfying_interface_is_kept() {
    let mut fs = MockFileSystem::new();
    let source = r#"
interface Eq<T> {
  fn equals(self, other: T) -> bool
}

struct Point { x: int }

impl Point {
  fn equals(self, other: Point) -> bool {
    self.x == other.x
  }
}

fn check(_a: Eq<Point>) -> bool {
  true
}

fn main() {
  let p = Point { x: 1 }
  let _ = check(p)
}
"#;
    fs.add_file(ENTRY_MODULE_ID, "main.lis", source);

    let result = compile_check(fs);
    assert!(
        result.errors.is_empty(),
        "unexpected errors: {:?}",
        result.errors
    );
    let codes: Vec<&str> = result.lints.iter().filter_map(|l| l.code_str()).collect();
    assert!(
        !codes.contains(&"lint.unused_function"),
        "Point.equals satisfies Eq and must not be flagged unused: {codes:?}"
    );
}

#[test]
fn equals_on_imported_type_does_not_keep_same_named_local_equals() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "models",
        "models.lis",
        r#"
pub struct Inner { pub x: int }

impl Inner {
  pub fn equals(self, _other: Inner) -> bool { true }
}
"#,
    );
    let source = r#"
import "models"

struct Inner { x: int }

impl Inner {
  fn equals(self, _other: Inner) -> bool { true }
}

fn main() {
  let a = models.Inner { x: 1 }
  let b = models.Inner { x: 2 }
  let _ = a.equals(b)
  let _ = Inner { x: 3 }
}
"#;
    fs.add_file(ENTRY_MODULE_ID, "main.lis", source);

    let result = compile_check(fs);
    assert!(
        result.errors.is_empty(),
        "unexpected errors: {:?}",
        result.errors
    );
    let codes: Vec<&str> = result.lints.iter().filter_map(|l| l.code_str()).collect();
    assert!(
        codes.contains(&"lint.unused_function"),
        "the local Inner.equals is unused: `a.equals(b)` dispatches to the imported \
         models.Inner.equals, not the same-named local method: {codes:?}"
    );
}

#[test]
fn equality_user_equals_does_not_root_field_equals() {
    let mut fs = MockFileSystem::new();
    let source = r#"
struct Inner { v: int }

impl Inner {
  fn equals(self, other: Inner) -> bool {
    self.v == other.v
  }
}

#[equality]
struct Outer { inner: Inner }

impl Outer {
  fn equals(self, other: Outer) -> bool {
    true
  }
}

fn main() {
  let a = Outer { inner: Inner { v: 1 } }
  let b = Outer { inner: Inner { v: 2 } }
  let _eq = a.equals(b)
}
"#;
    fs.add_file(ENTRY_MODULE_ID, "main.lis", source);

    let result = compile_check(fs);
    assert!(
        result.errors.is_empty(),
        "unexpected errors: {:?}",
        result.errors
    );
    let unused_fns = result
        .lints
        .iter()
        .filter(|l| l.code_str() == Some("lint.unused_function"))
        .count();
    assert_eq!(
        unused_fns,
        1,
        "Outer's hand-written equals suppresses synthesis and never calls Inner.equals, so Inner.equals must be flagged rather than rooted off the bare attribute: {:?}",
        result
            .lints
            .iter()
            .filter_map(|l| l.code_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn equality_keeps_field_equals_but_flags_unrelated_equals() {
    let mut fs = MockFileSystem::new();
    let source = r#"
struct Inner { v: int }

impl Inner {
  fn equals(self, other: Inner) -> bool {
    self.v == other.v
  }
}

struct Other { v: int }

impl Other {
  fn equals(self, other: Other) -> bool {
    self.v == other.v
  }
}

#[equality]
struct Wrap { inner: Inner }

fn main() {
  let _w = Wrap { inner: Inner { v: 1 } }
  let _o = Other { v: 2 }
}
"#;
    fs.add_file(ENTRY_MODULE_ID, "main.lis", source);

    let result = compile_check(fs);
    assert!(
        result.errors.is_empty(),
        "unexpected errors: {:?}",
        result.errors
    );
    let unused_fns = result
        .lints
        .iter()
        .filter(|l| l.code_str() == Some("lint.unused_function"))
        .count();
    assert_eq!(
        unused_fns,
        1,
        "expected only the unrelated Other.equals flagged: {:?}",
        result
            .lints
            .iter()
            .filter_map(|l| l.code_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn equality_field_rooting_does_not_credit_undispatched_nested_equals() {
    let mut fs = MockFileSystem::new();
    let source = r#"
struct Leaf { v: int }

impl Leaf {
  fn equals(self, other: Leaf) -> bool {
    self.v == other.v
  }
}

struct Holder<T> { item: T }

impl<T> Holder<T> {
  fn equals(self, other: Holder<T>) -> bool {
    true
  }
}

#[equality]
struct Wrap { holder: Holder<Leaf> }

fn main() {
  let _w = Wrap { holder: Holder { item: Leaf { v: 1 } } }
}
"#;
    fs.add_file(ENTRY_MODULE_ID, "main.lis", source);

    let result = compile_check(fs);
    assert!(
        result.errors.is_empty(),
        "unexpected errors: {:?}",
        result.errors
    );
    let unused_fns = result
        .lints
        .iter()
        .filter(|l| l.code_str() == Some("lint.unused_function"))
        .count();
    assert_eq!(
        unused_fns,
        1,
        "Wrap.equals dispatches to Holder.equals (kept), which never calls Leaf.equals; only Leaf.equals must be flagged, not rooted through Holder's type argument: {:?}",
        result
            .lints
            .iter()
            .filter_map(|l| l.code_str())
            .collect::<Vec<_>>()
    );
}

#[test]
fn equality_method_satisfying_interface_is_kept() {
    let mut fs = MockFileSystem::new();
    let source = r#"
interface Eq<T> {
  fn equals(self, other: T) -> bool
}

struct Point { x: int }

impl Point {
  fn equals(self, other: Point) -> bool {
    self.x == other.x
  }
}

fn check(_a: Eq<Point>) -> bool {
  true
}

fn main() {
  let p = Point { x: 1 }
  let _ = check(p)
}
"#;
    fs.add_file(ENTRY_MODULE_ID, "main.lis", source);

    let result = compile_check(fs);
    assert!(
        result.errors.is_empty(),
        "unexpected errors: {:?}",
        result.errors
    );
    let codes: Vec<&str> = result.lints.iter().filter_map(|l| l.code_str()).collect();
    assert!(
        !codes.contains(&"lint.unused_function"),
        "Point.equals satisfies Eq and must not be flagged unused: {codes:?}"
    );
}

#[test]
fn equality_accepts_alpha_renamed_generic_bound() {
    let mut fs = MockFileSystem::new();
    let source = r#"
interface Parent<T> {
  fn p(self, value: T)
}

#[equality]
struct Box<T: Parent<T>> { value: T }

impl<U: Parent<U>> Box<U> {
  fn equals(self, _other: Box<U>) -> bool {
    true
  }
}

fn main() {}
"#;
    fs.add_file(ENTRY_MODULE_ID, "main.lis", source);

    let result = compile_check(fs);
    let codes: Vec<&str> = result.lints.iter().filter_map(|l| l.code_str()).collect();
    assert!(
        result.errors.is_empty(),
        "alpha-equivalent impl bound must be accepted: {:?} {codes:?}",
        result.errors
    );
}

#[test]
fn equality_imported_field_does_not_root_local_equals() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "models",
        "models.lis",
        r#"
pub struct Inner { pub x: int }

impl Inner {
  pub fn equals(self, _other: Inner) -> bool { true }
}
"#,
    );
    let source = r#"
import "models"

struct Inner { x: int }

impl Inner {
  fn equals(self, _other: Inner) -> bool { true }
}

#[equality]
struct Wrap { item: models.Inner }

fn main() {
  let _ = Wrap { item: models.Inner { x: 1 } }
  let _ = Inner { x: 2 }
}
"#;
    fs.add_file(ENTRY_MODULE_ID, "main.lis", source);

    let result = compile_check(fs);
    assert!(
        result.errors.is_empty(),
        "unexpected errors: {:?}",
        result.errors
    );
    let codes: Vec<&str> = result.lints.iter().filter_map(|l| l.code_str()).collect();
    assert!(
        codes.contains(&"lint.unused_function"),
        "local Inner.equals is unused: Wrap dispatches to the imported models.Inner.equals, \
         not the local one, so the local method must still be flagged: {codes:?}"
    );
}

#[test]
fn equality_generic_param_field_does_not_root_local_equals() {
    let mut fs = MockFileSystem::new();
    let source = r#"
struct T { x: int }

impl T {
  fn equals(self, _other: T) -> bool { true }
}

#[equality]
struct Box<T: Comparable> { value: T }

fn main() {
  let _ = Box { value: 1 }
  let _ = T { x: 2 }
}
"#;
    fs.add_file(ENTRY_MODULE_ID, "main.lis", source);

    let result = compile_check(fs);
    assert!(
        result.errors.is_empty(),
        "unexpected errors: {:?}",
        result.errors
    );
    let codes: Vec<&str> = result.lints.iter().filter_map(|l| l.code_str()).collect();
    assert!(
        codes.contains(&"lint.unused_function"),
        "the struct T's equals is unused: the field's `T` is a generic parameter, not the \
         struct, so the method must still be flagged: {codes:?}"
    );
}

#[test]
fn container_equals_imported_element_does_not_root_local_equals() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "models",
        "models.lis",
        r#"
pub struct Inner { pub x: int }

impl Inner {
  pub fn equals(self, _other: Inner) -> bool { true }
}
"#,
    );
    let source = r#"
import "models"

struct Inner { x: int }

impl Inner {
  fn equals(self, _other: Inner) -> bool { true }
}

fn main() {
  let xs = [models.Inner { x: 1 }]
  let ys = [models.Inner { x: 1 }]
  let _ok = xs.equals(ys)
  let _ = Inner { x: 2 }
}
"#;
    fs.add_file(ENTRY_MODULE_ID, "main.lis", source);

    let result = compile_check(fs);
    assert!(
        result.errors.is_empty(),
        "unexpected errors: {:?}",
        result.errors
    );
    let codes: Vec<&str> = result.lints.iter().filter_map(|l| l.code_str()).collect();
    assert!(
        codes.contains(&"lint.unused_function"),
        "local Inner.equals is unused: the slice elements are imported models.Inner, so \
         `xs.equals(ys)` dispatches to models.Inner.equals, not the local method: {codes:?}"
    );
}

#[test]
fn redundant_guards_int() {
    assert_lint_snapshot!(
        r#"
pub fn test(o: Option<int>) -> int {
  match o {
    Some(x) if x == 5 => 1,
    _ => 0,
  }
}
"#
    );
}

#[test]
fn redundant_guards_string() {
    assert_lint_snapshot!(
        r#"
pub fn test(o: Option<string>) -> int {
  match o {
    Some(x) if x == "hi" => 1,
    _ => 0,
  }
}
"#
    );
}

#[test]
fn redundant_guards_reversed_operands() {
    assert_lint_snapshot!(
        r#"
pub fn test(o: Option<int>) -> int {
  match o {
    Some(x) if 5 == x => 1,
    _ => 0,
  }
}
"#
    );
}

#[test]
fn redundant_guards_top_level_binding() {
    assert_lint_snapshot!(
        r#"
pub fn test(n: int) -> int {
  match n {
    x if x == 5 => 1,
    _ => 0,
  }
}
"#
    );
}

#[test]
fn redundant_guards_non_literal_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn test(o: Option<int>, y: int) -> int {
  match o {
    Some(x) if x == y => 1,
    _ => 0,
  }
}
"#
    );
}

#[test]
fn redundant_guards_binding_used_in_body_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn test(o: Option<int>) -> int {
  match o {
    Some(x) if x == 5 => x + 1,
    _ => 0,
  }
}
"#
    );
}

#[test]
fn redundant_guards_inequality_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn test(o: Option<int>) -> int {
  match o {
    Some(x) if x > 5 => 1,
    _ => 0,
  }
}
"#
    );
}

#[test]
fn redundant_guards_compound_guard_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn test(o: Option<int>, flag: bool) -> int {
  match o {
    Some(x) if x == 5 && flag => 1,
    _ => 0,
  }
}
"#
    );
}

#[test]
fn wildcard_in_or_patterns_literal() {
    assert_lint_snapshot!(
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
fn wildcard_in_or_patterns_variant() {
    assert_lint_snapshot!(
        r#"
pub fn test(o: Option<int>) -> int {
  match o {
    Some(0) | _ => 1,
  }
}
"#
    );
}

#[test]
fn wildcard_in_or_patterns_no_wildcard_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn test(n: int) -> int {
  match n {
    0 | 1 => 1,
    _ => 0,
  }
}
"#
    );
}

#[test]
fn match_same_arms_enum() {
    assert_lint_snapshot!(
        r#"
pub enum Sig { A, B, C }

pub fn test(s: Sig) -> int {
  match s {
    Sig.A => 10,
    Sig.B => 20,
    Sig.C => 10,
  }
}
"#
    );
}

#[test]
fn match_same_arms_literals() {
    assert_lint_snapshot!(
        r#"
pub fn test(n: int) -> int {
  match n {
    0 => 10,
    1 => 20,
    2 => 10,
    _ => 0,
  }
}
"#
    );
}

#[test]
fn match_same_arms_data_variant() {
    assert_lint_snapshot!(
        r#"
pub fn test(o: Option<int>) -> int {
  match o {
    Some(0) => 5,
    Some(1) => 6,
    Some(2) => 5,
    _ => 0,
  }
}
"#
    );
}

#[test]
fn match_same_arms_distinct_bodies_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn test(n: int) -> int {
  match n {
    0 => 1,
    1 => 2,
    _ => 3,
  }
}
"#
    );
}

#[test]
fn match_same_arms_binding_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn test(o: Option<int>) -> int {
  let a = 5;
  match o {
    Some(x) => a,
    None => a,
  }
}
"#
    );
}

#[test]
fn match_same_arms_guarded_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn test(n: int, flag: bool) -> int {
  match n {
    0 if flag => 7,
    1 => 8,
    2 if flag => 7,
    _ => 0,
  }
}
"#
    );
}

#[test]
fn match_same_arms_guarded_intervening_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn test(n: int) -> int {
  match n {
    0 => 10,
    k if k < 5 => 20,
    1 => 10,
    _ => 0,
  }
}
"#
    );
}

#[test]
fn match_same_arms_opaque_const_intervening_does_not_merge() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "codes",
        "codes.lis",
        r#"
pub const BASE: int = 1
pub const OTHER: int = 2
pub const A: int = BASE
pub const B: int = BASE
pub const C: int = OTHER
"#,
    );
    let source = r#"
import "codes"

pub fn f(c: int) -> int {
  match c {
    codes.C => 10,
    codes.A => 20,
    codes.B => 10,
    _ => 0,
  }
}
"#;
    fs.add_file(ENTRY_MODULE_ID, "main.lis", source);

    let result = compile_check(fs);
    assert!(
        result.errors.is_empty(),
        "the checker keys opaque consts by name, so it accepts this match: {:?}",
        result.errors
    );
    let codes: Vec<&str> = result.lints.iter().filter_map(|l| l.code_str()).collect();
    assert!(
        !codes.contains(&"lint.match_same_arms"),
        "codes.A and codes.B share the value BASE, so merging codes.C | codes.B over \
         codes.A would change c == BASE from 20 to 10: {codes:?}"
    );
}

#[test]
fn match_same_arms_qualified_and_bare_variant_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub enum Sig { A, B, C }

pub fn f(s: Sig, flag: bool) -> int {
  match s {
    Sig.B => 10,
    A if flag => 20,
    Sig.A => 10,
    Sig.C => 0,
  }
}
"#
    );
}

#[test]
fn match_same_arms_as_binding_arm_does_not_merge() {
    let mut fs = MockFileSystem::new();
    let source = r#"
pub fn f(o: Option<int>) -> int {
  match o {
    Some(0) as x => 10,
    Some(1) => 20,
    Some(2) => 10,
    _ => 0,
  }
}
"#;
    fs.add_file(ENTRY_MODULE_ID, "main.lis", source);

    let result = compile_check(fs);
    assert!(
        result.errors.is_empty(),
        "the `as` arm is valid: {:?}",
        result.errors
    );
    let codes: Vec<&str> = result.lints.iter().filter_map(|l| l.code_str()).collect();
    assert!(
        !codes.contains(&"lint.match_same_arms"),
        "merging `Some(2)` into the `Some(0) as x` arm would yield the malformed \
         `Some(0) as x | Some(2)`: {codes:?}"
    );
}

#[test]
fn needless_bool_assign() {
    assert_lint_snapshot!(
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
fn needless_bool_assign_negated() {
    assert_lint_snapshot!(
        r#"
pub fn f(c: bool) -> bool {
  let mut x = false
  if c {
    x = false
  } else {
    x = true
  }
  x
}
"#
    );
}

#[test]
fn needless_bool_assign_comparison_condition() {
    assert_lint_snapshot!(
        r#"
pub fn f(a: bool, b: bool) -> bool {
  let mut x = false
  if a == b {
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
fn needless_bool_assign_different_targets_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn f(c: bool) -> bool {
  let mut x = false
  let mut y = false
  if c {
    x = true
  } else {
    y = false
  }
  x || y
}
"#
    );
}

#[test]
fn needless_bool_assign_non_bool_values_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn f(c: bool) -> int {
  let mut x = 0
  if c {
    x = 1
  } else {
    x = 2
  }
  x
}
"#
    );
}

#[test]
fn needless_bool_assign_field_target_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub struct Box {
  flag: bool
}

pub fn f(o: Box, c: bool) -> bool {
  let mut obj = o
  if c {
    obj.flag = true
  } else {
    obj.flag = false
  }
  obj.flag
}
"#
    );
}

#[test]
fn redundant_closure_call() {
    assert_lint_snapshot!(
        r#"
pub fn f() -> int {
  (|| 42)()
}
"#
    );
}

#[test]
fn redundant_closure_call_with_argument_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn f() -> int {
  (|x: int| x + 1)(5)
}
"#
    );
}

#[test]
fn redundant_closure_call_escaping_return_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn f() -> int {
  (|| {
    return 5
  })()
}
"#
    );
}

#[test]
fn redundant_closure_call_defer_body_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"

fn cleanup() {
  fmt.Println("done")
}

pub fn f() {
  (|| {
    defer cleanup()
  })()
}
"#
    );
}

#[test]
fn single_element_loop() {
    assert_lint_snapshot!(
        r#"
pub fn f() -> int {
  let mut total = 0
  for x in [7] {
    total += x
  }
  total
}
"#
    );
}

#[test]
fn single_element_loop_multiple_elements_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn f() -> int {
  let mut total = 0
  for x in [1, 2] {
    total += x
  }
  total
}
"#
    );
}

#[test]
fn single_element_loop_break_in_body_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn f() -> int {
  let mut total = 0
  for x in [7] {
    if x > 0 {
      break
    }
    total += x
  }
  total
}
"#
    );
}

#[test]
fn single_element_loop_variable_iterable_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn f(xs: Slice<int>) -> int {
  let mut total = 0
  for x in xs {
    total += x
  }
  total
}
"#
    );
}

#[test]
fn while_let_loop() {
    assert_lint_snapshot!(
        r#"
import "go:fmt"

pub fn drain(xs: Slice<int>) {
  let mut it = xs
  loop {
    match it.get(0) {
      Some(v) => {
        fmt.Println(v)
        it = it[1..]
      },
      _ => break,
    }
  }
}
"#
    );
}

#[test]
fn while_let_loop_guard_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"

pub fn f(it: Option<int>) {
  loop {
    match it {
      Some(v) if v > 0 => fmt.Println(v),
      _ => break,
    }
  }
}
"#
    );
}

#[test]
fn while_let_loop_non_break_dismissal_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"

pub fn f(it: Option<int>) {
  loop {
    match it {
      Some(v) => fmt.Println(v),
      _ => continue,
    }
  }
}
"#
    );
}

#[test]
fn while_let_loop_value_carrying_no_warning() {
    assert_no_lint_warnings!(
        r#"
pub fn f(xs: Slice<int>) -> int {
  let mut it = xs
  loop {
    match it.get(0) {
      Some(v) => {
        it = it[1..]
        if v > 100 {
          break v
        }
      },
      _ => break 0,
    }
  }
}
"#
    );
}

#[test]
fn while_let_loop_single_variant_enum_no_warning() {
    let mut fs = MockFileSystem::new();
    let source = r#"
import "go:fmt"

enum Single { Only(int) }

pub fn f(s: Single) {
  loop {
    match s {
      Only(v) => fmt.Println(v),
      _ => break,
    }
  }
}
"#;
    fs.add_file(ENTRY_MODULE_ID, "main.lis", source);

    let result = compile_check(fs);
    let codes: Vec<&str> = result.lints.iter().filter_map(|l| l.code_str()).collect();
    assert!(
        !codes.contains(&"lint.while_let_loop"),
        "a single-variant enum makes the variant pattern irrefutable, so the \
         `while let` rewrite would loop forever: {codes:?}"
    );
}

#[test]
fn redundant_rebinding() {
    assert_lint_snapshot!(
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
fn redundant_rebinding_parameter() {
    assert_lint_snapshot!(
        r#"
pub fn f(a: int) {
  let a = a;
  let _ = a
}
"#
    );
}

#[test]
fn redundant_rebinding_parenthesized_rhs() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let a = 5;
  let a = (a);
  let _ = a
}
"#
    );
}

#[test]
fn redundant_rebinding_mut_outer_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let mut a = 5;
  a = 6;
  let a = a;
  let _ = a
}
"#
    );
}

#[test]
fn redundant_rebinding_mut_binding_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let a = 5;
  let mut a = a;
  a = 6;
  let _ = a
}
"#
    );
}

#[test]
fn redundant_rebinding_annotation_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let a = 5;
  let a: int = a;
  let _ = a
}
"#
    );
}

#[test]
fn redundant_rebinding_different_name_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let b = 5;
  let a = b;
  let _ = a
}
"#
    );
}

#[test]
fn redundant_rebinding_referenced_outer_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let a = 1;
  let r = &a;
  let a = a;
  let _ = r;
  let _ = a
}
"#
    );
}

#[test]
fn redundant_rebinding_referenced_new_no_warning() {
    assert_no_lint_warnings!(
        r#"
fn main() {
  let a = 1;
  let a = a;
  let r = &a;
  let _ = r
}
"#
    );
}

#[test]
fn redundant_rebinding_unused_ceded_to_unused_variable() {
    let mut fs = MockFileSystem::new();
    let source = r#"
fn main() {
  let a = 5;
  let a = a;
}
"#;
    fs.add_file(ENTRY_MODULE_ID, "main.lis", source);

    let result = compile_check(fs);
    let codes: Vec<&str> = result.lints.iter().filter_map(|l| l.code_str()).collect();
    assert!(
        !codes.contains(&"lint.redundant_rebinding"),
        "an unused rebinding is owned by `unused_variable`, which gives the same \
         remove-the-line advice: {codes:?}"
    );
    assert!(
        codes.contains(&"lint.unused_variable"),
        "the unused rebinding should still draw an unused-variable warning: {codes:?}"
    );
}

#[test]
fn unused_result_in_if_let_branch() {
    assert_lint_snapshot!(
        r#"
fn get_result() -> Result<int, string> {
  Ok(42)
}

fn main() {
  let opt = Some(1)
  if let Some(_) = opt {
    get_result()
  } else {
    ()
  }
  ()
}
"#
    );
}

#[test]
fn excess_parens_on_condition_if_let() {
    assert_lint_snapshot!(
        r#"
fn main() {
  let x = Some(5)
  if let Some(n) = (x) {
    let _ = n
  }
}
"#
    );
}

#[test]
fn redundant_field_names() {
    assert_lint_snapshot!(
        r#"
struct Point { x: int, y: int }

fn read(p: Point) -> int { p.x + p.y }

fn make(x: int) -> Point {
  Point { x: x, y: 0 }
}

fn main() {
  let _ = read(make(5))
}
"#
    );
}

#[test]
fn redundant_field_names_enum_variant() {
    assert_lint_snapshot!(
        r#"
enum Action {
  Move { x: int, y: int },
  Stop,
}

fn build(x: int) -> Action {
  Action.Move { x: x, y: 0 }
}

fn main() {
  let _ = match build(5) {
    Action.Move { x, y } => x + y,
    Action.Stop => 0,
  }
}
"#
    );
}

#[test]
fn redundant_field_names_shorthand_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Point { x: int, y: int }

fn read(p: Point) -> int { p.x + p.y }

fn make(x: int, y: int) -> Point {
  Point { x, y }
}

fn main() {
  let _ = read(make(1, 2))
}
"#
    );
}

#[test]
fn redundant_field_names_different_name_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Point { x: int, y: int }

fn read(p: Point) -> int { p.x + p.y }

fn make(a: int, b: int) -> Point {
  Point { x: a, y: b }
}

fn main() {
  let _ = read(make(1, 2))
}
"#
    );
}

#[test]
fn redundant_field_names_parenthesized_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Point { x: int, y: int }

fn read(p: Point) -> int { p.x + p.y }

fn make(x: int) -> Point {
  Point { x: (x), y: 0 }
}

fn main() {
  let _ = read(make(5))
}
"#
    );
}

#[test]
fn needless_update() {
    assert_lint_snapshot!(
        r#"
struct Config { debug: bool, port: int }

fn read(c: Config) -> int { if c.debug { c.port } else { 0 } }

fn rebuild(base: Config) -> Config {
  Config { debug: true, port: 80, ..base }
}

fn main() {
  let _ = read(rebuild(Config { debug: false, port: 1 }))
}
"#
    );
}

#[test]
fn needless_update_enum_variant() {
    assert_lint_snapshot!(
        r#"
enum Shape {
  Rect { w: int, h: int },
  Dot,
}

fn resize(base: Shape) -> Shape {
  Shape.Rect { w: 5, h: 6, ..base }
}

fn main() {
  let _ = match resize(Shape.Rect { w: 1, h: 2 }) {
    Shape.Rect { w, h } => w + h,
    Shape.Dot => 0,
  }
}
"#
    );
}

#[test]
fn needless_update_partial_spread_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Config { debug: bool, port: int }

fn read(c: Config) -> int { if c.debug { c.port } else { 0 } }

fn rebuild(base: Config) -> Config {
  Config { debug: true, ..base }
}

fn main() {
  let _ = read(rebuild(Config { debug: false, port: 1 }))
}
"#
    );
}

#[test]
fn needless_update_side_effecting_base_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Config { debug: bool, port: int }

fn read(c: Config) -> int { if c.debug { c.port } else { 0 } }

fn fresh() -> Config {
  Config { debug: false, port: 1 }
}

fn rebuild() -> Config {
  Config { debug: true, port: 80, ..fresh() }
}

fn main() {
  let _ = read(rebuild())
}
"#
    );
}

#[test]
fn needless_update_zero_fill_no_warning() {
    assert_no_lint_warnings!(
        r#"
struct Config { debug: bool, port: int }

fn read(c: Config) -> int { if c.debug { c.port } else { 0 } }

fn rebuild() -> Config {
  Config { debug: true, port: 80, .. }
}

fn main() {
  let _ = read(rebuild())
}
"#
    );
}

#[test]
fn enum_variant_names_prefix() {
    assert_lint_snapshot!(
        r#"
enum Color { ColorRed, ColorGreen }

fn describe(c: Color) -> int {
  match c {
    Color.ColorRed => 1,
    Color.ColorGreen => 2,
  }
}

fn main() {
  let _ = describe(Color.ColorRed)
}
"#
    );
}

#[test]
fn enum_variant_names_suffix() {
    assert_lint_snapshot!(
        r#"
enum MyError { ParseMyError, IoMyError }

fn code(e: MyError) -> int {
  match e {
    MyError.ParseMyError => 1,
    MyError.IoMyError => 2,
  }
}

fn main() {
  let _ = code(MyError.ParseMyError)
}
"#
    );
}

#[test]
fn enum_variant_names_no_warning_distinct_variants() {
    assert_no_lint_warnings!(
        r#"
enum Color { Red, Green }

fn pick(c: Color) -> int {
  match c {
    Color.Red => 1,
    Color.Green => 2,
  }
}

fn main() {
  let _ = pick(Color.Red)
}
"#
    );
}

#[test]
fn enum_variant_names_no_warning_word_lookalike() {
    assert_no_lint_warnings!(
        r#"
enum Color { Colorful, Colorize }

fn pick(c: Color) -> int {
  match c {
    Color.Colorful => 1,
    Color.Colorize => 2,
  }
}

fn main() {
  let _ = pick(Color.Colorful)
}
"#
    );
}

#[test]
fn enum_variant_names_no_warning_partial_share() {
    assert_no_lint_warnings!(
        r#"
enum Mixed { MixedOne, Two }

fn pick(m: Mixed) -> int {
  match m {
    Mixed.MixedOne => 1,
    Mixed.Two => 2,
  }
}

fn main() {
  let _ = pick(Mixed.MixedOne)
}
"#
    );
}

#[test]
fn enum_variant_names_no_warning_single_variant() {
    assert_no_lint_warnings!(
        r#"
enum Wrapper { WrapperInner }

fn pick(w: Wrapper) -> int {
  match w {
    Wrapper.WrapperInner => 1,
  }
}

fn main() {
  let _ = pick(Wrapper.WrapperInner)
}
"#
    );
}

#[test]
fn enum_variant_names_no_warning_variant_equals_enum() {
    assert_no_lint_warnings!(
        r#"
enum Exact { Exact, ExactMore }

fn pick(e: Exact) -> int {
  match e {
    Exact.Exact => 1,
    Exact.ExactMore => 2,
  }
}

fn main() {
  let _ = pick(Exact.Exact)
}
"#
    );
}

#[test]
fn self_named_constructors_fires() {
    assert_lint_snapshot!(
        r#"
struct Point { x: int }

impl Point {
  fn point(x: int) -> Point {
    Point { x }
  }
}

fn main() {
  let p = Point.point(1)
  let _ = p.x
}
"#
    );
}

#[test]
fn self_named_constructors_no_warning_new() {
    assert_no_lint_warnings!(
        r#"
struct Point { x: int }

impl Point {
  fn new(x: int) -> Point {
    Point { x }
  }
}

fn main() {
  let p = Point.new(1)
  let _ = p.x
}
"#
    );
}

#[test]
fn self_named_constructors_no_warning_instance_method() {
    assert_no_lint_warnings!(
        r#"
struct Point { x: int }

impl Point {
  fn point(self) -> int {
    self.x
  }
}

fn main() {
  let p = Point { x: 1 }
  let _ = p.point()
}
"#
    );
}

#[test]
fn self_named_constructors_no_warning_returns_other_type() {
    assert_no_lint_warnings!(
        r#"
struct Widget { id: int }

impl Widget {
  fn widget() -> int {
    5
  }
  fn build(id: int) -> Widget {
    Widget { id }
  }
}

fn main() {
  let _ = Widget.widget()
  let w = Widget.build(1)
  let _ = w.id
}
"#
    );
}

#[test]
fn nested_fstring() {
    assert_lint_snapshot!(
        r#"
import "go:fmt"

fn main() {
  let a = 1
  let b = 2
  fmt.Print(f"result: {f"{a} of {b}"}")
}
"#
    );
}

#[test]
fn nested_fstring_solo_int_inner() {
    assert_lint_snapshot!(
        r#"
import "go:fmt"

fn main() {
  let n = 1
  fmt.Print(f"n = {f"{n}"} done")
}
"#
    );
}

#[test]
fn nested_fstring_solo_string_inner_cedes_to_expression_only() {
    assert_lint_snapshot!(
        r#"
import "go:fmt"

fn main() {
  let s = "x"
  fmt.Print(f"s = {f"{s}"} done")
}
"#
    );
}

#[test]
fn nested_fstring_uninterpolated_inner_cedes_to_uninterpolated() {
    assert_lint_snapshot!(
        r#"
import "go:fmt"

fn main() {
  fmt.Print(f"x {f"y"}")
}
"#
    );
}

#[test]
fn nested_fstring_separate_interpolations_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"

fn main() {
  let a = 1
  let b = 2
  fmt.Print(f"{a} and {b}")
}
"#
    );
}

#[test]
fn redundant_fstring_conversion_itoa() {
    assert_lint_snapshot!(
        r#"
import "go:fmt"
import "go:strconv"

fn main() {
  let n = 42
  fmt.Print(f"count: {strconv.Itoa(n)}")
}
"#
    );
}

#[test]
fn redundant_fstring_conversion_format_bool() {
    assert_lint_snapshot!(
        r#"
import "go:fmt"
import "go:strconv"

fn main() {
  let ok = true
  fmt.Print(f"ok: {strconv.FormatBool(ok)}")
}
"#
    );
}

#[test]
fn redundant_fstring_conversion_sprint() {
    assert_lint_snapshot!(
        r#"
import "go:fmt"

fn main() {
  let n = 42
  fmt.Print(f"n = {fmt.Sprint(n)}")
}
"#
    );
}

#[test]
fn redundant_fstring_conversion_sprint_rune_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"

fn main() {
  let c = 'a'
  fmt.Print(f"c = {fmt.Sprint(c)}")
}
"#
    );
}

#[test]
fn redundant_fstring_conversion_sprint_non_scalar_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"

fn show(pair: (int, int)) -> string {
  f"pair = {fmt.Sprint(pair)}"
}

fn main() {
  fmt.Print(show((1, 2)))
}
"#
    );
}

#[test]
fn redundant_fstring_conversion_user_method_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"

struct Box { v: int }

impl Box {
  fn Sprint(self: Box) -> string {
    f"box {self.v}"
  }
}

fn show(b: Box) -> string {
  f"b = {b.Sprint()}"
}

fn main() {
  fmt.Print(show(Box { v: 1 }))
}
"#
    );
}

#[test]
fn redundant_fstring_conversion_format_int_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"
import "go:strconv"

fn show(n: int64) -> string {
  f"n = {strconv.FormatInt(n, 10)}"
}

fn main() {
  fmt.Print(show(5))
}
"#
    );
}

#[test]
fn redundant_fstring_conversion_plain_interpolation_no_warning() {
    assert_no_lint_warnings!(
        r#"
import "go:fmt"

fn show(n: int) -> string {
  f"n = {n}"
}

fn main() {
  fmt.Print(show(6))
}
"#
    );
}
