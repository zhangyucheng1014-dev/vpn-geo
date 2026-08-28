package config

import (
	"os"
	"path/filepath"
	"testing"
)

func TestLoadAppliesDefaultsAndNormalizesCountry(t *testing.T) {
	path := filepath.Join(t.TempDir(), "config.toml")
	content := "[[nodes]]\nname = 'Tokyo'\nuuid = 'uuid-1'\ncountry = 'jp'\n"
	if err := os.WriteFile(path, []byte(content), 0600); err != nil {
		t.Fatal(err)
	}
	cfg, err := Load(path)
	if err != nil {
		t.Fatal(err)
	}
	if cfg.Nodes[0].Country != "JP" || cfg.SettleDelay.String() != "3s" || cfg.GeoIP.Timeout.String() != "10s" {
		t.Fatalf("config = %#v", cfg)
	}
}

func TestLoadRejectsUnknownKeysAndInvalidNodeURL(t *testing.T) {
	tests := []struct {
		name    string
		content string
	}{
		{"unknown key", "mystery = true\n[[nodes]]\nname='n'\nuuid='u'\ncountry='JP'\n"},
		{"invalid url", "[[nodes]]\nname='n'\nuuid='u'\ncountry='JP'\ntest_url='file:///tmp/x'\n"},
		{"invalid coordinates", "[[nodes]]\nname='n'\nuuid='u'\ncountry='JP'\nlatitude=91\nlongitude=0\n"},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			path := filepath.Join(t.TempDir(), "config.toml")
			if err := os.WriteFile(path, []byte(test.content), 0600); err != nil {
				t.Fatal(err)
			}
			if _, err := Load(path); err == nil {
				t.Fatal("Load() accepted invalid configuration")
			}
		})
	}
}
