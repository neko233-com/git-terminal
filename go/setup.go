// Copyright 2026 git-next contributors. MIT license.
package gitterminal

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
)

const productVersion = "0.2.0"

// Setup is the session description printed by --print-setup and used to
// launch shells. Its fields are the public --print-setup JSON contract.
type Setup struct {
	Product            string  `json:"product"`
	Version            string  `json:"version"`
	Renderer           string  `json:"renderer"`
	Cwd                string  `json:"cwd"`
	Shell              string  `json:"shell"`
	Profile            string  `json:"profile"`
	Git                *string `json:"git"`
	NoProfile          bool    `json:"no_profile"`
	Integration        *string `json:"integration"`
	SyntaxHighlighting bool    `json:"syntax_highlighting"`
	GitPrompt          bool    `json:"git_prompt"`
}

// discoverSetup resolves the working directory, git-next identity, shell
// profile and integration scripts. A non-git-next executable is rejected.
func discoverSetup(cwd, profile, explicitGit string) (*Setup, error) {
	absolute, err := filepath.Abs(cwd)
	if err != nil {
		return nil, fmt.Errorf("working directory is unavailable: %w", err)
	}
	if resolved, err := filepath.EvalSymlinks(absolute); err == nil {
		absolute = resolved
	}
	info, err := os.Stat(absolute)
	if err != nil || !info.IsDir() {
		return nil, errors.New("working directory must be an existing directory")
	}

	integration := findIntegration()
	git, err := discoverGit(explicitGit)
	if err != nil {
		return nil, err
	}
	bash := ""
	if git != "" {
		candidate := filepath.Join(filepath.Dir(filepath.Dir(git)), "runtime", "usr", "bin")
		if runtime.GOOS == "windows" {
			candidate = filepath.Join(filepath.Dir(filepath.Dir(git)), "runtime", "usr", "bin", "bash.exe")
		} else {
			candidate = filepath.Join(candidate, "bash")
		}
		if info, err := os.Stat(candidate); err == nil && !info.IsDir() {
			bash = candidate
		}
	}
	if profile == "" || profile == "auto" {
		switch {
		case bash != "":
			profile = "bash"
		case runtime.GOOS == "windows":
			profile = "powershell"
		default:
			profile = "shell"
		}
	}
	shell, err := shellForProfile(profile, bash)
	if err != nil {
		return nil, err
	}
	var gitPtr, integrationPtr *string
	if git != "" {
		gitPtr = &git
	}
	if integration != "" {
		integrationPtr = &integration
	}
	return &Setup{
		Product: "git-terminal", Version: productVersion, Renderer: "mygo",
		Cwd: absolute, Shell: shell, Profile: profile, Git: gitPtr,
		NoProfile: false, Integration: integrationPtr,
		SyntaxHighlighting: true, GitPrompt: true,
	}, nil
}

// findIntegration first looks beside the executable, then materializes the
// embedded scripts into the user config directory so the standalone binary
// has a managed prompt without an installer.
func findIntegration() string {
	if executable, err := os.Executable(); err == nil {
		candidate := filepath.Join(filepath.Dir(executable), "shell-integration")
		if info, err := os.Stat(filepath.Join(candidate, "bashrc")); err == nil && !info.IsDir() {
			return candidate
		}
	}
	base := os.Getenv("APPDATA")
	if base == "" {
		base = os.Getenv("XDG_CONFIG_HOME")
	}
	if base == "" {
		base = filepath.Join(os.Getenv("HOME"), ".config")
	}
	dir := filepath.Join(base, "git-terminal", "shell-integration")
	for _, name := range []string{"bashrc", "powershell.ps1", "prompt.bash"} {
		data, err := integrationFS.ReadFile("shell-integration/" + name)
		if err != nil {
			return ""
		}
		path := filepath.Join(dir, name)
		if existing, err := os.ReadFile(path); err == nil && string(existing) == string(data) {
			continue
		}
		if err := os.MkdirAll(dir, 0o755); err != nil {
			return ""
		}
		if err := os.WriteFile(path, data, 0o644); err != nil {
			return ""
		}
	}
	return dir
}

// gitCandidate picks one executable in a fixed priority: explicit --git,
// GIT_NEXT_EXECUTABLE, a sibling, then the installed locations. One candidate
// is chosen and then verified; a wrong explicit git must fail rather than
// silently falling through to another.
func gitCandidate(explicit string) string {
	if explicit != "" {
		return explicit
	}
	if fromEnv := os.Getenv("GIT_NEXT_EXECUTABLE"); fromEnv != "" {
		return fromEnv
	}
	if executable, err := os.Executable(); err == nil {
		name := "git"
		if runtime.GOOS == "windows" {
			name = "git.exe"
		}
		sibling := filepath.Join(filepath.Dir(executable), name)
		if info, err := os.Stat(sibling); err == nil && !info.IsDir() {
			return sibling
		}
	}
	programFiles := os.Getenv("ProgramW6432")
	if programFiles == "" {
		programFiles = os.Getenv("ProgramFiles")
	}
	if programFiles == "" {
		programFiles = `C:\Program Files`
	}
	standard := filepath.Join(programFiles, "Git", "bin", "git.exe")
	if info, err := os.Stat(standard); err == nil && !info.IsDir() {
		return standard
	}
	legacy := filepath.Join(programFiles, "git-next", platformDir(), "bin", "git.exe")
	if info, err := os.Stat(legacy); err == nil && !info.IsDir() {
		return legacy
	}
	return ""
}

func discoverGit(explicit string) (string, error) {
	candidate := gitCandidate(explicit)
	if candidate == "" {
		return "", nil
	}
	identity, err := identifyGit(candidate)
	if err != nil {
		return "", fmt.Errorf("the selected Git executable must identify itself as git-next: %w", err)
	}
	if identity != "git-next" {
		return "", errors.New("the selected Git executable must identify itself as git-next")
	}
	if resolved, err := filepath.EvalSymlinks(candidate); err == nil {
		candidate = resolved
	}
	return candidate, nil
}

func platformDir() string {
	arch := runtime.GOARCH
	if arch == "arm64" {
		return "windows-arm64"
	}
	return "windows-x64"
}

func identifyGit(path string) (string, error) {
	cmd := exec.Command(path, "version", "--json")
	configureIdentify(cmd)
	output, err := cmd.Output()
	if err != nil {
		return "", err
	}
	return identityFromJSON(output)
}

func identityFromJSON(data []byte) (string, error) {
	var identity struct {
		Product string `json:"product"`
	}
	if err := json.Unmarshal(data, &identity); err != nil {
		return "", err
	}
	if identity.Product != "git-next" {
		return "", fmt.Errorf("product is %q, not git-next", identity.Product)
	}
	return identity.Product, nil
}

func shellForProfile(profile, bash string) (string, error) {
	switch profile {
	case "bash":
		if bash != "" {
			return bash, nil
		}
		if runtime.GOOS != "windows" {
			if found := findCommand("bash"); found != "" {
				return found, nil
			}
		}
		return "", errors.New("the owned Bash shell is unavailable")
	case "powershell":
		if found := findCommand("pwsh.exe"); found != "" {
			return found, nil
		}
		if found := findCommand("powershell.exe"); found != "" {
			return found, nil
		}
		root := os.Getenv("SystemRoot")
		if root == "" {
			root = `C:\Windows`
		}
		candidate := filepath.Join(root, "System32", "WindowsPowerShell", "v1.0", "powershell.exe")
		if info, err := os.Stat(candidate); err == nil && !info.IsDir() {
			return candidate, nil
		}
		return "", errors.New("PowerShell is unavailable")
	case "cmd":
		root := os.Getenv("SystemRoot")
		if root == "" {
			root = `C:\Windows`
		}
		return filepath.Join(root, "System32", "cmd.exe"), nil
	case "shell":
		if shell := os.Getenv("SHELL"); shell != "" {
			return shell, nil
		}
		return "/bin/sh", nil
	default:
		return "", fmt.Errorf("unsupported shell profile: %s", profile)
	}
}

func findCommand(name string) string {
	path, err := exec.LookPath(name)
	if err != nil {
		return ""
	}
	return path
}

// windowsPath normalizes a path for a Windows ConPTY child: forward slashes
// become backslashes and the \\?\ prefix from filepath.EvalSymlinks is
// removed. Bash arguments use slashPath instead.
func windowsPath(path string) string {
	if runtime.GOOS != "windows" {
		return path
	}
	path = strings.ReplaceAll(path, "/", `\`)
	if strings.HasPrefix(path, `\\?\UNC\`) {
		return `\\` + strings.TrimPrefix(path, `\\?\UNC\`)
	}
	return strings.TrimPrefix(path, `\\?\`)
}

func slashPath(path string) string {
	return strings.ReplaceAll(windowsPath(path), `\`, "/")
}

// launch returns the command and environment for the mygo terminal plugin.
// The plugin owns the PTY; this setup describes what it starts.
func (s *Setup) launch(command string) ([]string, []string) {
	shell := s.Shell
	if runtime.GOOS == "windows" {
		shell = windowsPath(shell)
	}
	args := []string{shell}
	integration := ""
	if s.Integration != nil {
		integration = *s.Integration
	}
	switch s.Profile {
	case "bash":
		switch {
		case command == "" && integration != "":
			rcfile := filepath.Join(integration, "bashrc")
			if runtime.GOOS == "windows" {
				rcfile = slashPath(rcfile)
			}
			args = append(args, "--noprofile", "--rcfile", rcfile)
		case s.NoProfile:
			args = append(args, "--noprofile", "--norc")
		default:
			args = append(args, "--login")
		}
		if command != "" {
			args = append(args, "-c", command)
		} else {
			args = append(args, "-i")
		}
	case "powershell":
		args = append(args, "-NoLogo")
		if s.NoProfile {
			args = append(args, "-NoProfile")
		}
		if command != "" {
			args = append(args, "-Command", command)
		} else if integration != "" {
			args = append(args, "-ExecutionPolicy", "Bypass", "-NoExit", "-File",
				filepath.Join(integration, "powershell.ps1"))
		}
	case "cmd":
		if command != "" {
			args = append(args, command)
		} else {
			args = append(args, "/d")
		}
	default:
		if command != "" {
			args = append(args, "-c", command)
		} else {
			args = append(args, "-i")
		}
	}

	env := append([]string(nil), os.Environ()...)
	set := func(key, value string) {
		prefix := key + "="
		for i, item := range env {
			if strings.HasPrefix(item, prefix) {
				env[i] = prefix + value
				return
			}
		}
		env = append(env, prefix+value)
	}
	set("TERM", "xterm-256color")
	set("COLORTERM", "truecolor")
	set("TERM_PROGRAM", "Git-Terminal")
	set("TERM_PROGRAM_VERSION", productVersion)
	if os.Getenv("USER") == "" && os.Getenv("USERNAME") != "" {
		set("USER", os.Getenv("USERNAME"))
	}
	if s.Profile == "bash" {
		set("TMPDIR", os.TempDir())
		if os.Getenv("LANG") == "" {
			set("LANG", "C.UTF-8")
		}
	}
	set("GIT_TERMINAL_LOAD_PROFILE", boolFlag(!s.NoProfile))
	set("GIT_TERMINAL_SYNTAX_HIGHLIGHTING", boolFlag(s.SyntaxHighlighting))
	set("GIT_TERMINAL_GIT_PROMPT", boolFlag(s.GitPrompt))
	if s.Git != nil {
		git := *s.Git
		bin := filepath.Dir(git)
		root := filepath.Dir(bin)
		paths := []string{bin}
		if runtime.GOOS == "windows" {
			prefix := "mingw64"
			if runtime.GOARCH == "arm64" {
				prefix = "clangarm64"
			}
			runtimeBin := filepath.Join(root, "runtime", prefix, "bin")
			usrBin := filepath.Join(root, "runtime", "usr", "bin")
			if info, err := os.Stat(runtimeBin); err == nil && info.IsDir() {
				paths = append(paths, runtimeBin)
			}
			if info, err := os.Stat(usrBin); err == nil && info.IsDir() {
				paths = append(paths, usrBin)
				set("MSYSTEM", strings.ToUpper(prefix))
				set("CHERE_INVOKING", "1")
				set("MSYS2_PATH_TYPE", "inherit")
			}
			home := os.Getenv("HOME")
			if home == "" {
				home = os.Getenv("USERPROFILE")
			}
			if home != "" {
				set("HOME", home)
			}
		}
		for _, dir := range filepath.SplitList(os.Getenv("PATH")) {
			if dir != "" {
				paths = append(paths, dir)
			}
		}
		if runtime.GOOS == "windows" {
			for i, dir := range paths {
				paths[i] = windowsPath(dir)
			}
			git = windowsPath(git)
		}
		set("PATH", strings.Join(paths, string(filepath.ListSeparator)))
		if s.Profile == "bash" {
			git = slashPath(git)
		}
		set("GIT_NEXT_EXECUTABLE", git)
	}
	return args, env
}

func boolFlag(value bool) string {
	if value {
		return "1"
	}
	return "0"
}
