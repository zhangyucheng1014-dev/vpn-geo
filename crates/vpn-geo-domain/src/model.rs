use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Region {
    Cn,
    Hk,
    Jp,
    Us,
    #[default]
    Global,
}

impl Region {
    pub const FOCUS: [Self; 4] = [Self::Cn, Self::Hk, Self::Jp, Self::Us];
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

    pub fn official_code(self) -> Option<&'static str> {
        match self {
            Self::Cn => Some("CN"),
            Self::Hk => Some("HK"),
            Self::Jp => Some("JP"),
            Self::Us => Some("US"),
            Self::Global => None,
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
    pub region: Region,
    pub url: String,
    pub test_url: String,
    #[serde(default)]
    pub priority: i32,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Serialize)]
pub struct Location {
    pub country_code: String,
    pub latitude: f64,
    pub longitude: f64,
    pub source: String,
    pub agreement: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct NetworkStatus {
    pub vpn_active: bool,
    pub vpn_connections: Vec<String>,
    pub active_connections: Vec<String>,
    pub default_devices: Vec<String>,
    pub system_proxy: bool,
    pub proxy_environment: bool,
    pub desktop_proxy: bool,
    pub route: String,
    pub confidence: String,
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
        self.mbps / self.latency_ms.unwrap_or(1000.0).max(1.0).ln_1p()
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct SyncSummary {
    pub total_official: usize,
    pub candidates: usize,
    pub selected: usize,
    pub regions: std::collections::BTreeMap<String, usize>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DoctorReport {
    pub checks: Vec<DoctorCheck>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DoctorCheck {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}
