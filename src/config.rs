use std::{collections::HashSet, env, path::PathBuf, time::Duration};

use serde::{Deserialize, Serialize};
use url::Url;

pub const APP_NAME: &str = "vpn-geo";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug)]
pub struct Config {
    pub settle_delay: Duration,
    pub quiet_period: Duration,
    pub geoip: GeoIpConfig,
    pub benchmark: BenchmarkConfig,
    pub nodes: Vec<Node>,
    pub mirrors: Vec<Mirror>,
}

#[derive(Clone, Debug)]
pub struct GeoIpConfig {
    pub urls: Vec<String>,
    pub timeout: Duration,
}

#[derive(Clone, Debug)]
pub struct BenchmarkConfig {
    pub candidate_countries: usize,
    pub samples: usize,
    pub request_timeout: Duration,
    pub download_bytes: u64,
    pub parallel: usize,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub uuid: String,
    pub country: String,
    pub latitude: f64,
    pub longitude: f64,
    pub test_url: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum MirrorKind {
    Pacman,
    Generic,
}

#[derive(Clone, Debug, Serialize)]
pub struct Mirror {
    pub name: String,
    pub kind: MirrorKind,
    pub url: String,
    pub test_url: String,
    pub priority: i32,
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    settle_delay: Option<String>,
    quiet_period: Option<String>,
    geoip: Option<RawGeoIp>,
    benchmark: Option<RawBenchmark>,
    #[serde(default)]
    nodes: Vec<RawNode>,
    #[serde(default)]
    mirrors: Vec<RawMirror>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGeoIp {
    url: Option<String>,
    urls: Option<Vec<String>>,
    timeout: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBenchmark {
    candidate_countries: Option<usize>,
    samples: Option<usize>,
    request_timeout: Option<String>,
    download_bytes: Option<u64>,
    parallel: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawNode {
    name: Option<String>,
    uuid: Option<String>,
    country: Option<String>,
    #[serde(default)]
    latitude: f64,
    #[serde(default)]
    longitude: f64,
    test_url: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMirror {
    name: Option<String>,
    kind: Option<String>,
    url: Option<String>,
    test_url: Option<String>,
    #[serde(default)]
    priority: i32,
    #[serde(default = "default_true")]
    enabled: bool,
}

fn default_true() -> bool {
    true
}

impl Config {
    pub fn default_path() -> PathBuf {
        if let Some(base) = env::var_os("XDG_CONFIG_HOME") {
            return PathBuf::from(base).join(APP_NAME).join("config.toml");
        }
        if let Some(home) = env::var_os("HOME") {
            return PathBuf::from(home)
                .join(".config")
                .join(APP_NAME)
                .join("config.toml");
        }
        PathBuf::from("config.toml")
    }

    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("read config {}: {error}", path.display()))?;
        let raw: RawConfig = toml::from_str(&text)
            .map_err(|error| format!("parse config {}: {error}", path.display()))?;
        Self::from_raw(raw)
    }

    fn from_raw(raw: RawConfig) -> Result<Self, String> {
        let geoip_raw = raw.geoip.unwrap_or_default();
        let mut urls = geoip_raw.urls.unwrap_or_default();
        if let Some(url) = geoip_raw.url {
            urls.insert(0, url);
        }
        if urls.is_empty() {
            urls = default_geoip_urls()
                .iter()
                .map(|url| (*url).to_owned())
                .collect();
        }
        if urls.len() > 8 {
            return Err("geoip supports at most 8 sources".to_owned());
        }
        let mut seen_sources = HashSet::new();
        for url in &urls {
            validate_url("geoip source", url)?;
            if !seen_sources.insert(url.clone()) {
                return Err(format!("duplicate GeoIP source: {url}"));
            }
        }

        let benchmark_raw = raw.benchmark.unwrap_or_default();
        let benchmark = BenchmarkConfig {
            candidate_countries: benchmark_raw.candidate_countries.unwrap_or(3),
            samples: benchmark_raw.samples.unwrap_or(3),
            request_timeout: parse_duration(
                "benchmark.request_timeout",
                benchmark_raw.request_timeout.as_deref().unwrap_or("8s"),
            )?,
            download_bytes: benchmark_raw.download_bytes.unwrap_or(10 * 1024 * 1024),
            parallel: benchmark_raw.parallel.unwrap_or(2),
        };
        if benchmark.candidate_countries == 0
            || benchmark.candidate_countries > 100
            || benchmark.samples == 0
            || benchmark.samples > 100
            || benchmark.request_timeout.is_zero()
            || benchmark.download_bytes == 0
            || benchmark.download_bytes > 1024 * 1024 * 1024
            || benchmark.parallel == 0
            || benchmark.parallel > 32
        {
            return Err("benchmark values exceed safe limits".to_owned());
        }

        let mut nodes = Vec::with_capacity(raw.nodes.len());
        let mut uuids = HashSet::new();
        for (index, node) in raw.nodes.into_iter().enumerate() {
            let name = node.name.unwrap_or_default().trim().to_owned();
            let uuid = node.uuid.unwrap_or_default().trim().to_owned();
            let country = node.country.unwrap_or_default().trim().to_ascii_uppercase();
            let test_url = node
                .test_url
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty());
            if name.is_empty() || uuid.is_empty() || !valid_country(&country) {
                return Err(format!(
                    "nodes[{index}] needs name, uuid, and a two-letter country code"
                ));
            }
            if !uuids.insert(uuid.clone()) {
                return Err(format!("nodes[{index}] repeats UUID {uuid:?}"));
            }
            if (node.latitude != 0.0 || node.longitude != 0.0)
                && (!node.latitude.is_finite()
                    || !(-90.0..=90.0).contains(&node.latitude)
                    || !node.longitude.is_finite()
                    || !(-180.0..=180.0).contains(&node.longitude))
            {
                return Err(format!("nodes[{index}] coordinates are out of range"));
            }
            if let Some(url) = &test_url {
                validate_url(&format!("nodes[{index}].test_url"), url)?;
            }
            nodes.push(Node {
                name,
                uuid,
                country,
                latitude: node.latitude,
                longitude: node.longitude,
                test_url,
            });
        }
        if nodes.is_empty() {
            return Err("at least one [[nodes]] entry is required".to_owned());
        }

        let mut mirrors = Vec::new();
        let mut mirror_names = HashSet::new();
        for (index, mirror) in raw.mirrors.into_iter().enumerate() {
            let name = mirror.name.unwrap_or_default().trim().to_owned();
            let url = mirror.url.unwrap_or_default().trim().to_owned();
            let test_url = mirror.test_url.unwrap_or_default().trim().to_owned();
            if name.is_empty() || url.is_empty() || test_url.is_empty() {
                return Err(format!("mirrors[{index}] needs name, url, and test_url"));
            }
            if !mirror_names.insert(name.clone()) {
                return Err(format!("mirrors[{index}] repeats name {name:?}"));
            }
            validate_url(&format!("mirrors[{index}].url"), &url)?;
            validate_url(&format!("mirrors[{index}].test_url"), &test_url)?;
            let kind = match mirror.kind.as_deref().unwrap_or("generic") {
                "pacman" => MirrorKind::Pacman,
                "generic" => MirrorKind::Generic,
                other => {
                    return Err(format!(
                        "mirrors[{index}].kind must be pacman or generic, got {other:?}"
                    ))
                }
            };
            mirrors.push(Mirror {
                name,
                kind,
                url,
                test_url,
                priority: mirror.priority,
                enabled: mirror.enabled,
            });
        }

        Ok(Self {
            settle_delay: parse_duration(
                "settle_delay",
                raw.settle_delay.as_deref().unwrap_or("3s"),
            )?,
            quiet_period: parse_duration(
                "quiet_period",
                raw.quiet_period.as_deref().unwrap_or("500ms"),
            )?,
            geoip: GeoIpConfig {
                urls,
                timeout: parse_duration(
                    "geoip.timeout",
                    geoip_raw.timeout.as_deref().unwrap_or("10s"),
                )?,
            },
            benchmark,
            nodes,
            mirrors,
        })
    }
}

pub fn default_geoip_urls() -> &'static [&'static str] {
    &[
        "https://ipwho.is/",
        "https://ipapi.co/json/",
        "https://ip-api.com/json/?fields=status,countryCode,lat,lon,message",
    ]
}

pub fn default_mirrors() -> Vec<(String, MirrorKind, &'static str, &'static str)> {
    vec![
        (
            "tsinghua".to_owned(),
            MirrorKind::Pacman,
            "https://mirrors.tuna.tsinghua.edu.cn/archlinux/$repo/os/$arch",
            "https://mirrors.tuna.tsinghua.edu.cn/archlinux/core/os/x86_64/core.db.tar.gz",
        ),
        (
            "ustc".to_owned(),
            MirrorKind::Pacman,
            "https://mirrors.ustc.edu.cn/archlinux/$repo/os/$arch",
            "https://mirrors.ustc.edu.cn/archlinux/core/os/x86_64/core.db.tar.gz",
        ),
        (
            "aliyun".to_owned(),
            MirrorKind::Pacman,
            "https://mirrors.aliyun.com/archlinux/$repo/os/$arch",
            "https://mirrors.aliyun.com/archlinux/core/os/x86_64/core.db.tar.gz",
        ),
        (
            "rsproxy".to_owned(),
            MirrorKind::Pacman,
            "https://mirrors.rsproxy.cn/archlinux/$repo/os/$arch",
            "https://mirrors.rsproxy.cn/archlinux/core/os/x86_64/core.db.tar.gz",
        ),
        (
            "cloudflare".to_owned(),
            MirrorKind::Generic,
            "https://cloudflaremirrors.com/archlinux/$repo/os/$arch",
            "https://cloudflaremirrors.com/archlinux/core/os/x86_64/core.db.tar.gz",
        ),
    ]
}

pub fn parse_duration(field: &str, value: &str) -> Result<Duration, String> {
    let value = value.trim();
    let split = value.find(|character: char| character.is_ascii_alphabetic());
    let (number, unit) = match split {
        Some(index) => (&value[..index], &value[index..]),
        None => (value, "s"),
    };
    let number: f64 = number
        .parse()
        .map_err(|_| format!("{field} must be a non-negative duration"))?;
    let multiplier = match unit {
        "ns" => 1e-9,
        "us" | "µs" => 1e-6,
        "ms" => 1e-3,
        "s" => 1.0,
        "m" => 60.0,
        "h" => 3600.0,
        _ => return Err(format!("{field} has an unsupported duration unit")),
    };
    let seconds = number * multiplier;
    if !seconds.is_finite() || seconds < 0.0 || seconds > 86400.0 {
        return Err(format!("{field} must be between 0 and 24 hours"));
    }
    Ok(Duration::from_secs_f64(seconds))
}

pub fn validate_url(field: &str, raw: &str) -> Result<(), String> {
    let parsed = Url::parse(raw).map_err(|_| format!("{field} must be an absolute http(s) URL"))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(format!("{field} must be an absolute http(s) URL"));
    }
    Ok(())
}

pub fn valid_country(country: &str) -> bool {
    country.len() == 2 && country.bytes().all(|byte| byte.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = r#"
[[nodes]]
name = "jp"
uuid = "uuid-jp"
country = "jp"
"#;

    #[test]
    fn parses_legacy_config_with_defaults() {
        let cfg = Config::from_raw(toml::from_str(BASE).unwrap()).unwrap();
        assert_eq!(cfg.nodes[0].country, "JP");
        assert_eq!(cfg.settle_delay, Duration::from_secs(3));
        assert_eq!(cfg.geoip.urls.len(), 3);
        assert!(cfg.mirrors.is_empty());
    }

    #[test]
    fn rejects_unknown_keys_and_duplicate_uuids() {
        let unknown = format!("unknown = true\n{BASE}");
        assert!(toml::from_str::<RawConfig>(&unknown).is_err());
        let duplicate =
            format!("{BASE}\n[[nodes]]\nname = \"jp2\"\nuuid = \"uuid-jp\"\ncountry = \"JP\"\n");
        assert!(Config::from_raw(toml::from_str(&duplicate).unwrap()).is_err());
    }

    #[test]
    fn parses_duration_units() {
        assert_eq!(
            parse_duration("d", "500ms").unwrap(),
            Duration::from_millis(500)
        );
        assert_eq!(parse_duration("d", "2").unwrap(), Duration::from_secs(2));
        assert!(parse_duration("d", "-1s").is_err());
    }
}
