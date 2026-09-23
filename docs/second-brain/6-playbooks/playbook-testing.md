---
title: "Playbook: testing"
type: playbook
updated: 2026-06-05
tags: [playbook, dev-workflow, quality]
---

# Playbook: testing — weryfikacja zmian

> FAKT: projekt nie ma zestawu testów jednostkowych. `package.json` ma tylko
> `dev`, `build`, `lint`, `preview`, `tauri` — nie ma skryptu `test`. Weryfikacja
> jest statyczna + manualna. Nie wymyślaj `npm test`.

## Bramki weryfikacji (uruchom przed PR/merge)

| Warstwa | Komenda | Co łapie |
|---|---|---|
| Typy TS + build front | `npm run build` (= `tsc -b && vite build`) | błędy typów, rozjazd kontraktu po stronie TS |
| Lint front | `npm run lint` (= `eslint .`) | reguły react-hooks, react-refresh (eslint.config.js) |
| Kompilacja Rust | `cd src-tauri && cargo check` | błędy typów/borrow-checker w backendzie |
| Lint Rust (zalecane) | `cd src-tauri && cargo clippy` | typowe pułapki Rust |
| End-to-end manualnie | `npm run tauri dev` | realne działanie skanu/usuwania/dialogu |

## Scenariusz manualny (smoke test)

1. Wybierz folder z kilkoma realnymi `node_modules` (np. `~/Projects`).
2. Sprawdź: rozmiary się liczą, badge package managera poprawne, badge'y
   technologii (React/Next/...) się pojawiają.
3. Zaznacz kilka, "Delete Selected" → potwierdź w dialogu → znikają z listy,
   total size maleje.
4. Test krawędziowy: pusty folder, folder bez uprawnień zapisu (oczekuj banera błędu).

## Co się NAPRAWDĘ zapomina

1. **`tsc -b` to część `build`** — sam `vite build` nie sprawdzi typów tak samo;
   używaj `npm run build`, nie `vite build`.
2. **CI buduje cały bundle** (release.yml) — to nie jest test, to release na tag
   `v*`. Lokalnie nie odpalaj pełnego `tauri build` do weryfikacji logiki — wolniej
   niż `cargo check` + `tauri dev`.

## Related
[[playbook-local-dev]] · [[playbook-bug-fix]] · [[module-ci-release]]
