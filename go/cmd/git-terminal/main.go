// Copyright 2026 git-next contributors. MIT license.
//
// Command git-terminal: a native terminal with git-next as its owned Git.
package main

import (
	"encoding/json"
	"fmt"
	"os"
	"strings"

	gt "github.com/neko233-com/git-terminal"
)

type options struct {
	cwd                string
	cdToHome           bool
	noCD               bool
	noProfile          bool
	noShellIntegration bool
	shell              string
	command            string
	git                string
	printSetup         bool
	jsonOutput         bool
	version            bool
	help               bool
	languageAction     string
	languageValue      string
}

func usage() {
	fmt.Fprintln(os.Stderr, `Usage: git-terminal [OPTIONS] [language COMMAND]

Options:
  --cwd, --cd PATH   Start in PATH
  --cd-to-home       Start in the home directory
  --no-cd            Ignore --cwd
  --no-profile       Do not load the user's shell profile
  --no-shell-integration  Disable the managed prompt
  --shell auto|bash|powershell|cmd|shell  Shell profile
  --command COMMAND  Run COMMAND, then exit with its status
  --git PATH         Use this git-next executable (identity is verified)
  --print-setup      Print the resolved session setup as JSON and exit
  --json             JSON output where supported
  -V, --version      Print the version
  -h, --help         Print this help

Language commands: language list | language get | language set CODE`)
}

func parseLanguage(args []string) (*options, error) {
	opts := &options{}
	if len(args) == 0 {
		return nil, fmt.Errorf("language needs a command: list, get or set")
	}
	opts.languageAction = args[0]
	switch args[0] {
	case "list", "get":
		if len(args) > 1 {
			return nil, fmt.Errorf("language %s takes no arguments", args[0])
		}
	case "set":
		if len(args) != 2 || strings.TrimSpace(args[1]) == "" {
			return nil, fmt.Errorf("language set needs a language code")
		}
		opts.languageValue = args[1]
	default:
		return nil, fmt.Errorf("unknown language command %q", args[0])
	}
	return opts, nil
}

func parseArgs(args []string) (*options, error) {
	if len(args) > 0 && args[0] == "language" {
		return parseLanguage(args[1:])
	}
	opts := &options{}
	for i := 0; i < len(args); i++ {
		arg := args[i]
		if arg == "language" {
			parsed, err := parseLanguage(args[i+1:])
			if err != nil {
				return nil, err
			}
			parsed.cwd, parsed.shell, parsed.git = opts.cwd, opts.shell, opts.git
			return parsed, nil
		}
		if !strings.HasPrefix(arg, "-") {
			return nil, fmt.Errorf("unexpected argument %q", arg)
		}
		name, value, hasValue := strings.Cut(strings.TrimLeft(arg, "-"), "=")
		needsValue := func() (string, error) {
			if hasValue {
				return value, nil
			}
			if i+1 < len(args) {
				i++
				return args[i], nil
			}
			return "", fmt.Errorf("--%s needs a value", name)
		}
		switch name {
		case "cwd", "cd":
			v, err := needsValue()
			if err != nil {
				return nil, err
			}
			opts.cwd = v
		case "cd-to-home":
			opts.cdToHome = true
		case "no-cd":
			opts.noCD = true
		case "no-profile":
			opts.noProfile = true
		case "no-shell-integration":
			opts.noShellIntegration = true
		case "shell":
			v, err := needsValue()
			if err != nil {
				return nil, err
			}
			opts.shell = v
		case "command":
			v, err := needsValue()
			if err != nil {
				return nil, err
			}
			opts.command = v
		case "git":
			v, err := needsValue()
			if err != nil {
				return nil, err
			}
			opts.git = v
		case "print-setup":
			opts.printSetup = true
		case "json":
			opts.jsonOutput = true
		case "version", "V":
			opts.version = true
		case "help", "h":
			opts.help = true
		default:
			return nil, fmt.Errorf("unknown option --%s", name)
		}
	}
	if opts.cdToHome && opts.noCD {
		return nil, fmt.Errorf("--cd-to-home conflicts with --no-cd")
	}
	return opts, nil
}

func printJSON(value any) error {
	data, err := json.MarshalIndent(value, "", "  ")
	if err != nil {
		return err
	}
	fmt.Println(string(data))
	return nil
}

func runLanguage(opts *options, settings *gt.Settings) error {
	switch opts.languageAction {
	case "list":
		for _, language := range gt.AvailableLanguages() {
			fmt.Printf("%s\t%s\t%s\n", language.Code, language.Name, language.EnglishName)
		}
		language := gt.ActiveLanguage(settings.Language)
		fmt.Printf("auto\t%s\n", gt.Translate(language, "Follow the device language"))
	case "get":
		language := gt.ActiveLanguage(settings.Language)
		supported := make([]string, 0)
		for _, entry := range gt.AvailableLanguages() {
			supported = append(supported, entry.Code)
		}
		return printJSON(map[string]any{
			"product": "git-terminal", "configured": settings.Language,
			"language": language.Code, "name": language.Name,
			"english_name": language.EnglishName, "supported": supported,
		})
	case "set":
		language, ok := gt.Resolve(opts.languageValue)
		if !ok {
			return fmt.Errorf("unsupported language; use one of the listed languages or auto")
		}
		settings.Language = gt.Canonical(opts.languageValue)
		if err := settings.Save(); err != nil {
			return err
		}
		return printJSON(map[string]any{
			"product": "git-terminal", "configured": settings.Language,
			"language": language.Code, "name": language.Name,
		})
	}
	return nil
}

func run(args []string) (int, error) {
	opts, err := parseArgs(args)
	if err != nil {
		return 1, err
	}
	if opts.help {
		usage()
		return 0, nil
	}
	if opts.version {
		fmt.Println("git-terminal " + gt.Version)
		return 0, nil
	}
	settings := gt.LoadSettings()
	if opts.languageAction != "" {
		return 0, runLanguage(opts, &settings)
	}
	cwd := ""
	if opts.cdToHome {
		cwd = os.Getenv("USERPROFILE")
		if cwd == "" {
			cwd = os.Getenv("HOME")
		}
		if cwd == "" {
			return 1, fmt.Errorf("home directory is unavailable")
		}
	} else if !opts.noCD {
		cwd = opts.cwd
		if cwd == "" {
			cwd, err = os.Getwd()
			if err != nil {
				return 1, err
			}
		}
	} else {
		cwd, err = os.Getwd()
		if err != nil {
			return 1, err
		}
	}
	profile := settings.Shell
	if opts.shell != "" && opts.shell != "auto" {
		profile = opts.shell
	}
	setup, err := gt.DiscoverSetup(cwd, profile, opts.git)
	if err != nil {
		return 1, err
	}
	setup.NoProfile = opts.noProfile
	if opts.noShellIntegration {
		setup.Integration = nil
	}
	supported := setup.Integration != nil && (setup.Profile == "bash" || setup.Profile == "powershell")
	setup.SyntaxHighlighting = settings.SyntaxHighlighting && supported
	setup.GitPrompt = settings.GitPrompt && supported
	if opts.printSetup {
		return 0, printJSON(setup)
	}
	return gt.RunDesktop(setup, settings, opts.command), nil
}

func main() {
	code, err := run(os.Args[1:])
	if err != nil {
		fmt.Fprintln(os.Stderr, "Git-Terminal:", err)
		os.Exit(1)
	}
	os.Exit(code)
}
