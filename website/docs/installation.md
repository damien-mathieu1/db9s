---
sidebar_position: 2
description: "Install db9s with Homebrew, Cargo or a prebuilt binary, and the desktop app."
---

# Installation

## Homebrew (macOS, Linux)

```bash
brew install damien-mathieu1/tap/db9s
```

## Cargo

```bash
cargo install db9s
```

## Prebuilt binaries

Linux (x86_64, arm64), macOS (arm64, Intel) and Windows archives are attached to
every [release](https://github.com/damien-mathieu1/db9s/releases/latest). Extract
and put `db9s` on your `PATH`.

## Desktop app (macOS)

```bash
brew install --cask damien-mathieu1/tap/db9s-gui
```

The app isn't notarized yet (that needs an Apple Developer ID). If macOS says it
can't be opened:

```bash
xattr -dr com.apple.quarantine /Applications/db9s.app
```

See [Desktop app](desktop-app.md) for what it adds.

## From source

```bash
git clone https://github.com/damien-mathieu1/db9s
cd db9s
cargo run --release                  # the terminal UI
cargo run --release -p db9s-gui      # the desktop app, on any platform GPUI supports
```
