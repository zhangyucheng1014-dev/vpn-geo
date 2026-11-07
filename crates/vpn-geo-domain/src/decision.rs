use crate::model::{Location, NetworkStatus, SpeedResult};

pub fn classify_route(
    vpn_active: bool,
    system_proxy: bool,
    active_connections: usize,
) -> (&'static str, &'static str) {
    if vpn_active && system_proxy {
        ("VPN + system proxy", "high")
    } else if vpn_active {
        ("VPN", "high")
    } else if system_proxy {
        ("System proxy", "medium")
    } else if active_connections == 0 {
        (
            "Unknown (no active NetworkManager connection was observed)",
            "low",
        )
    } else {
        ("Direct (no VPN or system proxy detected)", "medium")
    }
}

pub fn network_status(
    vpn_active: bool,
    vpn_connections: Vec<String>,
    active_connections: Vec<String>,
    default_devices: Vec<String>,
    proxy_environment: bool,
    desktop_proxy: bool,
) -> NetworkStatus {
    let system_proxy = proxy_environment || desktop_proxy;
    let (route, confidence) = classify_route(vpn_active, system_proxy, active_connections.len());
    NetworkStatus {
        vpn_active,
        vpn_connections,
        active_connections,
        default_devices,
        system_proxy,
        proxy_environment,
        desktop_proxy,
        route: route.to_owned(),
        confidence: confidence.to_owned(),
    }
}

pub fn nearest_nodes(
    nodes: &[crate::config::Node],
    location: &Location,
    count: usize,
) -> Vec<(crate::config::Node, f64)> {
    let mut items: Vec<(crate::config::Node, f64)> = nodes
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

pub fn best_result(results: &[SpeedResult]) -> Option<&SpeedResult> {
    results
        .iter()
        .filter(|result| result.error.is_none())
        .max_by(|left, right| left.score().total_cmp(&right.score()))
}

pub fn geoip_consensus(votes: Vec<Location>, minimum: usize) -> Result<Location, String> {
    let mut groups: std::collections::BTreeMap<String, Vec<Location>> =
        std::collections::BTreeMap::new();
    for vote in votes {
        groups
            .entry(vote.country_code.clone())
            .or_default()
            .push(vote);
    }
    let Some((_, mut group)) =
        groups
            .into_iter()
            .max_by(|(left_country, left), (right_country, right)| {
                left.len()
                    .cmp(&right.len())
                    .then_with(|| left[0].source.cmp(&right[0].source))
                    .then_with(|| right_country.cmp(left_country))
            })
    else {
        return Err("no GeoIP source succeeded".to_owned());
    };
    group.sort_by(|left, right| left.source.cmp(&right.source));
    let agreement = group.len();
    if agreement < minimum {
        return Err(format!(
            "GeoIP consensus is too weak: {agreement} < {minimum}"
        ));
    }
    let mut best = group.remove(0);
    best.agreement = agreement;
    Ok(best)
}

fn haversine(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let delta_lat = (lat2 - lat1).to_radians();
    let delta_lon = (lon2 - lon1).to_radians();
    let a = (delta_lat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (delta_lon / 2.0).sin().powi(2);
    6371.0 * 2.0 * a.sqrt().atan2((1.0 - a).sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn location(country: &str, source: &str) -> Location {
        Location {
            country_code: country.into(),
            latitude: 0.0,
            longitude: 0.0,
            source: source.into(),
            agreement: 1,
        }
    }

    #[test]
    fn consensus_requires_minimum_agreement() {
        let votes = vec![
            location("JP", "a"),
            location("JP", "b"),
            location("US", "c"),
        ];
        assert_eq!(
            geoip_consensus(votes.clone(), 2).unwrap().country_code,
            "JP"
        );
        assert!(geoip_consensus(votes, 3).is_err());
    }

    #[test]
    fn route_classification_is_stable() {
        assert_eq!(classify_route(true, false, 1).0, "VPN");
        assert_eq!(classify_route(false, true, 1).0, "System proxy");
        assert_eq!(classify_route(false, false, 0).1, "low");
    }

    #[test]
    fn best_result_ignores_errors() {
        let results = vec![
            SpeedResult {
                label: "failed".into(),
                url: "x".into(),
                distance_km: None,
                latency_ms: None,
                bytes: 0,
                mbps: 999.0,
                error: Some("bad".into()),
            },
            SpeedResult {
                label: "ok".into(),
                url: "x".into(),
                distance_km: None,
                latency_ms: Some(10.0),
                bytes: 100,
                mbps: 10.0,
                error: None,
            },
        ];
        assert_eq!(best_result(&results).unwrap().label, "ok");
    }
}
