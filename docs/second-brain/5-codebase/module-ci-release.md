---
title: "Module: ci-release"
type: module-map
module: ci-release
updated: 2026-06-05
tags: [codebase, module, ci, release, fastlane]
---

# Module: ci-release — CI build + podpisywanie macOS

> Pipeline wydań: GitHub Actions buduje 4 targety na push taga `v*`, fastlane
> obsługuje certy i notaryzację macOS. To support, nie kod produktu — notatka
> cienka.

## Code map

- **`.github/workflows/release.yml`** — trigger: push tag `v*` (z filtrem
  `paths:`) + `workflow_dispatch`. Matrix: macOS aarch64, macOS x86_64,
  ubuntu-22.04, windows-latest. Cache node_modules + Cargo + sccache. Buduje
  przez `tauri-apps/tauri-action@v0`, tworzy **draft** release.
- **`fastlane/Fastfile`** — lane'y `setup_ci_keychain`, `sync_certs` (match).
- **`fastlane/Matchfile`** — konfiguracja match (git-stored certs).
- **`fastlane/Gemfile`** — zależności Ruby (bundler-cache w CI).

## How to extend

- Nowy target platformy? → dodaj wpis do `matrix.include` w `release.yml`.
- Zmiana treści release notes? → blok `releaseBody` w `release.yml`.
- Pełny przepływ wydania krok-po-kroku → [[playbook-release]].

## Common bugs

- **Confirmed** (cite source):
  - Sprzeczność dokumentacji: README "not code-signed" vs CI robi fastlane
    match + notaryzację (`release.yml`, kroki macOS). Realny stan zależy od
    obecności sekretów. Patrz [[adr-004-macos-signing-strategy]].
- *Hipotezy (zweryfikuj)*:
  - Filtr `paths:` na evencie `push.tags` (release.yml) — path-filtry przy tag
    pushach bywają zawodne; job może nie wystartować. Awaryjnie `workflow_dispatch`.

## Related
[[moc-codebase]] · [[module-tauri-config]] · [[playbook-release]] · [[playbook-testing]]
