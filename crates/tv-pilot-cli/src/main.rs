#![forbid(unsafe_code)]

use std::path::PathBuf;
use anyhow::Result;
use clap::{Parser, Subcommand};
use serde::Serialize;
use tv_pilot_core::{
    adb::{AdbClient, TvDeviceInfo, TvRamInfo},
    config::TvConfig,
    error::TvError,
    media::{ScoredTorrentStream, StreamScorer, StreamVerdict},
    optimizer::{boost_tv, scan_lan_subnet},
    security::run_security_audit,
    torrserve::TorrServeClient,
};

#[derive(Parser)]
#[command(name = "tv-pilot", version, about = "100% Local Wi-Fi & On-Device TV Pilot for Google TV")]
struct Cli {
    #[arg(long, default_value = "tv-pilot.toml", global = true)]
    config: PathBuf,

    #[arg(long, global = true, help = "Вывод в формате JSON")]
    json: bool,

    #[arg(long, global = true, help = "Режим работы прямо внутри ОС Android TV (127.0.0.1)")]
    on_device: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Статус ТВ, профиль ОЗУ (Было/Стало), версия TorrServe и активное окно
    Status,
    /// Внешний аудит безопасности (сторонние APK, опасные разрешения, VPN wg0, кэш TorrServe)
    SecurityAudit,
    /// Список торрентов в памяти ТВ со скорингом под 2 ГБ ОЗУ
    Torrents,
    /// Разбудить ТВ, очистить ОЗУ, выбрать лучший поток без лагов и запустить в Just Player
    Watch {
        /// Поисковый запрос по названию или хеш торрента (если пусто — лучший доступный)
        query: Option<String>,
    },
    /// Ускорение в 1 кнопку: очистка фона (am kill-all) + безопасный RAM-кэш 64 МБ (UseDisk: false)
    Boost,
    /// Поиск телевизоров в домашней подсети /24 (RFC 1918) по портам 5555 и 8090
    ScanLan,
}

#[derive(Serialize)]
struct StatusOutput {
    device: TvDeviceInfo,
    ram: TvRamInfo,
    torrserve_version: String,
}

#[derive(Serialize)]
struct WatchOutput {
    woke_from_screensaver: bool,
    ram_available_mb: u64,
    selected: ScoredTorrentStream,
    stream_url: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut cfg = TvConfig::load_from_path(&cli.config)?;
    if cli.on_device {
        cfg.tv_ip = "127.0.0.1".to_string();
    }

    let adb = AdbClient::from_config(&cfg, cli.on_device)?;
    let ts = TorrServeClient::from_config(&cfg, cli.on_device)?;

    match cli.command {
        Commands::Status => {
            let device = adb.get_device_info().await?;
            let ram = adb.get_ram_info().await?;
            let torrserve_version = ts.echo().await.unwrap_or_else(|_| "Offline".to_string());
            let out = StatusOutput {
                device,
                ram,
                torrserve_version,
            };
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                println!(
                    "📺 ТВ: {} ({}, Android {}, ABI: {}, режим: {:?})",
                    out.device.model,
                    out.device.device_chassis,
                    out.device.android_version,
                    out.device.cpu_abi,
                    out.device.execution_mode
                );
                println!(
                    "🧠 Память: свободно {} МБ из {} МБ | Профиль: {}",
                    out.ram.mem_available_mb,
                    out.ram.mem_total_mb,
                    out.ram.profile.human_label()
                );
                println!("🎬 TorrServe ({}): {}", ts.base_url(), out.torrserve_version);
                println!(
                    "🪟 Экран: {} (заставка/сон: {})",
                    out.device.active_window, out.device.is_screensaver_or_asleep
                );
            }
        }
        Commands::SecurityAudit => {
            let report = run_security_audit(&adb, &ts, &cfg).await?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("🛡️ Внешний аудит безопасности ТВ ({}):", cfg.tv_ip);
                println!(
                    "  • Системный мастер setupwraith на месте: {}",
                    if report.setupwraith_intact { "✅ Да" } else { "❌ НЕТ (риск чёрного экрана!)" }
                );
                println!(
                    "  • Кэш TorrServe только в ОЗУ (UseDisk=false): {} ({} МБ)",
                    if report.torrserve_use_disk_off { "✅ Да" } else { "❌ НЕТ (износ eMMC!)" },
                    report.torrserve_cache_mb
                );
                println!(
                    "  • Туннель WireGuard (wg0): {}",
                    if report.wg0_vpn_active { "🟢 Активен" } else { "⚪ Выключен" }
                );
                if report.unknown_third_party_packages.is_empty() {
                    println!("  • Сторонние APK вне белого списка: ✅ 0 (чисто)");
                } else {
                    println!(
                        "  • ⚠️ Сторонние APK вне белого списка ({}): {}",
                        report.unknown_third_party_packages.len(),
                        report.unknown_third_party_packages.join(", ")
                    );
                }
                if !report.dangerous_permissions.is_empty() {
                    println!(
                        "  • Полномочия поверх окон / установка APK: {}",
                        report.dangerous_permissions.join(", ")
                    );
                }
            }
        }
        Commands::Torrents => {
            let ram = adb.get_ram_info().await?;
            let scorer = StreamScorer::new(ram.profile);
            let entries = ts.list_torrents().await?;
            let mut scored: Vec<ScoredTorrentStream> = entries
                .iter()
                .filter_map(|e| scorer.score_torrent(e))
                .collect();
            scored.sort_by_key(|b| std::cmp::Reverse(b.score));

            if cli.json {
                println!("{}", serde_json::to_string_pretty(&scored)?);
            } else {
                println!(
                    "🎞️ Торренты в базе ТВ ({} шт., профиль {}):",
                    scored.len(),
                    ram.profile.human_label()
                );
                for s in scored.iter().take(25) {
                    println!(
                        "  [{} | {:>3} б.] {:>5.1} ГБ (~{:>4.1} Мбит/с) | {} ({})",
                        s.verdict.human_badge(),
                        s.score,
                        s.size_gb,
                        s.estimated_bitrate_mbps,
                        s.title,
                        &s.hash[..8.min(s.hash.len())]
                    );
                }
            }
        }
        Commands::Watch { query } => {
            let woke = adb.wake_up_from_dreamx().await?;
            let _ = adb.kill_background_processes().await;
            let ram = adb.get_ram_info().await?;
            let scorer = StreamScorer::new(ram.profile);
            let q = query.unwrap_or_default().to_lowercase();

            let entries = ts.list_torrents().await?;
            let best = entries
                .iter()
                .filter(|e| {
                    q.is_empty()
                        || e.hash.to_lowercase().contains(&q)
                        || e.display_title().to_lowercase().contains(&q)
                })
                .filter_map(|e| scorer.score_torrent(e))
                .filter(|s| {
                    !matches!(
                        s.verdict,
                        StreamVerdict::TooHeavyRemux | StreamVerdict::BlockedUnsafe
                    )
                })
                .max_by(|a, b| a.score.cmp(&b.score))
                .ok_or_else(|| TvError::NoStreamFound(q.clone()))?;

            let stream_url = ts.build_stream_url(&best.hash, best.file_id, &best.file_path);
            adb.launch_video_stream(&stream_url, &best.title, &cfg.default_player_activity)
                .await?;

            let out = WatchOutput {
                woke_from_screensaver: woke,
                ram_available_mb: ram.mem_available_mb,
                selected: best,
                stream_url,
            };
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                println!(
                    "▶️ Запущен в Just Player: {} ({:.1} ГБ, {} баллов)",
                    out.selected.title, out.selected.size_gb, out.selected.score
                );
                println!("🔗 Поток: {}", out.stream_url);
            }
        }
        Commands::Boost => {
            let rep = boost_tv(&adb, &ts, &cfg).await?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&rep)?);
            } else {
                println!(
                    "🚀 Ускорение ТВ выполнено: ОЗУ Было {} МБ -> Стало {} МБ ({:+} МБ) | Кэш TorrServe: {} МБ в RAM (UseDisk={})",
                    rep.ram_before_mb,
                    rep.ram_after_mb,
                    rep.freed_mb,
                    rep.torrserve_cache_mb,
                    rep.use_disk
                );
            }
        }
        Commands::ScanLan => {
            let found = scan_lan_subnet(&cfg.tv_ip, cfg.adb_port, cfg.torrserve_port).await?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&found)?);
            } else {
                println!("📡 Найдено устройств в локальной сети ({}):", found.len());
                for tv in &found {
                    println!(
                        "  • {} (ADB :5555={}, TorrServe :8090={})",
                        tv.ip, tv.adb_open, tv.torrserve_open
                    );
                }
            }
        }
    }

    Ok(())
}
