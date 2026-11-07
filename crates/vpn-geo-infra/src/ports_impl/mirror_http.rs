use std::collections::{BTreeMap, HashSet};

use async_trait::async_trait;
use serde::Deserialize;
use vpn_geo_app::{AppError, AppResult, ports::MirrorGateway};
use vpn_geo_domain::model::{Mirror, MirrorKind, Region, SyncSummary};

use super::{http_support::client, mirrorlist};

const STATUS_URL: &str = "https://archlinux.org/mirrors/status/json/";

pub struct HttpMirrorGateway;

#[derive(Debug, Deserialize)]
struct OfficialStatus {
    urls: Vec<OfficialMirror>,
}

#[derive(Debug, Deserialize)]
struct OfficialMirror {
    url: String,
    protocol: String,
    country_code: String,
    active: bool,
    completion_pct: Option<f64>,
    score: Option<f64>,
    delay: Option<u64>,
}

#[async_trait]
impl MirrorGateway for HttpMirrorGateway {
    async fn sync_official(
        &self,
        regions: &[Region],
        per_region: usize,
    ) -> AppResult<(Vec<Mirror>, SyncSummary)> {
        if per_region == 0 || per_region > 20 {
            return Err(AppError::Domain(vpn_geo_domain::DomainError::invalid(
                "per_region must be between 1 and 20",
            )));
        }
        let client = client(std::time::Duration::from_secs(20), false)?;
        let response = client
            .get(STATUS_URL)
            .timeout(std::time::Duration::from_secs(20))
            .send()
            .await
            .map_err(|error| AppError::http(format!("official mirror status: {error}")))?;
        if !response.status().is_success() {
            return Err(AppError::http(format!(
                "official mirror status HTTP {}",
                response.status()
            )));
        }
        let status: OfficialStatus = response
            .json()
            .await
            .map_err(|error| AppError::http(format!("decode official mirror status: {error}")))?;
        let mut selected = Vec::new();
        let mut seen_hosts = HashSet::new();
        let mut candidates = 0usize;
        let mut counts = BTreeMap::new();
        for region in regions {
            let Some(code) = region.official_code() else {
                counts.insert(region.as_str().to_owned(), 0);
                continue;
            };
            let mut items: Vec<&OfficialMirror> = status
                .urls
                .iter()
                .filter(|item| {
                    item.active
                        && item.protocol == "https"
                        && item.country_code.eq_ignore_ascii_case(code)
                })
                .filter(|item| item.completion_pct.unwrap_or_default() >= 0.95)
                .filter(|item| item.score.is_some_and(|score| score <= 20.0))
                .filter(|item| item.delay.is_some_and(|delay| delay <= 30000))
                .collect();
            items.sort_by(|left, right| {
                left.score
                    .unwrap_or(f64::MAX)
                    .total_cmp(&right.score.unwrap_or(f64::MAX))
                    .then_with(|| {
                        left.delay
                            .unwrap_or(u64::MAX)
                            .cmp(&right.delay.unwrap_or(u64::MAX))
                    })
                    .then_with(|| left.url.cmp(&right.url))
            });
            candidates += items.len();
            let mut chosen = Vec::new();
            for item in items {
                let host = item
                    .url
                    .split('/')
                    .nth(2)
                    .unwrap_or("official")
                    .to_ascii_lowercase();
                if seen_hosts.insert(host) {
                    chosen.push(item);
                }
                if chosen.len() == per_region {
                    break;
                }
            }
            counts.insert(region.as_str().to_owned(), chosen.len());
            for (index, item) in chosen.into_iter().enumerate() {
                let base = item.url.trim_end_matches('/');
                let host = item
                    .url
                    .split('/')
                    .nth(2)
                    .unwrap_or("official")
                    .replace('.', "-");
                selected.push(Mirror {
                    name: format!("{}-{host}", region.as_str()),
                    kind: MirrorKind::Pacman,
                    region: *region,
                    url: format!("{base}/$repo/os/$arch"),
                    test_url: format!("{base}/core/os/x86_64/core.db.tar.gz"),
                    priority: (per_region - index) as i32 * 10,
                    enabled: true,
                });
            }
        }
        let total = selected.len();
        Ok((
            selected,
            SyncSummary {
                total_official: status.urls.len(),
                candidates,
                selected: total,
                regions: counts,
            },
        ))
    }

    async fn apply(&self, mirror: &Mirror) -> AppResult<String> {
        mirrorlist::apply(mirror).await
    }
}
