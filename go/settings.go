// Copyright 2026 git-next contributors. MIT license.
package gitterminal

import (
	"encoding/json"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
)

// Settings mirrors the fields of the Rust implementation so existing
// configuration files keep working.
type Settings struct {
	Language           string  `json:"language"`
	FontSize           float64 `json:"font_size"`
	FontFamily         string  `json:"font_family"`
	Scrollback         int     `json:"scrollback"`
	Shell              string  `json:"shell"`
	SyntaxHighlighting bool    `json:"syntax_highlighting"`
	GitPrompt          bool    `json:"git_prompt"`
}

func defaultSettings() Settings {
	font := "monospace"
	if runtime.GOOS == "windows" {
		font = "Consolas"
	}
	return Settings{
		Language: "auto", FontSize: 16, FontFamily: font, Scrollback: 20000,
		Shell: "auto", SyntaxHighlighting: true, GitPrompt: true,
	}
}

func settingsPath() string {
	base := os.Getenv("APPDATA")
	if base == "" {
		base = os.Getenv("XDG_CONFIG_HOME")
	}
	if base == "" {
		base = filepath.Join(os.Getenv("HOME"), ".config")
	}
	return filepath.Join(base, "git-terminal", "settings.json")
}

func loadSettings() Settings {
	settings := defaultSettings()
	data, err := os.ReadFile(settingsPath())
	if err == nil {
		var stored Settings
		if json.Unmarshal(data, &stored) == nil {
			settings = stored
		}
	}
	// A file written by an older build may not have every field.
	if settings.Language == "" {
		settings.Language = "auto"
	}
	if settings.FontFamily == "" {
		settings.FontFamily = defaultSettings().FontFamily
	}
	if settings.Shell == "" {
		settings.Shell = "auto"
	}
	if settings.FontSize < 8 || settings.FontSize > 48 {
		settings.FontSize = 16
	}
	if settings.Scrollback < 1000 || settings.Scrollback > 100000 {
		settings.Scrollback = 20000
	}
	return settings
}

func (s Settings) save() error {
	path := settingsPath()
	if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
		return err
	}
	data, err := json.MarshalIndent(s, "", "  ")
	if err != nil {
		return err
	}
	temp := filepath.Join(filepath.Dir(path), "settings-"+strconv.Itoa(os.Getpid())+".tmp")
	file, err := os.OpenFile(temp, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0o600)
	if err != nil {
		return err
	}
	if _, err = file.Write(append(data, '\n')); err != nil {
		_ = file.Close()
		_ = os.Remove(temp)
		return err
	}
	if err = file.Sync(); err != nil {
		_ = file.Close()
		_ = os.Remove(temp)
		return err
	}
	if err = file.Close(); err != nil {
		_ = os.Remove(temp)
		return err
	}
	if err = os.Rename(temp, path); err != nil && runtime.GOOS == "windows" {
		// os.Rename does not replace an existing file on Windows.
		if os.Remove(path) == nil {
			err = os.Rename(temp, path)
		}
	}
	if err != nil {
		_ = os.Remove(temp)
	}
	return err
}
