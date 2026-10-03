//! Page layout reconstruction: reading order (XY-cut), headings, lists,
//! code blocks, paragraphs and table placement.

use std::collections::HashSet;

use super::content::{ImagePlacement, LinkRect, PageContent, STYLE_BOLD, STYLE_MONO};
use super::table::{Table, aligned_tables, ruled_tables};
use super::words::{Line, Word, build_lines, plain, render_inline, text_width};

pub enum Block {
    Heading(u8, String),
    Paragraph(String),
    ListItem {
        level: u8,
        marker: Option<String>,
        text: String,
    },
    Code(Vec<String>),
    Table(Vec<Vec<String>>),
    Image(usize),
    Raw(String),
}

#[derive(Default)]
pub struct DocStats {
    pub body_size: f32,
    /// Distinct heading font sizes, largest first.
    pub heading_sizes: Vec<f32>,
    /// Normalised running header/footer texts to drop.
    pub edge_texts: HashSet<String>,
}

// ---------------------------------------------------------------------------
// Document statistics
// ---------------------------------------------------------------------------

pub fn compute_body_and_headings(all_words: &[&[Word]]) -> (f32, Vec<f32>) {
    let key = |s: f32| (s * 2.0).round() as i32;
    let mut weights: std::collections::BTreeMap<i32, usize> = Default::default();
    for words in all_words {
        for w in *words {
            *weights.entry(key(w.size)).or_default() += w.text.chars().count();
        }
    }
    let Some((&body_key, _)) = weights.iter().max_by_key(|(_, n)| **n) else {
        return (10.0, Vec::new());
    };
    let body = body_key as f32 / 2.0;
    let mut heads: Vec<f32> = weights
        .iter()
        .filter(|(k, n)| **k as f32 / 2.0 >= body * 1.12 && **n >= 4)
        .map(|(k, _)| *k as f32 / 2.0)
        .collect();
    heads.sort_by(|a, b| b.total_cmp(a));
    let mut dedup: Vec<f32> = Vec::new();
    for h in heads {
        if dedup.last().is_none_or(|l| (l - h).abs() > 0.6) {
            dedup.push(h);
        }
    }
    (body, dedup)
}

pub fn norm_edge(s: &str) -> String {
    let mut out = String::new();
    let mut last_digit = false;
    for c in s.split_whitespace().collect::<Vec<_>>().join(" ").chars() {
        if c.is_ascii_digit() {
            if !last_digit {
                out.push('#');
            }
            last_digit = true;
        } else {
            out.extend(c.to_lowercase());
            last_digit = false;
        }
    }
    out
}

fn in_edge_band(y: f32, height: f32) -> bool {
    y <= height * 0.10 || y >= height * 0.90
}

/// First/last two text lines of the page when they sit in the header/footer band.
fn edge_lines(lines: &[Line], height: f32) -> Vec<usize> {
    let n = lines.len();
    let mut idx: Vec<usize> = (0..n.min(2)).chain(n.saturating_sub(2)..n).collect();
    idx.dedup();
    idx.retain(|&i| in_edge_band(lines[i].y, height));
    idx
}

pub fn edge_keys(words: &[Word], height: f32) -> HashSet<String> {
    let lines = build_lines(words);
    let mut set = HashSet::new();
    for i in edge_lines(&lines, height) {
        let t = plain(&lines[i].words);
        if t.chars().count() <= 100 {
            let n = norm_edge(&t);
            if !n.is_empty() {
                set.insert(n);
            }
        }
    }
    set
}

fn strip_edges(words: Vec<Word>, height: f32, edges: &HashSet<String>) -> Vec<Word> {
    if edges.is_empty() {
        return words;
    }
    let mut drop = vec![false; words.len()];
    let lines = build_lines(&words);
    for li in edge_lines(&lines, height) {
        if edges.contains(&norm_edge(&plain(&lines[li].words))) {
            for &i in &lines[li].ids {
                drop[i] = true;
            }
        }
    }
    words
        .into_iter()
        .zip(drop)
        .filter_map(|(w, d)| (!d).then_some(w))
        .collect()
}

// ---------------------------------------------------------------------------
// XY-cut reading order
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Kind {
    Word(usize),
    Table(usize),
    Image(usize),
}

#[derive(Clone, Copy)]
struct Item {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    kind: Kind,
}

fn find_gap(
    items: &[Item],
    ids: &[usize],
    lo: fn(&Item) -> f32,
    hi: fn(&Item) -> f32,
    min_gap: f32,
) -> Option<(Vec<usize>, Vec<usize>)> {
    let mut sorted = ids.to_vec();
    sorted.sort_by(|&a, &b| lo(&items[a]).total_cmp(&lo(&items[b])));
    let mut max_hi = hi(&items[sorted[0]]);
    let mut best: Option<(usize, f32)> = None;
    for (k, &id) in sorted.iter().enumerate().skip(1) {
        let gap = lo(&items[id]) - max_hi;
        if gap >= min_gap && best.is_none_or(|(_, g)| gap > g) {
            best = Some((k, gap));
        }
        max_hi = max_hi.max(hi(&items[id]));
    }
    best.map(|(k, _)| (sorted[..k].to_vec(), sorted[k..].to_vec()))
}

fn xy_cut(
    items: &[Item],
    ids: Vec<usize>,
    body: f32,
    h_gap: f32,
    depth: u32,
    out: &mut Vec<Vec<usize>>,
) {
    if ids.len() <= 1 || depth > 48 {
        out.push(ids);
        return;
    }
    let v_gap = if ids.len() < 6 {
        body * 2.5
    } else {
        (body * 1.2).max(9.0)
    };
    if let Some((l, r)) = find_gap(items, &ids, |i| i.x0, |i| i.x1, v_gap) {
        xy_cut(items, l, body, h_gap, depth + 1, out);
        xy_cut(items, r, body, h_gap, depth + 1, out);
        return;
    }
    if let Some((t, b)) = find_gap(items, &ids, |i| i.y0, |i| i.y1, h_gap) {
        xy_cut(items, t, body, h_gap, depth + 1, out);
        xy_cut(items, b, body, h_gap, depth + 1, out);
        return;
    }
    out.push(ids);
}

// ---------------------------------------------------------------------------
// Entries: ordered lines / tables / images
// ---------------------------------------------------------------------------

enum Entry {
    Line { line: Line, leaf_right: f32 },
    Table(usize),
    Image(usize),
}

fn is_sentence_end(s: &str) -> bool {
    s.trim_end()
        .chars()
        .last()
        .is_some_and(|c| matches!(c, '.' | '!' | '?' | '。' | '！' | '？' | '」' | ':' | '：'))
}

fn line_text(l: &Line) -> String {
    plain(&l.words)
}

fn all_style(l: &Line, flag: u8) -> bool {
    !l.words.is_empty() && l.words.iter().all(|w| w.style & flag != 0)
}

fn list_marker(l: &Line) -> Option<(Option<String>, usize)> {
    // Returns (ordered marker, number of words the marker occupies) and
    // for bullets the first word may carry the text after the bullet.
    let first = l.words.first()?;
    let t = first.text.as_str();
    let mut chars = t.chars();
    if let Some(c) = chars.next()
        && BULLETS.contains(&c)
        && t.chars().count() == 1
        && l.words.len() > 1
    {
        return Some((None, 1));
    }
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    let rest = &t[digits.len()..];
    if !digits.is_empty() && digits.len() <= 3 && (rest == "." || rest == ")") && l.words.len() > 1
    {
        return Some((Some(format!("{digits}.")), 1));
    }
    None
}

const BULLETS: &[char] = &[
    '•', '●', '○', '◦', '▪', '■', '□', '▫', '‣', '◆', '◇', '・', '-', '–', '—', '*', '·',
];

fn is_marker_text(t: &str) -> bool {
    let mut chars = t.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        return BULLETS.contains(&c);
    }
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    let rest = &t[digits.len()..];
    !digits.is_empty() && digits.len() <= 3 && (rest == "." || rest == ")")
}

const LEADERS: &[char] = &['·', '.', '…', '・', '‥', '∙', '•'];

fn is_leader_word(t: &str) -> bool {
    !t.is_empty() && t.chars().all(|c| LEADERS.contains(&c))
}

/// A table-of-contents style line ("Title ........ 12"): returns the line with
/// the dot leader collapsed into a single "…" word.
fn collapse_leader(l: &Line) -> Option<Line> {
    let leader_chars: usize = l
        .words
        .iter()
        .flat_map(|w| w.text.chars())
        .filter(|c| LEADERS.contains(c))
        .count();
    if leader_chars < 5 || l.words.len() < 3 {
        return None;
    }
    let mut words: Vec<Word> = Vec::new();
    let mut ids: Vec<usize> = Vec::new();
    let mut last_was_leader = false;
    for (w, &id) in l.words.iter().zip(&l.ids) {
        if is_leader_word(&w.text) {
            if !last_was_leader {
                let mut dots = w.clone();
                dots.text = "…".into();
                dots.style = 0;
                words.push(dots);
                ids.push(id);
            }
            last_was_leader = true;
            continue;
        }
        let trimmed = w.text.trim_end_matches(|c| LEADERS.contains(&c));
        let mut word = w.clone();
        word.text = trimmed.to_string();
        let had_leader = trimmed.len() != w.text.len();
        words.push(word);
        ids.push(id);
        if had_leader {
            let mut dots = w.clone();
            dots.text = "…".into();
            dots.style = 0;
            words.push(dots);
            ids.push(id);
        }
        last_was_leader = had_leader;
    }
    // Needs real text on both sides of the leader to be a contents entry.
    let has_leader = words.iter().any(|w| w.text == "…");
    let has_text = words.iter().any(|w| w.text != "…");
    let mut out = l.clone();
    out.words = words;
    out.ids = ids;
    (has_leader && has_text && out.words.last().is_some_and(|w| w.text != "…")).then_some(out)
}

fn heading_level(l: &Line, stats: &DocStats) -> Option<u8> {
    if stats.heading_sizes.is_empty() || l.size < stats.body_size * 1.12 {
        return None;
    }
    let text = line_text(l);
    if text_width(&text) > 160 {
        return None;
    }
    let rank = stats
        .heading_sizes
        .iter()
        .enumerate()
        .min_by(|a, b| (a.1 - l.size).abs().total_cmp(&(b.1 - l.size).abs()))
        .map(|(i, _)| i)?;
    Some((rank as u8 + 1).min(6))
}

// ---------------------------------------------------------------------------
// Page layout
// ---------------------------------------------------------------------------

pub fn layout_page(
    page: &PageContent,
    words: Vec<Word>,
    stats: &DocStats,
    with_images: bool,
) -> Vec<Block> {
    let words = strip_edges(words, page.height, &stats.edge_texts);

    let (mut tables, mut consumed) = ruled_tables(&words, &page.rules);
    let aligned = aligned_tables(&words, &mut consumed);
    tables.extend(aligned);
    let free: Vec<Word> = words
        .into_iter()
        .zip(consumed)
        .filter(|(_, taken)| !taken)
        .map(|(w, _)| w)
        .collect();

    let body = stats.body_size.max(4.0);

    // A list marker and its text must stay in one XY-cut item, otherwise the
    // gap between them looks like a column gutter.
    let mut right: Vec<f32> = free.iter().map(|w| w.x1).collect();
    for l in build_lines(&free) {
        for k in 0..l.words.len().saturating_sub(1) {
            let (a, b) = (&l.words[k], &l.words[k + 1]);
            if is_marker_text(&a.text) && b.x0 - a.x1 < 3.5 * l.size {
                right[l.ids[k]] = b.x0 + 0.1;
            }
        }
    }

    // Baseline pitch of body lines, measured within columns (a line and the
    // next line below it that overlaps horizontally).
    let all_lines = build_lines(&free);
    let mut pitches: Vec<f32> = Vec::new();
    for (i, a) in all_lines.iter().enumerate() {
        if let Some(b) = all_lines[i + 1..]
            .iter()
            .take(6)
            .find(|b| b.y - a.y > 0.5 * a.size && a.x0 < b.x1 && b.x0 < a.x1)
        {
            let p = b.y - a.y;
            if p < 3.0 * a.size.max(b.size) {
                pitches.push(p);
            }
        }
    }
    pitches.sort_by(f32::total_cmp);
    let pitch_med = pitches
        .get(pitches.len() / 2)
        .copied()
        .unwrap_or(body * 1.2)
        .max(body);
    // Gaps up to ~1.5x the ordinary inter-line gap never separate blocks.
    let h_gap = (body * 0.45).max((pitch_med - body * 1.1).max(0.0) * 1.5);

    let mut items: Vec<Item> = Vec::new();
    for (i, w) in free.iter().enumerate() {
        items.push(Item {
            x0: w.x0,
            x1: right[i],
            y0: w.y - 0.85 * w.size,
            y1: w.y + 0.25 * w.size,
            kind: Kind::Word(i),
        });
    }
    for (i, t) in tables.iter().enumerate() {
        items.push(Item {
            x0: t.x0,
            y0: t.y0,
            x1: t.x1,
            y1: t.y1,
            kind: Kind::Table(i),
        });
    }
    let images: Vec<&ImagePlacement> = if with_images {
        page.images.iter().collect()
    } else {
        Vec::new()
    };
    for (i, im) in images.iter().enumerate() {
        items.push(Item {
            x0: im.x0,
            y0: im.y0,
            x1: im.x1,
            y1: im.y1,
            kind: Kind::Image(i),
        });
    }
    if items.is_empty() {
        return Vec::new();
    }

    let mut leaves: Vec<Vec<usize>> = Vec::new();
    xy_cut(
        &items,
        (0..items.len()).collect(),
        body,
        h_gap,
        0,
        &mut leaves,
    );

    let mut entries: Vec<Entry> = Vec::new();
    for leaf in &leaves {
        let leaf_words: Vec<Word> = leaf
            .iter()
            .filter_map(|&i| match items[i].kind {
                Kind::Word(w) => Some(free[w].clone()),
                _ => None,
            })
            .collect();
        let lines = build_lines(&leaf_words);
        let leaf_right = lines.iter().map(|l| l.x1).fold(f32::MIN, f32::max);
        let mut others: Vec<(f32, Entry)> = leaf
            .iter()
            .filter_map(|&i| match items[i].kind {
                Kind::Table(t) => Some((items[i].y0, Entry::Table(t))),
                Kind::Image(m) => Some((items[i].y0, Entry::Image(m))),
                Kind::Word(_) => None,
            })
            .collect();
        others.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut others = others.into_iter().peekable();
        for line in lines {
            while others.peek().is_some_and(|(y, _)| *y < line.y - line.size) {
                entries.push(others.next().unwrap().1);
            }
            entries.push(Entry::Line { line, leaf_right });
        }
        entries.extend(others.map(|(_, e)| e));
    }

    let mut blocks = Builder::new(stats, &page.links, pitch_med).run(&entries);
    attach_tables(&mut blocks, tables);
    blocks
}

/// The builder emits `Block::Table(vec![])` placeholders carrying the table
/// index in a single cell; swap them for the real rows.
fn attach_tables(blocks: &mut [Block], tables: Vec<Table>) {
    let mut tables: Vec<Option<Table>> = tables.into_iter().map(Some).collect();
    for b in blocks.iter_mut() {
        if let Block::Table(rows) = b
            && let Some(idx) = rows
                .first()
                .and_then(|r| r.first())
                .and_then(|c| c.parse::<usize>().ok())
            && let Some(t) = tables.get_mut(idx).and_then(Option::take)
        {
            *rows = t.rows;
        }
    }
}

// ---------------------------------------------------------------------------
// Block builder
// ---------------------------------------------------------------------------

enum Cur {
    None,
    Para(Vec<Line>),
    Item {
        level: u8,
        marker: Option<String>,
        lines: Vec<Line>,
        text_x0: f32,
    },
    Code(Vec<Line>),
    Head {
        level: u8,
        lines: Vec<Line>,
    },
}

struct Builder<'a> {
    stats: &'a DocStats,
    links: &'a [LinkRect],
    pitch: f32,
    out: Vec<Block>,
    cur: Cur,
    list_base: Option<f32>,
}

impl<'a> Builder<'a> {
    fn new(stats: &'a DocStats, links: &'a [LinkRect], pitch: f32) -> Self {
        Builder {
            stats,
            links,
            pitch,
            out: Vec::new(),
            cur: Cur::None,
            list_base: None,
        }
    }

    fn join(&self, lines: &[Line], emphasis: bool) -> String {
        // Flatten to one word list so emphasis/link runs span line breaks, and
        // undo end-of-line hyphenation ("redirec-" + "tion").
        let mut words: Vec<Word> = Vec::new();
        for l in lines {
            let mut rest = l.words.iter().cloned().peekable();
            if let (Some(last), Some(first)) = (words.last_mut(), rest.peek()) {
                let hyphenated = last.text.ends_with('-')
                    && last
                        .text
                        .chars()
                        .rev()
                        .nth(1)
                        .is_some_and(|c| c.is_alphabetic())
                    && first.text.chars().next().is_some_and(|c| c.is_lowercase());
                if hyphenated {
                    last.text.pop();
                    last.text.push_str(&first.text);
                    last.x1 = first.x1;
                    rest.next();
                }
            }
            words.extend(rest);
        }
        render_inline(&words, self.links, emphasis)
    }

    fn flush(&mut self) {
        match std::mem::replace(&mut self.cur, Cur::None) {
            Cur::None => {}
            Cur::Para(lines) => {
                let single_bold = lines.len() == 1
                    && all_style(&lines[0], STYLE_BOLD)
                    && !all_style(&lines[0], STYLE_MONO)
                    && text_width(&line_text(&lines[0])) <= 90
                    && !line_text(&lines[0]).ends_with(['.', ',', ';', ':', '。', '、', '!', '?']);
                if single_bold {
                    let level = (self.stats.heading_sizes.len() as u8 + 1).clamp(2, 6);
                    let text = self.join(&lines, false);
                    self.out.push(Block::Heading(level, text));
                } else {
                    let mut text = self.join(&lines, true);
                    if text.starts_with(['#', '>']) {
                        text.insert(0, '\\');
                    }
                    self.out.push(Block::Paragraph(text));
                }
            }
            Cur::Item {
                level,
                marker,
                lines,
                ..
            } => {
                let text = self.join(&lines, true);
                self.out.push(Block::ListItem {
                    level,
                    marker,
                    text,
                });
            }
            Cur::Code(lines) => {
                let min_x = lines.iter().map(|l| l.x0).fold(f32::MAX, f32::min);
                let rows = lines
                    .iter()
                    .map(|l| {
                        let cw = (l.size * 0.6).max(1.0);
                        let mut s = String::new();
                        for w in &l.words {
                            let col = ((w.x0 - min_x) / cw).round().max(0.0) as usize;
                            while s.chars().count() < col {
                                s.push(' ');
                            }
                            if !s.is_empty() && !s.ends_with(' ') {
                                s.push(' ');
                            }
                            s.push_str(&w.text);
                        }
                        s
                    })
                    .collect();
                self.out.push(Block::Code(rows));
            }
            Cur::Head { level, lines } => {
                let text = self.join(&lines, false);
                self.out.push(Block::Heading(level, text));
            }
        }
    }

    fn breaks(&self, prev: &Line, l: &Line, leaf_right: f32) -> bool {
        let size = prev.size.max(l.size);
        let pitch = l.y - prev.y;
        if pitch < -0.5 * size {
            return true;
        }
        if pitch > 1.4 * self.pitch.max(size * 1.1) {
            return true;
        }
        let prev_text = line_text(prev);
        if is_sentence_end(&prev_text) {
            if l.x0 > prev.x0 + 0.9 * size {
                return true;
            }
            if prev.x1 < leaf_right - 6.0 * size && prev_text.chars().count() > 3 {
                return true;
            }
        }
        false
    }

    fn run(mut self, entries: &[Entry]) -> Vec<Block> {
        let mut prev: Option<&Line> = None;
        for e in entries {
            match e {
                Entry::Table(i) => {
                    self.flush();
                    prev = None;
                    self.out.push(Block::Table(vec![vec![i.to_string()]]));
                }
                Entry::Image(i) => {
                    self.flush();
                    prev = None;
                    self.out.push(Block::Image(*i));
                }
                Entry::Line {
                    line: l,
                    leaf_right,
                } => {
                    self.line(l, prev, *leaf_right);
                    prev = Some(l);
                }
            }
        }
        self.flush();
        self.out
    }

    fn line(&mut self, l: &Line, prev: Option<&Line>, leaf_right: f32) {
        if l.words.is_empty() {
            return;
        }
        if let Some(entry) = collapse_leader(l) {
            self.flush();
            let text = self.join(std::slice::from_ref(&entry), true);
            self.out.push(Block::ListItem {
                level: 0,
                marker: None,
                text,
            });
            return;
        }
        if all_style(l, STYLE_MONO) {
            match &mut self.cur {
                Cur::Code(lines) => lines.push(l.clone()),
                _ => {
                    self.flush();
                    self.cur = Cur::Code(vec![l.clone()]);
                }
            }
            return;
        }
        if let Some(level) = heading_level(l, self.stats) {
            match &mut self.cur {
                Cur::Head { level: lv, lines }
                    if *lv == level
                        && prev.is_some_and(|p| l.y - p.y <= 1.7 * l.size && l.y > p.y) =>
                {
                    lines.push(l.clone());
                }
                _ => {
                    self.flush();
                    self.cur = Cur::Head {
                        level,
                        lines: vec![l.clone()],
                    };
                }
            }
            return;
        }
        if let Some((marker, n)) = list_marker(l) {
            self.flush();
            let mut item = l.clone();
            item.words.drain(..n);
            item.ids.drain(..n);
            let text_x0 = item.words.first().map_or(l.x0, |w| w.x0);
            item.x0 = text_x0;
            let base = *self.list_base.get_or_insert(l.x0);
            let level = ((l.x0 - base) / (1.5 * l.size)).round().clamp(0.0, 4.0) as u8;
            self.cur = Cur::Item {
                level,
                marker,
                lines: vec![item],
                text_x0,
            };
            return;
        }
        let brk = prev.is_none_or(|p| self.breaks(p, l, leaf_right));
        match &mut self.cur {
            Cur::Item { lines, text_x0, .. } if !brk && l.x0 >= *text_x0 - 0.7 * l.size => {
                lines.push(l.clone());
            }
            Cur::Para(lines) if !brk => lines.push(l.clone()),
            _ => {
                self.flush();
                self.list_base = None;
                self.cur = Cur::Para(vec![l.clone()]);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

fn escape_cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

pub fn render_blocks(blocks: &[Block]) -> String {
    let mut out = String::new();
    let mut prev_list = false;
    for b in blocks {
        let is_list = matches!(b, Block::ListItem { .. });
        if !out.is_empty() && !(is_list && prev_list) {
            out.push('\n');
        }
        prev_list = is_list;
        match b {
            Block::Heading(level, text) => {
                out.push_str(&format!("{} {}\n", "#".repeat(*level as usize), text));
            }
            Block::Paragraph(text) => out.push_str(&format!("{text}\n")),
            Block::ListItem {
                level,
                marker,
                text,
            } => {
                let indent = "  ".repeat(*level as usize);
                let m = marker.clone().unwrap_or_else(|| "-".to_string());
                out.push_str(&format!("{indent}{m} {text}\n"));
            }
            Block::Code(lines) => {
                out.push_str("```\n");
                for l in lines {
                    out.push_str(l);
                    out.push('\n');
                }
                out.push_str("```\n");
            }
            Block::Table(rows) => {
                if let Some(first) = rows.first() {
                    let n = first.len();
                    let row = |r: &Vec<String>| {
                        format!(
                            "| {} |\n",
                            r.iter()
                                .map(|c| escape_cell(c))
                                .collect::<Vec<_>>()
                                .join(" | ")
                        )
                    };
                    out.push_str(&row(first));
                    out.push_str(&format!("| {} |\n", vec!["---"; n].join(" | ")));
                    for r in &rows[1..] {
                        out.push_str(&row(r));
                    }
                }
            }
            Block::Image(_) => {}
            Block::Raw(s) => out.push_str(&format!("{s}\n")),
        }
    }
    out
}
