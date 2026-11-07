use std::{path::Path, sync::Arc, time::Duration};

use log::{debug, info, warn};
use vpn_geo_domain::{
    catalog,
    config::Config,
    decision,
    model::{
        DoctorCheck, DoctorReport, Location, Mirror, NetworkStatus, Region, SpeedResult,
        SyncSummary,
    },
    policy::{DisconnectPolicy, Transition, VpnState},
};

use crate::{
    AppError, AppResult,
    ports::{
        ConfigSource, GeoIpGateway, MirrorGateway, NetworkGateway, PriorityGateway, SpeedGateway,
        StateStore, VpnEventStream,
    },
};

pub struct App<P> {
    pub config: Config,
    pub config_path: std::path::PathBuf,
    pub ports: Arc<P>,
}

#[async_trait::async_trait]
pub trait AppPorts: Send + Sync {
    type Config: ConfigSource;
    type GeoIp: GeoIpGateway;
    type Network: NetworkGateway;
    type Priority: PriorityGateway;
    type Speed: SpeedGateway;
    type Mirror: MirrorGateway;
    type State: StateStore;
    type Events: VpnEventStream;
}

impl<P> App<P> {
    pub fn new(config: Config, config_path: std::path::PathBuf, ports: Arc<P>) -> Self {
        Self {
            config,
            config_path,
            ports,
        }
    }
}

pub async fn load_config(source: &impl ConfigSource, path: &Path) -> AppResult<Config> {
    source.load(path).await
}

pub async fn locate(
    geoip: &impl GeoIpGateway,
    config: &Config,
    direct: bool,
) -> AppResult<Location> {
    let votes = geoip.votes(config, direct).await?;
    decision::geoip_consensus(votes, config.geoip.min_agreement).map_err(AppError::Http)
}

pub async fn speed_nodes<P>(
    app: &App<P>,
    direct: bool,
    apply: bool,
) -> AppResult<(Location, Vec<SpeedResult>, bool)>
where
    P: PortsSpeed,
{
    let location = locate(app.ports.geoip(), &app.config, direct).await?;
    let candidates = decision::nearest_nodes(
        &app.config.nodes,
        &location,
        app.config.benchmark.candidate_countries,
    );
    if candidates.is_empty() {
        return Err(AppError::Domain(vpn_geo_domain::DomainError::not_found(
            "no nodes have both latitude/longitude and test_url",
        )));
    }
    let targets = candidates
        .into_iter()
        .map(|(node, distance)| (node.name, node.test_url.unwrap_or_default(), Some(distance)))
        .collect();
    let results = speed(&app.config, app.ports.speed(), targets, direct).await?;
    let mut changed = false;
    if apply {
        let best = decision::best_result(&results).ok_or_else(|| {
            AppError::Domain(vpn_geo_domain::DomainError::not_found(
                "no successful speed result to apply",
            ))
        })?;
        changed = app
            .ports
            .priority()
            .promote_node(&app.config, &best.label)
            .await?;
    }
    Ok((location, results, changed))
}

async fn speed(
    config: &Config,
    gateway: &impl SpeedGateway,
    targets: Vec<(String, String, Option<f64>)>,
    direct: bool,
) -> AppResult<Vec<SpeedResult>> {
    gateway
        .measure(
            targets,
            config.benchmark.samples,
            config.benchmark.request_timeout,
            config.benchmark.download_bytes,
            config.benchmark.parallel,
            direct,
        )
        .await
}

pub async fn route<P>(
    app: &App<P>,
    compare: bool,
    direct: bool,
) -> AppResult<(Option<NetworkStatus>, Location, Vec<SpeedResult>)>
where
    P: PortsSpeed,
{
    let status = app.ports.network().status().await.ok();
    let location = locate(app.ports.geoip(), &app.config, direct).await?;
    let test_url = "https://speed.cloudflare.com/__down?bytes=1000000";
    let mut results = speed(
        &app.config,
        app.ports.speed(),
        vec![("current-route".to_owned(), test_url.to_owned(), None)],
        false,
    )
    .await?;
    if compare {
        let direct_results = speed(
            &app.config,
            app.ports.speed(),
            vec![("direct-baseline".to_owned(), test_url.to_owned(), None)],
            true,
        )
        .await?;
        results.extend(direct_results);
    }
    Ok((status, location, results))
}

pub fn mirror_catalog(config: &Config) -> (Vec<Mirror>, bool) {
    let custom: Vec<Mirror> = config
        .mirrors
        .iter()
        .filter(|mirror| mirror.enabled)
        .cloned()
        .collect();
    if custom.is_empty() {
        (catalog::built_in(), true)
    } else {
        (custom, false)
    }
}

pub async fn mirror_speed<P>(
    app: &App<P>,
    region: Option<Region>,
    direct: bool,
) -> AppResult<Vec<SpeedResult>>
where
    P: PortsSpeed,
{
    let (mirrors, _) = mirror_catalog(&app.config);
    let selected = catalog::select(&mirrors, region);
    if selected.is_empty() {
        return Err(AppError::Domain(vpn_geo_domain::DomainError::not_found(
            "no mirrors match the selected region",
        )));
    }
    let targets = selected
        .into_iter()
        .map(|mirror| (mirror.name, mirror.test_url, None))
        .collect();
    speed(&app.config, app.ports.speed(), targets, direct).await
}

pub async fn mirror_best<P>(
    app: &App<P>,
    region: Option<Region>,
    direct: bool,
    apply: bool,
) -> AppResult<(Mirror, SpeedResult, Vec<SpeedResult>, Option<String>)>
where
    P: PortsMirror,
{
    let results = mirror_speed(app, region, direct).await?;
    let best = decision::best_result(&results).ok_or_else(|| {
        AppError::Domain(vpn_geo_domain::DomainError::not_found(
            "all mirror speed tests failed",
        ))
    })?;
    let (mirrors, _) = mirror_catalog(&app.config);
    let mirror = mirrors
        .into_iter()
        .find(|mirror| mirror.name == best.label)
        .ok_or_else(|| {
            AppError::Internal(format!(
                "cannot map speed result {:?} to a mirror",
                best.label
            ))
        })?;
    let applied = if apply {
        Some(app.ports.mirror().apply(&mirror).await?)
    } else {
        None
    };
    Ok((mirror, best.clone(), results, applied))
}

pub async fn mirror_sync<P>(
    app: &App<P>,
    regions: &[Region],
    per_region: usize,
) -> AppResult<(Vec<Mirror>, SyncSummary)>
where
    P: PortsMirror,
{
    app.ports.mirror().sync_official(regions, per_region).await
}

pub async fn mirror_apply<P>(app: &App<P>, name: &str) -> AppResult<String>
where
    P: PortsMirror,
{
    let (mirrors, _) = mirror_catalog(&app.config);
    let mirror = mirrors
        .into_iter()
        .find(|mirror| mirror.name == name)
        .ok_or_else(|| {
            AppError::Domain(vpn_geo_domain::DomainError::not_found(format!(
                "unknown mirror source {name:?}"
            )))
        })?;
    app.ports.mirror().apply(&mirror).await
}

pub async fn doctor<P>(app: &App<P>, skip_network: bool) -> DoctorReport
where
    P: PortsDoctor,
{
    let (mirrors, built_in) = mirror_catalog(&app.config);
    let network_status = if skip_network {
        None
    } else {
        app.ports.network().status().await.ok()
    };
    let location = if skip_network {
        None
    } else {
        locate(app.ports.geoip(), &app.config, false).await.ok()
    };
    let mirror_count = mirrors.len();
    let probe = if skip_network || mirrors.is_empty() {
        None
    } else {
        app.ports
            .speed()
            .measure(
                mirrors
                    .into_iter()
                    .take(1)
                    .map(|mirror| (mirror.name, mirror.test_url, None))
                    .collect(),
                1,
                Duration::from_secs(3),
                4096,
                1,
                false,
            )
            .await
            .ok()
    };
    DoctorReport {
        checks: vec![
            DoctorCheck {
                name: "configuration".into(),
                ok: true,
                detail: app.config_path.display().to_string(),
            },
            DoctorCheck {
                name: "nodes".into(),
                ok: !app.config.nodes.is_empty(),
                detail: format!("{} configured", app.config.nodes.len()),
            },
            DoctorCheck {
                name: "geoip_sources".into(),
                ok: !app.config.geoip.urls.is_empty(),
                detail: format!("{} configured", app.config.geoip.urls.len()),
            },
            DoctorCheck {
                name: "mirrors".into(),
                ok: mirror_count > 0,
                detail: format!(
                    "{} available{}",
                    mirror_count,
                    if built_in { " (built-in)" } else { "" }
                ),
            },
            DoctorCheck {
                name: "mirror_reachability".into(),
                ok: probe
                    .as_ref()
                    .is_some_and(|results| results.iter().any(|result| result.error.is_none())),
                detail: "one-source probe".into(),
            },
            DoctorCheck {
                name: "networkmanager".into(),
                ok: network_status.is_some(),
                detail: if network_status.is_some() {
                    "OK".into()
                } else {
                    "unavailable".into()
                },
            },
            DoctorCheck {
                name: "public_ip_location".into(),
                ok: location.is_some(),
                detail: location
                    .map(|value| value.country_code)
                    .unwrap_or_else(|| "unavailable".into()),
            },
        ],
    }
}

pub async fn daemon<P>(app: &App<P>) -> AppResult<()>
where
    P: PortsDaemon,
{
    let mut events = app.ports.events();
    let mut state = if events.initial_vpn_active().await? {
        VpnState::Connected
    } else {
        VpnState::Disconnected
    };
    info!(
        "NetworkManager monitoring started (vpn_active={})",
        matches!(state, VpnState::Connected)
    );
    let policy = DisconnectPolicy {
        settle_delay: app.config.settle_delay,
        quiet_period: app.config.quiet_period,
        ..DisconnectPolicy::default()
    };
    let mut job: Option<tokio::task::JoinHandle<()>> = None;
    loop {
        tokio::select! {
            _ = daemon_shutdown() => break,
            event = events.wait_event() => {
                event?;
                let current = if events.current_vpn_active().await? { VpnState::Connected } else { VpnState::Disconnected };
                match policy.transition(state, current) {
                    Transition::StartDisconnectJob => {
                        if let Some(task) = job.take() { task.abort(); }
                        job = Some(spawn_disconnect(app, policy.clone()));
                    }
                    Transition::CancelDisconnectJob => {
                        if let Some(task) = job.take() { task.abort(); }
                    }
                    Transition::None => {}
                }
                state = current;
            }
        }
    }
    if let Some(task) = job {
        task.abort();
    }
    Ok(())
}

fn spawn_disconnect<P>(app: &App<P>, policy: DisconnectPolicy) -> tokio::task::JoinHandle<()>
where
    P: PortsDaemon,
{
    let ports = app.ports.clone();
    let config = app.config.clone();
    tokio::spawn(async move {
        tokio::time::sleep(policy.settle_delay).await;
        if let Err(error) = process_disconnect(ports, config).await {
            warn!("disconnect processing failed; no unsafe retry: {error}");
        }
    })
}

async fn process_disconnect<P>(ports: Arc<P>, config: Config) -> AppResult<()>
where
    P: PortsDaemon,
{
    if !ports.state().lock().await? {
        debug!("disconnect processing already in progress; skipping duplicate event");
        return Ok(());
    }
    let outcome = async {
        let votes = ports.geoip().votes(&config, false).await?;
        let location =
            decision::geoip_consensus(votes, config.geoip.min_agreement).map_err(AppError::Http)?;
        if ports.state().load_country().await?.as_deref() == Some(location.country_code.as_str()) {
            debug!("country unchanged; no priority update");
            return Ok(());
        }
        if !config
            .nodes
            .iter()
            .any(|node| node.country == location.country_code)
        {
            info!("no configured node matches detected country; no changes made");
            return Ok(());
        }
        ports
            .priority()
            .promote_country(&config, &location.country_code)
            .await?;
        ports.state().save_country(&location.country_code).await?;
        info!("disconnect processed (country={})", location.country_code);
        Ok(())
    }
    .await;
    ports.state().unlock().await;
    outcome
}

async fn daemon_shutdown() {
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("install SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
}

pub trait PortsSpeed: Send + Sync {
    type GeoIp: GeoIpGateway;
    type Network: NetworkGateway;
    type Speed: SpeedGateway;
    type Priority: PriorityGateway;
    fn geoip(&self) -> &Self::GeoIp;
    fn network(&self) -> &Self::Network;
    fn speed(&self) -> &Self::Speed;
    fn priority(&self) -> &Self::Priority;
}

pub trait PortsMirror: PortsSpeed {
    type Mirror: MirrorGateway;
    fn mirror(&self) -> &Self::Mirror;
}

pub trait PortsDoctor: PortsSpeed {
    fn events_unused() {}
}

pub trait PortsDaemon: Send + Sync + 'static {
    type GeoIp: GeoIpGateway;
    type Priority: PriorityGateway;
    type State: StateStore;
    type Events: VpnEventStream;
    fn geoip(&self) -> &Self::GeoIp;
    fn priority(&self) -> &Self::Priority;
    fn state(&self) -> &Self::State;
    fn events(&self) -> Self::Events;
}
