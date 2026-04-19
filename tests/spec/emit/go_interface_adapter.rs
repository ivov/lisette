use crate::assert_emit_snapshot_with_go_typedefs;

// Issue #90: a Lisette concrete whose `Read` returns `Partial<int, error>`
// must be wrappable as `io.Reader`. The adapter unpacks Partial into
// Go's native `(int, error)`.
#[test]
fn partial_return_is_wrapped_when_used_as_go_interface() {
    let input = r#"
import "go:io"

struct Doubler {}

impl Doubler {
  fn Read(self, mut p: Slice<uint8>) -> Partial<int, error> {
    Partial.Ok(0)
  }
}

fn consume(r: io.Reader) {
  let _ = r
}

fn main() {
  let d = Doubler {}
  consume(d as io.Reader)
}
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[]);
}

// A Lisette struct satisfying a Go interface whose methods use bare Go
// shapes (no Result/Partial/Option/tuple) must NOT trigger the adapter
// pass. Regression guard for the pass-through path.
#[test]
fn pure_signature_interface_skips_adapter() {
    let input = r#"
import "go:example.com/simple"

struct Greeter {}

impl Greeter {
  fn Greet(self, name: string) -> string {
    name
  }
}

fn call(g: simple.Greeter) -> string {
  g.Greet("world")
}

fn main() {
  let g = Greeter {}
  let _ = call(g as simple.Greeter)
}
"#;
    let typedef = r#"
pub interface Greeter {
  fn Greet(name: string) -> string
}
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[("go:example.com/simple", typedef)]);
}

// Mixed adaptation: one method's return is adapted, another is bare.
// The adapter forwards both; only the adapted method runs the shim.
#[test]
fn mixed_adapted_and_bare_methods() {
    let input = r#"
import "go:example.com/mixed"

struct Svc {}

impl Svc {
  fn Load(self, key: string) -> Result<int, error> {
    Ok(1)
  }
  fn Name(self) -> string {
    "svc"
  }
}

fn run(s: mixed.Service) -> string {
  s.Name()
}

fn main() {
  let s = Svc {}
  let _ = run(s as mixed.Service)
}
"#;
    let typedef = r#"
pub interface Service {
  fn Load(key: string) -> Result<int, error>
  fn Name() -> string
}
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[("go:example.com/mixed", typedef)]);
}

// Two cast sites that refer to the same (concrete, interface) pair
// share a single synthesized adapter type declaration.
#[test]
fn adapter_type_is_deduplicated_across_cast_sites() {
    let input = r#"
import "go:io"

struct Doubler {}

impl Doubler {
  fn Read(self, mut p: Slice<uint8>) -> Partial<int, error> {
    Partial.Ok(0)
  }
}

fn consume_a(r: io.Reader) {
  let _ = r
}

fn consume_b(r: io.Reader) {
  let _ = r
}

fn main() {
  let d = Doubler {}
  consume_a(d as io.Reader)
  consume_b(d as io.Reader)
}
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[]);
}

// Coverage for scenario 3: function argument. The concrete flows
// through a call arg position without an explicit `as` cast.
#[test]
fn implicit_coercion_in_function_argument() {
    let input = r#"
import "go:io"

struct Doubler {}

impl Doubler {
  fn Read(self, mut p: Slice<uint8>) -> Partial<int, error> {
    Partial.Ok(0)
  }
}

fn consume(r: io.Reader) {
  let _ = r
}

fn main() {
  let d = Doubler {}
  consume(d)
}
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[]);
}

// Coverage for scenario 4: return value. The function returns a
// concrete in a slot typed as the Go interface.
#[test]
fn coercion_in_tail_return_position() {
    let input = r#"
import "go:io"

struct Doubler {}

impl Doubler {
  fn Read(self, mut p: Slice<uint8>) -> Partial<int, error> {
    Partial.Ok(0)
  }
}

fn make() -> io.Reader {
  let d = Doubler {}
  d
}

fn main() {
  let _ = make()
}
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[]);
}

// Coverage for scenario 2: typed let binding. A concrete is assigned
// to a local declared as the Go interface type.
#[test]
fn coercion_in_typed_let_binding() {
    let input = r#"
import "go:io"

struct Doubler {}

impl Doubler {
  fn Read(self, mut p: Slice<uint8>) -> Partial<int, error> {
    Partial.Ok(0)
  }
}

fn main() {
  let d = Doubler {}
  let r: io.Reader = d
  let _ = r
}
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[]);
}

// Coverage for scenario 6: struct literal field.
#[test]
fn coercion_in_struct_literal_field() {
    let input = r#"
import "go:io"

struct Doubler {}

impl Doubler {
  fn Read(self, mut p: Slice<uint8>) -> Partial<int, error> {
    Partial.Ok(0)
  }
}

struct Wrapper {
  reader: io.Reader,
}

fn main() {
  let d = Doubler {}
  let w = Wrapper { reader: d }
  let _ = w
}
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[]);
}

// Coverage for scenario 8: map value. Lisette has no map literal; maps are
// built via `Map.new()` + indexed assignment, which routes through the
// assignment hook already covered by `assignments.rs`.
#[test]
fn coercion_in_map_value_via_indexed_assignment() {
    let input = r#"
import "go:io"

struct Doubler {}

impl Doubler {
  fn Read(self, mut p: Slice<uint8>) -> Partial<int, error> {
    Partial.Ok(0)
  }
}

fn main() {
  let mut m = Map.new<string, io.Reader>()
  let d = Doubler {}
  m["src"] = d
  let _ = m
}
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[]);
}

// Coverage for scenario 9, Position::Assign variant: match is stored in
// a typed let binding. Arm values must be wrapped as the match flows
// through `emit_block_to_var_with_braces`.
#[test]
fn coercion_in_match_arm_via_typed_let() {
    let input = r#"
import "go:io"

struct A {}
struct B {}

impl A {
  fn Read(self, mut p: Slice<uint8>) -> Partial<int, error> {
    Partial.Ok(0)
  }
}

impl B {
  fn Read(self, mut p: Slice<uint8>) -> Partial<int, error> {
    Partial.Ok(0)
  }
}

fn main() {
  let flag = true
  let r: io.Reader = match flag {
    true => A {},
    false => B {},
  }
  let _ = r
}
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[]);
}

// Coverage for scenario 9: match arm value. Each arm produces a
// concrete and the match's result type is a Go interface.
#[test]
fn coercion_in_match_arm_value() {
    let input = r#"
import "go:io"

struct A {}
struct B {}

impl A {
  fn Read(self, mut p: Slice<uint8>) -> Partial<int, error> {
    Partial.Ok(0)
  }
}

impl B {
  fn Read(self, mut p: Slice<uint8>) -> Partial<int, error> {
    Partial.Ok(0)
  }
}

fn pick(flag: bool) -> io.Reader {
  match flag {
    true => A {},
    false => B {},
  }
}

fn main() {
  let _ = pick(true)
}
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[]);
}

// Regression: an interface that embeds another interface must expose
// its inherited methods in the synthesized adapter. A Lisette struct
// satisfying `ReadWriteCloser` (which embeds `Reader` and `Writer`)
// must have the adapter emit `Read` and `Write` even though
// `ReadWriteCloser.methods` is empty — inherited methods live under
// `parents`.
#[test]
fn adapter_includes_methods_inherited_from_parent_interfaces() {
    let input = r#"
import "go:example.com/rw"

struct Dev {}

impl Dev {
  fn Read(self, mut p: Slice<uint8>) -> Partial<int, error> {
    Partial.Ok(0)
  }
  fn Write(self, p: Slice<uint8>) -> Partial<int, error> {
    Partial.Ok(0)
  }
}

fn use_rw(rw: rw.ReadWriter) {
  let _ = rw
}

fn main() {
  let d = Dev {}
  use_rw(d as rw.ReadWriter)
}
"#;
    let typedef = r#"
pub interface Reader {
  fn Read(mut p: Slice<uint8>) -> Partial<int, error>
}

pub interface Writer {
  fn Write(p: Slice<uint8>) -> Partial<int, error>
}

pub interface ReadWriter {
  impl Reader
  impl Writer
}
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[("go:example.com/rw", typedef)]);
}

// Regression: Option<Ref<T>> and Option<Interface> are nullable Go
// shapes. A Lisette impl of a Go interface method returning such an
// Option must emit the adapter's Go method with the bare nilable
// pointer/interface and unwrap `Some(v) -> v, None -> nil`. The old
// behavior emitted `(T, bool)` which does not satisfy the Go
// interface.
#[test]
fn option_ref_in_interface_method_adapts_to_bare_nilable_pointer() {
    let input = r#"
import "go:example.com/store"

struct Store {}

impl Store {
  fn Find(self, key: string) -> Option<Ref<store.Entry>> {
    None
  }
}

fn use_store(s: store.Storage) {
  let _ = s
}

fn main() {
  let s = Store {}
  use_store(s as store.Storage)
}
"#;
    let typedef = r#"
pub struct Entry { pub Name: string }

pub interface Storage {
  fn Find(key: string) -> Option<Ref<Entry>>
}
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[("go:example.com/store", typedef)]);
}

// Regression for tea.Cmd alias collapse. A Go function-type alias
// (`type Cmd func() Msg`) used inside an `Option<Cmd>` in return
// position must preserve the Cmd name end-to-end — otherwise Go's
// strict generic inference instantiates `Option[func() Msg]` and
// rejects it where `Option[Cmd]` is expected. The fix required:
// (1) `infer_tuple` resolving expected_ty through type variables,
// (2) `is_generic_container_with_interface` firing for go:-prefixed
// params, and (3) `resolve_call_type_args` emitting explicit type
// args for Go function aliases on Some/Ok/Err.
#[test]
fn go_named_function_alias_preserved_through_option_tuple_return() {
    let input = r#"
import "go:example.com/tea"

struct Model {}

impl Model {
  fn Init(self) -> Option<tea.Cmd> {
    None
  }
  fn Update(self, msg: tea.Msg) -> (tea.Model, Option<tea.Cmd>) {
    (self as tea.Model, Some(tea.Quit))
  }
  fn View(self) -> string {
    ""
  }
}

fn main() {
  let _ = tea.NewProgram(Model {} as tea.Model)
}
"#;
    let typedef = r#"// Package: tea

pub interface Msg {}

pub type Cmd = fn() -> Msg

pub interface Model {
  fn Init() -> Option<Cmd>
  fn Update(arg0: Msg) -> (Model, Option<Cmd>)
  fn View() -> string
}

pub type Program

pub fn NewProgram(model: Model) -> Ref<Program>

pub fn Quit() -> Msg
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[("go:example.com/tea", typedef)]);
}

#[test]
fn tuple_interface_slot_implicit_coercion_tail_position() {
    let input = r#"
import "go:example.com/tea"

struct Model {}

impl Model {
  fn Init(self) -> Option<tea.Cmd> {
    None
  }
  fn Update(self, msg: tea.Msg) -> (tea.Model, Option<tea.Cmd>) {
    (self, Some(tea.Quit))
  }
  fn View(self) -> string {
    ""
  }
}

fn main() {
  let _ = tea.NewProgram(Model {} as tea.Model)
}
"#;
    let typedef = r#"// Package: tea

pub interface Msg {}

pub type Cmd = fn() -> Msg

pub interface Model {
  fn Init() -> Option<Cmd>
  fn Update(arg0: Msg) -> (Model, Option<Cmd>)
  fn View() -> string
}

pub type Program

pub fn NewProgram(model: Model) -> Ref<Program>

pub fn Quit() -> Msg
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[("go:example.com/tea", typedef)]);
}

#[test]
fn tuple_interface_slot_implicit_coercion_assign_position() {
    let input = r#"
import "go:example.com/tea"

struct Model {}

impl Model {
  fn Init(self) -> Option<tea.Cmd> {
    None
  }
  fn Update(self, msg: tea.Msg) -> (tea.Model, Option<tea.Cmd>) {
    let result = match msg {
      _ => (self, Some(tea.Quit)),
    }
    result
  }
  fn View(self) -> string {
    ""
  }
}

fn main() {
  let _ = tea.NewProgram(Model {} as tea.Model)
}
"#;
    let typedef = r#"// Package: tea

pub interface Msg {}

pub type Cmd = fn() -> Msg

pub interface Model {
  fn Init() -> Option<Cmd>
  fn Update(arg0: Msg) -> (Model, Option<Cmd>)
  fn View() -> string
}

pub type Program

pub fn NewProgram(model: Model) -> Ref<Program>

pub fn Quit() -> Msg
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[("go:example.com/tea", typedef)]);
}

// Coverage for scenario 11: interface-to-interface. A Go interface value
// assigned to another Go interface uses Go's structural conversion; no
// adapter is synthesized (the `needs_adapter` source-side guard early-
// returns when source is already Go-imported).
#[test]
fn interface_to_interface_does_not_synthesize_adapter() {
    let input = r#"
import "go:io"

fn narrow(rwc: io.ReadWriteCloser) -> io.Reader {
  rwc
}

fn main() {
  let _ = narrow
}
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[]);
}
