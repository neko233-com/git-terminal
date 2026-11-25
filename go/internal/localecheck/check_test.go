// Copyright 2026 git-next contributors. MIT license.
package localecheck

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestCheckShippedLocales(t *testing.T) {
	root, err := filepath.Abs("../..")
	if err != nil {
		t.Fatal(err)
	}
	result, err := Check(root, false, true)
	if err != nil {
		t.Fatal(err)
	}
	if len(result.Problems) != 0 {
		t.Fatalf("shipped locale documents have problems:\n%s", strings.Join(result.Problems, "\n"))
	}
	if !strings.Contains(result.Report, "messages known to the shipped languages") {
		t.Errorf("coverage report is empty: %q", result.Report)
	}
}

func TestPresentAndPlaceholders(t *testing.T) {
	corpus := "ui.Text(c, \"Follow the device language\")\nvar raw = `New Ctrl+T    Close Ctrl+W`\nplainMarker"
	for _, key := range []string{"Follow the device language", "New Ctrl+T    Close Ctrl+W", "plainMarker"} {
		if !present(key, corpus) {
			t.Errorf("present(%q) = false", key)
		}
	}
	if present("a message that is not in the source", corpus) {
		t.Error("a missing message was reported as present")
	}
	found := placeholders("Connect {host} to {port} and {x}")
	if !found["host"] || !found["port"] || found["x"] {
		t.Errorf("placeholders = %v, want host and port only", found)
	}
}

func TestCheckReportsStructuralProblems(t *testing.T) {
	root := t.TempDir()
	if err := os.MkdirAll(filepath.Join(root, "locales"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "source.go"), []byte("package main\n// \"Cancel\"\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	bad := `{
  "code": "en",
  "name": "",
  "english_name": "Bad",
  "messages": {
    "Cancel": "ab {other}",
    "Cancel": "duplicate"
  }
}`
	path := filepath.Join(root, "locales", "de.json")
	if err := os.WriteFile(path, []byte(bad), 0o644); err != nil {
		t.Fatal(err)
	}
	result, err := Check(root, false, false)
	if err != nil {
		t.Fatal(err)
	}
	joined := strings.Join(result.Problems, "\n")
	for _, want := range []string{"name is missing", "defines \"Cancel\" more than once", "placeholders"} {
		if !strings.Contains(joined, want) {
			t.Errorf("problems miss %q:\n%s", want, joined)
		}
	}
}

func TestFixSortsMessages(t *testing.T) {
	root := t.TempDir()
	if err := os.MkdirAll(filepath.Join(root, "locales"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(root, "source.go"), []byte("package main\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(root, "locales", "de.json")
	original := "{\"code\":\"de\",\"name\":\"Deutsch\",\"english_name\":\"German\",\"messages\":{\"Zulu\":\"z\",\"Alpha\":\"a\"}}"
	if err := os.WriteFile(path, []byte(original), 0o644); err != nil {
		t.Fatal(err)
	}
	if _, err := Check(root, true, false); err != nil {
		t.Fatal(err)
	}
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Index(string(data), `"Alpha"`) > strings.Index(string(data), `"Zulu"`) {
		t.Errorf("--fix did not sort keys: %s", data)
	}
}
