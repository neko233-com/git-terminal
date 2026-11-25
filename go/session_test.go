// Copyright 2026 git-next contributors. MIT license.
package gitterminal

import (
	"errors"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

type fakeSession struct {
	closes atomic.Int32
	err    error
}

func (f *fakeSession) Close() error {
	f.closes.Add(1)
	return f.err
}

func TestSessionCloseIsIdempotent(t *testing.T) {
	session := &fakeSession{}
	manager := NewSessionManager()
	entry := manager.Add("bash", session, nil)
	if err := manager.Close(entry.ID()); err != nil {
		t.Fatal(err)
	}
	if err := entry.Close(); err != nil {
		t.Fatal(err)
	}
	if got := session.closes.Load(); got != 1 {
		t.Fatalf("session closed %d times, want 1", got)
	}
	if !entry.Closed() {
		t.Error("entry is not marked closed")
	}
	if manager.Len() != 0 {
		t.Errorf("manager still holds %d entries", manager.Len())
	}
}

func TestConcurrentCloseAndExitCallback(t *testing.T) {
	session := &fakeSession{}
	var exits atomic.Int32
	manager := NewSessionManager()
	var entry *Entry
	entry = manager.Add("bash", session, func(id uint64, code int) {
		if id != entry.ID() || code != 7 {
			t.Errorf("exit callback got id=%d code=%d", id, code)
		}
		exits.Add(1)
	})
	var wg sync.WaitGroup
	for i := 0; i < 64; i++ {
		wg.Add(3)
		go func() { defer wg.Done(); _ = manager.Close(entry.ID()) }()
		go func() { defer wg.Done(); _ = entry.Close() }()
		go func() { defer wg.Done(); entry.NotifyExit(7) }()
	}
	wg.Wait()
	if got := session.closes.Load(); got != 1 {
		t.Errorf("session closed %d times, want 1", got)
	}
	if got := exits.Load(); got != 1 {
		t.Errorf("exit callback ran %d times, want 1", got)
	}
	if !entry.Exited() {
		t.Error("entry is not marked exited")
	}
}

func TestCloseAllClosesEverySession(t *testing.T) {
	manager := NewSessionManager()
	sessions := make([]*fakeSession, 3)
	for i := range sessions {
		sessions[i] = &fakeSession{}
		manager.Add("tab", sessions[i], nil)
	}
	if err := manager.CloseAll(); err != nil {
		t.Fatal(err)
	}
	if manager.Len() != 0 || len(manager.List()) != 0 {
		t.Error("CloseAll left entries behind")
	}
	for i, session := range sessions {
		if got := session.closes.Load(); got != 1 {
			t.Errorf("session %d closed %d times", i, got)
		}
	}
	if err := manager.CloseAll(); err != nil {
		t.Fatal(err)
	}
}

func TestCloseAllReportsErrorsOnce(t *testing.T) {
	manager := NewSessionManager()
	want := errors.New("pty is gone")
	session := &fakeSession{err: want}
	manager.Add("tab", session, nil)
	if err := manager.CloseAll(); !errors.Is(err, want) {
		t.Errorf("CloseAll error = %v, want %v", err, want)
	}
}

func TestCloseThenNotifyExitUsesNoSession(t *testing.T) {
	session := &fakeSession{}
	manager := NewSessionManager()
	var exits atomic.Int32
	entry := manager.Add("bash", session, func(uint64, int) { exits.Add(1) })
	if err := manager.Close(entry.ID()); err != nil {
		t.Fatal(err)
	}
	entry.NotifyExit(0)
	entry.NotifyExit(1)
	if got := session.closes.Load(); got != 1 {
		t.Errorf("session closed %d times after Close", got)
	}
	if got := exits.Load(); got != 1 {
		t.Errorf("exit callback ran %d times, want 1", got)
	}
	if !entry.Exited() || !entry.Closed() {
		t.Error("entry did not record closed and exited state")
	}
	if _, ok := manager.Get(entry.ID()); ok {
		t.Error("closed entry still registered in the manager")
	}
}

func TestNotifyExitCallbackMayReenterManager(t *testing.T) {
	manager := NewSessionManager()
	session := &fakeSession{}
	var entry *Entry
	var exits atomic.Int32
	entry = manager.Add("bash", session, func(id uint64, code int) {
		exits.Add(1)
		if id != entry.ID() || code != 3 {
			t.Errorf("exit callback got id=%d code=%d", id, code)
		}
		// The callback runs outside the entry lock, so closing and removing
		// the very entry that is notifying must not deadlock or double-close.
		_ = manager.Close(id)
		entry.NotifyExit(9)
	})
	done := make(chan struct{})
	go func() {
		entry.NotifyExit(3)
		close(done)
	}()
	select {
	case <-done:
	case <-time.After(2 * time.Second):
		t.Fatal("reentrant exit callback deadlocked")
	}
	if got := exits.Load(); got != 1 {
		t.Errorf("exit callback ran %d times, want 1", got)
	}
	if got := session.closes.Load(); got != 1 {
		t.Errorf("session closed %d times, want 1", got)
	}
	if manager.Len() != 0 {
		t.Errorf("manager still holds %d entries", manager.Len())
	}
}

func TestTitleIsSafeAcrossGoroutines(t *testing.T) {
	entry := NewSessionManager().Add("initial", &fakeSession{}, nil)
	var wg sync.WaitGroup
	for i := 0; i < 32; i++ {
		wg.Add(2)
		go func(i int) { defer wg.Done(); entry.SetTitle("bash " + string(rune('a'+i%26))) }(i)
		go func() { defer wg.Done(); _ = entry.Title() }()
	}
	wg.Wait()
	if entry.Title() == "" {
		t.Error("title became empty")
	}
}
