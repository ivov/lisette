# Concurrency

Lisette follows Go's concurrency model, where lightweight tasks communicate through channels.

## `task`

A `task` spawns concurrent work, using call or block syntax.

```rust
task do_long_running_job()

task { 
  do_first_step() 
  do_second_step() 
}
```

## Channels

Tasks pass values via channels.

```rust
let ch = Channel.new<int>()

task { 
  let value = do_process()
  ch.send(value) 
}

match ch.receive() {
  Some(v) => fmt.Println(v),
  None => {},
}

ch.close()
```

`Channel.receive` returns `Some(value)` when a value is available, or `None` when the channel is closed.

`Channel.send` returns `true` if the value was sent, or `false` if the channel was closed (instead of panicking). `Channel.close` is idempotent, so calling it on an already-closed channel is a no-op.

For type safety, you can split a channel into sender and receiver:

```rust
let ch = Channel.new<int>()
let (tx, rx) = ch.split()

task { tx.send(42) }

match rx.receive() {
  Some(v) => fmt.Println(v),
  None => fmt.Println("closed"),
}
```

For signaling without a value — the Go `chan struct{}` pattern — use `Channel<()>`:

```rust
let done = Channel.new<()>()

task { 
  do_work()
  done.send(())
}

done.receive()  // blocks until the task signals completion
```

Run `lis doc Channel` for the full method list.

## `select`

`select` waits on multiple channel operations and runs the first one that is ready:

```rust
let result = select {
  match ch1.receive() {
    Some(v) => v,
    None => 0, // channel closed
  },
  match ch2.receive() {
    Some(v) => v * 2,
    None => 0, // channel closed
  },
}
```

A shorthand `receive` arm destructures the `Some` case:

```rust
let result = select {
  let Some(v) = ch.receive() => v,
  _ => 0,
}
```

The `_` arm serves two roles: it is a default case that runs immediately if no channel operation is ready (making the `select` non-blocking), and it is also the fallback for shorthand receive arms when the channel is closed.

A send arm in a `select` runs when the send completes:

```rust
select {
  ch.send(42) => fmt.Println("sent"),
  _ => fmt.Println("channel full"),
}
```

Send arms in `select` use raw Go channel operations. Unlike `ch.send()` outside of `select`, they can panic if the target channel is closed. Use `recover { select { ... } }` to guard against this.

## Iteration

Channels can be iterated until closed:

```rust
let ch = Channel.buffered<int>(3)
ch.send(1)
ch.send(2)
ch.send(3)
ch.close()

for v in ch {
  fmt.Println(v)  // 1, 2, 3
}
```

`Receiver<T>` is also iterable.

<br>

<table><tr>
<td>← <a href="13-go-interop.md"><code>13-go-interop.md</code></a></td>
<td align="right"><a href="15-attributes.md"><code>15-attributes.md</code></a> →</td>
</tr></table>
