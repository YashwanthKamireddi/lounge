# Changelog

## 1.2.0

### Minor Changes

- 0cec1c0: A Search button at the top left and an Inputs button beside the time: press Up from the top row of apps to reach the one above it. Inputs lists Live TV and every HDMI input with whether something is plugged in, plugged-in ones first. Search and the inputs leave the grid, and Settings no longer has an Inputs section. Menus, the Inputs panel, the uninstall alert and Move's hint share the dock's clear glass, and hiding or uninstalling an app lets it shrink away before the others close the gap.

### Patch Changes

- bc344d0: Settings › General has Focus outline (a bold white ring around what is selected) and Larger app names, for anyone who finds the focus or the names hard to see from the sofa.
- b1567b0: An app installed, updated or removed shows in the grid at once: Lounge follows webOS's app list
  events instead of re-reading the list on the next return to Home.
- fa41857: Holding OK on an app opens Move, Hide and Uninstall again (the LG remote has no Menu key, so Hide and Uninstall were out of reach). Move's hint follows the app being moved instead of covering the dock. An app has one name everywhere. Settings pages are titled for what they show, inputs appear and disappear on their own when a device is plugged in, and the manual refresh rows and the Quit row (as the TV's Home) are gone. The app switcher says what OK, Up and Back do.
- f484277: A long Back press (the Magic Remote's Exit key) acts as Back in Lounge instead of letting LG's
  compositor close it.
- 16d1289: A cold boot shows Lounge: LG's boot manager opens the last input and never starts Home, so the boot hook opens Home over that input (or Live TV) once the boot manager has; an app someone opened meanwhile stays in front.
- 83306c9: - Without a network after power-on the TV's clock is unset: the time pill leaves the date and time
  out until the TV has set it, instead of showing 1 January.
  - A press on an app is held back only for a moment after the last one: a launch webOS answers
    late no longer swallows the presses after it.
  - LG's Settings and Search show in the app switcher with the apps.
  - Every app Lounge opens is logged with what opened it (OK, a number key, a pointer click).
- a8c42b6: The Inputs panel grows out of its button and shrinks back into it, wears the dock's glass, and keeps its button lit while open; its highlight glides with the text dark under it and light around it. Holding OK on an input names it with LG's own names (Set-Top Box, Game Console and more), which LG's menus then show too.
- e2f99cb: An app that takes more than two seconds to open says "Opening …", and after fifteen that it is taking a while, so a press never seems to do nothing. Settings, LG Content Store, Live TV, Web Browser, Media Player, Room to Room Share and Sports Alert have a new family of layered icons.
- 52ee3cc: Media Player, Room to Room Share and Sports Alert show LG's own icons large on white cards, like Web Browser. Live TV leaves the grid: it is the first source under Inputs.
- 2b8d333: Settings, Live TV, Web Browser, Media Player, Room to Room Share and Sports Alert wear LG's own icons.
- 69bb0c6: Settings has its own teal icon in LG's style, and LG's system apps and Settings sit on white cards in the Light appearance and graphite ones in Dark, crossfading as the appearance changes.
- 121e026: The tile being moved or focused is drawn above its neighbours, so in Move it passes over the others instead of under them. The README shows the app grid and Search.
- 0767097: The hold-OK menu grows out of its app as it opens and shrinks back into it as it closes, as on
  tvOS, instead of sliding.
- 78a8506: Scrolling from the dock into the app grid paints one glass platter that blends both backdrops, not two stacked layers: the busiest frame of that move paints 2.11 screens of pixels instead of 2.30, with the same look.
- e277743: An app LG's preloader started before the TV had set its clock (its page then fails its certificate
  checks, as YouTube does after a cold start) is started afresh on its first press, instead of
  opening onto that dead page.
- 489a59f: - A press opens an app only if it went down and came up on Lounge while it was on screen and
  already drawn: a key that went down on an app, the screen saver or a waking TV and comes up on
  Lounge does nothing, and neither does a key read only after the first frame of a return.
  - An app whose last launch never showed (YouTube on a slow network: webOS keeps its dead page and
    a launch only signals it) is closed and started afresh on the next press.
  - Launches and closes no longer wait behind slower TV requests; as Home, closing an app in the
    switcher calls webOS directly instead of starting a shell.
  - An app closed in the switcher, or ended by webOS, stays out of the switcher until it is opened
    again, even when LG's preloader starts it in the background.
  - A late answer to Back no longer opens the switcher over what was pressed meanwhile.
  - While the switcher fades out, the home under it takes presses at once; focus never stays on a
    notice that has gone, or on an app removed while the switcher or a menu was open.
  - Root commands as Home are bounded in time.
- 0cec1c0: Search has a letter grid beside its results (any letter within five presses), and the Settings and LG Content Store tiles have Lounge's own icons.
- 45b3310: Move shows no hint bubble: the app in hand lifts and the others make way. The Settings icon is teal, told apart from Sports Alert at a glance, and the README shows the current design.
- 6065c08: Home starts about 200 ms faster after power-on or an update: the TV keeps Lounge's compiled shaders instead of compiling them on every start.
- 8f6ca9d: A focused tile grows 9% instead of 14%: it reads as selected without looking like a different card, and in the first column it stays inside the dock.
- 2d5c78e: Home's first frame no longer waits for its TV event streams to connect (about 75 ms sooner).
- d763216: An update installed while an app is open no longer brings Home over that app: Lounge closes behind it and the new version starts on the next Home press.
- 82b34c2: With the Magic Remote's pointer hidden, the wheel moves focus a step at a time as the arrows do,
  as on LG's own Home, instead of scrolling the page away from it.

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
