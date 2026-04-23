# Pointers

`Ref<T>` is a reference (pointer) to a value of type `T`.

## Creating references

Use `&` to create a reference:

```rust
let x = 42
let r = &x          // Ref<int>
```

References can be taken to variables, fields, and slice elements:

```rust
let p = Point { x: 10, y: 20 }
let rx = &p.x       // Ref<int>

let nums = [1, 2, 3]
let first = &nums[0] // Ref<int>
```

Some expressions cannot be referenced:

```
  [error] Non-addressable expression
   ╭─[example.lis:1:9]
 1 │ let r = &42
   ·          ─┬
   ·           ╰── cannot take address of literal
   ╰────
  help: Assign the value to a variable first, then take its address
```

`const` bindings are also not addressable. Copy the value into a local `let` first if you need a reference:

```rust
const N = 42

let x = N
let r = &x          // ok
```

## Dereferencing

Use `.*` to access the referenced value:

```rust
let x = 42
let r = &x
let value = r.*     // 42
```

## Mutation

To mutate a referenced value, assign to its dereference:

```rust
fn increment(r: Ref<int>) {
  r.* = r.* + 1
}

let mut x = 1
increment(&x)
// x is now 2
```

Mutation through a `Ref<T>` does not require `mut` on either the reference binding or the original binding. `Ref<T>` is a Go pointer, so it can always write to its target. This escape hatch does not apply to `const` bindings, since `&CONST` is rejected at compile time.

## Nil safety

`Ref<T>` is guaranteed non-null. There is no way to create a null reference in Lisette.

Go functions that return nullable pointers become `Option<Ref<T>>`:

```rust
// Go: func FindUser(id int) *User (returns nil if not found)
let user = FindUser(42)  // Option<Ref<User>>

if let Some(u) = user {
  fmt.Println(u.Name)
}
```

📚 See [`13-go-interop.md`](13-go-interop.md)

## Auto-coercion

When calling methods, Lisette automatically adds `&` or `.*` as needed. A value can call a `Ref<T>` method without explicit `&`, and a reference can call a value method without explicit `.*`:

```rust
let mut r = Rectangle { width: 10.0, height: 5.0 }
let ref = &r
let a = ref.area()  // auto-dereferenced: no need for `ref.*`
r.scale(2.0)        // auto-addressed: no need for `&r`
```

This means you rarely need to think about `&` or `.*` at method call sites.

📚 See [`10-methods.md`](10-methods.md)

<br>

<table><tr>
<td>← <a href="06-structs-and-enums.md"><code>06-structs-and-enums.md</code></a></td>
<td align="right"><a href="08-pattern-matching.md"><code>08-pattern-matching.md</code></a> →</td>
</tr></table>
