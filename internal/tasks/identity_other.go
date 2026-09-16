//go:build !linux && !darwin

package tasks

func ProcessIdentity(pid int) string               { return "" }
func DefinitelyGone(pid int, identity string) bool { return false }
