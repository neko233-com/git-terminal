// Copyright 2026 git-next contributors. MIT license.
package gitterminal

import (
	"encoding/json"
	"regexp"
	"sort"
	"strings"
	"testing"
)

type testLocale struct {
	Code        string            `json:"code"`
	Name        string            `json:"name"`
	EnglishName string            `json:"english_name"`
	Messages    map[string]string `json:"messages"`
}

var namedPlaceholder = regexp.MustCompile(`\{([a-z_][a-z0-9_]*)\}`)

func placeholders(text string) map[string]bool {
	found := make(map[string]bool)
	for _, match := range namedPlaceholder.FindAllStringSubmatch(text, -1) {
		if len(match[1]) > 1 {
			found[match[1]] = true
		}
	}
	return found
}

func TestLocaleDocuments(t *testing.T) {
	entries, err := localeFS.ReadDir("locales")
	if err != nil {
		t.Fatal(err)
	}
	if len(entries) != 9 {
		t.Fatalf("expected 9 locale documents, got %d", len(entries))
	}
	for _, entry := range entries {
		data, err := localeFS.ReadFile("locales/" + entry.Name())
		if err != nil {
			t.Fatal(err)
		}
		var doc testLocale
		if err := json.Unmarshal(data, &doc); err != nil {
			t.Fatalf("%s: invalid JSON: %v", entry.Name(), err)
		}
		code := strings.TrimSuffix(entry.Name(), ".json")
		if doc.Code != code {
			t.Errorf("%s: code %q does not match file name", entry.Name(), doc.Code)
		}
		if doc.Name == "" || doc.EnglishName == "" {
			t.Errorf("%s: missing language name", entry.Name())
		}
		for key, value := range doc.Messages {
			if strings.TrimSpace(value) == "" {
				t.Errorf("%s: empty translation of %q", entry.Name(), key)
			}
			want, got := placeholders(key), placeholders(value)
			if len(want) != len(got) {
				t.Errorf("%s: placeholders changed in %q", entry.Name(), key)
			} else {
				for name := range want {
					if !got[name] {
						t.Errorf("%s: placeholder %q lost in %q", entry.Name(), name, key)
					}
				}
			}
			if strings.HasSuffix(key, "\n") != strings.HasSuffix(value, "\n") {
				t.Errorf("%s: trailing newline differs in %q", entry.Name(), key)
			}
		}
		// The documents are sorted by key (the Python checker enforces this).
		var raw struct {
			Messages json.RawMessage `json:"messages"`
		}
		if err := json.Unmarshal(data, &raw); err != nil {
			t.Fatal(err)
		}
		keyPattern := regexp.MustCompile(`"(?:[^"\\]|\\.)*"\s*:`)
		keys := make([]string, 0)
		for _, match := range keyPattern.FindAllString(string(raw.Messages), -1) {
			key := strings.TrimSpace(strings.TrimSuffix(match, ":"))
			var decoded string
			if err := json.Unmarshal([]byte(key), &decoded); err == nil {
				keys = append(keys, decoded)
			}
		}
		if !sort.StringsAreSorted(keys) {
			t.Errorf("%s: messages are not sorted", entry.Name())
		}
	}
}

func TestTranslateFallsBackPerMessage(t *testing.T) {
	all := availableLanguages()
	if len(all) != 10 { // nine documents plus the English source
		t.Fatalf("expected 10 languages, got %d", len(all))
	}
	chinese, ok := resolve("zh-Hans")
	if !ok {
		t.Fatal("zh-Hans did not resolve")
	}
	if got := translate(chinese, "Language"); got != "界面语言" {
		t.Errorf("translation = %q, want 界面语言", got)
	}
	if got := translate(chinese, "a message no document translates"); got != "a message no document translates" {
		t.Errorf("missing translation did not fall back: %q", got)
	}
	if got := translate(translateSource(), "Language"); got != "Language" {
		t.Errorf("source language changed a message: %q", got)
	}
}

func translateSource() Language {
	language, _ := resolve("en")
	return language
}

func TestDeviceLanguagePrecedesEnvironment(t *testing.T) {
	env := func(key string) string {
		if key == "LANG" {
			return "en_US.UTF-8"
		}
		return ""
	}
	if got := systemLanguageWith([]string{"zh-TW"}, env); got.Code != "zh-Hant" {
		t.Errorf("device zh-TW was overridden by LANG: %q", got.Code)
	}
	if got := systemLanguageWith([]string{"de-DE"}, env); got.Code != "de" {
		t.Errorf("device de-DE was overridden by LANG: %q", got.Code)
	}
	// An unusable device preference, or none at all, falls through to LANG.
	if got := systemLanguageWith([]string{"xx-YY"}, env); got.Code != "en" {
		t.Errorf("LANG fallback = %q, want en", got.Code)
	}
	if got := systemLanguageWith(nil, env); got.Code != "en" {
		t.Errorf("LANG fallback without device preference = %q, want en", got.Code)
	}
	// LC_ALL wins over LANG when no device language applies.
	got := systemLanguageWith(nil, func(key string) string {
		if key == "LC_ALL" {
			return "ja_JP"
		}
		return "en_US"
	})
	if got.Code != "ja" {
		t.Errorf("LC_ALL = %q, want ja", got.Code)
	}
	if got := systemLanguageWith(nil, func(string) string { return "" }); got.Code != "en" {
		t.Errorf("no preference = %q, want source language en", got.Code)
	}
}

func TestResolveAndCanonical(t *testing.T) {
	cases := []struct{ input, want string }{
		{"zh-CN", "zh-Hans"}, {"zh-TW", "zh-Hant"}, {"zh_Hans", "zh-Hans"},
		{"chinese", "zh-Hans"}, {"ja-JP", "ja"}, {"en-US", "en"}, {"auto", ""},
	}
	for _, test := range cases {
		language, ok := resolve(test.input)
		if !ok {
			t.Errorf("resolve(%q) failed", test.input)
			continue
		}
		if test.want != "" && language.Code != test.want {
			t.Errorf("resolve(%q) = %q, want %q", test.input, language.Code, test.want)
		}
	}
	for _, test := range []struct{ input, want string }{
		{"zh-CN", "zh-Hans"}, {"chinese", "zh-Hans"}, {"auto", "auto"}, {"bogus", "bogus"},
	} {
		if got := canonical(test.input); got != test.want {
			t.Errorf("canonical(%q) = %q, want %q", test.input, got, test.want)
		}
	}
	if _, ok := resolve("not-a-language"); ok {
		t.Error("an unknown language resolved")
	}
}
