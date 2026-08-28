// Package config loads and validates the user-owned application configuration.
package config

import (
	"errors"
	"fmt"
	"math"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/BurntSushi/toml"
)

const AppName = "vpn-geo"

const (
	maxCandidateCountries = 100
	maxSamples            = 100
	maxParallel           = 32
	maxDownloadBytes      = 1 << 30
)

type Config struct {
	SettleDelay time.Duration   `toml:"-"`
	QuietPeriod time.Duration   `toml:"-"`
	Settle      string          `toml:"settle_delay"`
	Quiet       string          `toml:"quiet_period"`
	GeoIP       GeoIPConfig     `toml:"geoip"`
	Benchmark   BenchmarkConfig `toml:"benchmark"`
	Nodes       []Node          `toml:"nodes"`
}

type GeoIPConfig struct {
	URL         string        `toml:"url"`
	Timeout     time.Duration `toml:"-"`
	TimeoutText string        `toml:"timeout"`
}

type BenchmarkConfig struct {
	CandidateCountries int           `toml:"candidate_countries"`
	Samples            int           `toml:"samples"`
	Timeout            time.Duration `toml:"-"`
	TimeoutText        string        `toml:"request_timeout"`
	DownloadBytes      int64         `toml:"download_bytes"`
	Parallel           int           `toml:"parallel"`
}

// Node never stores VPN secrets. UUID is the only value used to alter a profile.
type Node struct {
	Name      string  `toml:"name"`
	UUID      string  `toml:"uuid"`
	Country   string  `toml:"country"`
	Latitude  float64 `toml:"latitude"`
	Longitude float64 `toml:"longitude"`
	TestURL   string  `toml:"test_url"`
}

func DefaultPath() string {
	if path := os.Getenv("XDG_CONFIG_HOME"); path != "" {
		return filepath.Join(path, AppName, "config.toml")
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return filepath.Join(".", "config.toml")
	}
	return filepath.Join(home, ".config", AppName, "config.toml")
}

func Load(path string) (Config, error) {
	var cfg Config
	if path == "" {
		path = DefaultPath()
	}
	metadata, decodeErr := toml.DecodeFile(path, &cfg)
	if decodeErr != nil {
		return Config{}, fmt.Errorf("read config %s: %w", path, decodeErr)
	}
	if undecoded := metadata.Undecoded(); len(undecoded) > 0 {
		return Config{}, fmt.Errorf("unknown configuration key %q", undecoded[0].String())
	}

	if cfg.Settle == "" {
		cfg.Settle = "3s"
	}
	if cfg.Quiet == "" {
		cfg.Quiet = "500ms"
	}
	if cfg.GeoIP.URL == "" {
		cfg.GeoIP.URL = "https://ipwho.is/"
	}
	if cfg.GeoIP.TimeoutText == "" {
		cfg.GeoIP.TimeoutText = "10s"
	}
	if cfg.Benchmark.CandidateCountries == 0 {
		cfg.Benchmark.CandidateCountries = 3
	}
	if cfg.Benchmark.Samples == 0 {
		cfg.Benchmark.Samples = 3
	}
	if cfg.Benchmark.TimeoutText == "" {
		cfg.Benchmark.TimeoutText = "8s"
	}
	if cfg.Benchmark.DownloadBytes == 0 {
		cfg.Benchmark.DownloadBytes = 10485760
	}
	if cfg.Benchmark.Parallel == 0 {
		cfg.Benchmark.Parallel = 2
	}

	if err := validateURL("geoip.url", cfg.GeoIP.URL); err != nil {
		return Config{}, err
	}
	var err error
	if cfg.SettleDelay, err = time.ParseDuration(cfg.Settle); err != nil || cfg.SettleDelay < 0 {
		return Config{}, errors.New("settle_delay must be a non-negative duration")
	}
	if cfg.QuietPeriod, err = time.ParseDuration(cfg.Quiet); err != nil || cfg.QuietPeriod < 0 {
		return Config{}, errors.New("quiet_period must be a non-negative duration")
	}
	if cfg.GeoIP.Timeout, err = time.ParseDuration(cfg.GeoIP.TimeoutText); err != nil || cfg.GeoIP.Timeout <= 0 {
		return Config{}, errors.New("geoip.timeout must be a positive duration")
	}
	if cfg.Benchmark.Timeout, err = time.ParseDuration(cfg.Benchmark.TimeoutText); err != nil || cfg.Benchmark.Timeout <= 0 {
		return Config{}, errors.New("benchmark.request_timeout must be a positive duration")
	}
	if cfg.Benchmark.CandidateCountries < 1 || cfg.Benchmark.Samples < 1 || cfg.Benchmark.DownloadBytes < 1 || cfg.Benchmark.Parallel < 1 {
		return Config{}, errors.New("benchmark values must be positive")
	}
	if cfg.Benchmark.CandidateCountries > maxCandidateCountries || cfg.Benchmark.Samples > maxSamples || cfg.Benchmark.Parallel > maxParallel || cfg.Benchmark.DownloadBytes > maxDownloadBytes {
		return Config{}, fmt.Errorf("benchmark values exceed safe limits (countries <= %d, samples <= %d, parallel <= %d, download_bytes <= %d)", maxCandidateCountries, maxSamples, maxParallel, maxDownloadBytes)
	}
	seenUUID := make(map[string]bool)
	for i := range cfg.Nodes {
		n := &cfg.Nodes[i]
		n.Country = strings.ToUpper(strings.TrimSpace(n.Country))
		n.UUID = strings.TrimSpace(n.UUID)
		n.Name = strings.TrimSpace(n.Name)
		n.TestURL = strings.TrimSpace(n.TestURL)
		if n.Name == "" || n.UUID == "" || len(n.Country) != 2 {
			return Config{}, fmt.Errorf("nodes[%d] needs name, uuid, and a two-letter country code", i)
		}
		if !validCountry(n.Country) {
			return Config{}, fmt.Errorf("nodes[%d].country must contain ASCII letters", i)
		}
		if !validCoordinate(n.Latitude, n.Longitude) {
			return Config{}, fmt.Errorf("nodes[%d] latitude/longitude out of range", i)
		}
		if n.TestURL != "" {
			if err := validateURL(fmt.Sprintf("nodes[%d].test_url", i), n.TestURL); err != nil {
				return Config{}, err
			}
		}
		if seenUUID[n.UUID] {
			return Config{}, fmt.Errorf("nodes[%d] repeats UUID %q", i, n.UUID)
		}
		seenUUID[n.UUID] = true
	}
	if len(cfg.Nodes) == 0 {
		return Config{}, errors.New("at least one [[nodes]] entry is required")
	}
	return cfg, nil
}

func validateURL(field, raw string) error {
	u, err := url.ParseRequestURI(raw)
	if err != nil || u.Scheme == "" || u.Host == "" || (u.Scheme != "http" && u.Scheme != "https") {
		return fmt.Errorf("%s must be an absolute http(s) URL", field)
	}
	return nil
}

func validCoordinate(latitude, longitude float64) bool {
	if latitude == 0 && longitude == 0 {
		return true // Coordinates are optional for benchmark participation.
	}
	return !math.IsNaN(latitude) && !math.IsInf(latitude, 0) && !math.IsNaN(longitude) && !math.IsInf(longitude, 0) && latitude >= -90 && latitude <= 90 && longitude >= -180 && longitude <= 180
}

func validCountry(country string) bool {
	for _, char := range country {
		if char < 'A' || char > 'Z' {
			return false
		}
	}
	return true
}
