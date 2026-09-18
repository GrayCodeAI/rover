package wire

import (
	"encoding/json"
	"testing"
)

type testInner struct {
	A string `json:"a"`
	B int    `json:"b,omitempty"`
}

type testOuter struct {
	Name   string          `json:"name"`
	Inner  testInner       `json:"inner"`
	Option *testInner      `json:"option,omitempty"`
	Items  []testInner     `json:"items"`
	Raw    json.RawMessage `json:"raw"`
}

func TestStrictNamesAndNull(t *testing.T) {
	type X struct {
		Name  string `json:"name"`
		Count int    `json:"count,omitempty"`
	}
	for _, b := range []string{`{"Name":"bad"}`, `{"name":null}`, `{"name":"a","name":"b"}`, `{"name":"a","extra":true}`} {
		var x X
		if Decode([]byte(b), &x) == nil {
			t.Fatal("accepted", b)
		}
	}
	var x X
	if e := Decode([]byte(`{"name":"ok"}`), &x); e != nil {
		t.Fatal(e)
	}
}

func TestDecodeRejectsUnknownTopLevelField(t *testing.T) {
	type S struct {
		Name string `json:"name"`
	}
	var s S
	if e := Decode([]byte(`{"name":"a","unknown":"b"}`), &s); e == nil {
		t.Fatal("accepted unknown field")
	}
}

func TestDecodeRejectsMisCasedField(t *testing.T) {
	type S struct {
		Name string `json:"name"`
	}
	var s S
	if e := Decode([]byte(`{"Name":"a"}`), &s); e == nil {
		t.Fatal("accepted mis-cased field")
	}
}

func TestDecodeRejectsNullScalar(t *testing.T) {
	type S struct {
		Name string `json:"name"`
	}
	var s S
	if e := Decode([]byte(`{"name":null}`), &s); e == nil {
		t.Fatal("accepted null scalar")
	}
}

func TestDecodeRejectsDuplicateKeys(t *testing.T) {
	type S struct {
		A string `json:"a"`
		B string `json:"b"`
	}
	var s S
	if e := Decode([]byte(`{"a":"1","a":"2","b":"3"}`), &s); e == nil {
		t.Fatal("accepted duplicate keys")
	}
}

func TestDecodeRejectsDeepNesting(t *testing.T) {
	type node struct {
		N *node `json:"n,omitempty"`
	}
	// Build depth 101 (limit is 100).
	s := `{"n":`
	for i := 0; i < 101; i++ {
		s += `{"n":`
	}
	for i := 0; i < 102; i++ {
		s += `}`
	}
	var dst node
	if e := Decode([]byte(s), &dst); e == nil {
		t.Fatal("accepted nesting > 100")
	}
}

func TestDecodeRejectsMultipleDocuments(t *testing.T) {
	type S struct {
		A string `json:"a"`
	}
	var s S
	if e := Decode([]byte(`{"a":"1"}{"a":"2"}`), &s); e == nil {
		t.Fatal("accepted multiple JSON documents")
	}
}

func TestDecodeAcceptsNestedUnknownField(t *testing.T) {
	var o testOuter
	if e := Decode([]byte(`{"name":"x","inner":{"a":"v","b":1,"bad":2},"raw":{},"items":[]}`), &o); e == nil {
		t.Fatal("accepted unknown field in nested struct")
	}
}

func TestDecodeAcceptsValidNested(t *testing.T) {
	var o testOuter
	if e := Decode([]byte(`{"name":"x","inner":{"a":"v","b":1},"raw":{},"items":[{"a":"y","b":2}]}`), &o); e != nil {
		t.Fatal(e)
	}
	if o.Inner.A != "v" || o.Inner.B != 1 {
		t.Fatalf("unexpected inner: %+v", o.Inner)
	}
}

func TestDecodeRejectsMisCasedNestedField(t *testing.T) {
	var o testOuter
	if e := Decode([]byte(`{"name":"x","Inner":{"a":"v"}}`), &o); e == nil {
		t.Fatal("accepted mis-cased field Inner")
	}
}

func TestDecodeRejectsNullInNestedStruct(t *testing.T) {
	var o testOuter
	if e := Decode([]byte(`{"name":"x","inner":null,"raw":{}}`), &o); e == nil {
		t.Fatal("accepted null for non-pointer nested struct")
	}
}

func TestDecodeAcceptsNullForOptionalPointer(t *testing.T) {
	var o testOuter
	if e := Decode([]byte(`{"name":"x","inner":{"a":"v"},"option":null,"raw":{}}`), &o); e != nil {
		t.Fatal(e)
	}
	if o.Option != nil {
		t.Fatal("expected nil pointer for null optional field")
	}
}

func TestDecodeRejectsNullInSliceElement(t *testing.T) {
	var o testOuter
	if e := Decode([]byte(`{"name":"x","inner":{"a":"v"},"raw":{},"items":[null]}`), &o); e == nil {
		t.Fatal("accepted null element in struct slice")
	}
}

func TestDecodeAcceptsRawMessageBypass(t *testing.T) {
	var o testOuter
	// raw is json.RawMessage — any JSON value is accepted.
	if e := Decode([]byte(`{"name":"x","inner":{"a":"v"},"raw":{"anything":true},"items":[]}`), &o); e != nil {
		t.Fatal(e)
	}
}

func TestDecodeRejectsNullRoot(t *testing.T) {
	type S struct {
		A string `json:"a"`
	}
	var s S
	if e := Decode([]byte(`null`), &s); e == nil {
		t.Fatal("accepted null root for struct target")
	}
}

func TestDecodeRequiresPointerTarget(t *testing.T) {
	type S struct {
		A string `json:"a"`
	}
	var s S
	if e := Decode([]byte(`{"a":"v"}`), s); e == nil {
		t.Fatal("accepted non-pointer target")
	}
}

func TestDecodeRejectsMapWithWrongValue(t *testing.T) {
	type S struct {
		M map[string]testInner `json:"m"`
	}
	var s S
	if e := Decode([]byte(`{"m":{"k":{"a":"v","bad":true}}}`), &s); e == nil {
		t.Fatal("accepted unknown field in map value struct")
	}
}

func TestDecodeAcceptsValidMap(t *testing.T) {
	type S struct {
		M map[string]testInner `json:"m"`
	}
	var s S
	if e := Decode([]byte(`{"m":{"k":{"a":"v","b":1}}}`), &s); e != nil {
		t.Fatal(e)
	}
	if s.M["k"].A != "v" {
		t.Fatalf("unexpected map value: %+v", s.M)
	}
}
