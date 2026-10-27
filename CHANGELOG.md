# Changelog

## 1.1.0

### Minor Changes

- 3863e92: Settings › General › Lounge as Home makes Lounge the TV's own Home: webOS starts it as
  `com.webos.app.home` for the Home button, backing out of the last app, LG ThinQ, waking the TV
  and a cold start, with no key filter or redirect. Turning it off gives LG's Home back unchanged,
  and a Lounge that cannot start gives Home back to LG by itself. Replaces the Home Button switch.
- 1a7a9b7: Show each open app's webOS snapshot on its switcher card where the TV takes them, end web apps for real when quitting from the switcher, offer Uninstall (with a confirmation alert) for apps the owner installed, and make the clock follow the TV's own 12/24-hour setting, which Settings › General › Clock format now sets.

### Patch Changes

- 886bbf8: - Behind an app Lounge does no work at all: nothing is composed, decoded or uploaded, the
  profile picture's rotation waits, and what was let go is asked for again on return.
  - Images start decoding as soon as the assets load, overlapping SDL, EGL and shader start-up;
    profile pictures are prepared on a background worker.
  - Leaving for an app frees text runs, vector meshes and glyphs no frame drew, images uploaded
    but never shown, and the CPU copy of every installed app icon (read back from the icon cache
    if ever needed again); texture memory counts compressed wallpapers at their real size.
  - Full-screen photos hide what is drawn beneath them (−15% fill on the Background page), a
    finished crossfade samples one image instead of two, text outlines use about a third fewer
    vertices, and logo meshes are a third smaller.
  - TV service calls sleep until the reply arrives instead of waking every 2 ms.
  - The status log stays bounded on long-running sessions.
- a5f2edb: Changing the wallpaper is one clean cross-fade, Settings and the home dissolve into each other without
  cutting to a sharp or blurred picture, the dock is the same lensed Liquid Glass as menus and the time
  pill, and scrolls, list focus, menus, search and text move without jumps or shimmer.
- f3a3e52: Every app on the TV has its real tvOS card: the official Apple TV App Store art where the app has
  one, otherwise its own logo laid out as tvOS does. The cards ship in the package, so they are on
  screen from the first frame. Settings shows the Settings app's own icon. Interface glyphs and logo
  meshes stay loaded behind an app, so Settings and the grid no longer stutter or flicker on return.

## 1.0.0

The first release of Lounge, a fork of [lg-luma](https://github.com/ecklf/lg-luma) 2.1.0
(its history is in [lg-luma's changelog](https://github.com/ecklf/lg-luma/blob/main/CHANGELOG.md)).

- Power-on lands on Lounge: it stays resident through standby and takes the screen at wake.
- The Home key from an app shows Lounge at once (it waited up to 2 s for the next event pump).
- LG's quick settings and other system panels no longer count as returning from an app.
- Full-screen dimming, Settings' veil and the switcher's fade are drawn inside the background
  pass: Settings to Home peaks at 1.74 screens of fill per frame instead of 2.74.
- Light, Dark and Automatic appearance; a context menu under its tile with a gliding highlight;
  profile pictures fitted to the circle; official tvOS app icons where available.
- HDMI inputs show when either of the TV's reports says a device is plugged in.
- Transparent app icons get a contrasting gradient card and clean edges.
- Reliability: preferences saved in the background and recovered when damaged, subscriptions
  that resubscribe with backoff, bounded avatar decoding, and a crash guard that never traps
  the TV in a relaunch loop.
- New name, app ID (`io.github.yashwanthkamireddi.lounge`) and icon.
