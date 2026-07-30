package divergence

func AlwaysPanics() { panic("boom") }

func MaybePanics(b bool) {
	if b {
		panic("boom")
	}
}

func Delegates() { AlwaysPanics() }
