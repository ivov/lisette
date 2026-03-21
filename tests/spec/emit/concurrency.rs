use crate::assert_emit_snapshot;

#[test]
fn task_simple() {
    let input = r#"
import "go:fmt"

fn compute() {
  fmt.Print(f"Hello from goroutine");
}

fn test() {
  task compute()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn task_with_expression() {
    let input = r#"
import "go:fmt"

fn background() {
  fmt.Print(f"Running in background");
}

fn test() -> int {
  let x = 42;
  task background();
  x
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn task_nested() {
    let input = r#"
import "go:fmt"

fn inner() {
  fmt.Print(f"Nested task");
}

fn outer() {
  task inner()
}

fn test() {
  task outer()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn task_defer_in_loop() {
    let input = r#"
import "go:sync"

fn test() {
  let mut wg = sync.WaitGroup{};
  for _ in 0..5 {
    wg.Add(1);
    task {
      defer wg.Done()
    }
  }
  wg.Wait()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_simple() {
    let input = r#"
fn test() -> int {
  let ch1 = Channel.new<int>();
  let ch2 = Channel.new<int>();

  select {
    match ch1.receive() {
      Some(val) => val + 10,
      None => 0,
    },
    match ch2.receive() {
      Some(val) => val + 20,
      None => 0,
    },
    _ => 0,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_with_default() {
    let input = r#"
fn test() -> int {
  let ch = Channel.new<int>();

  select {
    let Some(val) = ch.receive() => val,
    _ => 0,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_multiple_arms() {
    let input = r#"
fn test() -> int {
  let ch1 = Channel.new<int>();
  let ch2 = Channel.new<int>();
  let ch3 = Channel.new<int>();

  select {
    match ch1.receive() {
      Some(val) => val,
      None => 0,
    },
    match ch2.receive() {
      Some(val) => val,
      None => 0,
    },
    match ch3.receive() {
      Some(val) => val,
      None => 0,
    },
    _ => 0,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_send() {
    let input = r#"
fn test() {
  let ch = Channel.new<int>();
  select {
    ch.send(42) => {},
    _ => {},
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_send_and_receive() {
    let input = r#"
fn test() -> int {
  let tx = Channel.new<int>();
  let rx = Channel.new<int>();

  select {
    tx.send(42) => 1,
    let Some(v) = rx.receive() => v,
    _ => 0,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_receive_discard() {
    let input = r#"
fn do_work() {}

fn test() {
  let ch = Channel.new<int>();

  select {
    ch.receive() => do_work(),
    _ => {},
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn for_loop_over_range() {
    let input = r#"
fn test() -> int {
  let xs = [1, 2, 3, 4, 5];
  let mut sum = 0;
  for x in xs {
    sum = sum + x;
  };
  sum
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn task_with_block_body() {
    let input = r#"
import "go:fmt"

fn test() {
  task {
    let x = 42;
    fmt.Print(f"Got {x}");
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn task_block_followed_by_return() {
    let input = r#"
import "go:fmt"

fn test() -> int {
  task {
    fmt.Print("background");
  };
  100
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn sender_type_in_struct() {
    let input = r#"
struct Producer { output: Sender<int> }

fn test(p: Producer) -> Sender<int> {
  p.output
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn receiver_type_in_struct() {
    let input = r#"
struct Consumer { input: Receiver<int> }

fn test(c: Consumer) -> Receiver<int> {
  c.input
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_struct_pattern() {
    let input = r#"
struct Message { id: int, data: int }

fn test(ch: Receiver<Message>) -> int {
  select {
    let Some(Message { id, data }) = ch.receive() => id + data,
    _ => 0,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn defer_simple() {
    let input = r#"
import "go:fmt"

fn test() {
  defer fmt.Print("cleanup")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn defer_with_method_call() {
    let input = r#"
import "go:fmt"

struct Resource {}

impl Resource {
  fn close(self) {
    fmt.Print("closed");
  }
}

fn test() {
  let r = Resource {};
  defer r.close()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn defer_with_block_body() {
    let input = r#"
import "go:fmt"

fn test() {
  defer {
    fmt.Print("cleanup 1");
    fmt.Print("cleanup 2");
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn defer_multiple() {
    let input = r#"
import "go:fmt"

fn test() {
  defer fmt.Print("first");
  defer fmt.Print("second");
  defer fmt.Print("third")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn defer_in_function_with_return() {
    let input = r#"
import "go:fmt"

fn test() -> int {
  defer fmt.Print("cleanup");
  42
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn defer_in_nested_block() {
    let input = r#"
fn record(acc: Ref<int>, digit: int) {
  acc.* = acc.* * 10 + digit
}

fn test() -> int {
  let mut acc = 0;
  {
    defer record(&acc, 1);
    record(&acc, 2)
  };
  record(&acc, 3);
  acc
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn defer_in_nested_block_multiple() {
    let input = r#"
import "go:fmt"

fn test() {
  {
    defer fmt.Print("block1 cleanup");
    fmt.Print("block1 work");
  };
  {
    defer fmt.Print("block2 cleanup");
    fmt.Print("block2 work");
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn buffered_channel() {
    let input = r#"
fn test() -> Channel<int> {
  Channel.buffered<int>(10)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn buffered_channel_inferred_type() {
    let input = r#"
fn test() -> Channel<string> {
  Channel.buffered(5)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn blocking_send() {
    let input = r#"
fn test() {
  let ch = Channel.new<int>();
  ch.send(42);
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn blocking_receive() {
    let input = r#"
fn test() -> Option<int> {
  let ch = Channel.new<int>();
  ch.receive()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn blocking_receive_match() {
    let input = r#"
fn test() -> int {
  let ch = Channel.new<int>();
  match ch.receive() {
    Some(v) => v,
    None => 0,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn channel_iteration() {
    let input = r#"
fn process(v: int) {}

fn test() {
  let ch = Channel.new<int>();
  for v in ch {
    process(v);
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn receiver_iteration() {
    let input = r#"
fn process(v: int) {}

fn test(rx: Receiver<int>) {
  for v in rx {
    process(v);
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn channel_split() {
    let input = r#"
fn test() -> (Sender<int>, Receiver<int>) {
  let ch = Channel.new<int>();
  ch.split()
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn channel_split_destructure() {
    let input = r#"
fn test() {
  let ch = Channel.new<int>();
  let (tx, rx) = ch.split();
  tx.send(42);
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_match_receive() {
    let input = r#"
fn process(v: int) {}
fn handle_close() {}

fn test() {
  let ch = Channel.new<int>();
  select {
    match ch.receive() {
      Some(v) => process(v),
      None => handle_close(),
    },
    _ => {},
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn task_parallel_same_variable_name() {
    let input = r#"
fn test() {
  let ch = Channel.new<int>()
  task {
    let mut found = 0
    found = 1
    ch.send(found)
  }
  task {
    let mut found = 0
    found = 2
    ch.send(found)
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn defer_channel_send() {
    let input = r#"
fn test() {
  let ch = Channel.new<int>();
  defer ch.send(42)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn task_channel_send() {
    let input = r#"
fn test() {
  let ch = Channel.new<int>();
  task ch.send(42)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_receive_wildcard_some() {
    let input = r#"
fn test() {
  let ch = Channel.new<int>();
  select {
    let Some(_) = ch.receive() => {},
    _ => {},
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_receive_ok_variable_not_shadowed() {
    let input = r#"
import "go:fmt"

fn test() {
  let ok = "everything is fine"
  let ch = Channel.buffered<int>(1)
  ch.send(42)
  select {
    match ch.receive() {
      Some(val) => {
        fmt.Println(f"received: {val}")
        fmt.Println(f"ok is: {ok}")
      },
      None => fmt.Println("closed"),
    },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_match_receive_wildcard_some() {
    let input = r#"
fn process_close() {}

fn test() {
  let ch = Channel.new<int>();
  select {
    match ch.receive() {
      Some(_) => {},
      None => process_close(),
    },
    _ => {},
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_break_in_loop_needs_label() {
    let input = r#"
import "go:fmt"

fn main() {
  let ch = Channel.buffered<string>(1)
  ch.send("hello")

  loop {
    select {
      let Some(msg) = ch.receive() => {
        fmt.Println(msg)
        break
      },
      _ => break,
    }
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_arm_binding_does_not_leak() {
    let input = r#"
import "go:fmt"

fn main() {
  let x = 100
  fmt.Println(x)
  let x = 200

  let ch = Channel.buffered<int>(1)
  ch.send(99)

  select {
    let Some(x) = ch.receive() => fmt.Println(x),
    _ => fmt.Println("default"),
  }

  fmt.Println(x)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_send_channel_expression_hoisted() {
    let input = r#"
import "go:fmt"

fn main() {
  let ch1 = Channel.buffered<int>(1)
  let ch2 = Channel.buffered<int>(1)

  let ch = if true { ch1 } else { ch2 }
  let result = select {
    ch.send(1) => 0,
    _ => 1,
  }

  fmt.Println(result)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_send_value_expression_hoisted() {
    let input = r#"
import "go:fmt"

fn main() {
  let ch = Channel.buffered<int>(1)
  let to_send = if true { 1 } else { 2 }
  let result = select {
    ch.send(to_send) => 0,
    _ => 1,
  }
  fmt.Println(result)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_complex_channel_expression_hoisted() {
    let input = r#"
fn main() {
  let ch1 = Channel.buffered<int>(1)
  let ch2 = Channel.buffered<int>(1)
  ch1.send(1)
  ch2.send(2)
  let flag = true
  let ch = if flag { ch1 } else { ch2 }
  select {
    let Some(v) = ch.receive() => {
      let _ = v
    },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_pattern_bindings_do_not_leak() {
    let input = r#"
import "go:fmt"

struct Point { x: int }

fn main() {
  let ch1 = Channel.buffered<Point>(1)
  let x = 99

  let result = select {
    let Some(Point { x }) = ch1.receive() => x,
    _ => x,
  }

  fmt.Println(result)
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn task_go_multi_return_call() {
    let input = r#"
import "go:fmt"

fn main() {
  task fmt.Println("hello")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_match_receive_binding_no_leak() {
    let input = r#"
import "go:fmt"

fn main() {
  let x = 1
  fmt.Println(x)
  let x = 2
  let ch = Channel.new<int>()
  task { ch.send(1) }

  select {
    match ch.receive() {
      Some(x) => {
        fmt.Println(x)
        0
      },
      None => 0,
    },
    _ => {
      fmt.Println(x)
      0
    },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn task_block_variable_shadowing() {
    let input = r#"
import "go:fmt"

fn main() {
  let x = 1
  fmt.Println(x)
  task {
    let x = 2
    fmt.Println(x)
    let x = 3
    fmt.Println(x)
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_hoist_temp_no_collision() {
    let input = r#"
fn get_ch() -> Channel<int> {
  Channel.buffered<int>(1)
}

fn main() -> int {
  let result = select {
    let Some(v) = get_ch().receive() => v,
    _ => 0,
  }
  let tmp_1 = result + 1
  tmp_1
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn task_go_nullable_return_call() {
    let input = r#"
import "go:html/template"

fn main() {
  let t = template.New("x")
  task t.Lookup("x")
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_ufcs_send() {
    let input = r#"
fn main() {
  let ch = Channel.new<int>()
  select {
    Channel.send(ch, 1) => { let _ = 0 },
    _ => { let _ = 1 },
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_send_hoisted_temp_no_redeclare() {
    let input = r#"
fn get(ch: Channel<int>) -> Channel<int> {
  ch
}

fn main() {
  let ch = Channel.buffered<int>(1)

  select {
    (&get(ch)).send(1) => { let _ = 0 },
    _ => { let _ = 1 },
  }

  let ref_1 = 2
  let _ = ref_1
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unit_call_in_channel_send() {
    let input = r#"
fn noop() {}

fn test() {
  let ch = Channel.buffered<()>(1)
  ch.send(noop())
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn unit_call_in_select_send_arm() {
    let input = r#"
fn noop() {}

fn test() {
  let ch = Channel.buffered<()>(1)
  select {
    ch.send(noop()) => {},
    _ => {},
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_shorthand_receive_complex_unused_recv() {
    let input = r#"
struct P { v: int }
fn test() {
  let ch = Channel.buffered<P>(1)
  ch.send(P { v: 1 })
  select {
    let Some(P { v }) = ch.receive() => (),
    _ => (),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_match_receive_complex_unused_recv() {
    let input = r#"
struct P { v: int }
fn test() {
  let ch = Channel.buffered<P>(1)
  ch.send(P { v: 1 })
  select {
    match ch.receive() {
      Some(P { v }) => (),
      None => (),
    },
    _ => (),
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_match_receive_none_branch_shadow() {
    let input = r#"
fn test() -> int {
  let x = 111
  let ch = Channel.new<int>()
  ch.close()
  let out = select {
    match ch.receive() {
      Some(x) => x,
      None => x,
    },
    _ => 0,
  }
  let _ = x
  out
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_closed_shorthand_receive_does_not_steal() {
    let input = r#"
fn test() -> int {
  let closed = Channel.new<int>()
  closed.close()
  let sink = Channel.buffered<int>(1)
  select {
    let Some(_x) = closed.receive() => 10,
    sink.send(5) => 20,
    _ => 30,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_send_before_shorthand_receive_eval_order() {
    let input = r#"
fn mark(log: Channel<int>, tag: int) -> int {
  log.send(tag)
  tag
}

fn choose(log: Channel<int>, tag: int, ch: Channel<int>) -> Channel<int> {
  let _ = mark(log, tag)
  ch
}

fn main() {
  let log = Channel.buffered<int>(8)
  let send_ch = Channel.buffered<int>(1)
  let recv_ch = Channel.buffered<int>(1)
  recv_ch.send(42)

  let _ = select {
    send_ch.send(mark(log, 1)) => 0,
    let Some(v) = choose(log, 2, recv_ch).receive() => v,
    _ => -1,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_retry_loop_hoists_send_values() {
    let input = r#"
fn mark(log: Channel<int>, tag: int) -> int {
  log.send(tag)
  tag
}

fn main() {
  let log = Channel.buffered<int>(8)
  let closed = Channel.new<int>()
  closed.close()
  let out_ch = Channel.new<int>()

  let _ = select {
    let Some(v) = closed.receive() => v,
    out_ch.send(mark(log, 1)) => 0,
    _ => -1,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_match_receive_before_shorthand_eval_order() {
    let input = r#"
fn choose(tag: int, ch: Channel<int>) -> Channel<int> {
  ch
}

fn main() {
  let ch1 = Channel.buffered<int>(1)
  let ch2 = Channel.buffered<int>(1)
  ch1.send(10)
  ch2.send(20)

  let _ = select {
    match choose(1, ch1).receive() {
      Some(v) => v,
      None => -1,
    },
    let Some(v) = choose(2, ch2).receive() => v,
    _ => -2,
  }
}
"#;
    assert_emit_snapshot!(input);
}

#[test]
fn select_raw_receive_before_shorthand_eval_order() {
    let input = r#"
fn choose(tag: int, ch: Channel<int>) -> Channel<int> {
  ch
}

fn main() {
  let ch1 = Channel.buffered<int>(1)
  let ch2 = Channel.buffered<int>(1)
  ch1.send(10)
  ch2.send(20)

  let _ = select {
    choose(1, ch1).receive() => 0,
    let Some(v) = choose(2, ch2).receive() => v,
    _ => -2,
  }
}
"#;
    assert_emit_snapshot!(input);
}
