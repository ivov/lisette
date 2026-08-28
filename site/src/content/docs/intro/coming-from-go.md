---
title: "Coming from Go"
description: "Go patterns and their Lisette equivalents"
---

Every section below is a Go pattern with a Lisette equivalent.

## Bindings

```go file="bindings.go"
count := 5
total := 0
total += count
```

```lisette file="bindings.lis"
let count = 5
let mut total = 0
total += count
```

📚 See [Bindings](/docs/bindings/)

## Functions

```go file="add.go"
func add(a int, b int) int {
    return a + b
}
```

```lisette file="add.lis"
fn add(a: int, b: int) -> int {
  a + b
}
```

📚 See [Functions](/docs/functions/)

## Lambdas

```go file="sort.go"
sort.Slice(scores, func(i, j int) bool {
    return scores[i] < scores[j]
})
```

```lisette file="sort.lis"
sort.Slice(scores, |i, j| scores[i] < scores[j])
```

📚 See [Lambdas](/docs/functions/#lambdas)

## Pipeline

```go file="slug.go"
trimmed := strings.TrimSpace("  Hello World  ")
lowered := strings.ToLower(trimmed)
result := strings.ReplaceAll(lowered, " ", "-")
```

```lisette file="slug.lis"
let result = "  Hello World  "
  |> strings.TrimSpace
  |> strings.ToLower
  |> strings.ReplaceAll(" ", "-")
```

📚 See [Operators](/docs/operators/#expression-operators)

## Loops

```go file="loops.go"
for i := 0; i < 10; i++ {
    fmt.Println(i)
}

for _, line := range lines {
    fmt.Println(line)
}

for {
    if queue.IsEmpty() { break }
}
```

```lisette file="loops.lis"
for i in 0..10 {
  fmt.Println(i)
}

for line in lines {
  fmt.Println(line)
}

loop {
  if queue.is_empty() { break }
}
```

📚 See [Control flow](/docs/control-flow/)

## Structs

```go file="user.go"
type User struct {
    Name  string
    email string
}

u := User{Name: "Alice", email: "a@b.com"}
```

```lisette file="user.lis"
struct User {
  pub name: string,
  email: string,
}

let u = User { name: "Alice", email: "a@b.com" }
```

📚 See [Structs](/docs/structs/)

## Pointers

```go file="pointer.go"
func rename(u *User, name string) {
    u.Name = name
}

user := User{Name: "Alice", email: "a@b.com"}
rename(&user, "Bob")
```

```lisette file="pointer.lis"
fn rename(u: mut Ref<User>, name: string) {
  u.name = name
}

let mut user = User { name: "Alice", email: "a@b.com" }
rename(&user, "Bob")
```

📚 See [References](/docs/references/)

## Enums

```go file="severity.go"
type Severity int

const (
    Low Severity = iota
    High
    Critical
)
```

```lisette file="severity.lis"
enum Severity { Low, High, Critical }
```

A `match` on an enum must cover every variant.

```go file="severity.go"
switch s {
case Low, High:
    fmt.Println("ignore")
case Critical:
    fmt.Println("alert")
}
```

```lisette file="severity.lis"
match s {
  Low | High => fmt.Println("ignore"),
  Critical => fmt.Println("alert"),
}
```

📚 See [Enums](/docs/enums/) and [Pattern matching](/docs/pattern-matching/)

## Collections

```go file="collections.go"
nums := []int{1, 2, 3}
nums = append(nums, 4)

buf := make([]byte, 1024)

ages := make(map[string]int)
ages["Alice"] = 20
age, ok := ages["Bob"]
```

```lisette file="collections.lis"
let nums = [1, 2, 3]
let nums = nums.append(4)

let buf = Slice.make<byte>(1024)

let mut ages = Map.new<string, int>()
ages["Alice"] = 20
// !callout-right `Option<int>`
let age = ages.get("Bob")
```

📚 See [Slice](/docs/prelude/slice/), [Map](/docs/prelude/map/) and [Array](/docs/prelude/array/)

## Methods

```go file="rectangle.go"
func (r Rectangle) Area() float64 {
    return r.Width * r.Height
}

func (r *Rectangle) Scale(factor float64) {
    r.Width *= factor
    r.Height *= factor
}
```

```lisette file="rectangle.lis"
impl Rectangle {
  fn area(self) -> float64 {
    self.width * self.height
  }

  fn scale(self: mut Ref<Rectangle>, factor: float64) {
    self.width *= factor
    self.height *= factor
  }
}
```

📚 See [Methods](/docs/methods/)

## Interfaces

```go file="reader.go"
type Reader interface {
    Read(p []byte) (n int, err error)
}
```

```lisette file="reader.lis"
interface Reader {
  fn Read(p: mut Slice<byte>) -> Partial<int, error>
}
```

📚 See [Interfaces](/docs/interfaces/)

## Error handling

```go file="handle.go"
bytes, err := os.ReadFile(path)
if err != nil {
    return Config{}, err
}
return parseConfig(bytes)
```

```lisette file="handle.lis"
let bytes = os.ReadFile(path)?
parse_config(bytes)
```

The `?` operator unwraps `Ok` or returns early with `Err`. Functions returning `(T, error)` in Go become `Result<T, error>` in Lisette.

📚 See [Failures](/docs/failures/)

## Absence handling

```go file="lookup.go"
// !callout-right pointer can be `nil`
var user *User
if user != nil {
    fmt.Println(user.Name)
}
```

```lisette file="lookup.lis"
// !callout-right `Option<Ref<User>>`
let user = get_user(id)
if let Some(u) = user {
  fmt.Println(u.name)
}
```

`Ref<T>` is guaranteed non-nil. Nilable pointers become `Option<Ref<T>>`.

📚 See [Option](/docs/prelude/option/) and [Safety](/docs/intro/safety/#absent-values)

## Concurrency

```go file="worker.go"
ch := make(chan int)
go func() {
    ch <- 42
}()
v := <-ch
```

```lisette file="worker.lis"
let ch = Channel.new<int>()
task ch.send(42)
// !callout-right `Option<int>`
let v = ch.receive()
```

📚 See [Concurrency](/docs/concurrency/)
