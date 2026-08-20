package cli

import (
	"reflect"
	"testing"
)

func TestPartitionByTargetKeepsAvailabilityWithSharedContent(t *testing.T) {
	targets := []Target{
		{goos: "darwin", goarch: "arm64"},
		{goos: "linux", goarch: "amd64"},
		{goos: "windows", goarch: "amd64"},
	}
	captured := map[Target]map[string]string{
		targets[0]: {"plugin": "content"},
		targets[1]: {"plugin": "content"},
		targets[2]: {},
	}

	got := partitionByTarget(captured, targets)["plugin"]

	if len(got.variants) != 1 || !reflect.DeepEqual(got.targets, targets[:2]) {
		t.Fatalf("shared package partition = %#v, want one variant on first two targets", got)
	}
}

func TestPartitionByTargetDoesNotMutateCapturedHeaderStub(t *testing.T) {
	targets := []Target{
		{goos: "linux", goarch: "amd64"},
		{goos: "windows", goarch: "amd64"},
	}
	captured := map[Target]map[string]string{
		targets[0]: {"log/syslog": "pub type Writer\n"},
		targets[1]: {"log/syslog": "// header only\n"},
	}

	got := partitionByTarget(captured, targets)["log/syslog"]

	if len(got.targets) != 1 || got.targets[0] != targets[0] {
		t.Fatalf("stubbed package targets = %v, want [%v]", got.targets, targets[0])
	}
	if _, ok := captured[targets[1]]["log/syslog"]; !ok {
		t.Fatal("partitionByTarget mutated its captured input")
	}
}

func TestParseTargetReadsOneTargetAndTreatsEmptyAsHost(t *testing.T) {
	target, err := ParseTarget("linux/amd64")
	if err != nil {
		t.Fatalf("ParseTarget failed: %v", err)
	}
	if target.goos != "linux" || target.goarch != "amd64" {
		t.Fatalf("ParseTarget = %v, want linux/amd64", target)
	}

	host, err := ParseTarget("")
	if err != nil {
		t.Fatalf("ParseTarget(\"\") failed: %v", err)
	}
	if host != (Target{}) {
		t.Fatalf("ParseTarget(\"\") = %v, want the zero target", host)
	}

	if _, err := ParseTarget("linux/amd64,darwin/arm64"); err == nil {
		t.Fatal("ParseTarget accepted a list")
	}
}

func TestParseTargetsRejectsInvalidAndDuplicateTargets(t *testing.T) {
	invalid := []string{
		"/amd64",
		"linux/",
		"linux/amd64/extra",
		"linux/amd64,linux/amd64",
	}
	for _, input := range invalid {
		t.Run(input, func(t *testing.T) {
			if _, err := ParseTargets(input); err == nil {
				t.Fatalf("ParseTargets(%q) succeeded", input)
			}
		})
	}
}
