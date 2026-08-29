---
title: "Interfaces"
description: "Structural typing, embedding, generic interfaces, exact type arguments, collections, nil Go interfaces"
---

An interface is a set of method signatures. A type satisfies an interface by implementing all its methods. No explicit declaration is needed.

```lisette
interface Shape {
  fn area() -> int
}

struct Rectangle {
  width: int,
  height: int,
}

// !callout-above[/Rectangle/] `Rectangle` implements `area()` so it satisfies `Shape`
impl Rectangle {
  fn area(self) -> int {
    self.width * self.height
  }
}

fn describe(shape: Shape) -> string {
  f"area is {shape.area()}"
}

// !callout-right `Rectangle` is accepted as a `Shape`
describe(Rectangle { width: 10, height: 5 })
```

## Embedding

An interface can embed other interfaces, composing their methods into one:

```lisette
interface Reader {
  fn read(buf: Slice<byte>) -> Result<int, error>
}

interface Writer {
  fn write(buf: Slice<byte>) -> Result<int, error>
}

// !callout-above[/ReadWriter/] satisfied only by a type with both `read()` and `write()`
interface ReadWriter {
  embed Reader
  embed Writer
}
```

## Generic interfaces

Interfaces accept type parameters:

```lisette
interface Iterator<T> {
  fn next() -> Option<T>
}

struct Counter {
  current: int,
  end: int,
}

// !callout-above[/Counter/] satisfies `Iterator<T>`
impl Counter {
  fn next(self: mut Ref<Counter>) -> Option<int> {
    if self.current >= self.end {
      return None
    }
    let value = self.current
    self.current += 1
    Some(value)
  }
}
```

## Exact type arguments

Type arguments must match exactly, as in Go. Even if `Cat` satisfies `Animal`, a `Box<Cat>` is not a `Box<Animal>`.

```lisette
interface Animal {
  fn speak() -> string
}

struct Box<T> {
  value: T,
}

impl<T> Box<T> {
  fn get(self) -> T { self.value }
}

fn take_box(box: Box<Animal>) {
  let _ = box.get()
}

struct Cat {}

impl Cat {
  fn speak(self) -> string {
    "meow"
  }
}

let cat_box = Box { value: Cat {} }
// !callout-error-right error: expected `Box<Animal>`, found `Box<Cat>`
take_box(cat_box)
```

## Mixed collections

A `Slice<Animal>` can hold a mix of different types, so long as each one satisfies `Animal`:

```lisette
struct Dog {}

impl Dog {
  fn speak(self) -> string {
    "woof"
  }
}

let pets: Slice<Animal> = [Cat {}, Dog {}]
```

A bound does the opposite. Every element must be the same concrete type:

```lisette
fn speak_all<T: Animal>(pets: Slice<T>) -> string {
  pets.fold("", |sounds, pet| sounds + pet.speak())
}

let cats = [Cat {}, Cat {}]
// !callout-right `T` is `Cat`
speak_all(cats)
```

## Nil Go interfaces

A Go interface holds [a type and a value](https://go.dev/doc/faq#nil_error), so it has three states, not two:

```go file="handler.go"
var p *MyHandler = nil
// !callout-right untyped nil
var h1 http.Handler
// !callout-right typed nil
var h2 http.Handler = p
// !callout-right non-nil
var h3 http.Handler = &MyHandler{}
```

A literal encoding would distinguish all three:

```lisette
match handler {
  None => fmt.Println("untyped nil"),
  Some(None) => fmt.Println("typed nil"),
  Some(Some(h)) => h.ServeHTTP(w, r),
}
```

Lisette collapses both typed nil and untyped nil into `None`. Neither state holds a value, and a method call on either panics once it touches the receiver. Go allows one exception: a method that never reads through its receiver runs on a typed nil. Lisette treats this exception as too rare in practice to model.

```lisette
match handler {
  Some(h) => h.ServeHTTP(w, r),
  None => fmt.Println("no handler"),
}
```
