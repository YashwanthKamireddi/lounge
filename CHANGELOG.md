# Changelog

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
