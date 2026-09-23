---
title: "Module: frontend-components"
type: module-map
module: frontend-components
updated: 2026-06-05
tags: [codebase, module, react, ui]
---

# Module: frontend-components — komponenty prezentacyjne

> 4 komponenty czysto prezentacyjne (props in, eventy out). Bez stanu
> globalnego, bez `invoke` — dane i akcje wstrzykuje [[module-frontend-app|App.tsx]].

## Code map (`src/components/`)

- **FolderList.tsx** (133 linie) — tabela folderów: checkboxy zaznaczania,
  select-all/deselect-all, badge package managera (`BADGE_COLORS`, FolderList.tsx:4-10),
  badge'y technologii, rozmiar przez `SizeDisplay`.
- **SizeDisplay.tsx** (35 linii) — `formatSize(bytes)` (B/KB/MB/GB/TB, baza 1024) +
  kolorowanie progowe rozmiaru (>500MB czerwony, >100MB pomarańcz, >50MB żółty).
  `formatSize` jest też importowany w `App.tsx` i `ConfirmDialog.tsx`.
- **SortControls.tsx** (44 linie) — przyciski sortowania name/size/manager +
  wskaźnik kierunku ↑/↓; emituje `onSort(field)`.
- **ConfirmDialog.tsx** (74 linie) — modal potwierdzenia usunięcia (backdrop +
  liczba/rozmiar zaznaczonych), `onConfirm`/`onCancel`.

## How to extend

- Nowa kolumna w liście? → `FolderList.tsx`; dane już są w `NodeModulesFolder`.
- Nowe pole sortowania? → dodaj do `SortField` (types.ts:25), gałąź w
  `sortedFolders` (useNodeModules.ts:21-31) ORAZ przycisk w `SortControls.tsx`.
- Nowy próg/jednostka rozmiaru? → `SizeDisplay.tsx` (`formatSize` + progi kolorów).

## Common bugs

- *Hipotezy (zweryfikuj)*:
  - `formatSize` używa bazy 1024 (KiB) ale etykiet KB/MB — rozmiary mogą wyglądać
    inaczej niż w Finderze (baza 1000). Świadomy wybór, nie bug, ale zaskakuje.

## Related
[[moc-codebase]] · [[module-frontend-app]] · [[module-frontend-hooks]]
