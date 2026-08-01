package receivermutation

import "sort"

type Values map[string][]string

func (v Values) Set(key, value string) { v[key] = []string{value} }
func (v Values) Get(key string) string { return v[key][0] }

type Counter struct{ n int }

func (c Counter) Increment() { c.n++ }
func (c Counter) Value() int { return c.n }

type Buffer struct{ data []byte }

func (b *Buffer) Append(p []byte) { b.data = append(b.data, p...) }

type Numbers []int

func (x Numbers) Len() int           { return len(x) }
func (x Numbers) Less(i, j int) bool { return x[i] < x[j] }
func (x Numbers) Swap(i, j int)      { x[i], x[j] = x[j], x[i] }
func (x Numbers) Sort()              { sort.Sort(x) }
func (x Numbers) Sorted() bool       { return sort.IsSorted(x) }
