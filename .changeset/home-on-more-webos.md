---
'lounge': patch
---

Lounge as Home now works on webOS 5 and 6 and on webOS 25 and 26 (versions 10 and 11), as well as webOS 22 to 24 (versions 7 to 9). On webOS 5 and 6 it stands in for the Home the Home key opens, the full-screen Home where the TV uses that one (`home.sh enable APP_DIR com.webos.app.fullhome` picks it by hand). On webOS 25 and 26 webOS reads a Home app that points at Lounge. Anything else is refused before anything changes, and `sh /var/lib/lounge/home.sh disable` gives back LG's Home on every one of them, even when Lounge itself no longer runs. At a cold boot Lounge goes into Home's place as soon as the boot hook runs, so where the hook comes before webOS starts Home, LG's Home never shows first.
