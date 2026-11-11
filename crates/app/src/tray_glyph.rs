//! The tray glyph, shared by every platform's tray.

/// Side of the square glyph, in pixels.
pub const N: usize = 36;

/// Template glyph matching the app icon: a 3/4 progress ring around a down arrow.
/// Drawn at 36x36 (@2x of 18pt) with anti-aliased edges via distance fields.
/// With `tint` off it is a black template (macOS tints it itself); with it on
/// it's drawn in the app's indigo, which reads on both light and dark taskbars.
pub fn rgba(tint: bool) -> Vec<u8> {
    let c = (N as f32 - 1.) / 2.;
    let (ring_r, ring_w, stroke) = (14.5f32, 3.2f32, 3.2f32);
    // Distance from p to segment ab.
    let seg = |px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32| {
        let (dx, dy) = (bx - ax, by - ay);
        let t = (((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)).clamp(0., 1.);
        ((px - ax - t * dx).powi(2) + (py - ay - t * dy).powi(2)).sqrt()
    };
    let mut px = vec![0u8; N * N * 4];
    for y in 0..N {
        for x in 0..N {
            let (fx, fy) = (x as f32, y as f32);
            let (dx, dy) = (fx - c, fy - c);
            // Angle clockwise from 12 o'clock, 0..1.
            let ang = (dx.atan2(-dy) / std::f32::consts::TAU).rem_euclid(1.);
            let ring = (dx.hypot(dy) - ring_r).abs() - ring_w / 2.;
            // Round caps at the arc ends (0 and 0.75 turns).
            let cap = |a: f32| {
                let a = a * std::f32::consts::TAU;
                (fx - (c + ring_r * a.sin())).hypot(fy - (c - ring_r * a.cos())) - ring_w / 2.
            };
            let arc = if ang <= 0.75 { ring } else { cap(0.).min(cap(0.75)) };
            let arrow = seg(fx, fy, c, c - 7.5, c, c + 7.).min(seg(fx, fy, c - 6., c + 1., c, c + 7.)).min(seg(fx, fy, c + 6., c + 1., c, c + 7.))
                - stroke / 2.;
            let d = arc.min(arrow);
            let a = (0.5 - d).clamp(0., 1.);
            let p = &mut px[(y * N + x) * 4..][..4];
            if tint {
                p[..3].copy_from_slice(&[0x5E, 0x6A, 0xD2]);
            }
            p[3] = (a * 255.) as u8;
        }
    }
    px
}

