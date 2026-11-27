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

const SHAPES: &str = r#"
interface Shape { fn area() -> int }
interface Empty {}
struct Sq { side: int }
struct Ci { r: int }
struct Pt(int, int)
struct Nt(int)
enum Kind { Big(int), Small }
impl Sq { fn area(self) -> int { self.side } }
impl Ci { fn area(self) -> int { self.r } }
impl Pt { fn area(self) -> int { self.0 } }
impl Nt { fn area(self) -> int { self.0 } }
impl Kind { fn area(self) -> int { 7 } }
"#;

fn with_shapes(body: &str) -> String {
    format!("{SHAPES}\n{body}")
}

#[test]
fn let_struct_pattern_on_interface_is_refutable() {
    let input = with_shapes("fn test(s: Shape) -> int { let Sq { side } = s; side }");
    infer(&input).assert_infer_code("refutable_pattern");
}

#[test]
fn let_tuple_struct_pattern_on_interface_is_refutable() {
    let input = with_shapes("fn test(s: Shape) -> int { let Pt(a, b) = s; a + b }");
    infer(&input).assert_infer_code("refutable_pattern");
}

#[test]
fn let_newtype_pattern_on_interface_is_refutable() {
    let input = with_shapes("fn test(s: Shape) -> int { let Nt(n) = s; n }");
    infer(&input).assert_infer_code("refutable_pattern");
}

#[test]
fn nested_struct_pattern_on_interface_is_refutable() {
    let input = with_shapes("fn test(s: Shape) -> int { let (Sq { side }, k) = (s, 1); side + k }");
    infer(&input).assert_infer_code("refutable_pattern");
}

#[test]
fn parameter_struct_pattern_on_interface_is_refutable() {
    let input = with_shapes("fn test(Sq { side }: Shape) -> int { side }");
    infer(&input).assert_infer_code("refutable_pattern");
}

#[test]
fn lambda_parameter_struct_pattern_on_interface_is_refutable() {
    let input =
        with_shapes("fn test() { let f = |Sq { side }: Shape| side; let _ = f(Sq { side: 1 }) }");
    infer(&input).assert_infer_code("refutable_pattern");
}

#[test]
fn for_struct_pattern_on_interface_is_refutable() {
    let input = with_shapes(
        "fn test(xs: Slice<Shape>) -> int { let mut t = 0; for Sq { side } in xs { t += side }; t }",
    );
    infer(&input).assert_infer_code("refutable_pattern");
}

#[test]
fn for_pair_pattern_on_interface_value_is_refutable() {
    let input = with_shapes(
        "fn test(m: Map<string, Shape>) -> int { let mut t = 0; for (_, Sq { side }) in m { t += side }; t }",
    );
    infer(&input).assert_infer_code("refutable_pattern");
}

#[test]
fn select_receive_struct_pattern_on_interface_is_refutable() {
    let input = with_shapes(
        "fn test(ch: Receiver<Shape>) -> int { select { let Some(Sq { side }) = ch.receive() => side, _ => 0 } }",
    );
    infer(&input).assert_infer_code("select_receive_refutable_pattern");
}

#[test]
fn select_match_receive_struct_pattern_on_interface_is_refutable() {
    let input = with_shapes(
        "fn test(ch: Receiver<Shape>) -> int { select { match ch.receive() { Some(Sq { side }) => side, None => 0 } } }",
    );
    infer(&input).assert_infer_code("select_receive_refutable_pattern");
}

#[test]
fn select_receive_binding_on_interface_is_irrefutable() {
    let input = with_shapes(
        "fn test(ch: Receiver<Shape>) -> int { select { let Some(v) = ch.receive() => v.area(), _ => 0 } }",
    );
    infer(&input).assert_no_errors();
}

#[test]
fn while_let_struct_pattern_on_interface_is_refutable() {
    let input = with_shapes(
        "fn test(s0: Shape) -> int { let mut s = s0; let mut t = 0; while let Sq { side } = s { t += side; s = Ci { r: 1 } }; t }",
    );
    infer(&input).assert_no_errors();
}

#[test]
fn enum_variants_on_interface_are_not_exhaustive() {
    let input =
        with_shapes("fn test(s: Shape) -> int { match s { Kind.Big(n) => n, Kind.Small => 1 } }");
    infer(&input).assert_infer_code("non_exhaustive");
}

#[test]
fn enum_variants_on_interface_leave_a_wildcard_reachable() {
    let input = with_shapes(
        "fn test(s: Shape) -> int { match s { Kind.Big(n) => n, Kind.Small => 1, _ => 0 } }",
    );
    infer(&input).assert_no_errors();
}

#[test]
fn nested_enum_variants_on_interface_are_not_exhaustive() {
    let input = with_shapes(
        "fn test(o: Option<Shape>) -> int { match o { Some(Kind.Big(n)) => n, Some(Kind.Small) => 1, None => 0 } }",
    );
    infer(&input).assert_infer_code("non_exhaustive");
}

#[test]
fn boolean_literals_on_interface_are_not_exhaustive() {
    let input = with_shapes("fn test(e: Empty) -> int { match e { true => 1, false => 2 } }");
    infer(&input).assert_infer_code("non_exhaustive");
}

#[test]
fn slice_pattern_on_interface_needs_a_fixed_element_type() {
    let input = with_shapes("fn test(e: Empty) -> int { match e { [a] => a + 1, _ => 0 } }");
    infer(&input).assert_infer_code("interface_pattern_type_not_fixed");
}

#[test]
fn generic_enum_pattern_on_interface_needs_fixed_type_arguments() {
    let input = with_shapes(
        "enum Box<T> { Full(T), Vacant }\nimpl<T> Box<T> { fn area(self) -> int { 1 } }\nfn test(s: Shape) -> int { match s { Box.Full(_) => 1, _ => 0 } }",
    );
    infer(&input).assert_infer_code("interface_pattern_type_not_fixed");
}

#[test]
fn generic_struct_pattern_on_interface_needs_fixed_type_arguments() {
    let input = with_shapes(
        "struct W<T> { v: T }\nimpl<T> W<T> { fn area(self) -> int { 1 } }\nfn test(s: Shape) -> int { match s { W { .. } => 1, _ => 0 } }",
    );
    infer(&input).assert_infer_code("interface_pattern_type_not_fixed");
}
