// Copyright 2026 git-next contributors. MIT license.
package main

import (
	"encoding/json"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

// captureRun swaps the standard streams while run executes. Tests using it must
// not run in parallel: the streams are process-wide.
func captureRun(t *testing.T, args []string) (int, string, string, error) {
	t.Helper()
	stdout, stderr := os.Stdout, os.Stderr
	outReader, outWriter, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	errReader, errWriter, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	os.Stdout, os.Stderr = outWriter, errWriter
	code, runErr := run(args)
	_ = outWriter.Close()
	_ = errWriter.Close()
	os.Stdout, os.Stderr = stdout, stderr
	out, _ := io.ReadAll(outReader)
	errOut, _ := io.ReadAll(errReader)
	_ = outReader.Close()
	_ = errReader.Close()
	return code, string(out), string(errOut), runErr
}

func isolatedHome(t *testing.T) {
	t.Helper()
	base := t.TempDir()
	t.Setenv("APPDATA", base)
	t.Setenv("XDG_CONFIG_HOME", filepath.Join(base, "xdg"))
	t.Setenv("HOME", filepath.Join(base, "home"))
	t.Setenv("GIT_NEXT_EXECUTABLE", "")
	t.Setenv("ProgramFiles", t.TempDir())
	t.Setenv("ProgramW6432", t.TempDir())
}

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

func TestRunVersionAndHelp(t *testing.T) {
	isolatedHome(t)
	code, out, errOut, err := captureRun(t, []string{"--version"})
	if err != nil || code != 0 || strings.TrimSpace(out) != "git-terminal 0.2.0" {
		t.Errorf("version = %d, %q, %q, %v", code, out, errOut, err)
	}
	code, out, errOut, err = captureRun(t, []string{"-V"})
	if err != nil || code != 0 || strings.TrimSpace(out) != "git-terminal 0.2.0" {
		t.Errorf("-V = %d, %q, %q, %v", code, out, errOut, err)
	}
	code, out, errOut, err = captureRun(t, []string{"--help"})
	if err != nil || code != 0 || out != "" || !strings.Contains(errOut, "Usage: git-terminal") {
		t.Errorf("help = %d, stdout %q, stderr %q, %v", code, out, errOut, err)
	}
	code, _, errOut, err = captureRun(t, []string{"-h"})
	if err != nil || code != 0 || !strings.Contains(errOut, "language list") {
		t.Errorf("-h = %d, stderr %q, %v", code, errOut, err)
	}
}

func TestRunPrintSetupJSON(t *testing.T) {
	isolatedHome(t)
	code, out, errOut, err := captureRun(t, []string{"--print-setup", "--json", "--shell", "shell", "--no-profile"})
	if err != nil || code != 0 {
		t.Fatalf("print-setup = %d, stderr %q, %v", code, errOut, err)
	}
	var setup struct {
		Product  string  `json:"product"`
		Version  string  `json:"version"`
		Renderer string  `json:"renderer"`
		Shell    string  `json:"shell"`
		Profile  string  `json:"profile"`
		Git      *string `json:"git"`
	}
	if err := json.Unmarshal([]byte(out), &setup); err != nil {
		t.Fatalf("setup is not JSON: %q: %v", out, err)
	}
	if setup.Product != "git-terminal" || setup.Version != "0.2.0" || setup.Renderer != "mygo" ||
		setup.Profile != "shell" || setup.Git != nil {
		t.Errorf("unexpected setup: %+v", setup)
	}
	if setup.Shell == "" {
		t.Error("resolved shell is empty")
	}
}

func TestRunLanguageCommands(t *testing.T) {
	isolatedHome(t)
	code, out, errOut, err := captureRun(t, []string{"language", "list"})
	if err != nil || code != 0 || !strings.Contains(out, "zh-Hans") || !strings.Contains(out, "auto") {
		t.Errorf("language list = %d, %q, %q, %v", code, out, errOut, err)
	}
	code, out, errOut, err = captureRun(t, []string{"language", "get"})
	if err != nil || code != 0 {
		t.Fatalf("language get = %d, %q, %v", code, errOut, err)
	}
	var status struct {
		Product    string   `json:"product"`
		Configured string   `json:"configured"`
		Language   string   `json:"language"`
		Supported  []string `json:"supported"`
	}
	if err := json.Unmarshal([]byte(out), &status); err != nil {
		t.Fatalf("language get is not JSON: %q: %v", out, err)
	}
	if status.Product != "git-terminal" || status.Language == "" || len(status.Supported) < 10 {
		t.Errorf("language get status = %+v", status)
	}
	code, out, errOut, err = captureRun(t, []string{"language", "set", "zh-Hans"})
	if err != nil || code != 0 {
		t.Fatalf("language set = %d, %q, %v", code, errOut, err)
	}
	if err := json.Unmarshal([]byte(out), &status); err != nil {
		t.Fatalf("language set is not JSON: %q: %v", out, err)
	}
	if status.Configured != "zh-Hans" || status.Language != "zh-Hans" {
		t.Errorf("language set status = %+v", status)
	}
	code, out, errOut, err = captureRun(t, []string{"language", "get"})
	if err != nil || code != 0 || !strings.Contains(out, `"configured": "zh-Hans"`) {
		t.Errorf("selection did not persist: %d, %q, %q, %v", code, out, errOut, err)
	}
	code, _, _, err = captureRun(t, []string{"language", "set", "not-a-language"})
	if err == nil || code != 1 || !strings.Contains(err.Error(), "unsupported language") {
		t.Errorf("bad language = %d, %v", code, err)
	}
}

func TestRunRejectsBadArgumentsAndGit(t *testing.T) {
	isolatedHome(t)
	for _, args := range [][]string{
		{"--unknown"},
		{"--cd-to-home", "--no-cd"},
		{"unexpected"},
		{"--shell"},
	} {
		code, _, _, err := captureRun(t, args)
		if err == nil || code != 1 {
			t.Errorf("%v was accepted: %d, %v", args, code, err)
		}
	}
	notGit := filepath.Join(t.TempDir(), "not-git.exe")
	if err := os.WriteFile(notGit, []byte("not an executable"), 0o755); err != nil {
		t.Fatal(err)
	}
	code, _, _, err := captureRun(t, []string{"--print-setup", "--git", notGit})
	if err == nil || code != 1 || !strings.Contains(err.Error(), "git-next") {
		t.Errorf("non-git-next executable was accepted: %d, %v", code, err)
	}
}

func TestRunCdHomeAndNoCD(t *testing.T) {
	isolatedHome(t)
	home := filepath.Join(t.TempDir(), "home")
	if err := os.MkdirAll(home, 0o755); err != nil {
		t.Fatal(err)
	}
	t.Setenv("USERPROFILE", home)
	t.Setenv("HOME", home)
	code, out, errOut, err := captureRun(t, []string{"--cd-to-home", "--print-setup", "--json", "--shell", "shell"})
	if err != nil || code != 0 {
		t.Fatalf("cd-to-home = %d, %q, %v", code, errOut, err)
	}
	var setup struct {
		Cwd string `json:"cwd"`
	}
	if err := json.Unmarshal([]byte(out), &setup); err != nil {
		t.Fatal(err)
	}
	want, err := filepath.EvalSymlinks(home)
	if err != nil {
		want = home
	}
	if setup.Cwd != want {
		t.Errorf("cwd = %q, want home %q", setup.Cwd, want)
	}
	// --no-cd keeps the launcher directory and ignores an explicit --cwd.
	code, out, errOut, err = captureRun(t, []string{"--no-cd", "--cwd", filepath.Join(t.TempDir(), "ignored"),
		"--print-setup", "--json", "--shell", "shell"})
	if err != nil || code != 0 {
		t.Fatalf("no-cd = %d, %q, %v", code, errOut, err)
	}
	if err := json.Unmarshal([]byte(out), &setup); err != nil {
		t.Fatal(err)
	}
	launcher, err := os.Getwd()
	if err != nil {
		t.Fatal(err)
	}
	if resolved, err := filepath.EvalSymlinks(launcher); err == nil {
		launcher = resolved
	}
	if setup.Cwd != launcher {
		t.Errorf("cwd = %q, want launcher %q", setup.Cwd, launcher)
	}
}
