---
'lounge': patch
---

On webOS 10 and 11, Lounge as Home stays running between Home presses instead of starting afresh each time: it registers with webOS through a root helper. If a TV never brings it forward after a press, Lounge notices within 3 seconds and goes back to starting afresh, so Home always opens.
