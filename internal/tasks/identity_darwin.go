//go:build darwin

package tasks

import "syscall"

// Native process-birth identity is not implemented on macOS. A stale heartbeat
// with a still-existing/reused PID remains uncertain rather than marked lost.
func ProcessIdentity(pid int) string { return "" }
func DefinitelyGone(pid int, identity string) bool {
	return pid > 0 && syscall.Kill(pid, 0) == syscall.ESRCH
}
