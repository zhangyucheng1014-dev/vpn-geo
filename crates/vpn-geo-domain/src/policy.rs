use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VpnState {
    Disconnected,
    Connected,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Transition {
    None,
    StartDisconnectJob,
    CancelDisconnectJob,
}

#[derive(Clone, Debug)]
pub struct DisconnectPolicy {
    pub settle_delay: Duration,
    pub quiet_period: Duration,
    pub quiet_deadline: Duration,
    pub max_wait: Duration,
}

impl Default for DisconnectPolicy {
    fn default() -> Self {
        Self {
            settle_delay: Duration::from_secs(3),
            quiet_period: Duration::from_millis(500),
            quiet_deadline: Duration::from_secs(10),
            max_wait: Duration::from_millis(100),
        }
    }
}

impl DisconnectPolicy {
    pub fn transition(&self, previous: VpnState, current: VpnState) -> Transition {
        match (previous, current) {
            (VpnState::Connected, VpnState::Disconnected) => Transition::StartDisconnectJob,
            (VpnState::Disconnected, VpnState::Connected) => Transition::CancelDisconnectJob,
            _ => Transition::None,
        }
    }

    pub fn quiet_wait(
        &self,
        last_signal: Instant,
        now: Instant,
        started: Instant,
    ) -> Option<Duration> {
        if now.duration_since(started) >= self.quiet_deadline {
            return None;
        }
        let quiet_for = now.saturating_duration_since(last_signal);
        if quiet_for >= self.quiet_period {
            return Some(Duration::ZERO);
        }
        Some((self.quiet_period - quiet_for).min(self.max_wait))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_disconnect_edge_starts_job() {
        let policy = DisconnectPolicy::default();
        assert_eq!(
            policy.transition(VpnState::Connected, VpnState::Disconnected),
            Transition::StartDisconnectJob
        );
        assert_eq!(
            policy.transition(VpnState::Disconnected, VpnState::Connected),
            Transition::CancelDisconnectJob
        );
        assert_eq!(
            policy.transition(VpnState::Disconnected, VpnState::Disconnected),
            Transition::None
        );
    }

    #[test]
    fn quiet_wait_respects_deadline() {
        let policy = DisconnectPolicy::default();
        let started = Instant::now();
        let last_signal = started;
        assert_eq!(
            policy.quiet_wait(last_signal, started, started),
            Some(Duration::from_millis(100))
        );
        assert_eq!(
            policy.quiet_wait(last_signal, started + Duration::from_secs(11), started),
            None
        );
    }
}
