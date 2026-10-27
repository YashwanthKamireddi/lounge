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

Taking over Home is done the way LG does it, not around it: Lounge runs as the TV's own Home,
started by webOS with LG Home's manifest and permissions, so power-on, the Home button and
backing out of an app all land on Lounge with nothing redirected. Turning it off is one switch
in Settings, and the TV is exactly as it was.

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
scp -O io.github.yashwanthkamireddi.lounge_1.1.0_arm.ipk root@TV_IP:/tmp/lounge.ipk
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

Open Lounge and go to **Settings › General › Lounge as Home** and turn it **On**. From then on
webOS itself starts Lounge as the TV's Home: the Home button, backing out of the last app, LG
ThinQ, waking the TV and a cold start all land on Lounge, exactly as they would on LG's Home.
Turn it **Off** and LG's Home is back, unchanged.

How it works: LG's Home is one native program, `/usr/bin/com.webos.app.home`. The switch mounts
Lounge's executable over that path (a bind mount, held in memory). LG's own Home manifest,
window type and permissions stay as they are, so the TV treats Lounge as its Home on every path,
with no key filter and no redirect. LG keeps a CRIU snapshot of its Home (in `/tmp` and
`/mnt/lg/cmn_data/checkpoint`) to start it quickly. That snapshot is of LG's program and cannot
be restored over Lounge; the TV's own attempt fails and then disturbs the restores of LG's input
apps, so the switch removes it while Lounge is Home, as LG's app manager itself does after such
a failure. LG's Home writes a fresh one the next time it runs. A boot hook in the Homebrew Channel's
`/var/lib/webosbrew/init.d` puts the mount back after a full restart, and opens Lounge only if
Home was on screen, never over an input or app. Nothing else runs at boot.

If Lounge as Home ever starts three times in a minute without coming up, it gives Home back to
LG by itself. To give Home back to LG by hand, over SSH as root:

```sh
sh /var/lib/lounge/home.sh disable
```

Deleting `/var/lib/webosbrew/init.d/10-lounge-home` and restarting the TV also gives the stock
Home: the mount does not survive a restart. Turn the switch off before uninstalling Lounge. The
switch needs the Homebrew Channel to be elevated (root). It also takes out what Lounge's earlier
Home Button left behind: its compositor key filter, and the TV's default and idle Home settings
pointed at a launcher (both go back to LG's `com.webos.app.home`, which is now Lounge).

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

**App cards and Top Shelf art.** Every card and Top Shelf picture ships in the package, so it is
on screen from the first frame: `assets/tiles/` holds each app's tvOS card and `assets/heroes/`
its 16:9 Top Shelf picture, both named by app ID (`netflix.png`, `youtube.leanback.v4.jpg`). To
add or replace one for your own build, put it in `local/tiles/` or `local/heroes/` under the same
name and build; those win over the bundled ones. The Top Shelf Art switch appears in Settings ›
General once any pictures are present.

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
dist/io.github.yashwanthkamireddi.lounge_1.1.0_arm.ipk
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
is one setting that can be undone, and none writes to LG's firmware or files, but rooting and
modifying a TV is your decision and your responsibility. Lounge is provided as is, without
warranty of any kind (see the [license](license)); its authors are not liable for any damage to
your TV, loss of data, voided warranty or anything else that comes from installing or using it.

## Credits

Lounge is a fork of [lg-luma](https://github.com/ecklf/lg-luma) by ecklf and its contributors,
under the MIT licence; their copyright notice is kept in [license](license). Thank you.

Lounge is not affiliated with LG, Apple or any app brand shown in its UI.
