//! Glyphs → words → lines, plus inline Markdown rendering of word runs.

use super::content::{Glyph, LinkRect, STYLE_BOLD, STYLE_ITALIC, STYLE_MONO};

#[derive(Clone, Debug)]
pub struct Word {
    pub text: String,
    pub x0: f32,
    pub x1: f32,
    pub y: f32,
    pub size: f32,
    pub style: u8,
    pub link: Option<usize>,
}

#[derive(Clone)]
pub struct Line {
    pub words: Vec<Word>,
    /// Indices of `words` in the slice the line was built from.
    pub ids: Vec<usize>,
    pub x0: f32,
    pub x1: f32,
    pub y: f32,
    /// Font size of the dominant (longest) word.
    pub size: f32,
}

pub fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x2E80..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF | 0x20000..=0x2FFFF)
}

/// Visual width in "characters" (CJK counts double).
pub fn text_width(s: &str) -> usize {
    s.chars().map(|c| if is_cjk(c) { 2 } else { 1 }).sum()
}

/// Group items into baseline-aligned clusters, each sorted left to right.
pub fn cluster_lines(
    mut order: Vec<usize>,
    y: impl Fn(usize) -> f32,
    size: impl Fn(usize) -> f32,
    x: impl Fn(usize) -> f32,
) -> Vec<Vec<usize>> {
    order.sort_by(|&a, &b| y(a).total_cmp(&y(b)));
    let mut lines: Vec<Vec<usize>> = Vec::new();
    let mut refs: Vec<(f32, f32)> = Vec::new();
    for i in order {
        if let (Some(line), Some(r)) = (lines.last_mut(), refs.last_mut())
            && (y(i) - r.0).abs() <= 0.3 * r.1.max(size(i))
        {
            line.push(i);
            r.1 = r.1.max(size(i));
            continue;
        }
        lines.push(vec![i]);
        refs.push((y(i), size(i)));
    }
    for line in &mut lines {
        line.sort_by(|&a, &b| x(a).total_cmp(&x(b)));
    }
    lines
}

pub fn build_words(glyphs: &[Glyph], links: &[LinkRect]) -> Vec<Word> {
    let order: Vec<usize> = (0..glyphs.len()).collect();
    let lines = cluster_lines(order, |i| glyphs[i].y, |i| glyphs[i].size, |i| glyphs[i].x0);

    #[derive(Default)]
    struct Acc {
        votes: [u32; 3],
        glyphs: u32,
        sizes: Vec<(f32, u32)>,
    }
    const FLAGS: [u8; 3] = [STYLE_BOLD, STYLE_ITALIC, STYLE_MONO];

    let mut words = Vec::new();
    for line in lines {
        let mut cur: Option<Word> = None;
        let mut last: Option<&Glyph> = None;
        let mut acc = Acc::default();
        let mut flush = |cur: &mut Option<Word>, acc: &mut Acc| {
            if let Some(mut w) = cur.take() {
                w.style = 0;
                for (k, flag) in FLAGS.iter().enumerate() {
                    if acc.glyphs > 0 && acc.votes[k] * 2 > acc.glyphs {
                        w.style |= flag;
                    }
                }
                // Dominant size: the most common one, so a single oversized
                // glyph (drop cap, symbol) does not turn a line into a heading.
                if let Some(&(size, _)) = acc.sizes.iter().max_by_key(|(_, n)| *n) {
                    w.size = size;
                }
                *acc = Acc::default();
                words.push(w);
            }
        };
        for &gi in &line {
            let g = &glyphs[gi];
            if g.text.trim().is_empty() {
                flush(&mut cur, &mut acc);
                last = None;
                continue;
            }
            if let (Some(w), Some(l)) = (cur.as_mut(), last) {
                // Overprinted duplicate (fake bold).
                if l.text == g.text && (g.x0 - l.x0).abs() < 0.08 * g.size {
                    continue;
                }
                let gap = g.x0 - w.x1;
                if gap > 0.17 * g.size.min(w.size) || gap < -0.5 * g.size {
                    flush(&mut cur, &mut acc);
                }
            }
            match cur.as_mut() {
                Some(w) => {
                    w.text.push_str(&g.text);
                    w.x1 = w.x1.max(g.x1);
                }
                None => {
                    cur = Some(Word {
                        text: g.text.clone(),
                        x0: g.x0,
                        x1: g.x1,
                        y: g.y,
                        size: g.size,
                        style: 0,
                        link: None,
                    });
                }
            }
            acc.glyphs += 1;
            for (k, flag) in FLAGS.iter().enumerate() {
                if g.style & flag != 0 {
                    acc.votes[k] += 1;
                }
            }
            let key = (g.size * 2.0).round() / 2.0;
            match acc.sizes.iter_mut().find(|(sz, _)| *sz == key) {
                Some((_, n)) => *n += 1,
                None => acc.sizes.push((key, 1)),
            }
            last = Some(g);
        }
        flush(&mut cur, &mut acc);
    }

    for w in &mut words {
        let (cx, cy) = ((w.x0 + w.x1) / 2.0, w.y - 0.3 * w.size);
        w.link = links
            .iter()
            .position(|l| cx >= l.x0 && cx <= l.x1 && cy >= l.y0 && cy <= l.y1);
    }
    words
}

pub fn build_lines(words: &[Word]) -> Vec<Line> {
    let order: Vec<usize> = (0..words.len()).collect();
    cluster_lines(order, |i| words[i].y, |i| words[i].size, |i| words[i].x0)
        .into_iter()
        .map(|idxs| {
            let ws: Vec<Word> = idxs.iter().map(|&i| words[i].clone()).collect();
            let dominant = ws
                .iter()
                .max_by_key(|w| w.text.chars().count())
                .map(|w| w.size)
                .unwrap_or(10.0);
            Line {
                x0: ws.iter().map(|w| w.x0).fold(f32::MAX, f32::min),
                x1: ws.iter().map(|w| w.x1).fold(f32::MIN, f32::max),
                y: ws.iter().map(|w| w.y).sum::<f32>() / ws.len() as f32,
                size: dominant,
                words: ws,
                ids: idxs,
            }
        })
        .collect()
}

fn needs_space(a: &Word, b: &Word) -> bool {
    let (Some(l), Some(f)) = (a.text.chars().last(), b.text.chars().next()) else {
        return true;
    };
    !(is_cjk(l) && is_cjk(f))
}

pub fn plain(words: &[Word]) -> String {
    let mut out = String::new();
    for (i, w) in words.iter().enumerate() {
        if i > 0 && needs_space(&words[i - 1], w) {
            out.push(' ');
        }
        out.push_str(&w.text);
    }
    out
}

fn wrap(text: &str, style: u8, whole_mono: bool) -> String {
    let mut t = text.to_string();
    if style & STYLE_MONO != 0 && !whole_mono {
        return format!("`{}`", t.replace('`', "'"));
    }
    if style & STYLE_BOLD != 0 && style & STYLE_ITALIC != 0 {
        t = format!("***{t}***");
    } else if style & STYLE_BOLD != 0 {
        t = format!("**{t}**");
    } else if style & STYLE_ITALIC != 0 {
        t = format!("*{t}*");
    }
    t
}

/// Render words to inline Markdown, applying emphasis and links.
pub fn render_inline(words: &[Word], links: &[LinkRect], emphasis: bool) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < words.len() {
        let key = |w: &Word| (if emphasis { w.style } else { 0 }, w.link);
        let k = key(&words[i]);
        let mut j = i + 1;
        while j < words.len() && key(&words[j]) == k {
            j += 1;
        }
        let run = &words[i..j];
        let mut text = plain(run);
        if emphasis {
            text = wrap(&text, k.0, false);
        }
        if let Some(l) = k.1.and_then(|l| links.get(l)) {
            text = format!("[{text}]({})", l.url);
        }
        if i > 0 && needs_space(&words[i - 1], &words[i]) {
            out.push(' ');
        }
        out.push_str(&text);
        i = j;
    }
    out
}
