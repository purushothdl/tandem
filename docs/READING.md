# Reading this as a systems project

The menus and the macOS quirks are not the point. These five ideas are, and each has a
counterpart on a microcontroller.

| Idea | Here | On a microcontroller |
|---|---|---|
| Newest value wins | `app/writer.rs` | Overwriting mailbox; one task owns the peripheral |
| Ask, read back, retry, time out | `writer.rs`, `core/refusal.rs` | Write a register, poll a flag, give up |
| State machines that take the time as input | `core/recheck.rs`, `card_pick.rs`, `bar_phase.rs` | Debouncing, protocol handlers |
| Events and polling side by side | `app/run.rs` | Interrupts and a superloop |
| A safe wrapper over raw calls | `platform/` | HAL over registers |

## Three sessions

About an hour each. Predict first, then run.

### 1. `app/writer.rs` and `core/refusal.rs`

- What happens to ten frames that arrive during one slow move?
- Why does only one thread move a window? What could go wrong with two?
- Why is `now` a parameter of `observe` instead of a call to `Instant::now()`?
- **Break it:** set `LASTING` to zero. Which test fails, and what would the user see?
- **Carry over:** sketch the writer as an RTOS task with a one-slot queue.

### 2. `core/layout`

- Why store cuts rather than the windows' frames?
- What makes neighbours share an edge exactly, even after rounding?
- **Break it:** make `fit` do nothing. Predict which tests fail before running them.
- **Build:** a `rotate` that turns columns into rows, test first.

### 3. `platform/timer.rs`, then `platform/element.rs`

- Core Foundation: "Create" and "Copy" hand over ownership, "Get" lends. Find one of each.
- Why can the timer be released right after it is added?
- A plain function travels through C as a `void *`. Which type must come out again?
- For each `unsafe` block: what must be true, and who makes sure of it?
- **Carry over:** compare with passing a context pointer to an interrupt callback.
