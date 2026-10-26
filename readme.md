<div align="center">

<img src="docs/logo.png" width="128" alt="Lounge logo: a sofa lit by the TV in a dark room">

# Lounge

**A calm, tvOS-style home screen for LG TVs.**<br>
Native, fast and quiet, so the only thing you notice is what you came to watch.

[![CI](https://github.com/YashwanthKamireddi/lounge/actions/workflows/ci.yml/badge.svg)](https://github.com/YashwanthKamireddi/lounge/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-3b4590.svg)](license)
[![webOS](https://img.shields.io/badge/LG%20webOS-7.6%20verified-1a1f45.svg)](#requirements)
[![Rust](https://img.shields.io/badge/Rust-OpenGL%20ES-1a1f45.svg)](#how-it-works)

[Features](#features) · [Tour](#a-tour) · [Why Lounge](#why-lounge) · [Install](#install) · [Make it your Home](#make-lounge-your-home) · [Make it yours](#make-it-yours) · [Build](#build-it-yourself)

<img src="docs/readme/home.jpg" alt="Lounge home screen: a glass dock of apps over the northern lights" width="100%">

</div>

## Features

- **A real tvOS home.** A glass dock of favourites over your wallpaper, an app grid that slides
  up beneath it, and your HDMI inputs as tiles.
- **Turns on into Lounge.** Power-on, the remote's Home key and backing out of an app all land on
  Lounge, with no LG Home in between.
- **Smooth on modest hardware.** Built for a one-core TV GPU: it repaints only what changed and
  never stacks full-screen layers, so menus, the switcher and Settings stay fluid.
- **Out of your apps' way.** Behind YouTube, Netflix or any other app it draws nothing and hands
  memory back, so the app you are watching keeps the TV to itself.
- **Yours.** Light, Dark or Automatic appearance, nine wallpapers, a profile picture, hidden apps,
  rearranging, and live weather in the corner.

## A tour

<table>
  <tr>
    <td width="50%"><img src="docs/readme/apps.jpg" alt="The app grid with Live TV, Settings, Search and HDMI inputs"></td>
    <td width="50%"><img src="docs/readme/switcher.jpg" alt="The app switcher: open apps as large cards"></td>
  </tr>
  <tr>
    <td align="center"><sub><b>Every app and input</b>, one press below the dock</sub></td>
    <td align="center"><sub><b>The app switcher</b>: Back on Home shows what is open</sub></td>
  </tr>
  <tr>
    <td width="50%"><img src="docs/readme/menu.jpg" alt="Holding OK on an app shows Move and Hide as glass pills against it"></td>
    <td width="50%"><img src="docs/readme/move.jpg" alt="YouTube lifted and moved one place along the dock"></td>
  </tr>
  <tr>
    <td align="center"><sub><b>Hold OK</b>: the app stays lit, its actions appear beside it</sub></td>
    <td align="center"><sub><b>Move</b>: the app lifts and slides where you want it</sub></td>
  </tr>
  <tr>
    <td width="50%"><img src="docs/readme/light-settings.jpg" alt="Settings in the Light appearance"></td>
    <td width="50%"><img src="docs/readme/search.jpg" alt="Search with a one-row keyboard"></td>
  </tr>
  <tr>
    <td align="center"><sub><b>Light</b>, <b>Dark</b> or <b>Automatic</b> by time of day</sub></td>
    <td align="center"><sub><b>Search</b> finds any app as you type</sub></td>
  </tr>
</table>

<sub>Screenshots are the desktop preview, which renders exactly what the TV draws, with sample
apps.</sub>

## Why Lounge

Most home screens made for LG TVs are web pages: HTML and CSS running inside the TV's built-in
browser. That is a fine way to build an app, but a heavy way to build the screen you see every
time the TV turns on. A browser engine has to start before anything appears, it stays in memory
while you watch, and a TV's small graphics chip struggles with the blur and layering a modern
interface needs.

Lounge is a native program instead. It is written in Rust and draws straight to the screen with
OpenGL ES, the same way a console's home screen does. On a freshly started TV it is up in about
half a second. Every frame paints only what changed, and the effects that make it look like
Apple TV, such as glass, the dim behind a menu and the fade into the app switcher, are folded
into a single pass, so the TV's GPU never repaints the screen twice.

It also knows when to step back. The moment an app opens, Lounge stops drawing and hands its
memory back: on the TV it was built on, it keeps about 16 MB of its own memory behind a playing app, so YouTube, Netflix and the rest have the TV to themselves. Press Home and it is back in under a second, usually about a tenth of one.

Taking over Home is done the way LG does it, not around it. Lounge sets the TV's own default-Home
setting and adds a small key filter of its own to LG's compositor for the Home button. It runs
no boot scripts and mounts nothing over LG's files, so turning it off is one switch in Settings,
and the TV is exactly as it was. Power-on, the Home button and backing out of an app all land on Lounge.

And it stays small on purpose: no screensaver, no video wallpapers, no background services.
What it does have, it does carefully, from the focus lift on every tile to a context menu that
keeps its app lit while the rest of the screen steps back.

## Requirements

- An LG TV running webOS, rooted with the [Homebrew Channel](https://www.webosbrew.org/)
  (elevated). Verified on an **LG 65UQ801C0SB, webOS 7.6**.
- Lounge started as a fork of [lg-luma](https://github.com/ecklf/lg-luma), which reports webOS 6
  and newer. Lounge's own changes (power-on, the Home key, memory behind apps) are tested on
  webOS 7.6 only; reports from other TVs are welcome in
  [Issues](https://github.com/YashwanthKamireddi/lounge/issues).

## Install

[Build the `.ipk`](#build-it-yourself) (ready-made packages will be on
[Releases](https://github.com/YashwanthKamireddi/lounge/releases) from the first release). Then, from
your computer, replacing `TV_IP`:

```sh
scp -O io.github.yashwanthkamireddi.lounge_1.0.0_arm.ipk root@TV_IP:/tmp/lounge.ipk
ssh root@TV_IP
```

In the TV's root shell:

```sh
luna-send -i luna://com.webos.appInstallService/dev/install \
  '{"id":"io.github.yashwanthkamireddi.lounge","ipkUrl":"/tmp/lounge.ipk","subscribe":true}'
```

[webOS Dev Manager](https://github.com/webosbrew/dev-manager-desktop) installs it with a click as
well. Open **Lounge** from the TV's app list.

## Make Lounge your Home

Open Lounge and go to **Settings › General › Home Button**, and set it to **Lounge**. From then
on the Home button, backing out of an app and turning the TV on all land on Lounge, with no LG
Home in between. Set it back to **LG Home** and the TV is exactly as it was.

The switch uses LG's own mechanisms and nothing else:

- **The TV's default Home** is set to Lounge, so leaving an app opens it.
- **A small key filter** goes into the compositor's own list of filters (configd
  `com.webos.surfacemanager.keyFilters`). LG's compositor otherwise always opens LG Home for the
  Home key, and this list is the one supported way to change that. The filter
  ([`tv/home-key-filter.js`](tv/home-key-filter.js)) catches Home only and passes every other key
  on untouched; if Lounge ever cannot open, it opens LG Home instead. If
  [Glasshouse](https://github.com/rorygallagher2024/glasshouse) already maps the Home key, its
  filter runs first and keeps it.

Nothing runs at boot that LG's compositor does not already load, and nothing is mounted over LG's
files. The switch needs the Homebrew Channel to be elevated (root); turn it back to LG Home before
uninstalling Lounge.

**Optional: keep Lounge loaded behind apps** so Home is instant. LG's memory manager (`memchute`)
reads `/home/root/memorymanager-conf.json` before its built-in `/etc/palm` copy. Adding
`io.github.yashwanthkamireddi.lounge` to `KeepOnLaunchEx`, and taking `com.webos.app.home` out of
it, keeps Lounge loaded while LG Home is freed when an app opens. It takes effect after a restart;
keep a copy of the original file to undo.

## Make it yours

**Profile picture.** Lounge shows your LG account's initial in the time pill. To use a photo
instead, copy JPEG or PNG files into Lounge's folder on the TV, then choose one in
Settings › General › Profile picture (several can take turns):

```sh
D=/media/developer/io.github.yashwanthkamireddi.lounge
mkdir -p $D/avatars && cp my-photo.jpg $D/avatars/
chown -R $(stat -c %u:%g $D) $D/avatars    # Lounge must own its folder
```

Lounge reads the folder when it starts: quit it from Settings › General and open it again.

**Top Shelf art.** Like tvOS, a picture can fill the screen behind the dock while an app is
focused. LG apps do not provide one, so Lounge ships none: put your own 16:9 images in
`local/heroes/`, named by app ID (`netflix.jpg`, `youtube.leanback.v4.jpg`), and build. The
Top Shelf Art switch appears in Settings › General once any are present.

## How it works

Lounge is a native program, not a web page: Rust on SDL2 and OpenGL ES 2, drawing straight to the
TV's graphics plane. That is what lets it start in about half a second on a fresh TV and stay
light behind your apps.

| Part                             | What it does                                                               |
| -------------------------------- | -------------------------------------------------------------------------- |
| `src/state/`                     | Focus, navigation, the switcher, rearranging and appearance                |
| `src/view/`                      | Turns state into a scene: home, Settings, menus, glass and themes          |
| `src/renderer/`                  | Draws the scene: shaders, textures, vector text, occlusion, partial redraw |
| `src/platform.rs`, `src/luna.rs` | Talks to webOS: apps, inputs, launches, power and lifecycle events         |
| `src/main.rs`                    | The `Shell`: each frame is tick → input → present, then sleep until needed |

Every full-screen effect (dimming, Settings' veil, the switcher's fade) is folded into the one
background pass instead of drawn as another layer, because a TV GPU is limited by how many pixels
it paints per frame.

## Build it yourself

### Setup & Building

From the repository root, with Nix flakes enabled:

```sh
nix develop
pnpm install --frozen-lockfile
pnpm sdk
export WEBOS_NDK="$PWD/.build/toolchain/arm-webos-linux-gnueabi_sdk-buildroot"
pnpm run package
```

The first native runtime preparation also downloads a
checksum-pinned SDL-webOS library; both downloads are cached under `.build/`.

Output:

```text
dist/io.github.yashwanthkamireddi.lounge_1.0.0_arm.ipk
dist/sha256sums
dist/repo.json
dist/app-icon-large.png
```

Open [webOS Dev Manager](https://github.com/webosbrew/dev-manager-desktop), connect to the TV's
IP on port **22** as **root**, and install the IPK. Open **Lounge** from the TV menu. Homebrew
Channel must be elevated.

Wait for installation success; Ctrl+C closes an ongoing subscription. Fully close and reopen
Lounge after updating. The [development guide](development.md#tv-startup-diagnostics) has a jailed
launch command that exposes errors hidden by the normal menu launch.

## Preview

```sh
nix develop
pnpm install --frozen-lockfile
pnpm assets
pnpm dev
```

The desktop preview uses sample inventory and never calls TV services. To manually exercise a
longer app grid:

```sh
pnpm dev --test-extra-apps 120
```

Preview preferences are separate from normal preferences. See [development.md](development.md)
for controls, vector rendering, diagnostics, tests and the source layout.

For release-worthy changes, include a file generated by `pnpm changeset` in your PR. The automated
Version Packages PR synchronizes app versions and publishes after merging and passing CI.
See [Releases](development.md#releases) for the workflow and repository setup.

## Privacy

App/input discovery uses fixed inventory commands through Homebrew Channel's elevated service.
App launches use the native Luna bus with declared permissions. There is no demo inventory
fallback on the TV. Native caller permissions and lifecycle behavior can differ by firmware.

Favorites, hidden apps, settings, weather location and cached readings stay in local JSON files
in an app-owned persistent directory outside the installed payload.
Weather search/forecast requests go to Open-Meteo, with embedded TLS roots and bounded responses.
There is no browser geolocation, IP-based location lookup, telemetry or runtime font download.

## Use at your own risk

Lounge runs on rooted TVs and changes how they start and what the Home key opens. Every change
is one setting that can be undone, and none touches LG's firmware or files, but rooting and
modifying a TV is your decision and your responsibility. Lounge is provided as is, without
warranty of any kind (see the [license](license)); its authors are not liable for any damage to
your TV, loss of data, voided warranty or anything else that comes from installing or using it.

## Credits

Lounge is a fork of [lg-luma](https://github.com/ecklf/lg-luma) by ecklf and its contributors,
under the MIT licence; their copyright notice is kept in [license](license). Thank you.

Lounge is not affiliated with LG, Apple or any app brand shown in its UI.
