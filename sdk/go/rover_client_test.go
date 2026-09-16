package rover

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

func TestArgvAndFailedDecisionAreNotHidden(t *testing.T) {
	dir := t.TempDir()
	fixture := filepath.Join(dir, "fixture")
	script := "#!/usr/bin/env python3\nimport json,sys\nprint(json.dumps({\"decision\":\"BLOCKED\",\"argv\":sys.argv[1:]}))\n"
	if err := os.WriteFile(fixture, []byte(script), 0700); err != nil {
		t.Fatal(err)
	}
	client, err := New(fixture, filepath.Join(dir, "state"))
	if err != nil {
		t.Fatal(err)
	}
	r, err := client.Call([]string{"inspect", "--repo", "space ; $(not-a-shell)"})
	if err != nil {
		t.Fatal(err)
	}
	if r.ExitCode != 0 {
		t.Fatalf("exit %d", r.ExitCode)
	}
	if r.Decision() != "BLOCKED" {
		t.Fatal(r.Data)
	}
	b, _ := json.Marshal(r.Data)
	if !contains(string(b), "space ; $(not-a-shell)") {
		t.Fatal(string(b))
	}
}

func TestNonJSONRejected(t *testing.T) {
	dir := t.TempDir()
	p := filepath.Join(dir, "fixture")
	if err := os.WriteFile(p, []byte("#!/bin/sh\nprintf not-json\n"), 0700); err != nil {
		t.Fatal(err)
	}
	client, _ := New(p, filepath.Join(dir, "state"))
	if _, err := client.Call([]string{"status"}); err == nil {
		t.Fatal("non-JSON accepted")
	}
}

func contains(s, sub string) bool {
	return len(s) >= len(sub) && (func() bool {
		for i := 0; i <= len(s)-len(sub); i++ {
			if s[i:i+len(sub)] == sub {
				return true
			}
		}
		return false
	})()
}
