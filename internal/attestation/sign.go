// Package attestation signs local evidence envelopes with an explicitly selected
// Ed25519 key. A valid signature authenticates bytes under a trusted key; it does
// not prove correctness, independent execution, SLSA compliance or human review.
package attestation

import (
	"crypto/ed25519"
	"crypto/rand"
	"crypto/x509"
	"encoding/json"
	"encoding/pem"
	"errors"
	"fmt"
	"io"
	"os"
	"syscall"

	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/store"
	"github.com/GrayCodeAI/rover/internal/wire"
)

const Domain = "rover.evidence-attestation/v1\x00"

type Envelope struct {
	Schema    string `json:"schema"`
	KeyID     string `json:"key_id"`
	Payload   []byte `json:"payload"`
	Signature []byte `json:"signature"`
}
type Payload struct {
	Schema        string              `json:"schema"`
	IssuerScope   string              `json:"issuer_scope"`
	At            string              `json:"at"`
	Investigation model.Investigation `json:"investigation"`
	Artifacts     []string            `json:"artifacts"`
}

func Keypair() (private, public []byte, err error) {
	pub, priv, e := ed25519.GenerateKey(rand.Reader)
	if e != nil {
		return nil, nil, e
	}
	pr, e := x509.MarshalPKCS8PrivateKey(priv)
	if e != nil {
		return nil, nil, e
	}
	pu, e := x509.MarshalPKIXPublicKey(pub)
	if e != nil {
		return nil, nil, e
	}
	return pem.EncodeToMemory(&pem.Block{Type: "PRIVATE KEY", Bytes: pr}), pem.EncodeToMemory(&pem.Block{Type: "PUBLIC KEY", Bytes: pu}), nil
}
func Private(b []byte) (ed25519.PrivateKey, error) {
	p, rest := pem.Decode(b)
	if p == nil || len(rest) != 0 {
		return nil, errors.New("invalid private key PEM")
	}
	k, e := x509.ParsePKCS8PrivateKey(p.Bytes)
	if e != nil {
		return nil, e
	}
	v, ok := k.(ed25519.PrivateKey)
	if !ok {
		return nil, errors.New("Ed25519 private key required")
	}
	return v, nil
}
func Public(b []byte) (ed25519.PublicKey, error) {
	p, rest := pem.Decode(b)
	if p == nil || len(rest) != 0 {
		return nil, errors.New("invalid public key PEM")
	}
	k, e := x509.ParsePKIXPublicKey(p.Bytes)
	if e != nil {
		return nil, e
	}
	v, ok := k.(ed25519.PublicKey)
	if !ok {
		return nil, errors.New("Ed25519 public key required")
	}
	return v, nil
}
func ReadKey(path string, private bool) ([]byte, error) {
	f, e := os.OpenFile(path, os.O_RDONLY|syscall.O_NOFOLLOW, 0)
	if e != nil {
		return nil, e
	}
	defer f.Close()
	st, e := f.Stat()
	if e != nil {
		return nil, e
	}
	if !st.Mode().IsRegular() || st.Size() > 16384 {
		return nil, errors.New("key must be a bounded regular file")
	}
	if private && st.Mode().Perm()&0077 != 0 {
		return nil, errors.New("private key permissions must exclude group and other users")
	}
	b, e := io.ReadAll(io.LimitReader(f, 16385))
	if e != nil {
		return nil, e
	}
	if int64(len(b)) > 16384 {
		return nil, errors.New("key must be a bounded regular file")
	}
	return b, nil
}
func Sign(s *store.Store, id string, key ed25519.PrivateKey) (Envelope, error) {
	env := Envelope{}
	if len(key) != ed25519.PrivateKeySize {
		return env, errors.New("invalid signing key")
	}
	var in model.Investigation
	if e := s.Get("investigation", id, &in); e != nil {
		return env, e
	}
	if in.FinishedAt == "" || in.Decision == "PENDING" {
		return env, errors.New("cannot sign an unfinished investigation")
	}
	refs := []string{in.ConfigDigest}
	for _, c := range in.Checks {
		for _, h := range []string{c.Process.StdoutSHA256, c.Process.StderrSHA256, c.ReportSHA256} {
			if h != "" {
				refs = append(refs, h)
			}
		}
	}
	seen := map[string]bool{}
	artifacts := []string{}
	for _, h := range refs {
		if seen[h] {
			continue
		}
		seen[h] = true
		if _, e := s.ReadBlob(h); e != nil {
			return env, fmt.Errorf("required artifact unavailable: %w", e)
		}
		artifacts = append(artifacts, h)
	}
	p := Payload{Schema: model.Schema, IssuerScope: "local operator-selected signing key; not independently protected execution", At: model.Now(), Investigation: in, Artifacts: artifacts}
	b, e := json.Marshal(p)
	if e != nil {
		return env, e
	}
	env = Envelope{Schema: "rover.attestation/v1", KeyID: model.Digest(key.Public().(ed25519.PublicKey)), Payload: b, Signature: ed25519.Sign(key, append([]byte(Domain), b...))}
	return env, nil
}
func Verify(e Envelope, expected ed25519.PublicKey) (Payload, error) {
	var p Payload
	if e.Schema != "rover.attestation/v1" || len(expected) != ed25519.PublicKeySize || len(e.Payload) > 16<<20 || e.KeyID != model.Digest(expected) {
		return p, errors.New("envelope or expected key mismatch")
	}
	if !ed25519.Verify(expected, append([]byte(Domain), e.Payload...), e.Signature) {
		return p, errors.New("signature invalid")
	}
	if err := wire.Decode(e.Payload, &p); err != nil {
		return p, err
	}
	if p.Schema != model.Schema || p.Investigation.Candidate == "" {
		return p, errors.New("invalid signed payload")
	}
	return p, nil
}
