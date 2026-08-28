// Package priority performs the only permitted NetworkManager mutation: priority.
package priority

import (
	"context"
	"errors"
	"fmt"
	"os/exec"
	"strconv"
	"strings"
	"time"

	"github.com/zhangyucheng1014-dev/vpn-geo/internal/config"
)

// NetworkManager stores this property as a signed 32-bit integer.
const (
	minPriority = -2147483648
	maxPriority = 2147483647
)

type Runner interface {
	Run(context.Context, string, ...string) ([]byte, error)
}

type commandRunner struct{}

func (commandRunner) Run(ctx context.Context, name string, args ...string) ([]byte, error) {
	cmd := exec.CommandContext(ctx, name, args...)
	return cmd.CombinedOutput()
}

type Manager struct {
	Nodes   []config.Node
	Runner  Runner
	Timeout time.Duration
}

func New(nodes []config.Node) *Manager {
	return &Manager{Nodes: nodes, Runner: commandRunner{}, Timeout: 10 * time.Second}
}

// Promote moves a configured node ahead of all managed nodes. No other profile
// field is read or written, and other priorities are deliberately left intact.
func (m *Manager) Promote(ctx context.Context, country string) (bool, error) {
	target, ok := m.Find(country)
	if !ok {
		return false, nil
	}
	return m.PromoteNode(ctx, target)
}

// PromoteNode promotes exactly the supplied configured node. It is used by the
// opt-in benchmark command, which may select one of several nodes in a country.
func (m *Manager) PromoteNode(ctx context.Context, target config.Node) (bool, error) {
	if err := ctx.Err(); err != nil {
		return false, err
	}
	configured := false
	for _, node := range m.Nodes {
		if node.UUID == target.UUID {
			configured = true
			break
		}
	}
	if !configured {
		return false, fmt.Errorf("node %q is not present in configured nodes", target.UUID)
	}
	if m.Runner == nil {
		return false, errors.New("priority command runner is nil")
	}
	timeout := m.Timeout
	if timeout <= 0 {
		timeout = 10 * time.Second
	}
	priorities := make(map[string]int, len(m.Nodes))
	maximum := 0
	havePriority := false
	for _, node := range m.Nodes {
		if err := ctx.Err(); err != nil {
			return false, err
		}
		value, err := m.priority(ctx, node.UUID)
		if err != nil {
			return false, fmt.Errorf("read priority for %s: %w", node.Name, err)
		}
		priorities[node.UUID] = value
		if !havePriority || value > maximum {
			maximum = value
			havePriority = true
		}
	}
	sharedMaximum := false
	for uuid, value := range priorities {
		if uuid != target.UUID && value == maximum {
			sharedMaximum = true
			break
		}
	}
	if priorities[target.UUID] == maximum && !sharedMaximum {
		return false, nil
	}
	if maximum == maxPriority {
		return false, fmt.Errorf("priority ceiling reached; refusing to normalize unrelated profile order")
	}
	newPriority := maximum + 1
	callCtx, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()
	output, err := m.Runner.Run(callCtx, "nmcli", "--wait", "10", "connection", "modify", "uuid", target.UUID,
		"connection.autoconnect-priority", strconv.Itoa(newPriority))
	if err != nil {
		return false, fmt.Errorf("set priority for %s: %w (%s)", target.Name, err, strings.TrimSpace(string(output)))
	}
	return true, nil
}

func (m *Manager) Find(country string) (config.Node, bool) {
	country = strings.ToUpper(strings.TrimSpace(country))
	for _, node := range m.Nodes {
		if node.Country == country {
			return node, true
		}
	}
	return config.Node{}, false
}

func (m *Manager) priority(parent context.Context, uuid string) (int, error) {
	timeout := m.Timeout
	if timeout <= 0 {
		timeout = 10 * time.Second
	}
	ctx, cancel := context.WithTimeout(parent, timeout)
	defer cancel()
	output, err := m.Runner.Run(ctx, "nmcli", "-g", "connection.autoconnect-priority", "connection", "show", "uuid", uuid)
	if err != nil {
		return 0, fmt.Errorf("nmcli: %w (%s)", err, strings.TrimSpace(string(output)))
	}
	value := strings.TrimSpace(string(output))
	if value == "" || value == "--" {
		return 0, nil
	}
	priority, err := strconv.Atoi(strings.Split(value, "\n")[0])
	if err != nil {
		return 0, fmt.Errorf("parse %q: %w", value, err)
	}
	if priority < minPriority || priority > maxPriority {
		return 0, fmt.Errorf("priority %d is outside NetworkManager's signed 32-bit range", priority)
	}
	return priority, nil
}
