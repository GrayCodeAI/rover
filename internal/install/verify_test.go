package install

import (
	"os"
	"path/filepath"
	"testing"
)

func TestVerifyInstall(t *testing.T) {
	root := t.TempDir()
	manifest := filepath.Join(root, "SOURCE_MANIFEST.json")
	// Create a minimal manifest for two files.
	contentA := "hello"
	contentB := "world"
	if e := os.WriteFile(filepath.Join(root, "a.txt"), []byte(contentA), 0600); e != nil {
		t.Fatal(e)
	}
	if e := os.MkdirAll(filepath.Join(root, "sub"), 0700); e != nil {
		t.Fatal(e)
	}
	if e := os.WriteFile(filepath.Join(root, "sub/b.txt"), []byte(contentB), 0600); e != nil {
		t.Fatal(e)
	}
	hashA, _ := fileHash(filepath.Join(root, "a.txt"))
	hashB, _ := fileHash(filepath.Join(root, "sub/b.txt"))
	manifestData := `{"version":"test","files":[{"path":"a.txt","sha256":"` + hashA + `","size":5},{"path":"sub/b.txt","sha256":"` + hashB + `","size":5}]}`
	if e := os.WriteFile(manifest, []byte(manifestData), 0600); e != nil {
		t.Fatal(e)
	}
	if e := Verify(root, manifest); e != nil {
		t.Fatal(e)
	}
	// Tamper.
	if e := os.WriteFile(filepath.Join(root, "a.txt"), []byte("tamper"), 0600); e != nil {
		t.Fatal(e)
	}
	if e := Verify(root, manifest); e == nil {
		t.Fatal("tampered file accepted")
	}
}

func TestIsWithinShortNames(t *testing.T) {
	root := t.TempDir()
	// Regression: single-character relative paths must not panic the slice check.
	if !isWithin(root, filepath.Join(root, "a")) {
		t.Fatal("single-char child rejected")
	}
	if isWithin(root, filepath.Join(root, "..", "escape")) {
		t.Fatal("escape accepted")
	}
	if isWithin(root, root) {
		t.Fatal("root itself accepted as within")
	}
}
