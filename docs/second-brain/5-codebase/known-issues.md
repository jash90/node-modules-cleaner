---
title: "Known Issues"
type: known-issues
updated: 2026-06-05
tags: [codebase, bugs, gotchas]
---

# Known Issues — potwierdzone gotchas i hipotezy

> Zasada proweniencji: każdy wpis jest albo **potwierdzony** (cytat `file:line`),
> albo oznaczony jako **hipoteza — zweryfikuj w kodzie**. Nie zgaduj jako fakt.
> Projekt nie ma audytu/bug-trackera w repo — lista pochodzi z czytania kodu.

## Potwierdzone (z cytatem)

### KI-1 — Kontrakt typów Rust↔TS synchronizowany ręcznie (NAJWAŻNIEJSZE)
Structy `NodeModulesFolder` itd. (commands.rs:8-34) są lustrzanie odwzorowane w
`types.ts:1-23` bez żadnego codegenu (brak ts-rs/specta w `Cargo.toml`).
Zmiana nazwy/typu pola tylko po jednej stronie kompiluje się czysto, ale daje
`undefined` w UI. **Przy każdej zmianie struct edytuj OBA pliki.**
→ [[module-backend]], [[playbook-add-command]].

### KI-2 — Skan pomija wszystkie ukryte katalogi
`is_hidden` (commands.rs:299-304) odrzuca każdy katalog zaczynający się od `.`.
`node_modules` schowane wewnątrz dowolnego dotted-dir NIE zostaną znalezione.
Świadome zachowanie (pomijanie `.git` itp.), ale zaskakuje przy `.config` itp.

### KI-3 — Skan nie wchodzi w zagnieżdżone node_modules
`filter_entry` (commands.rs:290-296) blokuje rekursję w katalogi, których
rodzic to `node_modules`. Skutek: raportowany jest tylko zewnętrzny `node_modules`,
zagnieżdżone (np. w workspace) nie są osobnymi wpisami. Zamierzone.

### KI-4 — Komunikat usuwania ukrywa realny błąd
"Failed to delete N folder(s). Check permissions." (useNodeModules.ts:132) jest
agregatem. Prawdziwy `io::Error` z `remove_dir_all` (commands.rs:343) siedzi w
`deleteResults[].error`. Przy debugowaniu usuwania patrz tam, nie na baner.

### KI-5 — Wersja w 3 miejscach
`package.json:4`, `tauri.conf.json` (version), `src-tauri/Cargo.toml` (version) —
trzeba synchronizować ręcznie przy release. → [[playbook-release]].

### KI-6 — Sprzeczność README vs CI w sprawie podpisu
README: "app is not code-signed". CI (`release.yml`) robi fastlane match +
notaryzację macOS; `tauri.conf.json` ma `signingIdentity: "-"` (ad-hoc). Realny
stan zależy od sekretów CI. → [[adr-004-macos-signing-strategy]].

## Hipotezy — zweryfikuj w kodzie przed poleganiem

### H-1 — Rozmiary mogą się różnić od `du` (symlinki)
`calculate_dir_size` (commands.rs:37) używa `WalkDir` (domyślnie NIE podąża za
symlinkami). Przy symlinkach/hardlinkach suma może odbiegać od `du`. Zweryfikuj.

### H-2 — Etykiety KB/MB przy bazie 1024
`formatSize` (SizeDisplay.tsx:6) dzieli przez 1024 ale etykietuje KB/MB —
rozmiary mogą wyglądać inaczej niż w Finderze (baza 1000). Świadome, ale myli.

### H-3 — Path-filter na tag pushach może blokować release job
`paths:` pod `push.tags` w `release.yml` — path-filtry przy tag pushach bywają
zawodne; job może nie wystartować. Awaryjnie `workflow_dispatch`. Zweryfikuj.

### H-4 — `shell:allow-open` może być martwym uprawnieniem
Jest w `default.json`, ale plugin-shell nie jest oczywiście używany we froncie.
Zweryfikuj zanim usuniesz (mogło zostać po szablonie Tauri).

## Related
[[moc-codebase]] · [[architecture]] · [[commands/bug-triage]]
