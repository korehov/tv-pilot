use std::net::Ipv4Addr;
use std::path::Path;
use serde::{Deserialize, Serialize};

use crate::error::{Result, TvError};

/// Режим выполнения: снаружи по Wi-Fi (ADB) или прямо внутри самого телевизора (On-Device).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionMode {
    /// Управление ТВ снаружи (с ПК или смартфона по локальной сети через ADB :5555)
    RemoteAdb,
    /// Работа нативного Rust-бинаря прямо внутри Android TV (127.0.0.1, прямой доступ к /proc и sh)
    OnDeviceLocal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TvConfig {
    pub tv_ip: String,
    pub adb_port: u16,
    pub torrserve_port: u16,
    pub default_player_package: String,
    pub default_player_activity: String,
    pub safe_torrserve_cache_bytes: u64,
    pub trusted_packages: Vec<String>,
}

impl Default for TvConfig {
    fn default() -> Self {
        let on_tv = is_running_on_android_tv();
        Self {
            tv_ip: if on_tv {
                "127.0.0.1".to_string()
            } else {
                "172.30.0.176".to_string()
            },
            adb_port: 5555,
            torrserve_port: 8090,
            default_player_package: "net.gtvbox.videoplayer".to_string(),
            default_player_activity: "net.gtvbox.videoplayer/.PlayerActivity".to_string(),
            safe_torrserve_cache_bytes: 67_108_864, // 64 MB
            trusted_packages: vec![
                "net.gtvbox.videoplayer".to_string(),
                "top.rootu.lampa".to_string(),
                "ru.yourok.num".to_string(),
                "ru.yourok.torrserve".to_string(),
                "org.smarttube.stable".to_string(),
                "com.wireguard.android".to_string(),
                "com.phlox.tvwebbrowser".to_string(),
                "de.eye_interactive.atvl.playstore".to_string(),
                "com.lonelycatgames.Xplore".to_string(),
                "com.internet.speed.meter.lite".to_string(),
                "com.analiti.fastest.android".to_string(),
            ],
        }
    }
}

/// Автоматическое определение: запущен ли бинарь прямо внутри ОС Android TV.
pub fn is_running_on_android_tv() -> bool {
    cfg!(target_os = "android")
        || (cfg!(target_os = "linux")
            && Path::new("/system/bin/sh").exists()
            && Path::new("/system/bin/pm").exists())
}

/// Строгая проверка RFC 1918 (10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16) или 127.0.0.0/8 (loopback на самом ТВ).
pub fn validate_lan_ip(ip_str: &str) -> Result<Ipv4Addr> {
    let ip: Ipv4Addr = ip_str
        .trim()
        .parse()
        .map_err(|_| TvError::InvalidIpAddress(ip_str.to_string()))?;

    if ip.is_private() || ip.is_loopback() {
        Ok(ip)
    } else {
        Err(TvError::NonPrivateIpForbidden(ip))
    }
}

impl TvConfig {
    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self> {
        let path_ref = path.as_ref();
        if !path_ref.exists() {
            let cfg = Self::default();
            cfg.validate()?;
            return Ok(cfg);
        }
        let raw = std::fs::read_to_string(path_ref)?;
        let cfg: Self = toml::from_str(&raw).map_err(|e| TvError::Config(e.to_string()))?;
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn validate(&self) -> Result<Ipv4Addr> {
        validate_lan_ip(&self.tv_ip)
    }

    pub fn execution_mode(&self, force_on_device: bool) -> ExecutionMode {
        if force_on_device
            || is_running_on_android_tv()
            || (cfg!(unix) && self.tv_ip.trim() == "127.0.0.1")
        {
            ExecutionMode::OnDeviceLocal
        } else {
            ExecutionMode::RemoteAdb
        }
    }

    pub fn adb_serial(&self) -> String {
        format!("{}:{}", self.tv_ip.trim(), self.adb_port)
    }

    pub fn torrserve_base_url(&self) -> Result<String> {
        let ip = self.validate()?;
        Ok(format!("http://{}:{}", ip, self.torrserve_port))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_rfc1918_and_loopback_ips() {
        assert!(validate_lan_ip("172.30.0.176").is_ok());
        assert!(validate_lan_ip("192.168.1.50").is_ok());
        assert!(validate_lan_ip("10.0.0.15").is_ok());
        assert!(validate_lan_ip("127.0.0.1").is_ok());
    }

    #[test]
    fn rejects_public_internet_ips() {
        let err = validate_lan_ip("8.8.8.8").unwrap_err();
        assert!(matches!(err, TvError::NonPrivateIpForbidden(_)));

        let err2 = validate_lan_ip("1.1.1.1").unwrap_err();
        assert!(matches!(err2, TvError::NonPrivateIpForbidden(_)));

        let err3 = validate_lan_ip("172.32.0.1").unwrap_err();
        assert!(matches!(err3, TvError::NonPrivateIpForbidden(_)));
    }
}
