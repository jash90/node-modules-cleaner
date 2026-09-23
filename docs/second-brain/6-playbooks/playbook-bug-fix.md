---
title: "Playbook: bug-fix"
type: playbook
updated: 2026-06-05
tags: [playbook, dev-workflow, debugging]
---

# Playbook: bug-fix — naprawa błędu od reprodukcji do write-backu

> Wersja wykonywalna: [[commands/bug-triage]] (uruchom `/bug-triage <objaw>`).
> Reprodukcja przed hipotezą, hipoteza przed naprawą.

## Steps

| # | Step | Gdzie żyje prawda |
|---|---|---|
| 1 | Sprawdź czy znane | [[known-issues]] |
| 2 | Odtwórz: `npm run tauri dev`, wybierz realny folder ze skanem | [[playbook-local-dev]] |
| 3 | Zlokalizuj warstwę (Rust / most / React) wg tabeli objawów | [[commands/bug-triage]] |
| 4 | Napraw; zmiany Rust → `cargo check` najpierw | [[module-backend]] |
| 5 | Zweryfikuj (brak testów jednostkowych — patrz niżej) | [[playbook-testing]] |
| 6 | Write-back: potwierdzony bug → "Common bugs" w module; decyzja → `/adr` | — |

## Co się NAPRAWDĘ zapomina (specyficzne dla projektu)

1. **Trzy warstwy, trzy miejsca logów** — front (DevTools webview), most invoke
   (komunikat błędu z `Result<_, String>` ląduje w banerze UI), backend (terminal
   `tauri dev`). Nie patrz tylko w konsolę przeglądarki.
2. **Komunikaty błędów są user-friendly, nie diagnostyczne** — np. "Failed to
   delete N folder(s). Check permissions." (useNodeModules.ts:132) ukrywa realny
   `io::Error` z `remove_dir_all` (commands.rs:343). Po szczegóły wejdź do
   `DeleteResult.error` w `deleteResults`.
3. **Brak `npm test`** — weryfikacja = `tsc -b` + `eslint .` + `cargo check`/
   `clippy` + manualny `tauri dev`. Nie wymyślaj testów, których nie ma.

## Related
[[commands/bug-triage]] · [[known-issues]] · [[playbook-testing]]
