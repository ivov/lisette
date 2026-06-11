package sealed

type Sealed interface {
	Do() int
	private()
}

type FullySealed interface {
	onlyPrivate()
}

type Open interface {
	Do() int
}

type SealedImpl struct{}

func (SealedImpl) Do() int  { return 0 }
func (SealedImpl) private() {}

// private has a pointer receiver, so only *SealedPtrImpl satisfies Sealed.
type SealedPtrImpl struct{}

func (SealedPtrImpl) Do() int   { return 0 }
func (*SealedPtrImpl) private() {}
