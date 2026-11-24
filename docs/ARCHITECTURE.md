# Architecture

## The idea

Other apps own their windows. We can only ask them to move, then look at what happened. So
wingroup keeps its own truth, the **layout**, and keeps steering the windows towards it.

```
          events                      commands
 mouse ──────────┐               ┌──────────────► writer threads ──► other apps
 timers ─────────┼──►   app   ───┤
 notifications ──┘   ▲      │    └──────────────► ui (bars, outlines, menu)
                     │      ▼
                     │    core: pure decisions
                     │
                  platform: every call into macOS
```

## Four folders

| Folder | Job | Rule |
|---|---|---|
| `core/` | Decide | No macOS, no clock, no threads. All of it is unit-tested |
| `platform/` | Talk to macOS | All C functions are declared here; the rest gets a safe API |
| `app/` | Hold the groups, connect events to decisions | The only code that changes a group |
| `ui/` | Draw | Reads `app::views`, calls `app::commands` |

`unsafe` outside `platform/` is Objective-C class boilerplate and a few AppKit calls, all in `ui/`.

## Where to look

| To change | Open |
|---|---|
| How a group divides its space | `core/layout` |
| What runs when | `app/run.rs` |
| Moving windows | `app/group.rs`, `app/writer.rs` |
| Keeping a group in front | `app/switching.rs`, `app/stacking.rs` |
| Dragging seams, dragging windows | `app/seams.rs`, `app/tiles.rs` |
| Mission Control: noticing picks | `app/mission_control.rs` |
| Mission Control: the bar | `ui/mission_control`, `core/bar_spot.rs`, `core/bar_phase.rs` |
| The menu | `ui/menu` |

## Threads

- **Main**: everything in `app` and `ui`. State lives in one `RefCell`, so there are no locks.
- **One writer per window**: the only thread that moves it.
- **Short-lived ones** for raising windows, so one slow app does not hold up the others.

A call into another app can block for up to 0.4 s. The main thread mostly asks the window
server instead, which always answers.

## The life of a seam drag

1. The mouse tap sees the press. `seams.rs` finds a seam there and swallows the event.
2. Each move updates the layout, which stops at the windows' minimum sizes.
3. New frames go to the writers. A writer that is behind skips to the newest.
4. If an app refuses to shrink, `core/refusal.rs` decides whether that is a real minimum.
5. The main thread learns it and the seam stops there.

## The life of a switch

1. A click, Cmd-Tab or Mission Control brings one window of a group to the front.
2. `come_to` remembers that window and starts looking every 5 ms.
3. Once it is on top, the rest of the group is raised under it.
4. macOS may reorder windows a moment later, so `core/recheck.rs` looks again with growing delays.
5. If something else is on top by then, the user moved on and it stops.

## Words

| Word | Meaning |
|---|---|
| tile | A window of a group |
| seam | The line between two tiles |
| pane | A small window floating over a group |
| region | The rectangle a group fills |
| raise | To the front, without activating the app |
| activate | Make the app the one with the keyboard |
