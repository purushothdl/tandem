//! While Mission Control is open: one bar above the thumbnails with an entry per group
//! (layout, app icons, name).
//! Clicks in Mission Control never reach a window of ours, so the bar lets them through:
//! the click lands on the empty background, which closes Mission Control, and the press is
//! noticed from the button itself. Hence only where there is no thumbnail underneath.

use crate::app::{self, Card};
use crate::labels::{panel, rect};
use crate::layout::Rect;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameVibrantDark, NSColor, NSEvent, NSFont, NSFontWeightMedium,
    NSFontWeightSemibold, NSImageScaling, NSImageView, NSLineBreakMode, NSPanel, NSRunningApplication, NSScreen, NSTextField, NSView,
    NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView,
};
use objc2_foundation::NSString;
use std::cell::RefCell;
use std::time::{Duration, Instant};

/// The bar's height, its layout pictures, and how round it and its entries are.
#[derive(Clone, Copy)]
struct Size {
    h: f64,
    pic: (f64, f64),
    radius: f64,
    inset: f64,
    font: f64,
    big: bool,
}

/// A preview of each group with the app icons in its tiles; and, where thumbnails leave no
/// room for that, a single line.
const BIG: Size = Size { h: 68.0, pic: (80.0, 48.0), radius: 15.0, inset: 4.0, font: 13.0, big: true };
const SLIM: Size = Size { h: 34.0, pic: (34.0, 20.0), radius: 11.0, inset: 3.0, font: 13.0, big: false };

const ICON: f64 = 18.0;
const PAD: f64 = 11.0;
const GAP: f64 = 8.0;
/// Between the bar and any thumbnail.
const CLEAR: f64 = 20.0;
/// The same for the single line, which has less room.
const ABOVE: f64 = 8.0;
/// The closed strip of desktops at the top.
const SPACES: f64 = 40.0;
/// Mission Control shows the Dock even when it is set to hide.
const DOCK: f64 = 70.0;
/// Thumbnails must have been still this long.
const STILL: Duration = Duration::from_millis(90);
/// After a press Mission Control is most likely closing: stay away.
const AWAY: Duration = Duration::from_millis(700);

struct Entry {
    group: u32,
    view: Retained<NSView>,
    x: f64,
    w: f64,
    hot: bool,
}

struct Bar {
    size: Size,
    cards: Vec<Card>,
    panel: Retained<NSPanel>,
    entries: Vec<Entry>,
    width: f64,
}

#[derive(Default)]
struct State {
    /// The big one and the single line.
    bars: Vec<Bar>,
    /// Which is up.
    which: usize,
    /// Top left, from the top of the screen.
    at: Option<(f64, f64)>,
    alpha: f64,
    crowded: bool,
    presses: u32,
    down: bool,
    away: Option<Instant>,
}

/// What the labels need to know.
#[derive(Default)]
pub struct Now {
    /// The group being pointed at.
    pub hot: Option<u32>,
    pub showing: bool,
    /// Something was pressed: everything of ours should be off the screen.
    pub away: bool,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
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
    if let Some(layer) = view.layer() {
        let c = if hot { NSColor::controlAccentColor() } else { NSColor::clearColor() };
        layer.setBackgroundColor(Some(&c.CGColor()));
    }
}

fn rounded(mtm: MainThreadMarker, f: objc2_foundation::NSRect, radius: f64) -> Retained<NSView> {
    let v = NSView::initWithFrame(mtm.alloc(), f);
    v.setWantsLayer(true);
    if let Some(layer) = v.layer() {
        layer.setCornerRadius(radius);
    }
    v
}

fn entry(mtm: MainThreadMarker, z: Size, c: &Card, x: f64, max_name: f64, first: bool) -> Entry {
    let text = NSTextField::labelWithString(&NSString::from_str(&c.name), mtm);
    let weight = unsafe { if first { NSFontWeightSemibold } else { NSFontWeightMedium } };
    text.setFont(Some(&NSFont::systemFontOfSize_weight(z.font, weight)));
    text.setTextColor(Some(&NSColor::whiteColor()));
    text.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    text.sizeToFit();
    let ts = text.frame().size;
    let tw = ts.width.min(max_name);
    let mut pids: Vec<i32> = Vec::new();
    for (_, p) in &c.tiles {
        if !pids.contains(p) {
            pids.push(*p);
        }
    }
    let icon = |p: &i32| NSRunningApplication::runningApplicationWithProcessIdentifier(*p)?.icon();
    let icons: Vec<_> = if z.big { Vec::new() } else { pids.iter().filter_map(icon).take(4).collect() };
    let icons_w = icons.len() as f64 * (ICON + 2.0);
    let gap = if z.big { 4.0 } else { GAP - 2.0 };
    let w = PAD + z.pic.0 + GAP + icons_w + gap + tw + PAD + if z.big { 6.0 } else { 0.0 };
    let h = z.h - 2.0 * z.inset;
    let view = rounded(mtm, rect(x, z.inset, w, h), z.radius - z.inset);

    let half = if z.big { 1.5 } else { 0.75 };
    for (r, pid) in &c.tiles {
        let f = rect(
            PAD + r.x * z.pic.0 + half,
            (h - z.pic.1) / 2.0 + (1.0 - r.y - r.h) * z.pic.1 + half,
            (r.w * z.pic.0 - 2.0 * half).max(1.0),
            (r.h * z.pic.1 - 2.0 * half).max(1.0),
        );
        let tile = rounded(mtm, f, if z.big { 5.0 } else { 2.0 });
        if let Some(layer) = tile.layer() {
            let white = if z.big { 0.22 } else { 0.75 };
            layer.setBackgroundColor(Some(&NSColor::colorWithWhite_alpha(1.0, white).CGColor()));
        }
        if let Some(img) = z.big.then(|| icon(pid)).flatten() {
            let s = (f.size.width.min(f.size.height) - 2.0).clamp(8.0, 30.0);
            let iv = NSImageView::imageViewWithImage(&img, mtm);
            iv.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
            iv.setFrame(rect((f.size.width - s) / 2.0, (f.size.height - s) / 2.0, s, s));
            tile.addSubview(&iv);
        }
        view.addSubview(&tile);
    }
    let x1 = PAD + z.pic.0 + GAP;
    for (i, img) in icons.iter().enumerate() {
        let iv = NSImageView::imageViewWithImage(img, mtm);
        iv.setFrame(rect(x1 + i as f64 * (ICON + 2.0), (h - ICON) / 2.0, ICON, ICON));
        view.addSubview(&iv);
    }
    text.setFrame(rect(x1 + icons_w + gap, (h - ts.height) / 2.0, tw, ts.height));
    view.addSubview(&text);
    Entry { group: c.group, view, x, w, hot: false }
}

fn build(mtm: MainThreadMarker, z: Size, cards: Vec<Card>, screen_w: f64) -> Bar {
    let each = (screen_w - 80.0) / cards.len().max(1) as f64;
    let max_name = (each - 2.0 * PAD - z.pic.0 - 2.0 * GAP - if z.big { 10.0 } else { 4.0 * (ICON + 2.0) }).clamp(0.0, 160.0);
    let mut entries: Vec<Entry> = Vec::new();
    let mut x = z.inset;
    for (i, c) in cards.iter().enumerate() {
        let e = entry(mtm, z, c, x, max_name, i == 0);
        x += e.w + 2.0;
        entries.push(e);
    }
    let width = x - 2.0 + z.inset;
    let glass = NSVisualEffectView::initWithFrame(mtm.alloc(), rect(0.0, 0.0, width, z.h));
    glass.setMaterial(NSVisualEffectMaterial::HUDWindow);
    glass.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
    glass.setState(NSVisualEffectState::Active);
    glass.setAppearance(NSAppearance::appearanceNamed(unsafe { NSAppearanceNameVibrantDark }).as_deref());
    glass.setWantsLayer(true);
    if let Some(layer) = glass.layer() {
        layer.setCornerRadius(z.radius);
        layer.setMasksToBounds(true);
        layer.setBorderWidth(0.5);
        layer.setBorderColor(Some(&NSColor::colorWithWhite_alpha(1.0, 0.22).CGColor()));
    }
    for e in &entries {
        glass.addSubview(&e.view);
    }
    let panel = panel(mtm);
    panel.setContentView(Some(&glass));
    Bar { size: z, cards, panel, entries, width }
}

fn close(s: &mut State) {
    if let Some(b) = s.bars.get_mut(s.which).filter(|_| s.at.is_some()) {
        b.panel.orderOut(None::<&AnyObject>);
        for e in &mut b.entries {
            e.hot = false;
            paint(&e.view, false);
        }
    }
    s.at = None;
    s.alpha = 0.0;
}

/// Mission Control is closed.
pub fn hide() {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        close(&mut s);
        s.away = None;
        s.presses = presses();
        s.down = NSEvent::pressedMouseButtons() & 1 != 0;
    });
}

/// The places to try: the big bar at the top, then at the bottom; then the single line.
/// `widths`: of the big bar and the line. Which of them, and its top left.
fn spot(widths: [f64; 2], screen_w: f64, floor: f64, taken: &[Rect]) -> Option<(usize, (f64, f64))> {
    let top = taken.iter().map(|t| t.y).fold(f64::INFINITY, f64::min);
    let (w, h) = (widths[0], BIG.h);
    if let Some(p) = high_spot(w, h, screen_w, top, taken).or_else(|| low_spot(w, h, screen_w, floor, taken)) {
        return Some((0, p));
    }
    let (w, h) = (widths[1], SLIM.h);
    let y = top - ABOVE - h;
    if y >= SPACES && y.is_finite() && w <= screen_w {
        return Some((1, ((screen_w - w) / 2.0, y)));
    }
    low_spot(w, h, screen_w, floor, taken).map(|p| (1, p))
}

/// Places from left to right worth trying, nearest the middle first.
fn columns(w: f64, screen_w: f64, taken: &[Rect]) -> Vec<f64> {
    const SIDE: f64 = 24.0;
    let mid = (screen_w - w) / 2.0;
    let mut xs = vec![mid, SIDE, screen_w - SIDE - w];
    // just beside each obstacle
    xs.extend(taken.iter().flat_map(|t| [t.x - CLEAR - w, t.right() + CLEAR]));
    xs.retain(|x| *x >= SIDE && x + w <= screen_w - SIDE);
    xs.sort_by(|a, b| (a - mid).abs().total_cmp(&(b - mid).abs()));
    xs
}

fn in_column<'a>(x: f64, w: f64, taken: &'a [Rect]) -> impl Iterator<Item = &'a Rect> {
    taken.iter().filter(move |t| t.x < x + w + CLEAR - 0.5 && t.right() > x - CLEAR + 0.5)
}

/// Level with the topmost thumbnails, or higher if those below need it. Thumbnails move
/// down with the strip of desktops when it opens, and so does this.
fn high_spot(w: f64, h: f64, screen_w: f64, top: f64, taken: &[Rect]) -> Option<(f64, f64)> {
    columns(w, screen_w, taken).into_iter().find_map(|x| {
        let below = in_column(x, w, taken).map(|t| t.y).fold(f64::INFINITY, f64::min);
        let y = top.min(below - CLEAR - h);
        (y.is_finite() && y >= SPACES + 10.0).then_some((x, y))
    })
}

/// Along the bottom.
fn low_spot(w: f64, h: f64, screen_w: f64, floor: f64, taken: &[Rect]) -> Option<(f64, f64)> {
    columns(w, screen_w, taken).into_iter().find_map(|x| {
        let low = in_column(x, w, taken).filter(|t| t.y < floor).map(|t| t.bottom()).fold(0.0, f64::max);
        let room = floor - low;
        (room >= h + 2.0 * CLEAR).then(|| (x, floor - h - ((room - h) / 2.0).min(36.0)))
    })
}

/// `calm`: how long no thumbnail has moved.
pub fn refresh(mtm: MainThreadMarker, calm: Duration, mx: f64, my: f64, screen_h: f64) -> Now {
    let Some((cards, taken)) = app::mission_control_cards() else { return Now::default() };
    let mut chosen = None;
    let now = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let s = &mut *s;
        let Some(screen) = NSScreen::screens(mtm).firstObject() else { return Now::default() };
        let (sw, free) = (screen.frame().size.width, screen.visibleFrame());

        // built while the thumbnails still move, so it is ready when they stop
        if s.bars.first().is_none_or(|b| b.cards != cards) {
            close(s);
            s.bars = [BIG, SLIM].into_iter().map(|z| build(mtm, z, cards.clone(), sw)).collect();
        }

        let n = presses();
        let down = NSEvent::pressedMouseButtons() & 1 != 0;
        let pressed = s.presses != n || (down && !s.down);
        (s.presses, s.down) = (n, down);
        if pressed {
            s.away = Some(Instant::now());
            if let (Some(b), Some(at)) = (s.bars.get(s.which), s.at) {
                chosen = under(b, at, mx, my);
            }
        }
        if s.away.is_some_and(|t| t.elapsed() < AWAY) {
            close(s);
            return Now { away: true, ..Default::default() };
        }
        if calm < STILL {
            close(s);
            return Now::default();
        }
        if s.at.is_none() {
            let floor = (screen_h - free.origin.y).min(screen_h - DOCK);
            let found = spot([s.bars[0].width, s.bars[1].width], sw, floor, &taken);
            if found.is_none() && !s.crowded {
                app::log(&format!("no room for the group bar: floor {floor:.0}, taken {taken:?}"));
            }
            s.crowded = found.is_none();
            let Some((which, (x, y))) = found else { return Now::default() };
            s.which = which;
            let b = &s.bars[which];
            b.panel.setAlphaValue(0.0);
            b.panel.setFrame_display(rect(x, screen_h - y - b.size.h, b.width, b.size.h), true);
            b.panel.orderFrontRegardless();
            s.at = Some((x, y));
        }
        let b = &mut s.bars[s.which];
        if s.alpha < 1.0 {
            s.alpha = (s.alpha + 0.25).min(1.0);
            b.panel.setAlphaValue(s.alpha);
        }
        let hot = s.at.and_then(|at| under(b, at, mx, my));
        for e in &mut b.entries {
            let over = hot == Some(e.group);
            if over != e.hot {
                e.hot = over;
                paint(&e.view, over);
            }
        }
        Now { hot, showing: true, away: false }
    });
    if let Some(id) = chosen {
        app::pick_card(id);
    }
    now
}

fn under(b: &Bar, (x, y): (f64, f64), mx: f64, my: f64) -> Option<u32> {
    if my < y || my > y + b.size.h {
        return None;
    }
    b.entries.iter().find(|e| mx >= x + e.x && mx <= x + e.x + e.w).map(|e| e.group)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }

    // measured on a real 1920x1080 display: two groups, the middle taken down to the Dock
    fn measured() -> Vec<Rect> {
        vec![
            r(1372.0, 89.0, 530.0, 543.0),
            r(136.0, 161.0, 530.0, 544.0),
            r(702.0, 435.0, 634.0, 543.0),
            r(18.0, 89.0, 427.0, 543.0),
            r(969.0, 923.0, 100.0, 109.0),
            r(283.0, 650.0, 118.0, 109.0),
            r(1587.0, 577.0, 100.0, 109.0),
        ]
    }

    fn clear(me: Rect) -> bool {
        measured().iter().all(|t| me.right() <= t.x || me.x >= t.right() || me.y >= t.bottom() || me.bottom() <= t.y)
    }

    #[test]
    fn big_at_the_top_between_thumbnails() {
        let (k, (x, y)) = spot([560.0, 500.0], 1920.0, 1010.0, &measured()).expect("room");
        assert!(k == 0 && y > SPACES && y < 200.0 && clear(r(x, y, 560.0, BIG.h)));
    }

    #[test]
    fn big_at_the_bottom_if_too_wide_for_that() {
        let (k, (x, y)) = spot([640.0, 500.0], 1920.0, 1010.0, &measured()).expect("room");
        assert!(k == 0 && clear(r(x, y, 640.0, BIG.h)));
    }

    #[test]
    fn a_line_when_there_is_only_that_much() {
        let full = [r(20.0, 89.0, 1880.0, 880.0)];
        let (k, (_, y)) = spot([560.0, 500.0], 1920.0, 1010.0, &full).expect("room");
        assert!(k == 1 && y >= SPACES && y + SLIM.h < 89.0);
    }

    #[test]
    fn none_when_full() {
        assert!(spot([560.0, 500.0], 1920.0, 1010.0, &[r(0.0, 60.0, 1920.0, 930.0)]).is_none());
    }
}
