//! The bar of groups: an entry per group with its layout, app icons and name.
//!
//! Clicks in Mission Control never reach a window of ours, so the bar lets them through. The
//! click lands on the empty background, which closes Mission Control, and the press is
//! noticed from the button itself. So it may only sit where there is no thumbnail underneath.

use crate::app::{self, GroupId, views};
use crate::core::bar_phase::{BarPhase, Decision};
use crate::core::bar_spot::{Spot, spot as find_spot};
use crate::core::geometry::Rect;
use crate::platform::mouse::PressWatch;
use crate::platform::{screen, settings};
use crate::ui::{cocoa, overlay_panel, rect, rounded_view};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSAppearance, NSAppearanceCustomization, NSAppearanceNameVibrantDark, NSColor, NSFont, NSFontWeightMedium,
    NSFontWeightSemibold, NSImageScaling, NSImageView, NSLineBreakMode, NSPanel, NSRunningApplication,
    NSScreenSaverWindowLevel, NSTextField, NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial,
    NSVisualEffectState, NSVisualEffectView,
};
use objc2_foundation::NSString;
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// The bar's height, its layout pictures, and how round it and its entries are.
#[derive(Clone, Copy)]
struct Style {
    h: f64,
    pic: (f64, f64),
    radius: f64,
    inset: f64,
    font: f64,
    big: bool,
}

/// A preview of each group with the app icons in its tiles; and, where thumbnails leave no
/// room for that, a single line.
const BIG: Style = Style { h: 68.0, pic: (80.0, 48.0), radius: 15.0, inset: 4.0, font: 13.0, big: true };
const SLIM: Style = Style { h: 34.0, pic: (34.0, 20.0), radius: 11.0, inset: 3.0, font: 13.0, big: false };

const ICON: f64 = 18.0;
const PAD: f64 = 11.0;
const GAP: f64 = 8.0;
/// Mission Control shows the Dock even when it is set to hide.
const DOCK: f64 = 70.0;
/// Opacity per frame.
const FADE: f64 = 0.25;
/// Window sets to remember a place for.
const REMEMBER: usize = 60;
/// After a press Mission Control is most likely closing: stay away.
const AWAY: Duration = Duration::from_millis(700);

struct Entry {
    group: GroupId,
    view: Retained<NSView>,
    x: f64,
    w: f64,
    lit: bool,
}

struct Bar {
    style: Style,
    shows: Vec<views::BarEntry>,
    panel: Retained<NSPanel>,
    entries: Vec<Entry>,
    width: f64,
}

#[derive(Default)]
struct State {
    /// The big one and the single line.
    bars: Vec<Bar>,
    /// Which is up, and its top left from the top of the screen.
    up: Option<Spot>,
    alpha: f64,
    phase: BarPhase,
    press: PressWatch,
    pressed_at: Option<Instant>,
    /// Where it went for these windows before: they get the same thumbnails again, so it can
    /// come up along with them.
    known: HashMap<Key, Spot>,
}

/// The windows on screen, and the size of the screen.
type Key = (Vec<u32>, (u32, u32));

impl State {
    fn bar(&self) -> Option<&Bar> {
        self.bars.get(index(self.up?))
    }

    fn close(&mut self) {
        if let Some(b) = self.up.take().and_then(|s| self.bars.get_mut(index(s))) {
            b.panel.orderOut(None::<&AnyObject>);
            b.entries.iter_mut().for_each(|e| e.light(false));
        }
        self.phase.unplace();
        self.alpha = 0.0;
    }

    fn show(&mut self, spot: Spot, screen_h: f64) {
        if self.up == Some(spot) {
            return;
        }
        if let Some(old) = self.up.filter(|o| o.big != spot.big) {
            self.bars[index(old)].panel.orderOut(None::<&AnyObject>);
        }
        let b = &self.bars[index(spot)];
        let frame = Rect { x: spot.at.0, y: spot.at.1, w: b.width, h: b.style.h };
        b.panel.setAlphaValue(self.alpha);
        b.panel.setFrame_display(cocoa(frame, screen_h), true);
        b.panel.orderFrontRegardless();
        self.up = Some(spot);
    }

    /// Put it where the thumbnails leave room.
    fn place(&mut self, key: Key, taken: &[Rect], (sw, sh): (f64, f64)) {
        let [big, line] = &self.bars[..] else { return };
        let floor = screen::visible().bottom().min(sh - DOCK);
        let Some(spot) = find_spot((big.width, BIG.h), (line.width, SLIM.h), sw, floor, taken) else {
            self.known.remove(&key);
            self.close();
            return;
        };
        self.show(spot, sh);
        if self.phase.place() {
            if self.known.len() >= REMEMBER {
                self.known.clear();
            }
            self.known.insert(key, spot);
        }
    }

    /// The group whose entry is at a point.
    fn entry_at(&self, x: f64, y: f64) -> Option<GroupId> {
        let (at, b) = (self.up?.at, self.bar()?);
        let (x, y) = (x - at.0, y - at.1);
        let hit = |e: &&Entry| (e.x..=e.x + e.w).contains(&x);
        b.entries.iter().find(hit).filter(|_| (0.0..=b.style.h).contains(&y)).map(|e| e.group)
    }
}

fn index(s: Spot) -> usize {
    if s.big { 0 } else { 1 }
}

/// What the rest of the Mission Control UI needs to know.
#[derive(Default)]
pub struct Now {
    /// The group being pointed at.
    pub hot: Option<GroupId>,
    pub showing: bool,
    /// Something was pressed: everything of ours should be off the screen.
    pub away: bool,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::default();
}

impl Entry {
    fn light(&mut self, on: bool) {
        if self.lit == on {
            return;
        }
        self.lit = on;
        if let Some(layer) = self.view.layer() {
            let c = if on { NSColor::controlAccentColor() } else { NSColor::clearColor() };
            layer.setBackgroundColor(Some(&c.CGColor()));
        }
    }
}

fn entry(mtm: MainThreadMarker, z: Style, c: &views::BarEntry, x: f64, max_name: f64, first: bool) -> Entry {
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
    let view = rounded_view(mtm, rect(x, z.inset, w, h), z.radius - z.inset);

    let half = if z.big { 1.5 } else { 0.75 };
    for (r, pid) in &c.tiles {
        let f = rect(
            PAD + r.x * z.pic.0 + half,
            (h - z.pic.1) / 2.0 + (1.0 - r.y - r.h) * z.pic.1 + half,
            (r.w * z.pic.0 - 2.0 * half).max(1.0),
            (r.h * z.pic.1 - 2.0 * half).max(1.0),
        );
        let tile = rounded_view(mtm, f, if z.big { 5.0 } else { 2.0 });
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
    Entry { group: c.group, view, x, w, lit: false }
}

fn build(mtm: MainThreadMarker, z: Style, cards: Vec<views::BarEntry>, screen_w: f64) -> Bar {
    let each = (screen_w - 80.0) / cards.len().max(1) as f64;
    let max_name =
        (each - 2.0 * PAD - z.pic.0 - 2.0 * GAP - if z.big { 10.0 } else { 4.0 * (ICON + 2.0) }).clamp(0.0, 160.0);
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
    let panel = overlay_panel(mtm, NSScreenSaverWindowLevel);
    panel.setContentView(Some(&glass));
    Bar { style: z, shows: cards, panel, entries, width }
}

/// The button went down since the last `refresh`. Takes the bar off the screen; what was
/// pressed is worked out in `refresh`.
pub fn pressed() -> bool {
    STATE.with(|s| {
        let s = s.borrow();
        let pressed = s.press.peek();
        if let Some(b) = s.bar().filter(|_| pressed) {
            b.panel.setAlphaValue(0.0);
        }
        pressed
    })
}

/// Mission Control is closed.
pub fn hide() {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.close();
        s.pressed_at = None;
        s.phase = BarPhase::default();
        s.press.take();
    });
}

/// `calm`: how long no thumbnail has moved.
pub fn refresh(mtm: MainThreadMarker, calm: Duration, x: f64, y: f64) -> Now {
    let (Some(view), Some(screen)) = (views::bar(), screen::size()) else { return Now::default() };
    let (now, chosen) = STATE.with(|s| update(&mut s.borrow_mut(), mtm, view, calm, (x, y), screen));
    if let Some(id) = chosen {
        app::press_bar_entry(id);
    }
    now
}

/// Also the group that was pressed.
fn update(
    s: &mut State,
    mtm: MainThreadMarker,
    view: views::Bar,
    calm: Duration,
    (x, y): (f64, f64),
    screen: (f64, f64),
) -> (Now, Option<GroupId>) {
    // built while the thumbnails still move, so it is ready when they stop
    if s.bars.first().is_none_or(|b| b.shows != view.entries) {
        s.close();
        s.bars = [BIG, SLIM].into_iter().map(|z| build(mtm, z, view.entries.clone(), screen.0)).collect();
    }
    if s.press.take() {
        s.pressed_at = Some(Instant::now());
        let chosen = s.entry_at(x, y);
        s.close();
        return (Now { away: true, ..Now::default() }, chosen);
    }
    if s.pressed_at.is_some_and(|t| t.elapsed() < AWAY) {
        s.close();
        return (Now { away: true, ..Now::default() }, None);
    }
    if !settings::shows_bar() {
        s.close();
        return (Now::default(), None);
    }
    let key = (view.windows, (screen.0 as u32, screen.1 as u32));
    match s.phase.step(view.area, calm, s.up.is_some()) {
        Decision::Hide => s.close(),
        Decision::Place => s.place(key, &view.taken, screen),
        Decision::Guess => {
            if let Some(spot) = s.known.get(&key).copied() {
                s.show(spot, screen.1);
            }
        }
        Decision::Stay => {}
    }
    let hot = s.entry_at(x, y);
    let Some(up) = s.up else { return (Now::default(), None) };
    s.alpha = (s.alpha + FADE).min(1.0);
    let b = &mut s.bars[index(up)];
    b.panel.setAlphaValue(s.alpha);
    b.entries.iter_mut().for_each(|e| e.light(hot == Some(e.group)));
    (Now { hot, showing: true, away: false }, None)
}
