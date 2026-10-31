---
'lounge': patch
---

Lounge no longer polls: background results wake it the moment they arrive, and between events it sleeps until its next timer. An idle Home wakes about once a minute (was ten times a second), and behind an app only for real events.
