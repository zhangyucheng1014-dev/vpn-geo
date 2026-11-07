use std::collections::BTreeMap;

use serde::Deserialize;

use crate::{Config, Error, Result, config::valid_country, http};

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Location {
    pub country_code: String,
    pub latitude: f64,
    pub longitude: f64,
    pub source: String,
    pub agreement: usize,
}

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

pub async fn locate(config: &Config, direct: bool) -> Result<Location> {
    let client = http::client(config.geoip.timeout, direct)?;
    let requests = config
        .geoip
        .urls
        .iter()
        .cloned()
        .map(|url| fetch_one(client.clone(), url, config.geoip.timeout));
    let responses = futures_util::future::join_all(requests).await;
    let mut errors = Vec::new();
    let mut votes: BTreeMap<String, Vec<Location>> = BTreeMap::new();
    for (index, response) in responses.into_iter().enumerate() {
        match response {
            Ok(location) => votes
                .entry(location.country_code.clone())
                .or_default()
                .push(location),
            Err(error) => errors.push(format!("{}: {error}", config.geoip.urls[index])),
        }
    }
    if votes.is_empty() {
        return Err(Error::http(format!(
            "all GeoIP sources failed: {}",
            errors.join("; ")
        )));
    }
    let (_, mut group) = votes
        .into_iter()
        .max_by(|(left_country, left), (right_country, right)| {
            left.len()
                .cmp(&right.len())
                .then_with(|| left[0].source.cmp(&right[0].source))
                .then_with(|| right_country.cmp(left_country))
        })
        .ok_or_else(|| Error::http("no GeoIP consensus"))?;
    group.sort_by(|left, right| left.source.cmp(&right.source));
    let agreement = group.len();
    if agreement < config.geoip.min_agreement {
        return Err(Error::http(format!(
            "GeoIP consensus is too weak: {agreement} < {}",
            config.geoip.min_agreement
        )));
    }
    let best = group.remove(0);
    Ok(Location { agreement, ..best })
}

async fn fetch_one(
    client: reqwest::Client,
    url: String,
    timeout: std::time::Duration,
) -> Result<Location> {
    let response = client
        .get(&url)
        .timeout(timeout)
        .send()
        .await
        .map_err(|error| Error::http(format!("request failed: {error}")))?;
    if !response.status().is_success() {
        return Err(Error::http(format!("HTTP status {}", response.status())));
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| Error::http(format!("read response: {error}")))?;
    if bytes.len() > 1024 * 1024 {
        return Err(Error::http("response is unexpectedly large"));
    }
    let parsed: GenericResponse = serde_json::from_slice(&bytes)
        .map_err(|error| Error::http(format!("decode JSON response: {error}")))?;
    if parsed.success == Some(false) || parsed.status.as_deref() == Some("fail") {
        return Err(Error::http(format!(
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
        return Err(Error::http(format!(
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
        return Err(Error::http("API returned invalid coordinates"));
    }
    Ok(Location {
        country_code: country,
        latitude,
        longitude,
        source: url,
        agreement: 1,
    })
}
