//go:build !windows

package grafikr

import "os/exec"

func configureProcess(*exec.Cmd) {}
