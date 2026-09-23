---
title: "Command /adr"
type: command
updated: 2026-06-05
tags: [command, dev-workflow, decisions]
---

# /adr — zapisz decyzję architektoniczną

> ADR opisuje DLACZEGO, którego kod nie powie. Każdy ADR musi mieć wszystkie
> cztery części — w tym **odrzucone alternatywy** — i cytować realny
> plik/config, który tłumaczy.

## Syntax
`/adr <temat>`

## Kiedy pisać ADR (heurystyka)
- rozwiązanie jest nieoczywiste / sprzeczne z domyślnym,
- zaskoczy kogoś za 3 miesiące,
- rozważono ≥2 sensowne opcje,
- bugfix ujawnił ukryte założenie.

## Algorytm

### Krok 1 — ustal numer
```bash
ls docs/second-brain/7-decisions/adr-*.md   # następny numer = max+1, zero-padded
```

### Krok 2 — wypełnij szablon
Skopiuj [[adr-template]] do `7-decisions/adr-NNN-<slug>.md`. Wypełnij WSZYSTKIE
cztery sekcje:
- **Context** — 2-4 konkretne zdania: jaki problem wymusił decyzję.
- **Decision** — co zdecydowano (tryb rozkazujący).
- **Rejected alternatives** — ≥1 opcja + dlaczego odrzucona. Bez tego ADR jest
  niekompletny.
- **Consequences** — zyski ✅ ORAZ koszty ⚠️ (te negatywne też!).

### Krok 3 — zacytuj źródło
W `Source:` podaj `file:line`, pole manifestu albo PR. ADR bez cytatu = porażka.

### Krok 4 — podlinkuj
Dodaj wikilinki do powiązanych notatek i wstaw ADR do listy w [[home]].

## Related
[[adr-template]] · [[home]]
