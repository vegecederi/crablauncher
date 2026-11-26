# Contributing to Crab Launcher

Thank you for your interest in contributing to **Crab Launcher**!

## Getting Started

### Prerequisites

- **Rust** (stable toolchain via [rustup](https://rustup.rs/))
- **Linux** system libraries for Slint (Wayland/X11, OpenGL/EGL, Fontconfig, D-Bus)
- **Git**

### Building Locally

```bash
git clone https://github.com/vegecederi/crablauncher.git
cd crablauncher
cargo check
cargo run
```

For release builds:

```bash
cargo build --release
```

## Development Guidelines

- Follow the architectural and UI guidelines described in `AGENTS.md`.
- Keep changes focused, minimal, and consistent with existing Rust and Slint code style.
- Format code before submitting:
  ```bash
  cargo fmt --all
  ```
- Ensure the project compiles cleanly without warnings:
  ```bash
  cargo check
  ```
- Never commit API keys, tokens, credentials, personal data, or build artifacts.

## Commit Messages

Commit messages should be written in English, concise, and start with a single relevant emoji:

- `🐛 Fix Forge library resolution`
- `⚡ Improve startup performance`
- `🎨 Refine instance grid spacing`
- `✨ Add Quilt modloader support`
- `📝 Update README`

Avoid prefixes like `feat:`, `fix:`, or `chore:`.

## Pull Requests

1. Fork the repository and create a branch from `main`.
2. Make your changes and verify them locally.
3. Open a Pull Request clearly describing what changed and why.
