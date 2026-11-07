use async_trait::async_trait;
use serde::Deserialize;
use vpn_geo_app::{AppError, AppResult, ports::GeoIpGateway};
use vpn_geo_domain::{
    config::{Config, valid_country},
    model::Location,
};

use super::http_support::client;

pub struct HttpGeoIpGateway;

#[derive(Debug, Deserialize)]
struct GenericResponse {
    #[serde(default)]
    success: Option<bool>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    country_code: Option<String>,
    #[serde(default, alias = "countryCode", alias = "country")]
    country: Option<String>,
    #[serde(default)]
    latitude: Option<f64>,
    #[serde(default, alias = "lat")]
    lat: Option<f64>,
    #[serde(default)]
    longitude: Option<f64>,
    #[serde(default, alias = "lon")]
    lon: Option<f64>,
    #[serde(default)]
    message: Option<String>,
}

#[async_trait]
impl GeoIpGateway for HttpGeoIpGateway {
    async fn votes(&self, config: &Config, direct: bool) -> AppResult<Vec<Location>> {
        let client = client(config.geoip.timeout, direct)?;
        let requests = config.geoip.urls.iter().cloned().map(|url| {
            let client = client.clone();
            async move { fetch_one(client, url, config.geoip.timeout).await.ok() }
        });
        Ok(futures_util::future::join_all(requests)
            .await
            .into_iter()
            .flatten()
            .collect())
    }
}

async fn fetch_one(
    client: reqwest::Client,
    url: String,
    timeout: std::time::Duration,
) -> AppResult<Location> {
    let response = client
        .get(&url)
        .timeout(timeout)
        .send()
        .await
        .map_err(|error| AppError::http(format!("request failed: {error}")))?;
    if !response.status().is_success() {
        return Err(AppError::http(format!("HTTP status {}", response.status())));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| AppError::http(format!("read response: {error}")))?;
    if bytes.len() > 1024 * 1024 {
        return Err(AppError::http("response is unexpectedly large"));
    }
    let parsed: GenericResponse = serde_json::from_slice(&bytes)
        .map_err(|error| AppError::http(format!("decode JSON response: {error}")))?;
    if parsed.success == Some(false) || parsed.status.as_deref() == Some("fail") {
        return Err(AppError::http(format!(
            "API reported failure: {}",
            parsed.message.unwrap_or_default()
        )));
    }
    let country = parsed
        .country_code
        .or(parsed.country)
        .unwrap_or_default()
        .trim()
        .to_ascii_uppercase();
    if !valid_country(&country) {
        return Err(AppError::http(format!(
            "API returned invalid country code {country:?}"
        )));
    }
    let latitude = parsed.latitude.or(parsed.lat).unwrap_or(0.0);
    let longitude = parsed.longitude.or(parsed.lon).unwrap_or(0.0);
    if !latitude.is_finite()
        || !(-90.0..=90.0).contains(&latitude)
        || !longitude.is_finite()
        || !(-180.0..=180.0).contains(&longitude)
    {
        return Err(AppError::http("API returned invalid coordinates"));
    }
    Ok(Location {
        country_code: country,
        latitude,
        longitude,
        source: url,
        agreement: 1,
    })
}
