//! What is drawn on one thumbnail: a pill with its group's name, and an outline.

use crate::app::views::Thumbnail;
use crate::core::geometry::Rect;
use crate::platform::screen;
use crate::ui::{cocoa, overlay_panel, rect, rounded_view};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{
    NSColor, NSFont, NSImageView, NSPanel, NSRunningApplication, NSScreenSaverWindowLevel, NSTextField,
};
use objc2_foundation::NSString;

const PILL_H: f64 = 30.0;
/// From the top of the thumbnail.
const PILL_Y: f64 = 10.0;
const MAX_NAME: f64 = 220.0;
const ICON: f64 = 20.0;
const OUTLINE: f64 = 4.0;
/// Above the thumbnails, below the app icons Mission Control draws on layer 17.
const OUTLINE_LEVEL: isize = 16;

/// Opacity per frame.
const FADE_IN: f64 = 0.16;
const FADE_OUT: f64 = 0.22;

struct Fading {
    panel: Retained<NSPanel>,
    alpha: f64,
}

impl Fading {
    /// A step towards where it should be.
    fn step(&mut self, on: bool) {
        let next = if on { (self.alpha + FADE_IN).min(1.0) } else { (self.alpha - FADE_OUT).max(0.0) };
        if next == self.alpha {
            return;
        }
        if next == 0.0 {
            return self.hide();
        }
        self.panel.setAlphaValue(next);
        if self.alpha == 0.0 {
            self.panel.orderFrontRegardless();
        }
        self.alpha = next;
    }

    fn hide(&mut self) {
        if self.alpha > 0.0 {
            self.panel.setAlphaValue(0.0); // at once; taking it off the screen can lag behind
            self.panel.orderOut(None::<&AnyObject>);
            self.alpha = 0.0;
        }
    }
}

pub struct Mark {
    pub name: String,
    at: Rect,
    pill: Fading,
    pill_w: f64,
    outline: Fading,
}

impl Mark {
    pub fn new(mtm: MainThreadMarker, t: &Thumbnail) -> Mark {
        let (pill, pill_w) = pill(mtm, t);
        Mark { name: t.name.clone(), at: Rect::default(), pill, pill_w, outline: outline(mtm) }
    }

    /// True if that was news.
    pub fn move_to(&mut self, at: Rect) -> bool {
        std::mem::replace(&mut self.at, at) != at
    }

    pub fn is_outlined(&self) -> bool {
        self.outline.alpha > 0.0
    }

    pub fn update(&mut self, pill: bool, outline: bool) {
        if self.pill.alpha == 0.0 && self.outline.alpha == 0.0 && (pill || outline) {
            self.put_in_place();
        }
        self.pill.step(pill);
        self.outline.step(outline);
    }

    pub fn hide(&mut self) {
        self.pill.hide();
        self.outline.hide();
    }

    fn put_in_place(&self) {
        let Some((_, screen_h)) = screen::size() else { return };
        let r = self.at;
        let pill = Rect { x: r.x + (r.w - self.pill_w) / 2.0, y: r.y + PILL_Y, w: self.pill_w, h: PILL_H };
        self.pill.panel.setFrame_display(cocoa(pill, screen_h), true);
        let around = Rect { x: r.x - OUTLINE, y: r.y - OUTLINE, w: r.w + 2.0 * OUTLINE, h: r.h + 2.0 * OUTLINE };
        self.outline.panel.setFrame_display(cocoa(around, screen_h), true);
    }
}

fn pill(mtm: MainThreadMarker, t: &Thumbnail) -> (Fading, f64) {
    let text = NSTextField::labelWithString(&NSString::from_str(&t.name), mtm);
    text.setFont(Some(&NSFont::boldSystemFontOfSize(12.0)));
    text.setTextColor(Some(&NSColor::whiteColor()));
    text.sizeToFit();
    let size = text.frame().size;
    let text_w = size.width.min(MAX_NAME);
    let icon = |p: &i32| NSRunningApplication::runningApplicationWithProcessIdentifier(*p)?.icon();
    let icons: Vec<_> = t.pids.iter().filter_map(icon).take(4).collect();
    let icons_w = icons.len() as f64 * (ICON + 3.0);
    let w = 10.0 + icons_w + 4.0 + text_w + 12.0;

    let view = rounded_view(mtm, rect(0.0, 0.0, w, PILL_H), PILL_H / 2.0);
    if let Some(layer) = view.layer() {
        layer.setBackgroundColor(Some(&NSColor::colorWithWhite_alpha(0.08, 0.88).CGColor()));
        layer.setBorderWidth(1.0);
        layer.setBorderColor(Some(&NSColor::colorWithWhite_alpha(1.0, 0.18).CGColor()));
    }
    for (i, img) in icons.iter().enumerate() {
        let iv = NSImageView::imageViewWithImage(img, mtm);
        iv.setFrame(rect(10.0 + i as f64 * (ICON + 3.0), (PILL_H - ICON) / 2.0, ICON, ICON));
        view.addSubview(&iv);
    }
    text.setFrame(rect(10.0 + icons_w + 4.0, (PILL_H - size.height) / 2.0, text_w, size.height));
    view.addSubview(&text);
    let panel = overlay_panel(mtm, NSScreenSaverWindowLevel);
    panel.setContentView(Some(&view));
    (Fading { panel, alpha: 0.0 }, w)
}

fn outline(mtm: MainThreadMarker) -> Fading {
    let view = rounded_view(mtm, rect(0.0, 0.0, 10.0, 10.0), 12.0);
    if let Some(layer) = view.layer() {
        layer.setBorderWidth(OUTLINE);
        layer.setBorderColor(Some(&NSColor::controlAccentColor().CGColor()));
    }
    let panel = overlay_panel(mtm, OUTLINE_LEVEL);
    panel.setContentView(Some(&view));
    Fading { panel, alpha: 0.0 }
}
