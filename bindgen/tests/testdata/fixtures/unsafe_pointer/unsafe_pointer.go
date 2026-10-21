// Fixture: `unsafe.Pointer` renders as the opaque `unsafe.Pointer` type, not `Unknown`.
package unsafe_pointer

import "unsafe"

type Handle unsafe.Pointer

type Slot struct {
	Target unsafe.Pointer
}

func Load(addr *unsafe.Pointer) unsafe.Pointer { return *addr }

func Wrap(p unsafe.Pointer) Handle { return Handle(p) }
