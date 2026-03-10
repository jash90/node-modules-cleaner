<p align="center">
  <img src="src-tauri/icons/128x128.png" alt="Node Modules Cleaner" width="128" height="128">
</p>

<h1 align="center">Node Modules Cleaner</h1>

<p align="center">
  <strong>Reclaim your disk space by removing unused node_modules folders</strong>
</p>

<p align="center">
  <a href="https://github.com/jash90/node-modules-cleaner/releases/latest">
    <img src="https://img.shields.io/github/v/release/jash90/node-modules-cleaner?style=flat-square" alt="Latest Release">
  </a>
  <a href="https://github.com/jash90/node-modules-cleaner/blob/master/LICENSE">
    <img src="https://img.shields.io/github/license/jash90/node-modules-cleaner?style=flat-square" alt="License">
  </a>
  <a href="https://github.com/jash90/node-modules-cleaner/releases">
    <img src="https://img.shields.io/github/downloads/jash90/node-modules-cleaner/total?style=flat-square" alt="Downloads">
  </a>
</p>

---

A fast, cross-platform desktop application that scans your filesystem for `node_modules` folders and helps you delete them to free up gigabytes of disk space.

## Features

- **Parallel Scanning** — Uses Rust's Rayon for blazing-fast recursive directory scanning
- **Size Analysis** — Displays the size of each `node_modules` folder found
- **Top Packages Detection** — Identifies technologies used (React, Next, Vue, Express, etc.) from `package.json` and shows them as badges
- **Package Manager Detection** — Detects npm, yarn, pnpm, or bun based on lock files
- **Selective Deletion** — Choose exactly which folders to remove with checkboxes
- **Sorting & Filtering** — Sort by name, size, or package manager
- **Cross-Platform** — Native apps for Windows, macOS, and Linux
- **Lightweight** — Small binary size thanks to Tauri architecture

## Download

<table>
  <tr>
    <th>Platform</th>
    <th>Download</th>
    <th>Architecture</th>
  </tr>
  <tr>
    <td>🪟 Windows</td>
    <td><a href="https://github.com/jash90/node-modules-cleaner/releases/latest">Download .msi</a></td>
    <td>x64</td>
  </tr>
  <tr>
    <td>🍎 macOS</td>
    <td><a href="https://github.com/jash90/node-modules-cleaner/releases/latest">Download .dmg</a></td>
    <td>Apple Silicon / Intel</td>
  </tr>
  <tr>
    <td>🐧 Linux</td>
    <td><a href="https://github.com/jash90/node-modules-cleaner/releases/latest">Download .deb / .AppImage</a></td>
    <td>x64</td>
  </tr>
</table>

> **Note:** The app is not code-signed.
> - **macOS:** Right-click → Open to bypass Gatekeeper
> - **Windows:** Click "More info" → "Run anyway"

## How It Works

1. **Select a folder** — Choose a directory to scan (e.g., your Projects folder)
2. **Wait for scan** — The app recursively finds all `node_modules` folders
3. **Review results** — See each folder's path and size
4. **Delete selected** — Check the folders you want to remove and click Delete

## Build from Source

### Prerequisites

| Tool | Version |
|------|---------|
| [Node.js](https://nodejs.org/) | 18+ |
| [Rust](https://rustup.rs/) | Latest stable |

**Platform-specific:**
- **Linux:** `sudo apt install libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf`
- **macOS:** Xcode Command Line Tools (`xcode-select --install`)
- **Windows:** [Visual Studio C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)

### Build

```bash
git clone https://github.com/jash90/node-modules-cleaner.git
cd node-modules-cleaner
npm install
npm run tauri build
```

Binaries will be in `src-tauri/target/release/bundle/`.

### Development

```bash
npm run tauri dev
```

## Tech Stack

| Layer | Technology |
|-------|------------|
| Frontend | React 19, TypeScript, Tailwind CSS |
| Backend | Rust, Tauri 2 |
| Build | Vite 7 |

## License

[MIT](LICENSE) © Bartłomiej Zimny

## Contributing

Contributions are welcome! Feel free to open an issue or submit a pull request.
