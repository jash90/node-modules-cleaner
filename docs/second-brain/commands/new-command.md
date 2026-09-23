---
title: "Command /new-command"
type: command
updated: 2026-06-05
tags: [command, dev-workflow, tauri]
---

# /new-command — dodaj nową komendę Tauri (backend ↔ frontend)

> Dodanie funkcji Rust wywoływanej z frontu to NIE jeden plik — to łańcuch
> 6 punktów rejestracji. Pominięcie któregokolwiek = "command not found" w
> runtime albo cicho rozjechany kontrakt typów. Ta komenda wylicza wszystkie 6.

## Syntax
`/new-command <nazwa_komendy>` (snake_case, np. `move_to_trash`)

## Wywiad (zadaj sobie zanim zaczniesz)
1. Co komenda przyjmuje i zwraca? (typy — muszą mieć odpowiednik w `types.ts`)
2. Czy potrzebuje nowych uprawnień FS/dialog/shell? (jeśli tak → krok 6)
3. Czy to operacja ciężka I/O? (jeśli tak → rozważ Rayon, patrz [[module-backend]])

## Algorytm — 6 punktów rejestracji (NIC nie pomijać)

### Krok 1 — napisz funkcję w Rust
Dodaj w `src-tauri/src/commands.rs`:
```rust
#[tauri::command]
pub async fn nazwa_komendy(arg: String) -> Result<TwojTyp, String> { ... }
```
Wzór gotowych: `scan_for_node_modules` (commands.rs:275), `delete_folders`
(commands.rs:337), `get_folder_size` (commands.rs:359). Zwracaj `Result<_, String>`
albo `Vec<_>` — błędy jako `String` (front pokazuje je w banerze).

### Krok 2 — wyeksportuj i zaimportuj w main.rs
`src-tauri/src/main.rs:6` — dopisz nazwę do `use commands::{ ... }`.

### Krok 3 — zarejestruj w generate_handler!
`src-tauri/src/main.rs:13-17` — dodaj nazwę do makra
`tauri::generate_handler![...]`. **To jest punkt, który wszyscy zapominają** —
bez niego `invoke` rzuci "command nazwa_komendy not found".

### Krok 4 — odwzoruj typy w TypeScript
`src/types.ts` — dodaj interfejs odpowiadający strukturze Rust. UWAGA: pola
muszą być **snake_case** (serde nie zmienia nazw — patrz `parent_project`,
`package_manager`, `top_packages`). Brak codegenu — kontrakt synchronizujesz
ręcznie. Patrz gotcha w [[module-backend]] i [[known-issues]].

### Krok 5 — wywołaj przez invoke w hooku
`src/hooks/useNodeModules.ts` (albo nowy hook) — `invoke<TwojTyp>('nazwa_komendy', { arg })`.
Klucze argumentów w obiekcie = nazwy parametrów funkcji Rust (camelCase po
stronie JS jest auto-mapowane na snake_case Rust przez Tauri).

### Krok 6 — uprawnienia (tylko jeśli nowe API natywne)
`src-tauri/capabilities/default.json` — dodaj wpis permission jeśli komenda
sięga po nowe API pluginu (np. `fs:allow-rename`). Patrz [[module-tauri-config]].

### Krok N — Zamknij pętlę
- [ ] `cd src-tauri && cargo check` — kompiluje się Rust?
- [ ] `npm run build` (`tsc -b`) — typy się zgadzają?
- [ ] `npm run tauri dev` — komenda działa end-to-end?
- [ ] Niespodzianka w trakcie? → dopisz do "Common bugs" w [[module-backend]]
- [ ] Decyzja architektoniczna? → `/adr`

## Related
[[playbook-add-command]] · [[module-backend]] · [[module-frontend-hooks]] · [[module-tauri-config]]
