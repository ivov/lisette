use crate::spec::infer::*;

#[test]
fn let_slice_fixed_length_is_refutable() {
    let input = r#"
fn test(slice: Slice<int>) {
  let [a] = slice;
}
"#;
    infer(input).assert_infer_code("refutable_pattern");
}

#[test]
fn let_slice_empty_is_refutable() {
    let input = r#"
fn test(slice: Slice<int>) {
  let [] = slice;
}
"#;
    infer(input).assert_infer_code("refutable_pattern");
}

#[test]
fn let_slice_with_rest_prefix_is_refutable() {
    let input = r#"
fn test(slice: Slice<int>) {
  let [a, ..] = slice;
}
"#;
    infer(input).assert_infer_code("refutable_pattern");
}

#[test]
fn let_enum_variant_is_refutable() {
    let input = r#"
fn test(opt: Option<int>) {
  let Some(x) = opt;
}
"#;
    infer(input).assert_infer_code("refutable_pattern");
}

#[test]
fn let_result_ok_is_refutable() {
    let input = r#"
fn test(res: Result<int, string>) {
  let Ok(x) = res;
}
"#;
    infer(input).assert_infer_code("refutable_pattern");
}

#[test]
fn let_nested_refutable_in_tuple() {
    let input = r#"
fn test(pair: (int, Slice<int>)) {
  let (a, [b]) = pair;
}
"#;
    infer(input).assert_infer_code("refutable_pattern");
}

#[test]
fn let_nested_refutable_in_struct() {
    let input = r#"
struct Container { items: Slice<int> }

fn test(c: Container) {
  let Container { items: [x] } = c;
}
"#;
    infer(input).assert_infer_code("refutable_pattern");
}

#[test]
fn let_identifier_is_irrefutable() {
    let input = r#"
fn test(opt: Option<int>) {
  let x = opt;
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn let_wildcard_is_irrefutable() {
    let input = r#"
fn test(opt: Option<int>) {
  let _ = opt;
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn let_tuple_is_irrefutable() {
    let input = r#"
fn test(pair: (int, string)) {
  let (a, b) = pair;
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn let_struct_is_irrefutable() {
    let input = r#"
struct Point { x: int, y: int }

fn test(p: Point) {
  let Point { x, y } = p;
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn let_unit_is_irrefutable() {
    let input = r#"
fn test(u: ()) {
  let () = u;
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn let_single_variant_enum_is_irrefutable() {
    let input = r#"
enum Wrapper { Value(int) }

fn test(w: Wrapper) {
  let Wrapper.Value(x) = w;
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn fn_param_slice_is_refutable() {
    let input = r#"
fn test([a]: Slice<int>) {
  a
}
"#;
    infer(input).assert_infer_code("refutable_pattern");
}

#[test]
fn fn_param_enum_variant_is_refutable() {
    let input = r#"
fn test(Some(x): Option<int>) {
  x
}
"#;
    infer(input).assert_infer_code("refutable_pattern");
}

#[test]
fn fn_param_tuple_is_irrefutable() {
    let input = r#"
fn test((a, b): (int, string)) {
  a
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn fn_param_struct_is_irrefutable() {
    let input = r#"
struct Point { x: int, y: int }

fn test(Point { x, y }: Point) {
  x + y
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn fn_param_single_variant_is_irrefutable() {
    let input = r#"
enum Wrapper { Value(int) }

fn test(Wrapper.Value(x): Wrapper) {
  x
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn lambda_param_slice_is_refutable() {
    let input = r#"
fn test() {
  let f = |[a]: Slice<int>| a;
}
"#;
    infer(input).assert_infer_code("refutable_pattern");
}

#[test]
fn lambda_param_tuple_is_irrefutable() {
    let input = r#"
fn test() {
  let f = |(a, b): (int, int)| a + b;
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn for_loop_slice_pattern_is_refutable() {
    let input = r#"
fn test(slices: Slice<Slice<int>>) {
  for [a] in slices {
    let _ = a;
  }
}
"#;
    infer(input).assert_infer_code("refutable_pattern");
}

#[test]
fn for_loop_identifier_is_irrefutable() {
    let input = r#"
fn test(items: Slice<int>) {
  for x in items {
    let _ = x;
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn for_loop_tuple_is_irrefutable() {
    let input = r#"
fn test(pairs: Slice<(int, int)>) {
  for (a, b) in pairs {
    let _ = a + b;
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn match_arm_slice_is_allowed() {
    let input = r#"
fn test(slice: Slice<int>) -> int {
  match slice {
    [a] => a,
    [] => 0,
    [_, ..] => 1,
  }
}
"#;
    infer(input).assert_no_errors();
}

#[test]
fn match_arm_enum_variant_is_allowed() {
    let input = r#"
fn test(opt: Option<int>) -> int {
  match opt {
    Some(x) => x,
    None => 0,
  }
}
"#;
    infer(input).assert_no_errors();
}
