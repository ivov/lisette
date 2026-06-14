package convert

import "strings"

// Lisette types are represented as plain strings. These constructors and
// predicates keep the generic vocabulary (the prefixes, separators, and the
// trailing `, error`) in one place instead of being spelled out at each call.

func sliceOf(elem string) string    { return "Slice<" + elem + ">" }
func optionOf(elem string) string   { return "Option<" + elem + ">" }
func refOf(elem string) string      { return "Ref<" + elem + ">" }
func mapOf(key, val string) string  { return "Map<" + key + ", " + val + ">" }
func channelOf(elem string) string  { return "Channel<" + elem + ">" }
func senderOf(elem string) string   { return "Sender<" + elem + ">" }
func receiverOf(elem string) string { return "Receiver<" + elem + ">" }
func varArgsOf(elem string) string  { return "VarArgs<" + elem + ">" }
func resultOf(ok string) string     { return "Result<" + ok + ", error>" }
func partialOf(ok string) string    { return "Partial<" + ok + ", error>" }

func isSliceType(s string) bool { return strings.HasPrefix(s, "Slice<") }
func isMapType(s string) bool   { return strings.HasPrefix(s, "Map<") }

// unwrapSlice returns the element type of a `Slice<...>` string.
func unwrapSlice(s string) (string, bool) {
	if strings.HasPrefix(s, "Slice<") && strings.HasSuffix(s, ">") {
		return s[len("Slice<") : len(s)-1], true
	}
	return "", false
}
