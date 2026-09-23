---
title: "MOC: Codebase"
type: moc
updated: 2026-06-05
tags: [codebase, map, moc]
---

# MOC: Codebase — mapa node-modules-cleaner

> Aplikacja desktopowa Tauri 2: backend Rust (skan/usuwanie/rozmiar
> `node_modules`, równolegle przez Rayon) + frontend React 19 / TypeScript /
> Tailwind v4 budowany Vite 7. Pojedynczy pakiet, NIE monorepo.
> Punkt wejścia mentalny: [[architecture]].

## Jednostki (kontrakt pokrycia — jedna notatka na wiersz)

| Jednostka | Ścieżka | Skala | Czym jest |
|---|---|---|---|
| [[module-backend]] | `src-tauri/src/` | `commands.rs` 368 linii + `main.rs` | Rdzeń: 3 komendy Tauri, logika skanu/usuwania, detekcja PM i technologii (monorepo-aware) |
| [[module-frontend-app]] | `src/App.tsx`, `main.tsx`, `types.ts` | 3 pliki | Powłoka UI + **kontrakt typów** (mirror struktur Rust) |
| [[module-frontend-components]] | `src/components/` | 4 pliki | Komponenty prezentacyjne: lista, sortowanie, rozmiar, dialog |
| [[module-frontend-hooks]] | `src/hooks/` | `useNodeModules.ts` | Cała logika stanu + most `invoke` do backendu |
| [[module-tauri-config]] | `tauri.conf.json`, `capabilities/`, `entitlements.plist` | 3 pliki | Konfiguracja okna, uprawnienia FS/dialog/shell, entitlements macOS |
| [[module-ci-release]] | `.github/workflows/`, `fastlane/` | release.yml + 3 pliki fastlane | CI build 4 platform + podpisywanie/notaryzacja macOS |

Pominięte świadomie (zasoby, nie źródło): `public/`, `src/assets/`,
`src-tauri/icons/`, `src-tauri/gen/`, `dist/`, `target/`, `node_modules/`.

## Przekrojowe notatki

- [[architecture]] — model myślowy, przepływ żądania, tabele diagnostyczne
- [[known-issues]] — potwierdzone gotchas z cytatami + hipotezy do weryfikacji

## Stack (zweryfikowany z manifestów)

- Frontend: React 19.2, TypeScript 5.9, Tailwind v4, Vite 7 (`package.json`)
- Backend: Rust edition 2021, Tauri 2, `rayon` 1.10, `walkdir` 2, `serde`/`serde_json` (`Cargo.toml`)
- Pluginy Tauri: dialog, shell, fs (`main.rs:10-12`)

## Related
[[home]] · [[CLAUDE]] · [[architecture]]
