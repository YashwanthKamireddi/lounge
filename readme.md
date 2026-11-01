<div align="center">

<img src="docs/logo.png" width="112" alt="Lounge logo: a sofa lit by the TV in a dark room">

# Lounge

**A calm, tvOS-style home screen for LG TVs. Native, instant, and out of the way.**

[![CI](https://img.shields.io/github/actions/workflow/status/YashwanthKamireddi/lounge/ci.yml?branch=main&label=CI&logo=githubactions&logoColor=white)](https://github.com/YashwanthKamireddi/lounge/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/YashwanthKamireddi/lounge?label=release&color=3b4590)](https://github.com/YashwanthKamireddi/lounge/releases/latest)
[![LG webOS 5, 6, 22 to 26](https://img.shields.io/badge/LG%20webOS-5%20%C2%B7%206%20%C2%B7%2022%E2%80%9326-a50034?logo=lg&logoColor=white)](#compatibility)
[![License: MIT](https://img.shields.io/badge/license-MIT-3b4590)](license)

[![Rust](https://img.shields.io/badge/Rust-native-000000?logo=rust&logoColor=white)](development.md)
[![OpenGL ES 2.0](https://img.shields.io/badge/OpenGL%20ES-2.0-5586a4?logo=opengl&logoColor=white)](development.md)
[![SDL2](https://img.shields.io/badge/SDL-2-1d4979)](https://github.com/webosbrew/SDL-webOS)
[![Homebrew Channel](https://img.shields.io/badge/Homebrew%20Channel-repo-6f42c1)](#install)

<img src="docs/readme/demo.webp" alt="Lounge in motion: the glass dock, the app grid and the Inputs panel on an LG TV" width="100%">

[Install](#install) · [Features](#features) · [Compatibility](#compatibility) · [Build](#build)

</div>

## Why Lounge

Lounge replaces LG's home screen with one that feels like an Apple TV: a glass dock of your apps,
nothing else competing for attention, and no ads. Measured on an LG 65UQ801C0SB:

|                      |                                                           |
| -------------------- | --------------------------------------------------------- |
| **Behind YouTube**   | holds about 25 MB, against about 113 MB for LG's own Home |
| **Cold start**       | first frame in 0.42 s                                     |
| **Home from an app** | back in about 0.1 s                                       |
| **Sitting on Home**  | wakes about once a minute (for the clock); nothing polls  |

### What makes it different

Most home screens for webOS, LG's included, are apps built on a web engine or a UI toolkit that
stays loaded the whole time. Lounge is one small native program that talks straight to the TV's
graphics chip, so there is less to load, less to keep in memory, and nothing between your button
press and the screen. Once you open an app, Lounge lets go of its pictures and goes to sleep, so
the app you watch has the TV to itself.

<details>
<summary><b>Under the hood</b>, for developers</summary>

- **Native Rust and OpenGL ES 2.0** on SDL, drawn straight to the TV's graphics plane. No web
  engine, no JavaScript runtime.
- **Event-driven, never polling:** webOS pushes app, input and window changes; worker threads
  wake the loop the moment they finish; idle, it sleeps until its next timer.
- **Cheap to draw:** glass is one shader pass over pre-blurred pictures, opaque tiles mask what
  lies behind them, frames repaint only what changed, and per-screen fill and memory budgets are
  checked in CI.
- **Fast to start:** compiled shaders kept on the TV, ASTC-compressed pictures, decoding and
  uploads on worker threads, never a file read on the drawing thread.
- **Safe as Home:** a bind mount and one boot hook (nothing written to LG's firmware), a crash
  guard that hands Home back to LG, and a plain-shell restore that works even if Lounge can't.
- **One build for every TV:** a portable ARMv8 binary; how it becomes Home is chosen from what
  each webOS version has.

[development.md](development.md) has the full architecture.

</details>

## Features

<table>
  <tr>
    <td width="50%"><img src="docs/readme/apps.jpg" alt="Home with the Inputs panel open under the Search and Inputs buttons"></td>
    <td width="50%"><img src="docs/readme/switcher.jpg" alt="The app switcher: open apps as large cards"></td>
  </tr>
  <tr>
    <td width="50%"><img src="docs/readme/menu.jpg" alt="Holding OK on an app shows Move, Hide and Uninstall under it"></td>
    <td width="50%"><img src="docs/readme/light-settings.jpg" alt="Settings › General in the Light appearance"></td>
  </tr>
  <tr>
    <td width="50%"><img src="docs/readme/grid.jpg" alt="Every app in a grid under the dock"></td>
    <td width="50%"><img src="docs/readme/search.jpg" alt="Search: a letter grid with results as you type"></td>
  </tr>
</table>

- **Liquid glass everywhere:** dock, menus, alerts and the app switcher share one material, in
  Light, Dark or Automatic.
- **Inputs one press away:** Up from the dock reaches Search and Inputs. Inputs lists Live TV and
  every HDMI port, plugged-in ones first, under LG's own names.
- **Hold OK on any app** to move, hide or uninstall it.
- **An app switcher** for what's open: OK opens, Up closes, Back returns.
- **Search** from a letter grid, with results as you type.
- **For everyone in the room:** a focus outline, larger app names and reduced motion, one switch
  each.

## Install

You need an LG TV rooted with an elevated [Homebrew Channel](https://www.webosbrew.org/).

1. In Homebrew Channel › Settings › Extra repositories, add
   `https://github.com/YashwanthKamireddi/lounge/releases/latest/download/repo.json`
2. Install **Lounge** from Homebrew Channel. Updates arrive there too.
3. Open Lounge and choose **Make Home**. From then on, the Home button opens Lounge.

Prefer a file? Install `io.github.yashwanthkamireddi.lounge_1.1.0_arm.ipk` from
[Releases](https://github.com/YashwanthKamireddi/lounge/releases/latest) with
[webOS Dev Manager](https://github.com/webosbrew/dev-manager-desktop).

**Back to LG's Home, any time:** turn off Settings › General › Lounge as Home, or run
`sh /var/lib/lounge/home.sh disable` over SSH. Nothing is written to LG's firmware, and if Lounge
ever fails to start three times in a minute, it hands Home back to LG by itself.

## Compatibility

Lounge runs on every LG TV from 2020 on: webOS 5 and 6, then webOS 22 to 26 (LG names these by
year; internally they are versions 7 to 11). It picks the right way to become Home
for each by itself. It is verified on webOS 22 (version 7.6); the others are covered by tests and
are waiting for reports. If a TV can't have Lounge as its Home, Settings says why: the
Homebrew Channel needs root, another launcher is already Home, or the webOS version isn't
supported yet. Lounge then still opens as an app.

## Privacy

No telemetry and no accounts. Settings stay on the TV; the only network requests are for the
weather (Open-Meteo, OpenStreetMap). Lounge writes no log files unless you switch diagnostics on
by creating `/var/lib/lounge/debug`. Rooting is your call, and Lounge comes without warranty
(see the [license](license)).

## Build

```sh
nix develop
pnpm install --frozen-lockfile
pnpm sdk
export WEBOS_NDK="$PWD/.build/toolchain/arm-webos-linux-gnueabi_sdk-buildroot"
pnpm run package   # dist/io.github.yashwanthkamireddi.lounge_1.1.0_arm.ipk
pnpm dev           # desktop preview with sample apps
```

[development.md](development.md) covers the architecture, tests, diagnostics and releases.

## Credits

A fork of [lg-luma](https://github.com/ecklf/lg-luma) by ecklf and contributors (MIT, notice kept
in the [license](license)). Not affiliated with LG, Apple or any app brand shown.
