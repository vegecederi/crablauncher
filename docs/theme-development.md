# Crab Theme Development Guide

Crab uses a declarative, data-driven Theme Engine powered by TOML manifests (`theme.toml`). Themes cannot execute scripts, arbitrary code, or network requests.

## Directory Structure

Place custom themes inside Crab's user data directory (`~/.local/share/crab/themes/` on Linux):

```text
themes/
└── my-custom-theme/
    ├── theme.toml
    └── preview.png (optional)
```

## Minimal Example (`theme.toml`)

```toml
schema_version = 1

[metadata]
id = "my-custom-theme"
name = "My Custom Theme"
author = "Your Name"
version = "1.0.0"
description = "A custom palette inheriting from Crab Dark."
base = "crab-dark"

[colors]
accent = "#4fc3f7"
accent_hover = "#81d4fa"
accent_pressed = "#29b6f6"
accent_muted = "#4fc3f726"

[geometry]
border_radius = 4
```

## Official Built-in Themes

- `crab-light` — Clean light palette with Crab orange accents.
- `crab-dark` — Classic dark palette with salmon-orange accents.
- `nord` — Arctic north-bluish palette.
- `legacy` — Nostalgic Alpha/Beta mossy earth palette.
- `deep-dark` — Subterranean Ancient City teal palette.

## Validation & Safety

- All colors must be valid `#RRGGBB` or `#RRGGBBAA` hex strings.
- Circular `base` inheritance is rejected automatically.
- Path traversal (`..`, absolute paths) in wallpaper or resource fields is blocked.
- Invalid or missing themes fall back safely to `crab-dark`.
