// Copyright 2026 git-next contributors. MIT license.
//
// A language is a document in locales/. English is the source: the strings in
// the program are English, a document lists only the messages it overrides,
// and a missing message falls back to the English source per message.
package gitterminal

import (
	"embed"
	"encoding/json"
	"os"
	"sort"
	"strings"
	"sync"
)

//go:embed locales/*.json
var localeFS embed.FS

//go:embed shell-integration/*
var integrationFS embed.FS

const sourceLanguage = "en"

type localeDocument struct {
	Code        string            `json:"code"`
	Name        string            `json:"name"`
	EnglishName string            `json:"english_name"`
	Messages    map[string]string `json:"messages"`
}

// Language is a locale shipped with this build.
type Language struct {
	Code        string
	Name        string
	EnglishName string
}

var (
	localesOnce sync.Once
	localeDocs  map[string]*localeDocument // normalized code -> document
	localeList  []Language
)

func loadLocales() {
	localesOnce.Do(func() {
		localeDocs = make(map[string]*localeDocument)
		entries, err := localeFS.ReadDir("locales")
		if err != nil {
			return
		}
		for _, entry := range entries {
			if entry.IsDir() || !strings.HasSuffix(entry.Name(), ".json") {
				continue
			}
			data, err := localeFS.ReadFile("locales/" + entry.Name())
			if err != nil {
				continue
			}
			var doc localeDocument
			if json.Unmarshal(data, &doc) != nil {
				continue
			}
			// The file name is the identity of a locale.
			doc.Code = strings.TrimSuffix(entry.Name(), ".json")
			localeDocs[normalizeCode(doc.Code)] = &doc
		}
		// The source language has no document, but must still resolve.
		localeDocs[sourceLanguage] = &localeDocument{
			Code: sourceLanguage, Name: "English", EnglishName: "English",
		}
		for _, doc := range localeDocs {
			name := doc.EnglishName
			if name == "" {
				name = doc.Name
			}
			localeList = append(localeList, Language{Code: doc.Code, Name: doc.Name, EnglishName: name})
		}
		sort.Slice(localeList, func(i, j int) bool { return localeList[i].Code < localeList[j].Code })
	})
}

func availableLanguages() []Language {
	loadLocales()
	return append([]Language(nil), localeList...)
}

func normalizeCode(code string) string {
	return strings.ToLower(strings.TrimSpace(strings.ReplaceAll(code, "_", "-")))
}

func findLocale(code string) *localeDocument {
	loadLocales()
	return localeDocs[normalizeCode(code)]
}

func toLanguage(doc *localeDocument) Language {
	name := doc.EnglishName
	if name == "" {
		name = doc.Name
	}
	return Language{Code: doc.Code, Name: doc.Name, EnglishName: name}
}

// matchLocale walks a device locale from its most specific form to its bare
// language. A region implies a writing system for zh, so zh-TW does not end
// up on the simplified document.
func matchLocale(locale string) (Language, bool) {
	normalized := normalizeCode(locale)
	if normalized == "" || normalized == "c" || normalized == "posix" {
		return Language{}, false
	}
	parts := strings.Split(normalized, "-")
	candidates := []string{normalized}
	if len(parts) > 1 {
		if script := scriptForRegion(parts[len(parts)-1]); script != "" {
			withScript := parts[0] + "-" + script
			if withScript != normalized {
				candidates = append(candidates, withScript)
			}
		}
		withoutRegion := strings.Join(parts[:len(parts)-1], "-")
		if withoutRegion != normalized {
			candidates = append(candidates, withoutRegion)
		}
		if len(parts) > 2 && parts[0] != normalized {
			candidates = append(candidates, parts[0])
		}
	}
	for _, candidate := range candidates {
		if doc := findLocale(candidate); doc != nil {
			return toLanguage(doc), true
		}
	}
	return Language{}, false
}

func scriptForRegion(region string) string {
	switch region {
	case "cn", "sg", "my":
		return "hans"
	case "tw", "hk", "mo":
		return "hant"
	default:
		return ""
	}
}

// systemLanguage consults the native preferred-UI-language API first: a
// Windows shell may export an environment locale unrelated to the language
// the user chose. Environment variables are used only where no native API
// exists.
func systemLanguage() Language {
	for _, locale := range preferredLocales() {
		if language, ok := matchLocale(locale); ok {
			return language
		}
	}
	for _, key := range []string{"LC_ALL", "LC_MESSAGES", "LANGUAGE", "LANG"} {
		value := os.Getenv(key)
		if value == "" {
			continue
		}
		entries := strings.Split(value, ":")
		for i, entry := range entries {
			if before, _, found := strings.Cut(entry, "."); found {
				entries[i] = before
			}
			if before, _, found := strings.Cut(entries[i], "@"); found {
				entries[i] = before
			}
		}
		for _, entry := range entries {
			if language, ok := matchLocale(entry); ok {
				return language
			}
		}
		// A locale that states no preference, or names a language with no
		// document, honestly falls back to the source language.
		return fallbackLanguage()
	}
	return fallbackLanguage()
}

func fallbackLanguage() Language {
	if doc := findLocale(sourceLanguage); doc != nil {
		return toLanguage(doc)
	}
	return Language{Code: sourceLanguage, Name: "English", EnglishName: "English"}
}

func legacyAlias(value string) string {
	switch normalizeCode(value) {
	case "english", "en", "en-us":
		return sourceLanguage
	case "chinese", "zh", "zh-cn", "zh-hans", "中文":
		return "zh-Hans"
	default:
		return ""
	}
}

// resolve maps a user selection to the language it selects.
func resolve(selection string) (Language, bool) {
	trimmed := strings.TrimSpace(selection)
	if trimmed == "" {
		return Language{}, false
	}
	switch normalizeCode(trimmed) {
	case "auto", "system":
		return systemLanguage(), true
	}
	if alias := legacyAlias(trimmed); alias != "" {
		if doc := findLocale(alias); doc != nil {
			return toLanguage(doc), true
		}
	}
	return matchLocale(trimmed)
}

// canonical is the stable value stored in settings.json.
func canonical(selection string) string {
	trimmed := strings.TrimSpace(selection)
	switch normalizeCode(trimmed) {
	case "auto", "system":
		return "auto"
	}
	if alias := legacyAlias(trimmed); alias != "" {
		return alias
	}
	if language, ok := resolve(trimmed); ok {
		return language.Code
	}
	return normalizeCode(trimmed)
}

// translate falls back to the English source text, per message.
func translate(language Language, english string) string {
	if language.Code == sourceLanguage {
		return english
	}
	doc := findLocale(language.Code)
	if doc == nil {
		return english
	}
	if translated, ok := doc.Messages[english]; ok {
		return translated
	}
	return english
}

// activeLanguage is the language described by the configured selection.
func activeLanguage(configured string) Language {
	if language, ok := resolve(configured); ok {
		return language
	}
	return systemLanguage()
}
