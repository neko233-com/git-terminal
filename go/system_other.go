// Copyright 2026 git-next contributors. MIT license.
//go:build !windows

package gitterminal

// preferredLocales is empty where the platform offers no native preferred
// UI language API; systemLanguage then uses the locale environment.
func preferredLocales() []string { return nil }
