---
title: "ADR-004: Podpisywanie macOS przez fastlane match w CI"
type: adr
status: accepted
updated: 2026-06-05
tags: [adr, release, ci, macos]
---

# ADR-004: Podpisuj i notaryzuj macOS przez fastlane match w CI

- **Status**: accepted
- **Source**: `.github/workflows/release.yml` (kroki "Setup keychain", "sync_certs", APPLE_* env), `src-tauri/tauri.conf.json` (signingIdentity: "-"), `fastlane/Matchfile`, `README.md` ("not code-signed")

## Context
Niepodpisana apka macOS odpala Gatekeeper (użytkownik musi robić right-click →
Open). README dokumentuje to obejście, ale dla wydań chcemy prawidłowego podpisu
i notaryzacji. Certyfikaty trzeba bezpiecznie współdzielić w CI bez trzymania ich
w repo.

## Decision
Podpisuj i notaryzuj buildy macOS w CI przez fastlane `match` (certy w prywatnym
repo git, odszyfrowywane sekretem `MATCH_PASSWORD`). Lokalnie i przy braku sekretów
`tauri.conf.json` zostaje `signingIdentity: "-"` (ad-hoc, niepodpisane).

## Rejected alternatives
- **Brak podpisu w ogóle** — zgodne z README, ale każdy użytkownik macOS musi
  ręcznie obchodzić Gatekeeper; gorszy UX dla wydań.
- **Certyfikat w repo / ręczny import** — niebezpieczne (sekret w VCS) i nieodtwarzalne
  w CI; `match` rozwiązuje to centralnie.

## Consequences
- ✅ Wydania macOS mogą być podpisane i notaryzowane (gdy sekrety CI obecne).
- ⚠️ **Sprzeczność dokumentacji**: README mówi "not code-signed", a CI podpisuje —
  zaktualizuj README albo zaznacz, że dotyczy buildów lokalnych. Patrz KI-6 w
  [[known-issues]].
- ⚠️ Build macOS w CI wymaga zestawu sekretów (`APPLE_ID`, `APPLE_TEAM_ID`,
  `MATCH_PASSWORD`, `MATCH_GIT_URL`, ...) — bez nich kroki padają.
- ⚠️ Realny stan podpisu artefaktu zależy od konfiguracji sekretów, nie od kodu.

## Related
[[module-ci-release]] · [[playbook-release]] · [[known-issues]]
