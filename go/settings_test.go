// Copyright 2026 git-next contributors. MIT license.
package gitterminal

import (
	"encoding/json"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
)

func isolatedConfig(t *testing.T) string {
	t.Helper()
	base := t.TempDir()
	t.Setenv("APPDATA", base)
	t.Setenv("XDG_CONFIG_HOME", filepath.Join(base, "xdg"))
	t.Setenv("HOME", filepath.Join(base, "home"))
	return base
}

func TestSettingsPathPrecedence(t *testing.T) {
	base := isolatedConfig(t)
	want := filepath.Join(base, "git-terminal", "settings.json")
	if got := settingsPath(); got != want {
		t.Fatalf("APPDATA path = %q, want %q", got, want)
	}
	t.Setenv("APPDATA", "")
	want = filepath.Join(base, "xdg", "git-terminal", "settings.json")
	if got := settingsPath(); got != want {
		t.Fatalf("XDG path = %q, want %q", got, want)
	}
	t.Setenv("XDG_CONFIG_HOME", "")
	want = filepath.Join(base, "home", ".config", "git-terminal", "settings.json")
	if got := settingsPath(); got != want {
		t.Fatalf("HOME path = %q, want %q", got, want)
	}
}

func TestLoadSettingsFallsBackOnCorruptJSON(t *testing.T) {
	base := isolatedConfig(t)
	path := settingsPath()
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}
	for _, data := range []string{"not json", "{", `[]`, `{"font_size":"big"}`} {
		if err := os.WriteFile(path, []byte(data), 0o600); err != nil {
			t.Fatal(err)
		}
		got := loadSettings()
		if got != defaultSettings() {
			t.Errorf("corrupt document %q loaded as %+v", data, got)
		}
	}
	// A document that cannot be read is also a fallback, not an error.
	if err := os.Remove(path); err != nil {
		t.Fatal(err)
	}
	if got := loadSettings(); got != defaultSettings() {
		t.Errorf("missing document loaded as %+v", got)
	}
	_ = base
}

func TestLoadSettingsFillsMissingFields(t *testing.T) {
	isolatedConfig(t)
	path := settingsPath()
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}
	partial := `{"language":"ja","font_family":"Custom"}`
	if err := os.WriteFile(path, []byte(partial), 0o600); err != nil {
		t.Fatal(err)
	}
	got := loadSettings()
	defaults := defaultSettings()
	if got.Language != "ja" || got.FontFamily != "Custom" {
		t.Errorf("stored fields lost: %+v", got)
	}
	if got.Shell != "auto" || got.FontSize != defaults.FontSize || got.Scrollback != defaults.Scrollback {
		t.Errorf("missing fields not filled: %+v", got)
	}
	if got.SyntaxHighlighting != defaults.SyntaxHighlighting || got.GitPrompt != defaults.GitPrompt {
		t.Errorf("missing booleans not filled: %+v", got)
	}
}

func TestLoadSettingsClampsOutOfRange(t *testing.T) {
	isolatedConfig(t)
	path := settingsPath()
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		t.Fatal(err)
	}
	for _, test := range []struct {
		doc    string
		font   float64
		scroll int
	}{
		{`{"font_size":4,"scrollback":10}`, 16, 20000},
		{`{"font_size":0,"scrollback":0}`, 16, 20000},
		{`{"font_size":99,"scrollback":999999}`, 16, 20000},
		{`{"font_size":12,"scrollback":40000}`, 12, 40000},
	} {
		if err := os.WriteFile(path, []byte(test.doc), 0o600); err != nil {
			t.Fatal(err)
		}
		got := loadSettings()
		if got.FontSize != test.font || got.Scrollback != test.scroll {
			t.Errorf("%s: font=%v scroll=%v, want %v/%v", test.doc, got.FontSize, got.Scrollback, test.font, test.scroll)
		}
	}
}

func TestSettingsSaveIsAtomicAndComplete(t *testing.T) {
	base := isolatedConfig(t)
	path := settingsPath()
	settings := defaultSettings()
	settings.Language = "zh-Hans"
	settings.FontSize = 18
	if err := settings.Save(); err != nil {
		t.Fatal(err)
	}
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if data[len(data)-1] != '\n' {
		t.Error("saved document does not end with a newline")
	}
	var stored Settings
	if err := json.Unmarshal(data, &stored); err != nil {
		t.Fatal(err)
	}
	if stored != settings {
		t.Errorf("round trip = %+v, want %+v", stored, settings)
	}
	// Saving again replaces the whole document and leaves no temporary file.
	settings.Language = "de"
	if err := settings.Save(); err != nil {
		t.Fatal(err)
	}
	data, err = os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(data, &stored); err != nil {
		t.Fatal(err)
	}
	if stored.Language != "de" {
		t.Errorf("second save kept language %q", stored.Language)
	}
	entries, err := os.ReadDir(filepath.Dir(path))
	if err != nil {
		t.Fatal(err)
	}
	for _, entry := range entries {
		if strings.HasSuffix(entry.Name(), ".tmp") {
			t.Errorf("temporary file left behind: %s", entry.Name())
		}
	}
	info, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	if runtime.GOOS != "windows" && info.Mode().Perm() != 0o600 {
		t.Errorf("settings mode = %o, want 600", info.Mode().Perm())
	}
	_ = base
}
