---
title: "Interfaces"
description: "Structural typing, embedding, generic interfaces, interfaces as type arguments, nil Go interfaces"
---

An interface is a set of method signatures. A type satisfies an interface by implementing all its methods. No explicit declaration is needed.

```lisette
interface Shape {
  fn area() -> int
}
```

```lisette
struct Rectangle {
  width: int,
  height: int,
}

// !callout-above[/Rectangle/] `Rectangle` satisfies `Shape`
impl Rectangle {
  fn area(self) -> int {
    self.width * self.height
  }
}
```

```lisette
// !callout-ok-right
let shape: Shape = Rectangle { width: 10, height: 5 }
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

// !callout-above[/ReadWriter/] satisfaction requires both `read()` and `write()`
interface ReadWriter {
  embed Reader
  embed Writer
}
```

## Generic interfaces

Interfaces accept [type parameters](/docs/types/#type-parameters):

```lisette
interface Iterator<T> {
  fn next() -> Option<T>
}

struct Counter {
  current: int,
  end: int,
}

// !callout-above[/Counter/] `Ref<Counter>` satisfies `Iterator<int>`
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

## Interfaces as type arguments

An interface can be a type argument, like any other type. For example, a `Slice<Animal>` can hold any mix of types that satisfy `Animal`:

```lisette
interface Animal {
  fn speak() -> string
}
```

```lisette
struct Cat {}

impl Cat {
  fn speak(self) -> string {
    "meow"
  }
}

struct Dog {}

impl Dog {
  fn speak(self) -> string {
    "woof"
  }
}
```

```lisette
// !callout-ok-right
let pets: Slice<Animal> = [Cat {}, Dog {}]
```

A value whose type satisfies an interface is accepted as that interface. A type argument is not accepted in the same way, because type arguments must match exactly, as in Go.

```lisette
struct Box<T> {
  value: T,
}

impl<T> Box<T> {
  fn get(self) -> T { self.value }
}
```

```lisette
fn open(box: Box<Animal>) -> string {
  box.get().speak()
}
```

```lisette
let cat_box = Box { value: Cat {} }
// !callout-error-right error: expected `Box<Animal>`, found `Box<Cat>`
open(cat_box)
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
