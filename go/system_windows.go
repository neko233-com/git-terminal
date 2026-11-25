// Copyright 2026 git-next contributors. MIT license.
//go:build windows

package gitterminal

import "golang.org/x/sys/windows"

// preferredLocales reads the language the user chose for Windows itself,
// rather than a locale a shell happened to export.
func preferredLocales() []string {
	locales, err := windows.GetUserPreferredUILanguages(0)
	if err != nil {
		return nil
	}
	return locales
}
