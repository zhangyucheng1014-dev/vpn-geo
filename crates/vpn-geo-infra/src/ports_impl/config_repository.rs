use std::path::Path;

use async_trait::async_trait;
use vpn_geo_app::{AppResult, ports::ConfigSource};
use vpn_geo_domain::config::Config;

pub struct ConfigRepository;

#[async_trait]
impl ConfigSource for ConfigRepository {
    async fn load(&self, path: &Path) -> AppResult<Config> {
        let text = tokio::fs::read_to_string(path).await.map_err(|error| {
            vpn_geo_app::AppError::io(format!("read {}: {error}", path.display()))
        })?;
        Ok(Config::parse(&text)?)
    }
}
