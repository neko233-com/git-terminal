// Copyright 2026 git-next contributors. MIT license.
package gitterminal

import (
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
)

func TestIdentityFromJSON(t *testing.T) {
	for _, test := range []struct {
		input string
		want  string
		fail  bool
	}{
		{`{"product":"git-next","version":"0.3.0"}`, "git-next", false},
		{`{"product":"git"}`, "", true},
		{`not json`, "", true},
	} {
		got, err := identityFromJSON([]byte(test.input))
		if test.fail {
			if err == nil {
				t.Errorf("identityFromJSON(%q) unexpectedly succeeded", test.input)
			}
			continue
		}
		if err != nil || got != test.want {
			t.Errorf("identityFromJSON(%q) = %q, %v", test.input, got, err)
		}
	}
}

func TestGitCandidatePriority(t *testing.T) {
	t.Setenv("APPDATA", t.TempDir())
	envGit := filepath.Join(t.TempDir(), "git-next.exe")
	if err := os.WriteFile(envGit, nil, 0o755); err != nil {
		t.Fatal(err)
	}
	t.Setenv("GIT_NEXT_EXECUTABLE", envGit)
	if got := gitCandidate(""); got != envGit {
		t.Errorf("environment git = %q, want %q", got, envGit)
	}
	explicit := filepath.Join(t.TempDir(), "other.exe")
	if err := os.WriteFile(explicit, nil, 0o755); err != nil {
		t.Fatal(err)
	}
	if got := gitCandidate(explicit); got != explicit {
		t.Errorf("explicit git = %q, want %q", got, explicit)
	}
}

func TestDiscoverSetupRejectsBadGitAndMissingDirectory(t *testing.T) {
	t.Setenv("APPDATA", t.TempDir())
	t.Setenv("GIT_NEXT_EXECUTABLE", "")
	if _, err := discoverSetup(filepath.Join(t.TempDir(), "missing"), "auto", ""); err == nil {
		t.Error("a missing working directory was accepted")
	}
	missingGit := filepath.Join(t.TempDir(), "git.exe")
	if _, err := discoverSetup(t.TempDir(), "shell", missingGit); err == nil {
		t.Error("a missing --git executable was accepted")
	}
}

func TestLaunchArgsAndEnvironment(t *testing.T) {
	git := filepath.Join(t.TempDir(), "bin", "git-next")
	integration := t.TempDir()
	if err := os.MkdirAll(filepath.Dir(git), 0o755); err != nil {
		t.Fatal(err)
	}
	setup := &Setup{
		Product: "git-terminal", Version: productVersion, Renderer: "mygo",
		Cwd: t.TempDir(), Shell: "/bin/bash", Profile: "bash", Git: &git,
		Integration: &integration, SyntaxHighlighting: true, GitPrompt: true,
	}
	args, env := setup.launch("")
	if args[1] != "--noprofile" || args[2] != "--rcfile" || args[4] != "-i" {
		t.Errorf("bash args = %v", args)
	}
	wantGit := git
	if runtime.GOOS == "windows" {
		wantGit = slashPath(git) // bash needs forward slashes
	}
	joined := strings.Join(env, "\n")
	for _, wanted := range []string{
		"TERM=xterm-256color", "COLORTERM=truecolor", "TERM_PROGRAM=Git-Terminal",
		"GIT_TERMINAL_LOAD_PROFILE=1", "GIT_TERMINAL_GIT_PROMPT=1",
		"GIT_NEXT_EXECUTABLE=" + wantGit,
	} {
		if !strings.Contains(joined, wanted) {
			t.Errorf("environment misses %q", wanted)
		}
	}
	var path string
	for _, item := range env {
		if strings.HasPrefix(item, "PATH=") {
			path = strings.TrimPrefix(item, "PATH=")
		}
	}
	if !strings.HasPrefix(path, filepath.Dir(git)+string(filepath.ListSeparator)) {
		t.Errorf("git-next bin is not first in PATH: %q", path)
	}
	setup.NoProfile = true
	setup.Integration = nil
	args, _ = setup.launch("echo hi")
	if args[1] != "--noprofile" || args[2] != "--norc" || args[3] != "-c" || args[4] != "echo hi" {
		t.Errorf("no-profile bash args = %v", args)
	}
}
