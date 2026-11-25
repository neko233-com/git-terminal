// Copyright 2026 git-next contributors. MIT license.
package gitterminal

import (
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/plugins/terminal"
	"github.com/egoist/mygo/ui"
)

type sessionTab struct {
	id    uint64
	title string
	term  *terminal.Terminal
}

type desktop struct {
	setup     *Setup
	settings  Settings
	command   string
	exitCode  *atomic.Int32
	manager   *SessionManager
	win       *mygo.Window
	ready     chan struct{}
	mu        sync.Mutex
	tabs      []*sessionTab
	active    int
	lastError string
}

// RunDesktop starts the native UI and returns the shell's exit status for
// --command. The terminal plugin owns each PTY; this desktop owns the
// terminals and closes every one of them when its window closes.
func RunDesktop(setup *Setup, settings Settings, command string) int {
	exit := &atomic.Int32{}
	d := &desktop{
		setup: setup, settings: settings, command: command, exitCode: exit,
		manager: NewSessionManager(), ready: make(chan struct{}),
	}
	if err := d.addTab(command); err != nil {
		fmt.Fprintln(os.Stderr, "Git-Terminal:", err)
		return 1
	}
	mygo.App.WhenReady(func() {
		d.win = mygo.NewWindow(mygo.WindowOptions{
			Title: "Git-Terminal", Width: 1120, Height: 720,
			MinWidth: 560, MinHeight: 340,
			Content: ui.View(d.view),
		})
		d.win.OnClosed(func() { _ = d.manager.CloseAll() })
		close(d.ready)
	})
	if err := mygo.App.Run(); err != nil {
		fmt.Fprintln(os.Stderr, "Git-Terminal:", err)
		return 1
	}
	return int(exit.Load())
}

func (d *desktop) addTab(command string) error {
	setup := *d.setup
	args, env := setup.launch(command)
	title := filepath.Base(setup.Cwd)
	if title == "" {
		title = "Terminal"
	}
	ready := d.ready
	var entry *Entry
	var term *terminal.Terminal
	var err error
	term, err = terminal.New(terminal.Options{
		Command: args,
		Dir:     setup.Cwd,
		Env:     env,
		// The setting counts lines; the mygo plugin counts bytes.
		Scrollback: d.settings.Scrollback * 100,
		OnTitle: func(shellTitle string) {
			<-ready // the window, and the entry, exist by now
			name := cleanTitle(shellTitle)
			if strings.HasPrefix(name, "git-terminal-prompt:") {
				name = strings.TrimSpace(strings.TrimPrefix(name, "git-terminal-prompt:"))
			}
			if name == "" {
				return
			}
			if entry != nil {
				entry.SetTitle(name)
			}
			d.mu.Lock()
			for _, tab := range d.tabs {
				if tab.term == term {
					tab.title = name
					break
				}
			}
			d.mu.Unlock()
			if d.win != nil {
				d.win.SetTitle("Git-Terminal — " + name)
				d.win.Invalidate()
			}
		},
		OnExit: func(code int) {
			<-ready
			d.exitCode.Store(int32(code))
			if entry != nil {
				entry.NotifyExit(code)
			}
			if d.win != nil {
				d.win.Invalidate()
			}
		},
	})
	if err != nil {
		return err
	}
	entry = d.manager.Add(title, term, func(id uint64, _ int) { d.closeTab(id) })
	d.mu.Lock()
	d.tabs = append(d.tabs, &sessionTab{id: entry.ID(), title: title, term: term})
	d.active = len(d.tabs) - 1
	d.mu.Unlock()
	return nil
}

func cleanTitle(title string) string {
	return strings.Map(func(r rune) rune {
		if r < 0x20 || r == 0x7f {
			return '?'
		}
		return r
	}, strings.TrimSpace(title))
}

func (d *desktop) closeTab(id uint64) {
	d.mu.Lock()
	index := -1
	for i, tab := range d.tabs {
		if tab.id == id {
			index = i
			break
		}
	}
	if index >= 0 {
		d.tabs = append(d.tabs[:index], d.tabs[index+1:]...)
	}
	if d.active >= len(d.tabs) && len(d.tabs) > 0 {
		d.active = len(d.tabs) - 1
	}
	empty := len(d.tabs) == 0
	d.mu.Unlock()
	// The session is closed outside the state lock: its terminal may call
	// back into the desktop while it releases the PTY.
	_ = d.manager.Close(id)
	if d.win != nil {
		if empty {
			d.win.Close()
		} else {
			d.win.Invalidate()
		}
	}
}

func (d *desktop) closeActive() {
	d.mu.Lock()
	var id uint64
	if len(d.tabs) > 0 {
		id = d.tabs[d.active].id
	}
	d.mu.Unlock()
	if id != 0 {
		d.closeTab(id)
	}
}

func (d *desktop) selectTab(index int) {
	d.mu.Lock()
	if index >= 0 && index < len(d.tabs) {
		d.active = index
	}
	d.mu.Unlock()
	if d.win != nil {
		d.win.Invalidate()
	}
}

func (d *desktop) switchTab(delta int) {
	d.mu.Lock()
	if len(d.tabs) > 0 {
		d.active = (d.active + delta + len(d.tabs)) % len(d.tabs)
	}
	d.mu.Unlock()
	if d.win != nil {
		d.win.Invalidate()
	}
}

func (d *desktop) newTab() {
	if err := d.addTab(""); err != nil {
		d.mu.Lock()
		d.lastError = err.Error()
		d.mu.Unlock()
	}
	if d.win != nil {
		d.win.Invalidate()
	}
}

func (d *desktop) view(c *ui.Context) {
	c.OnWindowShortcut(ui.Ctrl, ui.KeyT, d.newTab)
	c.OnWindowShortcut(ui.Ctrl, ui.KeyW, d.closeActive)
	c.OnWindowShortcut(ui.Ctrl, ui.KeyTab, func() { d.switchTab(1) })
	c.OnWindowShortcut(ui.Ctrl|ui.Shift, ui.KeyTab, func() { d.switchTab(-1) })

	d.mu.Lock()
	tabs := append([]*sessionTab(nil), d.tabs...)
	active := d.active
	errText := d.lastError
	d.mu.Unlock()

	ui.Column(c).Fill().Children(func() {
		theme := c.Theme()
		ui.Row(c).Padding(6, 8).Gap(6).Children(func() {
			for i, tab := range tabs {
				label := tab.title
				if label == "" {
					label = fmt.Sprintf("Tab %d", i+1)
				}
				if runes := []rune(label); len(runes) > 32 {
					label = string(runes[:32]) + "…"
				}
				if i == active {
					label = "● " + label
				}
				if ui.Button(c.Key(fmt.Sprintf("tab-%d", tab.id)), label).Clicked() {
					d.selectTab(i)
				}
				if ui.Button(c.Key(fmt.Sprintf("close-%d", tab.id)), "×").Clicked() {
					d.closeTab(tab.id)
				}
			}
			if ui.Button(c.Key("new-tab"), "+").Clicked() {
				d.newTab()
			}
		})
		if errText != "" {
			ui.Text(c, errText).TextColor(theme.TextMuted)
		}
		if len(tabs) > 0 && active < len(tabs) {
			terminal.View(c, tabs[active].term).Fill().AutoFocus()
		} else {
			ui.Column(c).Fill().Center().Children(func() {
				ui.Text(c, "No terminal")
			})
		}
	})
}
