use std::path::{Path, PathBuf};

use tokio::{fs, io::AsyncWriteExt, process::Command};
use vpn_geo_app::{AppError, AppResult};
use vpn_geo_domain::model::Mirror;

const MARKER_START: &str = "# BEGIN vpn-geo managed mirrors";
const MARKER_END: &str = "# END vpn-geo managed mirrors";

pub async fn apply(mirror: &Mirror) -> AppResult<String> {
    if mirror.kind != vpn_geo_domain::model::MirrorKind::Pacman {
        return Err(AppError::Domain(vpn_geo_domain::DomainError::invalid(
            "only pacman mirrors can be applied",
        )));
    }
    let path = pacman_mirrorlist();
    let original = fs::read_to_string(&path)
        .await
        .map_err(|error| AppError::io(format!("read {}: {error}", path.display())))?;
    let updated = replace_or_append(&original, &render(mirror));
    if updated == original {
        return Ok(format!(
            "{} is already the active managed source",
            path.display()
        ));
    }
    let backup = backup_path(&path);
    fs::write(&backup, &original)
        .await
        .map_err(|error| AppError::io(format!("write {}: {error}", backup.display())))?;
    let temp = path.with_extension(format!("vpn-geo-{}.tmp", std::process::id()));
    write_atomic(&temp, &path, &updated).await?;
    let output = Command::new("pacman")
        .arg("-Syy")
        .output()
        .await
        .map_err(|error| AppError::Internal(format!("run pacman -Syy: {error}")))?;
    if !output.status.success() {
        return Err(AppError::Internal(format!(
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

fn render(mirror: &Mirror) -> String {
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

async fn write_atomic(temp: &Path, target: &Path, contents: &str) -> AppResult<()> {
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(temp)
        .await
        .map_err(|error| AppError::io(format!("create {}: {error}", temp.display())))?;
    file.write_all(contents.as_bytes())
        .await
        .map_err(|error| AppError::io(format!("write {}: {error}", temp.display())))?;
    file.sync_all()
        .await
        .map_err(|error| AppError::io(format!("sync {}: {error}", temp.display())))?;
    drop(file);
    fs::rename(temp, target).await.map_err(|error| {
        let _ = std::fs::remove_file(temp);
        AppError::io(format!("replace {}: {error}", target.display()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_only_managed_block() {
        let mirror = Mirror {
            name: "fast".into(),
            kind: vpn_geo_domain::model::MirrorKind::Pacman,
            region: vpn_geo_domain::model::Region::Cn,
            url: "https://example.test/$repo/os/$arch".into(),
            test_url: "https://example.test/db".into(),
            priority: 10,
            enabled: true,
        };
        let original = "# before\n# BEGIN vpn-geo managed mirrors\nServer = old\n# END vpn-geo managed mirrors\n# after\n";
        let updated = replace_or_append(original, &render(&mirror));
        assert!(updated.contains("Server = https://example.test/$repo/os/$arch"));
        assert!(!updated.contains("Server = old"));
        assert!(updated.ends_with("# after\n"));
    }
}
