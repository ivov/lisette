use crate::spec::infer::*;

#[test]
fn simple_function_no_params() {
    infer(
        r#"
    fn greet() -> string {
      return "hello";
    }
        "#,
    )
    .assert_function_type(vec![], string_type());
}

#[test]
fn function_with_one_param() {
    infer(
        r#"
    fn double(x: int) -> int {
      return x * 2;
    }
        "#,
    )
    .assert_function_type(vec![int_type()], int_type());
}

#[test]
fn function_with_multiple_params() {
    infer(
        r#"
    fn add(x: int, y: int) -> int {
      return x + y;
    }
        "#,
    )
    .assert_function_type(vec![int_type(), int_type()], int_type());
}

#[test]
fn function_returns_bool() {
    infer(
        r#"
    fn is_positive(x: int) -> bool {
      return x > 0;
    }
        "#,
    )
    .assert_function_type(vec![int_type()], bool_type());
}

#[test]
fn function_returns_unit() {
    infer(
        r#"
    fn do_nothing() -> () {
      return ();
    }
        "#,
    )
    .assert_function_type(vec![], unit_type());
}

#[test]
fn infer_return_type_int() {
    infer(
        r#"
    fn get_value() -> int {
      return 42;
    }
        "#,
    )
    .assert_function_type(vec![], int_type());
}

#[test]
fn infer_with_param_usage() {
    infer(
        r#"
    fn process(x: int) -> int {
      return x + 1;
    }
        "#,
    )
    .assert_function_type(vec![int_type()], int_type());
}

#[test]
fn return_type_mismatch() {
    infer(r#"fn get_number() -> int { return "not a number"; }"#).assert_type_mismatch();
}

#[test]
fn undefined_function() {
    infer("undefined_function()").assert_not_found();
}

#[test]
fn recursive_function() {
    infer(
        r#"
    fn factorial(n: int) -> int {
      if n <= 1 {
        return 1;
      }
      return n * factorial(n - 1);
    }
        "#,
    )
    .assert_function_type(vec![int_type()], int_type());
}

#[test]
fn function_with_if_statement() {
    infer(
        r#"
    fn abs(x: int) -> int {
      if x < 0 {
        return -x;
      }
      return x;
    }
        "#,
    )
    .assert_function_type(vec![int_type()], int_type());
}

#[test]
fn inconsistent_return_types() {
    infer(
        r#"
    fn inconsistent(x: int) -> int {
      if x < 0 {
        return true;
      }
      return x;
    }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn call_function_no_args() {
    infer(
        r#"{
    let get_five = || -> int { 5 };
    get_five()
    }"#,
    )
    .assert_type_int();
}

#[test]
fn call_function_with_one_arg() {
    infer(
        r#"{
    let double = |x: int| -> int { x * 2 };
    double(5)
    }"#,
    )
    .assert_type_int();
}

#[test]
fn call_function_with_multiple_args() {
    infer(
        r#"{
    let add = |x: int, y: int| -> int { x + y };
    add(3, 7)
    }"#,
    )
    .assert_type_int();
}

#[test]
fn call_function_returning_bool() {
    infer(
        r#"{
    let is_positive = |x: int| -> bool { x > 0 };
    is_positive(10)
    }"#,
    )
    .assert_type_bool();
}

#[test]
fn call_function_returning_string() {
    infer(
        r#"{
    let get_greeting = || -> string { "hello" };
    get_greeting()
    }"#,
    )
    .assert_type_string();
}

#[test]
fn call_with_arithmetic_arg() {
    infer(
        r#"{
    let double = |x: int| -> int { x * 2 };
    double(3 + 4)
    }"#,
    )
    .assert_type_int();
}

#[test]
fn call_with_comparison_arg() {
    infer(
        r#"{
    let negate = |b: bool| -> bool { !b };
    negate(5 > 3)
    }"#,
    )
    .assert_type_bool();
}

#[test]
fn call_with_multiple_expression_args() {
    infer(
        r#"{
    let add = |x: int, y: int| -> int { x + y };
    add(1 + 2, 3 * 4)
    }"#,
    )
    .assert_type_int();
}

#[test]
fn nested_function_call() {
    infer(
        r#"{
    let double = |x: int| -> int { x * 2 };
    let increment = |x: int| -> int { x + 1 };
    double(increment(5))
    }"#,
    )
    .assert_type_int();
}

#[test]
fn deeply_nested_calls() {
    infer(
        r#"{
    let add_one = |x: int| -> int { x + 1 };
    add_one(add_one(add_one(0)))
    }"#,
    )
    .assert_type_int();
}

#[test]
fn multiple_nested_calls_as_args() {
    infer(
        r#"{
    let add = |x: int, y: int| -> int { x + y };
    let double = |x: int| -> int { x * 2 };
    add(double(2), double(3))
    }"#,
    )
    .assert_type_int();
}

#[test]
fn call_result_in_arithmetic() {
    infer(
        r#"{
    let get_value = || -> int { 10 };
    get_value() + 5
    }"#,
    )
    .assert_type_int();
}

#[test]
fn call_result_in_comparison() {
    infer(
        r#"{
    let get_value = || -> int { 10 };
    get_value() > 5
    }"#,
    )
    .assert_type_bool();
}

#[test]
fn multiple_calls_in_expression() {
    infer(
        r#"{
    let get_x = || -> int { 5 };
    let get_y = || -> int { 3 };
    get_x() + get_y()
    }"#,
    )
    .assert_type_int();
}

#[test]
fn call_in_let_binding() {
    infer(
        r#"{
    let compute = || -> int { 42 };
    let result = compute();
    result
    }"#,
    )
    .assert_type_int();
}

#[test]
fn call_in_if_condition() {
    infer(
        r#"{
    let is_ready = || -> bool { true };
    if is_ready() { 1 } else { 0 }
    }"#,
    )
    .assert_type_int();
}

#[test]
fn call_in_if_branches() {
    infer(
        r#"{
    let get_default = || -> int { 0 };
    let get_value = || -> int { 42 };
    if true { get_value() } else { get_default() }
    }"#,
    )
    .assert_type_int();
}

#[test]
fn function_calling_another_function() {
    infer(
        r#"{
    let helper = || -> int { 5 };
    let main_fn = || -> int { helper() + 10 };
    main_fn()
    }"#,
    )
    .assert_type_int();
}

#[test]
fn recursive_call_result() {
    infer(
        r#"
    fn factorial(n: int) -> int {
      if n <= 1 {
        return 1;
      }
      return n * factorial(n - 1);
    }
        "#,
    )
    .assert_function_type(vec![int_type()], int_type());
}

#[test]
fn wrong_number_of_args() {
    infer(
        r#"{
    fn add(x: int, y: int) -> int {
      return x + y;
    }
    add(5)
    }"#,
    )
    .assert_infer_code("arg_count_mismatch");
}

#[test]
fn wrong_arg_type() {
    infer(
        r#"{
    fn double(x: int) -> int {
      return x * 2;
    }
    double(true)
    }"#,
    )
    .assert_type_mismatch();
}

#[test]
fn undefined_function_call() {
    infer("nonexistent_function()").assert_not_found();
}

#[test]
fn lambda_no_params() {
    infer(
        r#"{
    let f = || 42;
    f
    }"#,
    )
    .assert_function_type(vec![], int_type());
}

#[test]
fn lambda_no_params_after_let_captures_binding() {
    infer(
        r#"{
    let make = || -> fn() -> int {
      let x = 42
      || x
    };
    make()
    }"#,
    )
    .assert_function_type(vec![], int_type());
}

#[test]
fn lambda_no_params_in_closure_captures_let() {
    infer(
        r#"{
    let outer = |x: int| {
      let y = x + 1
      || y
    };
    outer
    }"#,
    )
    .assert_function_type(vec![int_type()], fun_type(vec![], int_type()));
}

#[test]
fn lambda_single_param_with_type() {
    infer(
        r#"{
    let double = |x: int| x * 2;
    double
    }"#,
    )
    .assert_function_type(vec![int_type()], int_type());
}

#[test]
fn lambda_multiple_params_with_types() {
    infer(
        r#"{
    let add = |x: int, y: int| x + y;
    add
    }"#,
    )
    .assert_function_type(vec![int_type(), int_type()], int_type());
}

#[test]
fn lambda_with_return_type_annotation() {
    infer(
        r#"{
    let square = |x: int| -> int { x * x };
    square
    }"#,
    )
    .assert_function_type(vec![int_type()], int_type());
}

#[test]
fn lambda_returns_bool() {
    infer(
        r#"{
    let is_positive = |x: int| x > 0;
    is_positive
    }"#,
    )
    .assert_function_type(vec![int_type()], bool_type());
}

#[test]
fn lambda_returns_string() {
    infer(
        r#"{
    let greet = |name: string| "Hello, " + name;
    greet
    }"#,
    )
    .assert_function_type(vec![string_type()], string_type());
}

#[test]
fn lambda_assigned_to_variable() {
    infer(
        r#"{
    let f = |x: int| x + 1;
    f(5)
    }"#,
    )
    .assert_type_int();
}

#[test]
fn lambda_call_immediately() {
    infer(
        r#"{
    let result = (|x: int| x + 1)(5);
    result
    }"#,
    )
    .assert_type_int();
}

#[test]
fn lambda_multiple_calls() {
    infer(
        r#"{
    let double = |x: int| x * 2;
    let a = double(5);
    double(10)
    }"#,
    )
    .assert_type_int();
}

#[test]
fn lambda_as_function_argument() {
    infer(
        r#"{
    let apply = |f: fn(int) -> int, x: int| -> int { f(x) };
    apply(|x: int| x * 2, 5)
    }"#,
    )
    .assert_type_int();
}

#[test]
fn lambda_as_argument_infers_from_expected_type() {
    infer(
        r#"{
    let apply = |f: fn(int) -> int, x: int| -> int { f(x) };
    apply(|x| x * 2, 5)
    }"#,
    )
    .assert_type_int();
}

#[test]
fn lambda_with_multiple_param_types_inferred() {
    infer(
        r#"{
    let combine = |f: fn(int, int) -> int, a: int, b: int| -> int { f(a, b) };
    combine(|x, y| x + y, 3, 4)
    }"#,
    )
    .assert_type_int();
}

#[test]
fn lambda_returned_from_function() {
    infer(
        r#"
    fn make_adder() -> fn(int) -> int {
      return |x: int| x + 1;
    }
        "#,
    )
    .assert_function_type(vec![], fun_type(vec![int_type()], int_type()));
}

#[test]
fn lambda_returned_and_called() {
    infer(
        r#"{
    let make_multiplier = || -> fn(int) -> int { |x: int| x * 2 };
    let mult = make_multiplier();
    mult(5)
    }"#,
    )
    .assert_type_int();
}

#[test]
fn nested_lambda_currying() {
    infer(
        r#"{
    let add_curry = |x: int| |y: int| x + y;
    add_curry
    }"#,
    )
    .assert_function_type(vec![int_type()], fun_type(vec![int_type()], int_type()));
}

#[test]
fn nested_lambda_called() {
    infer(
        r#"{
    let add_curry = |x: int| |y: int| x + y;
    let add5 = add_curry(5);
    add5(10)
    }"#,
    )
    .assert_type_int();
}

#[test]
fn triple_nested_lambda() {
    infer(
        r#"{
    let triple = |a: int| |b: int| |c: int| a + b + c;
    triple
    }"#,
    )
    .assert_function_type(
        vec![int_type()],
        fun_type(vec![int_type()], fun_type(vec![int_type()], int_type())),
    );
}

#[test]
fn lambda_captures_local_variable() {
    infer(
        r#"{
    let threshold = 10;
    let check = |x: int| x > threshold;
    check
    }"#,
    )
    .assert_function_type(vec![int_type()], bool_type());
}

#[test]
fn lambda_captures_and_uses_variable() {
    infer(
        r#"{
    let base = 100;
    let add_to_base = |x: int| x + base;
    add_to_base(50)
    }"#,
    )
    .assert_type_int();
}

#[test]
fn lambda_captures_multiple_variables() {
    infer(
        r#"{
    let a = 10;
    let b = 20;
    let combine = |x: int| x + a + b;
    combine
    }"#,
    )
    .assert_function_type(vec![int_type()], int_type());
}

#[test]
fn lambda_captures_string_variable() {
    infer(
        r#"{
    let greeting = "Hello, ";
    let greet = |name: string| greeting + name;
    greet
    }"#,
    )
    .assert_function_type(vec![string_type()], string_type());
}

#[test]
fn lambda_returns_tuple() {
    infer(
        r#"{
    let make_pair = |x: int| (x, x * 2);
    make_pair
    }"#,
    )
    .assert_function_type(vec![int_type()], tuple_type(vec![int_type(), int_type()]));
}

#[test]
fn lambda_with_slice_param() {
    infer(
        r#"
    fn process_slice(f: fn(Slice<int>) -> int, items: Slice<int>) -> int {
      return f(items);
    }
        "#,
    )
    .assert_function_type(
        vec![
            fun_type(vec![slice_type(int_type())], int_type()),
            slice_type(int_type()),
        ],
        int_type(),
    );
}

#[test]
fn lambda_with_struct_param() {
    infer(
        r#"{
    struct Point {
      x: int,
      y: int,
    }
    |p: Point| p.x
    }"#,
    )
    .assert_function_type(vec![con_type("Point", vec![])], int_type());
}

#[test]
fn lambda_returns_struct() {
    infer(
        r#"{
    struct Point {
      x: int,
      y: int,
    }
    |x: int, y: int| Point { x: x, y: y }
    }"#,
    )
    .assert_function_type(vec![int_type(), int_type()], con_type("Point", vec![]));
}

#[test]
fn lambda_with_generic_struct() {
    infer(
        r#"{
    struct Box<T> {
      value: T,
    }
    |b: Box<int>| b.value
    }"#,
    )
    .assert_function_type(vec![con_type("Box", vec![int_type()])], int_type());
}

#[test]
fn lambda_with_option_param() {
    infer(
        r#"{
    |opt: Option<int>| match opt {
      Option.Some(_) => true,
      Option.None => false,
    }
    }"#,
    )
    .assert_function_type(vec![con_type("Option", vec![int_type()])], bool_type());
}

#[test]
fn lambda_returns_result() {
    infer(
        r#"{
    |x: int, y: int| {
      if y == 0 {
        Result.Err("division by zero")
      } else {
        Result.Ok(x / y)
      }
    }
    }"#,
    )
    .assert_function_type(
        vec![int_type(), int_type()],
        con_type("Result", vec![int_type(), string_type()]),
    );
}

#[test]
fn lambda_param_type_mismatch() {
    infer(
        r#"{
    let f = |x: int| x + 1;
    f("not a number")
    }"#,
    )
    .assert_type_mismatch();
}

#[test]
fn lambda_return_type_mismatch() {
    infer(
        r#"
    let f = |x: int| -> bool { x + 1 };
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn lambda_wrong_number_of_args() {
    infer(
        r#"{
    let add = |x: int, y: int| x + y;
    add(5)
    }"#,
    )
    .assert_infer_code("arg_count_mismatch");
}

#[test]
fn lambda_capture_undefined_variable() {
    infer(
        r#"
    let f = |x: int| x + undefined_var;
        "#,
    )
    .assert_not_found();
}

#[test]
fn lambda_returns_unit() {
    infer(
        r#"{
    let do_nothing = |x: int| ();
    do_nothing
    }"#,
    )
    .assert_function_type(vec![int_type()], unit_type());
}

#[test]
fn lambda_with_unit_param() {
    infer(
        r#"{
    let get_value = |_: ()| 42;
    get_value
    }"#,
    )
    .assert_function_type(vec![unit_type()], int_type());
}

#[test]
fn lambda_passed_to_higher_order_function() {
    infer(
        r#"{
    let map = |f: fn(int) -> int, x: int| -> int { f(x) };
    map(|x: int| x * 2, 5)
    }"#,
    )
    .assert_type_int();
}

#[test]
fn lambda_composition() {
    infer(
        r#"
    fn compose(f: fn(int) -> int, g: fn(int) -> int) -> fn(int) -> int {
      return |x: int| f(g(x));
    }
        "#,
    )
    .assert_function_type(
        vec![
            fun_type(vec![int_type()], int_type()),
            fun_type(vec![int_type()], int_type()),
        ],
        fun_type(vec![int_type()], int_type()),
    );
}

#[test]
fn load_simple_module_with_function() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "mylib",
        "lib.lis",
        r#"
    fn add(x: int, y: int) -> int {
      return x + y;
    }
        "#,
    );

    infer_module("mylib", fs).assert_no_errors();
}

#[test]
fn module_with_multiple_files() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "mylib",
        "math.lis",
        r#"
    fn add(x: int, y: int) -> int {
      return x + y;
    }
        "#,
    );

    fs.add_file(
        "mylib",
        "string.lis",
        r#"
    fn concat(a: string, b: string) -> string {
      return a;
    }
        "#,
    );

    infer_module("mylib", fs).assert_no_errors();
}

#[test]
fn module_with_struct_definition() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "shapes",
        "lib.lis",
        r#"
    struct Point {
      x: int,
      y: int,
    }
        "#,
    );

    infer_module("shapes", fs).assert_no_errors();
}

#[test]
fn module_with_enum_definition() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "types",
        "lib.lis",
        r#"
    pub enum Color {
      Red,
      Green,
      Blue,
    }
        "#,
    );

    infer_module("types", fs).assert_no_errors();
}

#[test]
fn module_using_imported_type_in_struct() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "geo",
        "lib.lis",
        r#"
    pub struct Point {
      pub x: int,
      pub y: int,
    }
        "#,
    );

    fs.add_file(
        "shapes",
        "lib.lis",
        r#"
    import "geo"

    pub struct Line {
      pub start: geo.Point,
      pub end: geo.Point,
    }
        "#,
    );

    fs.add_file(
        "main",
        "main.lis",
        r#"
    import "geo"
    import "shapes"

    fn test() -> shapes.Line {
      let p1 = geo.Point { x: 0, y: 0 };
      let p2 = geo.Point { x: 1, y: 1 };
      return shapes.Line { start: p1, end: p2 };
    }
        "#,
    );

    infer_module("main", fs).assert_no_errors();
}

#[test]
fn import_and_use_function() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "math",
        "lib.lis",
        r#"
    pub fn double(x: int) -> int {
      return x * 2;
    }
        "#,
    );

    fs.add_file(
        "main",
        "main.lis",
        r#"
    import "math"

    fn test() -> int {
      return math.double(5);
    }
        "#,
    );

    infer_module("main", fs).assert_no_errors();
}

#[test]
fn import_multiple_modules() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "math",
        "lib.lis",
        r#"
    pub fn add(x: int, y: int) -> int {
      return x + y;
    }
        "#,
    );

    fs.add_file(
        "utils",
        "lib.lis",
        r#"
    pub fn identity(x: int) -> int {
      return x;
    }
        "#,
    );

    fs.add_file(
        "main",
        "main.lis",
        r#"
    import "math"
    import "utils"

    fn test() -> int {
      let a = math.add(1, 2);
      return utils.identity(a);
    }
        "#,
    );

    infer_module("main", fs).assert_no_errors();
}

#[test]
fn import_type_from_module() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "shapes",
        "lib.lis",
        r#"
    pub struct Point {
      pub x: int,
      pub y: int,
    }
        "#,
    );

    fs.add_file(
        "main",
        "main.lis",
        r#"
    import "shapes"

    fn origin() -> shapes.Point {
      return shapes.Point { x: 0, y: 0 };
    }
        "#,
    );

    infer_module("main", fs).assert_no_errors();
}

#[test]
fn import_and_use_struct_in_function_param() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "shapes",
        "lib.lis",
        r#"
    pub struct Point {
      pub x: int,
      pub y: int,
    }
        "#,
    );

    fs.add_file(
        "main",
        "main.lis",
        r#"
    import "shapes"

    fn get_x(p: shapes.Point) -> int {
      return p.x;
    }
        "#,
    );

    infer_module("main", fs).assert_no_errors();
}

#[test]
fn cross_module_enum_variant_access() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "types",
        "lib.lis",
        r#"
    pub enum Status {
      Active,
      Inactive,
    }
        "#,
    );

    fs.add_file(
        "main",
        "main.lis",
        r#"
    import "types"

    fn check(status: types.Status) -> int {
      match status {
        types.Status.Active => 1,
        types.Status.Inactive => 0,
      }
    }

    fn make_active() -> types.Status {
      types.Status.Active
    }
        "#,
    );

    infer_module("main", fs).assert_no_errors();
}

#[test]
fn module_not_found() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "main",
        "main.lis",
        r#"
    import "nonexistent"

    fn test() {}
        "#,
    );

    infer_module("main", fs).assert_resolve_code("module_not_found");
}

#[test]
fn imported_function_not_found() {
    let mut fs = MockFileSystem::new();

    fs.add_file("math", "lib.lis", "fn add(x: int, y: int) -> int { x + y }");

    fs.add_file(
        "main",
        "main.lis",
        r#"
    import "math"

    fn test() -> int {
      return math.subtract(5, 2);
    }
        "#,
    );

    infer_module("main", fs).assert_resolve_code("not_found_in_module");
}

#[test]
fn imported_type_not_found() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "shapes",
        "lib.lis",
        r#"
    struct Point {
      x: int,
      y: int,
    }
        "#,
    );

    fs.add_file(
        "main",
        "main.lis",
        r#"
    import "shapes"

    fn test() -> shapes.Circle {
      return shapes.Circle { radius: 5 };
    }
        "#,
    );

    infer_module("main", fs).assert_resolve_code("struct_not_found");
}

#[test]
fn private_type_not_accessible_cross_module() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "types",
        "lib.lis",
        r#"
    struct PrivateStruct {
      value: int,
    }
        "#,
    );

    fs.add_file(
        "main",
        "main.lis",
        r#"
    import "types"

    fn test() -> types.PrivateStruct {
      types.PrivateStruct { value: 42 }
    }
        "#,
    );

    infer_module("main", fs).assert_resolve_code("struct_not_found");
}

#[test]
fn private_enum_variant_not_accessible_cross_module() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "types",
        "lib.lis",
        r#"
    enum Status {
      Active,
      Inactive,
    }
        "#,
    );

    fs.add_file(
        "main",
        "main.lis",
        r#"
    import "types"

    fn check(status: types.Status) -> int {
      match status {
        types.Status.Active => 1,
        types.Status.Inactive => 0,
      }
    }
        "#,
    );

    infer_module("main", fs).assert_resolve_code("type_not_found");
}

#[test]
fn type_error_across_modules() {
    let mut fs = MockFileSystem::new();

    fs.add_file("math", "lib.lis", "pub fn square(x: int) -> int { x * x }");

    fs.add_file(
        "main",
        "main.lis",
        r#"
    import "math"

    fn test() {
      math.square("hello");
    }
        "#,
    );

    infer_module("main", fs).assert_type_mismatch();
}

#[test]
fn wrong_number_of_arguments_across_modules() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "math",
        "lib.lis",
        "pub fn add(x: int, y: int) -> int { x + y }",
    );

    fs.add_file(
        "main",
        "main.lis",
        r#"
    import "math"

    fn test() -> int {
      return math.add(5);
    }
        "#,
    );

    infer_module("main", fs).assert_infer_code("arg_count_mismatch");
}

#[test]
fn transitive_imports() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "core",
        "lib.lis",
        r#"
    pub fn identity(x: int) -> int {
      return x;
    }
        "#,
    );

    fs.add_file(
        "utils",
        "lib.lis",
        r#"
    import "core"

    pub fn process(x: int) -> int {
      return core.identity(x);
    }
        "#,
    );

    fs.add_file(
        "main",
        "main.lis",
        r#"
    import "utils"

    fn test() -> int {
      return utils.process(42);
    }
        "#,
    );

    infer_module("main", fs).assert_no_errors();
}

#[test]
fn diamond_dependency() {
    let mut fs = MockFileSystem::new();

    fs.add_file("base", "lib.lis", "pub fn base_fn() -> int { 1 }");

    fs.add_file(
        "left",
        "lib.lis",
        r#"
    import "base"
    pub fn left_fn() -> int { base.base_fn() }
        "#,
    );

    fs.add_file(
        "right",
        "lib.lis",
        r#"
    import "base"
    pub fn right_fn() -> int { base.base_fn() }
        "#,
    );

    fs.add_file(
        "main",
        "main.lis",
        r#"
    import "left"
    import "right"

    fn test() -> int {
      return left.left_fn() + right.right_fn();
    }
        "#,
    );

    infer_module("main", fs).assert_no_errors();
}

#[test]
fn import_generic_struct() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "container",
        "lib.lis",
        r#"
    pub struct Box<T> {
      pub value: T,
    }
        "#,
    );

    fs.add_file(
        "main",
        "main.lis",
        r#"
    import "container"

    fn test() -> container.Box<int> {
      return container.Box { value: 42 };
    }
        "#,
    );

    infer_module("main", fs).assert_no_errors();
}

#[test]
fn import_generic_function() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "utils",
        "lib.lis",
        r#"
    pub fn identity<T>(x: T) -> T {
      return x;
    }
        "#,
    );

    fs.add_file(
        "main",
        "main.lis",
        r#"
    import "utils"

    fn test() -> int {
      return utils.identity(42);
    }
        "#,
    );

    infer_module("main", fs).assert_no_errors();
}

#[test]
fn method_calls_across_modules() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "shapes",
        "lib.lis",
        r#"
    pub struct Point {
      pub x: int,
      pub y: int,
    }

    impl Point {
      pub fn distance_from_origin(self: Point) -> int {
        return self.x * self.x + self.y * self.y;
      }
    }
        "#,
    );

    fs.add_file(
        "main",
        "main.lis",
        r#"
    import "shapes"

    fn test() -> int {
      let p = shapes.Point { x: 3, y: 4 };
      return p.distance_from_origin();
    }
        "#,
    );

    infer_module("main", fs).assert_no_errors();
}

#[test]
fn type_alias_across_modules() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "types",
        "lib.lis",
        r#"
    pub type MyInt = int
        "#,
    );

    fs.add_file(
        "main",
        "main.lis",
        r#"
    import "types"

    fn test(x: types.MyInt) -> types.MyInt {
      return x + 1;
    }
        "#,
    );

    infer_module("main", fs).assert_no_errors();
}

#[test]
fn method_with_ref_receiver_can_mutate() {
    infer(
        r#"
    struct Point { x: int, y: int }

    impl Point {
      fn set_x(self: Ref<Point>, x: int) {
        self.x = x;
      }
    }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn explicit_unit_return_rejects_non_unit_body() {
    infer(
        r#"
    fn f() -> () { 1 }
        "#,
    )
    .assert_type_mismatch();
}

#[test]
fn implicit_unit_return_allows_any_body() {
    infer(
        r#"
    fn f() { 1 }
        "#,
    )
    .assert_no_errors();
}

#[test]
fn lambda_explicit_unit_return_rejects_non_unit_body() {
    infer(
        r#"{
    let f = || -> () { 1 };
    f
    }"#,
    )
    .assert_type_mismatch();
}

#[test]
fn lambda_implicit_unit_return_allows_any_body() {
    infer(
        r#"{
    let f = || { 1 };
    f
    }"#,
    )
    .assert_no_errors();
}

#[test]
fn lambda_contextual_unit_return_allows_call_returning_result() {
    infer(
        r#"
import "go:fmt"

fn take(f: fn() -> ()) { f() }

fn main() {
  take(|| { fmt.Println("hi") })
}
"#,
    )
    .assert_no_errors();
}

#[test]
fn lambda_in_option_callback_allows_call_returning_result() {
    infer(
        r#"
import "go:fmt"

struct Cmd {
  pub Run: Option<fn(string) -> ()>,
}

fn main() {
  let _c = Cmd {
    Run: Some(|name: string| { fmt.Println(name) }),
  }
}
"#,
    )
    .assert_no_errors();
}

#[test]
fn lambda_explicit_unit_annotation_still_rejects_non_unit_body() {
    infer(
        r#"
fn take(f: fn() -> ()) { f() }
fn main() { take(|| -> () { 42 }) }
"#,
    )
    .assert_type_mismatch();
}

#[test]
fn private_function_not_accessible_via_module_struct() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "lib",
        "lib.lis",
        r#"
    fn private_fn() -> int { 42 }
    pub fn public_fn() -> int { 1 }
        "#,
    );

    fs.add_file(
        "main",
        "main.lis",
        r#"
    import "lib"

    fn test() -> int {
      lib.private_fn()
    }
        "#,
    );

    infer_module("main", fs).assert_resolve_code("not_found_in_module");
}

#[test]
fn private_function_not_accessible_via_bare_name() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "lib",
        "lib.lis",
        r#"
    fn private_fn() -> int { 42 }
    pub fn public_fn() -> int { 1 }
        "#,
    );

    fs.add_file(
        "main",
        "main.lis",
        r#"
    import "lib"

    fn test() -> int {
      private_fn()
    }
        "#,
    );

    infer_module("main", fs).assert_resolve_code("name_not_found");
}

#[test]
fn compile_private_function_not_accessible_via_bare_name() {
    use crate::_harness::build::compile_check;
    use semantics::store::ENTRY_MODULE_ID;

    let mut fs = MockFileSystem::new();

    fs.add_file(
        "lib",
        "lib.lis",
        r#"
    fn private_fn() -> int { 42 }
    pub fn public_fn() -> int { 1 }
        "#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
    import "lib"

    fn test() -> int {
      private_fn()
    }
        "#,
    );

    let result = compile_check(fs);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code_str() == Some("resolve.name_not_found")),
        "Expected name_not_found error for private_fn, got: {:?}",
        result.errors
    );
}

#[test]
fn private_function_not_accessible_through_module_struct() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "lib",
        "lib.lis",
        r#"
    pub fn public_fn() -> int { 1 }
    fn private_fn() -> int { 2 }
        "#,
    );

    fs.add_file(
        "main",
        "main.lis",
        r#"
    import "lib"

    fn test() -> int {
      lib.private_fn()
    }
        "#,
    );

    infer_module("main", fs).assert_resolve_code("not_found_in_module");
}

#[test]
fn cross_module_enum_exhaustive() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "types",
        "lib.lis",
        r#"
    pub enum Status {
      Active,
      Inactive,
      Pending,
    }
        "#,
    );

    fs.add_file(
        "main",
        "main.lis",
        r#"
    import "types"

    fn check(s: types.Status) -> int {
      match s {
        types.Status.Active => 1,
        types.Status.Inactive => 2,
        types.Status.Pending => 3,
      }
    }
        "#,
    );

    infer_module("main", fs).assert_no_errors();
}

#[test]
fn cross_module_enum_non_exhaustive() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "types",
        "lib.lis",
        r#"
    pub enum Status {
      Active,
      Inactive,
      Pending,
    }
        "#,
    );

    fs.add_file(
        "main",
        "main.lis",
        r#"
    import "types"

    fn check(s: types.Status) -> int {
      match s {
        types.Status.Active => 1,
        types.Status.Inactive => 2,
      }
    }
        "#,
    );

    infer_module("main", fs).assert_exhaustiveness_error();
}

#[test]
fn cross_module_enum_redundant() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "types",
        "lib.lis",
        r#"
    pub enum Status {
      Active,
      Inactive,
    }
        "#,
    );

    fs.add_file(
        "main",
        "main.lis",
        r#"
    import "types"

    fn check(s: types.Status) -> int {
      match s {
        types.Status.Active => 1,
        types.Status.Inactive => 2,
        types.Status.Active => 3,
      }
    }
        "#,
    );

    infer_module("main", fs).assert_redundancy_error();
}

#[test]
fn calling_non_function_produces_error() {
    infer(
        r#"
    fn main() {
      let x = 1;
      x(2);
    }
        "#,
    )
    .assert_infer_code("not_callable");
}

#[test]
fn calling_string_produces_error() {
    infer(
        r#"
    fn main() {
      let s = "hello";
      s();
    }
        "#,
    )
    .assert_infer_code("not_callable");
}

#[test]
fn calling_generic_type_constructor_no_ice() {
    infer(
        r#"
    fn main() {
      Slice<byte>()
    }
        "#,
    )
    .assert_infer_code("not_callable");
}

#[test]
fn immediate_closure_application_infers_param_type() {
    infer(
        r#"{
    let result = (|x| x + 1)(41);
    result
    }"#,
    )
    .assert_type_int();
}

#[test]
fn immediate_closure_application_multiple_params() {
    infer(
        r#"{
    let result = (|a, b| a + b)(10, 20);
    result
    }"#,
    )
    .assert_type_int();
}

#[test]
fn lambda_in_match_infers_params_from_return_type() {
    infer(
        r#"
    fn get_op(op: string) -> fn(int, int) -> int {
      match op {
        "add" => |a, b| a + b,
        _ => |_a, _b| 0,
      }
    }
        "#,
    )
    .assert_function_type(
        vec![string_type()],
        fun_type(vec![int_type(), int_type()], int_type()),
    );
}

#[test]
fn lambda_in_if_infers_params_from_return_type() {
    infer(
        r#"
    fn get_op(add: bool) -> fn(int) -> int {
      if add { |x| x + 1 } else { |x| x - 1 }
    }
        "#,
    )
    .assert_function_type(vec![bool_type()], fun_type(vec![int_type()], int_type()));
}

#[test]
fn private_method_not_accessible_cross_module() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "shapes",
        "lib.lis",
        r#"
    pub struct Point {
      pub x: int,
      pub y: int,
    }

    impl Point {
      fn internal_calc(self: Point) -> int {
        self.x * self.y
      }
    }
        "#,
    );

    fs.add_file(
        "main",
        "main.lis",
        r#"
    import "shapes"

    fn test() -> int {
      let p = shapes.Point { x: 3, y: 4 };
      p.internal_calc()
    }
        "#,
    );

    let result = infer_module("main", fs);
    assert!(
        !result.errors.is_empty(),
        "Expected private method error, but no errors were raised"
    );
    let errors_str = format!("{:?}", result.errors);
    assert!(
        errors_str.contains("Private method"),
        "Expected 'Private method' error, got: {}",
        errors_str
    );
}

#[test]
fn pub_method_accessible_cross_module() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "shapes",
        "lib.lis",
        r#"
    pub struct Point {
      pub x: int,
      pub y: int,
    }

    impl Point {
      pub fn distance(self: Point) -> int {
        self.x * self.x + self.y * self.y
      }
    }
        "#,
    );

    fs.add_file(
        "main",
        "main.lis",
        r#"
    import "shapes"

    fn test() -> int {
      let p = shapes.Point { x: 3, y: 4 };
      p.distance()
    }
        "#,
    );

    infer_module("main", fs).assert_no_errors();
}

#[test]
fn cross_module_turbofish_on_static_method() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "collections",
        "mod.lis",
        r#"
    pub struct Stack<T> {
      pub items: Slice<T>,
    }

    impl<T> Stack<T> {
      pub fn empty() -> Stack<T> {
        Stack { items: [] }
      }

      pub fn push(self: Stack<T>, item: T) -> Stack<T> {
        Stack { items: self.items.append(item) }
      }

      pub fn size(self: Stack<T>) -> int {
        self.items.length()
      }
    }
        "#,
    );

    fs.add_file(
        "main",
        "main.lis",
        r#"
    import "collections"

    fn test() -> int {
      let s = collections.Stack.empty<int>().push(1).push(2);
      s.size()
    }
        "#,
    );

    infer_module("main", fs).assert_no_errors();
}

#[test]
fn private_static_method_not_accessible_cross_module() {
    use crate::_harness::filesystem::MockFileSystem;
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "shapes",
        "lib.lis",
        r#"
    pub struct Circle {
      pub radius: float64,
    }

    impl Circle {
      fn secret_factory(r: float64) -> Circle {
        Circle { radius: r * 2.0 }
      }
    }
        "#,
    );

    fs.add_file(
        "main",
        "main.lis",
        r#"
    import "shapes"

    fn test() -> shapes.Circle {
      shapes.Circle.secret_factory(3.0)
    }
        "#,
    );

    let result = infer_module("main", fs);
    assert!(
        !result.errors.is_empty(),
        "Expected private method error, but no errors were raised"
    );
    let errors_str = format!("{:?}", result.errors);
    assert!(
        errors_str.contains("Private method"),
        "Expected 'Private method' error, got: {}",
        errors_str
    );
}

#[test]
fn go_function_with_non_variadic_any_param_accepts_concrete_type() {
    use crate::_harness::filesystem::MockFileSystem;
    let mut fs = MockFileSystem::new();
    fs.add_file(
        "mylib",
        "lib.d.lis",
        r#"
pub fn process(data: Slice<uint8>, target: Unknown) -> error
"#,
    );

    let source = r#"
import "mylib"

struct User {
  pub name: string,
}

fn main() {
  let mut u = User { name: "" };
  let _ = mylib.process("data" as Slice<uint8>, &u);
}
"#;
    fs.add_file("main", "main.lis", source);

    infer_module("main", fs).assert_no_errors();
}

#[test]
fn mut_param_allows_mutation() {
    infer(
        r#"
fn sort(mut items: Slice<int>) {
  items = [1, 2, 3]
}

fn main() {
  let mut data = [3, 1, 2];
  sort(data)
}
"#,
    )
    .assert_no_errors();
}

#[test]
fn mut_param_requires_mut_arg() {
    infer(
        r#"
fn sort(mut items: Slice<int>) {
  items = [1, 2, 3]
}

fn main() {
  let data = [3, 1, 2];
  sort(data)
}
"#,
    )
    .assert_infer_code("immutable_arg_to_mut_param");
}

#[test]
fn mut_param_propagation_through_wrapper() {
    infer(
        r#"
fn sort(mut items: Slice<int>) {
  items = [1, 2, 3]
}

fn my_sort(mut items: Slice<int>) {
  sort(items)
}

fn main() {
  let mut data = [3, 1, 2];
  my_sort(data)
}
"#,
    )
    .assert_no_errors();
}

#[test]
fn mut_param_propagation_missing_mut_on_wrapper() {
    infer(
        r#"
fn sort(mut items: Slice<int>) {
  items = [1, 2, 3]
}

fn my_sort(items: Slice<int>) {
  sort(items)
}
"#,
    )
    .assert_infer_code("immutable_arg_to_mut_param");
}
