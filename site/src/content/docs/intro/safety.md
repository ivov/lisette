---
title: "Safety"
description: "How Lisette guards against Go runtime errors at compile time"
---

Lisette guards against Go runtime errors at compile time.

## Absent values

Go does not distinguish between nilable and non-nilable types.

```go file="nilmap.go"
var ages map[string]int
// !callout-error-right+6 panic: assignment to `nil` map
ages["michael"] = 30
```

Lisette defines nil out of existence and encodes absence in the type system.

```ansi wrap
  [31m✕[0m [95mnil[39m[1m[31m is not supported[39m[0m
   ╭─[example.lis:7:12]
 [2m6[0m │   if name.is_empty() {
 [2m7[0m │     return [31mnil[0m
   · [31m           ─┬─[0m
   ·             [31m╰── [31m[31mdoes not exist[39m[0m[0m
 [2m8[0m │   }
   ╰────
[2m  help: [0m[2mAbsence is encoded with [0m[95mOption<T>[39m[2m in Lisette. Use [0m[95mNone[39m[2m to represent absent values[0m[2m · code: [0m[2m[resolve.nil_not_supported][0m
```

📚 See [Option](/docs/prelude/option/)

### Nil pointers

Go's pointer type may or may not be nil. In Lisette, a pointer type is guaranteed non-nil, and a potentially absent one is typed as optional.

```go file="find.go"
// func find(id int) *Person

p := find(1)
if p != nil {
    fmt.Println(p.Name)
}
```

```lisette file="find.lis"
// fn find(id: int) -> Option<Ref<Person>>

match find(1) {
  Some(person) => fmt.Println(person.name),
  None => fmt.Println("no person"),
}
```

📚 See [References](/docs/references/)

### Nil maps

A Go map may be nil, which panics on write. Lisette reflects this optionality in the incoming type.

```lisette file="users.lis"
// Go:  func (h Header) Clone() Header
// Lis: fn Clone(self) -> Option<Header>

match request.Header.Clone() {
  Some(headers) => forward(headers),
  None => fmt.Println("no headers to forward"),
}
```

📚 See [Map](/docs/prelude/map/)

### Nil interfaces

A Go interface may be nil, and calling methods on it panics.

```go file="handler.go"
var h http.Handler
// !callout-error-right+8 panic: `nil` pointer dereference
h.ServeHTTP(w, r)
```

There is also a subtler case: typed nil. A nil pointer assigned to an interface makes the interface non-nil, so the type is known, but the value is nil. Go's `!= nil` check passes, but calling methods still panics.

```go file="handler.go"
var p *MyHandler = nil
var h http.Handler = p
// !callout-right+17 `true`, the interface has a type
h != nil
// !callout-error-right+12 panic: the value inside is `nil`
h.ServeHTTP()
```

Lisette wraps a Go interface in `Option` when it crosses the interop boundary in a position where it could be nil. Both a nil interface and a typed nil interface become `None`.

```lisette file="handler.lis"
// Go:  func FindHandler(name string) http.Handler
// Lis: fn FindHandler(name: string) -> Option<http.Handler>

match FindHandler("api") {
  Some(h) => router.Handle("/api", h),
  None => fmt.Println("no handler"),
}
```

📚 See [Nil Go interfaces](/docs/interfaces/#nil-go-interfaces)

### Zero values

Go zero-values an uninitialized variable. This blurs the difference between set and unset.

```go file="zero.go"
// !callout-right `0`
var count int
// !callout-right `""`
var name string
// !callout-right `false`
var ready bool
```

In Lisette, every binding must be initialized.

```lisette file="zero.lis"
let count = 0
let name = ""
let ready = false
```

For struct fields, zero values risk failure at runtime.

```go file="server.go"
type Server struct {
    Handler http.Handler
    Logger  *log.Logger
    DB      *sql.DB
}

// !callout-right `Logger` and `DB` are `nil`
s := Server{Handler: mux}
// !callout-error-right panic: `nil` pointer dereference
s.Logger.Print("ready")
```

In Lisette, every field must be initialized.

```ansi wrap
  [31m✕[0m [1m[31mStruct [39m[0m[95mServer[39m[1m[31m is missing fields[39m[0m
    ╭─[example.lis:9:11]
 [2m 8[0m │   let mux = 1
 [2m 9[0m │   let s = [31mServer[0m { handler: mux }
    · [31m          ───┬──[0m
    ·              [31m╰── [31m[31mmissing fields: db, logger[39m[0m[0m
 [2m10[0m │ }
    ╰────
[2m  help: [0m[2mInitialize all fields, or add [0m[95m..[39m[2m to autofill the rest[0m[2m · code: [0m[2m[infer.missing_struct_fields][0m
```

A map lookup blurs similarly.

```go file="scores.go"
scores := map[string]int{"alice": 0}
// !callout-right `0`, and alice is `0`
scores["alice"]
// !callout-right `0`, and bob is missing
scores["bob"]
```

In Lisette, `Map.get` separates absence from default value.

```lisette file="scores.lis"
// !callout-right `Some(0)`
let stored = scores.get("alice")
// !callout-right `None`
let absent = scores.get("bob")
```

📚 See [Struct instantiation](/docs/structs/#instantiation) and [Map](/docs/prelude/map/)

### Bad indexes

An out-of-range index panics in Go. In Lisette, `Slice.get` and `Array.get` return an `Option` instead.

```lisette file="items.lis"
match items.get(7) {
  Some(item) => fmt.Println(item),
  None => fmt.Println("out of range"),
}
```

📚 See [Slice](/docs/prelude/slice/) and [Array](/docs/prelude/array/)

## Unhandled cases

### Dropped errors

Go allows ignoring errors from fallible operations.

```go file="config.go"
func readConfig(path string) (Config, error) {
    // !callout-error-right error ignored with `_`
    bytes, _ := os.ReadFile(path)
    return parseConfig(bytes)
}
```

Lisette flags an unhandled `Result`.

```ansi wrap
  [33m▲[0m [95mResult[39m[1m[33m is silently discarded[39m[0m
    ╭─[example.lis:10:3]
 [2m 9[0m │ fn read_config(path: string) -> Config {
 [2m10[0m │   [33mos.ReadFile(path)[0m
    · [33m  ────────┬────────[0m
    ·           [33m╰── [33m[33mfailure will go unnoticed[39m[0m[0m
 [2m11[0m │   parse_config()
    ╰────
[2m  help: [0m[2mHandle this [0m[95mResult[39m[2m with [0m[95m?[39m[2m or [0m[95mmatch[39m[2m, or explicitly discard it with [0m[95mlet _ = ...[39m[2m · code: [0m[2m[lint.unused_result][0m
```

Lisette omits Rust's `unwrap()`.

📚 See [Failures](/docs/failures/)

### Non-exhaustiveness

Go tolerates missing cases in `switch` statements.

```go file="severity.go"
type Severity int

const (
    Low Severity = iota
    High
    Critical
)

func shouldAlert(s Severity) bool {
    switch s {
    case Low:
        return false
    case High:
        return true
    }
    // !callout-right `Critical` produces no alert
    return false
}
```

Lisette requires `match` to be exhaustive.

```ansi wrap
  [31m✕[0m [95mmatch[39m[1m[31m is not exhaustive[39m[0m
   ╭─[example.lis:4:3]
 [2m1[0m │ enum Severity { Low, High, Critical }
 [2m2[0m │ 
 [2m3[0m │ fn should_alert(s: Severity) -> bool {
 [2m4[0m │   [31mmatch s[0m {
   · [31m  ───┬───[0m
   ·      [31m╰── [31m[31mnot all patterns covered[39m[0m[0m
 [2m5[0m │     Low => false,
 [2m6[0m │     High => true,
 [2m7[0m │   }
 [2m8[0m │ }
   ╰────
[2m  help: [0m[2mHandle the missing case [0m[95mCritical[39m[2m, e.g. [0m[95mCritical => { ... }[39m[2m · code: [0m[2m[infer.non_exhaustive][0m
```

📚 See [Pattern matching](/docs/pattern-matching/)

### Panicking assertions

In Go, a type assertion can panic, and the `ok` check is opt-in.

```go file="request.go"
func getRequestID(ctx context.Context) string {
    val := ctx.Value("request_id")
    // !callout-error[/\(string\)/] panics if not string
    str := val.(string)
    return str
}
```

In Lisette, `Unknown` is narrowed safely.

```lisette file="request.lis"
fn get_request_id(ctx: context.Context) -> Option<string> {
  // !callout[/raw_id/] `Unknown` type
  let raw_id = ctx.Value("request_id")
  // !callout[/assert_type/] either `id` is `string`, or `None` propagates
  let id = assert_type<string>(raw_id)?
  Some(id)
}
```

📚 See [Unknown](/docs/prelude/types/#unknown)

### Panicking conversions

Go converts a slice to a fixed-size array without a length check, and panics when the slice is too short.

```go file="addr.go"
func addr(b []byte) [4]byte {
    // !callout-error-right panic: cannot convert slice with length 2 to array with length 4
    return [4]byte(b)
}
```

In Lisette, `Array.from` reports the length mismatch as `None`.

```lisette file="addr.lis"
fn addr(b: Slice<byte>) -> Option<Array<byte, 4>> {
  // !callout[/from/] `Some([1, 2, 3, 4])`, or `None` unless `b` holds exactly 4
  Array.from(b)
}
```

📚 See [Array](/docs/prelude/array/)

## Unintended writes

### Silent mutation

Go does not signal that a function may mutate the caller's data.

```go file="sort.go"
nums := []int{3, 1, 2}
// !callout-error-right mutates `nums`
slices.Sort(nums)
```

Lisette makes write permission part of the type.

```lisette file="permission.lis"
// !callout-right cannot write to `items`
fn total(items: Slice<int>) -> int
// !callout-right may write to `items`
fn fill(items: mut Slice<int>)
```

Lisette knows which Go functions write to their arguments.

```ansi wrap
  [31m✕[0m [1m[31mImmutable variable[39m[0m
   ╭─[example.lis:5:15]
 [2m4[0m │   let nums = [3, 1, 2]
 [2m5[0m │   slices.Sort([31mnums[0m)
   · [31m              ──┬─[0m
   ·                 [31m╰── [31m[95mnums[39m[31m was declared without [39m[95mmut[39m[0m[0m
 [2m6[0m │ }
   ╰────
[2m  help: [0m[95mslices.Sort()[39m[2m writes to [0m[95mnums[39m[2m. Declare using [0m[95mlet mut nums[39m[2m to mark the variable mutable[0m[2m · code: [0m[2m[infer.immutable][0m
```

📚 See [Write permission in parameters](/docs/functions/#write-permission-in-parameters)

### Mutable bindings

Go's bindings are mutable by default, so they may change unexpectedly.

```go file="process.go"
// !callout-error-right nothing marks it as changeable
timeout := config.Timeout
timeout = 30
```

Lisette bindings are immutable, unless marked otherwise.

```ansi wrap
  [31m✕[0m [1m[31mImmutable variable[39m[0m
   ╭─[example.lis:7:3]
 [2m6[0m │   let timeout = config.timeout
 [2m7[0m │   [31mtimeout = 30[0m
   · [31m  ──────┬─────[0m
   ·         [31m╰── [31m[95mtimeout[39m[31m was declared without [39m[95mmut[39m[0m[0m
 [2m8[0m │ }
   ╰────
[2m  help: [0m[2mDeclare using [0m[95mlet mut timeout[39m[2m to mark the variable mutable[0m[2m · code: [0m[2m[infer.immutable][0m
```

The same applies to method receivers.

```go file="counter.go"
func (c Counter) Increment() {
    // !callout-error-right mutates copy, original intact
    c.count++
}
```

In Lisette, a method that mutates must declare `self` writable with `mut`.

```lisette file="counter.lis"
impl Counter {
  fn increment(self: mut Ref<Counter>) {
    self.count += 1
  }
}
```

📚 See [Bindings](/docs/bindings/)

### Aliased collections

In Go, assigning a slice or map copies the handle, not the data.

```go file="share.go"
a := []int{1, 2, 3}
b := a
// !callout-right `a` is now `[99 2 3]`
b[0] = 99
```

Lisette copies the handle and its permission. If the original is read-only, so is the new handle.

```ansi wrap
  [31m✕[0m [1m[31mCannot write to [39m[0m[95mb[0][39m
   ╭─[example.lis:4:3]
 [2m2[0m │   let a = [1, 2, 3]
 [2m3[0m │   let b = a
 [2m4[0m │   [31mb[0] = 99[0m
   · [31m  ────┬────[0m
   ·       [31m╰── [31m[95mb[39m[31m is read-only[39m[0m[0m
 [2m5[0m │ }
   ╰────
[2m  help: [0m[95mb[39m[2m shares storage with [0m[95ma[39m[2m, which is read-only. Make [0m[95ma[39m[2m writable, or write to a [0m[95m.clone()[39m[2m for an independent copy[0m[2m · code: [0m[2m[infer.write_through_read_only][0m
```

The guarantee is "no unmarked mutation" (explicitness), not "only one writer" (exclusivity). No write should happen without `mut` in the type, but two `mut` values may share storage.

📚 See [Write permission in bindings](/docs/bindings/#write-permission-in-bindings)

### Sub-slicing

In Go, `append` on a sub-slice may mutate the original, depending on capacity at runtime.

```go file="subslice.go"
original := []int{1, 2, 3, 4}
// !callout-right `subslice` is `[2, 3]`, capacity `3`
subslice := original[1:3]
// !callout-error-right mutates `original[3]`, giving `[1, 2, 3, 99]`
subslice = append(subslice, 99)
```

In Lisette, `append` on a sub-slice always allocates, so the original never changes. Sub-slicing stays free.

```lisette file="subslice.lis"
let original = [1, 2, 3, 4]
let subslice = original[1..3]
// !callout-ok-right `original` is intact
let subslice = subslice.append(99)
```

📚 See [Slice](/docs/prelude/slice/)

### Shadowing

Go's `:=` declares and assigns in one step. This can create subtle bugs.

```go file="bindings.go"
var err error
if condition {
    // !callout-error-right declares `x`, shadows `err`
    x, err := doSomething()
    process(x)
}
// !callout-error-right always `nil`
return err
```

Lisette's `let` always creates a new binding, and reassignment requires `mut`.

```lisette file="let.lis"
// !callout-right declared, cannot be reassigned
let result = step1()?
if result > 0 {
  // !callout-right new binding
  let result = step2()?
  use(result)
}
```

```lisette file="let_mut.lis"
// !callout-right declared, reassignable
let mut result = step1()?
if result > 0 {
  // !callout-right reassigned
  result = step2()?
}
```

📚 See [Bindings](/docs/bindings/)

## Channel hazards

### Closed channels

In Go, a closed channel silently yields the zero value, and the `ok` check is opt-in. A zero can pass for a sent value.

```go file="closed.go"
ch := make(chan int)
close(ch)
// !callout-error-right `v` is `0`, no indication `ch` is closed
v := <-ch
```

In Lisette, `Channel.receive` returns `None` for closed channels.

```lisette file="closed.lis"
match ch.receive() {
  Some(v) => process(v),
  None => handle_closed(),
}
```

Go also panics if you send to a closed channel, or if you close an already closed channel.

```go file="panic.go"
ch := make(chan int)
close(ch)
// !callout-error-right panic: send on closed channel
ch <- 42
// !callout-error-right panic: close of closed channel
close(ch)
```

In Lisette, `send` returns `false` and `close` is idempotent.

```lisette file="send_close.lis"
let ch = Channel.new<int>()
ch.close()
// !callout-right returns `false`
ch.send(42)
// !callout-right no-op
ch.close()
```

Sending inside a `select` is the exception, and a `recover` block catches it.

📚 See [Channels](/docs/prelude/channels/)

### Nil channels

In Go, sending to or receiving from a nil channel blocks forever.

```go file="nil_channel.go"
var ch chan int
// !callout-error-right fatal error: all goroutines are asleep, deadlock!
v := <-ch
```

In Lisette, a `Channel` has no zero value.

```ansi wrap
  [31m✕[0m [1m[31mMissing initializer[39m[0m
   ╭─[example.lis:2:11]
 [2m1[0m │ fn main() {
 [2m2[0m │   let ch: [31mChannel<int>[0m
   · [31m          ──────┬─────[0m
   ·                 [31m╰── [31m[31mannotated binding needs a value[39m[0m[0m
 [2m3[0m │   let v = ch.receive()
   ╰────
[2m  help: [0m[2mBindings must be initialized[0m[2m · code: [0m[2m[parse.missing_initializer][0m
```

A Go channel that may be nil arrives as an `Option`. Inside a `select`, a nil channel keeps its Go meaning, i.e. that arm is never ready.

📚 See [Channels](/docs/prelude/channels/)

### Channel direction

Any Go channel can be sent to, received from, and closed. Closing it while a producer is still sending panics. Go's directional types `chan<- T` and `<-chan T` are opt-in.

In Lisette, channel direction is encoded in the type.

```lisette file="split.lis"
let (tx, rx) = Channel.new<int>().split()

// !callout-right `Sender<int>` can only `send()` and `close()`
task send_jobs(tx)
// !callout-right `Receiver<int>` can only `receive()`
run_jobs(rx)
```

📚 See [Concurrency](/docs/concurrency/)

## Beyond the basics

In all, Lisette reports 500+ diagnostics across errors, warnings, and advisories.

- `wg.Add(1)` inside a goroutine can run after `wg.Wait()`.
- `defer f.Close()` in a loop keeps every file open until return.
- `for _, r := range rows` binds a copy, so writes to `r` never reach `rows`.
- `m[key]` has nothing to return when the value type has no zero.
- `append` to a `make([]T, n)` slice leaves `n` zeros in front.
- `==` on an interface holding a slice, map or function panics in Go.
- `os.Exit` terminates the process before any `defer` runs.
- `cancel` from `context.WithCancel` leaks the context until called.
- `URL.Query()` returns a copy in Go, so mutating it changes nothing.
