use std::{collections::HashSet, env, process::Output};

use async_trait::async_trait;
use log::warn;
use tokio::process::Command;
use vpn_geo_app::{AppResult, ports::NetworkGateway};
use vpn_geo_domain::{decision, model::NetworkStatus};

use super::dbus_support::{self, Snapshot};

pub struct NetworkManagerGateway;

#[async_trait]
impl NetworkGateway for NetworkManagerGateway {
    async fn status(&self) -> AppResult<NetworkStatus> {
        let snapshot = match dbus_support::connection().await {
            Ok(connection) => dbus_support::snapshot(&connection)
                .await
                .unwrap_or_else(|error| {
                    warn!("cannot read NetworkManager active connections: {error}");
                    Snapshot::default()
                }),
            Err(error) => {
                warn!("cannot connect to system D-Bus: {error}");
                Snapshot::default()
            }
        };
        let default_devices = devices().await;
        let proxy_environment = proxy_env();
        let desktop_proxy = desktop_proxy().await;
        Ok(decision::network_status(
            snapshot.vpn_active,
            snapshot.vpn_connections,
            snapshot.active_connections,
            default_devices,
            proxy_environment,
            desktop_proxy,
        ))
    }

    async fn vpn_active(&self) -> AppResult<bool> {
        Ok(dbus_support::snapshot(&dbus_support::connection().await?)
            .await?
            .vpn_active)
    }
}

async fn devices() -> Vec<String> {
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        Command::new("nmcli")
            .args([
                "-t",
                "-f",
                "DEVICE,TYPE,STATE,CONNECTION",
                "device",
                "status",
            ])
            .output(),
    )
    .await;
    match output {
        Ok(Ok(result)) if result.status.success() => parse_devices(&result),
        _ => Vec::new(),
    }
}

fn parse_devices(output: &Output) -> Vec<String> {
    let mut devices = HashSet::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() >= 4 && fields[2] == "connected" && !fields[0].is_empty() {
            devices.insert(fields[0].to_owned());
        }
    }
    let mut devices: Vec<String> = devices.into_iter().collect();
    devices.sort();
    devices
}

fn proxy_env() -> bool {
    [
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
    ]
    .iter()
    .any(|key| env::var_os(key).is_some_and(|value| !value.is_empty()))
}

async fn desktop_proxy() -> bool {
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        Command::new("gsettings")
            .args(["get", "org.gnome.system.proxy", "mode"])
            .output(),
    )
    .await;
    matches!(output, Ok(Ok(result)) if result.status.success() && !matches!(String::from_utf8_lossy(&result.stdout).trim(), "none" | "'none'" | ""))
}
