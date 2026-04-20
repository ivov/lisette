use crate::assert_emit_snapshot;
use crate::assert_emit_snapshot_with_go_typedefs;

#[test]
fn interop_result_direct_call() {
    let input = r#"
import "go:strconv"

fn main() {
  let r = strconv.Atoi("42")
  match r {
    Ok(v) => { let _ = v },
    Err(_) => { let _ = 0 },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_result_let_call() {
    let input = r#"
import "go:strconv"

fn main() {
  let f = strconv.Atoi
  let r = f("42")
  match r {
    Ok(v) => { let _ = v },
    Err(_) => { let _ = 0 },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_result_alias_call() {
    let input = r#"
import "go:strconv"

fn main() {
  let f = strconv.Atoi
  let g = f
  let r = g("42")
  match r {
    Ok(v) => { let _ = v },
    Err(_) => { let _ = 0 },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_result_return_pos() {
    let input = r#"
import "go:strconv"

fn make_parser() -> fn(string) -> Result<int, error> {
  strconv.Atoi
}

fn main() {
  let f = make_parser()
  let r = f("42")
  match r {
    Ok(v) => { let _ = v },
    Err(_) => { let _ = 0 },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_result_assignment() {
    let input = r#"
import "go:strconv"

fn fallback(s: string) -> Result<int, error> { Ok(0) }

fn main() {
  let mut f = fallback
  f = strconv.Atoi
  let r = f("42")
  match r {
    Ok(v) => { let _ = v },
    Err(_) => { let _ = 0 },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_result_struct_field() {
    let input = r#"
import "go:strconv"

struct Parser { parse: fn(string) -> Result<int, error> }

fn main() {
  let p = Parser { parse: strconv.Atoi }
  let r = p.parse("42")
  match r {
    Ok(v) => { let _ = v },
    Err(_) => { let _ = 0 },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_result_call_arg() {
    let input = r#"
import "go:strconv"

fn apply(f: fn(string) -> Result<int, error>, s: string) -> Result<int, error> {
  f(s)
}

fn main() {
  let r = apply(strconv.Atoi, "42")
  match r {
    Ok(v) => { let _ = v },
    Err(_) => { let _ = 0 },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_result_task_block() {
    let input = r#"
import "go:strconv"

fn main() {
  let f = strconv.Atoi
  task {
    let r = f("42")
    let _ = r
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_comma_ok_direct_call() {
    let input = r#"
import "go:os"

fn main() {
  let r = os.LookupEnv("HOME")
  match r {
    Some(v) => { let _ = v },
    None => { let _ = "" },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_comma_ok_let_call() {
    let input = r#"
import "go:os"

fn main() {
  let f = os.LookupEnv
  let r = f("HOME")
  match r {
    Some(v) => { let _ = v },
    None => { let _ = "" },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_comma_ok_call_arg() {
    let input = r#"
import "go:os"

fn apply(f: fn(string) -> Option<string>, key: string) -> string {
  match f(key) {
    Some(v) => v,
    None => "unset",
  }
}

fn main() {
  let r = apply(os.LookupEnv, "HOME")
  let _ = r
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_comma_ok_return_pos() {
    let input = r#"
import "go:os"

fn make_lookup() -> fn(string) -> Option<string> {
  os.LookupEnv
}

fn main() {
  let f = make_lookup()
  let r = f("HOME")
  match r {
    Some(v) => { let _ = v },
    None => { let _ = "" },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_nullable_direct_call() {
    let input = r#"
import "go:flag"

fn main() {
  let r = flag.Lookup("verbose")
  match r {
    Some(f) => { let _ = f },
    None => {},
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_nullable_let_call() {
    let input = r#"
import "go:flag"

fn main() {
  let f = flag.Lookup
  let r = f("verbose")
  match r {
    Some(v) => { let _ = v },
    None => {},
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_nullable_call_arg() {
    let input = r#"
import "go:flag"

fn apply(f: fn(string) -> Option<Ref<flag.Flag>>, name: string) -> bool {
  match f(name) {
    Some(_) => true,
    None => false,
  }
}

fn main() {
  let r = apply(flag.Lookup, "verbose")
  let _ = r
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_nullable_return_pos() {
    let input = r#"
import "go:flag"

fn make_lookup() -> fn(string) -> Option<Ref<flag.Flag>> {
  flag.Lookup
}

fn main() {
  let f = make_lookup()
  let r = f("verbose")
  match r {
    Some(v) => { let _ = v },
    None => {},
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_array_return_direct_call() {
    let input = r#"
import "go:crypto/sha256"

fn main() {
  let data = "hello" as Slice<uint8>
  let hash = sha256.Sum256(data)
  let _ = hash
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_array_return_let_call() {
    let input = r#"
import "go:crypto/sha256"

fn main() {
  let f = sha256.Sum256
  let data = "hello" as Slice<uint8>
  let hash = f(data)
  let _ = hash
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_tuple_direct_call() {
    let input = r#"
import "go:path"

fn main() {
  let r = path.Split("/foo/bar.txt")
  let _ = r
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_tuple_let_call() {
    let input = r#"
import "go:path"

fn main() {
  let f = path.Split
  let r = f("/foo/bar.txt")
  let _ = r
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_tuple_call_arg() {
    let input = r#"
import "go:path"

fn use_split(f: fn(string) -> (string, string), p: string) -> string {
  let (dir, file) = f(p)
  f"{dir}/{file}"
}

fn main() {
  let r = use_split(path.Split, "/foo/bar.txt")
  let _ = r
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_tuple_return_pos() {
    let input = r#"
import "go:path"

fn make_splitter() -> fn(string) -> (string, string) {
  path.Split
}

fn main() {
  let f = make_splitter()
  let r = f("/foo/bar.txt")
  let _ = r
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_result_let_mut() {
    let input = r#"
import "go:strconv"

fn main() {
  let mut f: fn(string) -> Result<int, error> = strconv.Atoi
  let r = f("42")
  let _ = r
  f = strconv.Atoi
  let r2 = f("99")
  let _ = r2
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_result_slice_element() {
    let input = r#"
import "go:strconv"

fn main() {
  let arr: Slice<fn(string) -> Result<int, error>> = [strconv.Atoi]
  let r = arr[0]("1")
  match r {
    Ok(v) => { let _ = v },
    Err(_) => { let _ = 0 },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_result_tuple_element() {
    let input = r#"
import "go:strconv"

fn main() {
  let t: (fn(string) -> Result<int, error>, int) = (strconv.Atoi, 1)
  let r = t.0("1")
  match r {
    Ok(v) => { let _ = v },
    Err(_) => { let _ = 0 },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_nullable_slice_element() {
    let input = r#"
import "go:flag"

fn main() {
  let arr: Slice<fn(string) -> Option<Ref<flag.Flag>>> = [flag.Lookup]
  let r = arr[0]("verbose")
  match r {
    Some(v) => { let _ = v },
    None => {},
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_array_return_explicit_return() {
    let input = r#"
import "go:crypto/sha256"

fn get() -> fn(Slice<uint8>) -> Slice<uint8> {
  return sha256.Sum256
}

fn main() {
  let f = get()
  let data: Slice<uint8> = []
  let out = f(data)
  let _ = out
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_array_return_parenthesized_call() {
    let input = r#"
import "go:crypto/sha256"

fn main() {
  let data: Slice<uint8> = []
  let out = (sha256.Sum256)(data)
  let out2 = out.append(1)
  let _ = out2
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_array_return_if_assignment() {
    let input = r#"
import "go:crypto/sha256"

fn main() {
  let f: fn(Slice<uint8>) -> Slice<uint8> = if true {
    sha256.Sum256
  } else {
    sha256.Sum224
  }
  let data: Slice<uint8> = []
  let out = f(data)
  let _ = out
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_array_return_if_tail() {
    let input = r#"
import "go:crypto/sha256"

fn get(cond: bool) -> fn(Slice<uint8>) -> Slice<uint8> {
  if cond {
    sha256.Sum256
  } else {
    sha256.Sum224
  }
}

fn main() {
  let f = get(true)
  let data: Slice<uint8> = []
  let out = f(data)
  let _ = out
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_array_return_call_arg() {
    let input = r#"
import "go:crypto/sha256"

fn apply(f: fn(Slice<uint8>) -> Slice<uint8>, data: Slice<uint8>) -> Slice<uint8> {
  f(data)
}

fn main() {
  let data: Slice<uint8> = []
  let out = apply(sha256.Sum256, data)
  let _ = out
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_result_map_field_assignment() {
    let input = r#"
import "go:strconv"

struct Holder { f: fn(string) -> Result<int, error> }

fn main() {
  let mut m = Map.new<string, Holder>()
  m["a"] = Holder { f: strconv.Atoi }
  let mut entry = m["a"]
  entry.f = strconv.Atoi
  m["a"] = entry
  let r = m["a"].f("1")
  match r {
    Ok(v) => { let _ = v },
    Err(_) => { let _ = 0 },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_result_ok_constructor() {
    let input = r#"
import "go:strconv"

fn make() -> Result<fn(string) -> Result<int, error>, error> {
  Ok(strconv.Atoi)
}

fn main() {
  let r = make()
  match r {
    Ok(f) => {
      let r2 = f("1")
      match r2 {
        Ok(v) => { let _ = v },
        Err(_) => { let _ = 0 },
      }
    },
    Err(_) => { let _ = 0 },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_result_try_block() {
    let input = r#"
import "go:strconv"

fn make() -> Result<fn(string) -> Result<int, error>, error> {
  try {
    let _ = Ok(1)?
    strconv.Atoi
  }
}

fn main() {
  let r = make()
  match r {
    Ok(f) => {
      let r2 = f("1")
      match r2 {
        Ok(v) => { let _ = v },
        Err(_) => { let _ = 0 },
      }
    },
    Err(_) => { let _ = 0 },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_result_break_value() {
    let input = r#"
import "go:strconv"

fn make() -> fn(string) -> Result<int, error> {
  loop {
    break strconv.Atoi
  }
}

fn main() {
  let f = make()
  let r = f("1")
  match r {
    Ok(v) => { let _ = v },
    Err(_) => { let _ = 0 },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_result_native_method_arg() {
    let input = r#"
import "go:strconv"

fn main() {
  let xs: Slice<string> = ["1"]
  let ys = xs.map(strconv.Atoi)
  match ys[0] {
    Ok(v) => { let _ = v },
    Err(_) => { let _ = 0 },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_result_tuple_struct_constructor() {
    let input = r#"
import "go:strconv"

type F = fn(string) -> Result<int, error>
struct Pair(F, int)

fn main() {
  let p = Pair(strconv.Atoi, 1)
  let r = p.0("1")
  match r {
    Ok(v) => { let _ = v },
    Err(_) => { let _ = 0 },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_result_ufcs_call_arg() {
    let input = r#"
import "go:strconv"

struct Box {}

impl Box {
  fn apply(self, f: fn(string) -> Result<int, error>) -> Result<int, error> {
    f("1")
  }
}

fn main() {
  let b = Box {}
  let r = Box.apply(b, strconv.Atoi)
  match r {
    Ok(v) => { let _ = v },
    Err(_) => { let _ = 0 },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_array_return_defer() {
    let input = r#"
import "go:crypto/sha256"

fn main() {
  let data: Slice<uint8> = [1, 2, 3]
  defer sha256.Sum256(data)
  let _ = 0
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_array_return_statement_position() {
    let input = r#"
import "go:crypto/sha256"

fn main() {
  let data: Slice<uint8> = [1, 2, 3]
  sha256.Sum256(data)
  let _ = 0
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_result_select_send() {
    let input = r#"
import "go:strconv"

fn main() {
  let ch = Channel.new<fn(string) -> Result<int, error>>()
  select {
    ch.send(strconv.Atoi) => { let _ = 0 },
    _ => { let _ = 1 },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_result_slice_append() {
    let input = r#"
import "go:strconv"

type F = fn(string) -> Result<int, error>

fn main() {
  let mut xs: Slice<F> = []
  xs = xs.append(strconv.Atoi)
  let _ = xs
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_slice_literal_option_import() {
    let input = r#"
fn main() {
  let xs: Slice<Option<int>> = []
  let _ = xs
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_result_expression_position_assignment() {
    let input = r#"
import "go:strconv"

type F = fn(string) -> Result<int, error>

fn main() {
  let mut f: F = |s| Ok(0)
  let u = { f = strconv.Atoi }
  let _ = u
  let _ = f("1")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_value_enum_aliased_import() {
    let input = r#"
import t "go:time"

fn main() {
  let d = t.Duration.Second
  let _ = d
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_value_enum_nested_module() {
    let input = r#"
import "go:debug/dwarf"

fn main() {
  let t = dwarf.Tag.TagArrayType
  let _ = t
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_value_enum_match_arm_aliased() {
    let input = r#"
import t "go:time"

fn describe(d: t.Duration) -> string {
  match d {
    t.Duration.Second => "one second",
    t.Duration.Minute => "one minute",
    _ => "other",
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_value_enum_match_arm_nested_module() {
    let input = r#"
import "go:debug/dwarf"

fn describe(a: dwarf.Attr) -> string {
  match a {
    dwarf.Attr.AttrArtificial => "artificial",
    dwarf.Attr.AttrByteSize   => "byte size",
    _ => "other",
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_tuple_tail_concrete_in_go_interface_slot() {
    let input = r#"
import "go:fmt"

struct Counter {
  count: int,
}

impl Counter {
  fn String(self) -> string {
    f"{self.count}"
  }
}

fn make_pair(c: Counter) -> (fmt.Stringer, int) {
  (c, c.count)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_tuple_explicit_return_concrete_in_go_interface_slot() {
    let input = r#"
import "go:fmt"

struct Counter {
  count: int,
}

impl Counter {
  fn String(self) -> string {
    f"{self.count}"
  }
}

fn make_pair(c: Counter, positive: bool) -> (fmt.Stringer, int) {
  if positive {
    return (Counter { count: c.count + 1 }, c.count)
  }
  (c, c.count)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_aliased_import_type_reference() {
    let input = r#"
import t "go:time"

fn f(x: t.Time) -> t.Duration {
  t.Since(x)
}

fn main() {
  let _ = f(t.Now())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_address_of_go_function_value() {
    let input = r#"
import "go:strconv"

fn main() {
  let r = &strconv.Atoi
  let _ = r
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_address_of_method_value() {
    let input = r#"
struct S {}

impl S {
  fn inc(self) -> int { 1 }
}

fn main() {
  let s = S {}
  let r = &s.inc
  let _ = r
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_result_if_assignment() {
    let input = r#"
import "go:strconv"

fn main() {
  let f: fn(string) -> Result<int, error> = if true {
    strconv.Atoi
  } else {
    strconv.Atoi
  }
  let r = f("1")
  match r {
    Ok(v) => { let _ = v },
    Err(_) => { let _ = 0 },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_typed_nil_interface_single_return() {
    let input = r#"
import "go:context"
import "go:fmt"

fn main() {
  let ctx = context.Background()
  match ctx.Err() {
    Some(e) => fmt.Println(e.Error()),
    None => fmt.Println("no error"),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_typed_nil_interface_result_return() {
    let input = r#"
import "go:fmt"
import "go:os"

fn main() {
  let info = os.Stat("/tmp")
  match info {
    Ok(i) => fmt.Println(i.Size()),
    Err(e) => fmt.Println(e),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_typed_nil_interface_collection() {
    let input = r#"
import "go:fmt"
import "go:go/ast"
import "go:go/token"

fn main() {
  let lit = ast.CompositeLit {
    Type: None,
    Lbrace: 0 as token.Pos,
    Elts: [],
    Rbrace: 0 as token.Pos,
    Incomplete: false,
  }
  let elts = lit.Elts
  for elt in elts {
    match elt {
      Some(e) => fmt.Println(e.Pos()),
      None => fmt.Println("nil"),
    }
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_err_sentinel_pattern() {
    let input = r#"
import "go:bufio"
import "go:fmt"
import "go:io"
import "go:os"

fn main() {
  let reader = bufio.NewReader(os.Stdin)
  while true {
    let r = match reader.ReadRune() {
      Ok((r, _)) => r,
      Err(io.EOF) => break,
      Err(_) => panic("error"),
    }
    fmt.Printf("%c", r)
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn interop_nullable_function_alias_some_lambda() {
    let input = r#"
import "go:example.com/scheduler"

fn main() {
  let n = 42
  let cmd: Option<scheduler.Cmd> = Some(|| n)
  let _ = cmd
}
"#;
    let typedef = r#"
pub type Cmd = fn() -> int

pub fn MakeCmd() -> Option<Cmd>
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[("go:example.com/scheduler", typedef)]);
}

#[test]
fn interop_nullable_function_alias_direct_call() {
    let input = r#"
import "go:example.com/scheduler"

fn main() {
  let cmd = scheduler.MakeCmd()
  match cmd {
    Some(c) => { let _ = c },
    None => {},
  }
}
"#;
    let typedef = r#"
pub type Cmd = fn() -> string

pub fn MakeCmd() -> Option<Cmd>
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[("go:example.com/scheduler", typedef)]);
}

#[test]
fn interop_lambda_unit_body_against_interface_aliased_return() {
    let input = r#"
import "go:example.com/scheduler"

fn make() -> scheduler.Cmd {
  || ()
}

fn main() {
  let _ = make()
}
"#;
    let typedef = r#"
pub interface Event {}

pub type Cmd = fn() -> Event
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[("go:example.com/scheduler", typedef)]);
}

#[test]
fn interop_option_of_aliased_interface_uses_is_nil_interface() {
    let input = r#"
import "go:example.com/evts"
import "go:fmt"

fn main() {
  match evts.Peek() {
    Some(e) => fmt.Println(e),
    None => fmt.Println("none"),
  }
}
"#;
    let typedef = r#"
pub interface Event {}
pub type Msg = Event
pub fn Peek() -> Option<Msg>
"#;
    assert_emit_snapshot_with_go_typedefs!(input, &[("go:example.com/evts", typedef)]);
}
