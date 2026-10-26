---
name: tv-security-audit
description: >-
  Use this skill when auditing Android TV security, checking installed APKs against
  the whitelist, inspecting dangerous permissions (Accessibility, Overlay, Install Packages),
  checking WireGuard VPN (wg0), hardening TorrServe settings, or debloating the TV safely.
---

# TV External Security Shield & Safe Debloat Runbook

Используй этот навык для внешней проверки безопасности телевизора (`0 МБ` нагрузки на ОЗУ самого ТВ взамен удалённого `com.tcl.guard`).

## 1. Доверенный белый список сторонних APK (`pm list packages -3`)
Разрешённые пользовательские приложения на тестовом ТВ `172.30.0.176`:
- `net.gtvbox.videoplayer` (Just Player)
- `top.rootu.lampa` (Lampa)
- `ru.yourok.num` (NUM)
- `ru.yourok.torrserve` (TorrServe MatriX)
- `org.smarttube.stable` (SmartTube)
- `com.wireguard.android` (WireGuard VPN)
- `com.phlox.tvwebbrowser` (TV Bro)
- `de.eye_interactive.atvl.playstore` (Play Store Shortcut)
- `com.lonelycatgames.Xplore` (X-plore File Manager)
- `com.internet.speed.meter.lite` / `com.analiti.fastest.android` (тесты скорости)

Любой пакет вне этого списка помечается в отчёте `security-audit` как `UnknownThirdPartyPackage`.

## 2. Проверка опасных разрешений (`appops` / `dumpsys package`)
Проверять сторонние пакеты на наличие:
- `SYSTEM_ALERT_WINDOW` (рисование поверх всех окон / фишинговые баннеры)
- `BIND_ACCESSIBILITY_SERVICE` (перехват нажатий пульта)
- `REQUEST_INSTALL_PACKAGES` (скрытая установка APK; допустимо только для файлового менеджера / браузера / SmartTube при обновлении)

## 3. Безопасный профиль настроек `TorrServe MatriX` (`POST /settings`)
Для защиты флеш-памяти eMMC и локальной сети:
- `"UseDisk": false` (кэш строго в ОЗУ!)
- `"CacheSize": 67108864` (64 МБ ОЗУ — безопасно для 2 ГБ ТВ)
- `"ReaderReadAhead": 90`
- `"PreloadCache": 50`
- `"ForceEncrypt": true`

## 4. Защита от чёрного экрана (`FallbackHome`) при деблоате
- **КРИТИЧЕСКИЙ ЗАПРЕТ:** Никогда не отключать и не удалять `com.google.android.tungsten.setupwraith`!
  - Если он удалён до завершения регистрации Google TV, система загружается в пустой чёрный экран `com.android.tv.settings/.system.FallbackHome`.
  - Экстренное восстановление, если это произошло:
    `adb shell cmd package install-existing com.google.android.tungsten.setupwraith`
