// Fixture: a call through an interface method writes what the pins and the name rule of that method say.
package interface_call_writes

import "io"

// Writes through io.Reader.Read.
func ReadVia(r io.Reader, buf []byte) { r.Read(buf) }

type Codec interface {
	ReadBody(body any) error
}

// Writes through the pinned Codec.ReadBody.
func Decode(c Codec, reply any) error { return c.ReadBody(reply) }

// Inherits the write from Decode.
func DecodeBoth(c Codec, a, b any) {
	Decode(c, a)
	Decode(c, b)
}

type Peeker interface {
	Peek(buf []byte) int
}

// Stays read-only, since Peeker.Peek has no pin.
func PeekVia(p Peeker, buf []byte) int { return p.Peek(buf) }

type Base interface {
	Stash(buf []byte)
}

type Derived interface{ Base }

// Writes through the pinned Derived.Stash, since Base.Stash has no pin.
func ViaDerived(d Derived, buf []byte) { d.Stash(buf) }

type Shared interface {
	Keep(buf []byte)
}

type Guarded interface{ Shared }

// Stays read-only, since Guarded.Keep is pinned read-only.
func ViaGuarded(g Guarded, buf []byte) { g.Keep(buf) }

type Generic[T any] interface {
	Read(buf T)
}

type Bytes interface{ Generic[[]byte] }

// Writes through Bytes.Read, whose buf is a byte slice.
func ViaBytes(b Bytes, buf []byte) { b.Read(buf) }
