// Copyright 2026 git-next contributors. MIT license.
//
// A Go port of tools/check_locales.py. A language is a document; the Go
// source is the English source corpus. The checker verifies the document,
// not the quality of a translation.
package localecheck

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strconv"
	"strings"
	"unicode/utf8"
)

type keyValue struct {
	key   string
	value json.RawMessage
}

type document struct {
	Code        string            `json:"code"`
	Name        string            `json:"name"`
	EnglishName string            `json:"english_name"`
	Messages    map[string]string `json:"messages"`
}

// CheckResult describes one checked language directory.
type CheckResult struct {
	Problems []string
	Report   string
}

var namedPlaceholder = regexp.MustCompile(`\{([a-z_][a-z0-9_]*)\}`)

func placeholders(text string) map[string]bool {
	found := make(map[string]bool)
	for _, match := range namedPlaceholder.FindAllStringSubmatch(text, -1) {
		if len(match[1]) > 1 { // a single letter is a plural marker
			found[match[1]] = true
		}
	}
	return found
}

// present reports whether an English message is produced by the Go source.
func present(key, corpus string) bool {
	if strings.Contains(corpus, strconv.Quote(key)) {
		return true
	}
	if strings.Contains(corpus, "`"+key+"`") {
		return true
	}
	return strings.Contains(corpus, key)
}

func sourceCorpus(root string) (string, error) {
	var parts []string
	err := filepath.WalkDir(root, func(path string, entry fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if entry.IsDir() {
			if entry.Name() == ".git" || entry.Name() == "target" {
				return filepath.SkipDir
			}
			return nil
		}
		if strings.HasSuffix(entry.Name(), ".go") {
			data, err := os.ReadFile(path)
			if err != nil {
				return err
			}
			parts = append(parts, string(data))
		}
		return nil
	})
	if err != nil {
		return "", err
	}
	sort.Strings(parts)
	return strings.Join(parts, "\n"), nil
}

// decodePairs reads a JSON object in file order, so duplicate keys and
// unsorted messages can be reported instead of being silently collapsed.
func decodePairs(data []byte) ([]keyValue, error) {
	decoder := json.NewDecoder(bytes.NewReader(data))
	token, err := decoder.Token()
	if err != nil {
		return nil, err
	}
	if delimiter, ok := token.(json.Delim); !ok || delimiter != '{' {
		return nil, fmt.Errorf("expected a JSON object")
	}
	var pairs []keyValue
	for decoder.More() {
		keyToken, err := decoder.Token()
		if err != nil {
			return nil, err
		}
		key, ok := keyToken.(string)
		if !ok {
			return nil, fmt.Errorf("object key is not a string")
		}
		var value json.RawMessage
		if err := decoder.Decode(&value); err != nil {
			return nil, err
		}
		pairs = append(pairs, keyValue{key: key, value: value})
	}
	if _, err := decoder.Token(); err != nil {
		return nil, err
	}
	return pairs, nil
}

func duplicateKeys(pairs []keyValue) []string {
	seen := make(map[string]bool)
	var duplicates []string
	for _, pair := range pairs {
		if seen[pair.key] {
			duplicates = append(duplicates, pair.key)
		}
		seen[pair.key] = true
	}
	return duplicates
}

// Check validates every locale document under root/locales against the Go
// source under root. With fix it first sorts each document's message keys.
func Check(root string, fix, coverage bool) (CheckResult, error) {
	localesDir := filepath.Join(root, "locales")
	files, err := filepath.Glob(filepath.Join(localesDir, "*.json"))
	if err != nil {
		return CheckResult{}, err
	}
	if len(files) == 0 {
		return CheckResult{}, fmt.Errorf("no locale documents in %s", localesDir)
	}
	sort.Strings(files)
	if fix {
		for _, path := range files {
			if err := sortDocument(path); err != nil {
				return CheckResult{}, err
			}
		}
	}
	corpus, err := sourceCorpus(root)
	if err != nil {
		return CheckResult{}, err
	}
	result := CheckResult{}
	reference := make(map[string]bool)
	counts := make(map[string]int)
	for _, path := range files {
		name := filepath.Base(path)
		data, err := os.ReadFile(path)
		if err != nil {
			result.Problems = append(result.Problems, name+": cannot read document")
			continue
		}
		result.Problems = append(result.Problems, checkDocument(name, data, corpus, reference, counts)...)
	}
	if coverage {
		var report strings.Builder
		fmt.Fprintf(&report, "%d messages known to the shipped languages\n", len(reference))
		for _, path := range files {
			code := strings.TrimSuffix(filepath.Base(path), ".json")
			total := len(reference)
			percent := 0
			if total > 0 {
				percent = 100 * counts[code] / total
			}
			fmt.Fprintf(&report, "%-10s messages %4d/%-4d (%3d%%)  falling back to English: %d\n",
				code, counts[code], total, percent, total-counts[code])
		}
		result.Report = report.String()
	}
	return result, nil
}

func checkDocument(name string, data []byte, corpus string, reference map[string]bool, counts map[string]int) []string {
	var problems []string
	if bytes.HasPrefix(data, []byte{0xef, 0xbb, 0xbf}) {
		problems = append(problems, name+": starts with a byte order mark")
	}
	if bytes.Contains(data, []byte("\r\n")) {
		problems = append(problems, name+": uses CRLF line endings")
	}
	if !bytes.HasSuffix(data, []byte("\n")) {
		problems = append(problems, name+": does not end with a newline")
	}
	if !utf8.Valid(data) {
		problems = append(problems, name+": is not valid UTF-8")
	}
	topPairs, err := decodePairs(data)
	if err != nil {
		return append(problems, name+": is not valid JSON ("+err.Error()+")")
	}
	for _, key := range duplicateKeys(topPairs) {
		problems = append(problems, fmt.Sprintf("%s: defines %q more than once", name, key))
	}
	var doc document
	if err := json.Unmarshal(data, &doc); err != nil {
		return append(problems, name+": is not valid JSON ("+err.Error()+")")
	}
	code := strings.TrimSuffix(name, ".json")
	if doc.Code != code {
		problems = append(problems, fmt.Sprintf("%s: code is %q, but the file is named %q", name, doc.Code, code))
	}
	if doc.Name == "" {
		problems = append(problems, name+": name is missing or empty")
	}
	if doc.EnglishName == "" {
		problems = append(problems, name+": english_name is missing or empty")
	}

	var messagePairs []keyValue
	for _, pair := range topPairs {
		if pair.key == "messages" {
			messagePairs, err = decodePairs(pair.value)
			if err != nil {
				problems = append(problems, name+": messages is not an object")
				return problems
			}
			for _, key := range duplicateKeys(messagePairs) {
				problems = append(problems, fmt.Sprintf("%s: defines %q more than once", name, key))
			}
		}
	}
	if messagePairs == nil {
		problems = append(problems, name+": messages is not an object")
		return problems
	}
	keys := make([]string, 0, len(messagePairs))
	for _, pair := range messagePairs {
		keys = append(keys, pair.key)
		var value string
		if err := json.Unmarshal(pair.value, &value); err != nil {
			problems = append(problems, fmt.Sprintf("%s: message %q is not a string", name, pair.key))
			continue
		}
		if strings.TrimSpace(value) == "" {
			problems = append(problems, fmt.Sprintf("%s: message %q is empty", name, pair.key))
		}
		if !present(pair.key, corpus) {
			problems = append(problems, fmt.Sprintf("%s: message %q does not appear in the Go source", name, pair.key))
		}
		want, got := placeholders(pair.key), placeholders(value)
		if len(want) != len(got) {
			problems = append(problems, fmt.Sprintf("%s: message %q has placeholders %v but the translation has %v",
				name, pair.key, sortedKeys(want), sortedKeys(got)))
		} else {
			for placeholder := range want {
				if !got[placeholder] {
					problems = append(problems, fmt.Sprintf("%s: placeholder %q lost in %q", name, placeholder, pair.key))
				}
			}
		}
		if strings.HasSuffix(pair.key, "\n") != strings.HasSuffix(value, "\n") {
			problems = append(problems, fmt.Sprintf("%s: message %q and its translation disagree on a trailing newline", name, pair.key))
		}
		reference[pair.key] = true
	}
	if !sort.StringsAreSorted(keys) {
		problems = append(problems, name+": messages is not sorted; run --fix")
	}
	counts[code] = len(keys)
	return problems
}

func sortedKeys(set map[string]bool) []string {
	keys := make([]string, 0, len(set))
	for key := range set {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	return keys
}

// sortDocument rewrites one document with sorted message keys and stable
// field order. json.Marshal sorts map keys, which is what --fix needs.
func sortDocument(path string) error {
	data, err := os.ReadFile(path)
	if err != nil {
		return err
	}
	var doc document
	if err := json.Unmarshal(data, &doc); err != nil {
		return err
	}
	var buffer bytes.Buffer
	encoder := json.NewEncoder(&buffer)
	encoder.SetEscapeHTML(false)
	encoder.SetIndent("", "  ")
	if err := encoder.Encode(doc); err != nil {
		return err
	}
	return os.WriteFile(path, buffer.Bytes(), 0o644)
}

// Summary is the short success line printed by the command.
func Summary(files int) string {
	return fmt.Sprintf("%d locale documents checked: structure, stale keys, placeholders, whitespace, order", files)
}
