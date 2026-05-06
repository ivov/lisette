# Types

- **Hindley-Milner type inference.** Lisette is sound (no runtime type mismatches), decidable (mostly no annotations), and polymorphic (has generics).
- **Concrete types are nominal.** Two structs with identical fields but different names are distinct types.
- **Interfaces are structural.** A type satisfies an interface by having all its methods, as in Go. No declaration required.
- **No subtyping.** No inheritance, no implicit widening. Generic type parameters are invariant: `Slice<Cat>` does not satisfy `Slice<Animal>` even if `Cat` satisfies `Animal`.
- **No ownership or lifetimes.** Memory is garbage-collected. `Ref<T>` is a pointer, not an ownership marker.

## Primitive types

### Numeric types

| Type                                  | Description                                                    |
| ------------------------------------- | -------------------------------------------------------------- |
| `int`                                 | Platform-sized signed integer                                  |
| `int8`, `int16`, `int32`, `int64`     | Fixed-width signed integers                                    |
| `uint`                                | Platform-sized unsigned integer                                |
| `uint8`, `uint16`, `uint32`, `uint64` | Fixed-width unsigned integers                                  |
| `uintptr`                             | Unsigned integer large enough to hold a pointer                |
| `byte`                                | 8-bit unsigned integer, identical to `uint8`                   |
| `rune`                                | Unicode code point                                             |
| `float32`, `float64`                  | Floating-point numbers                                         |
| `complex64`, `complex128`             | Complex numbers                                                |

Integer literals default to `int`. Float literals default to `float64`. Both adapt to the expected type when the context is unambiguous:

```rust
let x = 42                  // int
let y: int64 = 42           // int64 — literal adapts to expected type
let z = 3.14                // float64
let w: float32 = 3.14       // float32 — literal adapts to expected type
```

Numeric types do not implicitly convert. Use [`as`](#casting) to convert explicitly.

```rust
let a: int = 1
let b: int64 = 2
let c = a + b // error
```

```
  [error] Type mismatch
   ╭─[example.lis:3:9]
 3 │ let c = a + b
   ·         ──┬──
   ·           ╰── cannot add `int` and `int64`
   ╰────
  help: The `+` operator requires both operands to have the same type
```

```rust
let c = a + (b as int) // explicit conversion
```

Complex numbers use an `i` suffix for the imaginary part:

```rust
let c: complex128 = 1.0 + 2.0i
```

### Boolean

```rust
let yes = true
let no = false
```

### String

Strings are immutable and UTF-8 encoded.

```rust
let name = "Alice"
let length = name.length()    // number of bytes
let empty = name.is_empty()   // false
```

To index into a string, pick a unit:

```rust
s.byte_at(i)              // byte
s.rune_at(i)              // rune
```

To slice a string, pick a unit:

```rust
s.substring(a..b)         // string (rune-indexed)
s.bytes()[a..b]           // Slice<byte>
s.runes()[a..b]           // Slice<rune>
```

📚 See [`04-control-flow.md`](04-control-flow.md#for) for iteration.

## Compound types

### `Option<T>`

A value that may or may not be present.

```rust
enum Option<T> {
  Some(T),
  None,
}
```

```rust
let name = Some("Alice")
let missing = None

let fallback = name.unwrap_or("unknown")
let mapped = name.map(|s| s.length())
```

Go functions with a `(T, bool)` return signature all return `Option`.

📚 See [`09-error-handling.md`](09-error-handling.md)

### `Result<T, E>`

The success or failure of a fallible operation.

```rust
enum Result<T, E> {
  Ok(T),
  Err(E),
}
```

```rust
fn parse_port(s: string) -> Result<int, error> {
  let n = strconv.Atoi(s)?
  if n < 0 || n > 65535 {
    return Err(errors.New("port out of range"))
  }
  Ok(n)
}
```

Go functions with a `(T, error)` return signature become `Result<T, error>` in Lisette.

📚 See [`09-error-handling.md`](09-error-handling.md)

### `Slice<T>`

A growable, indexable sequence of elements.

```rust
let nums = [1, 2, 3]
let empty: Slice<int> = []
let also_empty = Slice.new<int>()
```

```rust
let first = nums[0]
let safe = nums.get(0)           // Option<int>
let larger = nums.append(4)
let len = nums.length()
let cap = nums.capacity()
let empty = nums.is_empty()
let sum = nums.fold(0, |acc, x| acc + x)
let first_positive = nums.find(|x| x > 0)
```

Run `lis doc Slice` for the full method list.

### `Map<K, V>`

A map from keys to values.

```rust
let ages = Map.from([("Alice", 20), ("Bob", 25)])

let alice = ages.get("Alice")    // Option<int>
let direct = ages["Alice"]
ages.delete("Bob")
let size = ages.length()
```

Run `lis doc Map` for the full method list.

### `Ref<T>`

A reference (pointer) to a value. Created with `&`, dereferenced with `ref.*`.

```rust
let x = 42
let r: Ref<int> = &x
let value = r.*                  // 42
```

`Ref<T>` is guaranteed non-null. Nullable pointers from Go become `Option<Ref<T>>`.

📚 See [`07-pointers.md`](07-pointers.md)

### Tuples

Tuples hold 2 to 5 values of different types. Access elements by position.

```rust
let pair = (42, "hello")
let first = pair.0               // 42
let second = pair.1              // "hello"

let triple = (1, true, "three")
let (a, b, c) = triple          // destructuring
```

For more than 5 elements, use a struct with named fields.

### Unit type `()`

The implicit return type of functions that return no value. Written as `()`.

```rust
fn greet(name: string) {
  fmt.Println(f"hello, {name}")
}

// equivalent to:
fn greet(name: string) -> () {
  fmt.Println(f"hello, {name}")
}
```

### Never

The return type of functions that never return. A function returning `Never` must diverge, e.g. panic or loop forever.

```rust
fn fail(msg: string) -> Never {
  panic(msg)
}
```

Inhabitance propagates through composite types: a struct with a `Never` field is itself uninhabited, and an enum is uninhabited if all its variants are, e.g. a recursive enum with no base case.

## Type parameters

Type parameters appear in angle brackets. `Option<int>` means "an `Option` containing an `int`." `Map<string, int>` has two type parameters: key type and value type.

```rust
let scores = Map.new<string, int>() // Map<string, int>
```

Type parameters are inferred where possible:

```rust
let names = ["Alice", "Bob"]     // Slice<string>
let result = Ok(42)              // Result<int, E>
```

📚 See [`05-functions.md`](05-functions.md) and [`06-structs-and-enums.md`](06-structs-and-enums.md)

## Bindings

`let` creates an immutable binding.

```rust
let x = 42
let name = "Alice"
let items = [1, 2, 3]
```

`let mut` creates a mutable binding. Mutable bindings can be reassigned. Note that `let` makes the _binding_ immutable, but it does not prevent mutation through a `Ref<T>` pointer to the value. See [`07-pointers.md`](07-pointers.md)

```rust
let mut items = [1, 2, 3]
items = items.append(4)

let mut count = 0
count += 1
```

`const` defines a compile-time constant. As in Go, only primitive values are allowed: `bool`, `int`, `float`, `string`. The initializer must be a literal or an expression built from literals. `const` bindings are immutable and unaddressable.

```rust
const MAX_SIZE = 1024
const GREETING = "hello"
const DOUBLED = MAX_SIZE * 2
```

Composite values (tuples, structs, lists) cannot be `const`. Use a function that returns the value instead:

```rust
fn origin() -> Point {
  Point { x: 0, y: 0 }
}
```

Add a type annotation with `:` after the binding name. Annotations are optional on bindings; the type typically can inferred from the value. Function parameters require annotations.

```rust
let x: int = 42
let y = 42

fn add(a: int, b: int) -> int {
  a + b
}
```

Bindings can destructure tuples, structs, and enums. See [`08-pattern-matching.md`](08-pattern-matching.md)

```rust
let (x, y) = (10, 20)
let Point { x, y } = point
let (name, age, active) = ("Alice", 30, true)
```

## Type aliases

To alias a type:

```rust
type UserId = int
type Handler = fn(Request) -> Response
type StringMap<V> = Map<string, V>
```

Type aliases are transparent, i.e. alternative names for the same type.

```rust
type UserId = int

let id: UserId = 42
let n: int = id           // works: `UserId` is just `int`
```

For distinct types, use a tuple struct:

```rust
struct UserId(int)
struct OrderId(int)

let user = UserId(1)
let order = OrderId(2)
let n: int = user        // error: expected `int`, found `UserId`
```

## Casting

The `as` operator converts between types.

Numeric conversions:

```rust
let x: int = 42
let y = x as float64
let z = x as int8
```

String to bytes or runes:

```rust
let s = "hello"
let bytes = s as Slice<byte>
let runes = s as Slice<rune>
let back = bytes as string
```

Casts between incompatible types are disallowed.

```
  [error] Invalid cast
   ╭─[example.lis:2:9]
 2 │ let n = b as int
   ·         ────┬───
   ·             ╰── cannot cast `bool` to `int`
   ╰────
  help: Casts are only supported between numeric types, and between strings and byte/rune slices
```

<br>

<table><tr>
<td>← <a href="01-lexical-structure.md"><code>01-lexical-structure.md</code></a></td>
<td align="right"><a href="03-operators.md"><code>03-operators.md</code></a> →</td>
</tr></table>
