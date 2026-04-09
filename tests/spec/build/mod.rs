use crate::_harness::build::{compile_check, compile_check_standalone};
use crate::_harness::filesystem::MockFileSystem;
use crate::_harness::infer::infer;
use crate::assert_build_snapshot;
use semantics::store::ENTRY_MODULE_ID;

#[test]
fn cross_module_generic_constructor_type_args() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "util",
        "box.lis",
        r#"
pub struct Box<T> {
  pub items: Slice<T>,
}

impl<T> Box<T> {
  pub fn new() -> Box<T> {
    Box { items: [] }
  }
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:fmt"
import "util"

fn main() {
  let b: util.Box<string> = util.Box.new()
  fmt.Println(b.items)
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn user_function_returning_result_no_type_args() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:fmt"
import "go:strconv"

fn parse_int(s: string) -> Result<int, error> {
  strconv.Atoi(s)
}

fn main() {
  match parse_int("42") {
    Ok(n) => fmt.Println(n),
    Err(e) => fmt.Println(e),
  }
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn go_tuple_call_wrapped() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:strings"
import "go:fmt"

fn main() {
  match strings.Cut("hello:world", ":") {
    Some(result) => fmt.Print(result.0),
    None => {},
  }
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn go_option_call_wrapped() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:go/doc/comment"
import "go:fmt"

fn main() {
  let result = comment.DefaultLookupPackage("math");
  fmt.Print(result.is_some())
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn go_result_ref_nil_guard() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:os"
import "go:fmt"

fn main() {
  let result = os.Open("/tmp/test.txt");
  fmt.Print(result.is_ok())
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn go_single_pointer_option_wrapped() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:flag"
import "go:fmt"

fn main() {
  let result = flag.Lookup("verbose");
  fmt.Print(result.is_some())
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn go_method_single_pointer_option_wrapped() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:container/list"
import "go:fmt"

fn main() {
  let l = list.New()
  let _ = l.PushBack(42)
  let front = l.Front()
  fmt.Print(front.is_some())
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn go_method_result_wrapped() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:bufio"
import "go:strings"
import "go:fmt"

fn main() {
  let reader = bufio.NewReader(strings.NewReader("hello\nworld"))
  let line = reader.ReadString(10)
  match line {
    Ok(s) => fmt.Print(s),
    Err(_) => fmt.Print("error"),
  }
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn go_method_option_comma_ok_wrapped() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:context"
import "go:fmt"

fn main() {
  let ctx = context.Background()
  let deadline = ctx.Deadline()
  fmt.Print(deadline.is_some())
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn go_nullable_comma_ok_nil_check() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:runtime/debug"
import "go:fmt"

fn main() {
  let info = debug.ReadBuildInfo()
  fmt.Print(info.is_some())
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn multimodule_type_alias_field_access() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "geo",
        "lib.lis",
        r#"
pub struct Point { pub x: int, pub y: int }

pub type Coordinate = Point
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "geo"

fn get_x(c: geo.Coordinate) -> int {
  c.x
}

fn main() {
  let c = geo.Point { x: 10, y: 20 };
  let _ = get_x(c)
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn multimodule_basic() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "utils"

fn main() {
  let result = utils.add(1, 2)
}
"#,
    );

    fs.add_file(
        "utils",
        "helpers.lis",
        r#"
pub fn add(a: int, b: int) -> int {
  return a + b
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn multimodule_tuple_struct_field_access() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "types",
        "lib.lis",
        r#"
pub struct UserId(int)
pub struct Point(int, int)
pub struct Pair<A, B>(A, B)
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "types"

fn get_raw_id(id: types.UserId) -> int {
  id.0
}

fn get_x(p: types.Point) -> int {
  p.0
}

fn main() {
  let id = types.UserId(42);
  let p = types.Point(10, 20);
  let pair = types.Pair(1, "hello");
  let _ = get_raw_id(id) + get_x(p) + pair.0
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn multimodule_generic_tuple_struct_method() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "types",
        "lib.lis",
        r#"
pub struct Wrapper<T>(T)

impl<T> Wrapper<T> {
  pub fn unwrap(self: Wrapper<T>) -> T {
    self.0
  }
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "types"

fn main() {
  let w = types.Wrapper(42);
  let _ = w.unwrap()
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn multimodule_ufcs_method() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "types",
        "lib.lis",
        r#"
pub struct Box<T> { pub value: T }

impl<T> Box<T> {
  pub fn map<U>(self: Box<T>, f: fn(T) -> U) -> Box<U> {
    Box { value: f(self.value) }
  }
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "types"

fn main() {
  let b = types.Box { value: 10 };
  let mapped = b.map(|x: int| x * 2);
  let _ = mapped.value
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn error_in_imported_file_shows_correct_source() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "math",
        "lib.lis",
        r#"
pub fn add(a: int, b: int) -> int {
  return "not a number"
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "math"

fn main() {
  let _ = math.add(1, 2)
}
"#,
    );

    let result = compile_check(fs);

    assert_eq!(result.errors.len(), 1, "Expected exactly one error");

    let error = &result.errors[0];
    let file_id = error.file_id().expect("Error should have a file_id");

    let file = result
        .files
        .get(&file_id)
        .expect("file_id should exist in files map");

    assert_eq!(
        file.name, "lib.lis",
        "Error should be in lib.lis, not main.lis"
    );

    assert!(
        file.source.contains(r#"return "not a number""#),
        "Source should contain the erroneous code from math/lib.lis"
    );
}

#[test]
fn multimodule_pipeline_in_dependency() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "utils",
        "lib.lis",
        r#"
fn double(x: int) -> int {
  x * 2
}

pub fn quadruple(x: int) -> int {
  x |> double |> double
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "utils"

fn main() {
  let _ = utils.quadruple(10)
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn multimodule_if_let_in_dependency() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "utils",
        "lib.lis",
        r#"
pub fn unwrap_or_default(opt: Option<int>) -> int {
  if let Some(x) = opt {
    x
  } else {
    0
  }
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "utils"

fn main() {
  let _ = utils.unwrap_or_default(Some(42))
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn entry_module_enum_qualified_variant_pattern() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
enum Color {
  Red,
  Green,
  Blue,
}

fn color_to_int(c: Color) -> int {
  match c {
    Color.Red => 1,
    Color.Green => 2,
    Color.Blue => 3,
  }
}

fn main() {
  let _ = color_to_int(Color.Red)
}
"#,
    );

    let result = compile_check(fs);

    assert!(
        result.errors.is_empty(),
        "Expected no errors when matching enum variants with qualified names, got: {:?}",
        result.errors
    );
}

#[test]
fn pattern_analysis_runs_on_dependency_modules() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "utils",
        "lib.lis",
        r#"
pub enum Status {
  Active,
  Inactive,
  Pending,
}

pub fn status_code(s: Status) -> int {
  match s {
    Status.Active => 1,
    Status.Inactive => 2,
    // Missing: Pending - should trigger non-exhaustive error
  }
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "utils"

fn main() {
  let _ = utils.status_code(utils.Status.Active)
}
"#,
    );

    let result = compile_check(fs);

    let has_exhaustiveness_error = result
        .errors
        .iter()
        .any(|e| e.to_string().to_lowercase().contains("not exhaustive"));

    assert!(
        has_exhaustiveness_error,
        "Expected non-exhaustive pattern error, got: {:?}",
        result.errors
    );
}

#[test]
fn linting_runs_on_dependency_modules() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "utils",
        "lib.lis",
        r#"
pub fn compute() -> int {
  let unused_var = 42;  // Should trigger unused variable warning
  100
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "utils"

fn main() {
  let _ = utils.compute()
}
"#,
    );

    let result = compile_check(fs);

    assert!(
        result.errors.is_empty(),
        "Expected no errors, got: {:?}",
        result.errors
    );

    let has_unused_warning = result
        .lints
        .iter()
        .any(|l| l.to_string().to_lowercase().contains("unused"));

    assert!(
        has_unused_warning,
        "Expected unused variable warning in dependency module, got lints: {:?}",
        result.lints
    );
}

#[test]
fn no_duplicate_fact_lints_in_multifile_module() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "utils",
        "lib.lis",
        r#"
pub fn compute() -> int {
  let unused_var = 42;
  100
}
"#,
    );

    fs.add_file(
        "utils",
        "helpers.lis",
        r#"
pub fn helper() -> int { 1 }
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "utils"

fn main() {
  let _ = utils.compute() + utils.helper()
}
"#,
    );

    let result = compile_check(fs);

    assert!(
        result.errors.is_empty(),
        "Expected no errors, got: {:?}",
        result.errors
    );

    let unused_warnings: Vec<_> = result
        .lints
        .iter()
        .filter(|l| l.to_string().to_lowercase().contains("unused"))
        .collect();

    assert_eq!(
        unused_warnings.len(),
        1,
        "Expected exactly 1 unused variable warning, got {}: {:?}",
        unused_warnings.len(),
        unused_warnings
    );
}

#[test]
fn unused_variables_prefixed_in_go_output() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
fn process(unused_param: int, used_param: int) -> int {
  let unused_var = 42;
  let used_var = used_param * 2;
  used_var
}

fn main() {
  let _ = process(1, 2)
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn multimodule_enum_constructors_not_leaked() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "utils",
        "lib.lis",
        r#"
pub fn add(a: int, b: int) -> int {
  a + b
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "utils"

enum Color {
  Red,
  Green,
}

fn main() {
  let x = utils.add(1, 2);
  let c = Color.Red;
  let _ = x
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn multimodule_cross_module_enum_usage() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "shapes",
        "lib.lis",
        r#"
pub struct Circle {
  pub radius: float64,
}

pub enum ShapeKind {
  CircleKind(Circle),
  RectKind { width: float64, height: float64 },
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "shapes"

fn describe(s: shapes.ShapeKind) -> float64 {
  match s {
    shapes.ShapeKind.CircleKind(c) => c.radius,
    shapes.ShapeKind.RectKind { width, height } => width * height,
  }
}

fn main() {
  let circle = shapes.Circle { radius: 5.0 };
  let shape = shapes.ShapeKind.CircleKind(circle);
  let _ = describe(shape)
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn multimodule_intra_module_function_call() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "math_utils",
        "lib.lis",
        r#"
pub fn square(x: float64) -> float64 {
  x * x
}

pub fn double_square(x: float64) -> float64 {
  square(x) * 2.0
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "math_utils"

fn main() {
  let _ = math_utils.double_square(3.0)
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn multimodule_static_method_call() {
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
  pub fn new(x: int, y: int) -> Point {
    Point { x: x, y: y }
  }

  pub fn squared_distance(self: Point) -> int {
    self.x * self.x + self.y * self.y
  }
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "shapes"

fn main() {
  let p = shapes.Point.new(3, 4);
  let _ = p.squared_distance()
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn static_method_name_casing_consistency() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "api",
        "lib.lis",
        r#"
pub struct Service {
  pub name: string,
}

impl Service {
  pub fn new(name: string) -> Service {
    Service { name: name }
  }
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "api"

struct Helper {
  value: int,
}

impl Helper {
  fn new(v: int) -> Helper {
    Helper { value: v }
  }
}

fn main() {
  let svc = api.Service.new("test")
  let h = Helper.new(42)
  let _ = svc.name
  let _ = h.value
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn multimodule_import_between_local_modules() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "mymath",
        "lib.lis",
        r#"
pub fn abs(n: int) -> int {
  if n < 0 { -n } else { n }
}
"#,
    );

    fs.add_file(
        "shapes",
        "lib.lis",
        r#"
import "mymath"

pub struct Point {
  pub x: int,
  pub y: int,
}

impl Point {
  pub fn manhattan_distance(self: Point, other: Point) -> int {
    mymath.abs(self.x - other.x) + mymath.abs(self.y - other.y)
  }
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "shapes"

fn main() {
  let p1 = shapes.Point { x: 3, y: 4 };
  let p2 = shapes.Point { x: 0, y: 0 };
  let _ = p1.manhattan_distance(p2)
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn multimodule_deep_nested_path() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "nested/deep/module",
        "mod.lis",
        r#"
pub fn foo() -> int {
  42
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "nested/deep/module"

fn main() {
  let _ = module.foo()
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn multimodule_type_alias_struct_literal() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "internal",
        "mod.lis",
        r#"
pub struct Secret {
  pub value: int,
}
"#,
    );

    fs.add_file(
        "api",
        "mod.lis",
        r#"
import "internal"

pub type PublicSecret = internal.Secret
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "api"

fn main() {
  let s = api.PublicSecret { value: 42 };
  let _ = s.value
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn multimodule_generic_type_alias() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "types",
        "mod.lis",
        r#"
pub struct Box<T> {
  pub value: T,
}
"#,
    );

    fs.add_file(
        "utils",
        "mod.lis",
        r#"
import "types"

pub type Container<T> = types.Box<T>
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "utils"

fn main() {
  let b = utils.Container { value: 42 };
  let _ = b.value
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn multimodule_type_alias_enum_all_variants() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "events",
        "mod.lis",
        r#"
pub enum Event {
  Click { x: int, y: int },
  KeyPress(string),
  Close,
}
"#,
    );

    fs.add_file(
        "api",
        "mod.lis",
        r#"
import "events"

pub type UIEvent = events.Event
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "api"

fn main() {
  // Struct variant through type alias
  let _ = api.UIEvent.Click { x: 10, y: 20 };
  // Tuple variant through type alias
  let _ = api.UIEvent.KeyPress("Enter");
  // Unit variant through type alias
  let _ = api.UIEvent.Close;
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn multimodule_type_alias_enum_pattern_matching() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "events",
        "mod.lis",
        r#"
pub enum Event {
  Click { x: int, y: int },
  KeyPress(string),
  Close,
}
"#,
    );

    fs.add_file(
        "api",
        "mod.lis",
        r#"
import "events"

pub type UIEvent = events.Event
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "api"

fn main() {
  let e = api.UIEvent.Click { x: 10, y: 20 };

  // Pattern matching through type alias
  match e {
    api.UIEvent.Click { x, y } => { let _ = x + y; },
    api.UIEvent.KeyPress(k) => { let _ = k; },
    api.UIEvent.Close => {},
  }
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn multimodule_enum_static_method() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "shapes",
        "mod.lis",
        r#"
pub enum Color {
  Red,
  Green,
  Blue,
}

impl Color {
  pub fn default() -> Color {
    Color.Red
  }
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "shapes"

fn main() {
  let c = shapes.Color.default();
  let _ = c;
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn receiver_name_collision_with_parameter() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "builder",
        "mod.lis",
        r#"
pub struct StringBuilder {
  pub content: string,
}

impl StringBuilder {
  pub fn append(self: StringBuilder, s: string) -> StringBuilder {
    StringBuilder { content: self.content + s }
  }
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "builder"

fn main() {
  let sb = builder.StringBuilder { content: "hello" };
  let sb2 = sb.append(" world");
  let _ = sb2;
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn go_struct_literal_none_unwrap() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:runtime/debug"
import "go:fmt"

fn main() {
  let mod_ = debug.Module {
    Path: "example.com/mod",
    Version: "v1.0.0",
    Sum: "",
    Replace: None,
  }
  fmt.Print(mod_.Path)
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn go_struct_field_assignment_unwrap() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:runtime/debug"
import "go:fmt"

fn main() {
  let replacement = debug.Module {
    Path: "example.com/replacement",
    Version: "v2.0.0",
    Sum: "",
    Replace: None,
  }
  let mut mod_ = debug.Module {
    Path: "example.com/mod",
    Version: "v1.0.0",
    Sum: "",
    Replace: None,
  }
  mod_.Replace = Some(&replacement)
  fmt.Print(mod_.Path)
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn go_map_nullable_value_unwrap_preserves_none_keys() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:go/ast"
import "go:fmt"

fn main() {
  let obj = ast.Object {
    Kind: ast.Bad,
    Name: "x",
    Decl: None,
    Data: None,
    Type: None,
  }
  let mut objects = Map.new<string, Option<Ref<ast.Object>>>()
  objects["present"] = Some(&obj)
  objects["absent"] = None
  let scope = ast.Scope {
    Outer: None,
    Objects: objects,
  }
  fmt.Print(scope.Objects)
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn same_module_cross_file_method_casing() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "shapes",
        "types.lis",
        r#"
pub struct Point {
  pub x: float64,
  pub y: float64,
}

impl Point {
  pub fn new(x: float64, y: float64) -> Point {
    Point { x: x, y: y }
  }
}

pub struct Builder {
  pub x: float64,
  pub y: float64,
}

impl Builder {
  pub fn new() -> Builder {
    Builder { x: 0.0, y: 0.0 }
  }

  pub fn with_x(self: Builder, x: float64) -> Builder {
    Builder { x: x, y: self.y }
  }
}
"#,
    );

    fs.add_file(
        "shapes",
        "use_types.lis",
        r#"
pub fn test_local_static() -> float64 {
  let p = Point.new(1.0, 2.0);
  let b = Builder.new().with_x(5.0);
  p.x + b.x
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "shapes"

fn main() {
  let _ = shapes.test_local_static()
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn cross_module_static_method_call() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "geometry",
        "lib.lis",
        r#"
pub struct Point {
  pub x: float64,
  pub y: float64,
}

impl Point {
  pub fn new(x: float64, y: float64) -> Point {
    Point { x: x, y: y }
  }

  pub fn translate(self: Point, dx: float64, dy: float64) -> Point {
    Point { x: self.x + dx, y: self.y + dy }
  }
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "geometry"

fn main() {
  let p = geometry.Point.new(3.0, 4.0);
  let p2 = p.translate(1.0, 1.0);
  let _ = p2.x
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn go_function_value_result_wrapping() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:fmt"
import "go:strconv"

fn main() {
  let parse = strconv.Atoi
  match parse("42") {
    Ok(n) => fmt.Printf("parsed: %d\n", n),
    Err(e) => {
      let msg = e.Error()
      fmt.Println(msg)
    },
  }
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn go_cross_package_type_alias_imports() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:fmt"
import "go:os"

fn main() {
  let result = os.Stat("/tmp")
  match result {
    Ok(info) => {
      let size = info.Size()
      fmt.Printf("size: %d\n", size)
    },
    Err(e) => {
      let msg = e.Error()
      fmt.Printf("error: %s\n", msg)
    },
  }
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn assert_type_emits_concrete_type_arg() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "store",
        "store.d.lis",
        r#"
fn get_value(key: string) -> Unknown
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "store"
import "go:fmt"

fn main() {
  let raw = store.get_value("count")
  match assert_type<int>(raw) {
    Some(n) => fmt.Print(n),
    None => fmt.Print("not an int"),
  }
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn go_type_same_name_as_prelude_uses_go_methods() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:fmt"
import "go:sync"

fn main() {
  let mut m = sync.Map{}
  m.Store("key", "value")
  match m.Load("key") {
    Some(v) => fmt.Println(f"got: {v}"),
    None => fmt.Println("not found"),
  }
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn cross_module_type_in_function_signature() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "models",
        "mod.lis",
        r#"
pub struct Item {
  pub name: string,
  pub value: int,
}
"#,
    );

    fs.add_file(
        "logic",
        "mod.lis",
        r#"
import "models"

pub fn process(item: models.Item) -> string {
  f"{item.name}: {item.value}"
}

pub fn create_item(name: string, value: int) -> models.Item {
  models.Item { name, value }
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:fmt"
import "models"
import "logic"

fn main() {
  let item = models.Item { name: "test", value: 42 }
  fmt.Println(logic.process(item))

  let created = logic.create_item("created", 100)
  fmt.Println(f"{created.name}: {created.value}")
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn cross_module_interface_method_call() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "models",
        "models.lis",
        r#"
pub interface Showable {
  fn show(self) -> string
}

pub struct Item {
  pub name: string,
}

impl Item {
  pub fn show(self) -> string { self.name }
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:fmt"
import "models"

fn display(item: models.Item) {
  fmt.Println(item.show())
}

fn main() {}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn same_module_pub_interface_method_call() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:fmt"

pub interface Shape {
  fn area(self) -> int
}

fn total_area(s: Shape) -> int {
  s.area()
}

fn main() {
  fmt.Println("test")
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn nested_module_type_qualifier() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "core/types",
        "types.lis",
        r#"
pub struct Item {
  pub name: string,
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:fmt"
import "core/types"

fn main() {
  let item = types.Item { name: "test" }
  fmt.Println(item.name)
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn cross_module_nested_generic_static_method() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "container",
        "container.lis",
        r#"
pub struct Box<T> { pub item: T }

impl<T> Box<T> {
  pub fn new(item: T) -> Box<T> { Box { item: item } }
  pub fn get(self) -> T { self.item }
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:fmt"
import "container"

fn main() {
  let nested = container.Box.new(container.Box.new(99))
  fmt.Println(nested.get().get())
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn go_three_value_return_option_tuple() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:strings"
import "go:fmt"

fn main() {
  let result = strings.Cut("hello-world", "-")
  fmt.Println(result)
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn cross_module_enum_variant_non_t_payload() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "data",
        "data.lis",
        r#"
pub enum Result2<T> {
  Success(T),
  Failure(string),
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:fmt"
import "data"

fn process(ok: bool) -> data.Result2<string> {
  if ok {
    data.Result2.Success("done")
  } else {
    data.Result2.Failure("failed")
  }
}

fn main() {
  fmt.Println(process(true))
  fmt.Println(process(false))
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn pub_interface_method_accessible_cross_module() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "types",
        "lib.lis",
        r#"
pub interface Greetable {
  fn greet(self) -> string
}

pub struct Person {
  pub name: string,
}

impl Person {
  pub fn greet(self) -> string {
    f"Hello, I'm {self.name}"
  }
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "types"

fn greet_anyone(g: types.Greetable) -> string {
  g.greet()
}

fn main() {
  let p = types.Person { name: "Alice" }
  let _ = greet_anyone(p)
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn import_alias_local_module() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "utils",
        "lib.lis",
        r#"
pub fn add(a: int, b: int) -> int {
  a + b
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import u "utils"

fn main() {
  let _ = u.add(1, 2)
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn multiple_json_attributes_merge() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:encoding/json"

#[json(camel_case)]
#[json(omitempty)]
struct User {
  pub first_name: string,
  pub middle_name: string,
}

fn main() {
  let u = User { first_name: "Alice", middle_name: "" }
  json.Marshal(u)
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn bare_error_return_wrapped_as_result() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:encoding/json"

#[json]
pub struct Data {
  pub value: int,
}

fn main() {
  let bytes = "{}" as Slice<uint8>
  let mut d = Data { value: 0 }
  match json.Unmarshal(bytes, &d) {
    Ok(_) => {},
    Err(e) => {},
  }
}
"#,
    );

    let result = compile_check(fs);
    assert!(
        result.errors.is_empty(),
        "Expected no errors, got: {:?}",
        result.errors
    );
}

#[test]
fn covariant_generics_in_slice_rejected() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
interface Describable {
  fn describe(self) -> string
}

struct Dog { name: string }

impl Dog {
  fn describe(self) -> string { self.name }
}

struct Box<T> {
  value: T,
  label: string,
}

fn main() {
  let boxes: Slice<Box<Describable>> = [
    Box { value: Dog { name: "A" }, label: "first" },
  ]
}
"#,
    );

    let result = compile_check(fs);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code_str() == Some("infer.type_mismatch")),
        "Expected type_mismatch error for covariant generics, got: {:?}",
        result.errors
    );
}

#[test]
fn nested_submodule_type_reference() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "lib/sub",
        "mod.lis",
        r#"
pub struct Item {
  pub name: string,
  pub score: int,
}
"#,
    );

    fs.add_file(
        "lib",
        "mod.lis",
        r#"
import "lib/sub"

pub struct Container {
  pub items: Slice<sub.Item>,
}

pub fn first_item(c: Container) -> sub.Item {
  c.items[0]
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:fmt"
import "lib"
import "lib/sub"

fn main() {
  let item = sub.Item { name: "test", score: 42 }
  let c = lib.Container { items: [item] }
  fmt.Println(lib.first_item(c).name)
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn cross_module_generic_return_only_string_vs_int() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "lib",
        "mod.lis",
        r#"
pub enum Validated<T> { Valid(T), Invalid(string) }

pub struct ValidationResult<T> {
  pub value: Validated<T>,
  pub field_name: string,
}

impl<T> ValidationResult<T> {
  pub fn new_invalid(field: string, msg: string) -> ValidationResult<T> {
    ValidationResult { value: Validated.Invalid(msg), field_name: field }
  }
}

pub fn validate_positive(field: string, val: int) -> ValidationResult<int> {
  ValidationResult.new_invalid(field, "must be positive")
}

pub fn validate_non_empty(field: string, val: string) -> ValidationResult<string> {
  ValidationResult.new_invalid(field, "must not be empty")
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:fmt"
import "lib"

fn main() {
  let r1 = lib.validate_positive("age", -1)
  let r2 = lib.validate_non_empty("name", "")
  fmt.Println(r1.field_name)
  fmt.Println(r2.field_name)
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn cross_module_generic_free_function_turbofish() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "lib",
        "mod.lis",
        r#"
pub enum Result2<T, E> { Ok2(T), Err2(E) }

pub fn ok2<T, E>(value: T) -> Result2<T, E> {
  Result2.Ok2(value)
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:fmt"
import "lib"

fn main() {
  let r = lib.ok2<int, string>(42)
  match r {
    lib.Result2.Ok2(v) => fmt.Println(v),
    lib.Result2.Err2(e) => fmt.Println(e),
  }
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn cross_module_interface_impl_in_main() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "lib",
        "mod.lis",
        r#"
pub interface Printable {
  fn display(self) -> string
}

pub fn print_all<T: Printable>(items: Slice<T>) -> string {
  items[0].display()
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:fmt"
import "lib"

struct Name { value: string }
impl Name {
  fn display(self) -> string { self.value }
}

fn main() {
  let names = [Name { value: "alice" }]
  fmt.Println(lib.print_all(names))
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn cross_module_generic_static_method_turbofish() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "lib",
        "mod.lis",
        r#"
pub struct Box<T> { pub val: T }

impl<T> Box<T> {
  pub fn new(v: T) -> Box<T> { Box { val: v } }
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:fmt"
import "lib"

fn main() {
  let b = lib.Box.new<int>(42)
  fmt.Println(b.val)
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn shadowing_prelude_types_is_forbidden() {
    for (type_name, definition) in [
        ("Ref", "pub struct Ref { pub name: string }"),
        ("Map", "pub struct Map { pub items: Slice<int> }"),
        ("Slice", "pub struct Slice { pub data: string }"),
        ("Option", "pub enum Option { Some(int), None }"),
        ("Result", "pub enum Result { Ok(int), Err(string) }"),
    ] {
        let mut fs = MockFileSystem::new();
        fs.add_file("lib", "lib.lis", definition);
        fs.add_file(ENTRY_MODULE_ID, "main.lis", "import \"lib\"\nfn main() {}");

        let result = compile_check(fs);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.code_str() == Some("infer.prelude_type_shadowed")),
            "Expected prelude shadowing error for `{}`, got: {:?}",
            type_name,
            result.errors
        );
    }
}

#[test]
fn import_alias_static_method_call() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "lib",
        "lib.lis",
        r#"
pub struct Point { pub x: int, pub y: int }
impl Point {
  pub fn new(x: int, y: int) -> Point { Point { x: x, y: y } }
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import L "lib"

fn main() {
  let p = L.Point.new(3, 4)
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn nested_module_static_method_call() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "models/user",
        "user.lis",
        r#"
pub struct User {
  pub name: string,
  pub email: string,
}

impl User {
  pub fn new(name: string, email: string) -> User {
    User { name: name, email: email }
  }
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "models/user"

fn main() {
  let u = user.User.new("Alice", "alice@test.com")
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn multiple_bounded_impl_blocks_merge_constraints() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:fmt"

pub interface Printable {
  fn print_val(self) -> string
}
pub interface Summable {
  fn sum_val(self) -> int
}

struct Container<T> {
  value: T,
  label: string
}

impl<T> Container<T> {
  fn get_label(self) -> string {
    self.label
  }
}

impl<T: Printable> Container<T> {
  fn display(self) -> string {
    f"[{self.label}] {self.value.print_val()}"
  }
}

impl<T: Summable> Container<T> {
  fn total(self) -> int {
    self.value.sum_val()
  }
}

struct Item {
  name: string,
  count: int
}

impl Item {
  fn print_val(self) -> string {
    self.name
  }
  fn sum_val(self) -> int {
    self.count
  }
}

fn main() {
  let c = Container { value: Item { name: "widget", count: 5 }, label: "box" }
  fmt.Println(c.display())
  fmt.Println(f"total: {c.total()}")
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn mixed_constrained_unconstrained_impl_blocks() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:fmt"

pub interface Printable {
  fn to_string(self) -> string
}

struct Box<T> {
  value: T,
}

impl<T: Printable> Box<T> {
  fn print(self) {
    fmt.Println(self.value.to_string())
  }
}

impl<T> Box<T> {
  fn get(self) -> T {
    self.value
  }
}

struct Name {
  name: string
}

impl Name {
  fn to_string(self) -> string {
    self.name
  }
}

fn main() {
  let b = Box { value: Name { name: "Alice" } }
  b.print()
  let b2 = Box { value: 42 }
  fmt.Println(f"int box: {b2.get()}")
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn cross_module_bounded_impl_tracks_imports() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "ifaces",
        "lib.lis",
        r#"
pub interface Showable {
  fn show(self) -> string
}
"#,
    );

    fs.add_file(
        "containers",
        "lib.lis",
        r#"
import "ifaces"

pub struct Box<T> {
  pub value: T,
}

impl<T: ifaces.Showable> Box<T> {
  pub fn display(self) -> string {
    f"Box({self.value.show()})"
  }
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:fmt"
import "containers"

struct Item {
  name: string
}

impl Item {
  fn show(self) -> string {
    self.name
  }
}

fn main() {
  let b = containers.Box { value: Item { name: "hello" } }
  fmt.Println(b.display())
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn nested_generics_with_bounded_impls() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "traits",
        "lib.lis",
        r#"
pub interface Showable {
  fn show(self) -> string
}
"#,
    );

    fs.add_file(
        "types",
        "lib.lis",
        r#"
import "traits"

pub struct Pair<A, B> {
  pub first: A,
  pub second: B,
}

impl<A: traits.Showable, B: traits.Showable> Pair<A, B> {
  pub fn show(self) -> string {
    f"({self.first.show()}, {self.second.show()})"
  }

  pub fn display(self) -> string {
    f"Pair{self.show()}"
  }
}

pub struct Tagged<T> {
  pub value: T,
  pub label: string,
}

impl<T: traits.Showable> Tagged<T> {
  pub fn show(self) -> string {
    f"[{self.label}] {self.value.show()}"
  }

  pub fn display(self) -> string {
    f"Tagged{self.show()}"
  }
}
"#,
    );

    fs.add_file(
        "ops",
        "lib.lis",
        r#"
import "traits"
import "types"

pub fn describe_tagged_pair<A: traits.Showable, B: traits.Showable>(tp: types.Tagged<types.Pair<A, B>>) -> string {
  f"tagged_pair: {tp.display()}"
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:fmt"
import "types"
import "ops"

struct Label {
  text: string,
}

impl Label {
  fn show(self) -> string {
    self.text
  }
}

fn main() {
  let l1 = Label { text: "x" }
  let l2 = Label { text: "y" }
  let inner_pair = types.Pair { first: l1, second: l2 }
  let tagged_pair = types.Tagged { value: inner_pair, label: "coords" }
  fmt.Println(ops.describe_tagged_pair(tagged_pair))
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn slice_map_with_different_output_type() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:fmt"

fn main() {
  let nums: Slice<int> = [1, 2, 3]
  let strs = nums.map<string>(|x: int| -> string { f"n={x}" })
  for s in strs {
    fmt.Println(s)
  }
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn interface_with_self_referential_method_covariant_rejected() {
    infer(
        r#"
interface Fluent {
  fn next(self) -> Fluent
}

struct Counter { n: int }
impl Counter {
  fn next(self) -> Counter { Counter { n: self.n + 1 } }
}

fn test() {
  let _c: Fluent = Counter { n: 0 }
}
"#,
    )
    .assert_infer_code("interface_not_implemented");
}

#[test]
fn generic_interface_embedding_type_substitution() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:fmt"

pub interface Mapper<T> {
  fn map_val(self) -> T
}

pub interface Filter {
  fn keep(self) -> bool
}

pub interface Processor<T> {
  impl Mapper<T>
  impl Filter
}

pub struct Score {
  name: string,
  val: int,
}

impl Score {
  pub fn map_val(self) -> string { self.name }
  pub fn keep(self) -> bool { self.val > 50 }
}

fn process_score(item: Score) -> string {
  if item.keep() { item.map_val() } else { "filtered" }
}

fn main() {
  let s = Score { name: "hello", val: 42 }
  fmt.Println(process_score(s))
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn match_on_unknown_type_rejected() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:context"

fn main() {
  let ctx = context.WithValue(context.Background(), "user", "Alice")
  match ctx.Value("user") {
    Some(v) => v,
    None => 0,
  }
}
"#,
    );

    let result = compile_check(fs);

    let has_unknown_error = result
        .errors
        .iter()
        .any(|e| e.to_string().contains("Unknown"));

    assert!(
        has_unknown_error,
        "Expected cannot_match_on_unknown error, got: {:?}",
        result.errors
    );
}

#[test]
fn cross_module_interface_method_casing() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "models",
        "mod.lis",
        r#"
pub struct User {
  pub name: string,
}

impl User {
  pub fn describe(self) -> string {
    self.name
  }
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:fmt"
import "models"

interface Describable {
  fn describe(self) -> string
}

fn print_it(item: Describable) {
  let _ = fmt.Println(item.describe())
}

fn main() {
  let u = models.User { name: "Alice" }
  print_it(u)
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn multi_file_module_sibling_visibility() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        ENTRY_MODULE_ID,
        "point.lis",
        r#"
struct Point {
  x: float64,
  y: float64,
}

impl Point {
  fn new(x: float64, y: float64) -> Point {
    Point { x: x, y: y }
  }
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
fn main() {
  let p = Point.new(3.0, 4.0)
  let _ = p.x + p.y
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn go_mut_param_accepted_with_let_mut() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:sort"

fn main() {
  let mut nums = [3, 1, 2];
  sort.Ints(nums)
}
"#,
    );
    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn go_mut_param_rejected_with_immutable_arg() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:sort"

fn main() {
  let nums = [3, 1, 2];
  sort.Ints(nums)
}
"#,
    );
    let result = compile_check(fs);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code_str() == Some("infer.immutable_arg_to_mut_param")),
        "Expected immutable_arg_to_mut_param error, got: {:?}",
        result.errors
    );
}

#[test]
fn go_mut_param_selective_only_dst() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:encoding/hex"

fn main() {
  let mut dst: Slice<uint8> = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
  let src: Slice<uint8> = [0xDE, 0xAD];
  let _ = hex.Encode(dst, src)
}
"#,
    );
    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn go_mut_param_not_bypassed_via_higher_order() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:sort"

fn apply(f: fn(Slice<int>), items: Slice<int>) {
  f(items)
}

fn main() {
  let items = [3, 1, 2]
  apply(sort.Ints, items)
}
"#,
    );
    let result = compile_check(fs);
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code_str() == Some("infer.type_mismatch")),
        "Expected type_mismatch error, got: {:?}",
        result.errors
    );
}

#[test]
fn unused_method_with_go_import_not_emitted() {
    let mut fs = MockFileSystem::new();
    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:fmt"
import "go:strings"

struct Name {
  first: string,
  last: string,
}

impl Name {
  fn full(self) -> string {
    strings.Join([self.first, self.last], " ")
  }
}

fn main() {
  let n = Name { first: "A", last: "B" }
  let _ = fmt.Println(n.first)
}
"#,
    );
    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn module_alias_used_in_type_references() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "models/user",
        "user.lis",
        r#"
pub struct User {
  pub name: string,
}

pub fn new(name: string) -> User {
  User { name: name }
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:fmt"
import u "models/user"

fn main() {
  let alice = u.new("Alice")
  let users: Slice<u.User> = [alice]
  fmt.Println(users)
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn multimodule_nested_path_enum_struct_literal() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "types/events",
        "mod.lis",
        r#"
pub enum Event {
  Click { x: int, y: int },
  Reset,
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:fmt"
import "types/events"

fn main() {
  let reset: events.Event = events.Event.Reset
  fmt.Println(reset)
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn cross_module_ufcs_uses_import_alias() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "lib",
        "box.lis",
        r#"
pub struct Box<T> { pub value: T }

impl<T> Box<T> {
  pub fn map<U>(self, f: fn(T) -> U) -> Box<U> {
    Box { value: f(self.value) }
  }
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import L "lib"
import "go:fmt"

fn main() {
  let b = L.Box { value: 1 }
  let c = b.map(|x| x + 1)
  fmt.Println(c.value)
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn cross_module_enum_construction_uses_import_alias() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "lib",
        "colors.lis",
        r#"
pub enum Color {
  Red,
  Blue,
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:fmt"
import L "lib"

fn main() {
  let c = L.Color.Red
  fmt.Println(c)
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn go_struct_nullable_field_raw_temp_var_no_collision() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:runtime/debug"
import "go:fmt"

fn main() {
  let mod_ = debug.Module {
    Path: "example.com/mod",
    Version: "v1.0.0",
    Sum: "",
    Replace: None,
  }
  let r = mod_.Replace
  let raw_1 = 3
  fmt.Println(r)
  fmt.Println(raw_1)
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn go_option_unwrap_temp_var_no_collision() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "go:net/http"
import "go:strings"
import "go:fmt"

fn main() {
  let body = strings.NewReader("hello")
  let req = http.NewRequest("POST", "https://example.com", Some(body))
  let unwrap_2 = 7
  let _ = unwrap_2
  fmt.Println(req)
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn cross_module_generic_function_value_instantiated() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "lib",
        "lib.lis",
        r#"
pub fn id<T>(x: T) -> T {
  x
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "lib"

fn main() {
  let f = lib.id
  let result = f(42)
  let _ = result
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn cross_module_generic_static_method_value_instantiated() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "lib",
        "lib.lis",
        r#"
pub struct Box<T> {
  pub value: T,
}

impl<T> Box<T> {
  pub fn new(x: T) -> Box<T> {
    Box { value: x }
  }
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "lib"

fn main() {
  let f = lib.Box.new
  let b = f(42)
  let _ = b.value
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn cross_module_instance_method_value() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "lib",
        "lib.lis",
        r#"
pub struct Point {
  pub x: int,
  pub y: int,
}

impl Point {
  pub fn sum(self) -> int {
    self.x + self.y
  }
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "lib"

fn main() {
  let p = lib.Point { x: 1, y: 2 }
  let g = lib.Point.sum
  let val = g(p)
  let _ = val
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn cross_module_pointer_receiver_method_value() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "lib",
        "lib.lis",
        r#"
pub struct Counter {
  pub count: int,
}

impl Counter {
  pub fn increment(self: Ref<Counter>) {
    self.count = self.count + 1
  }
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "lib"

fn main() {
  let mut c = lib.Counter { count: 0 }
  let f = lib.Counter.increment
  f(&c)
  let _ = c.count
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn cross_module_generic_instance_method_value() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "lib",
        "lib.lis",
        r#"
pub struct Box<T> {
  pub value: T,
}

impl<T> Box<T> {
  pub fn get(self) -> T {
    self.value
  }
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "lib"

fn main() {
  let b = lib.Box { value: 42 }
  let f = lib.Box.get
  let val = f(b)
  let _ = val
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn cross_module_instance_method_value_as_callback() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "lib",
        "lib.lis",
        r#"
pub struct Point {
  pub x: int,
  pub y: int,
}

impl Point {
  pub fn sum(self) -> int {
    self.x + self.y
  }
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "lib"

fn apply(p: lib.Point, f: fn(lib.Point) -> int) -> int {
  f(p)
}

fn main() {
  let p = lib.Point { x: 3, y: 4 }
  let result = apply(p, lib.Point.sum)
  let _ = result
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn module_name_go_keyword() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "type",
        "mod.lis",
        r#"
pub fn foo() -> int { 1 }
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import t "type"

fn main() {
  let _ = t.foo()
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn module_name_go_builtin() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "print",
        "mod.lis",
        r#"
pub fn hello() -> string { "hello" }
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "print"

fn main() {
  let _ = print.hello()
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn module_name_non_identifier_chars() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "foo-bar",
        "mod.lis",
        r#"
pub fn id(x: int) -> int { x }
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import fb "foo-bar"

fn main() {
  let _ = fb.id(1)
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn cross_module_return_only_type_args_via_alias() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "lib",
        "lib.lis",
        r#"
pub fn make<T>() -> T {
  panic("nope")
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import L "lib"

fn main() {
  let x: int = L.make()
  let _ = x
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn local_alias_cross_module_enum_struct_variant_tag() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "lib",
        "lib.lis",
        r#"
pub enum Event {
  Click { x: int },
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "lib"

type Alias = lib.Event

fn main() {
  let e = Alias.Click { x: 1 }
  let _ = e
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn enum_type_alias_with_import_alias() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "lib",
        "lib.lis",
        r#"
pub enum Event {
  Click { x: int },
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import L "lib"

type Alias = L.Event

fn main() {
  let e = Alias.Click { x: 1 }
  let _ = e
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn impl_bounds_with_module_alias_import_path() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "ifaces",
        "ifaces.lis",
        r#"
pub interface Printable {
  fn print(self) -> string
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import I "ifaces"

struct Box<T> { value: T }

impl<T: I.Printable> Box<T> {
  fn show(self) -> string {
    self.value.print()
  }
}

struct Name { name: string }

impl Name {
  fn print(self) -> string { self.name }
}

fn main() {
  let b = Box { value: Name { name: "hello" } }
  let _ = b.show()
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn cross_module_type_alias_remote_static_method() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "lib",
        "mod.lis",
        r#"
pub struct Box { pub x: int }

impl Box {
  pub fn new(x: int) -> Box { Box { x: x } }
}

pub type Alias = Box
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "lib"

fn main() {
  let b = lib.Alias.new(1)
  let _ = b.x
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn cross_module_type_alias_native_type_import_dropped() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "lib",
        "mod.lis",
        r#"
pub type Alias = Slice<int>
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "lib"

fn main() {
  let s: lib.Alias = [1]
  let _ = s[0]
}
"#,
    );

    assert_build_snapshot!(fs, "github.com/user/myproject");
}

#[test]
fn standalone_check_ignores_sibling_files() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
fn main() {
  let n = 42
  let _ = n
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "other.lis",
        r#"
struct Point { x: int, y: int }

fn main() {
  let p = Point { x: 1, y: 2 }
  let _ = p
}
"#,
    );

    let result = compile_check_standalone(fs);
    assert!(
        result.errors.is_empty(),
        "Expected no errors in standalone check, got: {:?}",
        result.errors
    );
}

#[test]
fn self_import_cycle_with_match_reports_cycle_error() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "mod",
        "mod.lis",
        r#"
import "mod"

enum Enum {
  Variant,
}

fn foo(e: Enum) {
  match e {
    Enum.Variant => {},
  }
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "mod"
"#,
    );

    let result = compile_check(fs);

    let has_cycle_error = result
        .errors
        .iter()
        .any(|e| e.to_string().contains("Import cycle"));

    assert!(
        has_cycle_error,
        "Expected import cycle error, got: {:?}",
        result.errors
    );
}

#[test]
fn cross_module_type_alias_as_qualifier() {
    let mut fs = MockFileSystem::new();

    fs.add_file(
        "types",
        "types.lis",
        r#"
pub enum Color {
  Red,
  Green,
  Blue,
}
"#,
    );

    fs.add_file(
        ENTRY_MODULE_ID,
        "main.lis",
        r#"
import "types"

type C = types.Color

fn main() {
  let x = C.Red
  match x {
    C.Red => {},
    C.Green => {},
    C.Blue => {},
  }
}
"#,
    );

    let result = compile_check(fs);
    assert!(
        result.errors.is_empty(),
        "Expected no errors, got: {:?}",
        result.errors
    );
}
