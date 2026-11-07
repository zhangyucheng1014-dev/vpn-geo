mod benchmark;
mod config;
mod daemon;
mod geoip;
mod http;
mod logging;
mod mirror;
mod network;
mod priority;
mod state;

use std::{path::PathBuf, process::ExitCode, sync::Arc, time::Duration};

use config::{Config, MirrorKind};
use log::{debug, error, info, warn};

#[derive(Debug)]
struct Global {
    config: PathBuf,
    verbose: bool,
    command: Command,
}

#[derive(Debug)]
enum Command {
    Daemon,
    Check { json: bool },
    Status { json: bool },
    Sources { json: bool },
    Speed(SpeedCommand),
    Route(RouteCommand),
    Mirror(MirrorCommand),
    Help,
}

#[derive(Debug)]
struct SpeedCommand {
    apply: bool,
    direct: bool,
    json: bool,
}

#[derive(Debug)]
struct RouteCommand {
    compare: bool,
    direct: bool,
    json: bool,
}

#[derive(Debug)]
enum MirrorCommand {
    List { json: bool },
    Speed { direct: bool, json: bool },
    Apply { name: String },
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let global = match parse_args(std::env::args().skip(1).collect()) {
        Ok(global) => global,
        Err(error) => {
            eprintln!("{error}\n");
            print_usage();
            return ExitCode::from(2);
        }
    };
    if matches!(global.command, Command::Help) {
        print_usage();
        return ExitCode::SUCCESS;
    }
    logging::init(global.verbose);
    let config = match Config::load(&global.config) {
        Ok(config) => Arc::new(config),
        Err(error) => {
            error!(
                "configuration is invalid (path={}): {error}",
                global.config.display()
            );
            return ExitCode::from(2);
        }
    };
    let code = dispatch(global, config).await;
    ExitCode::from(code)
}

async fn dispatch(global: Global, config: Arc<Config>) -> u8 {
    match global.command {
        Command::Daemon => run_daemon(config).await,
        Command::Check { json } => run_check(&global.config, &config, json),
        Command::Status { json } => match network::status().await {
            Ok(status) => {
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&status).unwrap_or_else(|_| "{}".to_owned())
                    );
                } else {
                    network::print_human(&status);
                }
                0
            }
            Err(error) => {
                error!("cannot inspect network status: {error}");
                1
            }
        },
        Command::Sources { json } => run_sources(&config, json),
        Command::Speed(command) => run_speed(&config, command).await,
        Command::Route(command) => run_route(&config, command).await,
        Command::Mirror(command) => match command {
            MirrorCommand::List { json } => mirror::list(&global.config, &config, json).await,
            MirrorCommand::Speed { direct, json } => mirror::speed(&config, direct, json).await,
            MirrorCommand::Apply { name } => run_mirror_apply(&config, &name),
        },
        Command::Help => 0,
    }
}

fn parse_args(args: Vec<String>) -> Result<Global, String> {
    let mut config = Config::default_path();
    let mut verbose = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "-c" | "--config" => {
                index += 1;
                config = PathBuf::from(args.get(index).ok_or("missing config path")?);
            }
            "-v" | "--verbose" => verbose = true,
            "-h" | "--help" | "help" => {
                return Ok(Global {
                    config,
                    verbose,
                    command: Command::Help,
                })
            }
            "--version" | "-V" => {
                println!("vpn-geo {}", config::VERSION);
                std::process::exit(0);
            }
            value if value.starts_with('-') => {
                return Err(format!("unknown global option {value:?}"))
            }
            _ => break,
        }
        index += 1;
    }
    let mut rest = args.into_iter().skip(index);
    let command = match rest.next().as_deref() {
        None | Some("daemon") => Command::Daemon,
        Some("check") => Command::Check {
            json: parse_json_flag(rest.collect())?,
        },
        Some("status") => Command::Status {
            json: parse_json_flag(rest.collect())?,
        },
        Some("sources") => Command::Sources {
            json: parse_json_flag(rest.collect())?,
        },
        Some("speed") => Command::Speed(parse_speed(rest.collect())?),
        Some("route") => Command::Route(parse_route(rest.collect())?),
        Some("mirror") => Command::Mirror(parse_mirror(rest.collect())?),
        Some(other) => return Err(format!("unknown command {other:?}")),
    };
    Ok(Global {
        config,
        verbose,
        command,
    })
}

fn parse_json_flag(args: Vec<String>) -> Result<bool, String> {
    let mut json = false;
    for arg in args {
        match arg.as_str() {
            "--json" => json = true,
            "-h" | "--help" => print_usage(),
            other => return Err(format!("unexpected argument {other:?}")),
        }
    }
    Ok(json)
}

fn parse_speed(args: Vec<String>) -> Result<SpeedCommand, String> {
    let mut command = SpeedCommand {
        apply: false,
        direct: false,
        json: false,
    };
    for arg in args {
        match arg.as_str() {
            "apply" | "--apply" => command.apply = true,
            "--direct" => command.direct = true,
            "--json" => command.json = true,
            "-h" | "--help" => {
                print_speed_usage();
                std::process::exit(0);
            }
            other => return Err(format!("unexpected speed argument {other:?}")),
        }
    }
    Ok(command)
}

fn parse_route(args: Vec<String>) -> Result<RouteCommand, String> {
    let mut command = RouteCommand {
        compare: false,
        direct: false,
        json: false,
    };
    for arg in args {
        match arg.as_str() {
            "compare" | "--compare" => command.compare = true,
            "--direct" => command.direct = true,
            "--json" => command.json = true,
            "-h" | "--help" => {
                print_route_usage();
                std::process::exit(0);
            }
            other => return Err(format!("unexpected route argument {other:?}")),
        }
    }
    Ok(command)
}

fn parse_mirror(args: Vec<String>) -> Result<MirrorCommand, String> {
    let mut args = args.into_iter();
    let subcommand = args.next().unwrap_or_else(|| "list".to_owned());
    let rest: Vec<String> = args.collect();
    match subcommand.as_str() {
        "list" | "ls" => Ok(MirrorCommand::List {
            json: parse_json_flag(rest)?,
        }),
        "speed" => {
            let mut direct = false;
            let mut json = false;
            for arg in rest {
                match arg.as_str() {
                    "--direct" => direct = true,
                    "--json" => json = true,
                    other => return Err(format!("unexpected mirror speed argument {other:?}")),
                }
            }
            Ok(MirrorCommand::Speed { direct, json })
        }
        "apply" => {
            let mut name = None;
            let mut yes = false;
            for arg in rest {
                match arg.as_str() {
                    "--yes" | "-y" => yes = true,
                    value if value.starts_with('-') => {
                        return Err(format!("unexpected mirror apply argument {value:?}"))
                    }
                    value if name.is_none() => name = Some(value.to_owned()),
                    _ => return Err("mirror apply accepts exactly one source name".to_owned()),
                }
            }
            if !yes {
                return Err(
                    "mirror apply changes /etc/pacman.d/mirrorlist; confirm with --yes".to_owned(),
                );
            }
            Ok(MirrorCommand::Apply {
                name: name.ok_or("mirror apply needs a source name")?,
            })
        }
        other => Err(format!("unknown mirror subcommand {other:?}")),
    }
}

async fn run_daemon(config: Arc<Config>) -> u8 {
    let mut backoff = Duration::from_secs(1);
    loop {
        match daemon::run(config.clone()).await {
            Ok(()) => return 0,
            Err(error) => {
                error!(
                    "NetworkManager watcher stopped; reconnecting: {error} (after {}s)",
                    backoff.as_secs()
                );
                tokio::select! {
                    _ = shutdown_signal() => return 0,
                    _ = tokio::time::sleep(backoff) => {}
                }
                backoff = (backoff * 2).min(Duration::from_secs(30));
            }
        }
    }
}

fn run_check(path: &std::path::Path, config: &Config, json: bool) -> u8 {
    let summary = serde_json::json!({
        "valid": true,
        "path": path,
        "version": config::VERSION,
        "nodes": config.nodes.len(),
        "geoip_sources": config.geoip.urls.len(),
        "mirrors": config.mirrors.len(),
        "settle_delay": format!("{:?}", config.settle_delay),
        "quiet_period": format!("{:?}", config.quiet_period),
    });
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&summary).unwrap_or_default()
        );
    } else {
        println!(
            "configuration is valid: {} ({} nodes, {} GeoIP sources, {} custom mirrors)",
            path.display(),
            config.nodes.len(),
            config.geoip.urls.len(),
            config.mirrors.len()
        );
    }
    0
}

fn run_sources(config: &Config, json: bool) -> u8 {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&config.geoip.urls).unwrap_or_else(|_| "[]".to_owned())
        );
        return 0;
    }
    println!("GeoIP sources (concurrent, majority vote):");
    for (index, url) in config.geoip.urls.iter().enumerate() {
        println!("  {}. {url}", index + 1);
    }
    println!("Timeout: {:?}", config.geoip.timeout);
    0
}

async fn run_speed(config: &Config, command: SpeedCommand) -> u8 {
    let location = match geoip::locate(config, command.direct).await {
        Ok(location) => location,
        Err(error) => {
            error!("cannot locate current public IP: {error}");
            return 1;
        }
    };
    let candidates = benchmark::nearest(
        &config.nodes,
        &location,
        config.benchmark.candidate_countries,
    );
    if candidates.is_empty() {
        error!("no nodes have both latitude/longitude and test_url");
        return 1;
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
    let results = match benchmark::run(
        config,
        targets,
        benchmark::Options {
            samples: config.benchmark.samples,
            timeout: config.benchmark.request_timeout,
            download_bytes: config.benchmark.download_bytes,
            parallel: config.benchmark.parallel,
            direct: command.direct,
        },
    )
    .await
    {
        Ok(results) => results,
        Err(error) => {
            error!("speed test failed: {error}");
            return 1;
        }
    };
    if command.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&results).unwrap_or_else(|_| "[]".to_owned())
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
    let fastest = results.iter().find(|result| result.error.is_none());
    if command.apply {
        let Some(fastest) = fastest else {
            error!("no successful speed result to apply");
            return 1;
        };
        let Some(node) = config.nodes.iter().find(|node| node.name == fastest.label) else {
            error!(
                "cannot map speed result {:?} to a configured node",
                fastest.label
            );
            return 1;
        };
        match priority::Manager::new(config).promote_node(node).await {
            Ok(changed) => info!(
                "benchmark result applied (node={}, priority_changed={changed})",
                node.name
            ),
            Err(error) => {
                error!("failed to apply benchmark result: {error}");
                return 1;
            }
        }
    }
    if results.iter().all(|result| result.error.is_some()) {
        1
    } else {
        0
    }
}

async fn run_route(config: &Config, command: RouteCommand) -> u8 {
    let status: Option<network::NetworkStatus> = match network::status().await {
        Ok(status) => Some(status),
        Err(error) => {
            warn!("cannot inspect NetworkManager state: {error}");
            None
        }
    };
    let location = match geoip::locate(config, command.direct).await {
        Ok(location) => location,
        Err(error) => {
            error!("cannot locate current public IP: {error}");
            return 1;
        }
    };
    let current_targets = if command.compare {
        vec![(
            "system-route".to_owned(),
            "https://speed.cloudflare.com/__down?bytes=1000000".to_owned(),
            None,
        )]
    } else {
        vec![(
            "current-route".to_owned(),
            "https://speed.cloudflare.com/__down?bytes=1000000".to_owned(),
            None,
        )]
    };
    let options = benchmark::Options {
        samples: config.benchmark.samples,
        timeout: config.benchmark.request_timeout,
        download_bytes: config.benchmark.download_bytes,
        parallel: 1,
        direct: false,
    };
    let mut results = match benchmark::run(config, current_targets, options.clone()).await {
        Ok(results) => results,
        Err(error) => {
            error!("route test failed: {error}");
            return 1;
        }
    };
    if command.compare {
        let direct_options = benchmark::Options {
            direct: true,
            ..options
        };
        if let Ok(mut direct) = benchmark::run(
            config,
            vec![(
                "direct-baseline".to_owned(),
                "https://speed.cloudflare.com/__down?bytes=1000000".to_owned(),
                None,
            )],
            direct_options,
        )
        .await
        {
            results.append(&mut direct);
        }
    }
    if command.json {
        let payload = serde_json::json!({
            "status": status,
            "public_country": location.country_code,
            "source": location.source,
            "agreement": location.agreement,
            "results": results,
        });
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
    if results.iter().all(|result| result.error.is_some()) {
        1
    } else {
        0
    }
}

fn run_mirror_apply(config: &Config, name: &str) -> u8 {
    let Some(mirror) = crate::benchmark::mirror_results(config)
        .into_iter()
        .find(|mirror| mirror.name == name)
    else {
        error!("unknown mirror source {name:?}; run `vpn-geo mirror list`");
        return 2;
    };
    if mirror.kind != MirrorKind::Pacman {
        error!("source {name:?} is generic and cannot be applied to pacman");
        return 2;
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build();
    let Ok(runtime) = runtime else {
        error!("cannot start runtime for mirror apply");
        return 1;
    };
    match runtime.block_on(mirror::apply(&mirror)) {
        Ok(message) => {
            info!("{message}");
            0
        }
        Err(error) => {
            error!("cannot apply mirror source: {error}");
            1
        }
    }
}

pub async fn shutdown_signal() {
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("install SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
    debug!("shutdown requested");
}

fn print_speed_usage() {
    eprintln!("Usage: vpn-geo speed [apply] [--direct] [--json]");
    eprintln!("  no apply       show node benchmark results only");
    eprintln!("  apply          benchmark and promote the fastest node");
}

fn print_route_usage() {
    eprintln!("Usage: vpn-geo route [compare] [--direct] [--json]");
    eprintln!("  compare        measure current route and no-proxy baseline");
    eprintln!("  --direct       force GeoIP lookup to bypass proxy settings");
}

fn print_usage() {
    println!(
        "vpn-geo {} - NetworkManager VPN prioritizer and network diagnostics",
        config::VERSION
    );
    println!("\nUsage: vpn-geo [-c PATH] [-v] <command>");
    println!("\nCommands:");
    println!("  (none) / daemon       run the event-driven background watcher");
    println!("  check [--json]        validate configuration");
    println!("  status [--json]       show VPN, system proxy and route state");
    println!("  sources [--json]      show GeoIP source catalog");
    println!("  speed [apply]         benchmark nearest configured nodes");
    println!("  route [compare]       benchmark current route and direct baseline");
    println!("  mirror list           list pacman/generic source catalog");
    println!("  mirror speed          benchmark configured mirrors");
    println!("  mirror apply NAME -y  apply a pacman source (root required)");
    println!("\nGlobal options:");
    println!("  -c, --config PATH     TOML configuration path");
    println!("  -v, --verbose         enable debug logs");
    println!("  -h, --help            show this help");
}
