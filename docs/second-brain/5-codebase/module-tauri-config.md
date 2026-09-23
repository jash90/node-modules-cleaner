---
title: "Module: tauri-config"
type: module-map
module: tauri-config
updated: 2026-06-05
tags: [codebase, module, tauri, config, security]
---

# Module: tauri-config — konfiguracja, uprawnienia, entitlements

> Konfiguracja runtime Tauri: okno, bundling, uprawnienia (capabilities) i
> entitlements macOS. To tutaj definiuje się, co backend może robić z systemem.

## Code map

- **`src-tauri/tauri.conf.json`** — `productName`, `identifier`
  (`com.nodemodulescleaner.desktop`), wersja, okno (1200×800, minWidth/minHeight
  1200×800), `beforeDevCommand`/`devUrl` (port 5173), bundle targets `all`,
  macOS: `signingIdentity: "-"` (ad-hoc), `minimumSystemVersion: 10.13`.
  **`security.csp: null`** — brak Content Security Policy.
- **`src-tauri/capabilities/default.json`** — uprawnienia: `fs:read-all`,
  `fs:write-all`, `fs:allow-remove`, `dialog:allow-open`, `shell:allow-open`.
  Szerokie uprawnienia FS są wymagane do skanu/usuwania w dowolnym katalogu.
- **`src-tauri/entitlements.plist`** — entitlements macOS (`allow-jit`,
  `allow-unsigned-executable-memory`, `disable-library-validation`,
  `automation.apple-events`) — wymagane przez WebKit w Tauri.
- **`src-tauri/build.rs`** — standardowy `tauri_build::build()`.

## How to extend

- Nowe API natywne w komendzie? → dodaj permission do `default.json`
  (patrz [[commands/new-command]] krok 6).
- Zmiana rozmiaru/zachowania okna? → `tauri.conf.json` (sekcja `app.windows`).
- Zmiana metadanych bundla / identyfikatora? → `tauri.conf.json` + zsynchronizuj
  wersję z `package.json` i `Cargo.toml` (patrz [[playbook-release]]).

## Common bugs

- **Confirmed** (cite source):
  - `csp: null` + szerokie `fs:read-all/write-all/allow-remove` = świadomy
    kompromis bezpieczeństwa (apka MUSI usuwać dowolne katalogi). Patrz
    [[adr-003-broad-fs-permissions]].
- *Hipotezy (zweryfikuj)*:
  - `shell:allow-open` jest w uprawnieniach, ale plugin-shell nie jest oczywiście
    używany w obecnym froncie — możliwy martwy kod uprawnień. Zweryfikuj przed
    zaostrzaniem (mógł zostać po szablonie).

## Related
[[moc-codebase]] · [[module-backend]] · [[module-ci-release]] · [[adr-003-broad-fs-permissions]]
