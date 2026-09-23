---
title: "Command /bug-triage"
type: command
updated: 2026-06-05
tags: [command, dev-workflow, debugging]
---

# /bug-triage — od objawu do warstwy, reprodukcja przed hipotezą

> Najpierw odtwórz błąd na żywej apce, potem zmapuj objaw na warstwę, dopiero
> potem zgaduj przyczynę. To app desktopowa (Tauri) — błędy żyją w jednej z
> trzech warstw: Rust backend / most invoke / React front.

## Syntax
`/bug-triage <objaw>`

## Krok 1 — najpierw sprawdź znane problemy
Przeczytaj [[known-issues]] — objaw może być już udokumentowany z lokalizacją.

## Krok 2 — reprodukcja (OBOWIĄZKOWA, zanim cokolwiek zmienisz)
```bash
npm run tauri dev          # uruchom apkę w trybie dev (Rust + Vite hot reload)
```
Wybierz folder do skanu (np. `~/Projects` z realnymi `node_modules`). Otwórz
DevTools webview (prawy klik → Inspect, lub w trybie dev) → konsola pokaże
błędy frontu. Błędy Rust lecą do **terminala**, w którym odpalono `tauri dev`.

## Krok 3 — mapa objaw → warstwa → plik

| Objaw | Warstwa | Gdzie szukać |
|---|---|---|
| "command X not found" | rejestracja | `main.rs:13-17` — brak w `generate_handler!`; patrz [[commands/new-command]] |
| Pola `undefined` w UI mimo danych | kontrakt typów | snake_case mismatch `types.ts` ↔ Rust struct (commands.rs:13-20); patrz [[known-issues]] |
| Skan nie znajduje folderu | logika skanu | `is_hidden` pomija dotted-dirs (commands.rs:299-304); nie wchodzi w zagnieżdżone node_modules (filter_entry, commands.rs:290-296) |
| "Failed to delete N folder(s)" | uprawnienia / FS | `delete_folders` → `fs::remove_dir_all` (commands.rs:343); komunikat w useNodeModules.ts:132 |
| Zła wielkość folderu vs `du` | sumowanie rozmiaru | `calculate_dir_size` (commands.rs:37) — symlinki/hardlinki (hipoteza, zweryfikuj) |
| Złe wykrycie package managera | detekcja | `detect_package_manager` (commands.rs:56) — kolejność lockfile'ów ma znaczenie |
| Brak / złe badge'y technologii | detekcja deps | `KNOWN_TECH` (commands.rs:71-94), `detect_top_packages` (commands.rs:231) |
| Dialog wyboru folderu nie otwiera | plugin dialog / perms | `dialog:allow-open` w default.json; `selectDirectory` useNodeModules.ts:39 |
| Sortowanie / zaznaczenie źle | stan React | `useNodeModules` (sortedFolders:17, selectedPaths) |
| Okno za małe / layout | konfiguracja okna | `tauri.conf.json` (minWidth/minHeight 1200×800) |

## Krok 4 — naprawa
Zlokalizuj w pliku z tabeli powyżej. Zmieniasz Rust → `cargo check` najpierw.
Zmieniasz typy → pamiętaj o obu stronach kontraktu (Rust + `types.ts`).
Brak testów jednostkowych w projekcie — weryfikacja = patrz [[playbook-testing]].

## Krok 5 — Zamknij pętlę
- [ ] Przyczyna nieoczywista? → dopisz **potwierdzony** wpis (z `file:line`) do
      sekcji "Common bugs" właściwego [[moc-codebase|module]] lub do [[known-issues]]
- [ ] Ujawniła się ukryta decyzja/założenie? → `/adr`

## Related
[[playbook-bug-fix]] · [[moc-codebase]] · [[known-issues]] · [[architecture]]
