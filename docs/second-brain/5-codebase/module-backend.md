---
title: "Module: backend"
type: module-map
module: backend
updated: 2026-06-05
tags: [codebase, module, rust, tauri]
---

# Module: backend — rdzeń Rust (skan / usuwanie / detekcja)

> Cała logika natywna apki. `commands.rs` (368 linii) zawiera 3 komendy Tauri
> oraz funkcje pomocnicze do skanu FS, liczenia rozmiarów (równolegle Rayon),
> detekcji package managera i technologii (monorepo-aware). `main.rs` tylko
> buduje aplikację Tauri i rejestruje komendy. To jest centrum tego projektu.

## Code map

- **Entry point**: `src-tauri/src/main.rs` — `tauri::Builder`, pluginy
  (dialog/shell/fs, main.rs:10-12), `generate_handler!` (main.rs:13-17).
- **Komendy Tauri** (`src-tauri/src/commands.rs`):
  - `scan_for_node_modules(path)` → `ScanResult` (commands.rs:275) — WalkDir +
    `filter_entry` pomija zagnieżdżone `node_modules` i dotted-dirs (290-304),
    potem równoległe liczenie rozmiarów (`par_iter`, 313).
  - `delete_folders(paths)` → `Vec<DeleteResult>` (commands.rs:337) —
    `fs::remove_dir_all` równolegle (`into_par_iter`, 339-343).
  - `get_folder_size(path)` → `u64` (commands.rs:359).
- **Logika pomocnicza**: `calculate_dir_size` (Rayon, 37), `detect_package_manager`
  (kolejność lockfile'ów, 56), `detect_top_packages` + `KNOWN_TECH` (71-263,
  obsługa workspaces/glob), `get_parent_project` (266).
- **Kontrakt danych**: structy `TopPackage`/`NodeModulesFolder`/`ScanResult`/
  `DeleteResult` (commands.rs:8-34) — serde, **mirrorowane ręcznie** w `types.ts`.

## How to extend

- Nowa komenda? → 6 punktów rejestracji: [[commands/new-command]] / [[playbook-add-command]].
- Nowa technologia do badge'y? → dopisz krotkę do `KNOWN_TECH` (commands.rs:71-94);
  kolejność w tablicy = priorytet, lista jest `truncate(5)` (261).
- Nowy package manager? → dopisz gałąź w `detect_package_manager` (commands.rs:56);
  kolejność `if/else` ma znaczenie (bun → pnpm → yarn → npm).

## Common bugs

- **Confirmed** (cite source):
  - Kontrakt typów Rust↔TS synchronizowany ręcznie, brak codegenu — zmiana nazwy
    pola w structach (commands.rs:8-34) cicho psuje `types.ts:1-23` (brak ts-rs/specta
    w `Cargo.toml`). Patrz [[known-issues]].
  - Skan pomija WSZYSTKIE dotted-dirs (`is_hidden`, commands.rs:299-304) — np.
    `.pnpm` wewnątrz nie jest osobno przeszukiwany, a foldery w ukrytych katalogach
    nie zostaną znalezione.
  - Błędy usuwania nie przerywają operacji — zbierane per-ścieżka do `DeleteResult.error`
    (commands.rs:349-353), front agreguje jako "Check permissions".
- *Hipotezy (zweryfikuj w kodzie)*:
  - `calculate_dir_size` (WalkDir domyślny, nie podąża za symlinkami) może dać
    rozmiar inny niż `du` — szczególnie przy symlinkach/hardlinkach. Zweryfikuj.
  - `par_bridge()` na `WalkDir` (commands.rs:42) — kolejność iteracji
    niedeterministyczna, ale suma atomowa (`AtomicU64`) jest poprawna.

## Related
[[moc-codebase]] · [[module-frontend-app]] · [[module-frontend-hooks]] · [[architecture]] · [[adr-002-rust-rayon-scanning]]
