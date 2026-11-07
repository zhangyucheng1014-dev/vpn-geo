use std::{
    path::{Path, PathBuf},
    process,
};

use tokio::{fs, io::AsyncWriteExt, process::Command};

use crate::config::{Mirror, MirrorKind};

const MARKER_START: &str = "# BEGIN vpn-geo managed mirrors";
const MARKER_END: &str = "# END vpn-geo managed mirrors";

pub async fn list(
    config_path: &Path,
    config: &crate::config::Config,
    region: Option<crate::config::MirrorRegion>,
    json: bool,
) -> u8 {
    let mirrors = crate::benchmark::mirror_results(config, region);
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&mirrors).unwrap_or_else(|_| "[]".to_owned())
        );
        return 0;
    }
    println!("Configuration: {}", config_path.display());
    println!(
        "{:<20} {:<10} {:<8} {:>8} {:<6} URL",
        "NAME", "KIND", "REGION", "PRIORITY", "STATE"
    );
    for mirror in &mirrors {
        println!(
            "{:<20} {:<10} {:<8} {:>8} {:<6} {}",
            mirror.name,
            kind_label(mirror.kind),
            mirror.region.as_str(),
            mirror.priority,
            if mirror.enabled { "on" } else { "off" },
            mirror.url
        );
    }
    if config.mirrors.is_empty() {
        println!(
            "\nUsing built-in source catalog ({} sources). Add [[mirrors]] to customize it.",
            mirrors.len()
        );
    }
    0
}

pub async fn speed(
    config: &crate::config::Config,
    region: Option<crate::config::MirrorRegion>,
    direct: bool,
    json: bool,
) -> u8 {
    let mirrors = crate::benchmark::mirror_results(config, region);
    if mirrors.is_empty() {
        eprintln!("no mirrors match the selected region");
        return 2;
    }
    let targets = mirrors
        .iter()
        .map(|mirror| (mirror.name.clone(), mirror.test_url.clone(), None))
        .collect();
    let options = crate::benchmark::Options {
        samples: config.benchmark.samples,
        timeout: config.benchmark.request_timeout,
        download_bytes: config.benchmark.download_bytes,
        parallel: config.benchmark.parallel,
        direct,
    };
    let results = match crate::benchmark::run(config, targets, options).await {
        Ok(results) => results,
        Err(error) => {
            eprintln!("mirror speed failed: {error}");
            return 1;
        }
    };
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&results).unwrap_or_else(|_| "[]".to_owned())
        );
    } else {
        crate::benchmark::print_table(&results, false);
    }
    if results.iter().all(|result| result.error.is_some()) {
        return 1;
    }
    0
}

pub async fn apply(mirror: &Mirror) -> Result<String, String> {
    if mirror.kind != MirrorKind::Pacman {
        return Err("only pacman mirrors can be applied".to_owned());
    }
    let path = pacman_mirrorlist();
    let original = fs::read_to_string(&path)
        .await
        .map_err(|error| format!("read {}: {error}", path.display()))?;
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
        .map_err(|error| format!("write backup {}: {error}", backup.display()))?;
    let temp = path.with_extension(format!("vpn-geo-{}.tmp", process::id()));
    write_atomic(&temp, &path, &updated).await?;
    let output = Command::new("pacman")
        .arg("-Syy")
        .output()
        .await
        .map_err(|error| format!("run pacman -Syy: {error}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(format!(
            "mirrorlist updated, but pacman -Syy failed: {detail} (backup: {})",
            backup.display()
        ));
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
        "{MARKER_START}\n## vpn-geo selected: {} (priority {})\nServer = {}\n{MARKER_END}\n",
        mirror.name, mirror.priority, mirror.url
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

async fn write_atomic(temp: &Path, target: &Path, contents: &str) -> Result<(), String> {
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(temp)
        .await
        .map_err(|error| format!("create {}: {error}", temp.display()))?;
    file.write_all(contents.as_bytes())
        .await
        .map_err(|error| format!("write {}: {error}", temp.display()))?;
    file.sync_all()
        .await
        .map_err(|error| format!("sync {}: {error}", temp.display()))?;
    drop(file);
    fs::rename(temp, target).await.map_err(|error| {
        let _ = std::fs::remove_file(temp);
        format!("replace {}: {error}", target.display())
    })
}

fn kind_label(kind: MirrorKind) -> &'static str {
    match kind {
        MirrorKind::Pacman => "pacman",
        MirrorKind::Generic => "generic",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_only_managed_block() {
        let original = "# before\n# BEGIN vpn-geo managed mirrors\nServer = old\n# END vpn-geo managed mirrors\n# after\n";
        let updated = replace_or_append(
            original,
            &render_managed_block(&Mirror {
                name: "fast".into(),
                kind: MirrorKind::Pacman,
                region: crate::config::MirrorRegion::Cn,
                url: "https://example.test/$repo/os/$arch".into(),
                test_url: "https://example.test/file".into(),
                priority: 10,
                enabled: true,
            }),
        );
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
