package daemon

import (
	"context"
	"log/slog"

	"github.com/zhangyucheng1014-dev/vpn-geo/internal/config"
	"github.com/zhangyucheng1014-dev/vpn-geo/internal/geoip"
	"github.com/zhangyucheng1014-dev/vpn-geo/internal/priority"
	"github.com/zhangyucheng1014-dev/vpn-geo/internal/state"
)

// Handler executes outside NetworkManager's D-Bus callback, so a slow or failed
// lookup can never delay the network state transition that caused it.
type Handler struct {
	Config   config.Config
	GeoIP    *geoip.Client
	Priority *priority.Manager
	State    state.Store
	Log      *slog.Logger
}

func (h *Handler) Process(ctx context.Context) {
	logger := h.Log
	if logger == nil {
		logger = slog.Default()
	}
	if h.GeoIP == nil || h.Priority == nil {
		logger.Error("disconnect processing is not configured")
		return
	}
	defer func() {
		if recovered := recover(); recovered != nil {
			logger.Error("recovered from disconnect processing panic", "panic", recovered)
		}
	}()
	lock, err := h.State.TryLock()
	if err != nil {
		logger.Error("cannot acquire processing lock", "error", err)
		return
	}
	if lock == nil {
		logger.Debug("disconnect processing already in progress; skipping duplicate event")
		return
	}
	defer func() {
		if err := lock.Close(); err != nil {
			logger.Warn("failed to release processing lock", "error", err)
		}
	}()

	location, err := h.GeoIP.Locate(ctx)
	if err != nil {
		logger.Warn("public IP country lookup failed; no changes made", "error", err)
		return
	}
	if err := ctx.Err(); err != nil {
		logger.Debug("disconnect processing cancelled", "error", err)
		return
	}

	current, err := h.State.Load()
	if err != nil {
		logger.Error("cannot read last-country state; no changes made", "error", err)
		return
	}
	if current.LastCountry == location.CountryCode {
		logger.Debug("country unchanged; no priority update", "country", location.CountryCode)
		return
	}
	if _, found := h.Priority.Find(location.CountryCode); !found {
		logger.Info("no configured node matches detected country; no changes made", "country", location.CountryCode)
		return
	}
	if _, err := h.Priority.Promote(ctx, location.CountryCode); err != nil {
		logger.Error("failed to update node priority; last-country state kept unchanged", "country", location.CountryCode, "error", err)
		return
	}
	if err := ctx.Err(); err != nil {
		logger.Debug("disconnect processing cancelled before state write", "error", err)
		return
	}
	if err := h.State.Save(location.CountryCode); err != nil {
		logger.Error("priority may have changed but state could not be saved", "country", location.CountryCode, "error", err)
		return
	}
	logger.Info("disconnect processed", "country", location.CountryCode)
}
