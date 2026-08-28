package priority

import (
	"context"
	"strings"
	"testing"
	"time"

	"github.com/zhangyucheng1014-dev/vpn-geo/internal/config"
)

type fakeRunner struct {
	values map[string]string
	calls  [][]string
}

func (r *fakeRunner) Run(_ context.Context, _ string, args ...string) ([]byte, error) {
	r.calls = append(r.calls, args)
	if len(args) >= 6 && args[2] == "connection" && args[3] == "show" {
		return []byte(r.values[args[5]] + "\n"), nil
	}
	return nil, nil
}

func TestPromoteOnlyWritesAutoconnectPriority(t *testing.T) {
	runner := &fakeRunner{values: map[string]string{"jp": "100", "us": "80", "kr": "60"}}
	manager := &Manager{Nodes: []config.Node{{Name: "JP", UUID: "jp", Country: "JP"}, {Name: "US", UUID: "us", Country: "US"}, {Name: "KR", UUID: "kr", Country: "KR"}}, Runner: runner, Timeout: time.Second}
	changed, err := manager.Promote(context.Background(), "KR")
	if err != nil || !changed {
		t.Fatalf("Promote() = %v, %v; want changed without error", changed, err)
	}
	if got, want := len(runner.calls), 4; got != want {
		t.Fatalf("calls = %d, want %d", got, want)
	}
	modify := strings.Join(runner.calls[3], " ")
	if !strings.Contains(modify, "connection modify uuid kr connection.autoconnect-priority 101") {
		t.Fatalf("unexpected modify invocation: %q", modify)
	}
}

func TestPromoteSameCountryDoesNotWrite(t *testing.T) {
	runner := &fakeRunner{values: map[string]string{"jp": "100", "us": "80"}}
	manager := &Manager{Nodes: []config.Node{{Name: "JP", UUID: "jp", Country: "JP"}, {Name: "US", UUID: "us", Country: "US"}}, Runner: runner, Timeout: time.Second}
	changed, err := manager.Promote(context.Background(), "JP")
	if err != nil || changed {
		t.Fatalf("Promote() = %v, %v; want no change", changed, err)
	}
	if got, want := len(runner.calls), 2; got != want {
		t.Fatalf("calls = %d, want %d", got, want)
	}
}

func TestPromoteBreaksHighestPriorityTie(t *testing.T) {
	runner := &fakeRunner{values: map[string]string{"jp": "100", "us": "100"}}
	manager := &Manager{Nodes: []config.Node{{Name: "JP", UUID: "jp", Country: "JP"}, {Name: "US", UUID: "us", Country: "US"}}, Runner: runner, Timeout: time.Second}
	changed, err := manager.Promote(context.Background(), "US")
	if err != nil || !changed {
		t.Fatalf("Promote() = %v, %v; want tie resolved", changed, err)
	}
	if got := strings.Join(runner.calls[2], " "); !strings.HasSuffix(got, "connection.autoconnect-priority 101") {
		t.Fatalf("tie command = %q", got)
	}
}

func TestPromoteNodeRejectsUnconfiguredTarget(t *testing.T) {
	runner := &fakeRunner{values: map[string]string{"jp": "100"}}
	manager := &Manager{Nodes: []config.Node{{Name: "JP", UUID: "jp", Country: "JP"}}, Runner: runner, Timeout: time.Second}
	if changed, err := manager.PromoteNode(context.Background(), config.Node{Name: "untrusted", UUID: "other", Country: "JP"}); err == nil || changed {
		t.Fatalf("PromoteNode() = %v, %v; want rejection", changed, err)
	}
	if len(runner.calls) != 0 {
		t.Fatalf("runner calls = %d, want 0", len(runner.calls))
	}
}
