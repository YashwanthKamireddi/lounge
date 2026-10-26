# Lounge design reference (Apple HIG, tvOS)

What Apple's Human Interface Guidelines say about tvOS, and how each rule applies to Lounge.

- **Source:** Apple HIG pages on developer.apple.com, read on 2026-10-26. They were fetched through the
  JSON data the HIG site renders from (`developer.apple.com/tutorials/data/design/human-interface-guidelines/<page>.json`),
  so the wording is the same as the live page. The WWDC25 sessions "Meet Liquid Glass" (219) and "Get to
  know the new design system" (356) were read from their transcripts.
- **Conventions:**
  - Plain bullets under **Apple says** restate the page.
  - **For Lounge** bullets apply those rules to this app.
  - Anything Apple does not state is marked *(inference)*.
  - "pt" means tvOS points. Apple supplies tvOS art at @1x and @2x, and its Top Shelf guidance describes a
    screen width of "1920 pixels at the 16:9 aspect ratio". So one tvOS point is one pixel on Lounge's
    logical 1920×1080 UI plane *(inference)*, and every number below can be used directly in Lounge's
    layout units.
- **Constraints that change how the rules apply:**
  1. **No Top Shelf content from apps.** LG apps can't supply Top Shelf content. Anything above the dock
     has to come from Lounge itself (wallpaper, clock, weather).
  2. **No live blur.** The GPU is a one-core Mali, so there is no live background blur. "Glass" is a
     pre-blurred copy of the wallpaper, sampled under each glass shape.
  3. **1080p UI on a 4K panel.** The UI plane is 1920×1080, upscaled to 4K by the TV. Apple's @2x asset
     sizes would only matter for a native 4K plane. Lounge's `--resolution 4k` mode is experimental.

---

## Contents

1. [Designing for tvOS (platform principles)](#1-designing-for-tvos)
2. [Top Shelf](#2-top-shelf)
3. [Focus and selection](#3-focus-and-selection)
4. [Remotes and gestures](#4-remotes-and-gestures)
5. [Layout, safe area and grids](#5-layout-safe-area-and-grids)
6. [Materials and Liquid Glass](#6-materials-and-liquid-glass)
7. [Motion](#7-motion)
8. [Color](#8-color)
9. [Dark Mode and appearance](#9-dark-mode-and-appearance)
10. [Typography](#10-typography)
11. [App icons (tvOS layered icons)](#11-app-icons-tvos-layered-icons)
12. [Images, layered images and parallax](#12-images-layered-images-and-parallax)
13. [Collections and lockups](#13-collections-and-lockups)
14. [Menus and context menus](#14-menus-and-context-menus)
15. [Buttons](#15-buttons)
16. [Search, text fields and keyboards](#16-search-text-fields-and-keyboards)
17. [Alerts, sheets, action sheets and modality](#17-alerts-sheets-action-sheets-and-modality)
18. [Accessibility](#18-accessibility)
19. [Branding](#19-branding)
20. [Writing](#20-writing)
21. [Launching, loading and onboarding](#21-launching-loading-and-onboarding)
22. [Settings](#22-settings)
23. [Other tvOS notes (tab bars, split views, scrolling, video, accounts)](#23-other-tvos-notes)
24. [Numbers at a glance](#24-numbers-at-a-glance)
25. [What the HIG does not publish](#25-what-the-hig-does-not-publish)

---

## 1. Designing for tvOS

Source: https://developer.apple.com/design/human-interface-guidelines/designing-for-tvos

**Apple says**
- The platform has four defining traits:
  - **Display:** a very large, high-resolution screen.
  - **Ergonomics:** people sit "often 8 feet or more" away, and sometimes interact while moving around
    the room.
  - **Inputs:** remote, game controller, voice, and apps on other devices.
  - **App interactions:** long immersion (hours), plus picture-in-picture.
- People expect apps to integrate with the system: the TV app, SharePlay, Top Shelf and TV provider
  accounts.
- Best practices:
  - Use the remote's fluid, familiar gestures.
  - Embrace the focus system, "letting it gently highlight and expand onscreen items as people move among
    them, helping them know what to do and where they are at all times".
  - Show edge-to-edge artwork, subtle and fluid animation and engaging audio, legible from across the
    room.
  - Make multiuser sign-in easy and infrequent, and switch profiles automatically.

**For Lounge**
- Design every screen for 8-foot viewing: large type, few items, and one obvious focused thing.
- Focus is the entire navigation model. Every screen must always show exactly one clearly focused item
  (see §3).
- The wallpaper is Lounge's "edge-to-edge artwork". It fills the screen behind the dock and grid; nothing
  boxes it in.
- The profile picture pill is Lounge's nod to multiuser. Keep it a passive identity marker. Apple's
  guidance is about not forcing people to re-pick a profile.

---

## 2. Top Shelf

Source: https://developer.apple.com/design/human-interface-guidelines/top-shelf

### What it is
- Top Shelf is the area of the Apple TV Home Screen that "showcases your content in a rich, engaging way
  while also giving people access to their favorite apps in the Dock".
- **Full-screen Top Shelf:** people can swipe through multiple full-screen content views, play trailers
  and previews, and get more information.
- **Example:** when Apple TV is selected in the Dock, full-screen previews start playing immediately and
  "soon the Dock slides away". People swipe between featured shows and pick **Play** or **More Info**.
- The system provides layout templates, which can be downloaded from Apple Design Resources.

### Best practices (Apple's wording, condensed)
- **Help people jump right into your content.** The Carousel actions and Carousel details templates each
  have two default buttons:
  - a primary button that starts playback;
  - **More Info**, which opens the app to a detail view.
- **Feature new content:** new releases and episodes, upcoming titles. Avoid promoting content people
  have already bought, rented or watched.
- **Personalize.** Show targeted recommendations, and let people resume playback or jump back into
  gameplay.
- **Avoid showing ads or prices.** Purchasable content is fine, but show prices "only when people show
  interest".
- **Prefer dynamic content.** People prefer "a captivating, dynamic Top Shelf experience". Prefer layered
  images.
- **Static fallback.** If there's no full-screen content, supply at least one static image. The system
  shows it when the app is in the Dock and in focus. tvOS "flips and blurs the image, ensuring that it
  fits into a width of 1920 pixels at the 16:9 aspect ratio".
  - Static image size: **2320×720 pt** (2320×720 px @1x, 4640×1440 px @2x).
- **Avoid implying interactivity in a static image.** A static Top Shelf image is not focusable.

### Static vs dynamic

| | Static | Dynamic |
|---|---|---|
| Content | One image, 2320×720 pt | Carousel actions, carousel details, sectioned content row, or scrolling inset banner |
| Focusable | No | Yes (row items and banners); carousels have buttons |
| Apple's preference | Fallback only | Preferred |

### Dynamic layouts
- **Carousel actions:** full-screen video and images, with a few unobtrusive controls. Good for content
  people already know (photos, a franchise).
  - Provide a succinct **title** (show, movie or album), plus an optional brief **subtitle**, such as a
    date range or the show name.
- **Carousel details:** carousel actions plus information such as a plot summary or cast list.
  - The title identifies the currently playing content and sits near the top of the screen.
  - A short phrase or attribution (for example "Featured on My App") can sit above the title.
- **Sectioned content row:** a single labeled row of focusable items, for recently viewed, new or
  favourite content. A label appears when an item is focused, and small touch movements "bring the
  focused image to life".
  - Load enough images to span the full screen width, and include at least one label.
- **Scrolling inset banner:** near-full-width images that auto-scroll on a timer until one is focused,
  then wrap around.
  - When focused, a small circular gesture triggers the focus effect: lighting, plus 3D if the image is
    layered. Swiping pans between banners.
  - Provide **3–8 images**. Fewer than three feels ineffective; more than eight is hard to navigate.
  - There are no labels, so all text goes inside the image, ideally on a raised layer. Repeat that text in
    the accessibility label.

### Image sizes (sectioned content row and banners)

| Shape | Actual size | Focused / safe zone | Unfocused |
|---|---|---|---|
| Poster 2:3 | 404×608 pt (808×1216 @2x) | 380×570 pt (760×1140 @2x) | 333×570 pt (666×1140 @2x) |
| Square 1:1 | 608×608 pt (1216×1216 @2x) | 570×570 pt (1140×1140 @2x) | 500×500 pt (1000×1000 @2x) |
| 16:9 | 908×512 pt (1816×1024 @2x) | 852×479 pt (1704×958 @2x) | 782×440 pt (1564×880 @2x) |
| Scrolling inset banner | 1940×692 pt (3880×1384 @2x) | 1740×620 pt (3480×1240 @2x) | 1740×560 pt (3480×1120 @2x) |

- **Safe zones:**
  - The "actual" size is the full canvas. The "focused / safe zone" size is what stays visible when the
    item is focused.
  - Keep essential content inside the safe-zone rectangle. The extra margin is cropped or used for
    parallax movement *(inference from the table structure)*.
- **Mixed sizes:** images in one row scale up to the height of the tallest image. For example, a 16:9
  image becomes 500 px high in a row that also has posters or squares.
- **Focused/unfocused ratios.** These ratios are computed from the table *(inference)*; Apple does not
  publish a focus scale.
  - Square: 570/500 ≈ **1.14×**.
  - 16:9: 852/782 ≈ **1.09×**.
  - Banner width: 1740 → 1740, i.e. 1.0×; the height grows 560 → 620.
- Platform: Top Shelf is tvOS-only.

### What belongs in Top Shelf, in short
New, featured, recommended or resumable content. Direct paths into it (Play, More Info). No ads, no
prices up front. Prefer dynamic content; a static image is the fallback.

### For Lounge
- **LG apps can't supply Top Shelf content**: there is no webOS API for per-app shelf media. So Lounge has
  no real Top Shelf, and must not fake one with stock imagery claiming to be an app's content.
- The space above the dock belongs to the **wallpaper**. That matches the static fallback Apple
  describes: a full-width, non-focusable image that never implies interactivity.
  - The clock, date and weather pill sits there as quiet information. It must not look focusable.
- If Lounge ever shows per-app art above the dock (for example an app's own banner from its `appinfo`),
  treat it as Apple's static image: not focusable, no text that looks like a button, and fitted to the
  1920-wide frame.
- Apple's Dock "slides away" when full-screen previews play. Lounge has no previews, so the dock stays.
  Don't add autoplaying video to imitate this: it would compete with the app the person is about to
  open and cost GPU time.
- The sectioned-row sizes above are a reference for future content rows (for example "Continue
  watching", if webOS ever exposes it). Use their scale ratios and keep spacing large enough for the
  focused size.

---

## 3. Focus and selection

Source: https://developer.apple.com/design/human-interface-guidelines/focus-and-selection

**Apple says**
- **Focus and selection are separate.** tvOS separates focus from selection: people move focus to find
  an item, and "selection requires a separate gesture".
- **Parallax is the focus treatment.** tvOS "generally uses the Parallax effect to give the focused item
  an appearance of depth and liveliness".
- **Prefer system focus effects;** create custom ones "only if it's absolutely necessary".
- **Don't move focus without user input.** The exception is directional input (remote, keyboard,
  controller): if the focused item disappears, move focus to an item within one discrete step of it.
  Without directional input, hide the indicator instead.
- **Everything must be focusable.** tvOS users reach every element with directional input.
- **Ring vs highlight.** Generally, use a focus ring for text and search fields, and a highlight for
  lists and collections.
- **In full-screen views, gestures act on the content,** not on focus. Full-screen items don't show
  focus.
- **Avoid a pointer.** People navigate "a fixed number of items by changing focus", not by dragging a
  tiny pointer.
- **Five focus states, each visually distinct.** Supply assets for the larger focused size so they stay
  sharp, and make sure the enlarged item doesn't crowd its neighbours.

| State | Meaning |
|---|---|
| Unfocused | Less prominent than focused items |
| Focused | Stands out through "elevation to the foreground, illumination, and animation" |
| Highlighted | Instant feedback on press (for example a button "might briefly invert its colors and animate") |
| Selected | Chosen or activated (for example a filled heart) |
| Unavailable | Can't be focused or chosen; looks inactive |

**For Lounge**
- **Implement all five states** for dock tiles, grid tiles, HDMI tiles, menu rows and keyboard keys.
  Lounge's renderer can't do Apple's live parallax cheaply, so express focus with:
  - scale up;
  - lift: a deeper shadow, plus a slight offset toward the viewer *(inference: Apple doesn't give a
    shadow spec)*;
  - illumination: a brighter rim or highlight.
- **Pressed (highlighted):** an immediate, brief response on OK-down, such as a quick scale-down or
  brighten, before the launch. It must appear on the next frame, without waiting for the app to start.
- **Never move focus on its own.** Apple's rule covers two Lounge cases:
  - Hiding an app or unplugging an HDMI source removes the focused tile. Move focus to the adjacent tile
    in the same row, not back to the start.
  - When Lounge returns from an app, restore focus to the tile that launched it (see §21, "restore
    previous state").
- **No pointer**, even though LG Magic Remotes have one. If pointer events arrive, either map them to
  focus or ignore them *(Lounge decision; HIG: "If your app requires a pointer, make sure it's highly
  visible")*.
- **Text fields** (the search field) use a ring or outline; **tiles and menu rows** use a highlight with
  scale.
- **Leave room for the focused size:** grid gaps must absorb the scale-up without overlapping (§5).
  Render or rasterise icon art at the **focused** size so it isn't upscaled blurry (§12).

---

## 4. Remotes and gestures

Sources:
- https://developer.apple.com/design/human-interface-guidelines/remotes
- https://developer.apple.com/design/human-interface-guidelines/gestures

**Apple says**
- **Use standard gestures for standard actions.** Unless the app is a game, people expect the remote to
  behave the same in every app.
- **Focus follows gestures:** always move focus in the same direction as the gesture.
- **Give clear feedback for gestures.**
- **Define new gestures only when they make sense** (for example in games).
- **Press vs tap:** a press is intentional (choose, confirm). A tap is fine for navigation but can be
  accidental, so ignore taps during live video.
- **Back** opens "the parent of the current screen". At an app's top level the parent is the Home
  Screen.
  - Within an app, the parent comes from the hierarchy and is "not necessarily the previous screen".
  - Holding Back goes to the Home Screen from anywhere.
  - In games, Back opens a pause menu.
- **Play/Pause** controls media playback.
- **Swipe** scrolls with momentum: fast first, then slowing. Edge swipes speed through items.
- **Press** activates or goes deeper.
- **Gestures table:** "Touch (or pinch) and hold" is listed for tvOS with the meaning "Reveal additional
  controls or functionality".
- **Compatible remotes:** guide and page buttons should open or page through an EPG, and page up/down
  changes channel during playback.

| Button | Expected in an app |
|---|---|
| Touch surface swipe | Navigates; changes focus |
| Touch surface press | Activates a control or item; navigates deeper |
| Back | Returns to previous screen; exits to Home Screen |
| Play/Pause | Starts, pauses or resumes media playback |

**For Lounge (LG remote: D-pad, OK, Back, Home, colour keys, Magic Remote wheel/pointer)**
- **D-pad = swipe, OK = press.** Moves are discrete, one step per key, so the focus animation must keep
  up with key auto-repeat. Holding a direction should feel like Apple's "speed through items".
- **Hold OK = "touch and hold"** reveals the context menu (Move / Hide). This matches Apple's gesture
  table. Use a clear hold threshold. A normal OK press must never open the menu, and a hold must never
  launch the app.
- **Back is hierarchical:**
  - Closes the menu → closes Settings or Search → leaves the grid for the dock.
  - At Lounge's top level, Back opens the **app switcher**. This is a Lounge choice: Lounge *is* the Home
    Screen, so it has no parent to return to. The HIG doesn't cover a launcher's Back.
- **Home always returns to Lounge's dock** with the wallpaper, like holding Back on Apple TV.
- **Play/Pause:** Lounge plays no media, so it has no role here. Don't repurpose it for launcher actions
  (Apple: don't redefine standard behaviors).
- **Colour keys / channel keys:** leave them alone unless Lounge adopts an obvious meaning *(inference
  from "prefer standard gestures")*.
- **Give feedback for every key that does something,** and nothing for keys that don't. A press that
  does nothing silently is worse than a key Lounge ignores entirely.

---

## 5. Layout, safe area and grids

Source: https://developer.apple.com/design/human-interface-guidelines/layout

**Apple says (general)**
- Order content by importance, top/leading first. Align elements to aid scanning.
- Group related items with space or shapes. Use progressive disclosure.
- Differentiate controls from content using Liquid Glass. "Instead of applying a solid or semi-opaque
  background color beneath controls, use a scroll edge effect".
- Full-screen background content extends under bars.
- **Background extension effect:** if a full-bleed background would be covered by sidebars, the system
  can "flip and blur the image, mirroring it beneath adjacent components".
- When the aspect ratio changes, scale background art to fill. Don't change its aspect ratio.

**Apple says (tvOS)**
- **Safe area:** inset primary content **60 pt from top and bottom** and **80 pt from the sides**, so it
  survives overscan and TV compatibility settings.
- **Padding between focusable elements:** elements grow when focused, so they must not overlap
  important information.
- **Grids** (unfocused width, 40 pt horizontal spacing, ≥100 pt vertical spacing):

| Columns | Unfocused content width | Horizontal spacing | Min vertical spacing |
|---|---|---|---|
| 2 | 860 pt | 40 pt | 100 pt |
| 3 | 560 pt | 40 pt | 100 pt |
| 4 | 410 pt | 40 pt | 100 pt |
| 5 | 320 pt | 40 pt | 100 pt |
| 6 | 260 pt | 40 pt | 100 pt |
| 7 | 217 pt | 40 pt | 100 pt |
| 8 | 184 pt | 40 pt | 100 pt |
| 9 | 160 pt | 40 pt | 100 pt |

- **Titled rows** need extra vertical space:
  - between the previous row's bottom and the centre of the title;
  - between the title's bottom and the top of the items.
- **Use consistent spacing.**
- **Partially hidden offscreen content** should show the same width on each side, so the layout looks
  symmetrical.

*(Inference: the arithmetic works out exactly. For 2–6 columns, `cols × width + (cols−1) × 40 = 1760`,
which is 1920 − 2 × 80. The grids are sized to fill the side safe area exactly.)*

**For Lounge**
- **Safe area on the 1920×1080 plane:** x 80…1840, y 60…1020. The clock/weather pill, dock, grid and
  Settings content stay inside it. Only the wallpaper (and glass that extends it) bleeds to the edges.
- **The app grid is six columns,** so use Apple's six-column spec as the baseline: **260 pt tiles,
  40 pt gaps, ≥100 pt row pitch gap**. That gives 6 × 260 + 5 × 40 = 1760 = the safe width. Check that a
  focused tile (scaled) still clears its neighbours and the row above or below.
- **The dock is one row.** Keep tile sizes uniform. When the dock scrolls, keep partially visible tiles
  symmetrical at both edges.
- **HDMI inputs:**
  - Grid tiles the same size as apps (Apple: "consistent lockup sizes within a row").
  - Group them as their own titled row or section, with the titled-row spacing above.
- **App switcher cards:**
  - Use a lower column count (2–3 across) with the matching widths (860 or 560 pt).
  - Centre the focused card.
  - Keep side peeks symmetrical.
- **Background extension:** Apple's "flip and blur" is the same idea as Lounge's pre-blurred wallpaper
  under glass. When a panel (Settings) covers part of the wallpaper, sample the pre-blurred version
  beneath it rather than a flat fill.

---

## 6. Materials and Liquid Glass

Sources:
- https://developer.apple.com/design/human-interface-guidelines/materials
- https://developer.apple.com/design/human-interface-guidelines/color (Liquid Glass color)
- WWDC25 "Meet Liquid Glass": https://developer.apple.com/videos/play/wwdc2025/219/
- WWDC25 "Get to know the new design system": https://developer.apple.com/videos/play/wwdc2025/356/

**Apple says (HIG)**
- There are two kinds of material. **Liquid Glass** is a functional layer for controls and navigation that
  floats above content. **Standard materials** separate things within the content layer.
- **Don't use Liquid Glass in the content layer.** The exception is transient controls such as sliders
  and toggles, which take on glass while they're active.
- **Use it sparingly** on custom controls, only for "the most important functional elements".
- **Variants:**
  - **Regular** blurs and adjusts the luminosity of what's behind it. Use it for components with
    significant text: alerts, sidebars, popovers.
  - **Clear** is highly translucent. Use it only over visually rich media.
- **Dimming behind clear glass:** if the content underneath is bright, add a dark dimming layer of
  **35% opacity**. No dimming is needed if the content is already dark.
- **tvOS:**
  - Liquid Glass "appears throughout navigation elements and system experiences such as Top Shelf and
    Control Center".
  - "Certain interface elements, like image views and buttons, adopt Liquid Glass when they gain focus."
  - tvOS standard materials:

| Material | Recommended for |
|---|---|
| ultraThin | Full-screen views that require a light color scheme |
| thin | Overlays that partially obscure content, light scheme |
| regular | Overlays that partially obscure content |
| thick | Overlays that partially obscure content, dark scheme |

- **Liquid Glass color:**
  - Glass has no colour of its own; it takes colour from what's behind it.
  - Tint sparingly. To emphasise a primary action, tint the **background**, not the label. Don't tint
    many controls.
  - Small elements flip between light and dark with the content beneath. Larger ones (sidebars) are
    more opaque.

**Apple says (WWDC25 talks)**
- **Lensing:** glass is defined by lensing, which "bends, shapes, and concentrates light", rather than
  scattering it.
- **Appearing and disappearing:** elements materialize in and out by modulating the lensing, instead of
  fading.
- **Size affects thickness:** larger glass (for example a menu that grows out of a button) simulates a
  thicker material, with deeper shadows, more lensing and softer light scattering.
- **Menus** "pop open" out of the control that opened them, keeping a direct relationship between the
  two.
- **Shadows adapt:** shadow opacity rises over text and falls over light, solid backgrounds.
- **Interaction:** glass "illuminates from within" as feedback.
- **Avoid glass on glass.** Elements on top of glass use fills, transparency and vibrancy, not another
  glass layer.
- **Regular and Clear are never mixed.** Use Clear only when all three hold:
  1. it sits over media-rich content;
  2. a dimming layer won't hurt the content;
  3. the content on top is bold and bright.
- **Light/dark flipping:** small elements flip light/dark; big ones (menus, sidebars) don't, because the
  flip would be distracting.
- **Avoid intersections at rest:** in steady states, such as first launch, avoid overlaps between content
  and glass.
- **Accessibility:**
  - Reduce Transparency makes glass "frostier".
  - Increase Contrast makes elements "predominantly black or white" with a contrasting border.
  - Reduce Motion "decreases the intensity of some effects and disables any elastic properties".
- **Concentricity (design-system talk):**
  - Fixed shapes have a constant radius.
  - Capsules use radius = height / 2.
  - Concentric shapes use radius = parent radius − padding.
  - Express hierarchy "through layout and grouping" rather than decoration.

**For Lounge (no live blur: glass = a pre-blurred wallpaper)**
- **Glass is for the functional layer only:** the dock, the context menu, the Settings panel, the search
  field and keyboard, the profile/clock pill, and banners. App tiles and the wallpaper are content and
  are not glass.
- **No glass on glass.**
  - The context menu floats over the dock; give it its own glass, but render the menu's rows as
    fills/vibrancy, not as nested glass.
  - Settings rows inside the Settings panel: same rule.
- **Implementation of "regular" glass:**
  - Sample the pre-blurred wallpaper texture at the same screen position (so it lines up with the sharp
    wallpaper).
  - Add a luminosity/tint adjustment and a thin specular rim.
  - This approximates Apple's regular variant without a live blur pass *(inference: Apple specifies the
    look, not the implementation)*.
- **Every wallpaper ships with its own pre-blurred glass.** Because the blur is precomputed, glass can't
  show anything that moves behind it. Keep moving content out from under glass. That is already Apple's
  steady-state advice: avoid content–glass intersections at rest.
- **Use the regular variant everywhere text sits on glass** (menus, Settings, search). Reserve a
  clear-like look for the dock over bright, rich wallpapers, with the **35% dark dimming** rule when the
  wallpaper behind the dock is bright.
- **Tint sparingly.** Tint the background of the single primary action (for example a confirm button),
  never every row. Wallpaper-tinted Settings is fine: it is Apple's "glass takes colour from what's
  behind it".
- **Menu open/close:** grow the menu out of the focused tile, so the origin is the tile. Don't fade it in
  from nowhere. Larger glass reads as "thicker": a slightly stronger shadow and rim on the menu than on
  the dock.
- **Shapes:**
  - Pills (clock/weather, search field, keyboard keys) are capsules.
  - Rounded panels use concentric radii: inner radius = outer radius − padding.
  - Apple publishes **no tvOS radius values** (§25), so choose one fixed radius family and derive the
    others concentrically.
- **Focused element "adopts glass":** on tvOS, focused buttons and images gain a glass look. For Lounge
  that means a focus rim/highlight drawn as glass light (a specular edge), not a solid coloured ring
  *(inference about how to emulate it)*.

---

## 7. Motion

Sources:
- https://developer.apple.com/design/human-interface-guidelines/motion
- https://developer.apple.com/design/human-interface-guidelines/accessibility (Reduce Motion)

**Apple says**
- **Add motion purposefully.** Gratuitous animation can distract or cause physical discomfort.
- **Make motion optional:** never use it as the only way to convey information.
- **Feedback motion should be realistic** and follow the gesture. What slides down shouldn't be
  dismissed sideways.
- **Keep feedback animation brief and precise.**
- **Avoid adding motion to frequent interactions;** the system's own subtle animations suffice.
- **Let people cancel motion.** Don't make them wait for an animation, especially one they'll see
  repeatedly.
- **Games:** 30–60 fps is typically smooth. Use the device's capabilities for good defaults.
- **tvOS:** no extra considerations on the Motion page.
- **Reduce Motion** (Accessibility page): reduce automatic and repetitive animation, including zooming,
  scaling and peripheral motion. Specifically:
  - tighten springs to reduce bounce;
  - track animations directly to gestures;
  - avoid animating z-axis depth changes;
  - replace x/y/z transitions with fades;
  - avoid animating into and out of blur.

**For Lounge**
- **Focus moves are the most frequent interaction.** Keep them short and interruptible.
  - A new key press retargets the animation from its current value; it never queues.
  - Holding a direction must not build up a backlog.
- **Spatial consistency:**
  - The grid slides **up** from beneath the dock, so it leaves **down**.
  - The context menu grows from its tile and collapses back into it.
  - The switcher opens from Home and returns there.
- **Launching an app:** the pressed feedback is instant; nothing blocks the launch call.
- **Wallpaper:** keep it static. A moving background is constant peripheral motion and also defeats the
  pre-blurred glass.
- **Reduce Motion (Lounge setting, or the webOS accessibility flag if readable):**
  - Scale-on-focus becomes a highlight/brightness change only.
  - Slides become cross-fades.
  - No spring overshoot.
  - No blur-in/out transitions.
  - Move-mode "lift" becomes a static outline.
- Aim for a steady frame rate over richer effects. On a one-core Mali, drop an effect before dropping
  frames *(Lounge constraint; Apple: 30–60 fps "typically" smooth)*.

---

## 8. Color

Source: https://developer.apple.com/design/human-interface-guidelines/color

**Apple says**
- **Don't use one colour for two meanings.**
- **Make colours work in light, dark and increased-contrast contexts.** Custom colours need light and
  dark variants, each with an increased-contrast option, "even if your app ships in a single appearance
  mode".
- **Test under different lighting.** In bright rooms colours look darker and more muted; in dark rooms
  they look bright and saturated.
- **Test tvOS apps on several brands of HD and 4K TVs,** with different display settings.
- **Artwork affects nearby colours,** so adjust the UI colours around it (Maps switches its scheme in
  satellite mode).
- **Inclusive colour:**
  - Never rely on colour alone.
  - Ensure contrast.
  - Remember that colour meanings differ between cultures.
- **Don't hard-code system colour values;** they change between releases.
- **Liquid Glass color:** see §6.
- **Color management:** apply colour profiles. sRGB is accurate on most displays. Use P3 for
  wide-colour displays.
- **tvOS:**
  - "Consider choosing a limited color palette that coordinates with your app logo." Subtle colour
    defers to content.
  - "**Avoid using only color to indicate focus.** Subtle scaling and responsive animation are the
    primary ways to denote interactivity when an element is in focus."

**For Lounge**
- Define the palette as **semantic tokens** (label, secondary label, glass fill, focus rim, destructive,
  and so on), each with Light, Dark and High-contrast variants.
- **Focus = scale + light**, never just a colour change. A colour-only focus ring breaks Apple's rule
  and is unreadable to colour-blind viewers.
- **Keep the palette small and neutral.** The wallpaper supplies the colour, and UI tints derive from it
  (as with wallpaper-tinted Settings).
- **"Hide" in the context menu:** it isn't destructive (it's reversible), so don't make it red *(inference
  from the Buttons role guidance: destructive means data loss)*.
- **TV panels vary hugely** (LG OLED vs LCD; Vivid vs Cinema picture modes). Check every token on a real
  TV in at least two picture modes, plus the desktop preview. Avoid pure-white large areas on OLED (see
  §9 and the image-retention note in §23).
- **Asset colour space:** export wallpapers and icons as sRGB with an embedded profile.

---

## 9. Dark Mode and appearance

Source: https://developer.apple.com/design/human-interface-guidelines/dark-mode

**Apple says**
- **People choose Dark Mode as their default** on tvOS and other platforms, and expect every app to
  respect it.
- **"Avoid offering an app-specific appearance setting"**: people would have to change two settings, and
  might think the app is broken.
- **Look good in both modes, including Auto,** which "switches between the light and dark appearances as
  conditions change throughout the day, potentially while your app is running".
- **Test with Increase Contrast and Reduce Transparency,** separately and together.
- **Dark-only is acceptable in rare cases,** for immersive media apps.
- **Dark Mode colours are not simple inversions.** Use adaptive colours, not hard-coded ones.
- **Contrast:** at least **4.5:1**. Aim for **7:1** for custom colours, especially small text.
- **Soften white backgrounds** in content images so they don't glow in dark contexts.
- **Icons:** make separate light and dark interface icons if needed, for example an outline that is only
  needed on a light background.
- **tvOS:** no additional considerations.

**For Lounge**
- Lounge's Light / Dark / Auto setting **is** the system-level appearance on this TV, because webOS has no
  equivalent that Lounge reads. Apple's rule against app-specific settings targets apps running under a
  system setting; it doesn't apply to a launcher acting as the system *(inference)*.
  - If webOS ever exposes a system appearance, default to following it.
- **Auto switches by time of day while Lounge is running.** The transition must be seamless: cross-fade
  the tokens; never flash or reload.
- **Contrast target:** ≥4.5:1 for all text, 7:1 for small/secondary text. Check text over **glass over
  the brightest wallpaper** in each mode, as that is the worst case.
- **Light mode on a TV:** large bright glass areas are fatiguing in a dark room. Prefer keeping the
  wallpaper visible, so Light mode means lighter glass, not white sheets *(inference from Apple's
  "colors appear bright and saturated in dark environments")*.
- **App icons with white backgrounds** can glow in Dark mode. Apple suggests slightly darkening such
  content images; consider a subtle dim on very bright icon art.

---

## 10. Typography

Source: https://developer.apple.com/design/human-interface-guidelines/typography

**Apple says**
- **Size:** tvOS default **29 pt**, minimum **23 pt**.
- **Weight:** avoid Ultralight, Thin and Light. Prefer Regular, Medium, Semibold or Bold.
- **Hierarchy:** use weight, size and colour. Keep the number of typefaces to a minimum.
- **Leading:** avoid tight leading for three or more lines.
- **System font:** SF Pro on tvOS (NY also available). Don't embed system fonts in apps.
- **Dynamic Type exists on tvOS.** Layouts must adapt as text grows: minimise truncation, stack instead
  of placing side by side, keep the hierarchy.
- **tvOS built-in text styles** (sizes at @1x; "72 ppi for @1x and 144 ppi for @2x"):

| Style | Weight | Size (pt) | Leading (pt) | Emphasized |
|---|---|---|---|---|
| Title 1 | Medium | 76 | 96 | Bold |
| Title 2 | Medium | 57 | 66 | Bold |
| Title 3 | Medium | 48 | 56 | Bold |
| Headline | Medium | 38 | 46 | Bold |
| Subtitle 1 | Regular | 38 | 46 | Medium |
| Callout | Medium | 31 | 38 | Bold |
| Body | Medium | 29 | 36 | Bold |
| Caption 1 | Medium | 25 | 32 | Bold |
| Caption 2 | Medium | 23 | 30 | Bold |

- **tvOS tracking** (excerpt, SF Pro). At 23 pt the tracking is −4/1000 em; at 24 pt, +3; at 29 pt,
  +14 (+0.40 pt); at 31 pt, +13; at 38 pt, +10. The full table on the page covers 6–79 pt.
- **Custom fonts** must stay legible and support Bold Text and Dynamic Type.

**For Lounge**
- **Map Lounge text to these styles** on the 1080p plane (1 pt = 1 px):
  - **Clock:** Title 2–3 (57/48).
  - **Section titles** ("Apps", "Inputs"): Headline 38.
  - **App name under a focused tile:** Callout 31 or Body 29.
  - **Settings rows:** Body 29. Secondary values: Caption 1 at 25.
  - **Weather and date:** Caption 1 (25), never below **23**.
- **Nothing on screen below 23 px.** That includes the keyboard's secondary labels and the HDMI source
  sub-labels.
- **Use Medium as the base weight,** as tvOS does. Avoid Light weights entirely, which matters even more
  after the 1080→4K upscale softens thin strokes *(inference)*.
- **Apply the tvOS tracking table** if Lounge's font renderer supports per-size tracking. Body sizes get
  slightly positive tracking on tvOS, unlike iOS.
- **Use a bundled open font, not SF Pro.** SF Pro may not be redistributed in a non-Apple product, so pick an
  open font with a Medium weight and good numerals for the clock *(licensing is an inference; check the
  SF licence)*.
- **Truncate app names with an ellipsis** and show the full name when the tile is focused, rather than
  shrinking text.

---

## 11. App icons (tvOS layered icons)

Source: https://developer.apple.com/design/human-interface-guidelines/app-icons

**Apple says**
- **Layers and parallax:** tvOS icons use **2–5 layers**. When focused, the icon "elevates to the
  foreground in response to someone's finger movement on their remote, and gently sways while the surface
  illuminates". Layer separation and transparency create depth.
- **Shape:** tvOS icons are **rectangular (landscape)**, with system-applied rounded corners ("concentric
  edges"). Provide **unmasked** rectangular layers; pre-masked layers make edges jagged.
- **Size:** the tvOS layout size is **800×480 px**, layered (parallax), with no appearance variants. The
  system scales it down for other locations.
- **Colour spaces:** sRGB, Gray Gamma 2.2, Display P3.
- **Safe zone:** focusing scales and moves the icon, which crops its edges. "The system crops foreground
  layers more than background layers." The safe zone varies with size, depth and motion.
- **Text in icons:** if a tvOS icon has text, keep it on the top layer so parallax doesn't crop it.
- **Design:** keep it simple, with a solid or gradient background. Avoid photos, fine lines and
  replicated UI. Let the system add highlights, shadows and blur.
- **Alternate app icons:** tvOS lets people choose them in the app's settings.

**For Lounge**
- **LG app icons arrive as flat images** (usually square, often with their own background), not layered.
  Lounge must present them in an Apple-like **landscape rounded rectangle**:
  - Place the icon on a backing plate derived from its dominant or edge colour, at **5:3** (800×480
    ratio).
  - Inset the art so focus scaling never crops it: the safe-zone rule.
  - Mask with Lounge's corner radius at draw time; the source art is never pre-masked.
- **Fake parallax:** with no layers, the closest approximation is a moving specular highlight and a
  slight tilt on the whole tile when it is focused. Under Reduce Motion, just scale. Keep the effect
  "almost unnoticeable", per §12.
- **Size the tile texture for the focused size,** so it isn't upscaled blurry.
  - Six-column tile: 260 pt wide, about 156 tall at 5:3.
  - At around 1.1× focus scale, rasterise at least 290 px wide *(inference)*.
- **HDMI and system tiles** (Live TV, Settings, Search) are Lounge's own art. Draw them as simple glyphs on
  a solid or gradient plate, consistent with each other. No photos, no text other than the name below the
  tile.
- Lounge's **own launcher icon** (`assets`, `appinfo.json`) is exempt from tvOS sizes. It follows LG's
  icon specs.

---

## 12. Images, layered images and parallax

Sources:
- https://developer.apple.com/design/human-interface-guidelines/images
- https://developer.apple.com/design/human-interface-guidelines/image-views

**Apple says**
- **Scale factors:** tvOS uses **@1x and @2x**. Design at the lowest resolution and scale up. Align
  vector points to whole values.
- **Formats:**
  - de-interlaced PNG for bitmaps;
  - 8-bit palettes where full colour isn't needed;
  - JPEG or HEIC for photos;
  - PDF or SVG for flat icons.
- Include colour profiles, and test on real devices.
- **Parallax:** "As an element comes into focus, the system elevates it to the foreground, gently swaying
  it while applying illumination that makes the element's surface appear to shine. **After a period of
  inactivity, out-of-focus content dims and the focused element expands.**"
- **Layered images:** 2–5 layers.
  - Foreground holds prominent elements and text.
  - Middle holds secondary content and shadows.
  - Background must be **opaque**.
  - Keep layering "simple and subtle": "Parallax is designed to be almost unnoticeable."
  - Leave a safe zone around the foreground. Preview on a real TV.
  - App icons must be layered; Top Shelf and other focusable images are "strongly encouraged" to be.
- **Text over images:** ensure contrast. Consider a text shadow or background layer.

**For Lounge**
- **Wallpapers:**
  - Ship at the plane size (1920×1080). Do the 4K variants only if the 4K plane ships.
  - Each has a **matching pre-blurred version** for glass, built offline from the same source.
  - JPEG for photos. Strip the progressive/interlaced encoding if the decoder prefers baseline (Apple:
    "de-interlaced PNG" for bitmaps) *(inference that the same holds for Lounge's decoder)*.
- **Idle dimming:** Apple's "after a period of inactivity, out-of-focus content dims and the focused
  element expands" is a cheap, faithful effect to copy. After a few seconds with no input, dim the
  unfocused dock tiles slightly. Wake instantly on any key.
- **Text over the wallpaper** (clock, date, weather) needs a contrast aid that is not a box. Use a soft
  shadow or a local glass pill, and check it on the brightest wallpaper.
- **Profile picture:** cropped to a circle. Apple's monogram lockup uses circular pictures with initials as
  the fallback (§13).

---

## 13. Collections and lockups

Sources:
- https://developer.apple.com/design/human-interface-guidelines/collections
- https://developer.apple.com/design/human-interface-guidelines/lockups

**Apple says**
- **Use standard row or grid layouts,** not custom ones that draw attention to themselves.
- **Use tables for text-heavy content.**
- **Make items easy to reach,** with padding so focus effects are visible and items never overlap.
- **Animate insert, delete and reorder** to give feedback.
- **Lockups** combine content, header and footer views that "expand and contract together as the lockup
  gets focus".
  - Leave space for the focused size.
  - Use consistent sizes within a row.
- **Lockup types:**
  - **Caption buttons** tilt with the swipe: up/down in a column, left/right in a row, both in a grid.
  - **Monograms** are a circular photo plus a name, with initials if there's no photo. "Prefer images
    over initials."
  - **Posters:** "an image and an optional title and subtitle, which are hidden until the poster comes
    into focus".

**For Lounge**
- **Dock and grid are standard row and grid collections.** Don't invent a curved or carousel dock.
- **App name labels follow the poster rule:** hidden until the tile is focused, then shown below it,
  scaling and moving with the tile as one lockup. This also cuts on-screen text (fewer labels at rest).
- **Move mode (reorder):**
  - The lifted tile follows the D-pad, and neighbours animate aside: Apple's "reorder" feedback.
  - OK drops the tile; Back cancels and restores the original position.
- **Hide:** animate the removal (neighbours close the gap), then focus the next tile (§3).
- **Tilt-with-swipe:** if Lounge tilts focused tiles, tilt in the direction of the last move, using the
  axis rules above (left/right in the dock, both axes in the grid).
- **Profile pill:** the picture is a monogram. Use initials on a coloured disc when there's no picture.
- **App switcher cards** are 16:9 lockups: a snapshot or icon, with the app name as a footer. Use uniform
  sizes.

---

## 14. Menus and context menus

Sources:
- https://developer.apple.com/design/human-interface-guidelines/menus
- https://developer.apple.com/design/human-interface-guidelines/context-menus

**Apple says (menus)**
- **Labels:**
  - Use verbs ("View", "Close") in title-style capitalization.
  - Drop articles.
  - Add an ellipsis if more input follows.
  - Dim unavailable items in regular menus.
- **Icons:** use them sparingly, and either for every item in a group or for none. Use the standard icons
  for common actions.
- **Order:** put frequent items first and group related ones with separators. Watch the length.
- **Submenus:** one level, roughly five items or fewer.
- **Toggled items:** use a changeable label ("Show Map" / "Hide Map"), or a checkmark for a state.

**Apple says (context menus)**
- **Contents:** only the commands most relevant in this context. Keep the number of items small.
- **Consistency:** support context menus consistently everywhere, or people assume something is broken.
- **Every item must also exist in the main interface.**
- **Unavailable items:** **hide** them, don't dim them (unlike regular menus).
- **Order:** put the most-used items where the eye lands first, reversing the order if the menu opens
  above its item.
- **Grouping and destructive items:**
  - About three groups at most, separated.
  - Destructive items go last and are marked destructive.
- **Title:** context menus seldom have one; add it only if it clarifies the effect.
- **tvOS:** no additional considerations. Reveal context menus with "touch and hold" (gestures table, §4).

**For Lounge (hold-OK menu: Move, Hide)**
- **Labels:** "Move" and "Hide" are verbs, short, and title case. Don't add "App" or "the".
- **Every action must also exist elsewhere,** per Apple's context-menu rule:
  - **Hide** must be reversible from Settings, with a Hidden Apps list that can un-hide.
  - **Move** should also be reachable, for example via a "Rearrange" option in Settings *(inference: the
    rule says main interface)*.
- **Only show what applies:**
  - HDMI tiles can't be uninstalled, so don't show an "Uninstall" there.
  - Fixed tiles can't be hidden, so hide "Hide" on those rather than dimming it.
- **Placement:**
  - Open the menu **beside** the focused tile.
  - Put the most-used item (Move) nearest the tile.
  - Reverse the order if the menu opens on the other side.
- **Focus starts on the first item.** Back closes the menu and returns focus to the tile.
- **Same gesture everywhere:** hold-OK must open a menu on every tile type (dock, grid, HDMI). If a tile
  has no actions, still respond, for example with a one-item menu or a brief shake. Apple: inconsistency
  makes people "think there's a problem" *(the response choice is inference)*.
- **Glass and growth:** the menu is regular glass, grows from the tile (§6), and has no title.

---

## 15. Buttons

Source: https://developer.apple.com/design/human-interface-guidelines/buttons

**Apple says**
- **Space around buttons:** leave room around buttons. The general minimum is 44×44 pt, but tvOS
  controls are bigger: the Accessibility page lists tvOS **default 66×66 pt, minimum 56×56 pt**.
- **Press state:** custom buttons must have one.
- **Prominence:** use a prominent (tinted background) style for the most likely action, with only one or
  two per view. Distinguish the preferred option by **style, not size**.
- **Labels on colourful content:** avoid colouring button labels the same as the content behind them.
  Default to monochrome labels.
- **Text labels:** verb-first in title case ("Add to Cart").
- **Roles:**
  - **Primary** uses the accent colour.
  - **Destructive** uses system red.
  - Never make a destructive action primary.
- **tvOS:** no additional considerations.

**For Lounge**
- **Size:**
  - Settings rows and keyboard keys: at least **66 pt tall** (absolute minimum 56).
  - Small round buttons (for example the clear-search key): at least 66×66.
- **Prominent buttons:** at most one per screen, such as "Done" or the confirm in a dialog. Everything
  else is plain glass rows.
- **Press feedback** on every button (§3, highlighted state).
- **Labels:** verbs ("Hide", "Show All", "Done"), never "OK" for an action (§17).

---

## 16. Search, text fields and keyboards

Sources:
- https://developer.apple.com/design/human-interface-guidelines/search-fields
- https://developer.apple.com/design/human-interface-guidelines/virtual-keyboards
- https://developer.apple.com/design/human-interface-guidelines/text-fields
- https://developer.apple.com/design/human-interface-guidelines/keyboards

**Apple says**
- **Search field anatomy:** a Search icon, a Clear button and placeholder text.
  - Placeholder text says what can be searched.
  - **Start searching as people type.**
  - Show suggestions (recent and predictive).
  - Put the most relevant results first; categorise and filter them.
- **tvOS search:**
  - "A search screen is a specialized keyboard screen … displaying search results **beneath the
    keyboard**".
  - "People typically don't want to do a lot of typing in tvOS": provide popular, contextual and recent
    suggestions.
- **tvOS keyboard:** "tvOS displays a **linear virtual keyboard** when people select a text field using
  the Siri Remote". A grid keyboard appears with other input devices. Digit entry gets a digit keyboard.
- **Keyboard feedback:** play the standard keyboard click for custom input views. Use a Return key type
  that matches the task (for example "Search").
- **Text fields:** "Minimize text entry in your tvOS … apps". Use buttons and choices instead where
  possible.
- **Focus ring:** text and search fields use a focus ring, not a highlight (§3).

**For Lounge (one-row keyboard)**
- Lounge's **one-row keyboard is Apple's linear keyboard:** a single row of letters, with results
  **beneath** it.
- **Filter on every key,** with no Search button needed. Apple: "start search immediately".
- **Placeholder text:** a short phrase such as "Apps and inputs". Don't add instructions.
- **Show suggestions before typing:** recently opened apps, which Lounge already knows from the switcher.
  The empty state therefore has useful content instead of a blank screen (§20).
- **Results:**
  - Exact or prefix matches first.
  - Apps and HDMI inputs can share one list, or be grouped if both are present.
  - D-pad Down moves from the keyboard into the results; Back from the results returns to the keyboard;
    Back from the keyboard closes Search.
- **Keys:**
  - Include delete and clear (the field's Clear button), and a space key only if names need it.
  - Key focus = scale plus glass highlight. Each key is ≥56 pt (aim for 66).
- **Clear:** Lounge has no keyboard click sound unless the TV's UI sounds are available; the HIG's
  standard-click rule assumes a system sound exists *(inference)*.

---

## 17. Alerts, sheets, action sheets and modality

Sources:
- https://developer.apple.com/design/human-interface-guidelines/alerts
- https://developer.apple.com/design/human-interface-guidelines/sheets
- https://developer.apple.com/design/human-interface-guidelines/action-sheets
- https://developer.apple.com/design/human-interface-guidelines/modality

**Apple says**
- **Alerts:**
  - Use them sparingly, only for essential, actionable information.
  - Never use one just to inform, never for common undoable actions, and **never at app start**. Startup
    problems such as "no network" should show cached data plus a non-intrusive label.
- **Alert content:** a title (complete and specific, at most two lines), optional informative text, and
  **up to three buttons**. Don't write "Error 329347".
- **Alert buttons:**
  - One or two specific words ("Erase", "Delete").
  - "OK" only for purely informational alerts.
  - "Cancel" is always titled "Cancel".
  - Default button on the trailing side or at the top; Cancel on the leading side or at the bottom.
  - Destructive style only for actions the person didn't deliberately choose.
- **tvOS cancel:** alerts can be cancelled with the remote's **Menu** (Back) button.
- **Sheets:** always modal on tvOS. Show one at a time. Pair Done with Cancel or Back, never all three.
- **Action sheets:** offer choices related to an intentional action. Short title, destructive options
  marked, Cancel at the bottom.
- **Modality:**
  - Use it only for a clear benefit.
  - Keep the task short.
  - No app-within-an-app.
  - Name the task.
  - Never stack modals or show two alerts at once.
  - On tvOS, people expect the dismiss button "in the main content view".

**For Lounge**
- **No alert on launch, ever.** Weather offline or the TV not on the network: show the last cached
  weather (or nothing) plus a quiet indicator in the pill.
- **Hide doesn't need an alert:** it's reversible.
- **When a confirmation is needed** (for example "Reset Layout"):
  - an action-sheet-style glass panel;
  - a specific verb as the button ("Reset");
  - "Cancel" below it;
  - Back = Cancel.
- **Settings is a full-screen modal surface.** Back dismisses it. Nested pages (Wallpaper, Appearance,
  Hidden Apps) form a single path; Back always goes up exactly one level.
- **Never open the context menu while another menu is open,** and never show two banners at once.

---

## 18. Accessibility

Sources:
- https://developer.apple.com/design/human-interface-guidelines/accessibility
- https://developer.apple.com/design/human-interface-guidelines/voiceover

**Apple says**
- **Text size:** support enlarging text by **at least 200%**. tvOS text: **29 pt default, 23 pt minimum**.
- **Contrast** (WCAG AA, as used by Accessibility Inspector):

  | Text | Weight | Minimum ratio |
  |---|---|---|
  | up to 17 pt | All | 4.5:1 |
  | 18 pt and up | All | 3:1 |
  | All | Bold | 3:1 |

  - If the default scheme falls short, provide a higher-contrast scheme under Increase Contrast.
- **Never use colour alone.** Describe the UI for VoiceOver.
- **Control sizes:** tvOS default **66×66 pt**, minimum **56×56 pt**. Padding is about **12 pt** around
  bezelled elements and about **24 pt** around unbezelled ones.
- **Interaction:** use simple gestures, with alternatives to gestures.
- **Timers:** minimise time-boxed UI and prefer explicit dismissal.
- **Media:** don't autoplay audio or video without controls.
- **Reduce Motion:** see §7 (no zoom/scale/peripheral motion, fades instead of slides, no blur
  animation, no z-depth animation, tighter springs).
- **Liquid Glass with accessibility settings** (WWDC25):
  - Reduce Transparency makes it "frostier".
  - Increase Contrast makes it black or white with a border.
  - Reduce Motion removes elasticity.
- **tvOS:** no additional considerations.

**For Lounge**
- **Reduce Transparency:** replace the glass sample with a nearly opaque fill tinted from the wallpaper.
  This is also cheaper to draw.
- **Increase Contrast:**
  - Solid near-black or near-white surfaces with a 1–2 px contrasting border.
  - A focus rim with a strong outline.
  - Text at 7:1.
- **Reduce Motion:** see §7.
- **Larger Text:** a Lounge text-size option needs to reach 200% for Settings and labels. Tile labels may
  wrap to two lines or truncate with the full name on focus; don't overlap neighbours.
- **No time-boxed UI:** banners and toasts persist until dismissed, or until they're truly irrelevant.
  The move mode never times out.
- **Holding OK is an extra gesture,** so the main interface also offers its actions (§14).
- **Screen reader:** if webOS exposes its screen reader (Audio Guidance) to native apps, label every tile
  with "App name, n of m" *(inference; HIG says describe UI for VoiceOver)*.
- **Read the TV's own accessibility flags where possible.** Apple: respect system settings rather than
  duplicating them (§22). Fall back to Lounge settings only if webOS doesn't expose them.

---

## 19. Branding

Source: https://developer.apple.com/design/human-interface-guidelines/branding

**Apple says**
- **Accent colour:** use it judiciously, only for primary actions and status. Move brand colour into the
  content layer.
- **Custom fonts** must be legible. Custom headline fonts with system body text work well.
- **Use familiar components** and standard patterns.
- **Branding defers to content.** Don't repeat the logo throughout the app.
- **Launch screens are not a branding opportunity.**
- **Follow Apple's trademark guidelines.** Apple trademarks must not appear in the app name or images.

**For Lounge**
- **No Lounge logo on the home screen.** The wallpaper and the person's apps are the content.
- **Lounge is "tvOS-style", not Apple.** No Apple logos, no "Apple TV", "tvOS" or SF Pro in the shipped
  UI or icon, and no Apple wallpapers. The README can describe the style, but the product must not
  look affiliated with Apple.
- **Accent:** a single accent for focus light and the primary button only.

---

## 20. Writing

Source: https://developer.apple.com/design/human-interface-guidelines/writing

**Apple says**
- **Voice and tone:** define a voice, match the tone to the situation, be clear, and use fewer words.
  Write for everyone (plain language, no jargon, not gendered).
- **Buttons and links:** action-oriented labels, usually verbs. "Send" beats "Let's do it!"
- **Capitalisation:** pick a rule per element type and keep it consistent.
- **Multi-step flows:** "Get Started", then "Continue"/"Next", then "Done".
- **Possessive pronouns:** use them sparingly. "Favorites", not "Your Favorites". Avoid "we": "Unable
  to load content" is better than "We're having trouble…".
- **TVs are shared:** TVs "are often in common living spaces, and several people are likely to see
  anything on the screen, so consider who you're addressing. Bigger screens also require brevity".
- **Empty states:** give clear next steps, with a button where possible.
- **Errors:** place them near the problem, don't blame, and say how to fix it. No "oops".
- **Settings labels:** say what a setting does when on; people infer the off state. Link directly to a
  setting instead of describing where it is.

**For Lounge**
- **Terse labels:** "Move", "Hide", "Hidden Apps", "Wallpaper", "Appearance: Light / Dark / Automatic".
- **No "my" or "your":** "Profile Picture", not "Your profile picture".
- **Shared screen:** never show anything personal-sounding beyond the chosen profile picture. No
  greetings with names, given the shared living room *(inference from Apple's TV note)*.
- **Errors:** "Weather unavailable", not "We couldn't load the weather". An HDMI input with no signal
  shows "No Signal", which is factual and short.
- **Empty Hidden Apps list:** one line ("Hidden apps appear here") with no extra instructions. This fits
  Apple's rule and the "low text load" goal.

---

## 21. Launching, loading and onboarding

Sources:
- https://developer.apple.com/design/human-interface-guidelines/launching
- https://developer.apple.com/design/human-interface-guidelines/loading
- https://developer.apple.com/design/human-interface-guidelines/onboarding

**Apple says**
- **Launch instantly.** People "sometimes … don't want to wait more than a couple of seconds".
- **Launch screen:**
  - Nearly identical to the first screen.
  - No text and no branding.
  - Match the current appearance mode.
  - On tvOS the launch screen is static.
- **Restore previous state** so people continue where they left off (for example the scroll position).
- **Loading:**
  - Show something as soon as possible, with placeholders.
  - Let people act while content loads.
  - Use determinate progress when the duration is known.
- **Onboarding:**
  - Optional, short and interactive.
  - Prefer contextual tips to a big flow.
  - Don't teach the system or device.
  - Postpone non-essential setup and provide good defaults.

**For Lounge**
- **Cold start (TV power-on) shows the dock over the wallpaper immediately:**
  - Decode the wallpaper and the cached icon atlas first.
  - Fill in weather, HDMI state and app list updates afterwards.
  - Never show a spinner over an empty screen.
- **The "launch screen" equivalent is the wallpaper itself:** same image, same appearance. It must not
  flash a different colour first. If something must be shown before the wallpaper decodes, use a solid
  colour matching its average.
- **Restore state:** on returning from an app, or on Home, focus the tile that was focused (or the app
  just used), and keep the grid scroll position.
- **Icons still loading:** draw a placeholder plate in the icon's cached average colour, then swap in the
  real art without layout shift.
- **No onboarding flow.** Defaults are good (dock pre-filled from the most-used apps). Hold-OK can be
  taught by a single contextual tip at most, shown once, if needed at all.

---

## 22. Settings

Source: https://developer.apple.com/design/human-interface-guidelines/settings

**Apple says**
- **Defaults:** choose defaults that are best for most people.
- **Keep settings few;** too many makes the experience less approachable.
- **Don't ask for what can be detected** (for example a connected controller, or Dark Mode).
- **Respect system-wide settings.** Don't duplicate them in your own settings.
- **General, infrequently changed settings** go in the settings area.
- **Task-specific options** (show/hide, reorder, filter) go where the task happens, not in Settings.

**For Lounge**
- **Keep Settings short:**
  - Appearance (Light / Dark / Automatic)
  - Wallpaper
  - Profile Picture
  - Hidden Apps
  - Weather location (only if auto-detect fails)
  - About
- **Reordering happens in place** (hold-OK → Move), not in Settings. This is Apple's "task-specific
  options" rule. Hidden Apps in Settings exists only as the recovery path (§14).
- **Detect rather than ask:**
  - Weather location from IP or TV settings.
  - HDMI inputs from the TV.
  - Resolution from the panel.
- **Don't duplicate TV settings** (picture, sound, network). If Lounge needs one, open the LG settings page
  directly, per Apple's "provide a button that opens it".
- **Layout:** Settings uses a split view or list. Apple's tvOS split view is one-third primary, two-thirds
  secondary, with one title above.

---

## 23. Other tvOS notes

Sources:
- https://developer.apple.com/design/human-interface-guidelines/tab-bars
- https://developer.apple.com/design/human-interface-guidelines/split-views
- https://developer.apple.com/design/human-interface-guidelines/scroll-views
- https://developer.apple.com/design/human-interface-guidelines/lists-and-tables
- https://developer.apple.com/design/human-interface-guidelines/segmented-controls
- https://developer.apple.com/design/human-interface-guidelines/page-controls
- https://developer.apple.com/design/human-interface-guidelines/playing-video
- https://developer.apple.com/design/human-interface-guidelines/managing-accounts
- https://developer.apple.com/design/human-interface-guidelines/multitasking
- https://developer.apple.com/design/human-interface-guidelines/live-viewing-apps

**Apple says (tvOS-specific)**
- **Tab bar:**
  - Height **68 pt**, top edge **46 pt** from the top of the screen; neither can be changed.
  - Translucent; only the selected tab is opaque.
  - Overflow fades out at the edges.
  - Pressing Menu always returns focus to the tab bar.
- **Split view:** the default split is **⅓ primary / ⅔ secondary** (or half and half), with a single title
  above.
- **Scrolling:** tvOS views have **no scroll indicators**. The system scrolls automatically to keep the
  focused item visible.
- **Lists:** a focused row highlights, grows slightly and may get rounded corners. Don't add your own
  masks to round them.
- **Segmented controls:** segments are selected **when focus moves to them**. Keep other focusable
  elements away. Consider a split view for filtering.
- **Page controls:** only for collections of full-screen peer pages.
- **Video overlays:**
  - Keep logos and overlays small.
  - "Some devices are prone to **image retention**", so prefer short-lived, translucent SDR overlays to
    bright, opaque ones.
  - Interactive overlays wait at least 0.5 s after pausing.
- **Accounts:** ask for the minimum. Prefer signing in on another device. Don't make people pick a
  profile every time.
- **Multitasking:** picture-in-picture while browsing.
- **Live-viewing apps:** minimise the steps to playback, and make live content look live (badge, symbol,
  sash or progress).

**For Lounge**
- **No scroll bars or indicators** in the grid or Settings. Focus drives scrolling, and the grid scrolls
  to keep the focused row visible with context above and below.
- **Settings rows:** focused rows grow and round per the list rule. Lounge draws its own rounding (it has
  no system mask), consistent with its tile radius.
- **Appearance chooser:** don't build it as a segmented control (focus-selects). Use a list with a
  checkmark, so moving focus never changes the appearance by accident.
- **Image retention (OLED):**
  - The clock/weather pill and dock are static, bright-edged elements on screen for hours when the TV
    idles on Home.
  - Keep them translucent and not pure white.
  - Consider a subtle periodic shift or a dim-on-idle (§12) *(inference applying Apple's video-overlay
    advice to a static home screen)*.
- **Live TV tile:** if it shows a "live" badge, use a shape or label as well as colour (§8).

---

## 24. Numbers at a glance

| Item | Value | Source page |
|---|---|---|
| Viewing distance | often 8 ft or more | designing-for-tvos |
| Safe area | 60 pt top/bottom, 80 pt sides | layout |
| Grid spacing | 40 pt horizontal, ≥100 pt vertical | layout |
| 6-column grid tile | 260 pt wide | layout |
| Other grids | 2:860, 3:560, 4:410, 5:320, 7:217, 8:184, 9:160 pt | layout |
| Body text | 29 pt (Medium, 36 leading) | typography |
| Minimum text | 23 pt | typography, accessibility |
| Title 1 / 2 / 3 | 76 / 57 / 48 pt | typography |
| Headline / Callout / Caption 1 | 38 / 31 / 25 pt | typography |
| Control size | 66×66 default, 56×56 minimum | accessibility |
| Control padding | ~12 pt (bezel), ~24 pt (no bezel) | accessibility |
| Contrast | ≥4.5:1, aim 7:1 for small custom text | dark-mode, accessibility |
| Text enlargement | support ≥200% | accessibility |
| Clear-glass dimming | 35% dark layer over bright content | materials |
| App icon | 800×480 px, landscape, 2–5 layers | app-icons |
| Layered image | 2–5 layers, opaque background | images |
| Scale factors | @1x and @2x | images |
| Top Shelf static | 2320×720 pt (4640×1440 @2x) | top-shelf |
| Top Shelf banner | 1940×692 actual / 1740×620 focused / 1740×560 unfocused | top-shelf |
| Top Shelf banners count | 3–8 | top-shelf |
| Tab bar | 68 pt tall, 46 pt from top | tab-bars |
| Split view | ⅓ / ⅔ (or ½ / ½) | split-views |
| Context menu groups | about 3 at most; submenus one level | context-menus |
| Alert buttons | up to 3 | alerts |
| Interactive video overlay delay | ≥0.5 s | playing-video |
| Game frame rate | 30–60 fps typical | motion |

---

## 25. What the HIG does not publish

These were looked for and are **not** on the current pages, so any Lounge value for them is a design
choice, not an Apple rule.

- **Focus scale factor.** No number is given. The Top Shelf focused/unfocused sizes imply roughly
  1.09–1.14× *(inference, §2)*.
- **Corner radii** for tvOS tiles, buttons, panels or icons. The pages say only "rounded corners" and
  "concentric edges". WWDC25 gives the rules (fixed, capsule = height / 2, concentric = parent −
  padding) but no tvOS values.
- **Animation durations or spring constants.** None are published for tvOS.
- **Shadow or glow specs** for the focused state.
- **Blur radius or tint values** for Liquid Glass or standard materials. The one number is the 35%
  dimming for clear glass.
- **tvOS context-menu specifics** (layout, placement, Apple TV Home's own "move apps" mode). The Context
  menus page says "No additional considerations for tvOS"; the Home Screen's edit mode is not documented
  in the HIG.
- **A launcher's Back behaviour, or the app switcher.** The HIG covers apps, where Back at top level goes
  to the Home Screen, not the Home Screen itself.
- **The tvOS app icon @1x/@2x and App Store sizes.** The current App icons page lists only the 800×480 px
  layout size; older versions of the page listed more.
- **Dock dimensions** on the Apple TV Home Screen.
