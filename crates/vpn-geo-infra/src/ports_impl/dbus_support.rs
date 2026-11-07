use vpn_geo_app::{AppError, AppResult};
use zbus::{
    Connection, MatchRule, MessageStream,
    message::Type as MessageType,
    zvariant::{OwnedObjectPath, OwnedValue},
};

pub const NM_NAME: &str = "org.freedesktop.NetworkManager";
pub const NM_PATH: &str = "/org/freedesktop/NetworkManager";
pub const ACTIVE_INTERFACE: &str = "org.freedesktop.NetworkManager.Connection.Active";
pub const ACTIVATED: u32 = 2;

#[derive(Debug, Default)]
pub struct Snapshot {
    pub vpn_active: bool,
    pub vpn_connections: Vec<String>,
    pub active_connections: Vec<String>,
}

pub async fn connection() -> AppResult<Connection> {
    Connection::system()
        .await
        .map_err(|error| AppError::nm(format!("connect system D-Bus: {error}")))
}

pub async fn snapshot(connection: &Connection) -> AppResult<Snapshot> {
    let paths = match active_connection_paths(connection).await {
        Ok(paths) => paths,
        Err(property_error) => legacy_active_connection_paths(connection).await.map_err(
            |method_error| {
                AppError::nm(format!(
                    "ActiveConnections property: {property_error}; legacy method: {method_error}"
                ))
            },
        )?,
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
            .map_err(|error| AppError::nm(format!("GetAll {path}: {error}")))?;
        let properties: std::collections::HashMap<String, OwnedValue> = message
            .body()
            .deserialize()
            .map_err(|error| AppError::nm(format!("decode {path} properties: {error}")))?;
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
        Ok::<_, AppError>((vpn, state, id))
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

async fn active_connection_paths(connection: &Connection) -> AppResult<Vec<OwnedObjectPath>> {
    let message = connection
        .call_method(
            Some(NM_NAME),
            NM_PATH,
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &(NM_NAME, "ActiveConnections"),
        )
        .await
        .map_err(|error| AppError::nm(format!("Get: {error}")))?;
    let value: OwnedValue = message
        .body()
        .deserialize()
        .map_err(|error| AppError::nm(format!("decode property wrapper: {error}")))?;
    Vec::<OwnedObjectPath>::try_from(value)
        .map_err(|error| AppError::nm(format!("decode object paths: {error}")))
}

async fn legacy_active_connection_paths(
    connection: &Connection,
) -> AppResult<Vec<OwnedObjectPath>> {
    let message = connection
        .call_method(
            Some(NM_NAME),
            NM_PATH,
            Some(NM_NAME),
            "GetActiveConnections",
            &(),
        )
        .await
        .map_err(|error| AppError::nm(format!("GetActiveConnections: {error}")))?;
    message
        .body()
        .deserialize()
        .map_err(|error| AppError::nm(format!("decode active connections: {error}")))
}

pub fn relevant_signal(message: &zbus::Message) -> bool {
    let header = message.header();
    let (Some(interface), Some(member)) = (header.interface(), header.member()) else {
        return false;
    };
    match interface.as_str() {
        "org.freedesktop.DBus.Properties" => message
            .body()
            .deserialize::<(
                String,
                std::collections::HashMap<String, OwnedValue>,
                Vec<String>,
            )>()
            .is_ok_and(|(name, _, _)| {
                member.as_str() == "PropertiesChanged" && name == ACTIVE_INTERFACE
            }),
        "org.freedesktop.DBus.ObjectManager" => match member.as_str() {
            "InterfacesAdded" => message
                .body()
                .deserialize::<(
                    OwnedObjectPath,
                    std::collections::HashMap<
                        String,
                        std::collections::HashMap<String, OwnedValue>,
                    >,
                )>()
                .is_ok_and(|(_, interfaces)| interfaces.contains_key(ACTIVE_INTERFACE)),
            "InterfacesRemoved" => message
                .body()
                .deserialize::<(OwnedObjectPath, Vec<String>)>()
                .is_ok_and(|(_, interfaces)| {
                    interfaces.iter().any(|name| name == ACTIVE_INTERFACE)
                }),
            _ => false,
        },
        _ => false,
    }
}

pub async fn signal_stream(connection: &Connection) -> AppResult<MessageStream> {
    let rule = MatchRule::builder()
        .msg_type(MessageType::Signal)
        .sender(NM_NAME)
        .map_err(|error| AppError::nm(format!("build match rule: {error}")))?
        .arg0ns(NM_NAME)
        .map_err(|error| AppError::nm(format!("build match rule: {error}")))?
        .build()
        .to_owned();
    MessageStream::for_match_rule(rule, connection, Some(8))
        .await
        .map_err(|error| AppError::nm(format!("subscribe to NetworkManager signals: {error}")))
}
