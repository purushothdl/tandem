# wingroup

A macOS menu-bar app that keeps windows together. Put two to six windows side by side, tile
them, and they resize, switch and go full screen as one.

Written in Rust. No dependencies beyond bindings to macOS.

## Build and run

```sh
./bundle.sh
open target/wingroup.app
```

Allow it in System Settings > Privacy & Security > Accessibility, then quit and reopen it.
Rebuilding keeps the permission.

## Use

| To | Do |
|---|---|
| Make a group | Arrange windows, then menu > **Tile** or ⌃⌥T |
| Resize | Drag the bar between two windows; double-click evens them |
| Swap | Drag a tile by its title bar onto another |
| Add | Hold Shift while dragging a window onto half of a tile |
| Change the layout | Hover the group in the menu, or ⌃⌥L |
| Switch | Click any window of the group, Cmd-Tab, the menu, or the bar in Mission Control |
| Everything else | The group's submenu |

A small window over a group stays with it, as in plain macOS. A large one counts as separate.

## Develop

```sh
cargo test                  # pure logic, no windows involved
cargo clippy --all-targets
tail -f ~/Library/Logs/wingroup.log
```

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): how it fits together
- [docs/QUIRKS.md](docs/QUIRKS.md): macOS behaviour that shaped the code
- [docs/READING.md](docs/READING.md): the systems ideas in it, and how to study them

## Limits

- Main display only.
- macOS cannot merge windows in Mission Control, so groups get a bar above them instead.
- Full screen stays below the menu bar, and hides the Dock for everything. If a crash leaves
  the Dock hidden: `target/wingroup.app/Contents/MacOS/wingroup --restore`
- Uses a few private macOS calls that an update could break.
