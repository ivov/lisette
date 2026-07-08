package convert

import (
	"strconv"
	"strings"
)

// Constructors and predicates for the string-based Lisette type vocabulary.

func sliceOf(elem string) string { return "Slice<" + elem + ">" }
func arrayOf(elem string, n int64) string {
	return "Array<" + elem + ", " + strconv.FormatInt(n, 10) + ">"
}
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

func unwrapSlice(s string) (string, bool) {
	if strings.HasPrefix(s, "Slice<") && strings.HasSuffix(s, ">") {
		return s[len("Slice<") : len(s)-1], true
	}
	return "", false
}
