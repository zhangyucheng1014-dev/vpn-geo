use std::{path::Path, time::Duration};

use async_trait::async_trait;
use vpn_geo_domain::{
    config::Config,
    model::{Location, Mirror, NetworkStatus, Region, SpeedResult, SyncSummary},
};

use crate::AppResult;

#[async_trait]
pub trait ConfigSource: Send + Sync {
    async fn load(&self, path: &Path) -> AppResult<Config>;
}

#[async_trait]
pub trait GeoIpGateway: Send + Sync {
    async fn votes(&self, config: &Config, direct: bool) -> AppResult<Vec<Location>>;
}

#[async_trait]
pub trait NetworkGateway: Send + Sync {
    async fn status(&self) -> AppResult<NetworkStatus>;
    async fn vpn_active(&self) -> AppResult<bool>;
}

#[async_trait]
pub trait PriorityGateway: Send + Sync {
    async fn promote_country(&self, config: &Config, country: &str) -> AppResult<bool>;
    async fn promote_node(&self, config: &Config, name: &str) -> AppResult<bool>;
}

#[async_trait]
pub trait SpeedGateway: Send + Sync {
    async fn measure(
        &self,
        targets: Vec<(String, String, Option<f64>)>,
        samples: usize,
        timeout: Duration,
        bytes: u64,
        parallel: usize,
        direct: bool,
    ) -> AppResult<Vec<SpeedResult>>;
}

#[async_trait]
pub trait MirrorGateway: Send + Sync {
    async fn sync_official(
        &self,
        regions: &[Region],
        per_region: usize,
    ) -> AppResult<(Vec<Mirror>, SyncSummary)>;
    async fn apply(&self, mirror: &Mirror) -> AppResult<String>;
}

#[async_trait]
pub trait StateStore: Send + Sync {
    async fn lock(&self) -> AppResult<bool>;
    async fn load_country(&self) -> AppResult<Option<String>>;
    async fn save_country(&self, country: &str) -> AppResult<()>;
    async fn unlock(&self);
}

#[async_trait]
pub trait VpnEventStream: Send + Sync {
    async fn initial_vpn_active(&mut self) -> AppResult<bool>;
    async fn wait_event(&mut self) -> AppResult<()>;
    async fn current_vpn_active(&mut self) -> AppResult<bool>;
}
