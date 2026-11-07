use std::sync::Arc;

use crate::ports_impl::{
    ConfigRepository, FileStateStore, HttpGeoIpGateway, HttpMirrorGateway, HttpSpeedGateway,
    NetworkManagerGateway, NmcliPriorityGateway, VpnWatcher,
};

#[derive(Clone)]
pub struct Ports {
    pub config: Arc<ConfigRepository>,
    pub geoip: Arc<HttpGeoIpGateway>,
    pub network: Arc<NetworkManagerGateway>,
    pub priority: Arc<NmcliPriorityGateway>,
    pub speed: Arc<HttpSpeedGateway>,
    pub mirror: Arc<HttpMirrorGateway>,
    pub state: Arc<FileStateStore>,
}

impl Default for Ports {
    fn default() -> Self {
        Self {
            config: Arc::new(ConfigRepository),
            geoip: Arc::new(HttpGeoIpGateway),
            network: Arc::new(NetworkManagerGateway),
            priority: Arc::new(NmcliPriorityGateway),
            speed: Arc::new(HttpSpeedGateway),
            mirror: Arc::new(HttpMirrorGateway),
            state: Arc::new(FileStateStore::default()),
        }
    }
}

impl vpn_geo_app::services::PortsSpeed for Ports {
    type GeoIp = HttpGeoIpGateway;
    type Network = NetworkManagerGateway;
    type Speed = HttpSpeedGateway;
    type Priority = NmcliPriorityGateway;
    fn geoip(&self) -> &Self::GeoIp {
        &self.geoip
    }
    fn network(&self) -> &Self::Network {
        &self.network
    }
    fn speed(&self) -> &Self::Speed {
        &self.speed
    }
    fn priority(&self) -> &Self::Priority {
        &self.priority
    }
}

impl vpn_geo_app::services::PortsMirror for Ports {
    type Mirror = HttpMirrorGateway;
    fn mirror(&self) -> &Self::Mirror {
        &self.mirror
    }
}

impl vpn_geo_app::services::PortsDoctor for Ports {}

impl vpn_geo_app::services::PortsDaemon for Ports {
    type GeoIp = HttpGeoIpGateway;
    type Priority = NmcliPriorityGateway;
    type State = FileStateStore;
    type Events = VpnWatcher;
    fn geoip(&self) -> &Self::GeoIp {
        &self.geoip
    }
    fn priority(&self) -> &Self::Priority {
        &self.priority
    }
    fn state(&self) -> &Self::State {
        &self.state
    }
    fn events(&self) -> Self::Events {
        VpnWatcher::default()
    }
}
