---
title: "Playbook: local-dev"
type: playbook
updated: 2026-06-05
tags: [playbook, dev-workflow, setup]
---

# Playbook: local-dev — uruchomienie i praca lokalna

> Bootstrap środowiska + pułapki specyficzne dla Tauri.
> Kanoniczny przewodnik: `README.md` (sekcja Build from Source) — ten playbook
> go NIE zastępuje, dodaje skróty i listę tego, co się zapomina.

## Wymagania
- Node.js 18+ (README); Rust stable (`rustup`).
- macOS: `xcode-select --install`. Linux: `libwebkit2gtk-4.1-dev`,
  `libappindicator3-dev`, `librsvg2-dev`, `patchelf`. Windows: VS C++ Build Tools.

## Steps

| # | Step | Gdzie żyje prawda |
|---|---|---|
| 1 | `npm install` | `package.json` |
| 2 | `npm run tauri dev` — odpala Vite (port 5173) + okno Tauri | `vite.config.ts:9`, `tauri.conf.json` (devUrl) |
| 3 | Edytuj `src/**` → hot reload webview; edytuj `src-tauri/**` → recompile Rust | — |
| 4 | Logi frontu → DevTools webview; logi Rust → terminal z `tauri dev` | — |

Build produkcyjny: `npm run tauri build` → artefakty w
`src-tauri/target/release/bundle/`.

## Co się NAPRAWDĘ zapomina (specyficzne dla projektu)

1. **Port 5173 jest `strictPort: true`** (vite.config.ts:12) — jeśli zajęty,
   Vite NIE przeskoczy na inny, tylko padnie. Zwolnij port albo zabij proces.
2. **Same `npm run dev` (bez `tauri`) NIE da dostępu do `invoke`** — `window.__TAURI__`
   istnieje tylko w oknie Tauri. Skan/usuwanie zadziałają tylko pod `tauri dev`.
3. **Błędy Rust nie pojawią się w przeglądarce** — `clearScreen: false`
   (vite.config.ts:8) celowo zostawia je w terminalu. Patrz tam przy crashach backendu.
4. **Brak pliku `.env` w repo** — `envPrefix` przepuszcza tylko `VITE_`/`TAURI_`
   (vite.config.ts:16). Apka nie wymaga env do działania.
5. Minimalne okno 1200×800 (tauri.conf.json) — na małych ekranach test layoutu rób w tym rozmiarze.

## Related
[[playbook-testing]] · [[module-tauri-config]] · [[architecture]]
