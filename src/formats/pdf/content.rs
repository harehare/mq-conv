//! Content-stream interpreter: turns a page into positioned glyphs, ruling
//! lines, image placements and link rectangles (all in a top-left, y-down
//! coordinate system measured in points).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use lopdf::content::Content;
use lopdf::{Dictionary, Document, Object, ObjectId};

use super::font::Font;
use super::util::{dict_get, num, resolve};

pub type FontCache = Mutex<HashMap<ObjectId, Arc<Font>>>;

#[derive(Clone, Copy)]
pub struct Matrix(pub [f32; 6]);

impl Matrix {
    pub const IDENTITY: Matrix = Matrix([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    /// `self` applied first, then `other`.
    pub fn then(&self, o: &Matrix) -> Matrix {
        let (a, b) = (&self.0, &o.0);
        Matrix([
            a[0] * b[0] + a[1] * b[2],
            a[0] * b[1] + a[1] * b[3],
            a[2] * b[0] + a[3] * b[2],
            a[2] * b[1] + a[3] * b[3],
            a[4] * b[0] + a[5] * b[2] + b[4],
            a[4] * b[1] + a[5] * b[3] + b[5],
        ])
    }

    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        let m = &self.0;
        (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
    }
}

pub const STYLE_BOLD: u8 = 1;
pub const STYLE_ITALIC: u8 = 2;
pub const STYLE_MONO: u8 = 4;

#[derive(Clone)]
pub struct Glyph {
    pub text: String,
    pub x0: f32,
    pub x1: f32,
    /// Baseline y.
    pub y: f32,
    pub size: f32,
    pub style: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct Rule {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

#[derive(Clone)]
pub struct ImagePlacement {
    pub id: ObjectId,
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

#[derive(Clone)]
pub struct LinkRect {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub url: String,
}

#[derive(Default)]
pub struct PageContent {
    pub height: f32,
    pub glyphs: Vec<Glyph>,
    pub rules: Vec<Rule>,
    pub images: Vec<ImagePlacement>,
    pub links: Vec<LinkRect>,
}

fn inherited<'a>(doc: &'a Document, page_id: ObjectId, key: &[u8]) -> Option<&'a Object> {
    let mut id = page_id;
    for _ in 0..32 {
        let dict = doc.get_object(id).ok()?.as_dict().ok()?;
        if let Ok(v) = dict.get(key) {
            return Some(resolve(doc, v));
        }
        id = dict.get(b"Parent").ok()?.as_reference().ok()?;
    }
    None
}

fn rect_of(doc: &Document, obj: &Object) -> Option<[f32; 4]> {
    let arr = resolve(doc, obj).as_array().ok()?;
    if arr.len() < 4 {
        return None;
    }
    let mut v = [0f32; 4];
    for (i, o) in arr.iter().take(4).enumerate() {
        v[i] = num(resolve(doc, o))?;
    }
    Some([
        v[0].min(v[2]),
        v[1].min(v[3]),
        v[0].max(v[2]),
        v[1].max(v[3]),
    ])
}

/// Matrix mapping default user space to the top-left device space.
fn base_matrix(doc: &Document, page_id: ObjectId) -> (Matrix, f32) {
    let [llx, lly, urx, ury] = inherited(doc, page_id, b"CropBox")
        .or_else(|| inherited(doc, page_id, b"MediaBox"))
        .and_then(|o| rect_of(doc, o))
        .unwrap_or([0.0, 0.0, 612.0, 792.0]);
    let rotate = inherited(doc, page_id, b"Rotate")
        .and_then(num)
        .map(|r| (r as i32).rem_euclid(360))
        .unwrap_or(0);
    let (w, h) = (urx - llx, ury - lly);
    match rotate {
        90 => (Matrix([0.0, 1.0, 1.0, 0.0, -lly, -llx]), w),
        180 => (Matrix([-1.0, 0.0, 0.0, 1.0, urx, -lly]), h),
        270 => (Matrix([0.0, -1.0, -1.0, 0.0, ury, urx]), w),
        _ => (Matrix([1.0, 0.0, 0.0, -1.0, -llx, ury]), h),
    }
}

pub fn extract_page(doc: &Document, page_id: ObjectId, fonts: &FontCache) -> PageContent {
    let (base, height) = base_matrix(doc, page_id);
    let mut page = PageContent {
        height,
        ..Default::default()
    };

    let resources = inherited(doc, page_id, b"Resources").and_then(|o| o.as_dict().ok());
    if let (data, Some(res)) = (doc.get_page_content(page_id), resources) {
        let mut interp = Interp {
            doc,
            fonts,
            page: &mut page,
            depth: 0,
        };
        interp.run(&data, res, base);
    }

    extract_links(doc, page_id, &base, &mut page);
    page
}

fn extract_links(doc: &Document, page_id: ObjectId, base: &Matrix, page: &mut PageContent) {
    let Ok(dict) = doc.get_object(page_id).and_then(|o| o.as_dict()) else {
        return;
    };
    let Some(annots) = dict_get(doc, dict, b"Annots").and_then(|o| o.as_array().ok()) else {
        return;
    };
    for a in annots {
        let Ok(ad) = resolve(doc, a).as_dict() else {
            continue;
        };
        if dict_get(doc, ad, b"Subtype").and_then(|o| o.as_name().ok()) != Some(b"Link") {
            continue;
        }
        let Some(rect) = ad.get(b"Rect").ok().and_then(|o| rect_of(doc, o)) else {
            continue;
        };
        let Some(action) = dict_get(doc, ad, b"A").and_then(|o| o.as_dict().ok()) else {
            continue;
        };
        let Some(Object::String(uri, _)) = dict_get(doc, action, b"URI") else {
            continue;
        };
        let (x0, y0) = base.apply(rect[0], rect[1]);
        let (x1, y1) = base.apply(rect[2], rect[3]);
        page.links.push(LinkRect {
            x0: x0.min(x1),
            y0: y0.min(y1),
            x1: x0.max(x1),
            y1: y0.max(y1),
            url: String::from_utf8_lossy(uri).into_owned(),
        });
    }
}

#[derive(Clone)]
struct TextState {
    font: Option<Arc<Font>>,
    size: f32,
    char_space: f32,
    word_space: f32,
    h_scale: f32,
    leading: f32,
    rise: f32,
}

impl Default for TextState {
    fn default() -> Self {
        TextState {
            font: None,
            size: 0.0,
            char_space: 0.0,
            word_space: 0.0,
            h_scale: 1.0,
            leading: 0.0,
            rise: 0.0,
        }
    }
}

enum PathItem {
    Rect([f32; 4]),
    Line((f32, f32), (f32, f32)),
}

struct Interp<'a> {
    doc: &'a Document,
    fonts: &'a FontCache,
    page: &'a mut PageContent,
    depth: u32,
}

fn f(o: &Object) -> f32 {
    num(o).unwrap_or(0.0)
}

impl Interp<'_> {
    fn run(&mut self, data: &[u8], resources: &Dictionary, ctm0: Matrix) {
        let Ok(content) = Content::decode(data) else {
            return;
        };
        let doc = self.doc;
        let mut ctm = ctm0;
        let mut stack: Vec<(Matrix, TextState)> = Vec::new();
        let mut ts = TextState::default();
        let mut tm = Matrix::IDENTITY;
        let mut tlm = Matrix::IDENTITY;
        let mut path: Vec<PathItem> = Vec::new();
        let mut cur = (0f32, 0f32);
        let mut start = (0f32, 0f32);

        for op in &content.operations {
            let a = &op.operands;
            match op.operator.as_str() {
                "q" => stack.push((ctm, ts.clone())),
                "Q" => {
                    if let Some((c, t)) = stack.pop() {
                        ctm = c;
                        ts = t;
                    }
                }
                "cm" if a.len() >= 6 => {
                    let m = Matrix([f(&a[0]), f(&a[1]), f(&a[2]), f(&a[3]), f(&a[4]), f(&a[5])]);
                    ctm = m.then(&ctm);
                }
                "BT" => {
                    tm = Matrix::IDENTITY;
                    tlm = Matrix::IDENTITY;
                }
                "Tc" if !a.is_empty() => ts.char_space = f(&a[0]),
                "Tw" if !a.is_empty() => ts.word_space = f(&a[0]),
                "Tz" if !a.is_empty() => ts.h_scale = f(&a[0]) / 100.0,
                "TL" if !a.is_empty() => ts.leading = f(&a[0]),
                "Ts" if !a.is_empty() => ts.rise = f(&a[0]),
                "Tf" if a.len() >= 2 => {
                    ts.size = f(&a[1]);
                    ts.font = a[0]
                        .as_name()
                        .ok()
                        .and_then(|n| self.load_font(resources, n));
                }
                "Td" if a.len() >= 2 => {
                    tlm = Matrix([1.0, 0.0, 0.0, 1.0, f(&a[0]), f(&a[1])]).then(&tlm);
                    tm = tlm;
                }
                "TD" if a.len() >= 2 => {
                    ts.leading = -f(&a[1]);
                    tlm = Matrix([1.0, 0.0, 0.0, 1.0, f(&a[0]), f(&a[1])]).then(&tlm);
                    tm = tlm;
                }
                "Tm" if a.len() >= 6 => {
                    tlm = Matrix([f(&a[0]), f(&a[1]), f(&a[2]), f(&a[3]), f(&a[4]), f(&a[5])]);
                    tm = tlm;
                }
                "T*" => {
                    tlm = Matrix([1.0, 0.0, 0.0, 1.0, 0.0, -ts.leading]).then(&tlm);
                    tm = tlm;
                }
                "Tj" if !a.is_empty() => self.show(&a[0], &ts, &mut tm, &ctm),
                "'" if !a.is_empty() => {
                    tlm = Matrix([1.0, 0.0, 0.0, 1.0, 0.0, -ts.leading]).then(&tlm);
                    tm = tlm;
                    self.show(&a[0], &ts, &mut tm, &ctm);
                }
                "\"" if a.len() >= 3 => {
                    ts.word_space = f(&a[0]);
                    ts.char_space = f(&a[1]);
                    tlm = Matrix([1.0, 0.0, 0.0, 1.0, 0.0, -ts.leading]).then(&tlm);
                    tm = tlm;
                    self.show(&a[2], &ts, &mut tm, &ctm);
                }
                "TJ" if !a.is_empty() => {
                    if let Ok(items) = a[0].as_array() {
                        for it in items {
                            match it {
                                Object::String(..) => self.show(it, &ts, &mut tm, &ctm),
                                other => {
                                    if let Some(n) = num(other) {
                                        let tx = -n / 1000.0 * ts.size * ts.h_scale;
                                        tm = Matrix([1.0, 0.0, 0.0, 1.0, tx, 0.0]).then(&tm);
                                    }
                                }
                            }
                        }
                    }
                }
                "m" if a.len() >= 2 => {
                    cur = (f(&a[0]), f(&a[1]));
                    start = cur;
                }
                "l" if a.len() >= 2 => {
                    let p = (f(&a[0]), f(&a[1]));
                    path.push(PathItem::Line(cur, p));
                    cur = p;
                }
                "c" if a.len() >= 6 => {
                    let p = (f(&a[4]), f(&a[5]));
                    path.push(PathItem::Line(cur, p));
                    cur = p;
                }
                "v" | "y" if a.len() >= 4 => {
                    let p = (f(&a[2]), f(&a[3]));
                    path.push(PathItem::Line(cur, p));
                    cur = p;
                }
                "h" => {
                    path.push(PathItem::Line(cur, start));
                    cur = start;
                }
                "re" if a.len() >= 4 => {
                    let (x, y, w, h) = (f(&a[0]), f(&a[1]), f(&a[2]), f(&a[3]));
                    path.push(PathItem::Rect([x, y, x + w, y + h]));
                }
                "S" | "s" => self.paint(&mut path, &ctm, true, false),
                "f" | "F" | "f*" => self.paint(&mut path, &ctm, false, true),
                "B" | "B*" | "b" | "b*" => self.paint(&mut path, &ctm, true, true),
                "n" => path.clear(),
                "Do" if !a.is_empty() => {
                    if let Ok(name) = a[0].as_name() {
                        self.do_xobject(resources, name, &ctm);
                    }
                }
                _ => {}
            }
        }
        let _ = doc;
    }

    fn load_font(&self, resources: &Dictionary, name: &[u8]) -> Option<Arc<Font>> {
        let fonts = dict_get(self.doc, resources, b"Font")?.as_dict().ok()?;
        let entry = fonts.get(name).ok()?;
        if let Object::Reference(id) = entry {
            if let Some(f) = self.fonts.lock().ok()?.get(id) {
                return Some(f.clone());
            }
            let dict = resolve(self.doc, entry).as_dict().ok()?;
            let font = Arc::new(Font::load(self.doc, dict));
            self.fonts.lock().ok()?.insert(*id, font.clone());
            Some(font)
        } else {
            Some(Arc::new(Font::load(self.doc, entry.as_dict().ok()?)))
        }
    }

    fn show(&mut self, s: &Object, ts: &TextState, tm: &mut Matrix, ctm: &Matrix) {
        let (Object::String(bytes, _), Some(font)) = (s, &ts.font) else {
            return;
        };
        let mut style = 0;
        if font.bold {
            style |= STYLE_BOLD;
        }
        if font.italic {
            style |= STYLE_ITALIC;
        }
        if font.mono {
            style |= STYLE_MONO;
        }
        for g in font.decode(bytes) {
            let trm = Matrix([ts.size * ts.h_scale, 0.0, 0.0, ts.size, 0.0, ts.rise])
                .then(tm)
                .then(ctm);
            let w0 = g.width / 1000.0;
            let size = trm.0[2].hypot(trm.0[3]);
            if !g.text.is_empty() && size.is_finite() && size >= 1.0 {
                let x0 = trm.0[4];
                let x1 = x0 + w0 * trm.0[0];
                self.page.glyphs.push(Glyph {
                    text: g.text,
                    x0: x0.min(x1),
                    x1: x0.max(x1),
                    y: trm.0[5],
                    size,
                    style,
                });
            }
            let tx = (w0 * ts.size + ts.char_space + if g.is_space { ts.word_space } else { 0.0 })
                * ts.h_scale;
            *tm = Matrix([1.0, 0.0, 0.0, 1.0, tx, 0.0]).then(tm);
        }
    }

    fn paint(&mut self, path: &mut Vec<PathItem>, ctm: &Matrix, stroke: bool, fill: bool) {
        const THIN: f32 = 3.0;
        for item in path.drain(..) {
            match item {
                PathItem::Rect([x0, y0, x1, y1]) => {
                    let (ax, ay) = ctm.apply(x0, y0);
                    let (bx, by) = ctm.apply(x1, y1);
                    let (l, r) = (ax.min(bx), ax.max(bx));
                    let (t, b) = (ay.min(by), ay.max(by));
                    let (w, h) = (r - l, b - t);
                    if fill && (w <= THIN || h <= THIN) {
                        if h <= w {
                            let y = (t + b) / 2.0;
                            self.push_rule(l, y, r, y);
                        } else {
                            let x = (l + r) / 2.0;
                            self.push_rule(x, t, x, b);
                        }
                    } else if stroke {
                        if w <= THIN || h <= THIN {
                            if h <= w {
                                let y = (t + b) / 2.0;
                                self.push_rule(l, y, r, y);
                            } else {
                                let x = (l + r) / 2.0;
                                self.push_rule(x, t, x, b);
                            }
                        } else {
                            self.push_rule(l, t, r, t);
                            self.push_rule(l, b, r, b);
                            self.push_rule(l, t, l, b);
                            self.push_rule(r, t, r, b);
                        }
                    }
                }
                PathItem::Line(p, q) => {
                    if stroke {
                        let (ax, ay) = ctm.apply(p.0, p.1);
                        let (bx, by) = ctm.apply(q.0, q.1);
                        if (ay - by).abs() < 0.5 && (ax - bx).abs() > 1.0 {
                            self.push_rule(ax.min(bx), ay, ax.max(bx), ay);
                        } else if (ax - bx).abs() < 0.5 && (ay - by).abs() > 1.0 {
                            self.push_rule(ax, ay.min(by), ax, ay.max(by));
                        }
                    }
                }
            }
        }
    }

    fn push_rule(&mut self, x0: f32, y0: f32, x1: f32, y1: f32) {
        if self.page.rules.len() < 20_000 {
            self.page.rules.push(Rule { x0, y0, x1, y1 });
        }
    }

    fn do_xobject(&mut self, resources: &Dictionary, name: &[u8], ctm: &Matrix) {
        let doc = self.doc;
        let Some(xobjs) = dict_get(doc, resources, b"XObject").and_then(|o| o.as_dict().ok())
        else {
            return;
        };
        let Ok(entry) = xobjs.get(name) else { return };
        let Ok(stream) = resolve(doc, entry).as_stream() else {
            return;
        };
        match dict_get(doc, &stream.dict, b"Subtype").and_then(|o| o.as_name().ok()) {
            Some(b"Image") => {
                if let Object::Reference(id) = entry {
                    let (x0, y0) = ctm.apply(0.0, 0.0);
                    let (x1, y1) = ctm.apply(1.0, 1.0);
                    let (l, r) = (x0.min(x1), x0.max(x1));
                    let (t, b) = (y0.min(y1), y0.max(y1));
                    if r - l >= 8.0 && b - t >= 8.0 {
                        self.page.images.push(ImagePlacement {
                            id: *id,
                            x0: l,
                            y0: t,
                            x1: r,
                            y1: b,
                        });
                    }
                }
            }
            Some(b"Form") if self.depth < 8 => {
                let Ok(data) = stream.decompressed_content() else {
                    return;
                };
                let matrix = dict_get(doc, &stream.dict, b"Matrix")
                    .and_then(|o| o.as_array().ok())
                    .filter(|m| m.len() >= 6)
                    .map(|m| {
                        let v: Vec<f32> = m.iter().map(|o| f(resolve(doc, o))).collect();
                        Matrix([v[0], v[1], v[2], v[3], v[4], v[5]])
                    })
                    .unwrap_or(Matrix::IDENTITY);
                let form_res = dict_get(doc, &stream.dict, b"Resources")
                    .and_then(|o| o.as_dict().ok())
                    .unwrap_or(resources);
                self.depth += 1;
                self.run(&data, form_res, matrix.then(ctm));
                self.depth -= 1;
            }
            _ => {}
        }
    }
}
