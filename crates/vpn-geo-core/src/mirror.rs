use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process,
    time::Duration,
};

use serde::Deserialize;
use tokio::{fs, io::AsyncWriteExt, process::Command};

use crate::{
    Config, Error, Result,
    benchmark::{self, Options, SpeedResult},
    config::{Mirror, MirrorKind, MirrorRegion},
    http,
};

const MARKER_START: &str = "# BEGIN vpn-geo managed mirrors";
const MARKER_END: &str = "# END vpn-geo managed mirrors";
const STATUS_URL: &str = "https://archlinux.org/mirrors/status/json/";

#[derive(Clone, Debug)]
pub struct Catalog {
    mirrors: Vec<Mirror>,
    built_in: bool,
}

impl Catalog {
    pub fn from_config(config: &Config) -> Self {
        let mirrors: Vec<Mirror> = config
            .mirrors
            .iter()
            .filter(|mirror| mirror.enabled)
            .cloned()
            .collect();
        if mirrors.is_empty() {
            Self {
                mirrors: crate::config::default_mirrors(),
                built_in: true,
            }
        } else {
            Self {
                mirrors,
                built_in: false,
            }
        }
    }

    pub fn is_built_in(&self) -> bool {
        self.built_in
    }

    pub fn len(&self) -> usize {
        self.mirrors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.mirrors.is_empty()
    }

    pub fn all(&self) -> &[Mirror] {
        &self.mirrors
    }

    pub fn region(&self, region: Option<MirrorRegion>) -> Vec<Mirror> {
        let mut mirrors: Vec<Mirror> = self
            .mirrors
            .iter()
            .filter(|mirror| region.is_none_or(|wanted| mirror.region == wanted))
            .cloned()
            .collect();
        mirrors.sort_by(|left, right| {
            right
                .priority
                .cmp(&left.priority)
                .then_with(|| left.name.cmp(&right.name))
        });
        mirrors
    }

    pub fn find(&self, name: &str) -> Option<Mirror> {
        self.mirrors
            .iter()
            .find(|mirror| mirror.name == name)
            .cloned()
    }
}

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

#[derive(Clone, Debug, serde::Serialize)]
pub struct SyncSummary {
    pub total_official: usize,
    pub candidates: usize,
    pub selected: usize,
    pub regions: HashMap<String, usize>,
}

pub async fn sync_official(
    config: &Config,
    regions: &[MirrorRegion],
    limit_per_region: usize,
) -> Result<(Vec<Mirror>, SyncSummary)> {
    if limit_per_region == 0 || limit_per_region > 20 {
        return Err(Error::InvalidArgument(
            "limit_per_region must be between 1 and 20".to_owned(),
        ));
    }
    let client = http::client(Duration::from_secs(20), false)?;
    let response = client
        .get(STATUS_URL)
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|error| Error::http(format!("official mirror status: {error}")))?;
    if !response.status().is_success() {
        return Err(Error::http(format!(
            "official mirror status HTTP {}",
            response.status()
        )));
    }
    let status: OfficialStatus = response
        .json()
        .await
        .map_err(|error| Error::http(format!("decode official mirror status: {error}")))?;
    let mut selected = Vec::new();
    let mut seen_hosts = std::collections::HashSet::new();
    let mut regions_count = HashMap::new();
    let mut candidates = 0usize;
    for region in regions {
        let code = region_code(*region);
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
            if chosen.len() == limit_per_region {
                break;
            }
        }
        regions_count.insert(region.as_str().to_owned(), chosen.len());
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
                priority: (limit_per_region - index) as i32 * 10,
                enabled: true,
            });
        }
    }
    let selected_count = selected.len();
    let _ = config;
    Ok((
        selected,
        SyncSummary {
            total_official: status.urls.len(),
            candidates,
            selected: selected_count,
            regions: regions_count,
        },
    ))
}

fn region_code(region: MirrorRegion) -> &'static str {
    match region {
        MirrorRegion::Cn => "CN",
        MirrorRegion::Hk => "HK",
        MirrorRegion::Jp => "JP",
        MirrorRegion::Us => "US",
        MirrorRegion::Global => "GLOBAL",
    }
}

pub async fn speed(config: &Config, mirrors: &[Mirror], direct: bool) -> Result<Vec<SpeedResult>> {
    if mirrors.is_empty() {
        return Err(Error::NotFound(
            "no mirrors match the selected region".to_owned(),
        ));
    }
    let targets = mirrors
        .iter()
        .map(|mirror| (mirror.name.clone(), mirror.test_url.clone(), None))
        .collect();
    benchmark::run(targets, Options::from_config(config, direct)).await
}

pub async fn apply(mirror: &Mirror) -> Result<String> {
    if mirror.kind != MirrorKind::Pacman {
        return Err(Error::InvalidArgument(
            "only pacman mirrors can be applied".to_owned(),
        ));
    }
    let path = pacman_mirrorlist();
    let original = fs::read_to_string(&path)
        .await
        .map_err(|error| Error::io(&path, error))?;
    let managed = render_managed_block(mirror);
    let updated = replace_or_append(&original, &managed);
    if updated == original {
        return Ok(format!(
            "{} is already the active managed source",
            path.display()
        ));
    }
    let backup = backup_path(&path);
    fs::write(&backup, &original)
        .await
        .map_err(|error| Error::io(&backup, error))?;
    let temp = path.with_extension(format!("vpn-geo-{}.tmp", process::id()));
    write_atomic(&temp, &path, &updated).await?;
    let output = Command::new("pacman")
        .arg("-Syy")
        .output()
        .await
        .map_err(|error| Error::Internal(format!("run pacman -Syy: {error}")))?;
    if !output.status.success() {
        return Err(Error::Internal(format!(
            "mirrorlist updated, but pacman -Syy failed: {} (backup: {})",
            String::from_utf8_lossy(&output.stderr).trim(),
            backup.display()
        )));
    }
    Ok(format!(
        "{} updated; database refreshed; backup: {}",
        path.display(),
        backup.display()
    ))
}

fn pacman_mirrorlist() -> PathBuf {
    std::env::var_os("VPN_GEO_PACMAN_MIRRORLIST")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/etc/pacman.d/mirrorlist"))
}

fn backup_path(path: &Path) -> PathBuf {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    path.with_extension(format!("vpn-geo-backup-{timestamp}"))
}

fn render_managed_block(mirror: &Mirror) -> String {
    format!(
        "{MARKER_START}\n## vpn-geo selected: {} ({}, priority {})\nServer = {}\n{MARKER_END}\n",
        mirror.name,
        mirror.region.as_str(),
        mirror.priority,
        mirror.url
    )
}

fn replace_or_append(original: &str, block: &str) -> String {
    if let Some(start) = original.find(MARKER_START) {
        let end = original[start..]
            .find(MARKER_END)
            .map(|offset| start + offset + MARKER_END.len())
            .unwrap_or(original.len());
        let mut updated = String::with_capacity(original.len() + block.len());
        updated.push_str(&original[..start]);
        updated.push_str(block);
        updated.push_str(original[end..].trim_start_matches('\n'));
        updated
    } else {
        format!("{}\n{}", block, original)
    }
}

async fn write_atomic(temp: &Path, target: &Path, contents: &str) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(temp)
        .await
        .map_err(|error| Error::io(temp, error))?;
    file.write_all(contents.as_bytes())
        .await
        .map_err(|error| Error::io(temp, error))?;
    file.sync_all()
        .await
        .map_err(|error| Error::io(temp, error))?;
    drop(file);
    fs::rename(temp, target).await.map_err(|error| {
        let _ = std::fs::remove_file(temp);
        Error::io(target, error)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Mirror {
        Mirror {
            name: "fast".into(),
            kind: MirrorKind::Pacman,
            region: MirrorRegion::Cn,
            url: "https://example.test/$repo/os/$arch".into(),
            test_url: "https://example.test/core/os/x86_64/core.db.tar.gz".into(),
            priority: 10,
            enabled: true,
        }
    }

    #[test]
    fn replaces_only_managed_block() {
        let original = "# before\n# BEGIN vpn-geo managed mirrors\nServer = old\n# END vpn-geo managed mirrors\n# after\n";
        let updated = replace_or_append(original, &render_managed_block(&sample()));
        assert!(updated.starts_with("# before\n# BEGIN vpn-geo managed mirrors"));
        assert!(updated.contains("Server = https://example.test/$repo/os/$arch"));
        assert!(!updated.contains("Server = old"));
        assert!(updated.ends_with("# after\n"));
    }

    #[test]
    fn appends_block_when_absent() {
        let updated = replace_or_append("# original\n", "managed\n");
        assert!(updated.starts_with("managed\n\n# original\n"));
    }
}
