---
title: "Playbook: add-command"
type: playbook
updated: 2026-06-05
tags: [playbook, dev-workflow, tauri]
---

# Playbook: add-command — dodaj funkcję backendu wywoływaną z frontu

> Wersja wykonywalna: [[commands/new-command]] (uruchom `/new-command <nazwa>`).
> To jest 80% pracy "rozszerzającej" w tej apce: cała logika natywna (FS, skan,
> usuwanie) żyje w Rust i jest mostkowana do React przez `invoke`.

## Steps

| # | Step | Gdzie żyje prawda |
|---|---|---|
| 1 | `#[tauri::command] pub async fn ...` | `src-tauri/src/commands.rs` |
| 2 | dopisz do `use commands::{...}` | `src-tauri/src/main.rs:6` |
| 3 | dopisz do `generate_handler![...]` | `src-tauri/src/main.rs:13-17` |
| 4 | odwzoruj typ (snake_case!) | `src/types.ts` |
| 5 | `invoke<T>('nazwa', { arg })` | `src/hooks/useNodeModules.ts` |
| 6 | uprawnienie (jeśli nowe API) | `src-tauri/capabilities/default.json` |

## Co się NAPRAWDĘ zapomina (specyficzne dla projektu)

1. **Krok 3 — rejestracja w `generate_handler!`**. Bez niej `invoke` rzuci
   "command not found" dopiero w runtime (kompiluje się czysto). Klasyczna pułapka.
2. **Kontrakt typów jest synchronizowany RĘCZNIE** — brak ts-rs/specta w
   `Cargo.toml`. Pole nazwane inaczej po obu stronach = `undefined` w UI, zero
   ostrzeżeń kompilatora. Patrz [[known-issues]].
3. **serde NIE konwertuje nazw** — struktura Rust `parent_project` → w TS musi
   być `parent_project`, nie `parentProject` (commands.rs:13-20 ↔ types.ts:5-11).
4. **Argumenty invoke**: klucze obiektu JS (camelCase) Tauri mapuje na snake_case
   parametrów Rust automatycznie — ale tylko argumenty, NIE pola struktur zwrotnych.

## Related
[[commands/new-command]] · [[module-backend]] · [[module-frontend-hooks]] · [[module-frontend-app]]
