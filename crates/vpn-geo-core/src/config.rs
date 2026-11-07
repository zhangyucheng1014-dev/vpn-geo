use std::{
    collections::HashSet,
    env,
    path::{Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use url::Url;

use crate::{APP_NAME, Error, Result};

#[derive(Clone, Debug, Serialize)]
pub struct Config {
    pub version: u32,
    pub settle_delay: Duration,
    pub quiet_period: Duration,
    pub geoip: GeoIpConfig,
    pub benchmark: BenchmarkConfig,
    pub nodes: Vec<Node>,
    pub mirrors: Vec<Mirror>,
}

#[derive(Clone, Debug, Serialize)]
pub struct GeoIpConfig {
    pub urls: Vec<String>,
    pub timeout: Duration,
    pub min_agreement: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct BenchmarkConfig {
    pub candidate_countries: usize,
    pub samples: usize,
    pub request_timeout: Duration,
    pub download_bytes: u64,
    pub parallel: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct Node {
    pub name: String,
    pub uuid: String,
    pub country: String,
    pub latitude: f64,
    pub longitude: f64,
    pub test_url: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MirrorRegion {
    Cn,
    Hk,
    Jp,
    Us,
    #[default]
    Global,
}

impl MirrorRegion {
    pub const ALL: [Self; 5] = [Self::Cn, Self::Hk, Self::Jp, Self::Us, Self::Global];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cn => "cn",
            Self::Hk => "hk",
            Self::Jp => "jp",
            Self::Us => "us",
            Self::Global => "global",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "cn" | "china" | "中" | "中国" | "大陆" => Some(Self::Cn),
            "hk" | "hongkong" | "hong-kong" | "港" | "香港" => Some(Self::Hk),
            "jp" | "japan" | "日" | "日本" => Some(Self::Jp),
            "us" | "usa" | "美" | "美国" => Some(Self::Us),
            "global" | "all" | "全球" => Some(Self::Global),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MirrorKind {
    #[default]
    Pacman,
    Generic,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Mirror {
    pub name: String,
    #[serde(default)]
    pub kind: MirrorKind,
    #[serde(default)]
    pub region: MirrorRegion,
    pub url: String,
    pub test_url: String,
    #[serde(default)]
    pub priority: i32,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    version: Option<u32>,
    settle_delay: Option<String>,
    quiet_period: Option<String>,
    geoip: Option<RawGeoIp>,
    benchmark: Option<RawBenchmark>,
    #[serde(default)]
    nodes: Vec<RawNode>,
    #[serde(default)]
    mirrors: Vec<Mirror>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawGeoIp {
    url: Option<String>,
    urls: Option<Vec<String>>,
    timeout: Option<String>,
    min_agreement: Option<usize>,
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

    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|error| Error::io(path, error))?;
        let raw: RawConfig = toml::from_str(&text)
            .map_err(|error| Error::config(format!("parse {}: {error}", path.display())))?;
        Self::from_raw(raw)
    }

    fn from_raw(raw: RawConfig) -> Result<Self> {
        let version = raw.version.unwrap_or(2);
        if version > 3 {
            return Err(Error::config(format!(
                "unsupported configuration version {version}"
            )));
        }
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
        if urls.is_empty() || urls.len() > 8 {
            return Err(Error::config("geoip needs between 1 and 8 sources"));
        }
        let min_agreement = geoip_raw.min_agreement.unwrap_or(2).max(1).min(urls.len());
        let mut seen_sources = HashSet::new();
        for url in &urls {
            validate_url("geoip source", url)?;
            if !seen_sources.insert(url.clone()) {
                return Err(Error::config(format!("duplicate GeoIP source: {url}")));
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
            return Err(Error::config("benchmark values exceed safe limits"));
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
                return Err(Error::config(format!(
                    "nodes[{index}] needs name, uuid, and a two-letter country code"
                )));
            }
            if !uuids.insert(uuid.clone()) {
                return Err(Error::config(format!(
                    "nodes[{index}] repeats UUID {uuid:?}"
                )));
            }
            if (node.latitude != 0.0 || node.longitude != 0.0)
                && (!node.latitude.is_finite()
                    || !(-90.0..=90.0).contains(&node.latitude)
                    || !node.longitude.is_finite()
                    || !(-180.0..=180.0).contains(&node.longitude))
            {
                return Err(Error::config(format!(
                    "nodes[{index}] coordinates are out of range"
                )));
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
            return Err(Error::config("at least one [[nodes]] entry is required"));
        }

        let mut mirror_names = HashSet::new();
        for (index, mirror) in raw.mirrors.iter().enumerate() {
            if mirror.name.trim().is_empty()
                || mirror.url.trim().is_empty()
                || mirror.test_url.trim().is_empty()
            {
                return Err(Error::config(format!(
                    "mirrors[{index}] needs name, url, and test_url"
                )));
            }
            if !mirror_names.insert(mirror.name.clone()) {
                return Err(Error::config(format!(
                    "mirrors[{index}] repeats name {:?}",
                    mirror.name
                )));
            }
            validate_url(&format!("mirrors[{index}].url"), &mirror.url)?;
            validate_url(&format!("mirrors[{index}].test_url"), &mirror.test_url)?;
        }

        Ok(Self {
            version,
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
                min_agreement,
            },
            benchmark,
            nodes,
            mirrors: raw.mirrors,
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

pub fn default_mirrors() -> Vec<Mirror> {
    const CATALOG: &str = r#"
cn-tsinghua,cn,https://mirrors.tuna.tsinghua.edu.cn/archlinux/
cn-ustc,cn,https://mirrors.ustc.edu.cn/archlinux/
cn-aliyun,cn,https://mirrors.aliyun.com/archlinux/
cn-jlu,cn,https://mirrors.jlu.edu.cn/archlinux/
cn-nju,cn,https://mirrors.nju.edu.cn/archlinux/
cn-xjtu,cn,https://mirrors.xjtu.edu.cn/archlinux/
cn-hust,cn,https://mirrors.hust.edu.cn/archlinux/
cn-nyist,cn,https://mirror.nyist.edu.cn/archlinux/
hk-cicku,hk,https://hk.mirrors.cicku.me/archlinux/
hk-rackspace,hk,https://hkg.mirror.rackspace.com/archlinux/
hk-koddos,hk,https://mirror-hk.koddos.net/archlinux/
jp-miraa,jp,https://www.miraa.jp/archlinux/
jp-aria,jp,https://mirror.aria-on-the-planet.es/archlinux/
jp-yamagata,jp,https://ftp.yz.yamagata-u.ac.jp/pub/linux/archlinux/
jp-jaist,jp,https://ftp.jaist.ac.jp/pub/Linux/ArchLinux/
jp-cicku,jp,https://jp.mirrors.cicku.me/archlinux/
jp-rain,jp,https://mirror.rain.ne.jp/archlinux/
jp-hashy,jp,https://mirror.hashy0917.net/archlinux/
us-mtu,us,https://mirrors.lug.mtu.edu/archlinux/
us-rit,us,https://mirrors.rit.edu/archlinux/
us-umd,us,https://mirror.umd.edu/archlinux/
us-osuosl,us,https://ftp.osuosl.org/pub/archlinux/
us-pkgbuild-la,us,https://losangeles.mirror.pkgbuild.com/
us-cicku,us,https://us.mirrors.cicku.me/archlinux/
us-constant,us,https://arch.mirror.constant.com/
us-tzulo,us,https://mirror.tzulo.com/archlinux/
us-misaka-iad,us,https://iad.mirrors.misaka.one/archlinux/
us-berkeley,us,https://mirrors.ocf.berkeley.edu/archlinux/
global-cloudflare,global,https://cloudflaremirrors.com/archlinux/
"#;
    CATALOG
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let (name, rest) = line.split_once(',')?;
            let (region, base) = rest.split_once(',')?;
            Some((name, region, base))
        })
        .map(|(name, region, base)| {
            let base = base.trim_end_matches('/');
            Mirror {
                name: name.to_owned(),
                kind: MirrorKind::Pacman,
                region: MirrorRegion::parse(region).unwrap_or_default(),
                url: format!("{base}/$repo/os/$arch"),
                test_url: format!("{base}/core/os/x86_64/core.db.tar.gz"),
                priority: 0,
                enabled: true,
            }
        })
        .collect()
}

pub fn parse_duration(field: &str, value: &str) -> Result<Duration> {
    let value = value.trim();
    let split = value.find(|character: char| character.is_ascii_alphabetic());
    let (number, unit) = match split {
        Some(index) => (&value[..index], &value[index..]),
        None => (value, "s"),
    };
    let number: f64 = number
        .parse()
        .map_err(|_| Error::config(format!("{field} must be a non-negative duration")))?;
    let multiplier = match unit {
        "ns" => 1e-9,
        "us" | "µs" => 1e-6,
        "ms" => 1e-3,
        "s" => 1.0,
        "m" => 60.0,
        "h" => 3600.0,
        _ => {
            return Err(Error::config(format!(
                "{field} has an unsupported duration unit"
            )));
        }
    };
    let seconds = number * multiplier;
    if !seconds.is_finite() || seconds < 0.0 || seconds > 86400.0 {
        return Err(Error::config(format!(
            "{field} must be between 0 and 24 hours"
        )));
    }
    Ok(Duration::from_secs_f64(seconds))
}

pub fn validate_url(field: &str, raw: &str) -> Result<()> {
    let parsed = Url::parse(raw)
        .map_err(|_| Error::config(format!("{field} must be an absolute http(s) URL")))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(Error::config(format!(
            "{field} must be an absolute http(s) URL"
        )));
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
    fn legacy_config_keeps_defaults() {
        let config = Config::from_raw(toml::from_str(BASE).unwrap()).unwrap();
        assert_eq!(config.version, 2);
        assert_eq!(config.nodes[0].country, "JP");
        assert_eq!(config.geoip.urls.len(), 3);
        assert_eq!(config.geoip.min_agreement, 2);
    }

    #[test]
    fn rejects_unknown_keys_and_duplicate_uuids() {
        assert!(toml::from_str::<RawConfig>(&format!("unknown = true\n{BASE}")).is_err());
        let duplicate =
            format!("{BASE}\n[[nodes]]\nname = \"jp2\"\nuuid = \"uuid-jp\"\ncountry = \"JP\"\n");
        assert!(Config::from_raw(toml::from_str(&duplicate).unwrap()).is_err());
    }

    #[test]
    fn catalog_has_multiple_sources_per_region() {
        let catalog = default_mirrors();
        assert_eq!(catalog.len(), 29);
        for region in [
            MirrorRegion::Cn,
            MirrorRegion::Hk,
            MirrorRegion::Jp,
            MirrorRegion::Us,
        ] {
            assert!(catalog.iter().filter(|item| item.region == region).count() >= 3);
        }
    }
}
