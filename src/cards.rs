//! While Mission Control is open: a row of cards at the bottom, one per group.
//! Clicks in Mission Control never reach a window of ours, so the cards let them through:
//! the click lands on the empty background, which closes Mission Control, and the press is
//! noticed from the button itself. Hence only where there is no thumbnail underneath.

use crate::app::{self, Card};
use crate::labels::{fade, panel, rect};
use crate::layout::Rect;
use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSColor, NSEvent, NSFont, NSImageScaling, NSImageView, NSLineBreakMode, NSPanel, NSRunningApplication, NSScreen, NSTextField, NSView,
};
use objc2_foundation::NSString;
use std::cell::RefCell;

const H: f64 = 58.0;
const PIC: (f64, f64) = (66.0, 42.0);
const PAD: f64 = 8.0;
const GAP: f64 = 10.0;
/// Kept free between the row and the thumbnails, and the Dock.
const CLEAR: f64 = 12.0;
/// Mission Control shows the Dock even when it is set to hide.
const DOCK: f64 = 70.0;

struct Shown {
    card: Card,
    panel: Retained<NSPanel>,
    view: Retained<NSView>,
    width: f64,
    x: f64,
    /// From the top of the screen.
    y: f64,
    hot: bool,
}

#[derive(Default)]
struct Row {
    cards: Vec<Shown>,
    on: Vec<f64>,
    visible: bool,
    presses: u32,
    down: bool,
    crowded: bool,
}

thread_local! {
    static ROW: RefCell<Row> = RefCell::new(Row::default());
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGEventSourceCounterForEventType(state: i32, kind: u32) -> u32;
}

/// How often the left button went down so far: a count, so a quick tap is not missed.
fn presses() -> u32 {
    unsafe { CGEventSourceCounterForEventType(1, 1) }
}

fn paint(view: &NSView, hot: bool) {
    let Some(layer) = view.layer() else { return };
    let (fill, edge) = if hot {
        (NSColor::controlAccentColor(), NSColor::colorWithWhite_alpha(1.0, 0.35))
    } else {
        (NSColor::colorWithWhite_alpha(0.08, 0.88), NSColor::colorWithWhite_alpha(1.0, 0.18))
    };
    layer.setBackgroundColor(Some(&fill.CGColor()));
    layer.setBorderColor(Some(&edge.CGColor()));
}

fn build(mtm: MainThreadMarker, c: Card, max_name: f64) -> Shown {
    let text = NSTextField::labelWithString(&NSString::from_str(&c.name), mtm);
    text.setFont(Some(&NSFont::boldSystemFontOfSize(13.0)));
    text.setTextColor(Some(&NSColor::whiteColor()));
    text.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    text.sizeToFit();
    let ts = text.frame().size;
    let tw = ts.width.min(max_name);
    let width = PAD + PIC.0 + 10.0 + tw + 14.0;

    let view = NSView::initWithFrame(mtm.alloc(), rect(0.0, 0.0, width, H));
    view.setWantsLayer(true);
    if let Some(layer) = view.layer() {
        layer.setCornerRadius(14.0);
        layer.setBorderWidth(1.0);
    }
    paint(&view, false);
    for (r, pid) in &c.tiles {
        let f = rect(
            PAD + r.x * PIC.0 + 1.0,
            PAD + (1.0 - r.y - r.h) * PIC.1 + 1.0,
            (r.w * PIC.0 - 2.0).max(1.0),
            (r.h * PIC.1 - 2.0).max(1.0),
        );
        let tile = NSView::initWithFrame(mtm.alloc(), f);
        tile.setWantsLayer(true);
        if let Some(layer) = tile.layer() {
            layer.setCornerRadius(3.0);
            layer.setBackgroundColor(Some(&NSColor::colorWithWhite_alpha(1.0, 0.22).CGColor()));
        }
        if let Some(img) = NSRunningApplication::runningApplicationWithProcessIdentifier(*pid).and_then(|a| a.icon()) {
            let s = (f.size.width.min(f.size.height) - 2.0).clamp(6.0, 20.0);
            let iv = NSImageView::imageViewWithImage(&img, mtm);
            iv.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
            iv.setFrame(rect((f.size.width - s) / 2.0, (f.size.height - s) / 2.0, s, s));
            tile.addSubview(&iv);
        }
        view.addSubview(&tile);
    }
    text.setFrame(rect(PAD + PIC.0 + 10.0, (H - ts.height) / 2.0, tw, ts.height));
    view.addSubview(&text);
    let p = panel(mtm);
    p.setContentView(Some(&view));
    Shown { card: c, panel: p, view, width, x: 0.0, y: 0.0, hot: false }
}

fn close(row: &mut Row) {
    for (c, on) in row.cards.iter().zip(&mut row.on) {
        if *on > 0.0 {
            c.panel.orderOut(None::<&AnyObject>);
            *on = 0.0;
        }
    }
    row.visible = false;
}

pub fn hide() {
    ROW.with(|r| close(&mut r.borrow_mut()));
}

/// Where a block of this size can go without covering a thumbnail: along the bottom, as
/// near the middle as possible, and not far up the screen.
fn free_spot(w: f64, h: f64, screen_w: f64, floor: f64, taken: &[Rect]) -> Option<(f64, f64)> {
    const SIDE: f64 = 24.0;
    let mid = (screen_w - w) / 2.0;
    let mut xs = vec![mid, SIDE, screen_w - SIDE - w];
    // just beside each obstacle
    xs.extend(taken.iter().flat_map(|t| [t.x - CLEAR - w, t.right() + CLEAR]));
    xs.retain(|x| *x >= SIDE && x + w <= screen_w - SIDE);
    xs.sort_by(|a, b| (a - mid).abs().total_cmp(&(b - mid).abs()));
    xs.into_iter().find_map(|x| {
        let low = taken
            .iter()
            .filter(|t| t.x < x + w + CLEAR - 0.5 && t.right() > x - CLEAR + 0.5 && t.y < floor)
            .map(|t| t.bottom())
            .fold(0.0, f64::max);
        let room = floor - low;
        (room >= h + 2.0 * CLEAR).then(|| (x, floor - h - ((room - h) / 2.0).min(36.0)))
    })
}

/// The group whose card is under the pointer. `still`: no thumbnail is moving.
pub fn refresh(mtm: MainThreadMarker, still: bool, mx: f64, my: f64, screen_h: f64) -> Option<u32> {
    let (cards, taken) = app::mission_control_cards()?;
    let (hot, chosen) = ROW.with(|r| -> Option<(Option<u32>, Option<u32>)> {
        let mut row = r.borrow_mut();
        let row = &mut *row;
        if !still && !row.visible {
            return None;
        }
        let screen = NSScreen::screens(mtm).firstObject()?;
        let (full, free) = (screen.frame(), screen.visibleFrame());
        if !row.cards.iter().map(|s| &s.card).eq(cards.iter()) {
            close(row);
            let each = (full.size.width - 80.0) / cards.len().max(1) as f64 - GAP;
            let max_name = (each - PAD - PIC.0 - 24.0).clamp(0.0, 170.0);
            row.cards = cards.into_iter().map(|c| build(mtm, c, max_name)).collect();
            row.on = vec![0.0; row.cards.len()];
        }
        if !row.visible {
            let n = row.cards.len() as f64;
            let total = row.cards.iter().map(|c| c.width + GAP).sum::<f64>() - GAP;
            let widest = row.cards.iter().map(|c| c.width).fold(0.0, f64::max);
            let (sw, floor) = (full.size.width, (screen_h - free.origin.y).min(screen_h - DOCK));
            // side by side, or one above the other if that is the only way they fit
            let side = free_spot(total, H, sw, floor, &taken).map(|p| (p, false));
            let spot = side.or_else(|| free_spot(widest, n * (H + GAP) - GAP, sw, floor, &taken).map(|p| (p, true)));
            if spot.is_none() != row.crowded {
                row.crowded = spot.is_none();
                if row.crowded {
                    let lows: Vec<String> = taken.iter().map(|t| format!("{:.0},{:.0} {:.0}x{:.0}", t.x, t.y, t.w, t.h)).collect();
                    app::log(&format!("no room for the group cards: floor {floor:.0}, taken {lows:?}"));
                }
            }
            let ((mut x, mut y), stacked) = spot?;
            for c in &mut row.cards {
                (c.x, c.y) = (x, y);
                c.panel.setFrame_display(rect(x, screen_h - y - H, c.width, H), true);
                if stacked {
                    y += H + GAP;
                } else {
                    x += c.width + GAP;
                }
            }
            row.visible = true;
            row.presses = presses();
            row.down = NSEvent::pressedMouseButtons() & 1 != 0;
        }
        let mut hot = None;
        for (c, on) in row.cards.iter_mut().zip(&mut row.on) {
            fade(&c.panel, true, on);
            let over = mx >= c.x && mx <= c.x + c.width && my >= c.y && my <= c.y + H;
            if over {
                hot = Some(c.card.group);
            }
            if over != c.hot {
                c.hot = over;
                paint(&c.view, over);
            }
        }
        let n = presses();
        let down = NSEvent::pressedMouseButtons() & 1 != 0;
        let pressed = std::mem::replace(&mut row.presses, n) != n || (down && !row.down);
        row.down = down;
        Some((hot, hot.filter(|_| pressed)))
    })?;
    if let Some(id) = chosen {
        hide();
        app::pick_card(id);
        return None;
    }
    hot
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }

    // measured on a real 1920x1080 display: two groups, the middle taken down to the Dock
    #[test]
    fn beside_a_low_thumbnail() {
        let taken = [
            r(1372.0, 89.0, 530.0, 543.0),
            r(136.0, 161.0, 530.0, 544.0),
            r(702.0, 435.0, 634.0, 543.0),
            r(18.0, 89.0, 427.0, 543.0),
            r(969.0, 923.0, 100.0, 109.0),
            r(283.0, 650.0, 118.0, 109.0),
            r(1587.0, 577.0, 100.0, 109.0),
        ];
        let (x, y) = free_spot(500.0, H, 1920.0, 1010.0, &taken).expect("room");
        let me = r(x, y, 500.0, H);
        assert!(y > 760.0 && me.bottom() <= 1010.0);
        for t in taken {
            assert!(me.right() <= t.x || me.x >= t.right() || me.y >= t.bottom() || me.bottom() <= t.y);
        }
    }

    #[test]
    fn none_when_full() {
        assert!(free_spot(500.0, H, 1920.0, 1010.0, &[r(0.0, 60.0, 1920.0, 930.0)]).is_none());
    }
}
