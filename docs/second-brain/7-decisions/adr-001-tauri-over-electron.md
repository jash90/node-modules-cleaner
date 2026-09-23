---
title: "ADR-001: Tauri 2 zamiast Electrona jako shell desktopowy"
type: adr
status: accepted
updated: 2026-06-05
tags: [adr, architecture, tauri]
---

# ADR-001: Użyj Tauri 2 (Rust + webview), nie Electrona

- **Status**: accepted
- **Source**: `src-tauri/Cargo.toml` (tauri = "2"), `package.json` (brak electron), `README.md` (sekcja Tech Stack)

## Context
Apka to narzędzie dyskowe: musi rekursywnie skanować FS i liczyć rozmiary tysięcy
plików szybko, oraz być małym, łatwym do dystrybucji binarium na 3 platformy.
Logika ciężka I/O lepiej żyje w języku natywnym niż w Node.

## Decision
Zbuduj aplikację na Tauri 2: UI w webview (React), a logikę systemową w procesie
Rust, mostkowaną przez `invoke`. Frontend i backend współdzielą jeden repo-pakiet.

## Rejected alternatives
- **Electron** — bundluje cały Chromium (dziesiątki MB), logika FS w Node byłaby
  wolniejsza i bez łatwego współbieżnego skanu; większe binarium kłóci się z celem
  "lightweight" (README).
- **Czysto natywny GUI (Swift/WinUI/GTK)** — 3× kod UI per platforma; brak
  współdzielonego frontu; wolniejszy rozwój.

## Consequences
- ✅ Małe binarium, szybki skan (Rust + Rayon, patrz [[adr-002-rust-rayon-scanning]]),
  jeden frontend na 3 platformy.
- ⚠️ Każda operacja systemowa wymaga komendy Tauri + mostu `invoke` (nie da się
  zrobić FS z JS) — patrz [[playbook-add-command]].
- ⚠️ Wymaga toolchainu Rust + zależności systemowych WebKit (Linux) do buildu.
- ⚠️ Webview różni się per platforma (WebKit vs Chromium) — patrz target buildu
  w `vite.config.ts:18`.

## Related
[[architecture]] · [[module-backend]] · [[adr-002-rust-rayon-scanning]]
