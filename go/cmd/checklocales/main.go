// Copyright 2026 git-next contributors. MIT license.
//
// Command checklocales validates go/locales against the Go source.
//
//	go run ./cmd/checklocales [--coverage] [--fix]
package main

import (
	"flag"
	"fmt"
	"os"
	"path/filepath"

	"github.com/neko233-com/git-terminal/internal/localecheck"
)

func main() {
	root := flag.String("root", ".", "the Go module directory containing locales/")
	coverage := flag.Bool("coverage", false, "also report what each language leaves to English")
	fix := flag.Bool("fix", false, "sort the keys of every document, then check")
	flag.Parse()
	result, err := localecheck.Check(*root, *fix, *coverage)
	if err != nil {
		fmt.Fprintln(os.Stderr, "checklocales:", err)
		os.Exit(1)
	}
	if *coverage && result.Report != "" {
		fmt.Print(result.Report)
		fmt.Println()
	}
	if len(result.Problems) > 0 {
		for _, problem := range result.Problems {
			fmt.Fprintln(os.Stderr, "error:", problem)
		}
		fmt.Fprintf(os.Stderr, "%d problem(s) in %s\n", len(result.Problems), filepath.Join(*root, "locales"))
		os.Exit(1)
	}
	files, _ := filepath.Glob(filepath.Join(*root, "locales", "*.json"))
	fmt.Println(localecheck.Summary(len(files)))
}
