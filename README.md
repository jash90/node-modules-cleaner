# Node Modules Cleaner

A fast, cross-platform desktop application to scan and clean `node_modules` folders from your filesystem, helping you reclaim disk space.

## Features

- **Fast Scanning** - Recursively scans directories using parallel processing (Rayon)
- **Size Calculation** - Shows the size of each `node_modules` folder found
- **Selective Deletion** - Choose which folders to delete with checkboxes
- **Cross-Platform** - Works on Windows, macOS, and Linux
- **Native Performance** - Built with Tauri (Rust backend, React frontend)

## Download

Download the latest release for your platform:

| Platform | Download |
|----------|----------|
| Windows | [.msi installer](https://github.com/jash90/node-modules-cleaner/releases/latest) |
| macOS | [.dmg installer](https://github.com/jash90/node-modules-cleaner/releases/latest) |
| Linux | [.deb](https://github.com/jash90/node-modules-cleaner/releases/latest) / [.AppImage](https://github.com/jash90/node-modules-cleaner/releases/latest) |

> **Note:** The app is not code-signed. On macOS, right-click and select "Open" to bypass Gatekeeper. On Windows, click "More info" → "Run anyway".

## Screenshots

<!-- Add screenshots here -->
*Screenshots coming soon*

## Build from Source

### Prerequisites

- [Node.js](https://nodejs.org/) (v18+)
- [Rust](https://rustup.rs/) (latest stable)
- Platform-specific dependencies:
  - **Linux:** `sudo apt install libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf`
  - **macOS:** Xcode Command Line Tools
  - **Windows:** Visual Studio C++ Build Tools

### Build Steps

```bash
# Clone the repository
git clone https://github.com/jash90/node-modules-cleaner.git
cd node-modules-cleaner

# Install dependencies
npm install

# Run in development mode
npm run tauri dev

# Build for production
npm run tauri build
```

Built binaries will be in `src-tauri/target/release/bundle/`.

## How It Works

1. Click "Select Folder" to choose a directory to scan
2. The app recursively searches for `node_modules` folders
3. Review the list and check folders you want to delete
4. Click "Delete Selected" to remove them and free up space

## Tech Stack

- **Frontend:** React 19, TypeScript, Tailwind CSS
- **Backend:** Rust, Tauri 2
- **Build:** Vite

## License

MIT License - see [LICENSE](LICENSE) for details.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.
