---
title: "ADR-002: Równoległy skan i liczenie rozmiarów przez Rayon"
type: adr
status: accepted
updated: 2026-06-05
tags: [adr, performance, rust]
---

# ADR-002: Skanuj i licz rozmiary równolegle (Rayon + WalkDir)

- **Status**: accepted
- **Source**: `src-tauri/src/commands.rs:37` (par_bridge), `:313` (par_iter), `:339` (into_par_iter); `Cargo.toml` (rayon = "1.10", walkdir = "2")

## Context
Folder `Projects` użytkownika może zawierać dziesiątki `node_modules`, każdy z
dziesiątkami tysięcy plików. Sekwencyjne liczenie rozmiarów (`stat` po pliku)
byłoby boleśnie wolne i blokowałoby UI.

## Decision
Użyj Rayon do zrównoleglenia trzech operacji: liczenia rozmiaru pojedynczego
katalogu (`par_bridge` na iteratorze WalkDir), liczenia rozmiarów wielu folderów
naraz (`par_iter`), oraz usuwania (`into_par_iter`). Sumę bajtów agreguj atomowo
(`AtomicU64`, commands.rs:38).

## Rejected alternatives
- **Skan sekwencyjny** — prostszy kod, ale rzędy wielkości wolniejszy na realnych
  drzewach katalogów; UI "zawiesza się" na czas skanu.
- **async/tokio z ręcznym task poolem** — większa złożoność i overhead schedulera
  dla zadania CPU/IO-bound; Rayon daje data-parallelism niemal za darmo.

## Consequences
- ✅ Szybki skan i usuwanie bez blokowania (komendy są `async`, Rust liczy na puli wątków).
- ⚠️ Kolejność iteracji jest niedeterministyczna (`par_bridge`) — nie polegaj na
  kolejności wyników; sortowanie robi front (`sortedFolders`).
- ⚠️ Pełne nasycenie CPU/dysku podczas skanu — na wolnych dyskach możliwy chwilowy
  spike obciążenia.
- ⚠️ Rozmiary mogą odbiegać od `du` przy symlinkach (WalkDir nie podąża) — patrz
  hipoteza H-1 w [[known-issues]].

## Related
[[module-backend]] · [[architecture]] · [[adr-001-tauri-over-electron]]
