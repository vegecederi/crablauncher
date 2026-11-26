<div align="center">

<img src="assets/app/logo-full.svg" alt="Crab Launcher" width="280" />

<h3>A fast, minimalist, native Minecraft launcher built with Rust and Slint.</h3>

<p>
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/Rust-2021-orange?style=flat&logo=rust&logoColor=white" alt="Rust" /></a>
  <a href="https://slint.dev/"><img src="https://img.shields.io/badge/Slint-1.19-2379F4?style=flat" alt="Slint" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-GPL--3.0-blue?style=flat" alt="License: GPL-3.0" /></a>
  <img src="https://img.shields.io/badge/Platform-Linux-lightgrey?style=flat&logo=linux&logoColor=white" alt="Platform: Linux" />
  <a href="https://github.com/vegecederi/crablauncher/stargazers"><img src="https://img.shields.io/github/stars/vegecederi/crablauncher?style=flat&label=Stars" alt="GitHub Stars" /></a>
</p>

</div>

---

Crab Launcher is an independent reimplementation of Prism Launcher, rebuilt from the ground up with Rust and Slint, focusing on performance, flexibility, and a modern native user interface.

## Features

- **Native & Lightweight**: Built from scratch in Rust and Slint with no Electron, WebView, or browser runtime overhead.
- **Instance Management**: Isolated `.minecraft/` directories per instance, custom groups, drag-and-drop organization, instance copying, and customizable layouts.
- **Multiple Modloaders**: Built-in support for **Fabric**, **Forge**, **NeoForge**, **Quilt**, and **LiteLoader**.
- **Integrated Content Downloader**: Browse, filter, and install mods, resource packs, and shader packs directly from Modrinth, with dependency verification and update checks.
- **Automatic Java Management**: Auto-detects local Java runtimes and downloads managed runtimes from Mojang, Adoptium, Azul, or IBM Semeru.
- **Themes & Icon Packs**: Full theme engine with official light, dark, and special themes, custom theme import, and Lucide-based icon packs.
- **Account Management**: Microsoft authentication (Device Code Flow) with secure system keyring storage, plus offline accounts.
- **Advanced Instance Customization**: Per-instance memory, JVM presets, custom arguments, worlds/servers/screenshots/logs management, and Linux performance tweaks (GameMode, MangoHud, Zink, GPU selection).
- **Legacy Minecraft Support**: Compatible with Alpha, Beta, legacy, and modern Minecraft versions.
- **Customizable Settings**: Click & Apply settings persistence, custom folder paths, console history controls, and configurable download concurrency, retries, and HTTP timeouts.

## Supported Platforms

- **Linux** (primary target, XDG Base Directory compliant, Wayland & X11)

## Building from Source

### Dependencies

- **Rust** (stable toolchain)
- **Linux development libraries** required by Slint and system integrations:
  - `pkg-config`, `fontconfig`, `dbus`, `libxkbcommon`, Wayland/X11 client libraries, OpenGL/EGL

### Build & Run

```bash
git clone https://github.com/vegecederi/crablauncher.git
cd crablauncher
cargo run --release
```

### Optional Environment Variables

Copy `.env.example` to `.env` if you want to configure optional external integrations (such as Microsoft OAuth Device Code Flow or CurseForge API):

```bash
cp .env.example .env
```

## Contributing

Contributions are welcome! Please read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request.

## License

Crab Launcher is licensed under the **GNU General Public License v3.0** — see [LICENSE](LICENSE) for the full text.

Third-party libraries, icons, fonts, and brand assets are documented in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

## Disclaimer

Crab Launcher is an independent open-source project and is **not** affiliated with, endorsed by, or associated with **Mojang Studios** or **Microsoft Corporation**. Minecraft is a trademark of Mojang Synergies AB.
