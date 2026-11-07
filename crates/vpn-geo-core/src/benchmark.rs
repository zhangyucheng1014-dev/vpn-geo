use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use futures_util::{StreamExt, stream};
use serde::Serialize;
use tokio::sync::Semaphore;

use crate::{Config, Error, Result, config::Node, geoip::Location, http};

#[derive(Clone, Debug, Serialize)]
pub struct Options {
    pub samples: usize,
    pub timeout: Duration,
    pub download_bytes: u64,
    pub parallel: usize,
    pub direct: bool,
}

impl Options {
    pub fn from_config(config: &Config, direct: bool) -> Self {
        Self {
            samples: config.benchmark.samples,
            timeout: config.benchmark.request_timeout,
            download_bytes: config.benchmark.download_bytes,
            parallel: config.benchmark.parallel,
            direct,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct SpeedResult {
    pub label: String,
    pub url: String,
    pub distance_km: Option<f64>,
    pub latency_ms: Option<f64>,
    pub bytes: u64,
    pub mbps: f64,
    pub error: Option<String>,
}

impl SpeedResult {
    pub fn score(&self) -> f64 {
        if self.error.is_some() {
            return f64::NEG_INFINITY;
        }
        let latency = self.latency_ms.unwrap_or(1000.0).max(1.0);
        self.mbps / latency.ln_1p()
    }
}

pub fn nearest(nodes: &[Node], location: &Location, count: usize) -> Vec<(Node, f64)> {
    let mut items: Vec<(Node, f64)> = nodes
        .iter()
        .filter(|node| node.test_url.is_some() && (node.latitude != 0.0 || node.longitude != 0.0))
        .map(|node| {
            (
                node.clone(),
                haversine(
                    location.latitude,
                    location.longitude,
                    node.latitude,
                    node.longitude,
                ),
            )
        })
        .collect();
    items.sort_by(|left, right| left.1.total_cmp(&right.1));
    let mut seen = std::collections::HashSet::new();
    items
        .into_iter()
        .filter(|(node, _)| seen.insert(node.country.clone()))
        .take(count)
        .collect()
}

pub async fn run(
    targets: Vec<(String, String, Option<f64>)>,
    options: Options,
) -> Result<Vec<SpeedResult>> {
    let client = http::client(options.timeout, options.direct)?;
    let semaphore = Arc::new(Semaphore::new(options.parallel));
    let options = Arc::new(options);
    let collected = stream::iter(targets)
        .map(|(label, url, distance_km)| {
            let client = client.clone();
            let semaphore = semaphore.clone();
            let options = options.clone();
            async move {
                let _permit = semaphore
                    .acquire()
                    .await
                    .map_err(|error| Error::Internal(error.to_string()))?;
                let mut result = SpeedResult {
                    label,
                    url: url.clone(),
                    distance_km,
                    latency_ms: None,
                    bytes: 0,
                    mbps: 0.0,
                    error: None,
                };
                match measure(&client, &url, &options).await {
                    Ok(measurement) => {
                        result.latency_ms = measurement.latency_ms;
                        result.bytes = measurement.bytes;
                        result.mbps = measurement.mbps;
                    }
                    Err(error) => result.error = Some(error.to_string()),
                }
                Ok::<_, Error>(result)
            }
        })
        .buffer_unordered(options.parallel.max(1))
        .collect::<Vec<Result<SpeedResult>>>()
        .await;
    let mut results = collected.into_iter().collect::<Result<Vec<_>>>()?;
    results.sort_by(|left, right| {
        right
            .score()
            .total_cmp(&left.score())
            .then_with(|| left.label.cmp(&right.label))
    });
    Ok(results)
}

struct Measurement {
    latency_ms: Option<f64>,
    bytes: u64,
    mbps: f64,
}

async fn measure(client: &reqwest::Client, url: &str, options: &Options) -> Result<Measurement> {
    let mut total_bytes = 0u64;
    let mut total_duration = Duration::ZERO;
    let mut first_latency = None;
    for _ in 0..options.samples {
        let start = Instant::now();
        let request = client
            .get(url)
            .header("Range", format!("bytes=0-{}", options.download_bytes - 1))
            .header("Cache-Control", "no-cache")
            .timeout(options.timeout)
            .send();
        let mut response = tokio::time::timeout(options.timeout, request)
            .await
            .map_err(|_| Error::Timeout("request".to_owned()))?
            .map_err(|error| Error::http(format!("request failed: {error}")))?;
        if !response.status().is_success() {
            return Err(Error::http(format!("HTTP status {}", response.status())));
        }
        let latency_ms = start.elapsed().as_secs_f64() * 1000.0;
        if first_latency.is_none() {
            first_latency = Some(latency_ms);
        }
        let mut sample_bytes = 0u64;
        while sample_bytes < options.download_bytes {
            let chunk = tokio::time::timeout(options.timeout, response.chunk())
                .await
                .map_err(|_| Error::Timeout("download".to_owned()))?
                .map_err(|error| Error::http(format!("read response: {error}")))?;
            let Some(chunk) = chunk else { break };
            sample_bytes += (chunk.len() as u64).min(options.download_bytes - sample_bytes);
        }
        if sample_bytes == 0 {
            return Err(Error::http("empty response"));
        }
        total_bytes += sample_bytes;
        total_duration += start.elapsed();
    }
    Ok(Measurement {
        latency_ms: first_latency,
        bytes: total_bytes,
        mbps: if total_duration.is_zero() {
            0.0
        } else {
            total_bytes as f64 * 8.0 / 1_000_000.0 / total_duration.as_secs_f64()
        },
    })
}

fn haversine(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let delta_lat = (lat2 - lat1).to_radians();
    let delta_lon = (lon2 - lon1).to_radians();
    let a = (delta_lat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (delta_lon / 2.0).sin().powi(2);
    6371.0 * 2.0 * a.sqrt().atan2((1.0 - a).sqrt())
}

pub fn print_table(results: &[SpeedResult], show_distance: bool) {
    if show_distance {
        println!(
            "{:<24} {:>10} {:>12} {:>14}  STATUS",
            "TARGET", "DISTANCE", "LATENCY", "THROUGHPUT"
        );
    } else {
        println!(
            "{:<24} {:>12} {:>14}  STATUS",
            "TARGET", "LATENCY", "THROUGHPUT"
        );
    }
    for result in results {
        let latency = result
            .latency_ms
            .map(|value| format!("{value:.1} ms"))
            .unwrap_or_else(|| "-".to_owned());
        let throughput = if result.error.is_none() {
            format!("{:.2} Mbps", result.mbps)
        } else {
            "-".to_owned()
        };
        let status = result.error.as_deref().unwrap_or("ok");
        if show_distance {
            let distance = result
                .distance_km
                .map(|value| format!("{value:.0} km"))
                .unwrap_or_else(|| "-".to_owned());
            println!(
                "{:<24} {:>10} {:>12} {:>14}  {status}",
                result.label, distance, latency, throughput
            );
        } else {
            println!(
                "{:<24} {:>12} {:>14}  {status}",
                result.label, latency, throughput
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_selects_one_node_per_country() {
        let nodes = vec![
            Node {
                name: "jp1".into(),
                uuid: "1".into(),
                country: "JP".into(),
                latitude: 35.0,
                longitude: 139.0,
                test_url: Some("https://example.test/a".into()),
            },
            Node {
                name: "jp2".into(),
                uuid: "2".into(),
                country: "JP".into(),
                latitude: 36.0,
                longitude: 140.0,
                test_url: Some("https://example.test/b".into()),
            },
            Node {
                name: "us".into(),
                uuid: "3".into(),
                country: "US".into(),
                latitude: 40.0,
                longitude: -74.0,
                test_url: Some("https://example.test/c".into()),
            },
            Node {
                name: "no-speed".into(),
                uuid: "4".into(),
                country: "KR".into(),
                latitude: 0.0,
                longitude: 0.0,
                test_url: None,
            },
        ];
        let location = Location {
            country_code: "JP".into(),
            latitude: 35.0,
            longitude: 139.0,
            source: "test".into(),
            agreement: 1,
        };
        let result = nearest(&nodes, &location, 2);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0.name, "jp1");
        assert_eq!(result[1].0.country, "US");
    }
}
