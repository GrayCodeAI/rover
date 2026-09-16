//go:build linux

package tasks

import (
	"fmt"
	"os"
	"strconv"
	"strings"
	"syscall"
)

func ProcessIdentity(pid int) string {
	b, e := os.ReadFile(fmt.Sprintf("/proc/%d/stat", pid))
	if e != nil {
		return ""
	}
	end := strings.LastIndexByte(string(b), ')')
	if end < 0 {
		return ""
	}
	fields := strings.Fields(string(b)[end+1:])
	if len(fields) < 20 {
		return ""
	}
	boot, e := os.ReadFile("/proc/sys/kernel/random/boot_id")
	if e != nil {
		return ""
	}
	return strings.TrimSpace(string(boot)) + ":" + strconv.Itoa(pid) + ":" + fields[19]
}
func DefinitelyGone(pid int, identity string) bool {
	if pid <= 0 {
		return false
	}
	if syscall.Kill(pid, 0) == syscall.ESRCH {
		return true
	}
	now := ProcessIdentity(pid)
	if identity != "" && now != "" && identity != now {
		return true
	}
	b, e := os.ReadFile(fmt.Sprintf("/proc/%d/stat", pid))
	if e == nil {
		end := strings.LastIndexByte(string(b), ')')
		if end >= 0 {
			f := strings.Fields(string(b)[end+1:])
			return len(f) > 0 && f[0] == "Z"
		}
	}
	return false
}
