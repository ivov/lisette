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
