package attestation

import (
	"context"
	"crypto/ed25519"
	"os"
	"path/filepath"
	"testing"

	"github.com/GrayCodeAI/rover/internal/assurance"
	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/source"
	"github.com/GrayCodeAI/rover/internal/testutil"
)

func TestSignedEvidenceAndTrustKey(t *testing.T) {
	repo, s := testutil.Repo(t, map[string]string{"a": "b"})
	snap, e := source.Capture(context.Background(), s, repo, "HEAD", false)
	if e != nil {
		t.Fatal(e)
	}
	c := model.Config{Schema: model.Schema, Checks: []model.CheckSpec{{ID: "command", Argv: []string{"true"}, Parser: "exit-code", Required: true, Timeout: "1s"}}}
	in, e := assurance.Verify(context.Background(), s, snap, snap, c, assurance.Options{Mode: "local-advisory", AllowLocal: true})
	if e != nil {
		t.Fatal(e)
	}
	pr, pu, e := Keypair()
	if e != nil {
		t.Fatal(e)
	}
	key, e := Private(pr)
	if e != nil {
		t.Fatal(e)
	}
	pub, e := Public(pu)
	if e != nil {
		t.Fatal(e)
	}
	env, e := Sign(s, in.ID, key)
	if e != nil {
		t.Fatal(e)
	}
	p, e := Verify(env, pub)
	if e != nil || p.Investigation.ID != in.ID {
		t.Fatal(p, e)
	}
	_, other, _ := Keypair()
	wrong, _ := Public(other)
	if _, e = Verify(env, wrong); e == nil {
		t.Fatal("accepted untrusted key")
	}
	bad := env
	bad.Payload = append([]byte(nil), env.Payload...)
	bad.Payload[len(bad.Payload)-1] ^= 1
	if _, e = Verify(bad, pub); e == nil {
		t.Fatal("accepted changed payload")
	}
	bad = env
	bad.Signature = ed25519.Sign(key, env.Payload)
	if _, e = Verify(bad, pub); e == nil {
		t.Fatal("missing signing domain separation")
	}
	keypath := filepath.Join(t.TempDir(), "key")
	os.WriteFile(keypath, pr, 0600)
	if _, e = ReadKey(keypath, true); e != nil {
		t.Fatal(e)
	}
	os.Chmod(keypath, 0644)
	if _, e = ReadKey(keypath, true); e == nil {
		t.Fatal("accepted world-readable private key")
	}
}
func TestPEMStrictness(t *testing.T) {
	pr, pu, _ := Keypair()
	if _, e := Private(append(pr, pr...)); e == nil {
		t.Fatal("multiple private keys accepted")
	}
	if _, e := Public(append(pu, pu...)); e == nil {
		t.Fatal("multiple public keys accepted")
	}
}
