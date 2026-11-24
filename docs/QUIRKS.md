# macOS quirks

Found by trying. Each one explains code that would otherwise look odd.

## Stacking

- **All windows of the active app stay above other apps' raised windows.** When another
  window of the active app sits inside a group, raising cannot get past it. The apps are
  activated in turn instead (`stacking.rs`, `hand_over`).
- **A raise lands just below the active app's windows.** So a stack is rebuilt from rear to
  front.
- **`AXFrontmost` brings up all of an app's windows.** `focus_without_raising` uses private
  calls to avoid that.
- **The chosen window's own activation lands after ours** and can undo it. Hence the rechecks.

## Mission Control

- **Real clicks reach neither the mouse tap nor our windows.** Simulated ones do, so they
  prove nothing in a test.
- A pick shows first as a change in the window order, long before Mission Control closes.
- The press itself can be seen by polling the button. That is how the bar works.
- A click that hits no thumbnail closes Mission Control, so the bar must not cover one.
- Thumbnails are listed with their scaled positions, so positions mean nothing while it is open.
- It is recognised by a Dock window on layer 18 that fills the display.
- The Dock adds normal-layer helper windows of its own meanwhile.
- Apps cannot merge thumbnails. "Group windows by application" is fixed to apps.

## Resizing

- **A busy app looks like one at its minimum size**: both return a larger frame than asked.
  Only time tells (`core/refusal.rs`).
- An app being dragged refuses to be resized until it has seen the button go up.
- What is on screen can be an animation. Before correcting a window, the app itself is asked.
- Sending the end frame before a glide's steps makes windows jump.

## The system

- `NSConvolutionOverride1` sets the corner radius of windows. 1 is square; 0 counts as unset.
  Apps read it when they start.
- A slow mouse tap is switched off by macOS and must be switched on again.
- Ad-hoc signing changes the app's identity on every build and loses the Accessibility
  permission. `bundle.sh` pins it to the bundle id.
