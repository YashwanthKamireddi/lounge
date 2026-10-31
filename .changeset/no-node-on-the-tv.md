---
'lounge': patch
---

Lounge no longer starts the TV's Node.js, which took 4-5 seconds of CPU each time: switching Home on or off, checking it at every launch and at power-on, reading an app's icon and uninstalling an app are all done by Lounge itself now, so the apps you open from Home get the TV's full attention. `sh /var/lib/lounge/home.sh disable` still gives Home back to LG, even if Lounge itself is broken.
