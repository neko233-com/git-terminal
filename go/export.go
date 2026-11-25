// Copyright 2026 git-next contributors. MIT license.
//
// Exported entry points used by cmd/git-terminal. The implementation lives
// beside its tests in this package.
package gitterminal

const Version = productVersion

func AvailableLanguages() []Language                     { return availableLanguages() }
func ActiveLanguage(configured string) Language          { return activeLanguage(configured) }
func Translate(language Language, english string) string { return translate(language, english) }
func Resolve(selection string) (Language, bool)          { return resolve(selection) }
func Canonical(selection string) string                  { return canonical(selection) }
func LoadSettings() Settings                             { return loadSettings() }
func (s Settings) Save() error                           { return s.save() }
func DiscoverSetup(cwd, profile, git string) (*Setup, error) {
	return discoverSetup(cwd, profile, git)
}
