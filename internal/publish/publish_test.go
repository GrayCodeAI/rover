package publish

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/store"
)

func TestPublishLocal(t *testing.T) {
	s, e := store.Open(filepath.Join(t.TempDir(), "state"))
	if e != nil {
		t.Fatal(e)
	}
	defer s.Close()
	inv := model.Investigation{Schema: model.Schema, ID: "inv_test123", RoverVersion: model.Version, Decision: "REVIEW_REQUIRED"}
	if e = s.Put("investigation", inv.ID, inv, "test"); e != nil {
		t.Fatal(e)
	}
	dest := filepath.Join(t.TempDir(), "publish")
	out, e := Publish(s, inv.ID, dest)
	if e != nil {
		t.Fatal(e)
	}
	if _, e = os.Stat(out); e != nil {
		t.Fatal(e)
	}
	// Destination inside state must be rejected.
	if _, e = Publish(s, inv.ID, filepath.Join(s.Root, "inside")); e == nil {
		t.Fatal("publish inside state accepted")
	}
	// Invalid ID.
	if _, e = Publish(s, "bad id", dest); e == nil {
		t.Fatal("bad id accepted")
	}
}
