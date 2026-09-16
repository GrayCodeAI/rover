//go:build !linux && !darwin

package execution

import "os/exec"

func configureProcess(c *exec.Cmd) {}
func cleanupProcess(c *exec.Cmd)   {}
func Detach(c *exec.Cmd)           {}
