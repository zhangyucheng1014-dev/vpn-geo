// Package state persists the last successfully applied country and serializes runs.
package state

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"syscall"
	"time"
)

const (
	appName       = "vpn-geo"
	schemaVersion = 1
)

type State struct {
	SchemaVersion int       `json:"schema_version"`
	LastCountry   string    `json:"last_country"`
	UpdatedAt     time.Time `json:"updated_at"`
}

type Store struct{ Dir string }

func DefaultDir() string {
	if base := os.Getenv("XDG_STATE_HOME"); base != "" {
		return filepath.Join(base, appName)
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return filepath.Join(".", ".state", appName)
	}
	return filepath.Join(home, ".local", "state", appName)
}

func (s Store) statePath() string { return filepath.Join(s.Dir, "state.json") }
func (s Store) lockPath() string  { return filepath.Join(s.Dir, "run.lock") }

func (s Store) Load() (State, error) {
	data, err := os.ReadFile(s.statePath())
	if errors.Is(err, os.ErrNotExist) {
		return State{}, nil
	}
	if err != nil {
		return State{}, fmt.Errorf("read state: %w", err)
	}
	var value State
	if err := json.Unmarshal(data, &value); err != nil {
		return State{}, fmt.Errorf("decode state: %w", err)
	}
	if value.SchemaVersion != 0 && value.SchemaVersion != schemaVersion {
		return State{}, fmt.Errorf("unsupported state schema version %d", value.SchemaVersion)
	}
	value.LastCountry = strings.ToUpper(strings.TrimSpace(value.LastCountry))
	if value.LastCountry != "" && !validCountry(value.LastCountry) {
		return State{}, fmt.Errorf("invalid last_country %q", value.LastCountry)
	}
	return value, nil
}

func (s Store) Save(country string) error {
	if err := os.MkdirAll(s.Dir, 0700); err != nil {
		return fmt.Errorf("create state directory: %w", err)
	}
	value := State{SchemaVersion: schemaVersion, LastCountry: strings.ToUpper(strings.TrimSpace(country)), UpdatedAt: time.Now().UTC()}
	if !validCountry(value.LastCountry) {
		return fmt.Errorf("invalid country %q", country)
	}
	data, err := json.MarshalIndent(value, "", "  ")
	if err != nil {
		return fmt.Errorf("encode state: %w", err)
	}
	temp, err := os.CreateTemp(s.Dir, ".state-*.json")
	if err != nil {
		return fmt.Errorf("create temporary state: %w", err)
	}
	tempName := temp.Name()
	defer os.Remove(tempName)
	if err = temp.Chmod(0600); err == nil {
		_, err = temp.Write(append(data, '\n'))
	}
	if err == nil {
		err = temp.Sync()
	}
	if closeErr := temp.Close(); err == nil {
		err = closeErr
	}
	if err != nil {
		return fmt.Errorf("write state: %w", err)
	}
	if err := os.Rename(tempName, s.statePath()); err != nil {
		return fmt.Errorf("replace state atomically: %w", err)
	}
	// Persist the directory entry as well as the file so a power loss cannot
	// leave the old state path pointing at a missing temporary file.
	dir, err := os.Open(s.Dir)
	if err != nil {
		return fmt.Errorf("open state directory for sync: %w", err)
	}
	syncErr := dir.Sync()
	closeErr := dir.Close()
	if syncErr != nil {
		return fmt.Errorf("sync state directory: %w", syncErr)
	}
	if closeErr != nil {
		return fmt.Errorf("close state directory: %w", closeErr)
	}
	return nil
}

type Lock struct{ file *os.File }

// TryLock never waits. A duplicate hook must not delay a user network event.
func (s Store) TryLock() (*Lock, error) {
	if err := os.MkdirAll(s.Dir, 0700); err != nil {
		return nil, fmt.Errorf("create state directory: %w", err)
	}
	f, err := os.OpenFile(s.lockPath(), os.O_CREATE|os.O_RDWR, 0600)
	if err != nil {
		return nil, fmt.Errorf("open lock: %w", err)
	}
	if err := syscall.Flock(int(f.Fd()), syscall.LOCK_EX|syscall.LOCK_NB); err != nil {
		f.Close()
		if errors.Is(err, syscall.EWOULDBLOCK) {
			return nil, nil
		}
		return nil, fmt.Errorf("lock state: %w", err)
	}
	return &Lock{file: f}, nil
}

func (l *Lock) Close() error {
	if l == nil || l.file == nil {
		return nil
	}
	err := syscall.Flock(int(l.file.Fd()), syscall.LOCK_UN)
	closeErr := l.file.Close()
	if err != nil {
		return err
	}
	return closeErr
}

func validCountry(country string) bool {
	if len(country) != 2 {
		return false
	}
	for _, char := range country {
		if char < 'A' || char > 'Z' {
			return false
		}
	}
	return true
}
