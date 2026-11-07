use std::time::Duration;

use tokio::process::Command;

use crate::{Config, Error, Result, config::Node};

const MIN_PRIORITY: i64 = i32::MIN as i64;
const MAX_PRIORITY: i64 = i32::MAX as i64;

pub struct Manager<'a> {
    config: &'a Config,
}

impl<'a> Manager<'a> {
    pub fn new(config: &'a Config) -> Self {
        Self { config }
    }

    pub fn find(&self, country: &str) -> Option<&Node> {
        let country = country.trim().to_ascii_uppercase();
        self.config
            .nodes
            .iter()
            .find(|node| node.country == country)
    }

    pub async fn promote_country(&self, country: &str) -> Result<bool> {
        match self.find(country) {
            Some(node) => self.promote_node(node).await,
            None => Ok(false),
        }
    }

    pub async fn promote_node(&self, target: &Node) -> Result<bool> {
        if !self
            .config
            .nodes
            .iter()
            .any(|node| node.uuid == target.uuid)
        {
            return Err(Error::InvalidArgument(format!(
                "node {:?} is not configured",
                target.uuid
            )));
        }
        let reads = self.config.nodes.iter().map(|node| async move {
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
            return Err(Error::Internal(
                "priority ceiling reached; refusing to normalize unrelated profile order"
                    .to_owned(),
            ));
        }
        set_priority(&target.uuid, maximum + 1).await
    }
}

async fn read_priority(uuid: &str) -> Result<i64> {
    let output = command_output(Command::new("nmcli").args([
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
        .map_err(|_| Error::nm(format!("cannot parse priority {first:?} for {uuid}")))?;
    if !(MIN_PRIORITY..=MAX_PRIORITY).contains(&value) {
        return Err(Error::nm(format!(
            "priority {value} is outside NetworkManager's range"
        )));
    }
    Ok(value)
}

async fn set_priority(uuid: &str, value: i64) -> Result<bool> {
    let value = value.to_string();
    command_output(Command::new("nmcli").args([
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

async fn command_output(command: &mut Command) -> Result<String> {
    let output = tokio::time::timeout(Duration::from_secs(10), command.output())
        .await
        .map_err(|_| Error::Timeout("nmcli".to_owned()))?
        .map_err(|error| Error::nm(format!("run nmcli: {error}")))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        return Err(Error::nm(format!(
            "nmcli failed: {}",
            if detail.is_empty() { stdout } else { detail }
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
