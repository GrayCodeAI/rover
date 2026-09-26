package model

import (
	"encoding/json"
	"os"
	"path/filepath"
	"regexp"
	"runtime"
	"strconv"
	"strings"
	"testing"
)

var versionPattern = regexp.MustCompile(`^[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$`)
var releaseHeading = regexp.MustCompile(`(?m)^## ([0-9][^\n]*)$`)
var toolchainPattern = regexp.MustCompile(`^go([0-9]+)\.([0-9]+)\.([0-9]+)`)

const minimumGoVersion = "1.26.6"

func repositoryFile(t *testing.T, name string) []byte {
	t.Helper()
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("runtime.Caller failed")
	}
	path := filepath.Clean(filepath.Join(filepath.Dir(file), "..", "..", name))
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read %s: %v", name, err)
	}
	return data
}

func TestVersionSurfaces(t *testing.T) {
	expected := strings.TrimSpace(string(repositoryFile(t, "VERSION")))
	if !versionPattern.MatchString(expected) {
		t.Fatalf("VERSION = %q, want a valid release version", expected)
	}
	if Version != expected {
		t.Fatalf("model.Version = %q, want %q", Version, expected)
	}
	if strings.TrimSpace(Commit) == "" {
		t.Fatal("model.Commit must not be empty")
	}

	var capabilities struct {
		Version string `json:"version"`
	}
	if err := json.Unmarshal(repositoryFile(t, "docs/capabilities.json"), &capabilities); err != nil {
		t.Fatalf("capabilities JSON: %v", err)
	}
	if capabilities.Version != expected {
		t.Fatalf("capabilities version = %q, want %q", capabilities.Version, expected)
	}

	var manifest struct {
		Version string `json:"version"`
	}
	if err := json.Unmarshal(repositoryFile(t, "SOURCE_MANIFEST.json"), &manifest); err != nil {
		t.Fatalf("manifest JSON: %v", err)
	}
	if manifest.Version != expected {
		t.Fatalf("manifest version = %q, want %q", manifest.Version, expected)
	}

	readme := string(repositoryFile(t, "README.md"))
	if !strings.Contains(readme, "version-"+expected+"-blue") {
		t.Fatalf("README version badge does not match %q", expected)
	}
	if !strings.Contains(readme, "/releases/tag/v"+expected) {
		t.Fatalf("README release link does not match %q", expected)
	}

	roadmap := string(repositoryFile(t, "ROADMAP.md"))
	if !strings.Contains(roadmap, "## Delivered in "+expected) {
		t.Fatalf("ROADMAP current release heading does not match %q", expected)
	}

	status := strings.TrimSpace(string(repositoryFile(t, "STATUS.md")))
	if !strings.HasPrefix(status, "# Rover implementation status — "+expected) {
		t.Fatalf("STATUS current heading does not match %q", expected)
	}

	changelog := string(repositoryFile(t, "CHANGELOG.md"))
	headings := releaseHeading.FindAllStringSubmatch(changelog, -1)
	if len(headings) == 0 || strings.TrimSpace(headings[0][1]) != expected {
		t.Fatalf("CHANGELOG current release heading does not match %q", expected)
	}

	release := string(repositoryFile(t, ".github/workflows/release.yml"))
	if !strings.Contains(release, "Version tag (e.g. v"+expected+")") {
		t.Fatalf("release workflow version example does not match %q", expected)
	}
	if !strings.Contains(release, "make version-check") {
		t.Fatal("release workflow must run make version-check")
	}

	goMod := string(repositoryFile(t, "go.mod"))
	if !strings.Contains(goMod, "go "+minimumGoVersion) {
		t.Fatalf("go.mod must require Go %s", minimumGoVersion)
	}
	makefile := string(repositoryFile(t, "Makefile"))
	if !strings.Contains(makefile, "GOTOOLCHAIN ?= go"+minimumGoVersion+"+auto") {
		t.Fatalf("Makefile must select Go %s or newer", minimumGoVersion)
	}
	if !strings.Contains(makefile, "govulncheck@v1.8.0") {
		t.Fatal("Makefile must pin govulncheck")
	}
	for _, workflow := range []string{".github/workflows/ci.yml", ".github/workflows/release.yml"} {
		text := string(repositoryFile(t, workflow))
		if !strings.Contains(text, "go-version: '"+minimumGoVersion+"'") {
			t.Fatalf("%s must pin Go %s", workflow, minimumGoVersion)
		}
	}
}

func TestSDKSourceTreeRemoved(t *testing.T) {
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("runtime.Caller failed")
	}
	path := filepath.Clean(filepath.Join(filepath.Dir(file), "..", "..", "sdk"))
	if _, err := os.Stat(path); !os.IsNotExist(err) {
		t.Fatalf("SDK source tree must be removed; stat %s returned %v", path, err)
	}
}

func TestToolchainMinimum(t *testing.T) {
	match := toolchainPattern.FindStringSubmatch(runtime.Version())
	if len(match) != 4 {
		t.Fatalf("runtime.Version() = %q", runtime.Version())
	}
	current := [3]int{}
	for i := range current {
		value, err := strconv.Atoi(match[i+1])
		if err != nil {
			t.Fatal(err)
		}
		current[i] = value
	}
	minimum := [3]int{1, 26, 6}
	for i := range minimum {
		if current[i] > minimum[i] {
			return
		}
		if current[i] < minimum[i] {
			t.Fatalf("Go toolchain %s is below minimum %d.%d.%d", runtime.Version(), minimum[0], minimum[1], minimum[2])
		}
	}
}
