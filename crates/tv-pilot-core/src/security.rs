use serde::{Deserialize, Serialize};

use crate::adb::{AdbClient, PROTECTED_SETUPWRAITH_PACKAGE};
use crate::config::TvConfig;
use crate::error::Result;
use crate::torrserve::TorrServeClient;

const ALLOWED_VIDEO_EXTS: &[&str] = &["mkv", "mp4", "avi", "ts", "m2ts", "mov", "webm"];
const DANGEROUS_EXTS: &[&str] = &["apk", "exe", "sh", "bat", "zip", "dex", "so", "msi"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileSafetyVerdict {
    SafeVideo,
    BlockedDangerousExtension(String),
    NotVideoExtension(String),
}

pub fn inspect_torrent_file_safety(path: &str) -> FileSafetyVerdict {
    let ext = path
        .rsplit('.')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();

    if DANGEROUS_EXTS.contains(&ext.as_str()) {
        FileSafetyVerdict::BlockedDangerousExtension(ext)
    } else if ALLOWED_VIDEO_EXTS.contains(&ext.as_str()) {
        FileSafetyVerdict::SafeVideo
    } else {
        FileSafetyVerdict::NotVideoExtension(ext)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityAuditReport {
    pub unknown_third_party_packages: Vec<String>,
    pub dangerous_permissions: Vec<String>,
    pub wg0_vpn_active: bool,
    pub setupwraith_intact: bool,
    pub torrserve_use_disk_off: bool,
    pub torrserve_cache_mb: u64,
}

pub async fn run_security_audit(
    adb: &AdbClient,
    ts: &TorrServeClient,
    config: &TvConfig,
) -> Result<SecurityAuditReport> {
    adb.ensure_connected().await?;

    let pkgs_raw = adb.exec_cmd("pm", &["list", "packages", "-3"]).await?;
    let installed: Vec<String> = pkgs_raw
        .lines()
        .filter_map(|l| l.trim().strip_prefix("package:").map(str::to_string))
        .collect();

    let unknown_third_party_packages: Vec<String> = installed
        .iter()
        .filter(|p| !config.trusted_packages.iter().any(|t| t == *p))
        .cloned()
        .collect();

    // shortcut: checks appops for SYSTEM_ALERT_WINDOW and REQUEST_INSTALL_PACKAGES in one pass, upgrade to per-package dumpsys if granular grant state needed
    let mut dangerous_permissions = Vec::new();
    for op in ["SYSTEM_ALERT_WINDOW", "REQUEST_INSTALL_PACKAGES"] {
        if let Ok(out) = adb.exec_cmd("appops", &["query-op", op, "allow"]).await {
            for pkg in out.lines().map(str::trim).filter(|l| !l.is_empty()) {
                if installed.iter().any(|p| p == pkg) {
                    dangerous_permissions.push(format!("{pkg} ({op})"));
                }
            }
        }
    }

    let wg_out = adb
        .exec_cmd("ip", &["-o", "addr", "show", "wg0"])
        .await
        .unwrap_or_default();
    let wg0_vpn_active = wg_out.contains("inet ");

    let all_pkgs = adb.exec_cmd("pm", &["list", "packages"]).await?;
    let setupwraith_intact = all_pkgs.contains(PROTECTED_SETUPWRAITH_PACKAGE);

    let (torrserve_use_disk_off, torrserve_cache_mb) = match ts.get_settings().await {
        Ok(s) => (!s.use_disk, s.cache_size / (1024 * 1024)),
        Err(_) => (false, 0),
    };

    Ok(SecurityAuditReport {
        unknown_third_party_packages,
        dangerous_permissions,
        wg0_vpn_active,
        setupwraith_intact,
        torrserve_use_disk_off,
        torrserve_cache_mb,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_video_and_dangerous_extensions() {
        assert_eq!(
            inspect_torrent_file_safety("Movie.2026.1080p.mkv"),
            FileSafetyVerdict::SafeVideo
        );
        assert_eq!(
            inspect_torrent_file_safety("trojan.apk"),
            FileSafetyVerdict::BlockedDangerousExtension("apk".to_string())
        );
    }
}
