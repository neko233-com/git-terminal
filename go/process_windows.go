// Copyright 2026 git-next contributors. MIT license.
//go:build windows

package gitterminal

import (
	"os/exec"
	"syscall"
)

// configureIdentify keeps the version --json probe from opening a console
// window when git-terminal itself starts without one.
func configureIdentify(cmd *exec.Cmd) {
	cmd.SysProcAttr = &syscall.SysProcAttr{CreationFlags: 0x08000000} // CREATE_NO_WINDOW
}
