---
title: "Module: frontend-app"
type: module-map
module: frontend-app
updated: 2026-06-05
tags: [codebase, module, react, types]
---

# Module: frontend-app — powłoka UI + kontrakt typów

> Punkt wejścia frontu i miejsce gdzie żyje **kontrakt typów** współdzielony z
> backendem. `App.tsx` to cała powłoka (header/stats/lista/footer/dialog,
> ~196 linii). `types.ts` lustrzanie odwzorowuje structy Rust.

## Code map

- **Entry point**: `src/main.tsx` — `createRoot` + `<StrictMode>`, montuje `App`.
- **Powłoka UI**: `src/App.tsx` — konsumuje hook [[module-frontend-hooks|useNodeModules]],
  trzyma lokalny stan `showConfirmDialog` (App.tsx:28), komponuje
  [[module-frontend-components|komponenty]]. Sekcje: header z przyciskiem skanu,
  baner błędu, pasek statystyk, lista/empty/spinner, footer akcji, dialog.
- **Kontrakt typów**: `src/types.ts` — `TopPackage`, `NodeModulesFolder`,
  `ScanResult`, `DeleteResult` (mirror Rust, **snake_case**, types.ts:1-23) +
  typy UI `SortField`/`SortDirection`/`SortConfig` (types.ts:25-32).
- **Style**: `src/index.css` (Tailwind v4 entry).

## How to extend

- Nowa sekcja UI? → dodaj w `App.tsx`, dane bierz z hooka (nie wołaj `invoke`
  bezpośrednio z komponentu — patrz [[module-frontend-hooks]]).
- Nowy typ danych z backendu? → zacznij od `types.ts`, dbaj o snake_case (mirror Rust).

## Common bugs

- **Confirmed** (cite source):
  - `types.ts` musi ręcznie nadążać za structami Rust (commands.rs:8-34) — brak
    codegenu. Rozjazd nazwy pola = `undefined` w UI bez błędu kompilacji.
    Patrz [[known-issues]].
- *Hipotezy (zweryfikuj)*:
  - `<StrictMode>` (main.tsx) podwaja efekty w dev — przy dodawaniu `useEffect`
    pilnuj idempotencji.

## Related
[[moc-codebase]] · [[module-frontend-components]] · [[module-frontend-hooks]] · [[module-backend]]
