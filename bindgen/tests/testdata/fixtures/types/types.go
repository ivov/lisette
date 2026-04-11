package types

// Primitives

func GetBool() bool       { return false }
func GetInt() int         { return 0 }
func GetInt8() int8       { return 0 }
func GetInt16() int16     { return 0 }
func GetInt32() int32     { return 0 }
func GetInt64() int64     { return 0 }
func GetUint() uint       { return 0 }
func GetUint8() uint8     { return 0 }
func GetUint16() uint16   { return 0 }
func GetUint32() uint32   { return 0 }
func GetUint64() uint64   { return 0 }
func GetFloat32() float32 { return 0 }
func GetFloat64() float64 { return 0 }
func GetString() string   { return "" }
func GetRune() rune       { return 0 } // → int32
func GetByte() byte       { return 0 } // → uint8
func GetUintptr() uintptr { return 0 } // → uint

// Pointers

type T struct{ X int }

func GetIntPtr() *int     { return nil }
func GetStructPtr() *T    { return nil }
func GetDoublePtr() **int { return nil }

// Slices and arrays

func GetIntSlice() []int       { return nil }
func GetStringSlice() []string { return nil }
func GetNestedSlice() [][]byte { return nil }
func GetArray() [10]int        { return [10]int{} } // arrays also become Slice

// Maps

func GetStringToInt() map[string]int         { return nil }
func GetIntToInterface() map[int]interface{} { return nil }
func GetNested() map[string]map[int]bool     { return nil }

// Channels

func GetBiDir() chan int         { return nil }
func GetRecvOnly() <-chan string { return nil }
func GetSendOnly() chan<- bool   { return nil }
