---
title: "Architecture"
type: architecture
updated: 2026-06-05
tags: [codebase, architecture, mental-model]
---

# Architecture — model myślowy

> Apka Tauri = jeden proces Rust (backend, dostęp do systemu) + jeden webview
> (React, UI). Jedyny kanał komunikacji to **`invoke`** (front → Rust) zwracający
> JSON serializowany przez serde. Cała wrażliwa robota (FS) jest po stronie Rust.

## Inwarianty systemu

1. **Front NIGDY nie dotyka systemu plików bezpośrednio** — każdy skan/usuwanie/
   rozmiar idzie przez komendę Tauri (`invoke`). Plugin-dialog to jedyny wyjątek
   (wybór folderu), też przez most.
2. **Kontrakt typów jest jednym, ręcznie synchronizowanym źródłem prawdy w DWÓCH
   plikach** — structy Rust (commands.rs:8-34) i `types.ts:1-23` MUSZĄ się zgadzać
   co do nazw pól (snake_case). Brak codegenu. To najwrażliwszy punkt utrzymania.
3. **Stan żyje w jednym hooku** — `useNodeModules` (patrz [[module-frontend-hooks]]);
   komponenty są bezstanowe.
4. **Operacje są równoległe i odporne na częściowe błędy** — skan i usuwanie
   używają Rayon; usuwanie zbiera wynik per-ścieżka (`DeleteResult`), nie przerywa
   na pierwszym błędzie.

## Przepływ żądania (skan + usuwanie)

```mermaid
sequenceDiagram
    participant U as User
    participant A as App.tsx
    participant H as useNodeModules
    participant R as Rust (commands.rs)
    participant FS as Filesystem

    U->>A: klik "Select Folder"
    A->>H: scan()
    H->>R: plugin-dialog open() (wybór folderu)
    H->>R: invoke scan_for_node_modules(path)
    R->>FS: WalkDir + filter_entry (pomija nested/.dot)
    R->>FS: par_iter → calculate_dir_size (Rayon)
    R-->>H: ScanResult { folders, total_size }
    H-->>A: setFolders / setTotalSize
    U->>A: zaznacz + "Delete Selected" + confirm
    A->>H: deleteSelected()
    H->>R: invoke delete_folders(paths)
    R->>FS: remove_dir_all (into_par_iter)
    R-->>H: DeleteResult[] (per ścieżka)
    H-->>A: usuń z listy, koryguj totalSize
```

## Tabele diagnostyczne

**Komenda "not found" w runtime mimo że kod jest:**
1. Czy dodana do `generate_handler!`? (main.rs:13-17)
2. Czy zaimportowana w `use commands::{...}`? (main.rs:6)
3. Czy nazwa w `invoke('...')` zgadza się 1:1 z nazwą funkcji?

**Dane przychodzą z backendu, ale UI pokazuje `undefined`:**
1. snake_case mismatch pola Rust ↔ `types.ts`? (najczęstsza przyczyna)
2. Czy `invoke<T>` ma poprawny typ generyczny T?
3. Sprawdź surowy JSON w DevTools Network/console.

**Operacja FS pada / "Check permissions":**
1. Uprawnienie w `capabilities/default.json`? (np. `fs:allow-remove`)
2. Realny `io::Error` w `DeleteResult.error` (nie w generycznym banerze).
3. Na macOS — czy folder nie jest chroniony przez SIP/TCC?

## Related
[[moc-codebase]] · [[module-backend]] · [[known-issues]] · [[adr-001-tauri-over-electron]]
