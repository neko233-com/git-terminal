// Copyright 2026 git-next contributors. MIT license.
package gitterminal

import (
	"errors"
	"sort"
	"sync"
)

// Session is the owned shell. The mygo terminal plugin's *Terminal
// implements it; tests use a small fake so no PTY or native library is
// needed. Closing a session must terminate its shell and release its PTY.
type Session interface {
	Close() error
}

// Entry is one tab's session. It owns exactly one Session and guarantees
// Close and the exit callback each run at most once, from any goroutine.
type Entry struct {
	id      uint64
	mu      sync.Mutex
	title   string
	session Session
	closed  bool
	exited  bool
	onExit  func(uint64, int)
}

func (e *Entry) ID() uint64 { return e.id }

func (e *Entry) Title() string {
	e.mu.Lock()
	defer e.mu.Unlock()
	return e.title
}

func (e *Entry) SetTitle(title string) {
	e.mu.Lock()
	e.title = title
	e.mu.Unlock()
}

func (e *Entry) Closed() bool {
	e.mu.Lock()
	defer e.mu.Unlock()
	return e.closed
}

func (e *Entry) Exited() bool {
	e.mu.Lock()
	defer e.mu.Unlock()
	return e.exited
}

// Close is idempotent and safe to call from a UI callback or a terminal
// goroutine. It owns the session: after Close nobody else may use it.
func (e *Entry) Close() error {
	e.mu.Lock()
	if e.closed {
		e.mu.Unlock()
		return nil
	}
	e.closed = true
	session := e.session
	e.session = nil
	e.mu.Unlock()
	if session == nil {
		return nil
	}
	// The session is released outside the lock: closing it may call back into
	// the manager or this entry, which must see the closed state already set.
	return session.Close()
}

// NotifyExit records the shell's exit status and invokes the callback once.
// The callback runs outside the lock so it may close or remove the entry.
func (e *Entry) NotifyExit(code int) {
	e.mu.Lock()
	if e.exited {
		e.mu.Unlock()
		return
	}
	e.exited = true
	callback := e.onExit
	e.mu.Unlock()
	// The callback runs outside the lock so it may close or remove the entry,
	// or re-enter NotifyExit, without deadlocking.
	if callback != nil {
		callback(e.id, code)
	}
}

// SessionManager owns the set of live tab sessions. Every session it accepts
// is closed exactly once, including on CloseAll at window teardown.
type SessionManager struct {
	mu      sync.Mutex
	nextID  uint64
	entries map[uint64]*Entry
}

func NewSessionManager() *SessionManager {
	return &SessionManager{entries: make(map[uint64]*Entry)}
}

// Add registers a session and returns its entry. onExit is called at most
// once, from Entry.NotifyExit.
func (m *SessionManager) Add(title string, session Session, onExit func(uint64, int)) *Entry {
	m.mu.Lock()
	m.nextID++
	entry := &Entry{
		id: m.nextID, title: title, session: session, onExit: onExit,
	}
	m.entries[entry.id] = entry
	m.mu.Unlock()
	return entry
}

func (m *SessionManager) Get(id uint64) (*Entry, bool) {
	m.mu.Lock()
	entry, ok := m.entries[id]
	m.mu.Unlock()
	return entry, ok
}

// List returns a stable snapshot, ordered by creation.
func (m *SessionManager) List() []*Entry {
	m.mu.Lock()
	entries := make([]*Entry, 0, len(m.entries))
	for _, entry := range m.entries {
		entries = append(entries, entry)
	}
	m.mu.Unlock()
	sort.Slice(entries, func(i, j int) bool { return entries[i].ID() < entries[j].ID() })
	return entries
}

func (m *SessionManager) Len() int {
	m.mu.Lock()
	defer m.mu.Unlock()
	return len(m.entries)
}

// Close removes the entry and closes its session. Repeated calls, including
// concurrent ones, are harmless.
func (m *SessionManager) Close(id uint64) error {
	m.mu.Lock()
	entry, ok := m.entries[id]
	if ok {
		delete(m.entries, id)
	}
	m.mu.Unlock()
	if !ok {
		return nil
	}
	return entry.Close()
}

// CloseAll is the window-close cleanup path: no shell outlives the window.
func (m *SessionManager) CloseAll() error {
	m.mu.Lock()
	entries := make([]*Entry, 0, len(m.entries))
	for id, entry := range m.entries {
		entries = append(entries, entry)
		delete(m.entries, id)
	}
	m.mu.Unlock()
	var errs []error
	for _, entry := range entries {
		if err := entry.Close(); err != nil {
			errs = append(errs, err)
		}
	}
	return errors.Join(errs...)
}

func (m *SessionManager) NotifyExit(id uint64, code int) {
	if entry, ok := m.Get(id); ok {
		entry.NotifyExit(code)
	}
}
