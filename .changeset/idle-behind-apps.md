---
'lounge': patch
---

- Behind an app Lounge does no work at all: nothing is composed, decoded or uploaded, the
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
