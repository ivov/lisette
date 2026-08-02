package lisette

func ChannelSend[T any](ch chan T, value T) (sent bool) {
	if ch == nil {
		return false
	}
	defer func() {
		if r := recover(); r != nil {
			if err, ok := r.(error); ok && err.Error() == "send on closed channel" {
				sent = false
				return
			}
			panic(r)
		}
	}()
	ch <- value
	return true
}

func ChannelClose[T any](ch chan T) {
	if ch == nil {
		return
	}
	defer func() {
		if r := recover(); r != nil {
			if err, ok := r.(error); ok && err.Error() == "close of closed channel" {
				return
			}
			panic(r)
		}
	}()
	close(ch)
}

func SenderSend[T any](ch chan<- T, value T) (sent bool) {
	if ch == nil {
		return false
	}
	defer func() {
		if r := recover(); r != nil {
			if err, ok := r.(error); ok && err.Error() == "send on closed channel" {
				sent = false
				return
			}
			panic(r)
		}
	}()
	ch <- value
	return true
}

func SenderClose[T any](ch chan<- T) {
	if ch == nil {
		return
	}
	defer func() {
		if r := recover(); r != nil {
			if err, ok := r.(error); ok && err.Error() == "close of closed channel" {
				return
			}
			panic(r)
		}
	}()
	close(ch)
}

func ChannelReceive[T any](ch chan T) Option[T] {
	if ch == nil {
		return Option[T]{Tag: OptionNone}
	}
	v, ok := <-ch
	if ok {
		return Option[T]{Tag: OptionSome, SomeVal: v}
	}
	return Option[T]{Tag: OptionNone}
}

func ReceiverReceive[T any](ch <-chan T) Option[T] {
	if ch == nil {
		return Option[T]{Tag: OptionNone}
	}
	v, ok := <-ch
	if ok {
		return Option[T]{Tag: OptionSome, SomeVal: v}
	}
	return Option[T]{Tag: OptionNone}
}

func ChannelRange[T any](ch <-chan T) <-chan T {
	if ch != nil {
		return ch
	}
	closed := make(chan T)
	close(closed)
	return closed
}

func ChannelSplit[T any](ch chan T) Tuple2[chan<- T, <-chan T] {
	return Tuple2[chan<- T, <-chan T]{First: ch, Second: ch}
}
