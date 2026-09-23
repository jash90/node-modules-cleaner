---
title: "Home — dashboard"
type: dashboard
updated: 2026-06-05
tags: [dashboard, home]
---

# Home — node-modules-cleaner Second Brain

Mózg projektu dla pracy dev (rozszerzanie / utrzymanie / naprawa) apki Tauri 2
(Rust + React 19). Instrukcja systemowa i konwencje: [[CLAUDE]].

## Chcę… → idź do

| Chcę… | Idź do |
|---|---|
| Uruchomić apkę lokalnie | [[playbook-local-dev]] |
| Dodać funkcję backendu (komendę Tauri) | `/new-command` → [[commands/new-command]] / [[playbook-add-command]] |
| Naprawić błąd | `/bug-triage` → [[commands/bug-triage]] / [[playbook-bug-fix]] |
| Zweryfikować zmianę (brak `npm test`!) | [[playbook-testing]] |
| Wydać wersję / podpisać macOS | [[playbook-release]] |
| Zrozumieć system (model + przepływ) | [[architecture]] |
| Zobaczyć znane bugi i pułapki | [[known-issues]] |
| Zapisać decyzję | `/adr` → [[commands/adr]] |
| Odświeżyć mapę kodu (drift) | `/sync-code` → [[commands/sync-code]] |
| Załadować pełny kontekst przed pracą | `/brain` |

## Mapa kodu
Entry point: [[moc-codebase]]. Moduły: [[module-backend]] ·
[[module-frontend-app]] · [[module-frontend-components]] ·
[[module-frontend-hooks]] · [[module-tauri-config]] · [[module-ci-release]].
Przekrojowo: [[architecture]] · [[known-issues]].

## Decyzje (ADR)
[[adr-001-tauri-over-electron]] · [[adr-002-rust-rayon-scanning]] ·
[[adr-003-broad-fs-permissions]] · [[adr-004-macos-signing-strategy]].
Szablon: [[adr-template]].

## Jak uruchomić
```bash
cd /Users/bartlomiejzimny/Projects/node-modules-cleaner && claude   # praca dev: slash-commandy /brain, /bug-triage, /new-command, /sync-code, /adr
cd docs/second-brain && claude                                       # praca z wiedzą: CLAUDE.md vaulta auto-ładuje się
```

## Status (przeliczony na końcu builda)

| Warstwa | Plików |
|---|---|
| 5-codebase | 9 |
| 6-playbooks | 5 |
| 7-decisions | 5 (4 ADR + szablon) |
| commands | 4 |
| root (CLAUDE/home/state) | 3 |
| **Razem .md** | **26** |

Wrappery slash-command: `.claude/commands/` (5: brain, bug-triage, new-command, sync-code, adr).

## Ostatnie akcje
- 2026-06-05 — utworzono vault (wariant cienki: bez PARA). Patrz [[state]].

## Related
[[CLAUDE]] · [[moc-codebase]] · [[architecture]]
