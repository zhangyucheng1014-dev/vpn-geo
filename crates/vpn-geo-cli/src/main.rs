use std::{path::PathBuf, process::ExitCode, sync::Arc, time::Duration};

use clap::{Args, Parser, Subcommand, ValueEnum};
use log::{error, info};
use vpn_geo_app::{
    AppError,
    ports::NetworkGateway,
    services::{self, App},
};
use vpn_geo_domain::{VERSION, model::SpeedResult};
use vpn_geo_infra::Ports;

#[derive(Debug, Parser)]
#[command(name = "vpn-geo", version = VERSION, about = "Hexagonal NetworkManager VPN prioritizer and diagnostics")]
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
    Status(StatusArgs),
    Sources(JsonArgs),
    Speed(SpeedArgs),
    Route(RouteArgs),
    Mirror(MirrorArgs),
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
    #[arg(long)]
    apply: bool,
    #[arg(long)]
    direct: bool,
    #[arg(long)]
    json: bool,
    #[arg(value_name = "APPLY")]
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
    #[arg(value_name = "COMPARE")]
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
    #[arg(long)]
    output: Option<PathBuf>,
}

#[derive(Args, Debug)]
struct MirrorBestArgs {
    #[arg(short = 'r', long, value_enum)]
    region: Option<Region>,
    #[arg(long)]
    direct: bool,
    #[arg(long, short = 'y')]
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
    fn core(self) -> vpn_geo_domain::model::Region {
        match self {
            Self::Cn => vpn_geo_domain::model::Region::Cn,
            Self::Hk => vpn_geo_domain::model::Region::Hk,
            Self::Jp => vpn_geo_domain::model::Region::Jp,
            Self::Us => vpn_geo_domain::model::Region::Us,
            Self::Global => vpn_geo_domain::model::Region::Global,
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    init_logging(cli.verbose);
    let path = cli.config.unwrap_or_else(default_config_path);
    let ports = Arc::new(Ports::default());
    let config = match services::load_config(ports.config.as_ref(), &path).await {
        Ok(config) => config,
        Err(error) => return fail(error),
    };
    let app = App::new(config, path, ports);
    match dispatch(cli.command.unwrap_or(Commands::Daemon), &app).await {
        Ok(code) => ExitCode::from(code),
        Err(error) => fail(error),
    }
}

fn fail(error: AppError) -> ExitCode {
    error!("{error}");
    ExitCode::from(error.exit_code())
}

fn init_logging(verbose: bool) {
    let _ = log::set_logger(&Logger);
    log::set_max_level(if verbose {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    });
}

struct Logger;

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            eprintln!("{} {}", record.level(), record.args());
        }
    }

    fn flush(&self) {}
}

fn default_config_path() -> PathBuf {
    if let Some(base) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(base).join("vpn-geo").join("config.toml");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".config/vpn-geo/config.toml");
    }
    PathBuf::from("config.toml")
}

async fn dispatch(command: Commands, app: &App<Ports>) -> Result<u8, AppError> {
    match command {
        Commands::Daemon => daemon(app).await,
        Commands::Check(args) => {
            let summary = serde_json::json!({
                "valid": true,
                "version": VERSION,
                "config_version": app.config.version,
                "path": app.config_path,
                "nodes": app.config.nodes.len(),
                "geoip_sources": app.config.geoip.urls.len(),
                "custom_mirrors": app.config.mirrors.len(),
                "architecture": "domain/app/infra/cli",
            });
            json_or(args.json, &summary, || {
                println!("configuration is valid: {}", app.config_path.display())
            });
            Ok(0)
        }
        Commands::Status(args) => {
            let status = if args.no_dbus {
                vpn_geo_domain::decision::network_status(
                    false,
                    vec![],
                    vec![],
                    vec![],
                    false,
                    false,
                )
            } else {
                app.ports.network.status().await?
            };
            json_or(args.json, &status, || print_status(&status));
            Ok(0)
        }
        Commands::Sources(args) => {
            let value = serde_json::json!({
                "urls": app.config.geoip.urls,
                "timeout": format!("{:?}", app.config.geoip.timeout),
                "min_agreement": app.config.geoip.min_agreement,
            });
            json_or(args.json, &value, || {
                println!("GeoIP sources (concurrent consensus):");
                for (index, url) in app.config.geoip.urls.iter().enumerate() {
                    println!("  {}. {url}", index + 1);
                }
                println!("Minimum agreement: {}", app.config.geoip.min_agreement);
            });
            Ok(0)
        }
        Commands::Speed(args) => {
            if let Some(value) = args.legacy_apply.as_deref() {
                if value != "apply" {
                    return Err(AppError::Domain(vpn_geo_domain::DomainError::invalid(
                        format!("unexpected speed argument {value:?}"),
                    )));
                }
            }
            let apply = args.apply || args.legacy_apply.is_some();
            let (location, results, changed) =
                services::speed_nodes(app, args.direct, apply).await?;
            let payload = serde_json::json!({"location": location, "results": results, "priority_changed": changed});
            json_or(args.json, &payload, || {
                println!(
                    "Country: {}; source: {}; agreement: {}\n",
                    location.country_code, location.source, location.agreement
                );
                print_table(&results, true);
            });
            Ok(if all_failed(&results) { 1 } else { 0 })
        }
        Commands::Route(args) => {
            if let Some(value) = args.legacy_compare.as_deref() {
                if value != "compare" {
                    return Err(AppError::Domain(vpn_geo_domain::DomainError::invalid(
                        format!("unexpected route argument {value:?}"),
                    )));
                }
            }
            let (status, location, results) = services::route(
                app,
                args.compare || args.legacy_compare.is_some(),
                args.direct,
            )
            .await?;
            let payload =
                serde_json::json!({"status": status, "location": location, "results": results});
            json_or(args.json, &payload, || {
                if let Some(status) = &status {
                    print_status(status);
                    println!();
                }
                println!(
                    "Country: {}; current-route measurement only; VPN is never toggled\n",
                    location.country_code
                );
                print_table(&results, false);
            });
            Ok(if all_failed(&results) { 1 } else { 0 })
        }
        Commands::Mirror(args) => mirror(app, args).await,
        Commands::Doctor(args) => {
            let report = services::doctor(app, args.skip_network).await;
            json_or(args.json, &report, || {
                println!("vpn-geo {VERSION} doctor (domain/app/infra/cli)");
                for check in &report.checks {
                    println!(
                        "  {:<22} {} {}",
                        check.name,
                        if check.ok { "OK" } else { "WARN" },
                        check.detail
                    );
                }
            });
            Ok(0)
        }
    }
}

async fn daemon(app: &App<Ports>) -> Result<u8, AppError> {
    let mut backoff = Duration::from_secs(1);
    loop {
        match services::daemon(app).await {
            Ok(()) => return Ok(0),
            Err(error) => {
                error!(
                    "NetworkManager watcher stopped; reconnecting: {error} (after {}s)",
                    backoff.as_secs()
                );
                tokio::select! {
                    _ = shutdown() => return Ok(0),
                    _ = tokio::time::sleep(backoff) => {}
                }
                backoff = (backoff * 2).min(Duration::from_secs(30));
            }
        }
    }
}

async fn shutdown() {
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("install SIGTERM handler");
    tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
}

async fn mirror(app: &App<Ports>, args: MirrorArgs) -> Result<u8, AppError> {
    match args.action {
        MirrorAction::List(args) => {
            let (mirrors, built_in) = services::mirror_catalog(&app.config);
            let selected = vpn_geo_domain::catalog::select(&mirrors, args.region.map(Region::core));
            json_or(args.json, &selected, || {
                println!(
                    "{:<24} {:<8} {:<8} {:>8} URL",
                    "NAME", "KIND", "REGION", "PRIORITY"
                );
                for mirror in &selected {
                    println!(
                        "{:<24} {:<8} {:<8} {:>8} {}",
                        mirror.name,
                        kind(mirror.kind),
                        mirror.region.as_str(),
                        mirror.priority,
                        mirror.url
                    );
                }
                if built_in {
                    println!("\nBuilt-in catalog: {} visible", selected.len());
                }
            });
            Ok(0)
        }
        MirrorAction::Speed(args) => {
            let results =
                services::mirror_speed(app, args.region.map(Region::core), args.direct).await?;
            json_or(args.json, &results, || print_table(&results, false));
            Ok(if all_failed(&results) { 1 } else { 0 })
        }
        MirrorAction::Apply(args) => {
            if !args.yes {
                return Err(AppError::Domain(vpn_geo_domain::DomainError::invalid(
                    "mirror apply changes /etc/pacman.d/mirrorlist; confirm with --yes",
                )));
            }
            info!("{}", services::mirror_apply(app, &args.name).await?);
            Ok(0)
        }
        MirrorAction::Sync(args) => {
            let regions: Vec<_> = args.regions.iter().map(|region| region.core()).collect();
            let (mirrors, summary) = services::mirror_sync(app, &regions, args.per_region).await?;
            if let Some(path) = args.output {
                let text = toml::to_string_pretty(&serde_json::json!({"mirrors": mirrors}))
                    .unwrap_or_default();
                tokio::fs::write(&path, text)
                    .await
                    .map_err(|error| AppError::io(format!("write {}: {error}", path.display())))?;
            }
            let payload = serde_json::json!({"summary": summary, "mirrors": mirrors});
            json_or(args.json, &payload, || {
                println!(
                    "Official sources: {} entries; {} candidates; {} selected",
                    summary.total_official, summary.candidates, summary.selected
                );
                for (region, count) in &summary.regions {
                    println!("  {region}: {count}");
                }
            });
            Ok(0)
        }
        MirrorAction::Best(args) => {
            let (mirror, best, results, applied) =
                services::mirror_best(app, args.region.map(Region::core), args.direct, args.yes)
                    .await?;
            let payload = serde_json::json!({"best": mirror, "result": best, "results": results, "applied": applied});
            json_or(args.json, &payload, || {
                println!(
                    "Fastest source: {} ({:.2} Mbps, {:.1} ms)",
                    mirror.name,
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

fn print_status(status: &vpn_geo_domain::model::NetworkStatus) {
    println!("Route:          {}", status.route);
    println!("Confidence:     {}", status.confidence);
    println!(
        "VPN:            {}",
        if status.vpn_active {
            "active"
        } else {
            "inactive"
        }
    );
    println!(
        "System proxy:   {}",
        if status.system_proxy {
            "detected"
        } else {
            "not detected"
        }
    );
    println!(
        "Proxy env:      {}",
        if status.proxy_environment {
            "set"
        } else {
            "unset"
        }
    );
    println!(
        "Desktop proxy:  {}",
        if status.desktop_proxy {
            "detected"
        } else {
            "not detected"
        }
    );
    if !status.default_devices.is_empty() {
        println!("Devices:        {}", status.default_devices.join(", "));
    }
    println!(
        "Active links:   {}",
        if status.active_connections.is_empty() {
            "none".into()
        } else {
            status.active_connections.join(", ")
        }
    );
}

fn print_table(results: &[SpeedResult], distance: bool) {
    if distance {
        println!(
            "{:<24} {:>10} {:>12} {:>14}  STATUS",
            "TARGET", "DISTANCE", "LATENCY", "THROUGHPUT"
        );
    } else {
        println!(
            "{:<24} {:>12} {:>14}  STATUS",
            "TARGET", "LATENCY", "THROUGHPUT"
        );
    }
    for result in results {
        let latency = result
            .latency_ms
            .map(|value| format!("{value:.1} ms"))
            .unwrap_or_else(|| "-".into());
        let throughput = if result.error.is_none() {
            format!("{:.2} Mbps", result.mbps)
        } else {
            "-".into()
        };
        let status = result.error.as_deref().unwrap_or("ok");
        if distance {
            let km = result
                .distance_km
                .map(|value| format!("{value:.0} km"))
                .unwrap_or_else(|| "-".into());
            println!(
                "{:<24} {:>10} {:>12} {:>14}  {status}",
                result.label, km, latency, throughput
            );
        } else {
            println!(
                "{:<24} {:>12} {:>14}  {status}",
                result.label, latency, throughput
            );
        }
    }
}

fn all_failed(results: &[SpeedResult]) -> bool {
    results.iter().all(|result| result.error.is_some())
}

fn kind(kind: vpn_geo_domain::model::MirrorKind) -> &'static str {
    match kind {
        vpn_geo_domain::model::MirrorKind::Pacman => "pacman",
        vpn_geo_domain::model::MirrorKind::Generic => "generic",
    }
}

fn json_or<T: serde::Serialize>(json: bool, value: &T, human: impl FnOnce()) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(value).unwrap_or_default()
        );
    } else {
        human();
    }
}
