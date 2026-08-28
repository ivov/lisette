---
title: "Concurrency"
description: "task, channels, select"
---

Lisette follows Go's concurrency model, where [goroutines](https://go.dev/ref/spec#Go_statements) communicate via channels.

## `task`

A `task` (goroutine) spawns concurrent work.

```lisette
task long_running_job()

task {
  let records = fetch_records()
  write_report(records)
}
```

## `Channel`

A [`Channel`](/docs/prelude/channels/#channelt) carries values between tasks.

```lisette
let ch = Channel.new<int>()

task {
  let value = heavy_computation()
  ch.send(value)
}

// !callout-right `value` sent by `task`
if let Some(v) = ch.receive() {
  fmt.Println(v)
}

ch.close()
```

You can split a channel into sender and receiver:

```lisette
let ch = Channel.new<int>()
// !callout-right `tx` is a `Sender<int>`, `rx` a `Receiver<int>`
let (tx, rx) = ch.split()

task tx.send(42)

match rx.receive() {
  Some(v) => fmt.Println(v),
  None => fmt.Println("channel closed"),
}
```

For signaling without a value, use `Channel<()>`:

```lisette
let done = Channel.new<()>()

task {
  rebuild_index()
  done.send(())
}

// !callout-right blocks until `task` signals completion
done.receive()
```

A `Channel` can be iterated until closed:

```lisette
let ch = Channel.buffered<int>(3)
ch.send(1)
ch.send(2)
ch.send(3)
ch.close()

for v in ch {
  // !callout-right prints `1, 2, 3`
  fmt.Println(v)
}
```

A [`Receiver<T>`](/docs/prelude/channels/#receivert) is iterable in the same way.

## `select`

`select` waits on several channel operations and runs whichever is ready first:

```lisette
let result = select {
  match ch1.receive() {
    Some(v) => v,
    None => 0,
  },
  match ch2.receive() {
    Some(v) => v * 2,
    None => 0,
  },
}
```

A shorthand `receive` arm destructures the `Some` case:

```lisette
let result = select {
  let Some(v) = ch.receive() => v,
  _ => 0,
}
```

The `_` arm runs immediately when no operation is ready, which makes the `select` non-blocking. It also catches the closed case for a shorthand receive arm.

A send arm in a `select` runs when the send completes:

```lisette
select {
  ch.send(42) => fmt.Println("sent"),
  _ => fmt.Println("channel full"),
}
```

A send arm can panic with `send on closed channel`, unlike `ch.send()` outside a `select`, which returns `false`. Wrap the `select` in a `recover` block to guard against this.
