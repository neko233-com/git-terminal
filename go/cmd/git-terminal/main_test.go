// Copyright 2026 git-next contributors. MIT license.
package main

import "testing"

func TestParseArgs(t *testing.T) {
	opts, err := parseArgs([]string{
		"--cwd", "D:/repo", "--no-profile", "--no-shell-integration",
		"--shell", "bash", "--command", "git status", "--git", "D:/bin/git.exe",
		"--print-setup", "--json", "-V",
	})
	if err != nil {
		t.Fatal(err)
	}
	if opts.cwd != "D:/repo" || opts.shell != "bash" || opts.command != "git status" ||
		opts.git != "D:/bin/git.exe" || !opts.noProfile || !opts.noShellIntegration ||
		!opts.printSetup || !opts.jsonOutput || !opts.version {
		t.Errorf("unexpected options: %+v", opts)
	}
	opts, err = parseArgs([]string{"--cd", "D:/repo", "--cd-to-home"})
	if err != nil || opts.cwd != "D:/repo" || !opts.cdToHome {
		t.Errorf("cd alias/home = %+v, %v", opts, err)
	}
	if _, err := parseArgs([]string{"--unknown"}); err == nil {
		t.Error("an unknown option was accepted")
	}
	if _, err := parseArgs([]string{"--cd-to-home", "--no-cd"}); err == nil {
		t.Error("conflicting cd options were accepted")
	}
}

func TestParseLanguage(t *testing.T) {
	for _, args := range [][]string{
		{"language", "list"}, {"language", "get"}, {"language", "set", "zh-Hans"},
	} {
		opts, err := parseArgs(args)
		if err != nil {
			t.Fatalf("%v: %v", args, err)
		}
		if opts.languageAction != args[1] {
			t.Errorf("%v: action = %q", args, opts.languageAction)
		}
		if len(args) > 2 && opts.languageValue != args[2] {
			t.Errorf("%v: value = %q", args, opts.languageValue)
		}
	}
	if _, err := parseArgs([]string{"language", "set"}); err == nil {
		t.Error("language set without a code was accepted")
	}
}
