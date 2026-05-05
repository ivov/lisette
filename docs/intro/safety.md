# Safety

Lisette prevents common Go runtime errors at compile time.

## `nil`

Go does not distinguish between nilable and non-nilable types.

```go
var m map[string]int
m["key"] = 1 // panic: assignment to `nil` map
```

Lisette defines `nil` out of existence and encodes absence as `Option<T>`:

```
  [error] `nil` is not supported
   ╭─[example.lis:2:11]
 1 │ fn main() {
 2 │   let x = nil
   ·           ─┬─
   ·            ╰── does not exist
 3 │ }
   ╰────
  help: Absence is encoded with `Option<T>` in Lisette. Use `None` to represent absent values
```

### Safe nilable pointers

In Go, a pointer may or may not be `nil`.

Lisette's pointer type `Ref<T>` is guaranteed non-`nil`. When Go returns a pointer, Lisette wraps it in an `Option<Ref<T>>`.

```rust
// Go:  `func Lookup(name string) *Flag`
// Lis: `fn Lookup(name: string) -> Option<Ref<Flag>>`

match flag.Lookup("verbose") {
  Some(f) => fmt.Println(f.Name),
  None => fmt.Println("flag not found"),
}
```

### Safe interface values

A Go interface can be `nil`, and calling methods on it panics:

```go
var h http.Handler
h.ServeHTTP(w, r) // panic: nil pointer dereference
```

There is also a subtler case: typed `nil`. A `nil` pointer assigned to an interface makes the interface non-`nil`, so the type is known, but the value is `nil`. Go's `!= nil` check passes, but calling methods still panics:

```go
var p *MyHandler = nil
var h http.Handler = p
h != nil      // true, the interface has a type
h.ServeHTTP() // panic: the value inside is nil
```

To protect against this, Lisette wraps a Go interface in `Option` when it crosses the interop boundary in a position where it could be `nil`. Both a `nil` interface and a typed `nil` interface become `None`:

```rs
// Go:  func FindHandler(name string) http.Handler
// Lis: fn FindHandler(name: string) -> Option<http.Handler>

match FindHandler("api") {
  Some(h) => router.Handle("/api", h),
  None => fmt.Println("no handler"),
}
```

### Safe access for maps and slices

Go zero-values a missing key in a map, and panics on out-of-bounds index access in a slice:

```go
users := map[string]*User{"alice": alice}
bob := users["bob"]    // `bob` is `nil`
bob.Name               // panic: `nil` pointer dereference

items := arr[1:]       // `items` is `[]`
items[0]               // panic: index out of range
```

Lisette offers `Map.get` and `Slice.get` returning `Option<V>`:

```rust
match users.get("bob") {
  Some(u) => fmt.Println(u.name),
  None => fmt.Println("user not found"),
}

match items.get(0) {
  Some(item) => fmt.Println(item),
  None => fmt.Println("item not found"),
}
```

### Safe sub-slicing

In Go, sub-slicing creates a new slice that shares the same backing array. Calling `append` on the sub-slice may silently mutate elements in the original, depending on capacity at runtime:

```go
source := []int{1, 2, 3, 4, 5}
sub := source[1:3]            // sub = [2, 3], cap = 4
sub = append(sub, 99)         // overwrites source[3] => [1, 2, 3, 99, 5]
```

Lisette compiles slices using Go's three-index slice syntax, which caps the sub-slice capacity to its length. Calling `append` on a Lisette sub-slice always allocates a fresh backing array, so the original is never silently mutated.

```rust
let source = [1, 2, 3, 4, 5]
let sub = source[1..3]        // compiles to source[1:3:3], cap = 2
let sub = sub.append(99)      // allocates new array, source untouched
```

In addition, when a sub-slice is bound with `let mut`, Lisette clones the sub-slice to sever the backing array alias entirely. This prevents writes through the sub-slice from mutating the original:

```rust
let source = [1, 2, 3, 4, 5]
let mut sub = source[1..3]    // cloned - fresh backing array
sub[0] = 99                   // only sub is affected, source unchanged
```

Immutable sub-slices with `let` remain zero-copy since element writes are not permitted on them.

## Enforced error handling

Go allows disregarding errors from fallible operations:

```go
func readConfig(path string) (Config, error) {
    file, _ := os.Open(path)       // error ignored with `_`
    bytes, _ := io.ReadAll(file)   // error ignored with `_`
    return parseConfig(bytes)
}
```

Lisette enforces error handling with `Result` and offers `?` for propagation:

```rust
fn read_config(path: string) -> Result<Config, error> {
  let file = os.Open(path)?
  let bytes = io.ReadAll(file)?
  parse_config(bytes)
}
```

Lisette omits Rust's `unwrap()`. To extract a value:

- `?` to propagate
- `match` to handle both cases
- `let else` for early exit
- `unwrap_or` with a default

📚 See [`09-error-handling.md`](../reference/09-error-handling.md)

## Exhaustive pattern matching

Go's `switch` silently tolerates a missing case:

```go
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
    // `Critical` silently returns `false`, no alert
    return false
}
```

Lisette enforces exhaustive matching:

```rust
enum Severity { Low, High, Critical }

fn should_alert(s: Severity) -> bool {
  match s {
    Severity.Low => false,
    Severity.High => true,
  }
}
```

```
  [error] `match` is not exhaustive
   ╭─[example.lis:4:3]
 3 │ fn should_alert(s: Severity) -> bool {
 4 │   match s {
   ·   ───┬───
   ·      ╰── not all patterns covered
 5 │     Severity.Low => false,
 6 │     Severity.High => true,
 7 │   }
   ╰────
  help: Handle the missing case `Severity.Critical`, e.g. `Severity.Critical => { ... }`
```

📚 See [`08-pattern-matching.md`](../reference/08-pattern-matching.md)

## Immutability

Go's bindings are mutable by default, so they may change unexpectedly.

```go
func process(config Config) {
    timeout := config.Timeout

    // multiple lines later...

    timeout = 30 // accident?

    // multiple lines later...

    connect(timeout) // what value is it now?
}
```

Lisette's bindings are immutable by default, requiring `mut` to mutate:

```
  [error] Immutable variable
   ╭─[example.lis:3:3]
 2 │   let timeout = config.timeout
 3 │   timeout = 30
   ·   ──────┬─────
   ·         ╰── cannot mutate an immutable variable
 4 │ }
   ╰────
  help: Declare using `let mut timeout` to make the variable mutable
```

This extends to method receivers. In Go, mutating through a value receiver is a common bug:

```go
func (c Counter) Increment() {
    c.count++ // mutates copy, original unchanged
}
```

In Lisette, value receivers are immutable like any other binding:

```
  [error] Immutable receiver
   ╭─[example.lis:7:5]
 5 │ impl Counter {
 6 │   fn increment(self) {
 7 │     self.count += 1
   ·     ───────┬───────
   ·            ╰── receiver is immutable
 8 │   }
   ╰────
  help: Use `self: Ref<Counter>` to mutate the receiver
```

### Mutability indicator

Go functions that mutate their parameters (e.g. `sort.Ints`) do not signal that they do so:

```go
nums := []int{3, 1, 2}
sort.Ints(nums) // silently mutates `nums`
```

In Lisette, functions that mutate their parameters must declare so:

```
  [error] Immutable argument passed to `mut` parameter
   ╭─[example.lis:5:13]
 4 │   let nums = [3, 1, 2]
 5 │   sort.Ints(nums)
   ·             ──┬─
   ·               ╰── expected mutable, found immutable
   ╰────
  help: Bindings in Lisette are immutable by default. Use `let mut nums = ...` to allow mutation
```

Lisette warns about excess mutability:

```
  [warning] Unnecessary `mut`
   ╭─[example.lis:2:11]
 1 │ fn main() {
 2 │   let mut count = items.length()
   ·           ──┬──
   ·             ╰── declared as mutable
 3 │ }
   ╰────
  help: Remove `mut` from the declaration if you do not need to mutate the variable
```

ℹ️ The immutability of `let` protects the binding from reassignment, but it does not protect the value from mutation through a `Ref<T>` pointer.

📚 See [`02-types.md`](../reference/02-types.md)

## Zero values

Since Go zero-values uninitialized variables, a zero value can be confused for a meaningful default:

```go
var count int       // 0
var name string     // ""
var ready bool      // false
```

Lisette requires explicit initialization for every variable:

```rust
let count = 0
let name = ""
let ready = false
```

Go also zero-values uninitialized struct fields, which can lead to panics:

```go
type Server struct {
    Handler http.Handler
    Logger  *log.Logger
    DB      *sql.DB
}

s := Server{Handler: mux} // `Logger` and `DB` are `nil`
s.Logger.Print("ready")   // panic: `nil` pointer dereference
```

In Lisette, all struct fields must be initialized:

```
  [error] Struct `Server` is missing fields
   ╭─[example.lis:8:11]
 7 │ fn main() {
 8 │   let s = Server { handler: mux }
   ·           ───┬──
   ·              ╰── missing fields: `db`, `logger`
 9 │ }
   ╰────
  help: Initialize all fields, or add `..` to zero-fill the rest
```

📚 See [`07-pointers.md`](../reference/07-pointers.md) and [`13-go-interop.md`](../reference/13-go-interop.md)

## Bindings

Go's `:=` declares and assigns in one step. This can create subtle bugs:

```go
var err error
if condition {
    x, err := doSomething()  // declares `x`, shadows `err`
    process(x)
}
return err  // always `nil`
```

Lisette's `let` always creates a new binding, and reassignment requires `mut`:

```rust
let result = step1()? // declared, cannot be reassigned
if result > 0 {
  let result = step2()?  // new binding
  use(result)
}

// vs.

let mut result = step1()? // declared
if result > 0 {
  result = step2()? // reassigned
}
```

## Defer

Go's `defer` runs at function exit, not at scope exit. Using `defer` inside a loop is a common bug:

```go
for _, path := range files {
    f, err := os.Open(path)
    if err != nil {
        continue
    }
    defer f.Close()
}
// all files close together at function exit, not per iteration
```

Lisette rejects this and other misuses of `defer` at compile time:

```
  [error] `defer` inside loop
   ╭─[example.lis:7:5]
 5 │   for path in files {
 6 │     let f = os.Open(path)?
 7 │     defer f.Close()
   ·     ───────┬───────
   ·            ╰── not allowed inside loop
 8 │   }
   ╰────
  help: Wrap the loop body in a helper function, e.g.
       `fn process(file: File) { defer file.close(); ... }`
       and call it in the loop: `for f in files { process(f) }`
```

📚 See [`04-control-flow.md`](../reference/04-control-flow.md)

## Channels

### Safe closed-channel checks

In Go, a closed channel silently yields the zero value:

```go
ch := make(chan int)
close(ch)
v := <-ch  // `v` is `0`, no indication `ch` is closed
```

Go offers `ok` in `v, ok := <-ch` to check for closed channels, but checking is opt-in. This can lead to processing zero values as real data.

In Lisette, `Channel.receive` returns `None` for closed channels:

```rust
match ch.receive() {
  Some(v) => process(v),
  None => handle_closed(),
}
```

This extends to `select` expressions:

```rust
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

### Panic-safe channel operations

Go panics if you send to a closed channel, or if you close an already closed channel:

```go
ch := make(chan int)
close(ch)
ch <- 42    // panic: send on closed channel
close(ch)   // panic: close of closed channel
```

In Lisette, `send` returns `false` and `close` is idempotent:

```rust
let ch = Channel.new<int>()
ch.close()
ch.send(42)  // returns `false`, no panic
ch.close()   // no-op, no panic
```

See [`14-concurrency.md`](../reference/14-concurrency.md)

### Type-safe channels

In Go, any `chan T` can be sent to, received from, and closed. If a consumer closes a channel while a producer is still sending, the runtime panics. Go offers directional types `chan<- T` and `<-chan T` but they are opt-in, so the user must manually narrow at every function boundary.

Lisette offers `Channel.split` to type channels upfront:

```rust
let (tx, rx) = Channel.new<int>().split()

task {
  produce(tx)  // `Sender<int>` can only `send` and `close`
}
consume(rx)    // `Receiver<int>` can only `receive`
```

📚 See [`14-concurrency.md`](../reference/14-concurrency.md)


## Safe type assertions

In Go, type assertions can panic:

```go
func getRequestID(ctx context.Context) string {
    val := ctx.Value("request_id")
    str := val.(string)  // panics if not string, must remember to check `ok` (opt-in)
    return str
}
```

In Lisette, values of unknown type coming from Go (`any` and `interface{}`) must be narrowed before use. This is done with `assert_type`:

```rust
fn get_request_id(ctx: context.Context) -> Option<string> {
  let value = ctx.Value("request_id")   // `value` is `Unknown`
  let id = assert_type<string>(value)?  // `id` is `string`, else `None`
  Some(id)
}
```

📚 See [`13-go-interop.md`](../reference/13-go-interop.md)

## More compile-time checks

Lisette catches several more Go pitfalls at compile time. For example:

- **Nil hiding behind nil error.** Go functions can return `(nil, nil)` — a nil pointer with no error. The `err != nil` check passes, and the nil pointer panics later. When wrapping Go calls that return `(*T, error)`, Lisette checks both values and converts a nil pointer to `Err` even when the error is nil.
- **Map field chain assignment.** In Go `m["key"].field = value` is a silent no-op (map lookup returns a copy). Lisette rejects this.
- **Non-comparable equality.** In Lisette, using `==` on a struct that contains a slice, function, or map field is a compile error. Go panics at runtime.
- **Numeric literal overflow.** Lisette rejects `let x: int8 = 200`. Go silently truncates.
