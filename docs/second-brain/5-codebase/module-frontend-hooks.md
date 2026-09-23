---
title: "Module: frontend-hooks"
type: module-map
module: frontend-hooks
updated: 2026-06-05
tags: [codebase, module, react, state]
---

# Module: frontend-hooks — stan + most do backendu

> Jedyny hook (`useNodeModules.ts`, 173 linie) trzyma CAŁY stan aplikacji i jest
> jedynym miejscem wołającym `invoke`. Komponenty są bezstanowe — most front↔Rust
> żyje tutaj.

## Code map (`src/hooks/useNodeModules.ts`)

- **Stan**: `folders`, `selectedPaths` (Set), `isScanning`, `isDeleting`,
  `scanPath`, `totalSize`, `error`, `sortConfig`, `deleteResults` (lines 7-15).
- **invoke → backend**:
  - `scan` → `invoke<ScanResult>('scan_for_node_modules', { path })` (line 70).
  - `deleteSelected` → `invoke<DeleteResult[]>('delete_folders', { paths })`
    (line 108); po sukcesie usuwa z listy i koryguje `totalSize` (116-127).
  - `selectDirectory` → plugin-dialog `open({ directory: true })` (line 41).
- **Logika UI**: `sortedFolders` (memo, 17-37), `toggleSelection`/`selectAll`/
  `deselectAll`, `setSort` (toggle asc/desc, 141), `selectedSize` (memo, 148).
- Zwraca `sortedFolders` jako `folders` (line 155) — komponenty dostają już posortowane.

## How to extend

- Nowa operacja backendu? → dodaj `useCallback` z `invoke<T>` tutaj, NIE w komponencie.
  Pamiętaj o całym łańcuchu: [[playbook-add-command]].
- Nowy stan? → dodaj `useState` + zwróć w obiekcie na końcu (154-172).

## Common bugs

- **Confirmed** (cite source):
  - Komunikat usuwania jest agregowany i generyczny: "Failed to delete N folder(s).
    Check permissions." (useNodeModules.ts:132) — prawdziwy błąd I/O siedzi w
    `deleteResults[].error` (zwracany przez backend, commands.rs:349-353).
- *Hipotezy (zweryfikuj)*:
  - `deleteSelected` zależy od `folders` w deps (line 139) — przy szybkim re-skanie
    w trakcie usuwania możliwy wyścig stanu. Zweryfikuj przy refaktorze.

## Related
[[moc-codebase]] · [[module-backend]] · [[module-frontend-app]] · [[playbook-add-command]]
