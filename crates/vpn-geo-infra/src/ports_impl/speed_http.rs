use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use futures_util::{StreamExt, stream};
use tokio::sync::Semaphore;
use vpn_geo_app::{AppError, AppResult, ports::SpeedGateway};
use vpn_geo_domain::model::SpeedResult;

use super::http_support::client;

pub struct HttpSpeedGateway;

struct Options {
    samples: usize,
    timeout: Duration,
    bytes: u64,
}

struct Measurement {
    latency_ms: Option<f64>,
    bytes: u64,
    mbps: f64,
}

#[async_trait]
impl SpeedGateway for HttpSpeedGateway {
    async fn measure(
        &self,
        targets: Vec<(String, String, Option<f64>)>,
        samples: usize,
        timeout: Duration,
        bytes: u64,
        parallel: usize,
        direct: bool,
    ) -> AppResult<Vec<SpeedResult>> {
        let client = client(timeout, direct)?;
        let semaphore = Arc::new(Semaphore::new(parallel));
        let options = Arc::new(Options {
            samples,
            timeout,
            bytes,
        });
        let collected = stream::iter(targets)
            .map(|(label, url, distance_km)| {
                let client = client.clone();
                let semaphore = semaphore.clone();
                let options = options.clone();
                async move {
                    let _permit = semaphore
                        .acquire()
                        .await
                        .map_err(|error| AppError::Internal(error.to_string()))?;
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
                        Ok(value) => {
                            result.latency_ms = value.latency_ms;
                            result.bytes = value.bytes;
                            result.mbps = value.mbps;
                        }
                        Err(error) => result.error = Some(error.to_string()),
                    }
                    Ok::<_, AppError>(result)
                }
            })
            .buffer_unordered(parallel.max(1))
            .collect::<Vec<AppResult<SpeedResult>>>()
            .await;
        let mut results = collected.into_iter().collect::<AppResult<Vec<_>>>()?;
        results.sort_by(|left, right| {
            right
                .score()
                .total_cmp(&left.score())
                .then_with(|| left.label.cmp(&right.label))
        });
        Ok(results)
    }
}

async fn measure(client: &reqwest::Client, url: &str, options: &Options) -> AppResult<Measurement> {
    let mut total_bytes = 0u64;
    let mut total_duration = Duration::ZERO;
    let mut first_latency = None;
    for _ in 0..options.samples {
        let start = Instant::now();
        let request = client
            .get(url)
            .header("Range", format!("bytes=0-{}", options.bytes - 1))
            .header("Cache-Control", "no-cache")
            .timeout(options.timeout)
            .send();
        let mut response = tokio::time::timeout(options.timeout, request)
            .await
            .map_err(|_| AppError::Timeout("request".into()))?
            .map_err(|error| AppError::http(format!("request failed: {error}")))?;
        if !response.status().is_success() {
            return Err(AppError::http(format!("HTTP status {}", response.status())));
        }
        let latency_ms = start.elapsed().as_secs_f64() * 1000.0;
        if first_latency.is_none() {
            first_latency = Some(latency_ms);
        }
        let mut sample_bytes = 0u64;
        while sample_bytes < options.bytes {
            let chunk = tokio::time::timeout(options.timeout, response.chunk())
                .await
                .map_err(|_| AppError::Timeout("download".into()))?
                .map_err(|error| AppError::http(format!("read response: {error}")))?;
            let Some(chunk) = chunk else { break };
            sample_bytes += (chunk.len() as u64).min(options.bytes - sample_bytes);
        }
        if sample_bytes == 0 {
            return Err(AppError::http("empty response"));
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
