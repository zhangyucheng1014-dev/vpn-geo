use std::{path::PathBuf, process::ExitCode, sync::Arc, time::Duration};

use clap::{Args, Parser, Subcommand, ValueEnum};
use log::{error, info};
use vpn_geo_core::{
    Config, VERSION,
    benchmark::{self, SpeedResult},
    config::MirrorRegion,
    daemon, geoip, logging, mirror, network,
};

#[derive(Debug, Parser)]
#[command(name = "vpn-geo", version = VERSION, about = "NetworkManager VPN prioritizer and network diagnostics")]
struct Cli {
    #[arg(short = 'c', long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,
    #[arg(short = 'v', long, global = true)]
    verbose: bool,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Daemon,
    Check(JsonArgs),
    #[command(about = "Show VPN, system proxy and route state")]
    Status(StatusArgs),
    #[command(about = "Show GeoIP source catalog")]
    Sources(JsonArgs),
    #[command(about = "Benchmark nearest configured nodes")]
    Speed(SpeedArgs),
    #[command(about = "Benchmark current route and optional no-proxy baseline")]
    Route(RouteArgs),
    #[command(about = "Manage pacman and generic mirror sources")]
    Mirror(MirrorArgs),
    #[command(about = "Run configuration, D-Bus, GeoIP and source checks")]
    Doctor(DoctorArgs),
}

#[derive(Args, Debug)]
struct JsonArgs {
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct StatusArgs {
    #[arg(long)]
    json: bool,
    #[arg(long)]
    no_dbus: bool,
}

#[derive(Args, Debug)]
struct SpeedArgs {
    #[arg(long, help = "promote fastest successfully measured node")]
    apply: bool,
    #[arg(long)]
    direct: bool,
    #[arg(long)]
    json: bool,
    #[arg(value_name = "APPLY", help = "legacy positional form: speed apply")]
    legacy_apply: Option<String>,
}

#[derive(Args, Debug)]
struct RouteArgs {
    #[arg(long)]
    compare: bool,
    #[arg(long)]
    direct: bool,
    #[arg(long)]
    json: bool,
    #[arg(value_name = "COMPARE", help = "legacy positional form: route compare")]
    legacy_compare: Option<String>,
}

#[derive(Args, Debug)]
struct MirrorArgs {
    #[command(subcommand)]
    action: MirrorAction,
}

#[derive(Debug, Subcommand)]
enum MirrorAction {
    List(MirrorListArgs),
    Speed(MirrorSpeedArgs),
    Apply(MirrorApplyArgs),
    Sync(MirrorSyncArgs),
    Best(MirrorBestArgs),
}

#[derive(Args, Debug)]
struct MirrorListArgs {
    #[arg(short = 'r', long, value_enum)]
    region: Option<Region>,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct MirrorSpeedArgs {
    #[arg(short = 'r', long, value_enum)]
    region: Option<Region>,
    #[arg(long)]
    direct: bool,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct MirrorApplyArgs {
    name: String,
    #[arg(long, short = 'y')]
    yes: bool,
}

#[derive(Args, Debug)]
struct MirrorSyncArgs {
    #[arg(long, value_enum, value_delimiter = ',', default_values_t = vec![Region::Cn, Region::Hk, Region::Jp, Region::Us])]
    regions: Vec<Region>,
    #[arg(long, default_value_t = 6)]
    per_region: usize,
    #[arg(long)]
    json: bool,
    #[arg(long, help = "write generated [[mirrors]] entries to a TOML file")]
    output: Option<PathBuf>,
}

#[derive(Args, Debug)]
struct MirrorBestArgs {
    #[arg(short = 'r', long, value_enum)]
    region: Option<Region>,
    #[arg(long)]
    direct: bool,
    #[arg(long, short = 'y', help = "apply the fastest source to pacman")]
    yes: bool,
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
struct DoctorArgs {
    #[arg(long)]
    json: bool,
    #[arg(long)]
    skip_network: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
enum Region {
    Cn,
    Hk,
    Jp,
    Us,
    Global,
}

impl Region {
    fn core(self) -> MirrorRegion {
        match self {
            Self::Cn => MirrorRegion::Cn,
            Self::Hk => MirrorRegion::Hk,
            Self::Jp => MirrorRegion::Jp,
            Self::Us => MirrorRegion::Us,
            Self::Global => MirrorRegion::Global,
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    logging::init(cli.verbose);
    let path = cli.config.unwrap_or_else(Config::default_path);
    let config = match Config::load(&path) {
        Ok(config) => Arc::new(config),
        Err(error) => return exit(error),
    };
    match dispatch(cli.command.unwrap_or(Commands::Daemon), path, config).await {
        Ok(code) => ExitCode::from(code),
        Err(error) => exit(error),
    }
}

fn exit(error: vpn_geo_core::Error) -> ExitCode {
    error!("{error}");
    ExitCode::from(error.exit_code())
}

async fn dispatch(
    command: Commands,
    path: PathBuf,
    config: Arc<Config>,
) -> vpn_geo_core::Result<u8> {
    match command {
        Commands::Daemon => run_daemon(config).await,
        Commands::Check(args) => {
            let summary = serde_json::json!({
                "valid": true,
                "version": VERSION,
                "config_version": config.version,
                "path": path,
                "nodes": config.nodes.len(),
                "geoip_sources": config.geoip.urls.len(),
                "custom_mirrors": config.mirrors.len(),
                "settle_delay": format!("{:?}", config.settle_delay),
                "quiet_period": format!("{:?}", config.quiet_period),
            });
            print_json_or(&args.json, &summary, || {
                println!(
                    "configuration is valid: {} ({} nodes, {} GeoIP sources, {} custom mirrors)",
                    path.display(),
                    config.nodes.len(),
                    config.geoip.urls.len(),
                    config.mirrors.len()
                );
            });
            Ok(0)
        }
        Commands::Status(args) => {
            let status = if args.no_dbus {
                network::NetworkStatus {
                    vpn_active: false,
                    vpn_connections: Vec::new(),
                    active_connections: Vec::new(),
                    default_devices: Vec::new(),
                    system_proxy: false,
                    proxy_environment: false,
                    desktop_proxy: false,
                    route: "Unknown (D-Bus inspection disabled)".to_owned(),
                    confidence: "low".to_owned(),
                }
            } else {
                network::status().await?
            };
            print_json_or(&args.json, &status, || network::print_human(&status));
            Ok(0)
        }
        Commands::Sources(args) => {
            let sources = serde_json::json!({
                "urls": config.geoip.urls,
                "timeout": format!("{:?}", config.geoip.timeout),
                "min_agreement": config.geoip.min_agreement,
            });
            print_json_or(&args.json, &sources, || {
                println!("GeoIP sources (concurrent, majority vote):");
                for (index, url) in config.geoip.urls.iter().enumerate() {
                    println!("  {}. {url}", index + 1);
                }
                println!(
                    "Timeout: {:?}; minimum agreement: {}",
                    config.geoip.timeout, config.geoip.min_agreement
                );
            });
            Ok(0)
        }
        Commands::Speed(args) => run_speed(&config, args).await,
        Commands::Route(args) => run_route(&config, args).await,
        Commands::Mirror(args) => run_mirror(&config, args).await,
        Commands::Doctor(args) => run_doctor(&config, &path, args).await,
    }
}

async fn run_daemon(config: Arc<Config>) -> vpn_geo_core::Result<u8> {
    let mut backoff = Duration::from_secs(1);
    loop {
        match daemon::run(config.clone()).await {
            Ok(()) => return Ok(0),
            Err(error) => {
                error!(
                    "NetworkManager watcher stopped; reconnecting: {error} (after {}s)",
                    backoff.as_secs()
                );
                tokio::select! {
                    _ = daemon::shutdown_signal() => return Ok(0),
                    _ = tokio::time::sleep(backoff) => {}
                }
                backoff = (backoff * 2).min(Duration::from_secs(30));
            }
        }
    }
}

async fn run_speed(config: &Config, args: SpeedArgs) -> vpn_geo_core::Result<u8> {
    if let Some(value) = args.legacy_apply.as_deref() {
        if value != "apply" {
            return Err(vpn_geo_core::Error::InvalidArgument(format!(
                "unexpected speed argument {value:?}"
            )));
        }
    }
    let apply = args.apply || args.legacy_apply.is_some();
    let location = geoip::locate(config, args.direct).await?;
    let candidates = benchmark::nearest(
        &config.nodes,
        &location,
        config.benchmark.candidate_countries,
    );
    if candidates.is_empty() {
        return Err(vpn_geo_core::Error::NotFound(
            "no nodes have both latitude/longitude and test_url".to_owned(),
        ));
    }
    let targets = candidates
        .iter()
        .map(|(node, distance)| {
            (
                node.name.clone(),
                node.test_url.clone().unwrap_or_default(),
                Some(*distance),
            )
        })
        .collect();
    let results = benchmark::run(
        targets,
        benchmark::Options::from_config(config, args.direct),
    )
    .await?;
    let payload = serde_json::json!({
        "location": location,
        "results": results,
    });
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).unwrap_or_default()
        );
    } else {
        println!(
            "Current public-IP country: {} ({:.4}, {:.4}); GeoIP source: {}; agreement: {}/{}\n",
            location.country_code,
            location.latitude,
            location.longitude,
            location.source,
            location.agreement,
            config.geoip.urls.len()
        );
        benchmark::print_table(&results, true);
    }
    if apply {
        let fastest = results
            .iter()
            .find(|result| result.error.is_none())
            .ok_or_else(|| {
                vpn_geo_core::Error::NotFound("no successful speed result to apply".to_owned())
            })?;
        let node = config
            .nodes
            .iter()
            .find(|node| node.name == fastest.label)
            .ok_or_else(|| {
                vpn_geo_core::Error::Internal(format!(
                    "cannot map speed result {:?} to a configured node",
                    fastest.label
                ))
            })?;
        let changed = vpn_geo_core::priority::Manager::new(config)
            .promote_node(node)
            .await?;
        info!(
            "benchmark result applied (node={}, priority_changed={changed})",
            node.name
        );
    }
    Ok(if all_failed(&results) { 1 } else { 0 })
}

async fn run_route(config: &Config, args: RouteArgs) -> vpn_geo_core::Result<u8> {
    if let Some(value) = args.legacy_compare.as_deref() {
        if value != "compare" {
            return Err(vpn_geo_core::Error::InvalidArgument(format!(
                "unexpected route argument {value:?}"
            )));
        }
    }
    let compare = args.compare || args.legacy_compare.is_some();
    let status = network::status().await.ok();
    let location = geoip::locate(config, args.direct).await?;
    let test_url = "https://speed.cloudflare.com/__down?bytes=1000000";
    let options = benchmark::Options {
        parallel: 1,
        ..benchmark::Options::from_config(config, false)
    };
    let mut results = benchmark::run(
        vec![("current-route".to_owned(), test_url.to_owned(), None)],
        options.clone(),
    )
    .await?;
    if compare {
        let direct_options = benchmark::Options {
            direct: true,
            ..options
        };
        let mut direct = benchmark::run(
            vec![("direct-baseline".to_owned(), test_url.to_owned(), None)],
            direct_options,
        )
        .await
        .unwrap_or_default();
        results.append(&mut direct);
    }
    let payload = serde_json::json!({
        "status": status,
        "location": location,
        "results": results,
    });
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).unwrap_or_default()
        );
    } else {
        if let Some(status) = &status {
            network::print_human(status);
            println!();
        }
        println!(
            "Public-IP country: {} (source: {}, agreement: {}/{})",
            location.country_code,
            location.source,
            location.agreement,
            config.geoip.urls.len()
        );
        println!("Note: this measures the current route; toggling VPN is not performed.\n");
        benchmark::print_table(&results, false);
    }
    Ok(if all_failed(&results) { 1 } else { 0 })
}

async fn run_mirror(config: &Config, args: MirrorArgs) -> vpn_geo_core::Result<u8> {
    let catalog = mirror::Catalog::from_config(config);
    match args.action {
        MirrorAction::List(args) => {
            let mirrors = catalog.region(args.region.map(Region::core));
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&mirrors).unwrap_or_default()
                );
            } else {
                println!(
                    "{:<24} {:<8} {:<8} {:>8} {:<6} URL",
                    "NAME", "KIND", "REGION", "PRIORITY", "STATE"
                );
                for item in &mirrors {
                    println!(
                        "{:<24} {:<8} {:<8} {:>8} {:<6} {}",
                        item.name,
                        match item.kind {
                            vpn_geo_core::config::MirrorKind::Pacman => "pacman",
                            vpn_geo_core::config::MirrorKind::Generic => "generic",
                        },
                        item.region.as_str(),
                        item.priority,
                        if item.enabled { "on" } else { "off" },
                        item.url
                    );
                }
                if catalog.is_built_in() {
                    println!(
                        "\nUsing built-in source catalog ({} visible). Add [[mirrors]] to customize it.",
                        mirrors.len()
                    );
                }
            }
            Ok(0)
        }
        MirrorAction::Speed(args) => {
            let mirrors = catalog.region(args.region.map(Region::core));
            let results = mirror::speed(config, &mirrors, args.direct).await?;
            print_json_or(&args.json, &results, || {
                benchmark::print_table(&results, false)
            });
            Ok(if all_failed(&results) { 1 } else { 0 })
        }
        MirrorAction::Apply(args) => {
            if !args.yes {
                return Err(vpn_geo_core::Error::InvalidArgument(
                    "mirror apply changes /etc/pacman.d/mirrorlist; confirm with --yes".to_owned(),
                ));
            }
            let item = catalog.find(&args.name).ok_or_else(|| {
                vpn_geo_core::Error::NotFound(format!(
                    "unknown mirror source {:?}; run `vpn-geo mirror list`",
                    args.name
                ))
            })?;
            info!("{}", mirror::apply(&item).await?);
            Ok(0)
        }
        MirrorAction::Sync(args) => {
            let regions: Vec<MirrorRegion> =
                args.regions.iter().map(|region| region.core()).collect();
            let (mirrors, summary) =
                mirror::sync_official(config, &regions, args.per_region).await?;
            if let Some(path) = &args.output {
                let text = toml::to_string_pretty(&serde_json::json!({ "mirrors": mirrors }))
                    .unwrap_or_default();
                tokio::fs::write(path, text)
                    .await
                    .map_err(|error| vpn_geo_core::Error::io(path, error))?;
            }
            let payload = serde_json::json!({ "summary": summary, "mirrors": mirrors });
            print_json_or(&args.json, &payload, || {
                println!(
                    "Official status: {} entries; {} region candidates; {} selected",
                    summary.total_official, summary.candidates, summary.selected
                );
                for (region, count) in &summary.regions {
                    println!("  {region}: {count}");
                }
            });
            Ok(0)
        }
        MirrorAction::Best(args) => {
            let mirrors = catalog.region(args.region.map(Region::core));
            let results = mirror::speed(config, &mirrors, args.direct).await?;
            let best = results
                .iter()
                .find(|result| result.error.is_none())
                .ok_or_else(|| {
                    vpn_geo_core::Error::NotFound("all mirror speed tests failed".to_owned())
                })?;
            let item = catalog.find(&best.label).ok_or_else(|| {
                vpn_geo_core::Error::Internal(format!(
                    "cannot map speed result {:?} to a mirror",
                    best.label
                ))
            })?;
            let applied = if args.yes {
                Some(mirror::apply(&item).await?)
            } else {
                None
            };
            let payload = serde_json::json!({ "best": item, "result": best, "applied": applied, "results": results });
            print_json_or(&args.json, &payload, || {
                println!(
                    "Fastest source: {} ({:.2} Mbps, {:.1} ms)",
                    item.name,
                    best.mbps,
                    best.latency_ms.unwrap_or_default()
                );
                if let Some(message) = applied {
                    println!("{message}");
                } else {
                    println!("Use --yes to apply this source.");
                }
            });
            Ok(0)
        }
    }
}

async fn run_doctor(
    config: &Config,
    path: &std::path::Path,
    args: DoctorArgs,
) -> vpn_geo_core::Result<u8> {
    let catalog = mirror::Catalog::from_config(config);
    let network_status = if args.skip_network {
        None
    } else {
        network::status().await.ok()
    };
    let location = if args.skip_network {
        None
    } else {
        geoip::locate(config, false).await.ok()
    };
    let probe = if args.skip_network {
        None
    } else {
        Some(
            benchmark::run(
                catalog
                    .region(Some(MirrorRegion::Cn))
                    .into_iter()
                    .take(1)
                    .map(|item| (item.name, item.test_url, None))
                    .collect(),
                benchmark::Options {
                    samples: 1,
                    timeout: Duration::from_secs(3),
                    download_bytes: 4096,
                    parallel: 1,
                    direct: false,
                },
            )
            .await
            .ok(),
        )
    };
    let checks = serde_json::json!([
        { "name": "configuration", "ok": true, "path": path },
        { "name": "nodes", "ok": !config.nodes.is_empty(), "count": config.nodes.len() },
        { "name": "geoip_sources", "ok": !config.geoip.urls.is_empty(), "count": config.geoip.urls.len() },
        { "name": "mirrors", "ok": !catalog.is_empty(), "count": catalog.len(), "built_in": catalog.is_built_in() },
        { "name": "networkmanager", "ok": network_status.is_some(), "status": network_status },
        { "name": "public_ip_location", "ok": location.is_some(), "location": location },
        { "name": "mirror_reachability", "ok": probe.as_ref().and_then(|value| value.as_ref()).is_some_and(|results| results.iter().any(|result| result.error.is_none())), "probe": probe },
    ]);
    print_json_or(&args.json, &checks, || {
        println!("vpn-geo {VERSION} doctor");
        println!("  configuration: OK ({})", path.display());
        println!("  nodes: {} configured", config.nodes.len());
        println!("  GeoIP sources: {} configured", config.geoip.urls.len());
        println!(
            "  mirrors: {} available{}",
            catalog.len(),
            if catalog.is_built_in() {
                " (built-in)"
            } else {
                ""
            }
        );
        println!(
            "  NetworkManager: {}",
            if network_status.is_some() {
                "OK"
            } else {
                "unavailable"
            }
        );
        println!(
            "  public-IP location: {}",
            if location.is_some() {
                "OK"
            } else {
                "unavailable"
            }
        );
    });
    Ok(0)
}

fn all_failed(results: &[SpeedResult]) -> bool {
    results.iter().all(|result| result.error.is_some())
}

fn print_json_or<T: serde::Serialize>(json: &bool, value: &T, human: impl FnOnce()) {
    if *json {
        println!(
            "{}",
            serde_json::to_string_pretty(value).unwrap_or_default()
        );
    } else {
        human();
    }
}
