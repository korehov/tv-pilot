use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;
use tokio::task::JoinSet;

use crate::adb::AdbClient;
use crate::config::{TvConfig, validate_lan_ip};
use crate::error::Result;
use crate::torrserve::TorrServeClient;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoostReport {
    pub ram_before_mb: u64,
    pub ram_after_mb: u64,
    pub freed_mb: i64,
    pub torrserve_cache_mb: u64,
    pub use_disk: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredTv {
    pub ip: String,
    pub adb_open: bool,
    pub torrserve_open: bool,
}

pub async fn boost_tv(adb: &AdbClient, ts: &TorrServeClient, config: &TvConfig) -> Result<BoostReport> {
    let before = adb.get_ram_info().await?;
    let settings = ts
        .apply_safe_ram_cache(config.safe_torrserve_cache_bytes)
        .await?;
    adb.kill_background_processes().await?;
    let after = adb.get_ram_info().await?;

    Ok(BoostReport {
        ram_before_mb: before.mem_available_mb,
        ram_after_mb: after.mem_available_mb,
        freed_mb: (after.mem_available_mb as i64) - (before.mem_available_mb as i64),
        torrserve_cache_mb: settings.cache_size / (1024 * 1024),
        use_disk: settings.use_disk,
    })
}

/// Сканирует локальную подсеть `/24` вокруг `base_ip` по портам ADB (5555) и TorrServe (8090).
pub async fn scan_lan_subnet(base_ip: &str, adb_port: u16, ts_port: u16) -> Result<Vec<DiscoveredTv>> {
    let ip = validate_lan_ip(base_ip)?;
    let [a, b, c, _] = ip.octets();
    let mut set = JoinSet::new();

    // shortcut: scans /24 around configured tv_ip with 180ms TCP timeout, upgrade to multi-NIC enumeration if multiple subnets needed
    for host in 1u8..=254 {
        let candidate = Ipv4Addr::new(a, b, c, host);
        set.spawn(async move {
            let adb_open = probe_port(candidate, adb_port).await;
            let ts_open = probe_port(candidate, ts_port).await;
            if adb_open || ts_open {
                Some(DiscoveredTv {
                    ip: candidate.to_string(),
                    adb_open,
                    torrserve_open: ts_open,
                })
            } else {
                None
            }
        });
    }

    let mut found = Vec::new();
    while let Some(Ok(Some(tv))) = set.join_next().await {
        found.push(tv);
    }
    found.sort_by(|x, y| x.ip.cmp(&y.ip));
    Ok(found)
}

async fn probe_port(ip: Ipv4Addr, port: u16) -> bool {
    let addr = SocketAddr::new(IpAddr::V4(ip), port);
    tokio::time::timeout(Duration::from_millis(180), TcpStream::connect(addr))
        .await
        .map(|r| r.is_ok())
        .unwrap_or(false)
}
