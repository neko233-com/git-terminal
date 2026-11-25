// Copyright 2026 git-next contributors. MIT license.
//go:build !windows

package gitterminal

import "os/exec"

func configureIdentify(_ *exec.Cmd) {}
