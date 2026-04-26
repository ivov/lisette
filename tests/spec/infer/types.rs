use crate::spec::infer::*;

#[test]
fn simple_struct_instantiation() {
    infer(
        r#"{
    struct Point {
      x: int,
      y: int,
    }
    Point { x: 1, y: 2 }
    }"#,
    )
    .assert_type_struct("Point");
}

#[test]
fn struct_with_different_field_types() {
    infer(
        r#"{
    struct Person {
      name: string,
      age: int,
      active: bool,
    }
    Person { name: "Alice", age: 30, active: true }
    }"#,
    )
    .assert_type_struct("Person");
}

#[test]
fn struct_with_single_field() {
    infer(
        r#"{
    struct Container {
      value: int,
    }
    Container { value: 42 }
    }"#,
    )
    .assert_type_struct("Container");
}

#[test]
fn struct_field_access() {
    infer(
        r#"{
    struct Point {
      x: int,
      y: int,
    }
    let p = Point { x: 10, y: 20 };
    p.x
    }"#,
    )
    .assert_type_int();
}

#[test]
fn struct_field_access_different_types() {
    infer(
        r#"{
    struct Person {
      name: string,
      age: int,
    }
    let person = Person { name: "Bob", age: 25 };
    person.name
    }"#,
    )
    .assert_type_string();
}

#[test]
fn struct_in_let_binding() {
    infer(
        r#"{
    struct Point {
      x: int,
      y: int,
    }
    let p = Point { x: 5, y: 10 };
    p
    }"#,
    )
    .assert_type_struct("Point");
}

#[test]
fn struct_field_in_expression() {
    infer(
        r#"{
    struct Point {
      x: int,
      y: int,
    }
    let p = Point { x: 5, y: 10 };
    p.x + p.y
    }"#,
    )
    .assert_type_int();
}

#[test]
fn struct_as_function_argument() {
    infer(
        r#"{
    struct Point {
      x: int,
      y: int,
    }
    let get_x = |p: Point| -> int { p.x };
    get_x(Point { x: 10, y: 20 })
    }"#,
    )
    .assert_type_int();
}

#[test]
fn struct_as_function_return() {
    infer(
        r#"{
    struct Point {
      x: int,
      y: int,
    }
    let make_point = || -> Point { Point { x: 1, y: 2 } };
    make_point()
    }"#,
    )
    .assert_type_struct("Point");
}

#[test]
fn struct_with_expression_fields() {
    infer(
        r#"{
    struct Point {
      x: int,
      y: int,
    }
    Point { x: 1 + 2, y: 3 * 4 }
    }"#,
    )
    .assert_type_struct("Point");
}

#[test]
fn struct_with_variable_fields() {
    infer(
        r#"{
    struct Point {
      x: int,
      y: int,
    }
    let a = 10;
    let b = 20;
    Point { x: a, y: b }
    }"#,
    )
    .assert_type_struct("Point");
}

#[test]
fn nested_struct() {
    infer(
        r#"{
    struct Inner {
      value: int,
    }
    struct Outer {
      inner: Inner,
    }
    Outer { inner: Inner { value: 42 } }
    }"#,
    )
    .assert_type_struct("Outer");
}

#[test]
fn nested_struct_field_access() {
    infer(
        r#"{
    struct Inner {
      value: int,
    }
    struct Outer {
      inner: Inner,
    }
    let o = Outer { inner: Inner { value: 42 } };
    o.inner.value
    }"#,
    )
    .assert_type_int();
}

#[test]
fn struct_wrong_field_type() {
    infer(
        r#"{
    struct Point {
      x: int,
      y: int,
    }
    Point { x: "wrong", y: 2 }
    }"#,
    )
    .assert_type_mismatch();
}

#[test]
fn struct_undefined() {
    infer("UndefinedStruct { x: 1 }").assert_resolve_code("struct_not_found");
}

#[test]
fn struct_undefined_field() {
    infer(
        r#"{
    struct Point {
      x: int,
      y: int,
    }
    Point { x: 1, y: 2, z: 3 }
    }"#,
    )
    .assert_infer_code("member_not_found");
}

#[test]
fn generic_struct_single_type_param() {
    infer(
        r#"{
    struct Container<T> {
      value: T,
    }
    Container { value: 42 }
    }"#,
    )
    .assert_type_struct_generic("Container", vec![int_type()]);
}

#[test]
fn generic_struct_string_param() {
    infer(
        r#"{
    struct Container<T> {
      value: T,
    }
    Container { value: "hello" }
    }"#,
    )
    .assert_type_struct_generic("Container", vec![string_type()]);
}

#[test]
fn generic_struct_bool_param() {
    infer(
        r#"{
    struct Container<T> {
      value: T,
    }
    Container { value: true }
    }"#,
    )
    .assert_type_struct_generic("Container", vec![bool_type()]);
}

#[test]
fn generic_struct_two_type_params() {
    infer(
        r#"{
    struct Pair<K, V> {
      key: K,
      value: V,
    }
    Pair { key: "name", value: 42 }
    }"#,
    )
    .assert_type_struct_generic("Pair", vec![string_type(), int_type()]);
}

#[test]
fn generic_struct_same_type_params() {
    infer(
        r#"{
    struct Pair<K, V> {
      key: K,
      value: V,
    }
    Pair { key: 1, value: 2 }
    }"#,
    )
    .assert_type_struct_generic("Pair", vec![int_type(), int_type()]);
}

#[test]
fn generic_struct_field_access() {
    infer(
        r#"{
    struct Container<T> {
      value: T,
    }
    let c = Container { value: 42 };
    c.value
    }"#,
    )
    .assert_type_int();
}

#[test]
fn generic_struct_field_access_string() {
    infer(
        r#"{
    struct Container<T> {
      value: T,
    }
    let c = Container { value: "test" };
    c.value
    }"#,
    )
    .assert_type_string();
}

#[test]
fn generic_struct_multiple_fields_access() {
    infer(
        r#"{
    struct Pair<K, V> {
      key: K,
      value: V,
    }
    let p = Pair { key: "id", value: 123 };
    p.value
    }"#,
    )
    .assert_type_int();
}

#[test]
fn generic_struct_in_let_binding() {
    infer(
        r#"{
    struct Container<T> {
      value: T,
    }
    let c = Container { value: 10 };
    c
    }"#,
    )
    .assert_type_struct_generic("Container", vec![int_type()]);
}

#[test]
fn generic_struct_field_in_expression() {
    infer(
        r#"{
    struct Container<T> {
      value: T,
    }
    let c = Container { value: 5 };
    c.value + 10
    }"#,
    )
    .assert_type_int();
}

#[test]
fn generic_struct_as_function_argument() {
    infer(
        r#"{
    struct Container<T> {
      value: T,
    }
    let get_value = |c: Container<int>| -> int { c.value };
    get_value(Container { value: 42 })
    }"#,
    )
    .assert_type_int();
}

#[test]
fn generic_struct_as_function_return() {
    infer(
        r#"{
    struct Container<T> {
      value: T,
    }
    let make_container = || -> Container<int> { Container { value: 42 } };
    make_container()
    }"#,
    )
    .assert_type_struct_generic("Container", vec![int_type()]);
}

#[test]
fn generic_struct_with_expression_field() {
    infer(
        r#"{
    struct Container<T> {
      value: T,
    }
    Container { value: 1 + 2 }
    }"#,
    )
    .assert_type_struct_generic("Container", vec![int_type()]);
}

#[test]
fn generic_struct_with_variable_field() {
    infer(
        r#"{
    struct Container<T> {
      value: T,
    }
    let x = 10;
    Container { value: x }
    }"#,
    )
    .assert_type_struct_generic("Container", vec![int_type()]);
}

#[test]
fn nested_generic_struct() {
    infer(
        r#"{
    struct Container<T> {
      value: T,
    }
    struct Outer<T> {
      inner: Container<T>,
    }
    Outer { inner: Container { value: 42 } }
    }"#,
    )
    .assert_type_struct_generic("Outer", vec![int_type()]);
}

#[test]
fn nested_generic_struct_field_access() {
    infer(
        r#"{
    struct Container<T> {
      value: T,
    }
    struct Outer<T> {
      inner: Container<T>,
    }
    let o = Outer { inner: Container { value: 42 } };
    o.inner.value
    }"#,
    )
    .assert_type_int();
}

#[test]
fn generic_struct_wrong_field_type() {
    infer(
        r#"{
    struct Container<T> {
      value: T,
    }
    fn needs_int_container(c: Container<int>) -> int {
      return c.value;
    }
    needs_int_container(Container { value: "wrong" })
    }"#,
    )
    .assert_type_mismatch();
}

#[test]
fn generic_struct_mismatched_type_params() {
    infer(
        r#"{
    struct Pair<K, V> {
      first: K,
      second: K,
    }
    Pair { first: 1, second: "wrong" }
    }"#,
    )
    .assert_type_mismatch();
}

#[test]
fn static_method_declares_without_errors() {
    infer(
        r#"
    struct Counter {
      value: int,
    }

    impl Counter {
      fn static_test() -> int {
        return 42;
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn static_method_with_parameters() {
    infer(
        r#"
    struct Math {}

    impl Math {
      fn add(a: int, b: int) -> int {
        return a + b;
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn instance_method_explicit_self() {
    infer(
        r#"
    struct Counter {
      value: int,
    }

    impl Counter {
      fn get(self: Counter) -> int {
        return self.value;
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn instance_method_field_access() {
    infer(
        r#"
    struct Point {
      x: int,
      y: int,
    }

    impl Point {
      fn get_x(self: Point) -> int {
        return self.x;
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn instance_method_multiple_fields() {
    infer(
        r#"
    struct Point {
      x: int,
      y: int,
    }

    impl Point {
      fn sum(self: Point) -> int {
        return self.x + self.y;
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn instance_method_with_parameters() {
    infer(
        r#"
    struct Counter {
      value: int,
    }

    impl Counter {
      fn add(self: Counter, amount: int) -> int {
        return self.value + amount;
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn multiple_methods_in_impl() {
    infer(
        r#"
    struct Counter {
      value: int,
    }

    impl Counter {
      fn get(self: Counter) -> int {
        return self.value;
      }

      fn double(self: Counter) -> int {
        return self.value + self.value;
      }

      fn static_default() -> int {
        return 0;
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn generic_struct_impl() {
    infer(
        r#"
    struct Container<T> {
      value: T,
    }

    impl<T> Container<T> {
      fn get(self: Container<T>) -> T {
        return self.value;
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn generic_impl_multiple_methods() {
    infer(
        r#"
    struct Box<T> {
      item: T,
    }

    impl<T> Box<T> {
      fn get(self: Box<T>) -> T {
        return self.item;
      }

      fn set(self: Box<T>, new_item: T) -> Box<T> {
        return Box { item: new_item };
      }

      fn is_empty(self: Box<T>) -> bool {
        return false;
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn generic_impl_with_static_constructor() {
    infer(
        r#"
    struct Wrapper<T> {
      value: T,
    }

    impl<T> Wrapper<T> {
      fn new(value: T) -> Wrapper<T> {
        return Wrapper { value: value };
      }

      fn unwrap(self: Wrapper<T>) -> T {
        return self.value;
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn generic_method_call_on_concrete_instance() {
    infer(
        r#"
    struct Box<T> {
      value: T,
    }

    impl<T> Box<T> {
      fn get(self: Box<T>) -> T {
        self.value
      }
    }

    fn main() -> int {
      let b = Box { value: 42 };
      b.get()
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn generic_method_call_infers_correct_return_type() {
    infer(
        r#"
    struct Container<T> {
      item: T,
    }

    impl<T> Container<T> {
      fn get(self: Container<T>) -> T {
        self.item
      }
    }

    fn main() -> string {
      let c = Container { item: "hello" };
      c.get()
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn static_method_call() {
    infer(
        r#"
    struct Point { x: int, y: int }

    impl Point {
      fn new(x: int, y: int) -> Point {
        Point { x: x, y: y }
      }
    }

    fn main() -> int {
      let p = Point.new(1, 2);
      p.x
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn static_method_with_instance_method() {
    infer(
        r#"
    struct Counter { value: int }

    impl Counter {
      fn new(start: int) -> Counter {
        Counter { value: start }
      }

      fn get(self: Counter) -> int {
        self.value
      }
    }

    fn main() -> int {
      let c = Counter.new(10);
      c.get()
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn multiple_static_methods() {
    infer(
        r#"
    struct Point { x: int, y: int }

    impl Point {
      fn new(x: int, y: int) -> Point {
        Point { x: x, y: y }
      }

      fn origin() -> Point {
        Point { x: 0, y: 0 }
      }
    }

    fn main() -> int {
      let p1 = Point.new(1, 2);
      let p2 = Point.origin();
      p1.x + p2.x
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn static_method_called_on_self_is_error() {
    infer(
        r#"
        struct Foo {}
        impl Foo {
          fn bar() {}
          fn baz(self) {
            self.bar()
          }
        }
        "#,
    )
    .assert_infer_code("static_method_on_instance");
}

#[test]
fn static_method_called_on_instance_binding_is_error() {
    infer(
        r#"
        struct Counter { value: int }
        impl Counter {
          fn new(start: int) -> Counter {
            Counter { value: start }
          }
        }
        fn main() {
          let c = Counter.new(1)
          c.new(2)
        }
        "#,
    )
    .assert_infer_code("static_method_on_instance");
}

#[test]
fn static_method_called_on_type_alias_still_works() {
    infer(
        r#"
        struct Counter { value: int }
        type CounterAlias = Counter
        impl Counter {
          fn new(start: int) -> Counter {
            Counter { value: start }
          }
        }
        fn main() {
          let _ = CounterAlias.new(1)
        }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn generic_option_some() {
    infer(
        r#"{
    Some(42)
    }"#,
    )
    .assert_type_struct_generic("Option", vec![int_type()]);
}

#[test]
fn generic_result_with_both_types() {
    infer(
        r#"{
    let test = |success: bool| -> Result<int, string> {
      if success { Ok(42) } else { Err("error") }
    };
    test
    }"#,
    )
    .assert_function_type(
        vec![bool_type()],
        con_type("Result", vec![int_type(), string_type()]),
    );
}

#[test]
fn nested_generic_option_of_result() {
    infer(
        r#"{
    let result: Option<Result<int, string>> = Some(Ok(42));
    result
    }"#,
    )
    .assert_type_struct_generic(
        "Option",
        vec![con_type("Result", vec![int_type(), string_type()])],
    );
}

#[test]
fn nested_generic_result_of_option() {
    infer(
        r#"{
    let result: Result<Option<int>, string> = Ok(Some(42));
    result
    }"#,
    )
    .assert_type_struct_generic(
        "Result",
        vec![con_type("Option", vec![int_type()]), string_type()],
    );
}

#[test]
fn nested_generic_option_of_option() {
    infer(
        r#"{
    Some(Some(42))
    }"#,
    )
    .assert_type_struct_generic("Option", vec![con_type("Option", vec![int_type()])]);
}

#[test]
fn triple_nested_generic() {
    infer(
        r#"{
    Some(Some(Some(42)))
    }"#,
    )
    .assert_type_struct_generic(
        "Option",
        vec![con_type(
            "Option",
            vec![con_type("Option", vec![int_type()])],
        )],
    );
}

#[test]
fn deeply_nested_mixed_generics() {
    infer(
        r#"{
    let nested: Option<Result<Option<int>, string>> = Some(Ok(Some(42)));
    nested
    }"#,
    )
    .assert_type_struct_generic(
        "Option",
        vec![con_type(
            "Result",
            vec![con_type("Option", vec![int_type()]), string_type()],
        )],
    );
}

#[test]
fn generic_struct_containing_generic() {
    infer(
        r#"{
    struct Container<T> { value: T }
    Container { value: Some(42) }
    }"#,
    )
    .assert_type_struct_generic("Container", vec![con_type("Option", vec![int_type()])]);
}

#[test]
fn generic_struct_containing_option_field_access() {
    infer(
        r#"{
    struct Container<T> { value: T }
    let c = Container { value: Some(42) };
    c.value
    }"#,
    )
    .assert_type_struct_generic("Option", vec![int_type()]);
}

#[test]
fn function_with_generic_param() {
    infer(
        r#"{
    let unwrap = |opt: Option<int>| -> int {
      match opt {
        Some(x) => x,
        None => 0,
      }
    };
    unwrap
    }"#,
    )
    .assert_function_type(vec![con_type("Option", vec![int_type()])], int_type());
}

#[test]
fn function_with_nested_generic_param() {
    infer(
        r#"{
    let process = |opt: Option<Option<int>>| -> int {
      match opt {
        Some(Some(x)) => x,
        Some(None) => 0,
        None => 0,
      }
    };
    process
    }"#,
    )
    .assert_function_type(
        vec![con_type(
            "Option",
            vec![con_type("Option", vec![int_type()])],
        )],
        int_type(),
    );
}

#[test]
fn function_returning_nested_generic() {
    infer(
        r#"{
    let wrap_twice = |x: int| -> Option<Option<int>> { Some(Some(x)) };
    wrap_twice
    }"#,
    )
    .assert_function_type(
        vec![int_type()],
        con_type("Option", vec![con_type("Option", vec![int_type()])]),
    );
}

#[test]
fn generic_inference_through_variable() {
    infer(
        r#"{
    let x = 42;
    Some(x)
    }"#,
    )
    .assert_type_struct_generic("Option", vec![int_type()]);
}

#[test]
fn nested_generic_inference_through_variable() {
    infer(
        r#"{
    let inner = Some(42);
    Some(inner)
    }"#,
    )
    .assert_type_struct_generic("Option", vec![con_type("Option", vec![int_type()])]);
}

#[test]
fn two_generic_params_both_inferred() {
    infer(
        r#"{
    struct Pair<K, V> { key: K, value: V }
    Pair { key: "name", value: 42 }
    }"#,
    )
    .assert_type_struct_generic("Pair", vec![string_type(), int_type()]);
}

#[test]
fn two_generic_params_with_nesting() {
    infer(
        r#"{
    struct Pair<K, V> { key: K, value: V }
    Pair { key: Some("name"), value: Some(42) }
    }"#,
    )
    .assert_type_struct_generic(
        "Pair",
        vec![
            con_type("Option", vec![string_type()]),
            con_type("Option", vec![int_type()]),
        ],
    );
}

#[test]
fn generic_function_call() {
    infer(
        r#"{
    let get_some = |x: int| -> Option<int> { Some(x) };
    get_some(42)
    }"#,
    )
    .assert_type_struct_generic("Option", vec![int_type()]);
}

#[test]
fn nested_generic_function_call() {
    infer(
        r#"{
    let wrap = |x: int| -> Option<int> { Some(x) };
    let wrap_twice = |x: int| -> Option<Option<int>> { Some(wrap(x)) };
    wrap_twice(42)
    }"#,
    )
    .assert_type_struct_generic("Option", vec![con_type("Option", vec![int_type()])]);
}

#[test]
fn generic_function_with_single_bound_type_error() {
    infer(
        r#"
    interface Display {
      fn show() -> string;
    }

    fn print_value<T: Display>(value: T) -> int {
      return 42;
    }

    fn test() {
      let f: string = print_value;
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn generic_function_with_multiple_bounds_type_error() {
    infer(
        r#"
    interface Display {
      fn show() -> string;
    }

    interface Clone {
      fn clone() -> int;
    }

    fn process<T: Display + Clone>(value: T) -> T {
      return value;
    }

    fn test() {
      let f: int = process;
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn nested_generic_pattern_match() {
    infer(
        r#"{
    let nested = Some(Some(42));
    match nested {
      Some(Some(x)) => x,
      Some(None) => 0,
      None => 0,
    }
    }"#,
    )
    .assert_type_int();
}

#[test]
fn nested_generic_pattern_match_different_types() {
    infer(
        r#"{
    let nested = Some(Ok(42));
    match nested {
      Some(Ok(x)) => x,
      Some(Err(_)) => 0,
      None => 0,
    }
    }"#,
    )
    .assert_type_int();
}

#[test]
fn immutable_let_assignment_fails() {
    infer(
        r#"{
    let x = 42;
    x = 10;
  }"#,
    )
    .assert_infer_code("immutable");
}

#[test]
fn mutable_let_assignment_succeeds() {
    infer(
        r#"{
    let mut x = 42;
    x = 10;
  }"#,
    )
    .assert_no_errors();
}

#[test]
fn mutable_let_multiple_assignments() {
    infer(
        r#"{
    let mut x = 1;
    x = 2;
    x = 3;
    x = 4;
  }"#,
    )
    .assert_no_errors();
}

#[test]
fn match_pattern_binding_immutable() {
    infer(
        r#"{
    match Some(42) {
      Some(x) => { x = 10; }
    }
  }"#,
    )
    .assert_infer_code("immutable");
}

#[test]
fn match_multiple_patterns_all_immutable() {
    infer(
        r#"{
    match Some(42) {
      Some(x) => { x = 1; },
      None => {}
    }
  }"#,
    )
    .assert_infer_code("immutable");
}

#[test]
fn match_struct_pattern_immutable() {
    infer(
        r#"{
    struct Point { x: int, y: int }
    let p = Point { x: 10, y: 20 };
    match p {
      Point { x, y } => { x = 5; }
    }
  }"#,
    )
    .assert_infer_code("immutable");
}

#[test]
fn mutable_in_nested_block() {
    infer(
        r#"{
    let mut x = 42;
    {
      x = 10;
    }
  }"#,
    )
    .assert_no_errors();
}

#[test]
fn immutable_in_nested_block_fails() {
    infer(
        r#"{
    let x = 42;
    {
      x = 10;
    }
  }"#,
    )
    .assert_infer_code("immutable");
}

#[test]
fn shadowing_changes_mutability() {
    infer(
        r#"{
    let x = 42;
    let mut x = x + 1;
    x = 10;
  }"#,
    )
    .assert_no_errors();
}

#[test]
fn function_parameter_immutable() {
    infer(
        r#"{
    fn foo(x: int) {
      x = 10;
    }
  }"#,
    )
    .assert_infer_code("immutable");
}

#[test]
fn struct_field_update_on_immutable_fails() {
    infer(
        r#"{
    struct Point { x: int, y: int }
    let p = Point { x: 10, y: 20 };
    p.x = 5;
  }"#,
    )
    .assert_infer_code("immutable");
}

#[test]
fn struct_field_update_on_mutable_succeeds() {
    infer(
        r#"{
    struct Point { x: int, y: int }
    let mut p = Point { x: 10, y: 20 };
    p.x = 5;
  }"#,
    )
    .assert_no_errors();
}

#[test]
fn slice_index_update_on_immutable_fails() {
    infer(
        r#"{
    let xs = [1, 2, 3];
    xs[0] = 10;
  }"#,
    )
    .assert_infer_code("immutable");
}

#[test]
fn slice_index_update_on_mutable_succeeds() {
    infer(
        r#"{
    let mut xs = [1, 2, 3];
    xs[0] = 10;
  }"#,
    )
    .assert_no_errors();
}

#[test]
fn slice_range_exclusive_returns_slice() {
    infer(
        r#"{
    let xs = [1, 2, 3, 4, 5];
    let sub: Slice<int> = xs[1..4];
  }"#,
    )
    .assert_no_errors();
}

#[test]
fn slice_range_inclusive_returns_slice() {
    infer(
        r#"{
    let xs = [1, 2, 3, 4, 5];
    let sub: Slice<int> = xs[1..=4];
  }"#,
    )
    .assert_no_errors();
}

#[test]
fn slice_range_from_returns_slice() {
    infer(
        r#"{
    let xs = [1, 2, 3, 4, 5];
    let tail: Slice<int> = xs[2..];
  }"#,
    )
    .assert_no_errors();
}

#[test]
fn slice_range_to_returns_slice() {
    infer(
        r#"{
    let xs = [1, 2, 3, 4, 5];
    let head: Slice<int> = xs[..3];
  }"#,
    )
    .assert_no_errors();
}

#[test]
fn slice_range_full_returns_slice() {
    infer(
        r#"{
    let xs = [1, 2, 3, 4, 5];
    let copy: Slice<int> = xs[..];
  }"#,
    )
    .assert_no_errors();
}

#[test]
fn slice_range_preserves_element_type() {
    infer(
        r#"{
    let xs = ["a", "b", "c"];
    let sub: Slice<string> = xs[0..2];
  }"#,
    )
    .assert_no_errors();
}

#[test]
fn string_range_returns_string() {
    infer(
        r#"{
    let s = "hello world";
    let sub: string = s[0..5];
  }"#,
    )
    .assert_no_errors();
}

#[test]
fn const_is_immutable() {
    infer(
        r#"
    const X: int = 42

    fn test() {
      X = 10;
    }
    "#,
    )
    .assert_infer_code("immutable");
}

#[test]
fn for_loop_binding_immutable() {
    infer(
        r#"{
    let xs = [1, 2, 3];
    for x in xs {
      x = 10;
    }
  }"#,
    )
    .assert_infer_code("immutable");
}

#[test]
fn field_assignment_checks_mutability() {
    infer(
        r#"
    struct Point {
      x: int,
      y: int,
    }

    fn test() {
      let p = Point { x: 1, y: 2 };
      p.x = 5;
    }
        "#,
    )
    .assert_infer_code("immutable");
}

#[test]
fn field_assignment_through_deref_allowed() {
    infer(
        r#"
    struct Point { x: int, y: int }

    fn set_field(p: Ref<Point>) {
      p.*.x = p.*.x + 1
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn option_qualified_some() {
    infer("{ Option.Some(42) }").assert_type_struct_generic("Option", vec![int_type()]);
}

#[test]
fn option_qualified_none() {
    infer("{ let x: Option<int> = Option.None; x }")
        .assert_type_struct_generic("Option", vec![int_type()]);
}

#[test]
fn result_qualified_ok() {
    infer("{ let x: Result<int, string> = Result.Ok(42); x }")
        .assert_type_struct_generic("Result", vec![int_type(), string_type()]);
}

#[test]
fn result_qualified_err() {
    infer(r#"{ let x: Result<int, string> = Result.Err("oops"); x }"#)
        .assert_type_struct_generic("Result", vec![int_type(), string_type()]);
}

#[test]
fn prelude_prefix_option_annotation() {
    infer("{ let x: prelude.Option<int> = prelude.Some(42); x }")
        .assert_type_struct_generic("Option", vec![int_type()]);
}

#[test]
fn prelude_prefix_option_none() {
    infer("{ let x: prelude.Option<int> = prelude.None; x }")
        .assert_type_struct_generic("Option", vec![int_type()]);
}

#[test]
fn generic_bounds_unsatisfied_produces_error() {
    infer(
        r#"
    interface Display {
      fn show() -> string;
    }

    fn print_display<T: Display>(value: T) -> string {
      return value.show();
    }

    fn main() {
      print_display(42);
    }
        "#,
    )
    .assert_infer_code("interface_not_implemented");
}

#[test]
fn generic_bounds_method_call_on_bounded_generic() {
    infer(
        r#"
    interface Display {
      fn show() -> string;
    }

    struct Person {
      name: string,
    }

    impl Person {
      fn show(self: Person) -> string {
        return self.name;
      }
    }

    fn print_value<T: Display>(value: T) -> string {
      return value.show();
    }

    fn main() {
      print_value(Person { name: "Ada" });
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn generic_bounds_satisfied_passes() {
    infer(
        r#"
    interface Display {
      fn show() -> string;
    }

    struct Person {
      name: string,
    }

    impl Person {
      fn show(self: Person) -> string {
        return self.name;
      }
    }

    fn print_value<T: Display>(value: T) {}

    fn main() {
      print_value(Person { name: "Ada" });
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn generic_interface_with_type_parameter_satisfied() {
    infer(
        r#"
    interface Iterable<T> {
      fn next() -> T;
    }

    struct Counter { value: int }

    impl Counter {
      fn next(self: Counter) -> int {
        return self.value + 1;
      }
    }

    fn use_iter<T: Iterable<int>>(v: T) -> int {
      v.next()
    }

    fn main() {
      use_iter(Counter { value: 0 });
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn unconstrained_bounded_type_param_produces_error() {
    infer(
        r#"
    interface Display {
      fn show() -> string;
    }

    fn require_display<T: Display>() {}

    fn main() {
      require_display();
    }
        "#,
    )
    .assert_infer_code("unconstrained_type_param");
}

#[test]
fn constrained_bounded_type_param_is_valid() {
    infer(
        r#"
    interface Display {
      fn show() -> string;
    }

    struct Person { name: string }

    impl Person {
      fn show(self: Person) -> string {
        return self.name;
      }
    }

    fn require_display<T: Display>(value: T) {}

    fn main() {
      require_display(Person { name: "Ada" });
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn impl_bound_is_enforced_on_method_call() {
    infer(
        r#"
    interface Display {
      fn show() -> string;
    }

    struct Box<T> { value: T }

    impl<T: Display> Box<T> {
      fn describe(self: Box<T>) -> string {
        return "desc";
      }
    }

    fn main() {
      let b: Box<int> = Box { value: 1 };
      let s = b.describe();
    }
        "#,
    )
    .assert_infer_code("interface_not_implemented");
}

#[test]
fn impl_bound_is_satisfied_when_type_implements_interface() {
    infer(
        r#"
    interface Display {
      fn show() -> string;
    }

    struct Box<T> { value: T }

    impl<T: Display> Box<T> {
      fn describe(self: Box<T>) -> string {
        return "desc";
      }
    }

    struct Person { name: string }

    impl Person {
      fn show(self: Person) -> string {
        return self.name;
      }
    }

    fn main() {
      let b: Box<Person> = Box { value: Person { name: "Ada" } };
      let s = b.describe();
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn impl_bound_propagated_to_method_return_type() {
    infer(
        r#"
    interface Printable {
      fn to_str(self) -> string
    }

    struct Box<T: Printable> { value: T }

    impl<T: Printable> Box<T> {
      fn clone_box(self) -> Box<T> {
        Box { value: self.value }
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn generic_bounds_tuple_cannot_satisfy_interface() {
    infer(
        r#"
    interface Display {
      fn show() -> string;
    }

    fn print_value<T: Display>(value: T) -> string {
      return value.show();
    }

    fn main() {
      let tuple = (1, 2);
      print_value(tuple);
    }
        "#,
    )
    .assert_infer_code("interface_not_implemented");
}

#[test]
fn generic_bounds_function_type_cannot_satisfy_interface() {
    infer(
        r#"
    interface Display {
      fn show() -> string;
    }

    fn print_value<T: Display>(value: T) -> string {
      return value.show();
    }

    fn some_func(x: int) -> int {
      return x;
    }

    fn main() {
      print_value(some_func);
    }
        "#,
    )
    .assert_infer_code("interface_not_implemented");
}

#[test]
fn type_parameter_does_not_get_methods_from_same_name_interface() {
    infer(
        r#"
    interface T {
      fn foo() -> int;
    }

    fn call_foo<T>(x: T) -> int {
      x.foo()
    }

    fn main() {}
        "#,
    )
    .assert_infer_code("member_not_found");
}

#[test]
fn never_as_bottom_type_coerces_to_any() {
    infer(
        r#"
    fn diverges() -> Never {
      return diverges();
    }

    fn test() -> int {
      diverges()
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn never_as_expected_rejects_inhabited_type() {
    infer(
        r#"
    fn returns_int_as_never() -> Never {
      1
    }
        "#,
    )
    .assert_infer_code("type_mismatch");
}

#[test]
fn never_in_let_rejects_inhabited_type() {
    infer(
        r#"
    fn main() {
      let x: Never = 1;
    }
        "#,
    )
    .assert_infer_code("type_mismatch");
}

#[test]
fn never_in_generic_expected_position_rejects_inhabited() {
    infer(
        r#"
    enum MyResult<T, E> {
      MyOk(T),
      MyErr(E),
    }

    fn main() {
      let x: MyResult<Never, int> = MyOk(1);
    }
        "#,
    )
    .assert_infer_code("type_mismatch");
}

#[test]
fn never_in_generic_actual_position_coerces() {
    infer(
        r#"
    enum MyResult<T, E> {
      MyOk(T),
      MyErr(E),
    }

    fn diverges() -> Never {
      return diverges();
    }

    fn test() -> MyResult<int, int> {
      MyOk(diverges())
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn never_first_in_match_does_not_poison_result_type() {
    infer(
        r#"
    fn diverges() -> Never {
      return diverges();
    }

    fn test(x: bool) -> int {
      match x {
        true => diverges(),
        false => 42,
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn never_in_if_then_branch_does_not_poison_result_type() {
    infer(
        r#"
    fn diverges() -> Never {
      return diverges();
    }

    fn test(x: bool) -> int {
      if x {
        diverges()
      } else {
        42
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn never_in_if_let_success_branch_does_not_poison_result_type() {
    infer(
        r#"
    fn diverges() -> Never {
      return diverges();
    }

    fn test(opt: Option<int>) -> int {
      if let Some(_) = opt {
        diverges()
      } else {
        42
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn multiple_never_arms_before_concrete_does_not_poison_result_type() {
    infer(
        r#"
    fn diverges() -> Never {
      return diverges();
    }

    fn test(x: int) -> int {
      match x {
        1 => diverges(),
        2 => diverges(),
        _ => 42,
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn never_last_in_match_still_works() {
    infer(
        r#"
    fn diverges() -> Never {
      return diverges();
    }

    fn test(x: bool) -> int {
      match x {
        true => 42,
        false => diverges(),
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn tuple_struct_zero_field() {
    infer(
        r#"{
    struct Marker()
    Marker()
    }"#,
    )
    .assert_type_struct("Marker");
}

#[test]
fn tuple_struct_single_field() {
    infer(
        r#"{
    struct UserId(int)
    UserId(42)
    }"#,
    )
    .assert_type_struct("UserId");
}

#[test]
fn tuple_struct_multi_field() {
    infer(
        r#"{
    struct Point(int, int)
    Point(10, 20)
    }"#,
    )
    .assert_type_struct("Point");
}

#[test]
fn tuple_struct_field_access_single() {
    infer(
        r#"{
    struct UserId(int)
    let id = UserId(42);
    id.0
    }"#,
    )
    .assert_type_int();
}

#[test]
fn tuple_struct_field_access_multi() {
    infer(
        r#"{
    struct Point(int, int)
    let p = Point(10, 20);
    p.0 + p.1
    }"#,
    )
    .assert_type_int();
}

#[test]
fn tuple_struct_generic() {
    infer(
        r#"{
    struct Wrapper<T>(T)
    Wrapper(42)
    }"#,
    )
    .assert_type_struct_generic("Wrapper", vec![int_type()]);
}

#[test]
fn tuple_struct_generic_field_access() {
    infer(
        r#"{
    struct Wrapper<T>(T)
    let w: Wrapper<string> = Wrapper("hello");
    w.0
    }"#,
    )
    .assert_type_string();
}

#[test]
fn tuple_struct_pattern_match() {
    infer(
        r#"{
    struct Point(int, int)
    let p = Point(10, 20);
    match p {
      Point(x, y) => x + y,
    }
    }"#,
    )
    .assert_type_int();
}

#[test]
fn tuple_struct_in_function_param() {
    infer(
        r#"
    struct UserId(int)

    fn get_raw(id: UserId) -> int {
      id.0
    }

    fn test() -> int {
      get_raw(UserId(42))
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn tuple_struct_in_function_return() {
    infer(
        r#"
    struct Point(int, int)

    fn make_point(x: int, y: int) -> Point {
      Point(x, y)
    }

    fn test() -> int {
      let p = make_point(10, 20);
      p.0 + p.1
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn value_enum_in_typedef_file_succeeds() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "weekday",
        "lib.d.lis",
        r#"
pub enum Weekday {
  Sunday = 0,
  Monday = 1,
  Tuesday = 2,
  Wednesday = 3,
  Thursday = 4,
  Friday = 5,
  Saturday = 6,
}
"#,
    );

    let source = r#"
import "weekday"

fn is_weekend(day: weekday.Weekday) -> bool {
  match day {
    weekday.Sunday | weekday.Saturday => true,
    _ => false,
  }
}
"#;
    fs.add_file("main", "main.lis", source);

    infer_module("main", fs).assert_no_errors();
}

#[test]
fn value_enum_pattern_requires_catch_all() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "weekday",
        "lib.d.lis",
        r#"
pub enum Weekday {
  Sunday = 0,
  Monday = 1,
}
"#,
    );

    let source = r#"
import "weekday"

fn get_name(day: weekday.Weekday) -> string {
  match day {
    weekday.Sunday => "Sunday",
    weekday.Monday => "Monday",
  }
}
"#;
    fs.add_file("main", "main.lis", source);

    infer_module("main", fs).assert_exhaustiveness_error();
}

#[test]
fn value_enum_pattern_with_catch_all_succeeds() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "weekday",
        "lib.d.lis",
        r#"
pub enum Weekday {
  Sunday = 0,
  Monday = 1,
}
"#,
    );

    let source = r#"
import "weekday"

fn get_name(day: weekday.Weekday) -> string {
  match day {
    weekday.Sunday => "Sunday",
    weekday.Monday => "Monday",
    _ => "Unknown",
  }
}
"#;
    fs.add_file("main", "main.lis", source);

    infer_module("main", fs).assert_no_errors();
}

#[test]
fn pointer_to_struct_satisfies_interface() {
    infer(
        r#"
    import "go:fmt"

    interface Describer {
      fn describe() -> string
    }

    struct Cat {
      name: string,
    }

    impl Cat {
      fn describe(self: Cat) -> string {
        self.name
      }
    }

    fn show(d: Describer) {
      let desc = d.describe();
      fmt.Print(f"{desc}\n");
    }

    fn main() {
      let cat = Cat { name: "Whiskers" };
      show(&cat);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn interface_method_with_bare_self_parameter() {
    infer(
        r#"
    interface Greetable {
      fn greet(self) -> string
    }

    struct Person { name: string }

    impl Person {
      fn greet(self) -> string { self.name }
    }

    fn print_greeting(g: Greetable) -> string {
      g.greet()
    }

    fn main() {
      let p = Person { name: "Alice" };
      print_greeting(p);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn generic_static_method_infers_type_param_from_int() {
    infer(
        r#"
    struct Box<T> { value: T }

    impl<T> Box<T> {
      fn new(value: T) -> Box<T> {
        Box { value: value }
      }

      fn get(self: Box<T>) -> T {
        self.value
      }
    }

    fn main() -> int {
      let b = Box.new(42);
      b.get()
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn generic_static_method_infers_type_param_from_string() {
    infer(
        r#"
    struct Box<T> { value: T }

    impl<T> Box<T> {
      fn new(value: T) -> Box<T> {
        Box { value: value }
      }

      fn get(self: Box<T>) -> T {
        self.value
      }
    }

    fn main() -> string {
      let s = Box.new("hello");
      s.get()
    }
        "#,
    )
    .assert_no_errors();
}

fn duration_typedef() -> &'static str {
    r#"
pub enum Duration: int64 {
  Nanosecond = 1,
  Microsecond = 1000,
  Millisecond = 1000000,
  Second = 1000000000,
}
"#
}

#[test]
fn numeric_alias_t_plus_t() {
    let mut fs = MockFileSystem::new();
    fs.add_file("time", "time.d.lis", duration_typedef());
    fs.add_file(
        "main",
        "main.lis",
        r#"
import "time"

fn test() -> time.Duration {
  time.Duration.Second + time.Duration.Millisecond
}
"#,
    );
    infer_module("main", fs).assert_no_errors();
}

#[test]
fn numeric_alias_t_minus_t() {
    let mut fs = MockFileSystem::new();
    fs.add_file("time", "time.d.lis", duration_typedef());
    fs.add_file(
        "main",
        "main.lis",
        r#"
import "time"

fn test() -> time.Duration {
  time.Duration.Second - time.Duration.Millisecond
}
"#,
    );
    infer_module("main", fs).assert_no_errors();
}

#[test]
fn numeric_alias_t_times_t() {
    let mut fs = MockFileSystem::new();
    fs.add_file("time", "time.d.lis", duration_typedef());
    fs.add_file(
        "main",
        "main.lis",
        r#"
import "time"

fn test() -> time.Duration {
  time.Duration.Second * time.Duration.Millisecond
}
"#,
    );
    infer_module("main", fs).assert_no_errors();
}

#[test]
fn numeric_alias_t_times_u() {
    let mut fs = MockFileSystem::new();
    fs.add_file("time", "time.d.lis", duration_typedef());
    fs.add_file(
        "main",
        "main.lis",
        r#"
import "time"

fn test() -> time.Duration {
  time.Duration.Second * 5
}
"#,
    );
    infer_module("main", fs).assert_no_errors();
}

#[test]
fn numeric_alias_u_times_t() {
    let mut fs = MockFileSystem::new();
    fs.add_file("time", "time.d.lis", duration_typedef());
    fs.add_file(
        "main",
        "main.lis",
        r#"
import "time"

fn test() -> time.Duration {
  5 * time.Duration.Second
}
"#,
    );
    infer_module("main", fs).assert_no_errors();
}

#[test]
fn numeric_alias_t_div_t_yields_underlying() {
    let mut fs = MockFileSystem::new();
    fs.add_file("time", "time.d.lis", duration_typedef());
    fs.add_file(
        "main",
        "main.lis",
        r#"
import "time"

fn test() -> int64 {
  time.Duration.Second / time.Duration.Millisecond
}
"#,
    );
    infer_module("main", fs).assert_no_errors();
}

#[test]
fn numeric_alias_t_div_u() {
    let mut fs = MockFileSystem::new();
    fs.add_file("time", "time.d.lis", duration_typedef());
    fs.add_file(
        "main",
        "main.lis",
        r#"
import "time"

fn test() -> time.Duration {
  time.Duration.Second / 2
}
"#,
    );
    infer_module("main", fs).assert_no_errors();
}

#[test]
fn numeric_alias_t_rem_u() {
    let mut fs = MockFileSystem::new();
    fs.add_file("time", "time.d.lis", duration_typedef());
    fs.add_file(
        "main",
        "main.lis",
        r#"
import "time"

fn test() -> time.Duration {
  time.Duration.Second % 3
}
"#,
    );
    infer_module("main", fs).assert_no_errors();
}

#[test]
fn numeric_alias_unary_neg() {
    let mut fs = MockFileSystem::new();
    fs.add_file("time", "time.d.lis", duration_typedef());
    fs.add_file(
        "main",
        "main.lis",
        r#"
import "time"

fn test() -> time.Duration {
  -time.Duration.Second
}
"#,
    );
    infer_module("main", fs).assert_no_errors();
}

#[test]
fn numeric_alias_t_lt_t() {
    let mut fs = MockFileSystem::new();
    fs.add_file("time", "time.d.lis", duration_typedef());
    fs.add_file(
        "main",
        "main.lis",
        r#"
import "time"

fn test() -> bool {
  time.Duration.Millisecond < time.Duration.Second
}
"#,
    );
    infer_module("main", fs).assert_no_errors();
}

#[test]
fn numeric_alias_t_gt_u() {
    let mut fs = MockFileSystem::new();
    fs.add_file("time", "time.d.lis", duration_typedef());
    fs.add_file(
        "main",
        "main.lis",
        r#"
import "time"

fn test() -> bool {
  time.Duration.Second > 1000
}
"#,
    );
    infer_module("main", fs).assert_no_errors();
}

#[test]
fn numeric_alias_u_lt_t() {
    let mut fs = MockFileSystem::new();
    fs.add_file("time", "time.d.lis", duration_typedef());
    fs.add_file(
        "main",
        "main.lis",
        r#"
import "time"

fn test() -> bool {
  1000 < time.Duration.Second
}
"#,
    );
    infer_module("main", fs).assert_no_errors();
}

#[test]
fn numeric_alias_t_eq_t() {
    let mut fs = MockFileSystem::new();
    fs.add_file("time", "time.d.lis", duration_typedef());
    fs.add_file(
        "main",
        "main.lis",
        r#"
import "time"

fn test() -> bool {
  time.Duration.Second == time.Duration.Millisecond
}
"#,
    );
    infer_module("main", fs).assert_no_errors();
}

#[test]
fn numeric_alias_t_eq_u() {
    let mut fs = MockFileSystem::new();
    fs.add_file("time", "time.d.lis", duration_typedef());
    fs.add_file(
        "main",
        "main.lis",
        r#"
import "time"

fn test() -> bool {
  time.Duration.Second == 1000000000
}
"#,
    );
    infer_module("main", fs).assert_no_errors();
}

#[test]
fn numeric_alias_t_neq_u() {
    let mut fs = MockFileSystem::new();
    fs.add_file("time", "time.d.lis", duration_typedef());
    fs.add_file(
        "main",
        "main.lis",
        r#"
import "time"

fn test() -> bool {
  time.Duration.Second != 0
}
"#,
    );
    infer_module("main", fs).assert_no_errors();
}

#[test]
fn numeric_alias_u_div_t_error() {
    let mut fs = MockFileSystem::new();
    fs.add_file("time", "time.d.lis", duration_typedef());
    fs.add_file(
        "main",
        "main.lis",
        r#"
import "time"

fn test() {
  let x = 100 / time.Duration.Second;
}
"#,
    );
    infer_module("main", fs).assert_infer_code("invalid_division_order");
}

#[test]
fn numeric_alias_u_rem_t_error() {
    let mut fs = MockFileSystem::new();
    fs.add_file("time", "time.d.lis", duration_typedef());
    fs.add_file(
        "main",
        "main.lis",
        r#"
import "time"

fn test() {
  let x = 100 % time.Duration.Second;
}
"#,
    );
    infer_module("main", fs).assert_infer_code("invalid_division_order");
}

#[test]
fn numeric_alias_with_variable() {
    let mut fs = MockFileSystem::new();
    fs.add_file("time", "time.d.lis", duration_typedef());
    fs.add_file(
        "main",
        "main.lis",
        r#"
import "time"

fn test() -> time.Duration {
  let n: int = 5;
  time.Duration.Second * n
}
"#,
    );
    infer_module("main", fs).assert_no_errors();
}

#[test]
fn numeric_alias_chained_ops() {
    let mut fs = MockFileSystem::new();
    fs.add_file("time", "time.d.lis", duration_typedef());
    fs.add_file(
        "main",
        "main.lis",
        r#"
import "time"

fn test() -> time.Duration {
  time.Duration.Second * 2 + time.Duration.Millisecond * 500
}
"#,
    );
    infer_module("main", fs).assert_no_errors();
}

#[test]
fn numeric_alias_in_function_param() {
    let mut fs = MockFileSystem::new();
    fs.add_file("time", "time.d.lis", duration_typedef());
    fs.add_file(
        "main",
        "main.lis",
        r#"
import "time"

fn sleep(d: time.Duration) {}

fn test() {
  sleep(time.Duration.Second * 2);
}
"#,
    );
    infer_module("main", fs).assert_no_errors();
}

#[test]
fn numeric_alias_in_return() {
    let mut fs = MockFileSystem::new();
    fs.add_file("time", "time.d.lis", duration_typedef());
    fs.add_file(
        "main",
        "main.lis",
        r#"
import "time"

fn get_delay(multiplier: int) -> time.Duration {
  time.Duration.Millisecond * multiplier
}
"#,
    );
    infer_module("main", fs).assert_no_errors();
}

#[test]
fn numeric_alias_t_rem_t() {
    let mut fs = MockFileSystem::new();
    fs.add_file("time", "time.d.lis", duration_typedef());
    fs.add_file(
        "main",
        "main.lis",
        r#"
import "time"

fn test() -> time.Duration {
  time.Duration.Second % time.Duration.Millisecond
}
"#,
    );
    infer_module("main", fs).assert_no_errors();
}

#[test]
fn numeric_alias_t_lte_u() {
    let mut fs = MockFileSystem::new();
    fs.add_file("time", "time.d.lis", duration_typedef());
    fs.add_file(
        "main",
        "main.lis",
        r#"
import "time"

fn test() -> bool {
  time.Duration.Second <= 2000000000
}
"#,
    );
    infer_module("main", fs).assert_no_errors();
}

#[test]
fn numeric_alias_t_gte_u() {
    let mut fs = MockFileSystem::new();
    fs.add_file("time", "time.d.lis", duration_typedef());
    fs.add_file(
        "main",
        "main.lis",
        r#"
import "time"

fn test() -> bool {
  time.Duration.Second >= 500000000
}
"#,
    );
    infer_module("main", fs).assert_no_errors();
}

#[test]
fn numeric_alias_u_eq_t() {
    let mut fs = MockFileSystem::new();
    fs.add_file("time", "time.d.lis", duration_typedef());
    fs.add_file(
        "main",
        "main.lis",
        r#"
import "time"

fn test() -> bool {
  1000000000 == time.Duration.Second
}
"#,
    );
    infer_module("main", fs).assert_no_errors();
}

#[test]
fn numeric_alias_u_neq_t() {
    let mut fs = MockFileSystem::new();
    fs.add_file("time", "time.d.lis", duration_typedef());
    fs.add_file(
        "main",
        "main.lis",
        r#"
import "time"

fn test() -> bool {
  0 != time.Duration.Second
}
"#,
    );
    infer_module("main", fs).assert_no_errors();
}

#[test]
fn numeric_alias_cross_family_error() {
    let mut fs = MockFileSystem::new();
    fs.add_file("time", "time.d.lis", duration_typedef());
    fs.add_file(
        "main",
        "main.lis",
        r#"
import "time"

fn test() {
  let x = time.Duration.Second * 1.5;
}
"#,
    );
    infer_module("main", fs).assert_infer_code("type_mismatch");
}

#[test]
fn numeric_alias_different_named_types_error() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "time",
        "time.d.lis",
        r#"
pub enum DurationA: int64 { Second = 1000000000 }
pub enum DurationB: int64 { Second = 1000000000 }
"#,
    );
    fs.add_file(
        "main",
        "main.lis",
        r#"
import "time"

fn test() {
  let x = time.DurationA.Second + time.DurationB.Second;
}
"#,
    );
    infer_module("main", fs).assert_infer_code("incompatible_named_numeric_types");
}

#[test]
fn numeric_alias_complex_chained() {
    let mut fs = MockFileSystem::new();
    fs.add_file("time", "time.d.lis", duration_typedef());
    fs.add_file(
        "main",
        "main.lis",
        r#"
import "time"

fn test() -> time.Duration {
  let base = time.Duration.Second * 2;
  let extra = time.Duration.Millisecond * 500;
  base + extra - time.Duration.Millisecond
}
"#,
    );
    infer_module("main", fs).assert_no_errors();
}

#[test]
fn numeric_alias_parenthesized_ratio() {
    let mut fs = MockFileSystem::new();
    fs.add_file("time", "time.d.lis", duration_typedef());
    fs.add_file(
        "main",
        "main.lis",
        r#"
import "time"

fn test() -> int64 {
  // T / T yields int64, which is the underlying type
  let ratio: int64 = time.Duration.Second / time.Duration.Millisecond;
  ratio
}
"#,
    );
    infer_module("main", fs).assert_no_errors();
}

#[test]
fn numeric_alias_assignment_from_expression() {
    let mut fs = MockFileSystem::new();
    fs.add_file("time", "time.d.lis", duration_typedef());
    fs.add_file(
        "main",
        "main.lis",
        r#"
import "time"

fn test() {
  let d: time.Duration = time.Duration.Second * 2;
  let ratio: int64 = time.Duration.Second / time.Duration.Millisecond;
}
"#,
    );
    infer_module("main", fs).assert_no_errors();
}

#[test]
fn ufcs_method_infers_closure_param_type() {
    infer(
        r#"
    struct Box<T> { value: T }

    impl<T> Box<T> {
      fn map<U>(self, f: fn(T) -> U) -> Box<U> {
        Box { value: f(self.value) }
      }
    }

    fn main() {
      let b: Box<int> = Box { value: 42 };
      let _mapped = b.map(|x| x * 2);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn ufcs_method_infers_closure_param_type_chained() {
    infer(
        r#"
    struct Box<T> { value: T }

    impl<T> Box<T> {
      fn map<U>(self, f: fn(T) -> U) -> Box<U> {
        Box { value: f(self.value) }
      }
    }

    fn main() {
      let b: Box<int> = Box { value: 42 };
      let _mapped = b.map(|x| x * 2).map(|y| y + 1);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn ufcs_method_infers_tuple_pattern_types() {
    infer(
        r#"
    struct Pair<A, B> { first: A, second: B }

    impl<A> Pair<A, A> {
      fn zip<B>(self, other: Pair<B, B>) -> Pair<(A, B), (A, B)> {
        Pair {
          first: (self.first, other.first),
          second: (self.second, other.second),
        }
      }
    }

    fn main() -> int {
      let a: Pair<int, int> = Pair { first: 1, second: 2 };
      let b: Pair<string, string> = Pair { first: "a", second: "b" };
      let zipped = a.zip(b);
      let (x, _y) = zipped.first;
      x
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn option_map_infers_closure_param() {
    infer(
        r#"
    fn main() {
      let opt: Option<int> = Some(42);
      let _mapped = opt.map(|x| x * 2);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn option_and_then_infers_closure_param() {
    infer(
        r#"
    fn main() {
      let opt: Option<int> = Some(10);
      let _result = opt.and_then(|x| if x > 5 { Some(x * 2) } else { None });
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn result_map_infers_closure_param() {
    infer(
        r#"
    fn main() {
      let res: Result<int, string> = Ok(50);
      let _mapped = res.map(|x| x * 2);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn result_and_then_infers_closure_param() {
    infer(
        r#"
    fn main() {
      let res: Result<int, string> = Ok(10);
      let _result = res.and_then(|x| if x > 5 { Ok(x * 10) } else { Err("too small") });
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn option_zip_infers_tuple_pattern() {
    infer(
        r#"
    fn main() -> int {
      let a: Option<int> = Some(40);
      let b: Option<int> = Some(60);
      let zipped = a.zip(b);
      match zipped {
        Some((x, y)) => x + y,
        None => 0,
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn ufcs_method_with_explicit_type_args() {
    infer(
        r#"
    struct Box<T> { value: T }

    impl<T> Box<T> {
      fn map<U>(self, f: fn(T) -> U) -> Box<U> {
        Box { value: f(self.value) }
      }
    }

    fn main() {
      let b: Box<int> = Box { value: 42 };
      // Explicit type args: T is int (from receiver), U is string
      let _mapped = b.map<int, string>(|x| "hello");
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn ufcs_method_with_method_only_type_args() {
    infer(
        r#"
    struct Box<T> { value: T }

    impl<T> Box<T> {
      fn map<U>(self, f: fn(T) -> U) -> Box<U> {
        Box { value: f(self.value) }
      }
    }

    fn main() {
      let b: Box<int> = Box { value: 42 };
      // Only provide U (method-own generic); T is inferred from receiver
      let _mapped = b.map<string>(|x| "hello");
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn enum_instance_method_basic() {
    infer(
        r#"
    enum Color {
      Red,
      Green,
      Blue,
    }

    impl Color {
      fn to_string(self) -> string {
        match self {
          Color.Red => "red",
          Color.Green => "green",
          Color.Blue => "blue",
        }
      }
    }

    fn main() -> string {
      let c = Color.Red;
      c.to_string()
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn enum_instance_method_with_explicit_self_type() {
    infer(
        r#"
    enum Status {
      Active,
      Inactive,
    }

    impl Status {
      fn is_active(self: Status) -> bool {
        match self {
          Status.Active => true,
          Status.Inactive => false,
        }
      }
    }

    fn main() -> bool {
      let s = Status.Active;
      s.is_active()
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn enum_instance_method_with_parameters() {
    infer(
        r#"
    enum Level {
      Low,
      High,
    }

    impl Level {
      fn add_offset(self, offset: int) -> int {
        match self {
          Level.Low => 0 + offset,
          Level.High => 100 + offset,
        }
      }
    }

    fn main() -> int {
      let l = Level.High;
      l.add_offset(5)
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn enum_static_method() {
    infer(
        r#"
    enum Direction {
      Up,
      Down,
    }

    impl Direction {
      fn default() -> Direction {
        Direction.Up
      }
    }

    fn main() {
      let _d = Direction.default();
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn enum_multiple_methods() {
    infer(
        r#"
    enum State {
      On,
      Off,
    }

    impl State {
      fn toggle(self) -> State {
        match self {
          State.On => State.Off,
          State.Off => State.On,
        }
      }

      fn is_on(self) -> bool {
        match self {
          State.On => true,
          State.Off => false,
        }
      }

      fn new() -> State {
        State.Off
      }
    }

    fn main() -> bool {
      let s = State.new();
      let toggled = s.toggle();
      toggled.is_on()
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn generic_enum_instance_method() {
    infer(
        r#"
    enum Maybe<T> {
      Just(T),
      Nothing,
    }

    impl<T> Maybe<T> {
      fn is_just(self) -> bool {
        match self {
          Maybe.Just(_) => true,
          Maybe.Nothing => false,
        }
      }
    }

    fn main() -> bool {
      let m = Maybe.Just(42);
      m.is_just()
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn generic_enum_method_using_type_param() {
    infer(
        r#"
    enum Maybe<T> {
      Just(T),
      Nothing,
    }

    impl<T> Maybe<T> {
      fn unwrap_or(self, fallback: T) -> T {
        match self {
          Maybe.Just(x) => x,
          Maybe.Nothing => fallback,
        }
      }
    }

    fn main() -> int {
      let m = Maybe.Just(42);
      m.unwrap_or(0)
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn enum_method_does_not_conflict_with_variant() {
    infer(
        r#"
    enum Color {
      Red,
      Green,
    }

    impl Color {
      fn red(self) -> bool {
        match self {
          Color.Red => true,
          Color.Green => false,
        }
      }
    }

    fn main() {
      let c = Color.Red;        // Variant access
      let is_red = c.red();     // Method call - should not conflict
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn static_method_on_value_should_not_resolve() {
    infer(
        r#"
    enum Color {
      Red,
      Green,
    }

    impl Color {
      fn new() -> Color {
        Color.Red
      }
    }

    fn main() {
      let c = Color.Green;
      let x = c.new();  // BUG: This should be an error, not valid
    }
        "#,
    )
    .assert_infer_code("member_not_found");
}

#[test]
fn tuple_struct_constructor_in_impl_block() {
    infer(
        r#"
    struct Wrapper(int)

    impl Wrapper {
      fn make(n: int) -> Wrapper {
        Wrapper(n)
      }

      fn doubled(self) -> Wrapper {
        let v = self.0 * 2;
        Wrapper(v)
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn map_with_slice_key_rejected() {
    infer(
        r#"
    fn main() {
      let mut m: Map<Slice<int>, string> = {};
    }
        "#,
    )
    .assert_infer_code("non_comparable_map_key");
}

#[test]
fn map_with_function_key_rejected() {
    infer(
        r#"
    fn main() {
      let mut m: Map<fn(int) -> int, string> = {};
    }
        "#,
    )
    .assert_infer_code("non_comparable_map_key");
}

#[test]
fn map_with_string_key_allowed() {
    infer(
        r#"
    fn main() {
      let mut m = Map.new<string, int>();
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn recursive_generic_instantiation_rejected() {
    infer(
        r#"
    struct Box<T> {
      value: T,
    }

    impl<T> Box<T> {
      fn wrap(self) -> Box<Box<T>> {
        Box { value: self }
      }
    }

    fn main() {}
        "#,
    )
    .assert_infer_code("recursive_instantiation");
}

#[test]
fn non_recursive_generic_method_allowed() {
    infer(
        r#"
    struct Box<T> {
      value: T,
    }

    impl<T> Box<T> {
      fn map<U>(self, f: fn(T) -> U) -> Box<U> {
        Box { value: f(self.value) }
      }
    }

    fn main() {}
        "#,
    )
    .assert_no_errors();
}

#[test]
fn constant_negative_to_unsigned_cast_rejected() {
    infer(
        r#"
    fn main() {
      let x = -1 as uint;
    }
        "#,
    )
    .assert_infer_code("integer_literal_overflow");
}

#[test]
fn runtime_negative_to_unsigned_cast_allowed() {
    infer(
        r#"
    fn main() {
      let neg: int = -1;
      let x = neg as uint;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn recursive_struct_without_ref_rejected() {
    infer(
        r#"
    struct BadRecursive {
      value: int,
      next: Option<BadRecursive>,
    }

    fn main() {
      let b = BadRecursive { value: 1, next: None };
    }
        "#,
    )
    .assert_infer_code("recursive_type");
}

#[test]
fn recursive_struct_through_option_self_rejected() {
    infer(
        r#"
    struct Node {
      pub value: int,
      pub next: Option<Node>,
    }

    fn main() {
      let n = Node { value: 1, next: Some(Node { value: 2, next: None }) };
    }
        "#,
    )
    .assert_infer_code("recursive_type");
}

#[test]
fn recursive_struct_with_ref_allowed() {
    infer(
        r#"
    struct Node {
      value: int,
      next: Option<Ref<Node>>,
    }

    fn main() {
      let n = Node { value: 1, next: None };
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn recursive_struct_indirect_rejected() {
    infer(
        r#"
    struct A {
      b: B,
    }

    struct B {
      a: Option<A>,
    }

    fn main() {}
        "#,
    )
    .assert_infer_code("recursive_type");
}

#[test]
fn recursive_struct_with_slice_allowed() {
    infer(
        r#"
    struct Node<T> {
      pub value: T,
      pub children: Slice<Node<T>>,
    }

    fn main() {
      let n = Node { value: 1, children: [] };
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn recursive_struct_with_map_allowed() {
    infer(
        r#"
    struct TreeNode {
      pub name: string,
      pub children: Map<string, TreeNode>,
    }

    fn main() {
      let n = TreeNode { name: "root", children: Map.new() };
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn recursive_enum_through_generic_struct_rejected() {
    infer(
        r#"
    struct Box<T> { value: T }

    enum Tree {
      Leaf(int),
      Node(Box<Tree>, Box<Tree>),
    }

    fn main() {}
        "#,
    )
    .assert_infer_code("recursive_type");
}

#[test]
fn recursive_enum_direct_self_reference_allowed() {
    infer(
        r#"
    enum Tree {
      Leaf(int),
      Node(Tree, Tree),
    }

    fn main() {
      let t = Tree.Node(Tree.Leaf(1), Tree.Leaf(2));
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn interface_self_embedding_rejected() {
    infer(
        r#"
    interface Z {
      impl Z
      fn z_method(self) -> string
    }

    fn main() {}
        "#,
    )
    .assert_infer_code("interface_cycle");
}

#[test]
fn interface_mutual_cycle_rejected() {
    infer(
        r#"
    interface P {
      impl Q
      fn p_method(self) -> string
    }

    interface Q {
      impl P
      fn q_method(self) -> string
    }

    fn main() {}
        "#,
    )
    .assert_infer_code("interface_cycle");
}

#[test]
fn interface_three_way_cycle_rejected() {
    infer(
        r#"
    interface R {
      impl S
      fn r_method(self) -> string
    }

    interface S {
      impl T
      fn s_method(self) -> string
    }

    interface T {
      impl R
      fn t_method(self) -> string
    }

    fn main() {}
        "#,
    )
    .assert_infer_code("interface_cycle");
}

#[test]
fn interface_method_conflict_rejected() {
    infer(
        r#"
    interface HasName {
      fn name(self) -> string
    }

    interface HasNameInt {
      fn name(self) -> int
    }

    interface Both {
      impl HasName
      impl HasNameInt
    }

    fn main() {}
        "#,
    )
    .assert_infer_code("interface_method_conflict");
}

#[test]
fn interface_embedding_no_conflict() {
    infer(
        r#"
    interface HasName {
      fn name(self) -> string
    }

    interface HasAge {
      fn age(self) -> int
    }

    interface Person {
      impl HasName
      impl HasAge
    }

    fn main() {}
        "#,
    )
    .assert_no_errors();
}

#[test]
fn byte_uint8_alias_direct_assignment() {
    infer(
        r#"
    fn main() {
      let b: byte = 66;
      let u: uint8 = b;
      let b2: byte = u;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn rune_int32_alias_direct_assignment() {
    infer(
        r#"
    fn main() {
      let r: rune = 'A';
      let i: int32 = r;
      let r2: rune = i;
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn byte_uint8_alias_in_generic_type() {
    infer(
        r#"
    fn takes_bytes(s: Slice<uint8>) {}

    fn main() {
      let data = "hello" as Slice<byte>;
      takes_bytes(data);
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn byte_uint8_alias_in_interface_method_signature() {
    infer(
        r#"
    import "go:encoding"

    struct Widget { id: int }

    impl Widget {
      fn MarshalText(self) -> Result<Slice<byte>, error> {
        Ok("custom-text" as Slice<byte>)
      }
    }

    fn main() {
      let w = Widget { id: 7 }
      let _: encoding.TextMarshaler = w
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn impl_block_generic_bound_struct_field_method() {
    infer(
        r#"
    interface Displayable {
      fn display(self) -> string
    }

    struct Wrapper<T: Displayable> {
      inner: T,
    }

    impl<T: Displayable> Wrapper<T> {
      fn show(self) -> string {
        self.inner.display()
      }
    }

    fn main() {}
        "#,
    )
    .assert_no_errors();
}

#[test]
fn impl_block_generic_bound_enum_pattern_match() {
    infer(
        r#"
    interface Displayable {
      fn display(self) -> string
    }

    enum Boxed<T: Displayable> {
      Val(T),
      Empty,
    }

    impl<T: Displayable> Boxed<T> {
      fn show(self) -> string {
        match self {
          Boxed.Val(inner) => inner.display(),
          Boxed.Empty => "empty",
        }
      }
    }

    fn main() {}
        "#,
    )
    .assert_no_errors();
}

#[test]
fn impl_block_generic_bound_with_ref() {
    infer(
        r#"
    interface Describable {
      fn describe(self) -> string
    }

    struct Container<T: Describable> {
      item: Ref<T>,
    }

    impl<T: Describable> Container<T> {
      fn describe_item(self) -> string {
        self.item.*.describe()
      }
    }

    fn main() {}
        "#,
    )
    .assert_no_errors();
}

#[test]
fn ref_method_on_immutable_binding_fails() {
    infer(
        r#"
    struct Counter { value: int }

    impl Counter {
      fn increment(self: Ref<Counter>) { self.value = self.value + 1 }
    }

    fn main() {
      let c = Counter { value: 0 };
      c.increment()
    }
        "#,
    )
    .assert_infer_code("immutable");
}

#[test]
fn ref_method_on_nested_field_through_ref_receiver() {
    infer(
        r#"
    struct Counter { count: int }
    impl Counter {
      fn increment(self: Ref<Counter>) { self.count = self.count + 1 }
    }

    struct Wrapper { inner: Counter }
    impl Wrapper {
      fn increment_inner(self: Ref<Wrapper>) {
        self.inner.increment()
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn specialized_impl_method_type_checks() {
    infer(
        r#"
    struct Wrapper<T> { value: T }
    impl Wrapper<string> {
      fn greet(self) -> string { "hello" }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn specialized_impl_method_rejected_on_wrong_type() {
    infer(
        r#"
    struct Wrapper<T> { value: T }
    impl Wrapper<string> {
      fn greet(self) -> string { "hello" }
    }
    fn test() -> string {
      let w = Wrapper { value: 42 };
      w.greet()
    }
        "#,
    )
    .assert_infer_code("type_mismatch");
}

#[test]
fn slice_string_join_type_checks() {
    infer(
        r#"{
    let items: Slice<string> = ["a", "b", "c"];
    items.join(", ")
    }"#,
    )
    .assert_type_string();
}

#[test]
fn slice_int_join_rejected() {
    let result = infer(
        r#"{
    let nums = [1, 2, 3];
    nums.join(", ")
    }"#,
    );
    assert!(
        !result.errors.is_empty(),
        "Expected type error for join on Slice<int>, but no errors were raised"
    );
}

#[test]
fn none_for_lisette_interface_still_rejected() {
    infer(
        r#"
    interface Display {
      fn show() -> string;
    }

    fn print_it(d: Display) -> string {
      d.show()
    }

    fn main() {
      print_it(None);
    }
        "#,
    )
    .assert_infer_code("interface_not_implemented");
}

#[test]
fn interface_covariant_return_rejected() {
    infer(
        r#"
interface Maker { fn make(self) -> Maker }
struct Widget {}
impl Widget { fn make(self) -> Widget { Widget {} } }
fn test() { let _m: Maker = Widget {} }
"#,
    )
    .assert_infer_code("interface_not_implemented");
}

#[test]
fn interface_generic_covariant_return_rejected() {
    infer(
        r#"
interface Container<T> {
  fn with(self, val: T) -> Container<T>
  fn get(self) -> T
}
struct Box<T> { value: T }
impl<T> Box<T> {
  fn with(self, val: T) -> Box<T> { Box { value: val } }
  fn get(self) -> T { self.value }
}
fn test() { let _c: Container<int> = Box { value: 0 } }
"#,
    )
    .assert_infer_code("interface_not_implemented");
}

#[test]
fn interface_cross_interface_return_rejected() {
    infer(
        r#"
interface Readable { fn read_val(self) -> string }
interface Source {
  fn name(self) -> string
  fn reader(self) -> Readable
}
struct TextReader { content: string }
impl TextReader { fn read_val(self) -> string { self.content } }
struct FileSource { filename: string, data: string }
impl FileSource {
  fn name(self) -> string { self.filename }
  fn reader(self) -> TextReader { TextReader { content: self.data } }
}
fn test() { let _s: Source = FileSource { filename: "f", data: "d" } }
"#,
    )
    .assert_infer_code("interface_not_implemented");
}

#[test]
fn interface_contravariant_param_rejected() {
    infer(
        r#"
interface Processable {
  fn value(self) -> int
  fn apply(self, f: fn(Processable) -> int) -> int
}
struct Data { n: int }
impl Data {
  fn value(self) -> int { self.n }
  fn apply(self, f: fn(Data) -> int) -> int { f(self) }
}
fn test() { let _p: Processable = Data { n: 1 } }
"#,
    )
    .assert_infer_code("interface_not_implemented");
}

#[test]
fn interface_pointer_receiver_rejected() {
    infer(
        r#"
interface Worker {
  fn name(self) -> string
  fn work(self) -> int
}
struct MyWorker { label: string, count: int }
impl MyWorker {
  fn name(self) -> string { self.label }
  fn work(self: Ref<MyWorker>) -> int { self.count }
}
fn test() { let _w: Worker = MyWorker { label: "t", count: 0 } }
"#,
    )
    .assert_infer_code("interface_not_implemented");
}

#[test]
fn cast_to_type_alias_to_interface() {
    infer(
        r#"
interface Named {
  fn Name() -> string
}
type MyNamed = Named
struct Dog { name: string }
impl Dog {
  fn Name(self: Dog) -> string { self.name }
}
fn test() -> MyNamed {
  Dog { name: "Rex" } as MyNamed
}
"#,
    )
    .assert_no_errors();
}

#[test]
fn ref_of_type_alias_to_interface_is_rejected() {
    infer(
        r#"
interface Named {
  fn Name() -> string
}
type MyNamed = Named
fn takes_ref(_r: Ref<MyNamed>) {}
"#,
    )
    .assert_infer_code("ref_of_interface");
}

#[test]
fn generic_type_alias_to_generic_interface_substitutes_methods() {
    infer(
        r#"
interface Container<T> {
  fn Get() -> T
}
type MyContainer<T> = Container<T>
struct IntBox { n: int }
impl IntBox {
  fn Get(self: IntBox) -> int { self.n }
}
fn take_int(c: MyContainer<int>) -> int {
  c.Get()
}
"#,
    )
    .assert_no_errors();
}

#[test]
fn distinct_aliases_unify_inside_option() {
    infer(
        r#"
type T1 = int
type T2 = int

fn main() {
  let _: Option<T1> = Some(1 as T2)
}
"#,
    )
    .assert_no_errors();
}

#[test]
fn distinct_aliases_unify_inside_nested_option() {
    infer(
        r#"
type T1 = int
type T2 = int

fn make() -> T2 { 1 as T2 }

fn main() {
  let _: Option<Option<T1>> = Some(Some(make()))
}
"#,
    )
    .assert_no_errors();
}

#[test]
fn distinct_aliases_unify_inside_map_value() {
    infer(
        r#"
type T1 = int
type T2 = int

fn make() -> T2 { 1 as T2 }

fn main() {
  let _: Map<string, T1> = Map.from([("a", make())])
}
"#,
    )
    .assert_no_errors();
}

#[test]
fn aliases_of_different_underlying_types_inside_option_error() {
    infer(
        r#"
type T1 = int
type T2 = string

fn main() {
  let _: Option<T1> = Some("hello" as T2)
}
"#,
    )
    .assert_infer_code("type_mismatch");
}

#[test]
fn cast_through_slice_with_alias_arg() {
    infer(
        r#"
type UserId = int

fn main() {
  let _ = [1, 2, 3] as Slice<UserId>
}
"#,
    )
    .assert_no_errors();
}

#[test]
fn cast_through_tuple_with_alias_elements() {
    infer(
        r#"
type T1 = int

fn main() {
  let _ = (1, 2) as (T1, T1)
}
"#,
    )
    .assert_no_errors();
}

#[test]
fn cast_through_generic_alias_to_underlying_generic() {
    infer(
        r#"
type MyOpt<T> = Option<T>
type T1 = int

fn main() {
  let _ = Some(1) as MyOpt<T1>
}
"#,
    )
    .assert_no_errors();
}

#[test]
fn cast_through_slice_with_distinct_underlying_alias_rejected() {
    infer(
        r#"
type T2 = string

fn main() {
  let _ = [1] as Slice<T2>
}
"#,
    )
    .assert_infer_code("invalid_cast");
}
