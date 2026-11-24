# wingroup

Menu-bar app for macOS that makes two or more windows behave as one group.
See [SPEC.md](SPEC.md) for the design.

## Build and run

```sh
./bundle.sh                 # builds target/wingroup.app
open target/wingroup.app    # a ▥ icon appears in the menu bar
```

First run: allow it in System Settings > Privacy & Security > Accessibility
(add `target/wingroup.app` with the + button if it is not listed), then quit
and reopen it. Rebuilding changes the app's signature, so macOS may ask again.

## Use

1. Arrange two to six windows however you like: halves, columns, one half and two stacked, a grid.
2. Click the menu-bar icon > **Tile These Windows**. They glide edge to edge on the main
   display and keep that shape; near-halves, thirds and quarters become exact. Windows more
   than half hidden are skipped, and so are windows already in a group, so you can tile
   several groups over each other.

The menu shows a picture of what it will make. To choose exactly which windows, use
**Choose Windows…** and tick them. Shortcut: Control-Option-T.

Then:

- **Resize**: drag the bar between windows (it shows on hover). It stops at each app's
  minimum size. Double-click it to even the split.
- **Swap**: drag a tile by its title bar onto another tile and they trade places. Dropped
  anywhere else, or resized by another edge, it glides back.
- **Drag a pane in**: start dragging it by its title bar, then hold Control. The highlight shows
  the half of a tile it will take.
- **Switch**: clicking one window, Cmd-Tab or Mission Control brings its whole group.
- **Panes**: a smaller window over the group behaves as in plain macOS: clicking a tile puts
  that tile in front of it, clicking the other tile sends it fully behind. If it was in front
  when you left the group, it comes back with the group. A window larger than about 60% of
  the group counts as a separate app.
- **Mission Control**: each grouped window carries its group's icons and name.
- Each group's own menu: **Switch To**, **Add Windows…** (splits the tile you used last), **Remove from Group**, **Even Out Sizes**, **Full Screen**, **Rename…**, **Ungroup**.

Rectangle can stay on. If it also snaps a tile you drop near a screen edge, wingroup puts it back.

## If something goes wrong

- Dock stuck hidden after a crash: `target/wingroup.app/Contents/MacOS/wingroup --restore`
- Check permission and window access: `target/wingroup.app/Contents/MacOS/wingroup --check`

## Limits (v1)

- Primary display only.
- Mission Control still shows each window separately; macOS has no API to merge them, so groups are labelled instead.
- Full screen fills the display below the menu bar. The Dock setting is system-wide, so it stays hidden until no group is full screen.
- Uses the private `_AXUIElementGetWindow` call, which could break in a macOS update.
- Slow-redrawing apps (Electron) may lag while the seam is dragged.
