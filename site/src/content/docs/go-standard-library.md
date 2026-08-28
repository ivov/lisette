---
title: "Go standard library"
description: "Importing Go standard library packages with the go: prefix"
---

To import a package from the Go standard library:

```lisette
import "go:fmt"
import "go:strings"

fn main() {
  let slug = "Hello World"
    |> strings.ToLower
    |> strings.ReplaceAll(" ", "-")

  // !callout-right prints `"hello-world"`
  fmt.Println(slug)
}
```

To import a Go package for its side effects only, use a blank import:

```lisette
// !callout-right registers PNG decoder
import _ "go:image/png"
```
