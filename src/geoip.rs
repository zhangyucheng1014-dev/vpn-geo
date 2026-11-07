use std::{collections::BTreeMap, time::Duration};

use serde::Deserialize;

use crate::{config::Config, http};

#[derive(Clone, Debug, PartialEq)]
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

pub async fn locate(config: &Config, direct: bool) -> Result<Location, String> {
    let client = http::client(config.geoip.timeout, direct)?;
    let responses = futures_util::future::join_all(
        config
            .geoip
            .urls
            .iter()
            .cloned()
            .map(|url| fetch_one(client.clone(), url, config.geoip.timeout)),
    )
    .await;

    let mut votes: BTreeMap<String, Vec<Location>> = BTreeMap::new();
    let mut errors = Vec::new();
    for (index, result) in responses.into_iter().enumerate() {
        match result {
            Ok(location) => votes
                .entry(location.country_code.clone())
                .or_default()
                .push(location),
            Err(error) => errors.push(format!("{}: {error}", config.geoip.urls[index])),
        }
    }
    if votes.is_empty() {
        return Err(format!("all GeoIP sources failed: {}", errors.join("; ")));
    }
    let (_, mut group) = votes
        .into_iter()
        .max_by(|(left_country, left), (right_country, right)| {
            left.len()
                .cmp(&right.len())
                .then_with(|| left[0].source.cmp(&right[0].source))
                .then_with(|| right_country.cmp(left_country))
        })
        .expect("votes are non-empty");
    group.sort_by(|left, right| left.source.cmp(&right.source));
    let best = group.remove(0);
    Ok(Location {
        agreement: group.len() + 1,
        ..best
    })
}

async fn fetch_one(
    client: reqwest::Client,
    url: String,
    timeout: Duration,
) -> Result<Location, String> {
    let response = tokio::time::timeout(timeout, client.get(&url).send())
        .await
        .map_err(|_| "request timed out".to_owned())?
        .map_err(|error| format!("request failed: {error}"))?;
    if !response.status().is_success() {
        return Err(format!("HTTP status {}", response.status()));
    }
    let bytes = tokio::time::timeout(timeout, response.bytes())
        .await
        .map_err(|_| "response timed out".to_owned())?
        .map_err(|error| format!("read response: {error}"))?;
    if bytes.len() > 1024 * 1024 {
        return Err("response is unexpectedly large".to_owned());
    }
    let parsed: GenericResponse =
        serde_json::from_slice(&bytes).map_err(|error| format!("decode JSON response: {error}"))?;
    if parsed.success == Some(false) || parsed.status.as_deref() == Some("fail") {
        return Err(format!(
            "API reported failure: {}",
            parsed.message.unwrap_or_default()
        ));
    }
    let country = parsed
        .country_code
        .or(parsed.country)
        .unwrap_or_default()
        .trim()
        .to_ascii_uppercase();
    if !crate::config::valid_country(&country) {
        return Err(format!("API returned invalid country code {country:?}"));
    }
    let latitude = parsed.latitude.or(parsed.lat).unwrap_or(0.0);
    let longitude = parsed.longitude.or(parsed.lon).unwrap_or(0.0);
    if !latitude.is_finite()
        || !(-90.0..=90.0).contains(&latitude)
        || !longitude.is_finite()
        || !(-180.0..=180.0).contains(&longitude)
    {
        return Err("API returned invalid coordinates".to_owned());
    }
    Ok(Location {
        country_code: country,
        latitude,
        longitude,
        source: url,
        agreement: 1,
    })
}
