# Project Environment

- Stack: Tauri 2 desktop app with a React 19/TypeScript frontend (Vite 7) and Rust backend.
- Platforms: macOS, Windows, Linux; this is not a React Native, iOS, or Android project.
- Full development app: `npm run tauri dev` (Vite uses port 5173).
- Frontend-only development: `npm run dev`; Tauri `invoke` commands do not work in a normal browser.
- Verification: `npm run build`, `npm run lint`, `cd src-tauri && cargo test`, `cargo check`, `cargo clippy`, and `cargo fmt -- --check`.
- Packaging: `npm run tauri build`; bundles are written under `src-tauri/target/release/bundle/`.
- Package manager: npm with `package-lock.json`; Node 22 and current stable Rust are available.
- No automated E2E suite is configured. Full UI behavior is verified manually with the Tauri app.
- Release automation lives in `.github/workflows/release.yml`; there is no pull-request quality-gate workflow.
