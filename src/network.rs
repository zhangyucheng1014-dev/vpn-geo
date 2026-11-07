use std::{collections::HashSet, env, process::Output};

use log::warn;
use serde::Serialize;
use tokio::process::Command;
use zbus::{
    message::Type as MessageType,
    zvariant::{OwnedObjectPath, OwnedValue},
    Connection, MatchRule, MessageStream,
};

const NM_NAME: &str = "org.freedesktop.NetworkManager";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";
const ACTIVE_INTERFACE: &str = "org.freedesktop.NetworkManager.Connection.Active";
const ACTIVATED: u32 = 2;

#[derive(Clone, Debug, Serialize)]
pub struct NetworkStatus {
    pub vpn_active: bool,
    pub vpn_connections: Vec<String>,
    pub active_connections: Vec<String>,
    pub default_devices: Vec<String>,
    pub system_proxy: bool,
    pub proxy_environment: bool,
    pub route: String,
    pub confidence: String,
}

pub async fn status() -> Result<NetworkStatus, String> {
    let snapshot = match Connection::system().await {
        Ok(connection) => snapshot(&connection).await.unwrap_or_else(|error| {
            warn!("cannot read NetworkManager active connections: {error}");
            Snapshot::default()
        }),
        Err(error) => {
            warn!("cannot connect to system D-Bus: {error}");
            Snapshot::default()
        }
    };
    let nmcli = tokio::time::timeout(
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
    let default_devices = match nmcli {
        Ok(Ok(output)) if output.status.success() => parse_default_devices(&output),
        _ => Vec::new(),
    };
    let proxy_environment = proxy_environment_present();
    let system_proxy = proxy_environment || detect_desktop_proxy().await;
    let (route, confidence) = classify(&snapshot, system_proxy);
    Ok(NetworkStatus {
        vpn_active: snapshot.vpn_active,
        vpn_connections: snapshot.vpn_connections,
        active_connections: snapshot.active_connections,
        default_devices,
        system_proxy,
        proxy_environment,
        route,
        confidence,
    })
}

fn classify(snapshot: &Snapshot, system_proxy: bool) -> (String, String) {
    if snapshot.vpn_active {
        if system_proxy {
            ("VPN + system proxy".to_owned(), "high".to_owned())
        } else {
            ("VPN".to_owned(), "high".to_owned())
        }
    } else if system_proxy {
        ("System proxy".to_owned(), "medium".to_owned())
    } else if snapshot.active_connections.is_empty() {
        (
            "Unknown (no active NetworkManager connection was observed)".to_owned(),
            "low".to_owned(),
        )
    } else {
        (
            "Direct (no VPN or system proxy detected)".to_owned(),
            "medium".to_owned(),
        )
    }
}

fn proxy_environment_present() -> bool {
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

async fn detect_desktop_proxy() -> bool {
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        Command::new("gsettings")
            .args(["get", "org.gnome.system.proxy", "mode"])
            .output(),
    )
    .await;
    match output {
        Ok(Ok(result)) if result.status.success() => !matches!(
            String::from_utf8_lossy(&result.stdout).trim(),
            "none" | "'none'" | ""
        ),
        _ => false,
    }
}

fn parse_default_devices(output: &Output) -> Vec<String> {
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

#[derive(Debug, Default)]
struct Snapshot {
    vpn_active: bool,
    vpn_connections: Vec<String>,
    active_connections: Vec<String>,
}

pub async fn snapshot_vpn(connection: &Connection) -> Result<bool, String> {
    Ok(snapshot(connection).await?.vpn_active)
}

async fn snapshot(connection: &Connection) -> Result<Snapshot, String> {
    let paths = match active_connection_paths(connection).await {
        Ok(paths) => paths,
        Err(property_error) => match legacy_active_connection_paths(connection).await {
            Ok(paths) => paths,
            Err(method_error) => {
                return Err(format!(
                    "ActiveConnections property: {property_error}; legacy method: {method_error}"
                ))
            }
        },
    };
    let calls = paths.into_iter().map(|path| async move {
        let message = connection
            .call_method(
                Some(NM_NAME),
                path.as_ref(),
                Some("org.freedesktop.DBus.Properties"),
                "GetAll",
                &(ACTIVE_INTERFACE,),
            )
            .await
            .map_err(|error| format!("GetAll {path}: {error}"))?;
        let properties: std::collections::HashMap<String, OwnedValue> = message
            .body()
            .deserialize()
            .map_err(|error| format!("decode {path} properties: {error}"))?;
        let vpn = properties
            .get("Vpn")
            .and_then(|value| bool::try_from(value).ok())
            .unwrap_or(false);
        let state = properties
            .get("State")
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(0);
        let id = properties
            .get("Id")
            .and_then(|value| <&str>::try_from(value).ok().map(str::to_owned))
            .unwrap_or_else(|| path.to_string());
        Ok::<_, String>((vpn, state, id))
    });
    let mut result = Snapshot::default();
    for item in futures_util::future::join_all(calls).await {
        let (vpn, state, id) = item?;
        if state != ACTIVATED {
            continue;
        }
        result.active_connections.push(id.clone());
        if vpn {
            result.vpn_active = true;
            result.vpn_connections.push(id);
        }
    }
    result.active_connections.sort();
    result.vpn_connections.sort();
    Ok(result)
}

async fn active_connection_paths(connection: &Connection) -> Result<Vec<OwnedObjectPath>, String> {
    let message = connection
        .call_method(
            Some(NM_NAME),
            NM_PATH,
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &(NM_NAME, "ActiveConnections"),
        )
        .await
        .map_err(|error| format!("Get: {error}"))?;
    let value: OwnedValue = message
        .body()
        .deserialize()
        .map_err(|error| format!("decode property wrapper: {error}"))?;
    Vec::<OwnedObjectPath>::try_from(value).map_err(|error| format!("decode object paths: {error}"))
}

async fn legacy_active_connection_paths(
    connection: &Connection,
) -> Result<Vec<OwnedObjectPath>, String> {
    let message = connection
        .call_method(
            Some(NM_NAME),
            NM_PATH,
            Some(NM_NAME),
            "GetActiveConnections",
            &(),
        )
        .await
        .map_err(|error| format!("GetActiveConnections: {error}"))?;
    message
        .body()
        .deserialize()
        .map_err(|error| format!("decode active connections: {error}"))
}

pub fn relevant_signal(message: &zbus::Message) -> bool {
    let header = message.header();
    let Some(interface) = header.interface() else {
        return false;
    };
    let Some(member) = header.member() else {
        return false;
    };
    match interface.as_str() {
        "org.freedesktop.DBus.Properties" => {
            if member.as_str() != "PropertiesChanged" {
                return false;
            }
            let Ok((name, _, _)) = message.body().deserialize::<(
                String,
                std::collections::HashMap<String, OwnedValue>,
                Vec<String>,
            )>() else {
                return false;
            };
            name == ACTIVE_INTERFACE
        }
        "org.freedesktop.DBus.ObjectManager" => {
            if member.as_str() == "InterfacesAdded" {
                let Ok((_, interfaces)) = message.body().deserialize::<(
                    OwnedObjectPath,
                    std::collections::HashMap<
                        String,
                        std::collections::HashMap<String, OwnedValue>,
                    >,
                )>() else {
                    return false;
                };
                interfaces.contains_key(ACTIVE_INTERFACE)
            } else if member.as_str() == "InterfacesRemoved" {
                let Ok((_, interfaces)) = message
                    .body()
                    .deserialize::<(OwnedObjectPath, Vec<String>)>()
                else {
                    return false;
                };
                interfaces.iter().any(|name| name == ACTIVE_INTERFACE)
            } else {
                false
            }
        }
        _ => false,
    }
}

pub async fn signal_stream(connection: &Connection) -> Result<MessageStream, String> {
    let rule = MatchRule::builder()
        .msg_type(MessageType::Signal)
        .sender(NM_NAME)
        .map_err(|error| format!("build D-Bus match rule: {error}"))?
        .arg0ns(NM_NAME)
        .map_err(|error| format!("build D-Bus match rule: {error}"))?
        .build()
        .to_owned();
    MessageStream::for_match_rule(rule, connection, Some(8))
        .await
        .map_err(|error| format!("subscribe to NetworkManager signals: {error}"))
}

pub fn print_human(status: &NetworkStatus) {
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
    if !status.vpn_connections.is_empty() {
        println!("VPN profiles:   {}", status.vpn_connections.join(", "));
    }
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
    if !status.default_devices.is_empty() {
        println!("Devices:        {}", status.default_devices.join(", "));
    }
    println!(
        "Active links:   {}",
        if status.active_connections.is_empty() {
            "none".to_owned()
        } else {
            status.active_connections.join(", ")
        }
    );
    println!("Note:           HTTP proxy clients can route outside NetworkManager VPN detection.");
}
