---
title: "ADR-003: Szerokie uprawnienia FS + csp:null jako świadomy kompromis"
type: adr
status: accepted
updated: 2026-06-05
tags: [adr, security, tauri]
---

# ADR-003: Przyznaj szerokie uprawnienia FS i wyłącz CSP

- **Status**: accepted
- **Source**: `src-tauri/capabilities/default.json` (fs:read-all, fs:write-all, fs:allow-remove), `src-tauri/tauri.conf.json` (security.csp: null)

## Context
Apka z definicji musi czytać rozmiary i USUWAĆ katalogi `node_modules` w dowolnym
miejscu wybranym przez użytkownika. Tauri domyślnie ogranicza dostęp FS przez
system capabilities; bez szerokich uprawnień skan/usuwanie poza whitelistą padają.

## Decision
Przyznaj `fs:read-all`, `fs:write-all`, `fs:allow-remove` w `default.json` i
ustaw `csp: null`. Logika destrukcyjna pozostaje wyłącznie w Rust, a usuwanie jest
bramkowane dialogiem potwierdzenia w UI (`ConfirmDialog`).

## Rejected alternatives
- **Scoped FS (whitelist ścieżek)** — niemożliwe z góry: użytkownik wybiera dowolny
  katalog w runtime; nie da się przewidzieć ścieżek do whitelisty.
- **Restrykcyjne CSP** — apka nie ładuje zewnętrznych zasobów, więc CSP dawałoby
  mało, a komplikowałoby dev (inline style/Tailwind). Niski zysk.

## Consequences
- ✅ Skan i usuwanie działają w dowolnym katalogu wybranym przez użytkownika.
- ⚠️ Większa powierzchnia ataku: gdyby webview wykonał obcy kod, miałby szeroki
  dostęp FS. Mitigacja: brak ładowania zdalnych treści, cała logika FS w Rust,
  potwierdzenie usuwania w UI.
- ⚠️ `shell:allow-open` jest przyznany, choć możliwe że nieużywany — patrz
  hipoteza H-4 w [[known-issues]]; rozważ usunięcie po weryfikacji.

## Related
[[module-tauri-config]] · [[architecture]] · [[known-issues]]
