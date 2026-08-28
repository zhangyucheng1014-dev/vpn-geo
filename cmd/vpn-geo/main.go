package main

import (
	"context"
	"errors"
	"flag"
	"fmt"
	"log/slog"
	"os"
	"os/signal"
	"path/filepath"
	"syscall"
	"time"

	"github.com/zhangyucheng1014-dev/vpn-geo/internal/benchmark"
	"github.com/zhangyucheng1014-dev/vpn-geo/internal/config"
	"github.com/zhangyucheng1014-dev/vpn-geo/internal/daemon"
	"github.com/zhangyucheng1014-dev/vpn-geo/internal/geoip"
	"github.com/zhangyucheng1014-dev/vpn-geo/internal/priority"
	"github.com/zhangyucheng1014-dev/vpn-geo/internal/state"
)

func main() {
	os.Exit(run())
}

func run() int {
	root := flag.NewFlagSet(filepath.Base(os.Args[0]), flag.ContinueOnError)
	root.SetOutput(os.Stderr)
	root.Usage = usage
	configPath := config.DefaultPath()
	verbose := false
	root.StringVar(&configPath, "config", configPath, "path to TOML configuration")
	root.StringVar(&configPath, "c", configPath, "path to TOML configuration (short form)")
	root.BoolVar(&verbose, "verbose", false, "enable debug logs")
	root.BoolVar(&verbose, "v", false, "enable debug logs (short form)")
	if err := root.Parse(os.Args[1:]); err != nil {
		if errors.Is(err, flag.ErrHelp) {
			return 0
		}
		return 2
	}
	level := slog.LevelInfo
	if verbose {
		level = slog.LevelDebug
	}
	logger := slog.New(slog.NewTextHandler(os.Stderr, &slog.HandlerOptions{Level: level}))
	command := "daemon"
	args := root.Args()
	if len(args) > 0 {
		command = args[0]
	}
	if command == "speed" && flagHasHelp(args[1:]) {
		printSpeedUsage()
		return 0
	}
	cfg, err := config.Load(configPath)
	if err != nil {
		logger.Error("configuration is invalid", "path", configPath, "error", err)
		return 2
	}

	switch command {
	case "daemon":
		if len(args) > 1 {
			fmt.Fprintln(os.Stderr, "daemon does not accept positional arguments")
			return 2
		}
		return runDaemon(cfg, logger)
	case "check":
		if len(args) > 1 {
			fmt.Fprintln(os.Stderr, "check does not accept positional arguments")
			return 2
		}
		fmt.Printf("configuration is valid: %s\n", configPath)
		return 0
	case "speed":
		return runBenchmark(cfg, logger, args[1:])
	default:
		logger.Error("unknown command", "command", command)
		usage()
		return 2
	}
}

func runDaemon(cfg config.Config, logger *slog.Logger) int {
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	handler := newHandler(cfg, logger)
	watcher := &daemon.Watcher{SettleDelay: cfg.SettleDelay, QuietPeriod: cfg.QuietPeriod, Processor: handler, Log: logger}
	backoff := time.Second
	for ctx.Err() == nil {
		if err := watcher.Run(ctx); err != nil && !errors.Is(err, context.Canceled) {
			logger.Error("NetworkManager watcher stopped; reconnecting", "error", err, "after", backoff)
			select {
			case <-ctx.Done():
				break
			case <-time.After(backoff):
			}
			if backoff < 30*time.Second {
				backoff *= 2
			}
			continue
		}
		break
	}
	return 0
}

func runBenchmark(cfg config.Config, logger *slog.Logger, args []string) int {
	// The short, memorable form is `vpn-geo speed apply`. Keep `--apply` as a
	// script-friendly alias as well.
	applyByWord := false
	if len(args) > 0 && args[0] == "apply" {
		applyByWord = true
		args = args[1:]
	}
	flags := flag.NewFlagSet("speed", flag.ContinueOnError)
	flags.SetOutput(os.Stderr)
	flags.Usage = printSpeedUsage
	apply := flags.Bool("apply", false, "promote fastest successfully measured node")
	if err := flags.Parse(args); err != nil {
		return 2
	}
	if flags.NArg() != 0 {
		fmt.Fprintln(os.Stderr, "speed accepts only the optional word 'apply'")
		return 2
	}
	ctx, cancel := context.WithTimeout(context.Background(), time.Duration(cfg.Benchmark.CandidateCountries*cfg.Benchmark.Samples)*cfg.Benchmark.Timeout+10*time.Second)
	defer cancel()
	location, err := geoip.New(cfg.GeoIP.URL, cfg.GeoIP.Timeout).Locate(ctx)
	if err != nil {
		logger.Error("cannot locate current public IP", "error", err)
		return 1
	}
	candidates := benchmark.Nearest(cfg.Nodes, location, cfg.Benchmark.CandidateCountries)
	if len(candidates) == 0 {
		logger.Error("no nodes have both latitude/longitude and test_url")
		return 1
	}
	results := benchmark.Run(ctx, candidates, benchmark.Options{Candidates: cfg.Benchmark.CandidateCountries, Samples: cfg.Benchmark.Samples, Timeout: cfg.Benchmark.Timeout, Bytes: cfg.Benchmark.DownloadBytes, Parallel: cfg.Benchmark.Parallel})
	fmt.Printf("Current public-IP country: %s (%.4f, %.4f)\n\n", location.CountryCode, location.Latitude, location.Longitude)
	fmt.Printf("%-22s %-7s %10s %12s %s\n", "NODE", "COUNTRY", "DISTANCE", "LATENCY", "THROUGHPUT")
	var fastest *benchmark.Result
	for i := range results {
		r := &results[i]
		if r.Err != nil {
			fmt.Printf("%-22s %-7s %8.0f km %12s %s\n", r.Node.Name, benchmark.Country(r.Node), r.DistanceKM, "failed", r.Err)
			continue
		}
		fmt.Printf("%-22s %-7s %8.0f km %10s %s\n", r.Node.Name, benchmark.Country(r.Node), r.DistanceKM, r.Latency.Round(time.Millisecond), benchmark.FormatMbps(r.Throughput))
		if fastest == nil {
			fastest = r
		}
	}
	if (applyByWord || *apply) && fastest != nil {
		changed, err := priority.New(cfg.Nodes).PromoteNode(ctx, fastest.Node)
		if err != nil {
			logger.Error("failed to apply benchmark result", "error", err)
			return 1
		}
		logger.Info("benchmark result applied", "node", fastest.Node.Name, "priority_changed", changed)
	}
	return 0
}

func flagHasHelp(args []string) bool {
	for _, arg := range args {
		if arg == "-h" || arg == "--help" {
			return true
		}
	}
	return false
}

func printSpeedUsage() {
	fmt.Fprintln(os.Stderr, "Usage: vpn-geo speed [apply]")
	fmt.Fprintln(os.Stderr, "  no apply         show benchmark results only")
	fmt.Fprintln(os.Stderr, "  apply            benchmark and promote the fastest node")
}

func newHandler(cfg config.Config, logger *slog.Logger) *daemon.Handler {
	return &daemon.Handler{Config: cfg, GeoIP: geoip.New(cfg.GeoIP.URL, cfg.GeoIP.Timeout), Priority: priority.New(cfg.Nodes), State: state.Store{Dir: state.DefaultDir()}, Log: logger}
}

func usage() {
	program := filepath.Base(os.Args[0])
	fmt.Fprintf(os.Stderr, "Usage: %s [-c PATH] [-v] [check|speed [apply]]\n", program)
	fmt.Fprintln(os.Stderr, "  no command       run the background watcher")
	fmt.Fprintln(os.Stderr, "  check            validate the configuration")
	fmt.Fprintln(os.Stderr, "  speed            test the nearest configured countries")
	fmt.Fprintln(os.Stderr, "  speed apply      test and promote the fastest node")
}
