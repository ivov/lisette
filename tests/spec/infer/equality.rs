use crate::spec::infer::*;

#[test]
fn variable_inferred_from_usage() {
    infer(
        r#"
    fn main() {
      let x = 42;
      let y: int = x;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn variable_conflicting_constraints() {
    infer(
        r#"
    fn main() {
      let x = 42;
      let y: string = x;
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn variable_in_collection() {
    infer(
        r#"
    fn main() {
      let xs = [1, 2, 3];
      let first: int = xs[0];
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn variable_in_empty_collection() {
    infer(
        r#"
    fn main() {
      let xs: Slice<int> = [];
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn variable_propagates_between_assignments() {
    infer(
        r#"
    fn main() {
      let x = 42;
      let y = x;
      let z: int = y;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn variable_from_function_return() {
    infer(
        r#"
    fn get_value() -> int {
      return 42;
    }

    fn main() {
      let x = get_value();
      let y: int = x;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn variable_in_nested_collection() {
    infer(
        r#"
    fn main() {
      let matrix = [[1, 2], [3, 4]];
      let row: Slice<int> = matrix[0];
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn variable_mismatch_in_collection() {
    infer(
        r#"
    fn main() {
      let xs = [1, 2, 3];
      let first: string = xs[0];
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn numeric_int_and_float64() {
    infer(
        r#"
    fn get_number() -> int {
      return 42;
    }

    fn main() {
      let x: float64 = get_number();
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn numeric_float64_and_int() {
    infer(
        r#"
    fn get_number() -> float64 {
      return 3.14;
    }

    fn main() {
      let x: int = get_number();
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn numeric_in_function_parameter() {
    infer(
        r#"
    fn process(x: int) -> int {
      return x;
    }

    fn main() {
      let n: float64 = 3.14;
      process(n);
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn numeric_in_nested_context() {
    infer(
        r#"
    fn get_int() -> int {
      return 42;
    }

    fn accept_float(x: float64) {
    }

    fn main() {
      accept_float(get_int());
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn numeric_explicit_int_to_float64() {
    infer(
        r#"
    fn get_number() -> int {
      return 42;
    }

    fn main() {
      let x: float64 = get_number() as float64;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn numeric_explicit_float64_to_int() {
    infer(
        r#"
    fn get_number() -> float64 {
      return 3.14;
    }

    fn main() {
      let x: int = get_number() as int;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn numeric_int_literal_in_float64_context() {
    infer(
        r#"
    fn main() {
      let x: float64 = 42;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn numeric_int_literal_to_float64_param() {
    infer(
        r#"
    fn accept_float(x: float64) {
    }

    fn main() {
      accept_float(42);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn numeric_int_literal_in_float32_context() {
    infer(
        r#"
    fn main() {
      let x: float32 = 42;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn numeric_int_literal_to_float32_param() {
    infer(
        r#"
    fn accept_float32(x: float32) {
    }

    fn main() {
      accept_float32(42);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn numeric_float_literal_not_in_int_context() {
    infer(
        r#"
    fn main() {
      let x: int = 3.14;
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn numeric_int_literal_flexibility_not_through_generics() {
    infer(
        r#"
    fn main() {
      let x: Option<float64> = Some(42);
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn numeric_int_literal_in_generic_requires_explicit_cast() {
    infer(
        r#"
    fn main() {
      let x: Option<float64> = Some(42.0);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn cast_int_sizes() {
    infer(
        r#"
    fn main() {
      let a: int8 = 42;
      let b: int16 = 42;
      let c: int32 = 42;
      let d: int64 = 42;
      let e: uint8 = 42;
      let f: uint16 = 42;
      let g: uint32 = 42;
      let h: uint64 = 42;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn cast_float_sizes() {
    infer(
        r#"
    fn main() {
      let a: float32 = 3.14;
      let b: float64 = 3.14;
      let c: float32 = 42;
      let d: float64 = 42;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn cast_int_to_float_and_back() {
    infer(
        r#"
    fn main() {
      let x: float64 = 42;
      let f = 3.14;
      let y: int = f as int;
      let z: int = x as int;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn cast_literal_without_type_context_not_redundant() {
    infer(
        r#"
    fn main() {
      let a = 1 as uint8;
      let b = 1 as int64;
      let c = 3.14 as float32;
      let _ = (a, b, c);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn cast_literal_with_type_context_is_redundant() {
    infer(
        r#"
    fn main() {
      let a: int64 = 100 as int64;
      let _ = a;
    }
        "#,
    )
    .assert_infer_code("redundant_cast");
}

#[test]
fn cast_negative_literal_with_type_context_is_redundant() {
    infer(
        r#"
    fn main() {
      let a: int8 = -50 as int8;
      let _ = a;
    }
        "#,
    )
    .assert_infer_code("redundant_cast");
}

#[test]
fn cast_paren_negative_literal_with_type_context_is_redundant() {
    infer(
        r#"
    fn main() {
      let a: int8 = (-50) as int8;
      let _ = a;
    }
        "#,
    )
    .assert_infer_code("redundant_cast");
}

#[test]
fn cast_string_to_byte_slice() {
    infer(
        r#"
    fn main() {
      let s = "hello";
      let bytes: Slice<byte> = s as Slice<byte>;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn cast_byte_slice_to_string() {
    infer(
        r#"
    fn main() {
      let bytes: Slice<byte> = "hello" as Slice<byte>;
      let s: string = bytes as string;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn cast_string_to_rune_slice() {
    infer(
        r#"
    fn main() {
      let s = "hello";
      let runes: Slice<rune> = s as Slice<rune>;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn cast_rune_slice_to_string() {
    infer(
        r#"
    fn main() {
      let runes: Slice<rune> = ['h', 'e', 'l', 'l', 'o'];
      let s: string = runes as string;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn cast_int_to_rune() {
    infer(
        r#"
    fn main() {
      let r: rune = 65 as rune;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn cast_rune_to_int() {
    infer(
        r#"
    fn main() {
      let r: rune = 'A';
      let n: int = r as int;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn cast_invalid_string_to_int() {
    infer(
        r#"
    fn main() {
      let x = "42" as int;
    }
        "#,
    )
    .assert_infer_code("invalid_cast");
}

#[test]
fn cast_invalid_bool_to_int() {
    infer(
        r#"
    fn main() {
      let x = true as int;
    }
        "#,
    )
    .assert_infer_code("invalid_cast");
}

#[test]
fn cast_invalid_struct_to_int() {
    infer(
        r#"
    struct Point { x: int, y: int }
    fn main() {
      let p = Point { x: 1, y: 2 };
      let n = p as int;
    }
        "#,
    )
    .assert_infer_code("invalid_cast");
}

#[test]
fn cast_precedence_with_arithmetic() {
    infer(
        r#"
    fn main() {
      let x: float64 = (1 + 2) as float64;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn cast_precedence_with_comparison() {
    infer(
        r#"
    fn main() {
      let a: float64 = 3.14;
      let b: int = 3;
      let result: bool = a as int == b;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn cast_chained_error() {
    infer(
        r#"
    fn main() {
      let x = 42 as float64 as int;
    }
        "#,
    )
    .assert_infer_code("chained_cast");
}

#[test]
fn cast_chained_with_parens_error() {
    infer(
        r#"
    fn main() {
      let x = (42 as float64) as int;
    }
        "#,
    )
    .assert_infer_code("chained_cast");
}

#[test]
fn cast_inside_range() {
    infer(
        r#"
    fn main() {
      let n: float64 = 10.0;
      for i in 0..n as int {
        let _ = i;
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn cast_byte_alias() {
    infer(
        r#"
    fn main() {
      let a: byte = 255;
      let b: uint8 = 255;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn cast_uint8_slice_to_byte_slice() {
    infer(
        r#"
    fn main() {
      let a: Slice<uint8> = "hello" as Slice<uint8>;
      let b = a as Slice<byte>;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn cast_type_alias_to_byte_slice() {
    infer(
        r#"
    type Bytes = Slice<byte>

    fn main() {
      let b: Bytes = "hello" as Bytes;
      let s: string = b as string;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn cast_type_alias_to_rune_slice() {
    infer(
        r#"
    type Runes = Slice<rune>

    fn main() {
      let r: Runes = "hello" as Runes;
      let s: string = r as string;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn cast_generic_type_target() {
    infer(
        r#"
    fn main() {
      let s = "hello";
      let bytes = s as Slice<byte>;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn cast_in_complex_expression() {
    infer(
        r#"
    fn main() {
      let a: int = 3;
      let b: int = 4;
      let c: int = 2;
      let result: float64 = ((a + b) as float64) / (c as float64);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn cast_int_to_tuple_struct_over_numeric() {
    infer(
        r#"
    struct FileMode(uint32)

    fn main() {
      let mode = 0o644 as FileMode;
      let raw = mode as uint32;
      let from_int = 777 as FileMode;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn integer_literal_overflow_int8() {
    infer(
        r#"
    fn main() {
      let x: int8 = 1000;
    }
        "#,
    )
    .assert_infer_code("integer_literal_overflow");
}

#[test]
fn integer_literal_overflow_uint8() {
    infer(
        r#"
    fn main() {
      let x: uint8 = 256;
    }
        "#,
    )
    .assert_infer_code("integer_literal_overflow");
}

#[test]
fn integer_literal_valid_bounds() {
    infer(
        r#"
    fn main() {
      let a: int8 = 127;
      let b: uint8 = 255;
      let c: int16 = 32767;
      let d: uint16 = 65535;
      let _ = (a, b, c, d);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn numeric_does_not_unify_with_string() {
    infer(
        r#"
    fn get_number() -> int {
      return 42;
    }

    fn main() {
      let x: string = get_number();
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn numeric_does_not_unify_with_bool() {
    infer(
        r#"
    fn get_number() -> int {
      return 42;
    }

    fn main() {
      let x: bool = get_number();
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn constructor_same_type() {
    infer(
        r#"
    fn get_list() -> Slice<int> {
      return [1, 2, 3];
    }

    fn main() {
      let x: Slice<int> = get_list();
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn constructor_different_type_params() {
    infer(
        r#"
    fn get_numbers() -> Slice<int> {
      return [1, 2, 3];
    }

    fn main() {
      let x: Slice<string> = get_numbers();
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn constructor_different_names() {
    infer(
        r#"
    fn get_option() -> Option<int> {
      return Option.Some(42);
    }

    fn main() {
      let x: Result<int, string> = get_option();
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn constructor_nested_params() {
    infer(
        r#"
    fn get_nested() -> Slice<Slice<int>> {
      return [[1, 2], [3, 4]];
    }

    fn main() {
      let x: Slice<Slice<int>> = get_nested();
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn constructor_nested_params_mismatch() {
    infer(
        r#"
    fn get_nested() -> Slice<Slice<int>> {
      return [[1, 2], [3, 4]];
    }

    fn main() {
      let x: Slice<Slice<string>> = get_nested();
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn constructor_different_param_count() {
    infer(
        r#"
    struct Box<T> {
      value: T,
    }

    struct Pair<K, V> {
      key: K,
      value: V,
    }

    fn get_box() -> Box<int> {
      return Box { value: 42 };
    }

    fn main() {
      let x: Pair<int, string> = get_box();
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn interface_definition() {
    infer(
        r#"
    interface ReadWriter {
      fn read() -> string;
      fn write(data: string) -> int;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn type_satisfies_interface() {
    infer(
        r#"
    interface Writer {
      fn write(data: string) -> int;
    }

    struct File {
      path: string,
    }

    impl File {
      fn write(self: File, data: string) -> int {
        return 0;
      }
    }

    fn use_writer(w: Writer) -> int {
      return w.write("hello");
    }

    fn main() {
      let f = File { path: "test.txt" };
      use_writer(f);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn type_satisfies_interface_in_return_type() {
    infer(
        r#"
    interface Writer {
      fn write(data: string) -> int;
    }

    struct File {
      path: string,
    }

    impl File {
      fn write(self: File, data: string) -> int {
        return 42;
      }
    }

    fn get_writer() -> Writer {
      return File { path: "test.txt" };
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn type_satisfies_interface_in_let_binding() {
    infer(
        r#"
    interface Writer {
      fn write(data: string) -> int;
    }

    struct File {
      path: string,
    }

    impl File {
      fn write(self: File, data: string) -> int {
        return 5;
      }
    }

    fn main() {
      let f = File { path: "test.txt" };
      let w: Writer = f;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn type_satisfies_interface_multiple_methods() {
    infer(
        r#"
    interface ReadWriter {
      fn read() -> string;
      fn write(data: string) -> int;
    }

    struct File {
      path: string,
    }

    impl File {
      fn read(self: File) -> string {
        return "contents";
      }

      fn write(self: File, data: string) -> int {
        return 10;
      }
    }

    fn use_rw(rw: ReadWriter) {
      rw.read();
      rw.write("data");
    }

    fn main() {
      let f = File { path: "test.txt" };
      use_rw(f);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn type_satisfies_interface_extra_methods() {
    infer(
        r#"
    interface Writer {
      fn write(data: string) -> int;
    }

    struct File {
      path: string,
    }

    impl File {
      fn write(self: File, data: string) -> int {
        return 0;
      }

      fn close(self: File) {
        // Extra method not in interface - should be fine
      }
    }

    fn use_writer(w: Writer) -> int {
      return w.write("hello");
    }

    fn main() {
      let f = File { path: "test.txt" };
      use_writer(f);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn type_missing_method() {
    infer(
        r#"
    interface Writer {
      fn write(data: string) -> int;
    }

    struct File {
      path: string,
    }

    // No write method implemented

    fn use_writer(w: Writer) -> int {
      return w.write("hello");
    }

    fn main() {
      let f = File { path: "test.txt" };
      use_writer(f);
    }
        "#,
    )
    .assert_infer_code("interface_not_implemented");
}

#[test]
fn type_wrong_method_return_type() {
    infer(
        r#"
    interface Writer {
      fn write(data: string) -> int;
    }

    struct File {
      path: string,
    }

    impl File {
      fn write(self: File, data: string) -> string {
        return "ok";
      }
    }

    fn use_writer(w: Writer) -> int {
      return w.write("hello");
    }

    fn main() {
      let f = File { path: "test.txt" };
      use_writer(f);
    }
        "#,
    )
    .assert_infer_code("interface_not_implemented");
}

#[test]
fn type_wrong_method_parameter_count() {
    infer(
        r#"
    interface Writer {
      fn write(data: string) -> int;
    }

    struct File {
      path: string,
    }

    impl File {
      fn write(self: File) -> int {
        return 0;
      }
    }

    fn use_writer(w: Writer) -> int {
      return w.write("hello");
    }

    fn main() {
      let f = File { path: "test.txt" };
      use_writer(f);
    }
        "#,
    )
    .assert_infer_code("interface_not_implemented");
}

#[test]
fn type_wrong_method_parameter_type() {
    infer(
        r#"
    interface Writer {
      fn write(data: string) -> int;
    }

    struct File {
      path: string,
    }

    impl File {
      fn write(self: File, data: int) -> int {
        return data;
      }
    }

    fn use_writer(w: Writer) -> int {
      return w.write("hello");
    }

    fn main() {
      let f = File { path: "test.txt" };
      use_writer(f);
    }
        "#,
    )
    .assert_infer_code("interface_not_implemented");
}

#[test]
fn type_missing_one_of_multiple_methods() {
    infer(
        r#"
    interface ReadWriter {
      fn read() -> string;
      fn write(data: string) -> int;
    }

    struct File {
      path: string,
    }

    impl File {
      fn read(self: File) -> string {
        return "contents";
      }
      // Missing write method
    }

    fn use_rw(rw: ReadWriter) {
      rw.read();
    }

    fn main() {
      let f = File { path: "test.txt" };
      use_rw(f);
    }
        "#,
    )
    .assert_infer_code("interface_not_implemented");
}

#[test]
fn empty_interface_allows_anything() {
    infer(
        r#"
    interface Empty {
    }

    struct Anything {
      value: int,
    }

    fn use_empty(e: Empty) {
    }

    fn main() {
      let a = Anything { value: 42 };
      use_empty(a);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn function_type_match() {
    infer(
        r#"
    fn double(x: int) -> int {
      return x * 2;
    }

    fn apply(f: fn(int) -> int, x: int) -> int {
      return f(x);
    }

    fn main() -> int {
      return apply(double, 5);
    }
        "#,
    )
    .assert_last_function_type(vec![], int_type());
}

#[test]
fn function_type_mismatch_arity() {
    infer(
        r#"
    fn wrong(x: int, y: int) -> int {
      return x + y;
    }

    fn apply(f: fn(int) -> int, x: int) -> int {
      return f(x);
    }

    fn main() -> int {
      return apply(wrong, 5);
    }
        "#,
    )
    .assert_infer_code("type_mismatch");
}

#[test]
fn function_type_mismatch_return() {
    infer(
        r#"
    fn wrong(x: int) -> bool {
      return x > 0;
    }

    fn apply(f: fn(int) -> int, x: int) -> int {
      return f(x);
    }

    fn main() -> int {
      return apply(wrong, 5);
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn function_type_mismatch_param_type() {
    infer(
        r#"
    fn wrong(x: bool) -> int {
      return 42;
    }

    fn apply(f: fn(int) -> int, x: int) -> int {
      return f(x);
    }

    fn main() -> int {
      return apply(wrong, 5);
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn function_type_match_multiple_params() {
    infer(
        r#"
    fn add(x: int, y: int) -> int {
      return x + y;
    }

    fn apply(f: fn(int, int) -> int, a: int, b: int) -> int {
      return f(a, b);
    }

    fn main() -> int {
      return apply(add, 3, 7);
    }
        "#,
    )
    .assert_last_function_type(vec![], int_type());
}

#[test]
fn function_type_match_bool_return() {
    infer(
        r#"
    fn is_positive(n: int) -> bool {
      return n > 0;
    }

    fn check(predicate: fn(int) -> bool, x: int) -> bool {
      return predicate(x);
    }

    fn main() -> bool {
      return check(is_positive, 10);
    }
        "#,
    )
    .assert_last_function_type(vec![], bool_type());
}

#[test]
fn occurs_check_direct_recursion() {
    infer(
        r#"
    struct Node<T> {
      value: T,
    }

    fn main() {
      let x: Node<Node<Node<int>>> = Node { value: Node { value: Node { value: 42 } } };
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn occurs_check_in_collection() {
    infer(
        r#"
    fn main() {
      let xs: Slice<int> = [];
      let ys = [xs];
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn unknown_in_var_type_allowed_in_lis_file() {
    infer(
        r#"
    fn main() {
      let x: Unknown = get_unknown();
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn unknown_in_param_type_allowed_in_lis_file() {
    infer(
        r#"
    fn process(x: Unknown) -> int {
      return 0;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn unknown_in_return_type_allowed_in_lis_file() {
    infer(
        r#"
    fn get_value() -> Unknown {
      return get_unknown();
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn unknown_in_const_annotation_disallowed() {
    infer(
        r#"
    const X: Unknown = 42;
        "#,
    )
    .assert_infer_code("unknown_in_const_annotation");
}

#[test]
fn unknown_in_const_annotation_nested_disallowed() {
    infer(
        r#"
    const XS: Slice<Unknown> = [];
        "#,
    )
    .assert_infer_code("unknown_in_const_annotation");
}

#[test]
fn unknown_in_const_annotation_via_alias_disallowed() {
    infer(
        r#"
    type Erased = Unknown;
    const X: Erased = 42;
        "#,
    )
    .assert_infer_code("unknown_in_const_annotation");
}

#[test]
fn unknown_in_bound_position_disallowed() {
    infer(
        r#"
    fn f<T: Unknown>(x: T) {}
        "#,
    )
    .assert_infer_code("unknown_in_bound_position");
}

#[test]
fn unknown_in_struct_field_allowed_in_lis_file() {
    infer(
        r#"
    struct Bag {
      pub value: Unknown,
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn unknown_in_let_mut_map_allowed() {
    infer(
        r#"
    fn main() {
      let mut m: Map<string, Unknown> = Map.new();
      m["k"] = "alice";
      m["n"] = 42;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn unknown_in_slice_literal_heterogeneous() {
    infer(
        r#"
    fn main() {
      let xs: Slice<Unknown> = ["alice", 30, true];
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn unknown_alias_declares_without_error() {
    infer(
        r#"
    type Claims = Map<string, Unknown>;
    type Erased = Unknown;
        "#,
    )
    .assert_no_errors();
}

#[test]
fn unknown_as_map_key_surface_disallowed() {
    infer(
        r#"
    fn main() {
      let m: Map<Unknown, int> = Map.new();
    }
        "#,
    )
    .assert_infer_code("unknown_as_map_key");
}

#[test]
fn unknown_as_map_key_turbofish_compiles() {
    // Non-surface forms inherit Go's runtime panic semantics; only direct
    // surface annotations of `Map<Unknown, _>` are statically rejected.
    infer(
        r#"
    fn main() {
      let m = Map.new<Unknown, int>();
      let _ = m;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn unknown_as_map_key_via_alias_disallowed() {
    infer(
        r#"
    type K = Unknown;
    fn main() {
      let m: Map<K, int> = Map.new();
    }
        "#,
    )
    .assert_infer_code("unknown_as_map_key");
}

#[test]
fn unknown_as_map_key_via_nested_alias_disallowed() {
    infer(
        r#"
    type K1 = Unknown;
    type K2 = K1;
    fn main() {
      let m: Map<K2, int> = Map.new();
    }
        "#,
    )
    .assert_infer_code("unknown_as_map_key");
}

#[test]
fn unknown_as_map_key_turbofish_via_alias_compiles() {
    infer(
        r#"
    type K = Unknown;
    fn main() {
      let m = Map.new<K, int>();
      let _ = m;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn unknown_as_map_key_via_generic_alias_wrapping_map_compiles() {
    infer(
        r#"
    type Claims<K, V> = Map<K, V>;
    fn make<K, V>() -> Claims<K, V> { Map.new() }
    fn main() {
      let m = make<Unknown, int>();
      let _ = m;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn unknown_as_map_key_method_only_turbofish_compiles() {
    infer(
        r#"
    struct Bag<T> {}
    impl<T> Bag<T> {
      pub fn make<K>(self) -> Map<K, T> { Map.new() }
    }
    fn main() {
      let b: Bag<int> = Bag {};
      let _ = b.make<Unknown>();
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn option_unknown_accepts_concrete_some() {
    infer(
        r#"
    fn opt_unknown() -> Option<Unknown> {
      Some("x")
    }
    fn main() {
      let _ = opt_unknown();
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn result_unknown_accepts_concrete_ok() {
    infer(
        r#"
    fn ok_unknown() -> Result<Unknown, error> {
      Ok(42)
    }
    fn main() {
      let _ = ok_unknown();
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn unknown_as_map_key_via_alias_at_annotation_site_compiles() {
    infer(
        r#"
    type AnyMap<K, V> = Map<K, V>;
    fn main() {
      let m: AnyMap<Unknown, int> = Map.new();
      let _ = m;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn unknown_map_return_from_typedef_api_allowed() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "ffi",
        "bindings.d.lis",
        r#"
    pub fn make() -> Map<Unknown, int>
        "#,
    );
    fs.add_file(
        "ffi",
        "main.lis",
        r#"
    fn main() {
      let _ = make()
    }
        "#,
    );
    infer_module("ffi", fs).assert_no_errors();
}

#[test]
fn unknown_as_map_key_via_inferred_receiver_compiles() {
    infer(
        r#"
    struct Bag<K> {}
    impl<K> Bag<K> {
      fn make(self) -> Map<K, int> { Map.new() }
    }
    fn main() {
      let b: Bag<Unknown> = Bag {};
      let _ = b.make();
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn tuple_unknown_accepts_concrete_at_unknown_position() {
    infer(
        r#"
    fn pair() -> (int, Unknown) {
      (1, "two")
    }
    fn main() {
      let _ = pair();
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn unknown_in_param_type_allowed_in_typedef_file() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "ffi",
        "bindings.d.lis",
        r#"
    fn external_call(x: Unknown) -> Unknown {
      return x;
    }
        "#,
    );
    infer_module("ffi", fs).assert_no_errors();
}

#[test]
fn unknown_accepts_concrete_type_upcast() {
    infer(
        r#"
    fn main() {
      takes_unknown(42);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn unknown_rejects_downcast_to_concrete() {
    infer(
        r#"
    fn process(x: int) -> int { x }
    fn test() {
      let data = get_unknown();
      process(data)
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn unknown_map_accepts_matching_unknown_map() {
    infer(
        r#"
    fn test() {
      let m = get_unknown_map();
      takes_unknown_map(m)
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn unknown_map_rejects_concrete_value_type() {
    infer(
        r#"
    fn test() {
      takes_unknown_map(Map.from([("k", "v")]))
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn unknown_slice_rejects_concrete_element_type() {
    infer(
        r#"
    fn test() {
      let xs: Slice<int> = [1, 2, 3]
      takes_unknown_slice(xs)
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn explicit_type_arg() {
    infer(
        r#"
    fn identity<T>(x: T) -> T {
      return x;
    }

    fn main() -> int {
      identity<int>(42)
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn explicit_type_arg_on_assert_type() {
    infer(
        r#"{
      let value = get_unknown();
      assert_type<int>(value)
    }"#,
    )
    .assert_type_struct_generic("Option", vec![int_type()]);
}

#[test]
fn explicit_type_args_arity_mismatch() {
    infer(
        r#"
    fn identity<T>(x: T) -> T {
      return x;
    }

    fn main() {
      let x = identity<int, string>(42);
    }
        "#,
    )
    .assert_infer_code("type_arg_count_mismatch");
}

#[test]
fn function_vs_constructor_error() {
    infer(
        r#"
    fn get_func() -> fn(int) -> int {
      let double = |x: int| -> int { x * 2 };
      return double;
    }

    fn main() {
      let x: int = get_func();
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn constructor_vs_function_error() {
    infer(
        r#"
    fn get_number() -> int {
      return 42;
    }

    fn main() {
      let f: fn(int) -> int = get_number();
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn both_refs_unify() {
    infer(
        r#"
    fn main() {
      let x = 5;
      let y = 10;
      let r = if true { &x } else { &y };
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn ref_does_not_unify_with_concrete() {
    infer(
        r#"
    fn main() {
      let x = 5;
      let r: int = &x;
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn ref_with_type_variable_in_collection() {
    infer(
        r#"
    fn main() {
      let x = 5;
      let refs = [&x];
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn nested_refs_both_sides() {
    infer(
        r#"
    fn main() {
      let x = 5;
      let y = 10;
      let r1 = &x;
      let r2 = &y;
      let rr = if true { &r1 } else { &r2 };
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn ref_inner_type_mismatch() {
    infer(
        r#"
    fn main() {
      let x = 5;
      let y = "hello";
      let r = if true { &x } else { &y };
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn ref_annotation_with_concrete_value() {
    infer(
        r#"
    fn main() {
      let x: Ref<int> = 5;
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn interface_inheritance_simple() {
    infer(
        r#"
    interface Display {
      fn show() -> string;
    }

    interface Logger {
      impl Display;
      fn log() -> ();
    }

    struct File {
      path: string,
    }

    impl File {
      fn show(self: File) -> string {
        return self.path;
      }

      fn log(self: File) {
        let _ = self.path;
      }
    }

    fn use_logger(l: Logger) {
      l.show();
      l.log();
    }

    fn main() {
      let f = File { path: "test.txt" };
      use_logger(f);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn interface_inheritance_methods_available() {
    infer(
        r#"
    interface Display {
      fn show() -> string;
    }

    interface Logger {
      impl Display;
      fn log() -> ();
    }

    struct File {
      path: string,
    }

    impl File {
      fn show(self: File) -> string {
        return self.path;
      }

      fn log(self: File) {
        let _ = self.path;
      }
    }

    fn use_logger(l: Logger) {
      let s: string = l.show();
      l.log();
    }

    fn main() {
      let f = File { path: "test.txt" };
      use_logger(f);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn interface_inheritance_with_type_parameters() {
    infer(
        r#"
    interface Display<T> {
      fn show() -> string;
    }

    interface Logger<T> {
      impl Display<T>;
      fn log() -> ();
    }

    struct File {
      path: string,
    }

    impl File {
      fn show(self: File) -> string {
        return self.path;
      }

      fn log(self: File) {
        let _ = self.path;
      }
    }

    fn use_logger(l: Logger<File>) {
      let s: string = l.show();
      l.log();
    }

    fn main() {
      let f = File { path: "test.txt" };
      use_logger(f);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn interface_inheritance_missing_parent_method_produces_error() {
    infer(
        r#"
    interface Display {
      fn show() -> string;
    }

    interface Logger {
      impl Display;
      fn log() -> ();
    }

    struct File {
      path: string,
    }

    impl File {
      fn log(self: File) {
        let _ = self.path;
      }
    }

    fn use_logger(l: Logger) {
      l.log();
    }

    fn main() {
      let f = File { path: "test.txt" };
      use_logger(f);
    }
        "#,
    )
    .assert_infer_code("interface_not_implemented");
}

#[test]
fn invariance_slice_rejects_covariant_assignment() {
    infer(
        r#"
    interface Writer {
      fn write(data: string) -> int;
    }

    struct File {
      path: string,
    }

    impl File {
      fn write(self: File, data: string) -> int {
        return 42;
      }
    }

    fn use_writers(writers: Slice<Writer>) {
    }

    fn main() {
      let files: Slice<File> = [File { path: "a.txt" }];
      use_writers(files);
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn invariance_map_rejects_covariant_value() {
    infer(
        r#"
    interface Writer {
      fn write(data: string) -> int;
    }

    struct File {
      path: string,
    }

    impl File {
      fn write(self: File, data: string) -> int {
        return 42;
      }
    }

    fn use_writer_map(m: Map<string, Writer>) {
    }

    fn main() {
      let files: Map<string, File> = {};
      use_writer_map(files);
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn invariance_generic_struct_rejects_covariant() {
    infer(
        r#"
    interface Writer {
      fn write(data: string) -> int;
    }

    struct File {
      path: string,
    }

    impl File {
      fn write(self: File, data: string) -> int {
        return 42;
      }
    }

    struct Box<T> {
      value: T,
    }

    fn use_writer_box(b: Box<Writer>) {
    }

    fn main() {
      let file_box: Box<File> = Box { value: File { path: "a.txt" } };
      use_writer_box(file_box);
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn invariance_same_element_type_allowed() {
    infer(
        r#"
    fn process(numbers: Slice<int>) -> int {
      return numbers[0];
    }

    fn main() {
      let nums: Slice<int> = [1, 2, 3];
      process(nums);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn invariance_with_type_variable_allowed() {
    infer(
        r#"
    fn first<T>(items: Slice<T>) -> T {
      return items[0];
    }

    fn main() {
      let nums = [1, 2, 3];
      let x: int = first(nums);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn invariance_nested_generics_rejected() {
    infer(
        r#"
    interface Writer {
      fn write(data: string) -> int;
    }

    struct File {
      path: string,
    }

    impl File {
      fn write(self: File, data: string) -> int {
        return 42;
      }
    }

    fn use_nested(items: Slice<Slice<Writer>>) {
    }

    fn main() {
      let files: Slice<Slice<File>> = [[File { path: "a.txt" }]];
      use_nested(files);
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn expected_type_propagation_option_with_interface() {
    infer(
        r#"
    interface Printable {
      fn display(self) -> string
    }

    struct Text { content: string }

    impl Text {
      fn display(self) -> string { self.content }
    }

    fn main() {
      let a: Option<Printable> = Some(Text { content: "hello" })
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn expected_type_propagation_result_with_interface() {
    infer(
        r#"
    interface Printable {
      fn display(self) -> string
    }

    struct Text { content: string }

    impl Text {
      fn display(self) -> string { self.content }
    }

    fn main() {
      let a: Result<Printable, error> = Ok(Text { content: "hello" })
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn expected_type_propagation_does_not_make_variable_covariant() {
    infer(
        r#"
    interface Printable {
      fn display(self) -> string
    }

    struct Text { content: string }

    impl Text {
      fn display(self) -> string { self.content }
    }

    fn main() {
      let x: Option<Text> = Some(Text { content: "hello" })
      let y: Option<Printable> = x
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn match_on_unconstrained_type_var_invents_structure() {
    infer(
        r#"
    fn get_something<T>() -> T {
      return get_something();
    }

    fn main() {
      let x = get_something();
      match x {
        (a, b) => {
          let _: int = a;
          let _: string = b;
        },
      };
    }
        "#,
    )
    .assert_infer_code("cannot_match_on_unconstrained_type");
}

#[test]
fn function_types_different_bounds_do_not_unify() {
    infer(
        r#"
    interface Display {
      fn show() -> string;
    }

    interface Cloneable {
      fn make_copy() -> int;
    }

    fn with_display<T: Display>(x: T) -> T {
      return x;
    }

    fn with_clone<T: Cloneable>(x: T) -> T {
      return x;
    }

    struct Point {
      x: int,
    }

    impl Point {
      fn show(self: Point) -> string {
        return "p";
      }

      fn make_copy(self: Point) -> int {
        return 1;
      }
    }

    fn main() {
      let funcs = [with_display, with_clone];
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn function_types_same_bounds_unify() {
    infer(
        r#"
    interface Display {
      fn show() -> string;
    }

    fn format1<T: Display>(x: T) -> T {
      return x;
    }

    fn format2<U: Display>(y: U) -> U {
      return y;
    }

    struct Point {
      x: int,
    }

    impl Point {
      fn show(self: Point) -> string {
        return "p";
      }
    }

    fn main() {
      let funcs = [format1, format2];
      funcs[0](Point { x: 1 });
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn negative_literal_adapts_to_signed_type() {
    infer(
        r#"
    fn main() {
      let a: int8 = -1;
      let b: int8 = -128;
      let c: int16 = -32768;
      let d: int32 = -2147483648;
      let _ = (a, b, c, d);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn negative_literal_overflow_signed_type() {
    infer(
        r#"
    fn main() {
      let x: int8 = -129;
    }
        "#,
    )
    .assert_infer_code("integer_literal_overflow");
}

#[test]
fn negative_literal_for_unsigned_type_error() {
    infer(
        r#"
    fn main() {
      let x: uint8 = -1;
    }
        "#,
    )
    .assert_infer_code("cannot_negate_unsigned");
}

#[test]
fn negative_zero_for_unsigned_type_allowed() {
    infer(
        r#"
    fn main() {
      let x: uint8 = -0;
      let _ = x;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn negation_of_unsigned_variable_error() {
    infer(
        r#"
    fn main() {
      let u: uint8 = 5;
      let z = -u;
      let _ = z;
    }
        "#,
    )
    .assert_infer_code("cannot_negate_unsigned");
}

#[test]
fn negation_of_unsigned_zero_literal_allowed() {
    infer(
        r#"
    fn main() {
      let a: uint8 = -0;
      let _ = a;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn negation_of_unsigned_paren_zero_literal_allowed() {
    infer(
        r#"
    fn main() {
      let a: uint8 = -(0);
      let _ = a;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn negative_paren_integer_literal_overflow() {
    infer(
        r#"
    fn main() {
      let x: int8 = -(129);
      let _ = x;
    }
        "#,
    )
    .assert_infer_code("integer_literal_overflow");
}

#[test]
fn negative_float_literal_overflow() {
    infer(
        r#"
    fn main() {
      let x: float32 = -3.5e39;
    }
        "#,
    )
    .assert_infer_code("float_literal_overflow");
}

#[test]
fn numeric_int_literal_coerces_in_comparison_rhs() {
    infer(
        r#"
    fn main() {
      let b: float64 = 3.0;
      if b == 0 {}
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn numeric_int_literal_coerces_in_comparison_lhs() {
    infer(
        r#"
    fn main() {
      let b: float64 = 3.0;
      if 0 == b {}
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn numeric_int_literal_coerces_in_arithmetic() {
    infer(
        r#"
    fn main() {
      let b: float64 = 3.0;
      let c: float64 = b + 1;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn numeric_int_literal_coerces_in_ordering() {
    infer(
        r#"
    fn main() {
      let b: float64 = 3.0;
      if b > 0 {}
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn interface_subtype_satisfies_supertype() {
    infer(
        r#"
    interface Reader {
      fn Read() -> int
    }

    interface ReadCloser {
      fn Read() -> int
      fn Close() -> int
    }

    fn take_reader(r: Reader) -> int {
      r.Read()
    }

    fn give_read_closer() -> ReadCloser {
      panic("stub")
    }

    fn main() {
      let rc = give_read_closer()
      take_reader(rc)
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn interface_subtype_satisfies_supertype_in_return_type() {
    infer(
        r#"
    interface Reader {
      fn Read() -> int
    }

    interface ReadCloser {
      fn Read() -> int
      fn Close() -> int
    }

    fn give_read_closer() -> ReadCloser {
      panic("stub")
    }

    fn get_reader() -> Reader {
      give_read_closer()
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn interface_subtype_satisfies_supertype_direct() {
    infer(
        r#"
    interface Reader {
      fn Read() -> int
    }

    interface ReadCloser {
      fn Read() -> int
      fn Close() -> int
    }

    fn take_reader(r: Reader) -> int {
      r.Read()
    }

    fn take_read_closer(rc: ReadCloser) {
      take_reader(rc)
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn interface_supertype_does_not_satisfy_subtype() {
    infer(
        r#"
    interface Reader {
      fn Read() -> int
    }

    interface ReadCloser {
      fn Read() -> int
      fn Close() -> int
    }

    fn take_read_closer(rc: ReadCloser) -> int {
      rc.Read()
    }

    fn give_reader() -> Reader {
      panic("stub")
    }

    fn main() {
      let r = give_reader()
      take_read_closer(r)
    }
        "#,
    )
    .assert_infer_code("interface_not_implemented");
}

#[test]
fn if_branches_interface_subtype_either_order() {
    // Subtype in first branch, supertype in second.
    infer(
        r#"
    interface Reader {
      fn Read() -> int
    }

    interface ReadCloser {
      fn Read() -> int
      fn Close() -> int
    }

    fn get_reader() -> Reader { panic("stub") }
    fn get_read_closer() -> ReadCloser { panic("stub") }

    fn main() {
      let x = true
      let a = if x { get_read_closer() } else { get_reader() }
      a.Read()
    }
        "#,
    )
    .assert_no_errors();

    // Supertype in first branch, subtype in second.
    infer(
        r#"
    interface Reader {
      fn Read() -> int
    }

    interface ReadCloser {
      fn Read() -> int
      fn Close() -> int
    }

    fn get_reader() -> Reader { panic("stub") }
    fn get_read_closer() -> ReadCloser { panic("stub") }

    fn main() {
      let x = true
      let a = if x { get_reader() } else { get_read_closer() }
      a.Read()
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn if_branches_incompatible_interfaces_rejected() {
    infer(
        r#"
    interface Reader {
      fn Read() -> int
    }

    interface Writer {
      fn Write() -> int
    }

    fn get_reader() -> Reader { panic("stub") }
    fn get_writer() -> Writer { panic("stub") }

    fn main() {
      let x = true
      let a = if x { get_reader() } else { get_writer() }
    }
        "#,
    )
    .assert_infer_code("interface_not_implemented");
}

#[test]
fn match_branches_interface_subtype_either_order() {
    // Subtype in first arm, supertype in second.
    infer(
        r#"
    interface Reader {
      fn Read() -> int
    }

    interface ReadCloser {
      fn Read() -> int
      fn Close() -> int
    }

    fn get_reader() -> Reader { panic("stub") }
    fn get_read_closer() -> ReadCloser { panic("stub") }

    fn main() {
      let x = 1
      let a = match x {
        1 => get_read_closer(),
        _ => get_reader(),
      }
      a.Read()
    }
        "#,
    )
    .assert_no_errors();

    // Supertype in first arm, subtype in second.
    infer(
        r#"
    interface Reader {
      fn Read() -> int
    }

    interface ReadCloser {
      fn Read() -> int
      fn Close() -> int
    }

    fn get_reader() -> Reader { panic("stub") }
    fn get_read_closer() -> ReadCloser { panic("stub") }

    fn main() {
      let x = 1
      let a = match x {
        1 => get_reader(),
        _ => get_read_closer(),
      }
      a.Read()
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn match_branches_incompatible_interfaces_rejected() {
    infer(
        r#"
    interface Reader {
      fn Read() -> int
    }

    interface Writer {
      fn Write() -> int
    }

    fn get_reader() -> Reader { panic("stub") }
    fn get_writer() -> Writer { panic("stub") }

    fn main() {
      let x = 1
      let a = match x {
        1 => get_reader(),
        _ => get_writer(),
      }
    }
        "#,
    )
    .assert_infer_code("interface_not_implemented");
}

#[test]
fn self_referential_fbound_accepts_matching_type() {
    infer(
        r#"
        pub interface Cloner<T: Cloner<T>> {
          fn clone(self) -> T
        }

        struct Foo{}

        impl Foo {
          fn clone(self) -> Foo { Foo{} }
        }

        fn squiggle<A: Cloner<B>, B>(_: A, _: B) {}

        fn main() {
          squiggle(Foo{}, Foo{})
        }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn self_referential_fbound_rejects_mismatched_type() {
    infer(
        r#"
        pub interface Cloner<T: Cloner<T>> {
          fn clone(self) -> T
        }

        struct Foo{}

        impl Foo {
          fn clone(self) -> Foo { Foo{} }
        }

        fn squiggle<A: Cloner<B>, B>(_: A, _: B) {}

        fn main() {
          squiggle(Foo{}, "hello")
        }
        "#,
    )
    .assert_infer_code("interface_not_implemented");
}
