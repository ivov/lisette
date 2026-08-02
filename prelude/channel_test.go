package lisette

import "testing"

func recoveredError(fn func()) (recovered error) {
	defer func() {
		if r := recover(); r != nil {
			recovered, _ = r.(error)
		}
	}()
	fn()
	return nil
}

func TestChannelSendReceive(t *testing.T) {
	ch := make(chan int, 1)
	if !ChannelSend(ch, 42) {
		t.Fatal("expected send on open channel to return true")
	}
	got := ChannelReceive(ch)
	if got.IsNone() {
		t.Fatal("expected Some from receive")
	}
	if got.SomeVal != 42 {
		t.Fatalf("expected 42, got %d", got.SomeVal)
	}
}

func TestChannelReceiveNoneAfterClose(t *testing.T) {
	ch := make(chan int)
	close(ch)
	if ChannelReceive(ch).IsSome() {
		t.Fatal("expected None receiving from closed channel")
	}
}

func TestChannelSendReturnsFalseAfterClose(t *testing.T) {
	ch := make(chan int)
	close(ch)
	if ChannelSend(ch, 42) {
		t.Fatal("expected send on closed channel to return false")
	}
}

func TestChannelCloseIsIdempotent(t *testing.T) {
	ch := make(chan int)
	ChannelClose(ch)
	ChannelClose(ch)
	if ChannelSend(ch, 42) {
		t.Fatal("expected send after double close to return false")
	}
}

func TestSenderSendReturnsFalseAfterClose(t *testing.T) {
	ch := make(chan int)
	close(ch)
	if SenderSend[int](ch, 42) {
		t.Fatal("expected send on closed channel to return false")
	}
}

func TestSenderCloseIsIdempotent(t *testing.T) {
	ch := make(chan int)
	SenderClose[int](ch)
	SenderClose[int](ch)
	if SenderSend[int](ch, 42) {
		t.Fatal("expected send after double close to return false")
	}
}

func TestChannelSplitSendReceive(t *testing.T) {
	ch := make(chan int, 1)
	split := ChannelSplit(ch)
	if !SenderSend(split.First, 7) {
		t.Fatal("expected send on split sender to return true")
	}
	got := ReceiverReceive(split.Second)
	if got.IsNone() {
		t.Fatal("expected Some from split receiver")
	}
	if got.SomeVal != 7 {
		t.Fatalf("expected 7, got %d", got.SomeVal)
	}
}

func TestNilChannelReadsAsClosed(t *testing.T) {
	var ch chan int
	if ChannelReceive(ch).IsSome() {
		t.Fatal("expected None receiving from a nil channel")
	}
	if ReceiverReceive[int](ch).IsSome() {
		t.Fatal("expected None receiving from a nil receiver")
	}
	if ChannelSend(ch, 42) {
		t.Fatal("expected send on a nil channel to return false")
	}
	if SenderSend[int](ch, 42) {
		t.Fatal("expected send on a nil sender to return false")
	}
	ChannelClose(ch)
	SenderClose[int](ch)
}

func TestChannelRangeOverNilEndsImmediately(t *testing.T) {
	var ch chan int
	count := 0
	for range ChannelRange[int](ch) {
		count++
	}
	if count != 0 {
		t.Fatalf("expected no iterations over a nil channel, got %d", count)
	}
}

func TestChannelRangePassesLiveChannelThrough(t *testing.T) {
	ch := make(chan int, 2)
	ch <- 1
	ch <- 2
	close(ch)
	var got []int
	for v := range ChannelRange[int](ch) {
		got = append(got, v)
	}
	if len(got) != 2 || got[0] != 1 || got[1] != 2 {
		t.Fatalf("expected [1 2], got %v", got)
	}
}

func TestSendOnClosedChannelPanicText(t *testing.T) {
	ch := make(chan int)
	close(ch)
	err := recoveredError(func() { ch <- 1 })
	if err == nil {
		t.Fatal("expected a panic sending on a closed channel")
	}
	if err.Error() != "send on closed channel" {
		t.Fatalf("Go reworded the send-on-closed panic to %q, update the literal in ChannelSend and SenderSend", err.Error())
	}
}

func TestCloseOfClosedChannelPanicText(t *testing.T) {
	ch := make(chan int)
	close(ch)
	err := recoveredError(func() { close(ch) })
	if err == nil {
		t.Fatal("expected a panic closing a closed channel")
	}
	if err.Error() != "close of closed channel" {
		t.Fatalf("Go reworded the close-of-closed panic to %q, update the literal in ChannelClose and SenderClose", err.Error())
	}
}
