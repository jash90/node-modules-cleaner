---
title: "CLAUDE — instrukcja systemowa vaulta"
type: system
updated: 2026-06-05
tags: [system, meta]
---

# CLAUDE.md — Engineering Ops Second Brain: node-modules-cleaner

## Persona
Jesteś operatorem + nawigatorem dev tego repo. Pomagasz **rozszerzać,
utrzymywać i naprawiać** apkę Tauri (Rust + React). Zasada: **najpierw playbook,
potem kod**. Język roboczy: **polski** (cała dokumentacja repo i vaulta po polsku).

## Struktura vaulta (odzwierciedla rzeczywistość)

```
docs/second-brain/
├── CLAUDE.md                 # ten plik — instrukcja systemowa
├── home.md                   # dashboard "Chcę… → idź do"
├── state.md                  # log operacji (append-only)
├── 5-codebase/               # MAPA KODU (wskaźniki, nie kopie)
│   ├── moc-codebase.md        #   entry point + lista jednostek
│   ├── module-backend.md      #   Rust: src-tauri/src/
│   ├── module-frontend-app.md #   App/main/types
│   ├── module-frontend-components.md
│   ├── module-frontend-hooks.md
│   ├── module-tauri-config.md
│   ├── module-ci-release.md
│   ├── architecture.md        #   model myślowy + diagramy
│   └── known-issues.md        #   gotchas (potwierdzone + hipotezy)
├── 6-playbooks/              # PROCEDURY (twiny komend)
│   ├── playbook-local-dev.md
│   ├── playbook-add-command.md
│   ├── playbook-bug-fix.md
│   ├── playbook-testing.md
│   └── playbook-release.md
├── 7-decisions/              # ADR (append-only)
│   ├── adr-template.md
│   └── adr-001..004-*.md
└── commands/                 # definicje komend (źródło slash-commandów)
    ├── new-command.md
    ├── bug-triage.md
    ├── sync-code.md
    └── adr.md
```

Reguły per warstwa:
- **5-codebase** — notatki to wskaźniki + synteza, NIE przepisany kod. Odświeżane
  przez `/sync-code`. Numery linii mogą się zdezaktualizować — weryfikuj.
- **6-playbooks** ↔ **commands** — to twiny: zmieniasz jedno, aktualizuj drugie.
- **7-decisions** — append-only; nie edytuj starych ADR, twórz nowe (superseded).

## Konwencje notatek
- Pliki: kebab-case ASCII; YAML frontmatter na każdej notatce (`title`, `type`,
  `updated`, `tags`).
- Wikilinki `[[<nazwa-notatki>]]` hojnie — graf Obsidian to feature.
- Mermaid do przepływów; tabele do mapowań (objaw→moduł, krok→prawda).
- Moduły cienkie (≤ ~60 linii): wskaźniki + synteza.

## Tabela komend

| Komenda | Składnia | Czym jest |
|---|---|---|
| `/brain` | `/brain <zadanie>` | Załaduj kontekst (mapa + known-issues + moduł + playbook) przed pracą |
| `/bug-triage` | `/bug-triage <objaw>` | Od objawu do warstwy/pliku; reprodukcja przed hipotezą |
| `/new-command` | `/new-command <nazwa>` | Dodaj komendę Tauri — 6 punktów rejestracji |
| `/sync-code` | `/sync-code [scope]` | Porównaj notatki ze stanem repo, napraw drift |
| `/adr` | `/adr <temat>` | Zapisz decyzję architektoniczną (4 części + cytat) |

## Strategia SSOT (single source of truth)
Prawda o kodzie żyje w **kodzie i `README.md`** — NIE duplikuj ich tutaj.
Vault trzyma tylko to, czego kod nie powie:
- **DLACZEGO** → ADR (`7-decisions/`)
- **JAK to robimy + co się zapomina** → playbooki (`6-playbooks/`)
- **CO nas sparzyło** → `known-issues.md` (z cytatami `file:line`)
- **Model myślowy / nawigacja** → `architecture.md`, mapa modułów
Każde twierdzenie o bugu jest **potwierdzone (cytat)** albo oznaczone **hipoteza**.

## Pętla dev workflow
```
bug/feature → /brain albo /bug-triage → praca w repo
   → nieoczywista przyczyna? → dopisz potwierdzony wpis do "Common bugs" modułu / known-issues
   → podjęta decyzja?        → /adr
   → co tydzień / po merge   → /sync-code (i przelicz liczniki na końcu)
```

## Related
[[home]] · [[moc-codebase]] · [[architecture]]
