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

func TestFindIntegrationMaterializesIdempotently(t *testing.T) {
	base := isolatedConfig(t)
	// An executable beside a shell-integration directory would short-circuit
	// materialization; the test binary never has one, so the config path is used.
	dir := findIntegration()
	wantDir := filepath.Join(base, "git-terminal", "shell-integration")
	if dir != wantDir {
		t.Fatalf("integration directory = %q, want %q", dir, wantDir)
	}
	stamps := make(map[string]int64)
	for _, name := range []string{"bashrc", "powershell.ps1", "prompt.bash"} {
		embedded, err := integrationFS.ReadFile("shell-integration/" + name)
		if err != nil {
			t.Fatal(err)
		}
		written, err := os.ReadFile(filepath.Join(dir, name))
		if err != nil {
			t.Fatalf("%s was not materialized: %v", name, err)
		}
		if string(written) != string(embedded) {
			t.Errorf("%s differs from the embedded script", name)
		}
		info, err := os.Stat(filepath.Join(dir, name))
		if err != nil {
			t.Fatal(err)
		}
		stamps[name] = info.ModTime().UnixNano()
	}
	if again := findIntegration(); again != dir {
		t.Errorf("second findIntegration = %q, want %q", again, dir)
	}
	for name, stamp := range stamps {
		info, err := os.Stat(filepath.Join(dir, name))
		if err != nil {
			t.Fatal(err)
		}
		if info.ModTime().UnixNano() != stamp {
			t.Errorf("%s was rewritten on the second call", name)
		}
	}
	// A tampered script is restored from the embedded source.
	path := filepath.Join(dir, "bashrc")
	if err := os.WriteFile(path, []byte("tampered"), 0o644); err != nil {
		t.Fatal(err)
	}
	_ = findIntegration()
	written, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	embedded, err := integrationFS.ReadFile("shell-integration/bashrc")
	if err != nil {
		t.Fatal(err)
	}
	if string(written) != string(embedded) {
		t.Error("a tampered script was not restored")
	}
}

func TestShellForProfileSelection(t *testing.T) {
	root := t.TempDir()
	t.Setenv("SystemRoot", root)
	t.Setenv("PATH", t.TempDir())
	t.Setenv("SHELL", "/opt/custom/sh")
	bash := filepath.Join(root, "bash")
	if err := os.WriteFile(bash, nil, 0o755); err != nil {
		t.Fatal(err)
	}
	powerShell := filepath.Join(root, "System32", "WindowsPowerShell", "v1.0", "powershell.exe")
	if err := os.MkdirAll(filepath.Dir(powerShell), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(powerShell, nil, 0o755); err != nil {
		t.Fatal(err)
	}
	for _, test := range []struct {
		profile string
		bash    string
		want    string
		fail    bool
	}{
		{"bash", bash, bash, false},
		{"powershell", "", powerShell, false},
		{"cmd", "", filepath.Join(root, "System32", "cmd.exe"), false},
		{"shell", "", "/opt/custom/sh", false},
		{"bogus", "", "", true},
	} {
		got, err := shellForProfile(test.profile, test.bash)
		if test.fail {
			if err == nil {
				t.Errorf("profile %q was accepted", test.profile)
			}
			continue
		}
		if err != nil || got != test.want {
			t.Errorf("profile %q = %q, %v; want %q", test.profile, got, err, test.want)
		}
	}
	if _, err := shellForProfile("bash", ""); runtime.GOOS == "windows" && err == nil {
		t.Error("a missing owned Bash was accepted on Windows")
	}
}

func TestLaunchCommandOverridesManagedPrompt(t *testing.T) {
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
	t.Setenv("GIT_TERMINAL_TEST_ENV", "kept")
	args, env := setup.launch("git status")
	joined := strings.Join(args, " ")
	if !strings.Contains(joined, "-c git status") || strings.Contains(joined, "--rcfile") {
		t.Errorf("command args = %v", args)
	}
	environment := strings.Join(env, "\n")
	for _, wanted := range []string{"GIT_TERMINAL_TEST_ENV=kept", "TERM_PROGRAM_VERSION=" + productVersion} {
		if !strings.Contains(environment, wanted) {
			t.Errorf("environment misses %q", wanted)
		}
	}
	// The inherited PATH survives prepending the git-next bin directory.
	if !strings.Contains(environment, "PATH="+filepath.Dir(git)) {
		t.Errorf("git-next bin is not first in PATH: %s", environment)
	}
	for _, item := range env {
		if strings.HasPrefix(item, "PATH=") && !strings.Contains(item, os.Getenv("PATH")) {
			t.Error("inherited PATH entries were dropped")
		}
	}
}

func TestLaunchOtherProfiles(t *testing.T) {
	integration := t.TempDir()
	setup := &Setup{
		Product: "git-terminal", Version: productVersion, Renderer: "mygo",
		Cwd: t.TempDir(), Shell: "pwsh", Profile: "powershell", Integration: &integration,
	}
	args, _ := setup.launch("Write-Output hi")
	if args[1] != "-NoLogo" || args[2] != "-Command" || args[3] != "Write-Output hi" {
		t.Errorf("powershell args = %v", args)
	}
	setup.Profile, setup.Shell = "cmd", "cmd.exe"
	args, _ = setup.launch("echo hi")
	if args[1] != "echo hi" {
		t.Errorf("cmd args = %v", args)
	}
	args, _ = setup.launch("")
	if args[1] != "/d" {
		t.Errorf("interactive cmd args = %v", args)
	}
	setup.Profile, setup.Shell = "shell", "/bin/sh"
	args, _ = setup.launch("pwd")
	if args[1] != "-c" || args[2] != "pwd" {
		t.Errorf("shell args = %v", args)
	}
}

func TestDiscoverSetupGoContract(t *testing.T) {
	isolatedConfig(t)
	t.Setenv("GIT_NEXT_EXECUTABLE", "")
	t.Setenv("ProgramFiles", t.TempDir())
	t.Setenv("ProgramW6432", t.TempDir())
	setup, err := discoverSetup(t.TempDir(), "shell", "")
	if err != nil {
		t.Fatal(err)
	}
	if setup.Product != "git-terminal" || setup.Version != productVersion || setup.Renderer != "mygo" {
		t.Errorf("setup identity = %+v", setup)
	}
	if setup.Git != nil {
		t.Errorf("a non-git-next executable was accepted: %q", *setup.Git)
	}
	if setup.Integration == nil {
		t.Error("managed integration was not materialized")
	}
	if setup.SyntaxHighlighting != true || setup.GitPrompt != true {
		t.Errorf("prompt flags = %v/%v", setup.SyntaxHighlighting, setup.GitPrompt)
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
