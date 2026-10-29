---
'lounge': patch
---

- A press opens an app only if it went down and came up on Lounge while it was on screen and
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
