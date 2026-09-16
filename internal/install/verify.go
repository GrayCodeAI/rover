package install

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
)

// Manifest mirrors SOURCE_MANIFEST.json.
type Manifest struct {
	Version string `json:"version"`
	Files   []struct {
		Path   string `json:"path"`
		SHA256 string `json:"sha256"`
		Size   int64  `json:"size"`
	} `json:"files"`
}

// Verify checks that every file listed in manifestPath exists under root and
// matches its recorded hash and size. It does not claim to be a signed public
// installer — it verifies a local checkout against its own manifest.
func Verify(root, manifestPath string) error {
	data, e := os.ReadFile(manifestPath)
	if e != nil {
		return fmt.Errorf("manifest read: %w", e)
	}
	var m Manifest
	if e = json.Unmarshal(data, &m); e != nil {
		return fmt.Errorf("manifest json: %w", e)
	}
	if len(m.Files) == 0 {
		return fmt.Errorf("empty manifest")
	}
	for _, f := range m.Files {
		if f.Path == "SOURCE_MANIFEST.json" {
			continue
		}
		if f.Path == "" || filepath.IsAbs(f.Path) || f.Path != filepath.Clean(f.Path) {
			return fmt.Errorf("invalid manifest path %q", f.Path)
		}
		p := filepath.Join(root, filepath.FromSlash(f.Path))
		if !isWithin(root, p) {
			return fmt.Errorf("manifest path escape %q", f.Path)
		}
		info, e := os.Lstat(p)
		if e != nil {
			return fmt.Errorf("missing file %s: %w", f.Path, e)
		}
		if !info.Mode().IsRegular() {
			return fmt.Errorf("not regular file %s", f.Path)
		}
		if info.Size() != f.Size {
			return fmt.Errorf("size mismatch %s: got %d want %d", f.Path, info.Size(), f.Size)
		}
		h, e := fileHash(p)
		if e != nil {
			return e
		}
		if h != f.SHA256 {
			return fmt.Errorf("hash mismatch %s", f.Path)
		}
	}
	return nil
}

func fileHash(p string) (string, error) {
	f, e := os.Open(p)
	if e != nil {
		return "", e
	}
	defer f.Close()
	h := sha256.New()
	if _, e = io.Copy(h, f); e != nil {
		return "", e
	}
	return hex.EncodeToString(h.Sum(nil)), nil
}

func isWithin(root, p string) bool {
	rel, e := filepath.Rel(root, p)
	if e != nil {
		return false
	}
	return rel != "." && rel != ".." && rel[:2] != ".."+string(filepath.Separator)
}
