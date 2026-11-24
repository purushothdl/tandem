//! Every call into macOS, wrapped so the rest of the program needs no `unsafe`.

pub mod apps;
pub mod dock;
pub mod element;
pub mod focus;
pub mod hotkeys;
pub mod log;
pub mod mouse;
pub mod screen;
pub mod timer;
pub mod windows;

use crate::core::geometry::Rect;

// Core Graphics' own structs, with its memory layout.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CGSize {
    w: f64,
    h: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}

impl From<CGRect> for Rect {
    fn from(r: CGRect) -> Rect {
        Rect { x: r.origin.x, y: r.origin.y, w: r.size.w, h: r.size.h }
    }
}
