// Copyright 2026 git-next contributors. MIT license.
package gitterminal

import (
	"os"
	"path/filepath"
	"regexp"
	"runtime"
	"strings"
	"testing"
)

// The toolchain names are assembled from pieces so this guard does not quote
// the very invocations it forbids.
var toolchainCall = regexp.MustCompile(`(^|[^a-zA-Z])(` + "car" + `go|` + `rust` + `c)([^a-zA-Z]|$)`)

func repoRoot(t *testing.T) string {
	t.Helper()
	_, file, _, ok := runtime.Caller(0)
	if !ok {
		t.Fatal("cannot locate the test source")
	}
	return filepath.Dir(filepath.Dir(file))
}

func TestNoRustAssetsInRepository(t *testing.T) {
	root := repoRoot(t)
	forbidden := []string{"*.rs", "Cargo.toml", "Cargo.lock", "build.rs", "rust-toolchain.toml"}
	for _, pattern := range forbidden {
		err := filepath.WalkDir(root, func(path string, entry os.DirEntry, err error) error {
			if err != nil {
				return err
			}
			if entry.IsDir() {
				if entry.Name() == ".git" {
					return filepath.SkipDir
				}
				return nil
			}
			matched, err := filepath.Match(pattern, entry.Name())
			if err != nil {
				return err
			}
			if matched {
				t.Errorf("forbidden asset %s", path)
			}
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
	}
}

func TestNoToolchainCallsInGoOrTools(t *testing.T) {
	root := repoRoot(t)
	for _, dir := range []string{"go", "tools"} {
		err := filepath.WalkDir(filepath.Join(root, dir), func(path string, entry os.DirEntry, err error) error {
			if err != nil {
				return err
			}
			if entry.IsDir() || !hasToolExtension(entry.Name()) {
				return nil
			}
			data, err := os.ReadFile(path)
			if err != nil {
				return err
			}
			for i, line := range strings.Split(string(data), "\n") {
				if toolchainCall.MatchString(line) {
					t.Errorf("%s:%d mentions a forbidden toolchain call: %s", path, i+1, line)
				}
			}
			return nil
		})
		if err != nil {
			t.Fatal(err)
		}
	}
}

func hasToolExtension(name string) bool {
	switch filepath.Ext(name) {
	case ".go", ".py", ".ps1", ".sh":
		return true
	}
	return false
}

func TestExternalAllowlistHasNoRustPaths(t *testing.T) {
	root := repoRoot(t)
	gitNext := filepath.Join(root, "..", "git-next")
	// This assertion is a post-migration contract. While git-next still owns
	// its own Rust tree, its historical evidence lists legitimately describe
	// that in-flight work; the guard activates once that migration lands.
	for _, marker := range []string{"Cargo.toml", "accept-runner/Cargo.toml", "rust-toolchain.toml"} {
		if _, err := os.Stat(filepath.Join(gitNext, marker)); err == nil {
			t.Skip("git-next Rust migration is still in progress")
		}
	}
	devtool := filepath.Join(gitNext, "tools", "devtool")
	entries, err := os.ReadDir(devtool)
	if os.IsNotExist(err) {
		t.Skip("git-next tooling is not present beside this checkout")
	}
	if err != nil {
		t.Fatal(err)
	}
	forbidden := []string{
		"../git-terminal/src/",
		"../git-terminal/Cargo.toml",
		"../git-terminal/build.rs",
		"../git-terminal/target/release",
		"../git-terminal/vendor/",
		"../git-terminal/tests/windows_desktop_acceptance.py",
	}
	for _, entry := range entries {
		if entry.IsDir() || filepath.Ext(entry.Name()) != ".go" {
			continue
		}
		path := filepath.Join(devtool, entry.Name())
		data, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		for _, unwanted := range forbidden {
			if strings.Contains(string(data), unwanted) {
				t.Errorf("%s still names %s", path, unwanted)
			}
		}
	}
}
