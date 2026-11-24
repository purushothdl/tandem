# wingroup — v1 spec

Menu-bar app (Rust, macOS 26) that makes two or more windows behave as one group.

1. Group model: a group has 2+ windows and a split layout (side by side, ratio). A hotkey or menu action groups the focused window with a picked one.
2. Window control: Accessibility API (AXUIElement) to move, resize and raise windows; window IDs via private `_AXUIElementGetWindow`. Needs Accessibility permission.
3. Linked raise: when any window in a group takes focus, raise the rest.
4. Linked resize: observe moved/resized notifications and reflow siblings to keep the split ratio.
5. Divider: thin borderless overlay window on the seam; dragging resizes both sides; double-click resets to 50/50.
6. Full mode: fill the screen with the group's layout; turn Dock and menu-bar auto-hide on, restore on exit.
7. Corners: optional toggle that runs `defaults write -g NSConvolutionOverride1 -float 10` (undocumented; apps need relaunching).
8. Out of v1: Mission Control showing a group as one tile (needs own overview), >2-window layouts beyond a simple split.

Risks: private ID call may break on updates; slow-redraw apps lag during linked resize; Dock/menu-bar toggle may flicker.

Layout plan: `core` (AX, groups, observers; no GUI dependency) + thin UI layer (menu bar item, divider window).
