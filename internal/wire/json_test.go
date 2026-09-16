package wire

import "testing"

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
