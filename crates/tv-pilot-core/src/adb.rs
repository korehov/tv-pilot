use std::path::{Path, PathBuf};
use std::time::Duration;
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::config::{ExecutionMode, TvConfig, validate_lan_ip};
use crate::error::{Result, TvError};
use crate::media::TvRamProfile;

/// Системный пакет, удаление или отключение которого вызывает чёрный экран FallbackHome.
pub const PROTECTED_SETUPWRAITH_PACKAGE: &str = "com.google.android.tungsten.setupwraith";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TvDeviceInfo {
    pub execution_mode: ExecutionMode,
    pub target_ip: String,
    pub model: String,
    pub device_chassis: String,
    pub android_version: String,
    pub cpu_abi: String,
    pub active_window: String,
    pub is_screensaver_or_asleep: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TvRamInfo {
    pub mem_total_mb: u64,
    pub mem_available_mb: u64,
    pub mem_free_mb: u64,
    pub cached_mb: u64,
    pub profile: TvRamProfile,
}

#[derive(Debug, Clone)]
pub struct AdbClient {
    adb_bin: PathBuf,
    serial: String,
    target_ip: String,
    mode: ExecutionMode,
}

/// Строгая валидация имени Android-пакета: только `[a-zA-Z0-9_.]`.
pub fn validate_package_name(pkg: &str) -> Result<()> {
    let trimmed = pkg.trim();
    if trimmed.is_empty()
        || !trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
    {
        return Err(TvError::InvalidPackageName(pkg.to_string()));
    }
    Ok(())
}

/// Строгая валидация компонента Activity (`package/.Activity` или `package/package.Activity`).
pub fn validate_component_name(component: &str) -> Result<()> {
    let trimmed = component.trim();
    if trimmed.is_empty()
        || !trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '/')
    {
        return Err(TvError::InvalidPackageName(component.to_string()));
    }
    Ok(())
}

/// Защита от удаления критического пакета `com.google.android.tungsten.setupwraith`.
pub fn guard_critical_system_package(pkg: &str) -> Result<()> {
    validate_package_name(pkg)?;
    if pkg.trim().eq_ignore_ascii_case(PROTECTED_SETUPWRAITH_PACKAGE) {
        return Err(TvError::CriticalSystemPackageProtected(pkg.to_string()));
    }
    Ok(())
}

fn resolve_adb_binary() -> PathBuf {
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let winget_adb = PathBuf::from(local_app_data).join(
            r"Microsoft\WinGet\Packages\Google.PlatformTools_Microsoft.Winget.Source_8wekyb3d8bbwe\platform-tools\adb.exe",
        );
        if winget_adb.exists() {
            return winget_adb;
        }
    }
    PathBuf::from("adb")
}

impl AdbClient {
    pub fn from_config(config: &TvConfig, force_on_device: bool) -> Result<Self> {
        validate_lan_ip(&config.tv_ip)?;
        Ok(Self {
            adb_bin: resolve_adb_binary(),
            serial: config.adb_serial(),
            target_ip: config.tv_ip.clone(),
            mode: config.execution_mode(force_on_device),
        })
    }

    pub fn mode(&self) -> ExecutionMode {
        self.mode
    }

    /// Проверяет и при необходимости устанавливает соединение ADB (в режиме OnDeviceLocal всегда true).
    pub async fn ensure_connected(&self) -> Result<bool> {
        if self.mode == ExecutionMode::OnDeviceLocal {
            return Ok(true);
        }

        let out = Command::new(&self.adb_bin)
            .args(["connect", &self.serial])
            .output()
            .await?;

        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        if stdout.contains("connected to") || stdout.contains("already connected") {
            Ok(true)
        } else {
            Err(TvError::AdbCommandFailed(format!(
                "Failed to connect to {}: {} {}",
                self.serial,
                stdout.trim(),
                stderr.trim()
            )))
        }
    }

    /// Выполняет команду на ТВ через массив аргументов без конкатенации шелл-строк.
    /// Работает как по сети через `adb -s <serial> shell`, так и напрямую внутри самого ТВ.
    pub async fn exec_cmd(&self, program: &str, args: &[&str]) -> Result<String> {
        match self.mode {
            ExecutionMode::RemoteAdb => {
                let mut cmd = Command::new(&self.adb_bin);
                cmd.args(["-s", &self.serial, "shell", program]);
                cmd.args(args);
                let out = cmd.output().await?;
                if !out.status.success() {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    return Err(TvError::AdbCommandFailed(format!(
                        "{} {:?} failed: {} {}",
                        program,
                        args,
                        stdout.trim(),
                        stderr.trim()
                    )));
                }
                Ok(String::from_utf8_lossy(&out.stdout).replace('\r', ""))
            }
            ExecutionMode::OnDeviceLocal => {
                let sys_bin = PathBuf::from("/system/bin").join(program);
                let bin_to_run = if sys_bin.exists() {
                    sys_bin
                } else {
                    PathBuf::from(program)
                };
                let mut cmd = Command::new(bin_to_run);
                cmd.env(
                    "PATH",
                    "/system/bin:/system/xbin:/vendor/bin:/product/bin:/sbin:/bin",
                );
                cmd.args(args);
                let out = cmd.output().await?;
                if !out.status.success() {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    return Err(TvError::AdbCommandFailed(format!(
                        "On-device {} {:?} failed: {} {}",
                        program,
                        args,
                        stdout.trim(),
                        stderr.trim()
                    )));
                }
                Ok(String::from_utf8_lossy(&out.stdout).replace('\r', ""))
            }
        }
    }

    pub async fn get_prop(&self, key: &str) -> Result<String> {
        Ok(self.exec_cmd("getprop", &[key]).await?.trim().to_string())
    }

    pub async fn get_active_window(&self) -> Result<(String, bool)> {
        let raw = self
            .exec_cmd("dumpsys", &["window", "windows"])
            .await
            .unwrap_or_default();

        let mut focus_line = String::new();
        for line in raw.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("mCurrentFocus=") {
                focus_line = trimmed.trim_start_matches("mCurrentFocus=").to_string();
                break;
            } else if focus_line.is_empty() && trimmed.starts_with("mFocusedApp=") {
                focus_line = trimmed.trim_start_matches("mFocusedApp=").to_string();
            }
        }

        let is_screensaver = focus_line.contains("dreamx")
            || focus_line.contains("DreamService")
            || focus_line.is_empty()
            || focus_line.contains("null");

        if focus_line.is_empty() {
            focus_line = "Unknown / Screen Off".to_string();
        }

        Ok((focus_line, is_screensaver))
    }

    pub async fn get_device_info(&self) -> Result<TvDeviceInfo> {
        self.ensure_connected().await?;
        let model = self.get_prop("ro.product.model").await.unwrap_or_default();
        let device_chassis = self.get_prop("ro.product.device").await.unwrap_or_default();
        let android_version = self
            .get_prop("ro.build.version.release")
            .await
            .unwrap_or_default();
        let cpu_abi = self
            .get_prop("ro.product.cpu.abi")
            .await
            .unwrap_or_default();
        let (active_window, is_screensaver_or_asleep) = self.get_active_window().await?;

        Ok(TvDeviceInfo {
            execution_mode: self.mode,
            target_ip: self.target_ip.clone(),
            model: if model.is_empty() {
                "Android TV".to_string()
            } else {
                model
            },
            device_chassis,
            android_version,
            cpu_abi,
            active_window,
            is_screensaver_or_asleep,
        })
    }

    /// Читает `/proc/meminfo` (напрямую из ФС при запуске на самом ТВ или через `cat /proc/meminfo` по ADB)
    /// и автоматически выбирает профиль ОЗУ (`Light2Gb`, `Standard3Gb`, `Ultra4GbPlus`).
    pub async fn get_ram_info(&self) -> Result<TvRamInfo> {
        let raw = if self.mode == ExecutionMode::OnDeviceLocal && Path::new("/proc/meminfo").exists()
        {
            tokio::fs::read_to_string("/proc/meminfo").await?
        } else {
            self.ensure_connected().await?;
            self.exec_cmd("cat", &["/proc/meminfo"]).await?
        };

        Ok(parse_meminfo(&raw))
    }

    /// Пробуждает телевизор из заставки `com.google.android.apps.tv.dreamx` или сна:
    /// `KEYCODE_WAKEUP` (224) -> пауза 400 мс -> `KEYCODE_BACK` (4).
    pub async fn wake_up_from_dreamx(&self) -> Result<bool> {
        let (_, is_dreamx) = self.get_active_window().await?;
        if is_dreamx {
            let _ = self.exec_cmd("input", &["keyevent", "224"]).await;
            tokio::time::sleep(Duration::from_millis(400)).await;
            let _ = self.exec_cmd("input", &["keyevent", "4"]).await;
            tokio::time::sleep(Duration::from_millis(250)).await;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Очищает фоновые процессы (`am kill-all`) для освобождения ОЗУ перед запуском фильма.
    pub async fn kill_background_processes(&self) -> Result<()> {
        let _ = self.exec_cmd("am", &["kill-all"]).await?;
        Ok(())
    }

    /// Запускает прямой видеопоток в выбранном плеере (по умолчанию `Just Player`).
    pub async fn launch_video_stream(
        &self,
        stream_url: &str,
        title: &str,
        player_activity: &str,
    ) -> Result<()> {
        validate_component_name(player_activity)?;
        let clean_title: String = title
            .chars()
            .filter(|c| !matches!(c, '\'' | '"' | '`' | '$' | ';' | '&' | '|'))
            .take(80)
            .collect();

        self.exec_cmd(
            "am",
            &[
                "start",
                "-a",
                "android.intent.action.VIEW",
                "-d",
                stream_url,
                "-t",
                "video/*",
                "-n",
                player_activity,
                "--es",
                "title",
                &clean_title,
            ],
        )
        .await?;
        Ok(())
    }
}

pub fn parse_meminfo(raw: &str) -> TvRamInfo {
    let mut total_kb = 0u64;
    let mut avail_kb = 0u64;
    let mut free_kb = 0u64;
    let mut cached_kb = 0u64;

    for line in raw.lines() {
        let mut parts = line.split_whitespace();
        let Some(key) = parts.next() else { continue };
        let Some(val_str) = parts.next() else { continue };
        let Ok(val) = val_str.parse::<u64>() else {
            continue;
        };
        match key {
            "MemTotal:" => total_kb = val,
            "MemAvailable:" => avail_kb = val,
            "MemFree:" => free_kb = val,
            "Cached:" => cached_kb = val,
            _ => {}
        }
    }

    let mem_total_mb = total_kb / 1024;
    let mem_available_mb = avail_kb / 1024;
    let mem_free_mb = free_kb / 1024;
    let cached_mb = cached_kb / 1024;
    let profile = TvRamProfile::from_mem_total_mb(mem_total_mb);

    TvRamInfo {
        mem_total_mb,
        mem_available_mb,
        mem_free_mb,
        cached_mb,
        profile,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_package_names_and_blocks_shell_injection() {
        assert!(validate_package_name("net.gtvbox.videoplayer").is_ok());
        assert!(validate_package_name("ru.yourok.torrserve").is_ok());
        assert!(validate_package_name("com.tcl.guard; rm -rf /").is_err());
        assert!(validate_package_name("pkg$(id)").is_err());
    }

    #[test]
    fn protects_critical_setupwraith_package() {
        let err = guard_critical_system_package("com.google.android.tungsten.setupwraith")
            .unwrap_err();
        assert!(matches!(err, TvError::CriticalSystemPackageProtected(_)));
    }

    #[test]
    fn parses_realtek_2gb_meminfo_accurately() {
        let sample = "\
MemTotal:        2097152 kB
MemFree:           62448 kB
MemAvailable:     754688 kB
Buffers:            2996 kB
Cached:           222616 kB
";
        let info = parse_meminfo(sample);
        assert_eq!(info.mem_total_mb, 2048);
        assert_eq!(info.mem_available_mb, 737);
        assert_eq!(info.profile, TvRamProfile::Light2Gb);
    }
}
