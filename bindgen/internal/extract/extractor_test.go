package extract

import (
	"runtime"
	"strings"
	"testing"
)

func lastValue(env []string, key string) string {
	value := ""
	for _, entry := range env {
		if rest, ok := strings.CutPrefix(entry, key+"="); ok {
			value = rest
		}
	}
	return value
}

func TestPackageLoadConfigPinsTheZeroTargetToTheHost(t *testing.T) {
	t.Setenv("GOOS", "linux")
	t.Setenv("GOARCH", "amd64")

	env := packageLoadConfig("", "").Env

	if got := lastValue(env, "GOOS"); got != runtime.GOOS {
		t.Errorf("GOOS = %q, want %q", got, runtime.GOOS)
	}
	if got := lastValue(env, "GOARCH"); got != runtime.GOARCH {
		t.Errorf("GOARCH = %q, want %q", got, runtime.GOARCH)
	}
	if got := lastValue(env, "CGO_ENABLED"); got != "1" {
		t.Errorf("CGO_ENABLED = %q, want 1 for a host load", got)
	}
}

func TestPackageLoadConfigKeepsCgoOffForACrossTarget(t *testing.T) {
	goos := "windows"
	if runtime.GOOS == goos {
		goos = "linux"
	}

	env := packageLoadConfig(goos, "amd64").Env

	if got := lastValue(env, "GOOS"); got != goos {
		t.Errorf("GOOS = %q, want %q", got, goos)
	}
	if got := lastValue(env, "CGO_ENABLED"); got != "0" {
		t.Errorf("CGO_ENABLED = %q, want 0 for a cross load", got)
	}
}
