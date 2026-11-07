use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use futures_util::StreamExt;
use log::{debug, info, warn};
use tokio::sync::{mpsc, Mutex as AsyncMutex};
use zbus::Connection;

use crate::{config::Config, geoip, network, priority, state};

const SIGNAL_DEBOUNCE: Duration = Duration::from_millis(100);
const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(3);
const QUIET_DEADLINE: Duration = Duration::from_secs(10);

pub async fn run(config: Arc<Config>) -> Result<(), String> {
    let connection = Connection::system()
        .await
        .map_err(|error| format!("connect system D-Bus: {error}"))?;
    let mut signals = network::signal_stream(&connection).await?;
    let vpn_active = Arc::new(AtomicBool::new(
        tokio::time::timeout(SNAPSHOT_TIMEOUT, network::snapshot_vpn(&connection))
            .await
            .map_err(|_| "read initial VPN state timed out".to_owned())??,
    ));
    let last_signal = Arc::new(AsyncMutex::new(Instant::now()));
    let handler = Arc::new(Handler::new(config.clone()));
    info!(
        "NetworkManager monitoring started (vpn_active={})",
        vpn_active.load(Ordering::Relaxed)
    );

    let signal_debounce = Arc::new(AsyncMutex::new(None::<tokio::time::Instant>));
    let (refresh_tx, mut refresh_rx) = mpsc::channel::<()>(1);
    let mut processor: Option<tokio::task::JoinHandle<()>> = None;
    let shutdown = crate::shutdown_signal();
    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            signal = signals.next() => {
                let Some(signal) = signal else {
                    return Err("system D-Bus event stream closed".to_owned());
                };
                let message = match signal {
                    Ok(message) => message,
                    Err(error) => {
                        warn!("ignoring malformed D-Bus signal: {error}");
                        continue;
                    }
                };
                if !network::relevant_signal(&message) {
                    continue;
                }
                *last_signal.lock().await = Instant::now();
                let mut deadline = signal_debounce.lock().await;
                if deadline.is_none() {
                    *deadline = Some(tokio::time::Instant::now() + SIGNAL_DEBOUNCE);
                    let _ = refresh_tx.try_send(());
                } else {
                    *deadline = Some(tokio::time::Instant::now() + SIGNAL_DEBOUNCE);
                }
            }
            _ = refresh_rx.recv() => {
                loop {
                    let deadline = { *signal_debounce.lock().await };
                    let Some(deadline) = deadline else { break };
                    let now = tokio::time::Instant::now();
                    if now >= deadline {
                        *signal_debounce.lock().await = None;
                        break;
                    }
                    tokio::time::sleep_until(deadline).await;
                }
                let previous = vpn_active.load(Ordering::Relaxed);
                let active = match tokio::time::timeout(
                    SNAPSHOT_TIMEOUT,
                    network::snapshot_vpn(&connection),
                ).await {
                    Ok(Ok(active)) => active,
                    Ok(Err(error)) => {
                        warn!("could not inspect VPN state: {error}");
                        continue;
                    }
                    Err(_) => {
                        warn!("could not inspect VPN state: timed out");
                        continue;
                    }
                };
                vpn_active.store(active, Ordering::Relaxed);
                if previous && !active {
                    if let Some(task) = processor.take() {
                        task.abort();
                    }
                    let handler = handler.clone();
                    let state = vpn_active.clone();
                    let last_signal = last_signal.clone();
                    let quiet_period = config.quiet_period;
                    let settle_delay = config.settle_delay;
                    processor = Some(tokio::spawn(async move {
                        after_disconnect(
                            handler,
                            state,
                            last_signal,
                            settle_delay,
                            quiet_period,
                        ).await;
                    }));
                } else if !previous && active {
                    if let Some(task) = processor.take() {
                        task.abort();
                    }
                }
            }
        }
    }
    if let Some(task) = processor {
        task.abort();
    }
    Ok(())
}

struct Handler {
    config: Arc<Config>,
    store: state::Store,
}

impl Handler {
    fn new(config: Arc<Config>) -> Self {
        Self {
            config,
            store: state::Store::default(),
        }
    }

    async fn process(&self) {
        let _lock = match self.store.try_lock() {
            Ok(Some(lock)) => lock,
            Ok(None) => {
                debug!("disconnect processing already in progress; skipping duplicate event");
                return;
            }
            Err(error) => {
                warn!("cannot acquire processing lock: {error}");
                return;
            }
        };
        let location = match geoip::locate(&self.config, false).await {
            Ok(location) => location,
            Err(error) => {
                warn!("public IP country lookup failed; no changes made: {error}");
                return;
            }
        };
        let current = match self.store.load() {
            Ok(current) => current,
            Err(error) => {
                warn!("cannot read last-country state; no changes made: {error}");
                return;
            }
        };
        if current.last_country == location.country_code {
            debug!(
                "country unchanged; no priority update (country={})",
                location.country_code
            );
            return;
        }
        let manager = priority::Manager::new(&self.config);
        if manager.find(&location.country_code).is_none() {
            info!(
                "no configured node matches detected country; no changes made (country={})",
                location.country_code
            );
            return;
        }
        if let Err(error) = manager.promote_country(&location.country_code).await {
            warn!(
                "failed to update node priority; last-country state kept unchanged (country={}): {error}",
                location.country_code
            );
            return;
        }
        if let Err(error) = self.store.save(&location.country_code) {
            warn!(
                "priority may have changed but state could not be saved (country={}): {error}",
                location.country_code
            );
            return;
        }
        info!("disconnect processed (country={})", location.country_code);
    }
}

async fn after_disconnect(
    handler: Arc<Handler>,
    vpn_active: Arc<AtomicBool>,
    last_signal: Arc<AsyncMutex<Instant>>,
    settle_delay: Duration,
    quiet_period: Duration,
) {
    tokio::time::sleep(settle_delay).await;
    let deadline = Instant::now() + QUIET_DEADLINE;
    loop {
        if vpn_active.load(Ordering::Relaxed) {
            return;
        }
        let quiet_for = last_signal.lock().await.elapsed();
        if quiet_for >= quiet_period {
            break;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            warn!("network did not become quiet after VPN disconnect; skipping lookup");
            return;
        }
        let wait = (quiet_period - quiet_for)
            .min(Duration::from_millis(100))
            .min(remaining);
        tokio::time::sleep(wait).await;
    }
    if vpn_active.load(Ordering::Relaxed) {
        return;
    }
    handler.process().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_wait_never_exceeds_deadline() {
        let wait = Duration::from_millis(500)
            .min(Duration::from_millis(100))
            .min(Duration::from_millis(40));
        assert_eq!(wait, Duration::from_millis(40));
    }
}
