//! StackED's icon, drawn rather than kept as a picture: an S of white sheets
//! of paper, stacked along the letter's path, each a little further along
//! than the one under it so that their edges show, fanned where the S turns;
//! on a black rounded square, as macOS draws an app's. Drawn at any size: for
//! the window and the Dock while StackED runs, and for StackED.app's icon;
//! and for the tab of `stacked web`'s page, as a PNG this module writes
//! itself (`png`): `web` is built without `app` too, with no eframe to
//! write one. Small, the sheets' edges are left out, and the S is plain
//! white. Built with either feature; what takes eframe's `IconData`
//! (`data`), with `app` alone.

#[cfg(feature = "app")]
use eframe::egui::IconData;

/// A colour: red, green, blue and alpha, each from 0 to 1.
#[derive(Clone, Copy)]
struct Colour([f32; 4]);

/// The grey `v`, from 0 (black) to 1 (white), `alpha` opaque.
fn grey(v: f32, alpha: f32) -> Colour {
    Colour([v, v, v, alpha])
}

/// A rounded rectangle on the square of 1024 units the icon is drawn on:
/// centred at (x, y), reaching `along` each way along (dx, dy), which is a
/// unit long, and `across` each way across it; its corners round by
/// `radius`.
#[derive(Clone, Copy)]
struct Rounded {
    x: f32,
    y: f32,
    dx: f32,
    dy: f32,
    along: f32,
    across: f32,
    radius: f32,
}

impl Rounded {
    /// One square to the icon's sides: its left, top, right and bottom.
    fn square(left: f32, top: f32, right: f32, bottom: f32, radius: f32) -> Rounded {
        Rounded {
            x: (left + right) / 2.0,
            y: (top + bottom) / 2.0,
            dx: 1.0,
            dy: 0.0,
            along: (right - left) / 2.0,
            across: (bottom - top) / 2.0,
            radius,
        }
    }

    /// How far (x, y) is outside it, in units: less than 0 inside. Its
    /// corners round no more than half its width or height allows.
    fn outside(&self, x: f32, y: f32) -> f32 {
        let (px, py) = (x - self.x, y - self.y);
        let (u, v) = (px * self.dx + py * self.dy, py * self.dx - px * self.dy);
        let r = self.radius.min(self.along).min(self.across);
        let qx = u.abs() - (self.along - r);
        let qy = v.abs() - (self.across - r);
        qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r
    }

    /// The box it fits in: its left, top, right and bottom.
    fn bounds(&self) -> [f32; 4] {
        let w = self.dx.abs() * self.along + self.dy.abs() * self.across;
        let h = self.dy.abs() * self.along + self.dx.abs() * self.across;
        [self.x - w, self.y - h, self.x + w, self.y + h]
    }

    /// The same, `by` units lower.
    fn down(self, by: f32) -> Rounded {
        Rounded { y: self.y + by, ..self }
    }

    /// The same, `by` units in from each side.
    fn inset(self, by: f32) -> Rounded {
        Rounded {
            along: self.along - by,
            across: self.across - by,
            radius: (self.radius - by).max(0.0),
            ..self
        }
    }
}

/// What a layer is to the sheets' shadows.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    /// Painted wherever its shape is.
    Plain,
    /// Part of a sheet: the shadows of the sheets above it fall on it.
    Sheet,
    /// A sheet's shadow: painted only over the sheets under it.
    Shadow,
}

/// One thing painted, over those painted before it.
struct Layer {
    shape: Rounded,
    kind: Kind,
    /// Its colour at the height `from` and at the height `to`, blended
    /// between.
    top: Colour,
    bottom: Colour,
    from: f32,
    to: f32,
    /// Its edge spread over this many units, as a shadow's is; else as
    /// sharp as the pixels allow.
    blur: f32,
}

impl Layer {
    /// `shape`, its colour `top` at its top and `bottom` at its bottom.
    fn new(kind: Kind, shape: Rounded, top: Colour, bottom: Colour) -> Layer {
        let [_, from, _, to] = shape.bounds();
        Layer {
            shape,
            kind,
            top,
            bottom,
            from,
            to,
            blur: 0.0,
        }
    }
}

/// The upper half of the polygon the S's path follows, from the S's end at
/// the top right: along the top, down the left side, and in toward the
/// middle. The lower half is the same, turned half a turn about the centre.
const UPPER: [(f32, f32); 7] = [
    (676.0, 345.0),
    (640.0, 310.0),
    (575.0, 290.0),
    (420.0, 290.0),
    (350.0, 345.0),
    (352.0, 420.0),
    (420.0, 475.0),
];

/// How far each sheet reaches from its middle along the S's path.
const ALONG: f32 = 40.0;

/// How far each sheet reaches from its middle across the S's path: half the
/// S's stroke.
const ACROSS: f32 = 57.0;

/// How far along the path a sheet lies past the one under it, at most.
const GAP: f32 = 22.0;

/// How far a sheet is turned from the one under it, at most: where the path
/// turns faster, the sheets lie closer together, fanned.
const TURN: f32 = 8.0 * std::f32::consts::PI / 180.0;

/// The S's path, along the middle of its stroke, from its end at the top
/// right to its end at the bottom left: points close enough together to be
/// taken for a curve. The curve is a B-spline over the polygon `UPPER` and
/// its lower half: pulled toward each corner in turn, and smooth all along,
/// so that the sheets along it turn smoothly too. It starts and ends where
/// the polygon does.
fn path() -> Vec<(f32, f32)> {
    let mut polygon = UPPER.to_vec();
    polygon.extend(UPPER.iter().rev().map(|&(x, y)| (1024.0 - x, 1024.0 - y)));
    // Each end three times, for the curve to reach it.
    let (first, last) = (polygon[0], polygon[polygon.len() - 1]);
    let corners: Vec<(f32, f32)> = [first, first].into_iter().chain(polygon).chain([last, last]).collect();
    let mut path = Vec::new();
    for p in corners.windows(4) {
        // A piece of the curve, near the middle two of four corners: each
        // point of it their weighted mean, the weights summing to 6.
        for k in 0..50 {
            let t = k as f32 / 50.0;
            let u = 1.0 - t;
            let w = [
                u * u * u,
                3.0 * t * t * t - 6.0 * t * t + 4.0,
                -3.0 * t * t * t + 3.0 * t * t + 3.0 * t + 1.0,
                t * t * t,
            ];
            let (x, y) = (0..4).fold((0.0, 0.0), |(x, y), q| (x + w[q] * p[q].0, y + w[q] * p[q].1));
            path.push((x / 6.0, y / 6.0));
        }
    }
    path.push(last);
    path
}

/// Where `path` is `s` units along it, `lengths` saying how far along each
/// of its points is: the point, and the way the path goes there, a unit
/// long.
fn at(path: &[(f32, f32)], lengths: &[f32], s: f32) -> (f32, f32, f32, f32) {
    let i = lengths.partition_point(|&l| l < s).clamp(1, path.len() - 1);
    let ((x0, y0), (x1, y1)) = (path[i - 1], path[i]);
    let t = ((s - lengths[i - 1]) / (lengths[i] - lengths[i - 1]).max(1e-6)).clamp(0.0, 1.0);
    let (dx, dy) = (x1 - x0, y1 - y0);
    let d = dx.hypot(dy).max(1e-6);
    (x0 + dx * t, y0 + dy * t, dx / d, dy / d)
}

/// How far along `path` each sheet's middle is: `GAP` past the one before,
/// or less where the path turns more than `TURN` over that; the first and
/// the last sheet reaching the path's ends.
fn places(path: &[(f32, f32)], lengths: &[f32]) -> Vec<f32> {
    let (start, end) = (ALONG, lengths[lengths.len() - 1] - ALONG);
    let (mut s, mut places) = (start, vec![start]);
    while s < end {
        let (_, _, ax, ay) = at(path, lengths, s);
        let mut step = GAP;
        loop {
            let (_, _, bx, by) = at(path, lengths, s + step);
            if (ax * by - ay * bx).atan2(ax * bx + ay * by).abs() <= TURN || step < 1.0 {
                break;
            }
            step *= 0.9;
        }
        s += step;
        places.push(s);
    }
    // The last one is past the end: all of them a little closer together,
    // for it to be at the end.
    places.iter().map(|p| start + (p - start) * (end - start) / (s - start)).collect()
}

/// What is painted, from the back, for the icon drawn `unit` units to a
/// pixel.
fn layers(unit: f32) -> Vec<Layer> {
    let body = Rounded::square(100.0, 100.0, 924.0, 924.0, 185.0);
    let shade = grey(0.0, 0.3);
    let mut l = vec![
        Layer {
            blur: 22.0,
            ..Layer::new(Kind::Plain, body.down(14.0), shade, shade)
        },
        Layer::new(Kind::Plain, body, grey(0.17, 1.0), grey(0.05, 1.0)),
    ];
    let path = path();
    let mut lengths = vec![0.0];
    for w in path.windows(2) {
        lengths.push(lengths[lengths.len() - 1] + (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1));
    }
    // The sheets' white greys a little from the S's top to its bottom.
    let (top, bottom) = path.iter().fold((f32::MAX, f32::MIN), |(t, b), &(_, y)| (t.min(y), b.max(y)));
    let (from, to) = (top - ACROSS, bottom + ACROSS);
    // How much the sheets' edges and shadows show: fully, except where a
    // sheet's strip is narrower than two pixels or so, where they would
    // only grey the S, unevenly.
    let detail = ((GAP / unit - 1.25) / 1.25).clamp(0.0, 1.0);
    let shadow = grey(0.0, 0.45 * detail);
    let edge = 1.0 - 0.28 * detail;
    // The stack's bottom sheet is at the S's end at the bottom left, its top
    // sheet at the end at the top right.
    for s in places(&path, &lengths).into_iter().rev() {
        let (x, y, dx, dy) = at(&path, &lengths, s);
        let sheet = Rounded {
            x,
            y,
            dx,
            dy,
            along: ALONG,
            across: ACROSS,
            radius: 8.0,
        };
        // Its shadow on the sheets under it, its edge, then its face.
        if detail > 0.0 {
            l.push(Layer {
                blur: 16.0,
                ..Layer::new(Kind::Shadow, sheet, shadow, shadow)
            });
        }
        l.push(Layer {
            from,
            to,
            ..Layer::new(Kind::Sheet, sheet, grey(edge, 1.0), grey(0.9 * edge, 1.0))
        });
        l.push(Layer {
            from,
            to,
            ..Layer::new(Kind::Sheet, sheet.inset(2.0), grey(1.0, 1.0), grey(0.9, 1.0))
        });
    }
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
    let (n, unit) = (size as usize, 1024.0 / size as f32);
    // Premultiplied: each layer over what is under it.
    let mut c = vec![[0.0f32; 4]; n * n];
    // How much of each pixel the sheets cover, for their shadows to fall on.
    let mut sheets = vec![0.0f32; n * n];
    for layer in layers(unit) {
        // Its pixels: those of the box it fits in, its blur and a pixel more.
        let reach = layer.blur + unit;
        let pixels = |from: f32, to: f32| {
            ((from - reach) / unit).floor().max(0.0) as usize..(((to + reach) / unit).ceil().max(0.0) as usize).min(n)
        };
        let [left, top, right, bottom] = layer.shape.bounds();
        for j in pixels(top, bottom) {
            let y = (j as f32 + 0.5) * unit;
            let t = ((y - layer.from) / (layer.to - layer.from)).clamp(0.0, 1.0);
            let colour: [f32; 4] = std::array::from_fn(|k| layer.top.0[k] + (layer.bottom.0[k] - layer.top.0[k]) * t);
            for i in pixels(left, right) {
                let x = (i as f32 + 0.5) * unit;
                let d = layer.shape.outside(x, y);
                let cover = match layer.blur > 0.0 {
                    true => smooth(d / layer.blur),
                    false => (0.5 - d / unit).clamp(0.0, 1.0),
                };
                if cover <= 0.0 {
                    continue;
                }
                let p = j * n + i;
                let a = colour[3] * cover * if layer.kind == Kind::Shadow { sheets[p] } else { 1.0 };
                if layer.kind == Kind::Sheet {
                    sheets[p] = cover + sheets[p] * (1.0 - cover);
                }
                for k in 0..3 {
                    c[p][k] = colour[k] * a + c[p][k] * (1.0 - a);
                }
                c[p][3] = a + c[p][3] * (1.0 - a);
            }
        }
    }
    let mut out = Vec::with_capacity(n * n * 4);
    for [r, g, b, a] in c {
        for v in [r, g, b] {
            out.push(if a > 0.0 { (v / a * 255.0).round() as u8 } else { 0 });
        }
        out.push((a * 255.0).round() as u8);
    }
    out
}

/// The icon, `size` pixels square, as a window takes it.
#[cfg(feature = "app")]
pub fn data(size: u32) -> IconData {
    IconData {
        rgba: rgba(size),
        width: size,
        height: size,
    }
}

/// The icon, `size` pixels square, as a PNG file: its signature, then its
/// chunks, IHDR (its size, and its kind: 8 bits for each of red, green,
/// blue and alpha, not premultiplied), IDAT (its pixels) and IEND. The
/// pixels are `rgba`'s, row by row from the top, each row after the byte
/// that names its filter, 0 for none; all in a zlib stream of deflate's
/// stored blocks, which compress nothing, and which every reader reads.
pub fn png(size: u32) -> Vec<u8> {
    let rgba = rgba(size);
    let mut rows = Vec::with_capacity(rgba.len() + size as usize);
    for row in rgba.chunks(size as usize * 4) {
        rows.push(0);
        rows.extend_from_slice(row);
    }
    // zlib's header: deflate, a window of 32K, no dictionary. Then the
    // blocks, of 65535 bytes at most, each after a byte saying that it is
    // stored and whether it is the last, then its length and the length's
    // complement. Then the Adler-32 of what they hold.
    let mut stream = vec![0x78, 0x01];
    let mut rest = &rows[..];
    loop {
        let (block, after) = rest.split_at(rest.len().min(0xffff));
        let len = block.len() as u16;
        stream.push(u8::from(after.is_empty()));
        stream.extend_from_slice(&len.to_le_bytes());
        stream.extend_from_slice(&(!len).to_le_bytes());
        stream.extend_from_slice(block);
        rest = after;
        if rest.is_empty() {
            break;
        }
    }
    stream.extend_from_slice(&adler32(&rows).to_be_bytes());
    // Its width and height; 8 bits a channel; red, green, blue and alpha
    // (6); deflate, PNG's filters, no interlacing.
    let mut header = [size.to_be_bytes(), size.to_be_bytes()].concat();
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(&mut png, b"IHDR", &header);
    chunk(&mut png, b"IDAT", &stream);
    chunk(&mut png, b"IEND", &[]);
    png
}

/// A chunk of a PNG file put after those of `png`: the length of `data`,
/// `kind`, `data`, then the CRC-32 of `kind` and `data`.
fn chunk(png: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    png.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let from = png.len();
    png.extend_from_slice(kind);
    png.extend_from_slice(data);
    let crc = crc32(&png[from..]);
    png.extend_from_slice(&crc.to_be_bytes());
}

/// The CRC-32 of `bytes`, as PNG checks each chunk (and zip each file): of
/// the polynomial 0xedb88320, each byte taken from its lowest bit, started
/// at all ones, its bits flipped at the end.
fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 { (crc >> 1) ^ 0xedb8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// The Adler-32 of `bytes`, as a zlib stream checks what it holds: in its
/// low half, 1 and the sum of the bytes; in its high half, the sum of what
/// the low half was after each byte; both modulo 65521.
fn adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b) = (1u32, 0u32);
    for &x in bytes {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_is_an_s_of_stacked_sheets_on_a_rounded_square() {
        // At the largest size and at the smallest.
        for size in [1024, 16] {
            let icon = rgba(size);
            assert_eq!(icon.len(), (size * size * 4) as usize);
            // Grey all over: black, white and the greys between, no colour.
            assert!(icon.chunks(4).all(|p| p[0] == p[1] && p[1] == p[2]), "{size}: a colour");
            // The pixel at (x, y) on the square of 1024 units.
            let at = |x: u32, y: u32| {
                let i = ((y * size / 1024 * size + x * size / 1024) * 4) as usize;
                <[u8; 4]>::try_from(&icon[i..i + 4]).unwrap()
            };
            let is = |[v, _, _, a]: [u8; 4], what: &str| match what {
                "nothing" => a == 0,
                "black" => a == 255 && v < 50,
                "light" => a == 255 && v > 150,
                "white" => a == 255 && v > 240,
                _ => unreachable!(),
            };
            for (x, y, what) in [
                // Past the rounded square's corners.
                (4, 4, "nothing"),
                (1020, 1020, "nothing"),
                // The square: above the S, left of it, and in its two
                // counters.
                (512, 140, "black"),
                (150, 512, "black"),
                (530, 400, "black"),
                (494, 624, "black"),
                // The S's sheets: along its top, down its left side, in its
                // middle, down its right side, and along its bottom.
                (512, 290, "light"),
                (352, 400, "light"),
                (512, 512, "light"),
                (672, 624, "light"),
                (512, 734, "light"),
                // The sheet on top, at the S's end at the top right: no
                // sheet is above it to shade it.
                (650, 318, "white"),
            ] {
                assert!(is(at(x, y), what), "{size}: {x},{y}: {what}, not {:?}", at(x, y));
            }
            // Along the S's middle stroke: large, the sheets' edges, one
            // after the other, each sheet's shadow on the one under it;
            // small, they are left out, and the S is white.
            let middle: Vec<u8> = (0..=60).map(|k| at(440 + 150 * k / 60, 472 + 80 * k / 60)[0]).collect();
            match size {
                1024 => assert!(middle.windows(2).filter(|w| w[0].saturating_sub(w[1]) > 30).count() >= 6, "{middle:?}"),
                _ => assert!(middle.iter().all(|&v| v > 235), "{middle:?}"),
            }
        }
    }

    /// A big-endian number of four bytes, at the start of `bytes`.
    fn be(bytes: &[u8]) -> u32 {
        u32::from_be_bytes(bytes[..4].try_into().unwrap())
    }

    #[test]
    fn the_icon_s_png_reads_back_as_its_pixels() {
        // Each check as its definition gives it: CRC-32's check value, of
        // "123456789", and the Adler-32 of "Wikipedia" that Wikipedia works
        // out.
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(adler32(b"Wikipedia"), 0x11e6_0398);
        // The page's two sizes, and one whose pixels take three blocks.
        for size in [32, 64, 200] {
            let png = png(size);
            assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "{size}");
            // Its chunks, IHDR, IDAT and IEND, nothing after, each its CRC
            // of its kind and its data.
            let (mut chunks, mut rest) = (vec![], &png[8..]);
            while !rest.is_empty() {
                let len = be(rest) as usize;
                let (kind, data) = (&rest[4..8], &rest[8..8 + len]);
                assert_eq!(be(&rest[8 + len..]), crc32(&rest[4..8 + len]), "{size}: {}", String::from_utf8_lossy(kind));
                chunks.push((kind, data));
                rest = &rest[12 + len..];
            }
            let [(b"IHDR", header), (b"IDAT", stream), (b"IEND", [])] = chunks[..] else {
                panic!("{size}: {:?}", chunks.iter().map(|(k, _)| String::from_utf8_lossy(k)).collect::<Vec<_>>());
            };
            // Its size, and its kind: 8 bits for each of red, green, blue
            // and alpha; deflate, PNG's filters, no interlacing.
            assert_eq!((be(header), be(&header[4..]), &header[8..]), (size, size, &[8, 6, 0, 0, 0][..]));
            // The zlib stream: deflate, with no dictionary, its header's
            // check right; its blocks, each stored, as long as it says, the
            // last marked so; then the Adler-32 of what they hold, nothing
            // after.
            assert_eq!((stream[0] & 0x0f, stream[1] & 0x20, u16::from_be_bytes([stream[0], stream[1]]) % 31), (8, 0, 0), "{size}");
            let (mut rows, mut at, mut blocks) = (vec![], 2, 0);
            loop {
                let (head, len) = (stream[at], u16::from_le_bytes([stream[at + 1], stream[at + 2]]));
                assert_eq!((head >> 1, u16::from_le_bytes([stream[at + 3], stream[at + 4]])), (0, !len), "{size}: block {blocks}");
                rows.extend_from_slice(&stream[at + 5..at + 5 + len as usize]);
                (at, blocks) = (at + 5 + len as usize, blocks + 1);
                if head & 1 == 1 {
                    break;
                }
            }
            assert_eq!(stream[at..], adler32(&rows).to_be_bytes(), "{size}");
            // As few blocks as hold the rows: one at the page's sizes, three
            // at the largest.
            assert_eq!(blocks, rows.len().div_ceil(0xffff), "{size}");
            // Its rows, from the top, each after its filter's byte, 0 for
            // none: the pixels `rgba` gives.
            let row = 1 + 4 * size as usize;
            assert_eq!(rows.len(), row * size as usize, "{size}");
            assert!(rows.chunks(row).all(|r| r[0] == 0), "{size}: a row filtered");
            let pixels: Vec<u8> = rows.chunks(row).flat_map(|r| &r[1..]).copied().collect();
            assert!(pixels == rgba(size), "{size}: other pixels than the icon's");
            // And the reader eframe has, another than this test's, reads the
            // same: the icon as StackED's window takes it.
            #[cfg(feature = "app")]
            assert!(eframe::icon_data::from_png_bytes(&png).unwrap() == data(size), "{size}: eframe reads another icon");
        }
    }
}
