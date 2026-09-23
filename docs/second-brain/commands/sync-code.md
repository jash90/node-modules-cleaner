---
title: "Command /sync-code"
type: command
updated: 2026-06-05
tags: [command, dev-workflow, maintenance]
---

# /sync-code — porównaj notatki vaulta ze stanem repo, napraw drift

> Notatki w `5-codebase/` to wskaźniki, nie kopie kodu. Z czasem rozjeżdżają
> się z rzeczywistością. Ta komenda wykrywa i naprawia rozjazd.

## Syntax
`/sync-code [scope]` — scope opcjonalny (np. `backend`, `frontend`); brak = całość

## Algorytm

### Krok 1 — re-enumeracja jednostek
```bash
ls -1 src/components src/hooks src-tauri/src
ls -1d .github fastlane src-tauri/capabilities
```
Każda jednostka MUSI mieć notatkę `5-codebase/module-*.md`. Brakująca = utwórz
(wzór: [[module-frontend-components]]). Lista kontraktowa:
`module-backend`, `module-frontend-app`, `module-frontend-components`,
`module-frontend-hooks`, `module-tauri-config`, `module-ci-release`.

### Krok 2 — weryfikacja faktów (nie ufaj, sprawdź)
Dla każdej notatki potwierdź komendą bash kluczowe twierdzenia:
```bash
# komendy Tauri zarejestrowane
grep -A4 'generate_handler' src-tauri/src/main.rs
# pola kontraktu typów
grep -E 'pub (path|size|parent_project|package_manager|top_packages)' src-tauri/src/commands.rs
# wersje stacku
grep -E '"(react|vite|@tauri-apps/api)"' package.json
grep -E '^(tauri|rayon|walkdir) ' src-tauri/Cargo.toml
```
Numery linii w notatkach (`commands.rs:NNN`) — zweryfikuj że nadal pasują.

### Krok 3 — sprawdź liczniki i integralność linków
```bash
python3 ~/.claude/skills/engineering-second-brain/scripts/check_links.py docs/second-brain
find docs/second-brain -name '*.md' | wc -l    # musi == licznik w home.md / state.md
```

### Krok 4 — napraw rozjazd
Zaktualizuj notatki, które kłamią. Jeśli zmieniła się STRUKTURA katalogów —
zaktualizuj też diagram w [[CLAUDE]] w tym samym przebiegu.

### Krok 5 — Zamknij pętlę
- [ ] Dopisz wpis do `state.md` (log operacji)
- [ ] Liczniki w `home.md` / `state.md` zaktualizuj **na samym końcu** (po find)

## Related
[[CLAUDE]] · [[moc-codebase]] · [[home]]
