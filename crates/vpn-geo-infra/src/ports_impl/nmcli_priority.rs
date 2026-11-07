use std::time::Duration;

use async_trait::async_trait;
use tokio::process::Command;
use vpn_geo_app::{AppError, AppResult, ports::PriorityGateway};
use vpn_geo_domain::config::Config;

const MIN_PRIORITY: i64 = i32::MIN as i64;
const MAX_PRIORITY: i64 = i32::MAX as i64;

pub struct NmcliPriorityGateway;

#[async_trait]
impl PriorityGateway for NmcliPriorityGateway {
    async fn promote_country(&self, config: &Config, country: &str) -> AppResult<bool> {
        let target = config
            .nodes
            .iter()
            .find(|node| node.country.eq_ignore_ascii_case(country));
        match target {
            Some(node) => self.promote_node(config, &node.name).await,
            None => Ok(false),
        }
    }

    async fn promote_node(&self, config: &Config, name: &str) -> AppResult<bool> {
        let target = config
            .nodes
            .iter()
            .find(|node| node.name == name)
            .ok_or_else(|| {
                AppError::Domain(vpn_geo_domain::DomainError::invalid(format!(
                    "node {name:?} is not configured"
                )))
            })?;
        let reads = config.nodes.iter().map(|node| async move {
            read_priority(&node.uuid)
                .await
                .map(|value| (node.uuid.clone(), value))
        });
        let values: Vec<(String, i64)> = futures_util::future::try_join_all(reads).await?;
        let maximum = values.iter().map(|(_, value)| *value).max().unwrap_or(0);
        let shared = values
            .iter()
            .any(|(uuid, value)| uuid != &target.uuid && *value == maximum);
        let target_priority = values
            .iter()
            .find(|(uuid, _)| uuid == &target.uuid)
            .map(|(_, value)| *value)
            .unwrap_or(0);
        if target_priority == maximum && !shared {
            return Ok(false);
        }
        if maximum == MAX_PRIORITY {
            return Err(AppError::Internal(
                "priority ceiling reached; refusing to normalize unrelated profile order".into(),
            ));
        }
        set_priority(&target.uuid, maximum + 1).await
    }
}

async fn read_priority(uuid: &str) -> AppResult<i64> {
    let output = command(Command::new("nmcli").args([
        "-g",
        "connection.autoconnect-priority",
        "connection",
        "show",
        "uuid",
        uuid,
    ]))
    .await?;
    let first = output.lines().next().unwrap_or("").trim();
    if first.is_empty() || first == "--" {
        return Ok(0);
    }
    let value: i64 = first
        .parse()
        .map_err(|_| AppError::nm(format!("cannot parse priority {first:?} for {uuid}")))?;
    if !(MIN_PRIORITY..=MAX_PRIORITY).contains(&value) {
        return Err(AppError::nm(format!(
            "priority {value} is outside NetworkManager's range"
        )));
    }
    Ok(value)
}

async fn set_priority(uuid: &str, value: i64) -> AppResult<bool> {
    let value = value.to_string();
    command(Command::new("nmcli").args([
        "--wait",
        "10",
        "connection",
        "modify",
        "uuid",
        uuid,
        "connection.autoconnect-priority",
        &value,
    ]))
    .await?;
    Ok(true)
}

async fn command(command: &mut Command) -> AppResult<String> {
    let output = tokio::time::timeout(Duration::from_secs(10), command.output())
        .await
        .map_err(|_| AppError::Timeout("nmcli".into()))?
        .map_err(|error| AppError::nm(format!("run nmcli: {error}")))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        return Err(AppError::nm(format!(
            "nmcli failed: {}",
            if detail.is_empty() { stdout } else { detail }
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
