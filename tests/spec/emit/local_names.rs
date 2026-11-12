use crate::assert_emit_snapshot;

#[test]
fn local_names_take_camel_case_in_every_binding_shape() {
    let input = r#"
import "go:fmt"
import "go:os"
import "go:strconv"

enum Wrap {
  Inner(int),
  Empty,
}

fn parse_num(raw_text: string) -> Result<int, error> {
  let parsed_value = strconv.Atoi(raw_text)?
  Ok(parsed_value)
}

fn main() {
  let item_list = [1, 2, 3]
  let mut item_total = 0
  for (item_index, item_value) in item_list.enumerate() {
    item_total += item_index * item_value
  }
  match item_list {
    [first_item, ..rest_items] => fmt.Println(first_item, rest_items),
    [] => fmt.Println("none"),
  }
  let mut stack_items = [5]
  while let Some(top_item) = stack_items.get(0) {
    fmt.Println(top_item)
    stack_items = []
  }
  match Wrap.Inner(9) {
    Wrap.Inner(inner_num) as whole_value => fmt.Println(inner_num, whole_value),
    Wrap.Empty => fmt.Println("empty"),
  }
  if let Some(first_arg) = os.Args.get(0) {
    fmt.Println(first_arg.length() > 0)
  }
  match parse_num("12") {
    Ok(good_value) => fmt.Println(good_value),
    Err(parse_err) => fmt.Println(parse_err),
  }
  let add_one = |input_value: int| input_value + 1
  if item_total != 8 || add_one(1) != 2 { panic("wrong total") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn parameters_that_share_a_go_name_take_suffixes() {
    let input = r#"
fn pick(x12: int, x_1: int, x1: int) -> int {
  x12 + x_1 * 10 + x1 * 100
}

fn main() {
  if pick(1, 2, 3) != 321 { panic("a parameter read the wrong value") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn trailing_underscore_names_take_suffixes() {
    let input = r#"
fn pick(foo_bar: int, foo_bar_: int) -> int {
  foo_bar * 10 + foo_bar_
}

fn main() {
  let foo_bar = 1
  let foo_bar_ = 2
  if pick(foo_bar, foo_bar_) != 12 { panic("a local read the wrong value") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_binding_components_take_camel_case() {
    let input = r#"
fn pair_of(seed: int) -> (int, int) {
  (seed, seed * 2)
}

fn main() {
  let user_pair = pair_of(3)
  if user_pair.0 + user_pair.1 != 9 { panic("a component read the wrong value") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn local_name_does_not_hide_private_function() {
    let input = r#"
fn load_data() -> int {
  1
}

fn main() {
  let load_data_ = 5
  if load_data_ + load_data() != 6 { panic("the local hid the function") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn chained_map_err_lambda_parameter_takes_camel_case() {
    let input = r#"
fn load() -> Result<int, string> {
  Err("bad")
}

fn wrap_error(message: string) -> string {
  "[" + message + "]"
}

fn run() -> Result<int, string> {
  let value = load().map_err(wrap_error).map_err(|error_value| error_value + error_value)?
  Ok(value)
}

fn main() {
  match run() {
    Ok(_) => panic("the load should fail"),
    Err(message) => if message != "[bad][bad]" { panic("the error was mapped wrong") },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn local_const_does_not_hide_package_function() {
    let input = r#"
pub fn max_size() -> int {
  10
}

fn main() {
  const MAX_SIZE = 1
  if MAX_SIZE + max_size() != 11 { panic("the const hid the function") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn local_consts_that_share_a_go_name_take_suffixes() {
    let input = r#"
fn main() {
  const MAX_SIZE = 1
  const MAX_SIZE_ = 2
  if MAX_SIZE * 10 + MAX_SIZE_ != 12 { panic("a const read the wrong value") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn loop_binding_does_not_hide_package_function() {
    let input = r#"
fn load_data() -> int {
  7
}

fn main() {
  let mut total = 0
  for load_data_ in 0..2 {
    total += load_data_ + load_data()
  }
  if total != 15 { panic("the loop read the wrong value") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_receive_does_not_hide_outer_local() {
    let input = r#"
fn main() {
  let user_id = 9
  let ch = Channel.new<int>()
  task {
    ch.send(4)
  }
  select {
    let Some(user_id_) = ch.receive() => {
      if user_id * 10 + user_id_ != 94 { panic("the receive read the wrong value") }
    },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_receive_match_does_not_hide_outer_local() {
    let input = r#"
fn main() {
  let user_id = 9
  let ch = Channel.new<int>()
  task {
    ch.send(4)
  }
  select {
    match ch.receive() {
      Some(user_id_) => {
        if user_id * 10 + user_id_ != 94 { panic("the receive read the wrong value") }
      },
      None => panic("the channel closed"),
    },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn let_else_binding_does_not_hide_loop_binding() {
    let input = r#"
fn item() -> Option<int> {
  Some(9)
}

fn main() {
  let mut total = 0
  for user_id in 0..2 {
    let Some(user_id_) = item() else { return }
    total += user_id * 10 + user_id_
  }
  if total != 28 { panic("the binding read the wrong value") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn tuple_siblings_that_share_a_go_name_take_suffixes() {
    let input = r#"
fn pair() -> (int, int) {
  (1, 2)
}

fn main() {
  let (user_id, user_id_) = pair()
  if user_id * 10 + user_id_ != 12 { panic("a sibling read the wrong value") }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn channel_split_siblings_that_share_a_go_name_take_suffixes() {
    let input = r#"
fn main() {
  let ch = Channel.buffered<int>(1)
  let (user_id, user_id_) = ch.split()
  user_id.send(5)
  match user_id_.receive() {
    Some(value) => {
      if value != 5 { panic("the receiver read the wrong value") }
    },
    None => panic("the channel closed"),
  }
}
"#;
    assert_emit_snapshot!(input);
}
