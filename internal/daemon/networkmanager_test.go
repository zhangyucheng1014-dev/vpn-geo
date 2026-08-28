package daemon

import (
	"context"
	"io"
	"log/slog"
	"testing"
	"time"
)

type processorSpy struct{ calls chan struct{} }

func (p processorSpy) Process(context.Context) { p.calls <- struct{}{} }

func TestOnlyConnectedToDisconnectedStartsProcessing(t *testing.T) {
	spy := processorSpy{calls: make(chan struct{}, 2)}
	w := &Watcher{
		SettleDelay: 0,
		QuietPeriod: 0,
		Processor:   spy,
		Log:         slog.New(slog.NewTextHandler(io.Discard, nil)),
	}
	// Initial inactive state and connecting must not trigger a lookup.
	w.updateVPNState(context.Background(), false)
	w.updateVPNState(context.Background(), true)
	select {
	case <-spy.calls:
		t.Fatal("connecting triggered processing")
	case <-time.After(20 * time.Millisecond):
	}
	// This is the only transition that may start work.
	w.updateVPNState(context.Background(), false)
	select {
	case <-spy.calls:
	case <-time.After(time.Second):
		t.Fatal("disconnect did not trigger processing")
	}
	// Duplicate inactive snapshots must not trigger a second run.
	w.updateVPNState(context.Background(), false)
	select {
	case <-spy.calls:
		t.Fatal("duplicate inactive state triggered processing")
	case <-time.After(20 * time.Millisecond):
	}
}
