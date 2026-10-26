//! StackED's icon, drawn rather than kept as a picture: the stack's patches
//! as cards, the one in front a diff with the cursor at the end of a line,
//! on a dark rounded square, as macOS draws an app's. Drawn at any size: for
//! the window and the Dock while StackED runs, and for StackED.app's icon.

use eframe::egui::IconData;

/// A colour: red, green, blue and alpha, each from 0 to 1.
#[derive(Clone, Copy)]
struct Colour([f32; 4]);

/// The colour `#rrggbb` is, `alpha` opaque.
fn rgb(c: u32, alpha: f32) -> Colour {
    let at = |shift: u32| ((c >> shift) & 0xff) as f32 / 255.0;
    Colour([at(16), at(8), at(0), alpha])
}

/// A rounded rectangle on the square of 1024 units the icon is drawn on:
/// its left, top, right and bottom, and the radius of its corners.
#[derive(Clone, Copy)]
struct Rounded {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    radius: f32,
}

impl Rounded {
    /// A bar, `height` high, from `left` to `right` around `y`: its ends
    /// round as a line of text's.
    fn bar(left: f32, right: f32, y: f32, height: f32) -> Rounded {
        Rounded {
            left,
            top: y - height / 2.0,
            right,
            bottom: y + height / 2.0,
            radius: height * 0.3,
        }
    }

    /// How far (x, y) is outside it, in units: less than 0 inside. Its
    /// corners round no more than half its width or height allows.
    fn outside(&self, x: f32, y: f32) -> f32 {
        let (w, h) = ((self.right - self.left) / 2.0, (self.bottom - self.top) / 2.0);
        let r = self.radius.min(w).min(h);
        let qx = (x - self.left - w).abs() - (w - r);
        let qy = (y - self.top - h).abs() - (h - r);
        qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r
    }

    /// The same, `by` units lower.
    fn down(self, by: f32) -> Rounded {
        Rounded {
            top: self.top + by,
            bottom: self.bottom + by,
            ..self
        }
    }
}

/// One thing painted, over those painted before it.
struct Layer {
    shape: Rounded,
    /// Nothing of it below this line: a card's band across its top.
    above: f32,
    /// Its colour at its top and at its bottom, blended between.
    top: Colour,
    bottom: Colour,
    /// Its edge spread over this many units, as a shadow's is; else as
    /// sharp as the pixels allow.
    blur: f32,
}

impl Layer {
    fn new(shape: Rounded, colour: Colour) -> Layer {
        Layer {
            shape,
            above: f32::INFINITY,
            top: colour,
            bottom: colour,
            blur: 0.0,
        }
    }

    /// The shadow `shape` casts, a little lower, blurred.
    fn shadow(shape: Rounded, alpha: f32) -> Layer {
        Layer {
            blur: 22.0,
            ..Layer::new(shape.down(14.0), rgb(0x000000, alpha))
        }
    }
}

/// What is painted, from the back.
fn layers() -> Vec<Layer> {
    let body = Rounded {
        left: 100.0,
        top: 100.0,
        right: 924.0,
        bottom: 924.0,
        radius: 185.0,
    };
    let mut l = vec![
        Layer::shadow(body, 0.3),
        Layer {
            top: rgb(0x384057, 1.0),
            bottom: rgb(0x161a23, 1.0),
            ..Layer::new(body, rgb(0, 1.0))
        },
    ];
    // The patches below, narrower the further back, their titles showing.
    let back = [
        (282.0, 218.0, 742.0, 0x56627f, 0x737f9d, 600.0),
        (240.0, 292.0, 784.0, 0x8793b0, 0xa9b4cc, 650.0),
    ];
    for (left, top, right, card, title, end) in back {
        let shape = Rounded {
            left,
            top,
            right,
            bottom: top + 430.0,
            radius: 40.0,
        };
        l.push(Layer::shadow(shape, 0.35));
        l.push(Layer::new(shape, rgb(card, 1.0)));
        l.push(Layer::new(Rounded::bar(left + 42.0, end, top + 38.0, 24.0), rgb(title, 1.0)));
    }
    // The patch in front: its title across its top, then its diff.
    let front = Rounded {
        left: 196.0,
        top: 370.0,
        right: 828.0,
        bottom: 812.0,
        radius: 44.0,
    };
    l.push(Layer::shadow(front, 0.4));
    l.push(Layer::new(front, rgb(0xeef1f6, 1.0)));
    l.push(Layer {
        above: 446.0,
        ..Layer::new(front, rgb(0xd9dee8, 1.0))
    });
    l.push(Layer::new(Rounded::bar(240.0, 600.0, 408.0, 28.0), rgb(0x8c97ad, 1.0)));
    let (context, removed, added) = (0xc3cad7, 0xe5534b, 0x3fb950);
    for (right, y, colour) in [
        (600.0, 500.0, context),
        (690.0, 560.0, removed),
        (750.0, 620.0, added),
        (560.0, 680.0, added),
        (660.0, 740.0, context),
    ] {
        l.push(Layer::new(Rounded::bar(240.0, right, y, 34.0), rgb(colour, 1.0)));
    }
    // The cursor, at the end of the line added last.
    l.push(Layer::new(Rounded::bar(578.0, 596.0, 680.0, 76.0), rgb(0xf0a73a, 1.0)));
    l
}

/// A blurred edge's cover at `d` blurs outside it: 1 well inside, 0 well
/// outside, smooth between.
fn smooth(d: f32) -> f32 {
    let t = ((1.0 - d) / 2.0).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The icon, `size` pixels square: its rows from the top, each pixel's red,
/// green, blue and alpha (not premultiplied).
pub fn rgba(size: u32) -> Vec<u8> {
    let layers = layers();
    let unit = 1024.0 / size as f32;
    let mut out = Vec::with_capacity(size as usize * size as usize * 4);
    for j in 0..size {
        for i in 0..size {
            let (x, y) = ((i as f32 + 0.5) * unit, (j as f32 + 0.5) * unit);
            // Premultiplied: each layer over what is under it.
            let mut c = [0.0f32; 4];
            for layer in &layers {
                let d = layer.shape.outside(x, y).max(y - layer.above);
                let cover = match layer.blur > 0.0 {
                    true => smooth(d / layer.blur),
                    false => (0.5 - d / unit).clamp(0.0, 1.0),
                };
                if cover <= 0.0 {
                    continue;
                }
                let s = &layer.shape;
                let t = ((y - s.top) / (s.bottom - s.top)).clamp(0.0, 1.0);
                let colour: Vec<f32> = (0..4).map(|k| layer.top.0[k] + (layer.bottom.0[k] - layer.top.0[k]) * t).collect();
                let a = colour[3] * cover;
                for k in 0..3 {
                    c[k] = colour[k] * a + c[k] * (1.0 - a);
                }
                c[3] = a + c[3] * (1.0 - a);
            }
            let a = c[3];
            for v in &c[..3] {
                out.push(if a > 0.0 { (v / a * 255.0).round() as u8 } else { 0 });
            }
            out.push((a * 255.0).round() as u8);
        }
    }
    out
}

/// The icon, `size` pixels square, as a window takes it.
pub fn data(size: u32) -> IconData {
    IconData {
        rgba: rgba(size),
        width: size,
        height: size,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_is_the_stack_s_patches_on_a_rounded_square() {
        let icon = data(128);
        assert_eq!((icon.width, icon.height, icon.rgba.len()), (128, 128, 128 * 128 * 4));
        // The pixel at (x, y) on the square of 1024 units.
        let at = |x: u32, y: u32| {
            let i = ((y / 8 * 128 + x / 8) * 4) as usize;
            <[u8; 4]>::try_from(&icon.rgba[i..i + 4]).unwrap()
        };
        let is = |[r, g, b, a]: [u8; 4], what: &str| match what {
            "nothing" => a == 0,
            "dark" => a == 255 && r < 100 && g < 100 && b < 120,
            "light" => a == 255 && r > 220 && g > 220 && b > 220,
            "red" => a == 255 && r > 200 && g < 120 && b < 120,
            "green" => a == 255 && g > 150 && r < 120 && b < 120,
            "amber" => a == 255 && r > 200 && g > 140 && b < 100,
            _ => unreachable!(),
        };
        for (x, y, what) in [
            (4, 4, "nothing"),
            (1020, 1020, "nothing"),
            (512, 140, "dark"),
            (780, 780, "light"),
            (400, 560, "red"),
            (400, 620, "green"),
            (588, 684, "amber"),
        ] {
            assert!(is(at(x, y), what), "{x},{y}: {what}, not {:?}", at(x, y));
        }
        // At every size: the smallest, the largest.
        assert_eq!(rgba(16).len(), 16 * 16 * 4);
        assert!(rgba(1024).chunks(4).any(|p| p == [0xf0, 0xa7, 0x3a, 255]), "the cursor, sharp");
    }
}
