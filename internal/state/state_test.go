package state

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

func TestSaveLoadAndReplace(t *testing.T) {
	dir := t.TempDir()
	store := Store{Dir: dir}
	if err := store.Save("jp"); err != nil {
		t.Fatal(err)
	}
	value, err := store.Load()
	if err != nil {
		t.Fatal(err)
	}
	if value.LastCountry != "JP" || value.SchemaVersion != 1 {
		t.Fatalf("unexpected state: %#v", value)
	}
	if info, err := os.Stat(filepath.Join(dir, "state.json")); err != nil || info.Mode().Perm() != 0600 {
		t.Fatalf("state permissions = %v, %v", info.Mode(), err)
	}
	if err := store.Save("kr"); err != nil {
		t.Fatal(err)
	}
	value, err = store.Load()
	if err != nil || value.LastCountry != "KR" {
		t.Fatalf("updated state = %#v, %v", value, err)
	}
}

func TestTryLockIsNonBlocking(t *testing.T) {
	store := Store{Dir: t.TempDir()}
	first, err := store.TryLock()
	if err != nil || first == nil {
		t.Fatalf("first lock = %v, %v", first, err)
	}
	defer first.Close()
	second, err := store.TryLock()
	if err != nil || second != nil {
		t.Fatalf("second lock = %v, %v; want unavailable", second, err)
	}
}

func TestLoadRejectsUnknownSchema(t *testing.T) {
	dir := t.TempDir()
	data, err := json.Marshal(State{SchemaVersion: 99, LastCountry: "JP"})
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, "state.json"), data, 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := (Store{Dir: dir}).Load(); err == nil {
		t.Fatal("Load() accepted unknown schema")
	}
}
