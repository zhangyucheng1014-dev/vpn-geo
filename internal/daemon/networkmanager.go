// Package daemon observes NetworkManager without invoking any connection action.
package daemon

import (
	"context"
	"fmt"
	"log/slog"
	"sync"
	"time"

	"github.com/godbus/dbus/v5"
)

const (
	nmName                 = "org.freedesktop.NetworkManager"
	nmPath                 = "/org/freedesktop/NetworkManager"
	propertiesInterface    = "org.freedesktop.DBus.Properties"
	objectManagerInterface = "org.freedesktop.DBus.ObjectManager"
	signalDebounce         = 100 * time.Millisecond
	nmActiveState          = uint32(2) // NM_ACTIVE_CONNECTION_STATE_ACTIVATED
)

type Processor interface{ Process(context.Context) }

type Watcher struct {
	SettleDelay time.Duration
	QuietPeriod time.Duration
	Processor   Processor
	Log         *slog.Logger

	mu         sync.Mutex
	vpnActive  bool
	cancel     context.CancelFunc
	lastSignal time.Time
}

func (w *Watcher) Run(ctx context.Context) error {
	if w.Log == nil {
		w.Log = slog.Default()
	}
	if w.Processor == nil {
		return fmt.Errorf("watcher processor is nil")
	}
	defer w.cancelPending()
	conn, err := dbus.ConnectSystemBus()
	if err != nil {
		return fmt.Errorf("connect system D-Bus: %w", err)
	}
	defer conn.Close()
	if err := conn.BusObject().Call("org.freedesktop.DBus.AddMatch", 0,
		"type='signal',sender='org.freedesktop.NetworkManager',interface='org.freedesktop.DBus.Properties',member='PropertiesChanged',arg0='org.freedesktop.NetworkManager.Connection.Active'").Err; err != nil {
		return fmt.Errorf("subscribe to NetworkManager signals: %w", err)
	}
	if err := conn.BusObject().Call("org.freedesktop.DBus.AddMatch", 0,
		"type='signal',sender='org.freedesktop.NetworkManager',interface='org.freedesktop.DBus.ObjectManager',member='InterfacesAdded'").Err; err != nil {
		return fmt.Errorf("subscribe to NetworkManager object signals: %w", err)
	}
	if err := conn.BusObject().Call("org.freedesktop.DBus.AddMatch", 0,
		"type='signal',sender='org.freedesktop.NetworkManager',interface='org.freedesktop.DBus.ObjectManager',member='InterfacesRemoved'").Err; err != nil {
		return fmt.Errorf("subscribe to NetworkManager removal signals: %w", err)
	}
	// Keep the D-Bus delivery channel drained even while a state snapshot is in
	// flight. The pump coalesces a burst into one event, preventing godbus from
	// spawning a deferred goroutine for every signal when the main loop is busy.
	signals := make(chan *dbus.Signal, 8)
	conn.Signal(signals)
	events := make(chan struct{}, 1)
	pumpDone := make(chan struct{})
	go func() {
		defer close(pumpDone)
		for {
			select {
			case <-ctx.Done():
				return
			case signal, ok := <-signals:
				if !ok {
					return
				}
				if !relevantSignal(signal) {
					continue
				}
				select {
				case events <- struct{}{}:
				default:
				}
			}
		}
	}()
	defer func() {
		conn.RemoveSignal(signals)
		<-pumpDone
	}()

	initial, err := snapshotVPN(ctx, conn)
	if err != nil {
		return fmt.Errorf("read initial VPN state: %w", err)
	}
	w.mu.Lock()
	w.vpnActive = initial
	w.mu.Unlock()
	w.Log.Info("NetworkManager monitoring started", "vpn_active", initial)

	var debounceTimer *time.Timer
	var debounce <-chan time.Time
	defer func() {
		if debounceTimer != nil {
			debounceTimer.Stop()
		}
	}()
	for {
		select {
		case <-ctx.Done():
			w.cancelPending()
			return nil
		case _, ok := <-events:
			if !ok {
				return fmt.Errorf("system D-Bus event stream closed")
			}
			if debounceTimer == nil {
				debounceTimer = time.NewTimer(signalDebounce)
				debounce = debounceTimer.C
			} else {
				if !debounceTimer.Stop() {
					select {
					case <-debounce:
					default:
					}
				}
				debounceTimer.Reset(signalDebounce)
			}
		case <-debounce:
			debounceTimer = nil
			debounce = nil
			w.observe(ctx, conn)
		}
	}
}

func relevantSignal(signal *dbus.Signal) bool {
	if signal == nil {
		return false
	}
	switch signal.Name {
	case propertiesInterface + ".PropertiesChanged":
		if len(signal.Body) == 0 {
			return false
		}
		iface, ok := signal.Body[0].(string)
		return ok && iface == nmName+".Connection.Active"
	case objectManagerInterface + ".InterfacesAdded", objectManagerInterface + ".InterfacesRemoved":
		if len(signal.Body) < 2 {
			return false
		}
		if signal.Name == objectManagerInterface+".InterfacesAdded" {
			interfaces, ok := signal.Body[1].(map[string]map[string]dbus.Variant)
			if !ok {
				return false
			}
			_, ok = interfaces[nmName+".Connection.Active"]
			return ok
		}
		interfaces, ok := signal.Body[1].([]string)
		if !ok {
			return false
		}
		for _, iface := range interfaces {
			if iface == nmName+".Connection.Active" {
				return true
			}
		}
		return false
	default:
		return false
	}
}

func (w *Watcher) observe(parent context.Context, conn *dbus.Conn) {
	w.mu.Lock()
	w.lastSignal = time.Now()
	w.mu.Unlock()
	ctx, cancel := context.WithTimeout(parent, 3*time.Second)
	active, err := snapshotVPN(ctx, conn)
	cancel()
	if err != nil {
		w.Log.Warn("could not inspect VPN state after NetworkManager event", "error", err)
		return
	}
	w.updateVPNState(parent, active)
}

// updateVPNState contains the transition rule: a handler is created only for
// the single active-to-inactive edge, never while initially inactive or while
// a VPN is being connected.
func (w *Watcher) updateVPNState(parent context.Context, active bool) {
	w.mu.Lock()
	previous := w.vpnActive
	w.vpnActive = active
	if !previous && active && w.cancel != nil {
		w.cancel()
		w.cancel = nil
	}
	if previous && !active {
		if w.cancel != nil {
			w.cancel()
		}
		eventCtx, eventCancel := context.WithCancel(parent)
		w.cancel = eventCancel
		go w.afterDisconnect(eventCtx, eventCancel)
	}
	w.mu.Unlock()
}

func (w *Watcher) afterDisconnect(ctx context.Context, cancel context.CancelFunc) {
	defer cancel()
	timer := time.NewTimer(w.SettleDelay)
	defer timer.Stop()
	select {
	case <-ctx.Done():
		return
	case <-timer.C:
	}
	// NetworkManager emits several signals while routes and DNS settle. Require a
	// short quiet window, but give up rather than waiting forever on a noisy bus.
	quietDeadline := time.Now().Add(10 * time.Second)
	waitTimer := time.NewTimer(0)
	if !waitTimer.Stop() {
		<-waitTimer.C
	}
	defer waitTimer.Stop()
	for {
		w.mu.Lock()
		stillDisconnected := !w.vpnActive
		lastSignal := w.lastSignal
		w.mu.Unlock()
		if !stillDisconnected || ctx.Err() != nil {
			return
		}
		quietFor := time.Since(lastSignal)
		if quietFor >= w.QuietPeriod {
			break
		}
		remaining := time.Until(quietDeadline)
		if remaining <= 0 {
			w.Log.Warn("network did not become quiet after VPN disconnect; skipping lookup")
			return
		}
		wait := w.QuietPeriod - quietFor
		if wait > 100*time.Millisecond {
			wait = 100 * time.Millisecond
		}
		if wait > remaining {
			wait = remaining
		}
		waitTimer.Reset(wait)
		select {
		case <-ctx.Done():
			return
		case <-waitTimer.C:
		}
	}
	w.Processor.Process(ctx)
}

func (w *Watcher) cancelPending() {
	w.mu.Lock()
	defer w.mu.Unlock()
	if w.cancel != nil {
		w.cancel()
		w.cancel = nil
	}
}

func snapshotVPN(ctx context.Context, conn *dbus.Conn) (bool, error) {
	root := conn.Object(nmName, dbus.ObjectPath(nmPath))
	call := root.CallWithContext(ctx, nmName+".GetActiveConnections", 0)
	if call.Err != nil {
		return false, call.Err
	}
	var paths []dbus.ObjectPath
	if err := call.Store(&paths); err != nil {
		return false, err
	}
	for _, path := range paths {
		if err := ctx.Err(); err != nil {
			return false, err
		}
		propsCall := conn.Object(nmName, path).CallWithContext(ctx, propertiesInterface+".GetAll", 0, nmName+".Connection.Active")
		if propsCall.Err != nil {
			return false, propsCall.Err
		}
		var props map[string]dbus.Variant
		if err := propsCall.Store(&props); err != nil {
			return false, err
		}
		vpn, _ := props["Vpn"].Value().(bool)
		state, _ := props["State"].Value().(uint32)
		if vpn && state == nmActiveState {
			return true, nil
		}
	}
	return false, nil
}
