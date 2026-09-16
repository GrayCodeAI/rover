package cli

import (
	"flag"
	"fmt"
	"github.com/GrayCodeAI/rover/internal/execution"
	"github.com/GrayCodeAI/rover/internal/model"
	"github.com/GrayCodeAI/rover/internal/store"
)

type extendedApp struct {
	*App
	root     string
	jsonMode bool
}

func contains(a []string, s string) bool {
	for _, x := range a {
		if x == s {
			return true
		}
	}
	return false
}
func (a *extendedApp) flags(name string) *flag.FlagSet {
	f := a.fs(name)
	f.Bool("json", false, "machine-readable output")
	return f
}
func (a *extendedApp) parse(f *flag.FlagSet, args []string) bool {
	if e := f.Parse(args); e != nil {
		return false
	}
	if len(f.Args()) > 0 {
		fmt.Fprintln(a.Err, "unexpected positional arguments")
		return false
	}
	return true
}
func (a *extendedApp) emit(v any) int {
	if e := a.App.emit(v); e != nil {
		return a.fail(e)
	}
	return 0
}
func (a *extendedApp) fail(e error) int {
	if e == nil {
		return 0
	}
	if a.jsonMode {
		_ = a.App.emit(map[string]any{"schema": model.Schema, "error": e.Error()})
	} else {
		fmt.Fprintln(a.Err, "rover:", execution.SafeText(e.Error()))
	}
	return 2
}
func (a *extendedApp) withStore(fn func(*store.Store) int) int {
	s, e := store.Open(a.root)
	if e != nil {
		return a.fail(e)
	}
	defer s.Close()
	return fn(s)
}

func parse(f *flag.FlagSet, args []string) error {
	if e := f.Parse(args); e != nil {
		return e
	}
	if len(f.Args()) > 0 {
		return fmt.Errorf("unexpected positional arguments")
	}
	return nil
}
