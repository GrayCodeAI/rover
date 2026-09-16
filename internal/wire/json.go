// Package wire enforces canonical field names at public control boundaries.
package wire

import (
	"bytes"
	"encoding/json"
	"fmt"
	"github.com/GrayCodeAI/rover/internal/config"
	"reflect"
	"strings"
)

func Decode(b []byte, v any) error {
	if e := config.Decode(b, v); e != nil {
		return e
	}
	t := reflect.TypeOf(v)
	if t.Kind() != reflect.Pointer {
		return fmt.Errorf("decode target must be pointer")
	}
	return names(b, t.Elem(), "$")
}

var rawType = reflect.TypeOf(json.RawMessage{})

func names(b []byte, t reflect.Type, p string) error {
	if t == rawType {
		return nil
	}
	if t.Kind() == reflect.Pointer {
		if bytes.Equal(bytes.TrimSpace(b), []byte("null")) {
			return nil
		}
		return names(b, t.Elem(), p)
	}
	if bytes.Equal(bytes.TrimSpace(b), []byte("null")) {
		return fmt.Errorf("%s: null is not an omitted field", p)
	}
	switch t.Kind() {
	case reflect.Struct:
		var m map[string]json.RawMessage
		if e := json.Unmarshal(b, &m); e != nil {
			return e
		}
		allowed := map[string]reflect.Type{}
		for i := 0; i < t.NumField(); i++ {
			f := t.Field(i)
			if f.PkgPath != "" {
				continue
			}
			tag := strings.Split(f.Tag.Get("json"), ",")[0]
			if tag == "-" {
				continue
			}
			if tag == "" {
				tag = f.Name
			}
			allowed[tag] = f.Type
		}
		for k, v := range m {
			x, ok := allowed[k]
			if !ok {
				return fmt.Errorf("%s: unknown or incorrectly cased field %q", p, k)
			}
			if e := names(v, x, p+"."+k); e != nil {
				return e
			}
		}
	case reflect.Slice, reflect.Array:
		if t.Elem().Kind() == reflect.Uint8 {
			return nil
		}
		var a []json.RawMessage
		if e := json.Unmarshal(b, &a); e != nil {
			return e
		}
		for i, v := range a {
			if e := names(v, t.Elem(), fmt.Sprintf("%s[%d]", p, i)); e != nil {
				return e
			}
		}
	case reflect.Map:
		var m map[string]json.RawMessage
		if e := json.Unmarshal(b, &m); e != nil {
			return e
		}
		for k, v := range m {
			if e := names(v, t.Elem(), p+"."+k); e != nil {
				return e
			}
		}
	}
	return nil
}
