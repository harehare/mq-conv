//! Table detection: ruled grids (drawn lines, merged cells) and borderless
//! tables inferred from aligned whitespace columns.

use super::content::Rule;
use super::words::{Word, build_lines, plain, text_width};

pub struct Table {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub rows: Vec<Vec<String>>,
}

const TOL: f32 = 2.5;

#[derive(Clone, Copy)]
struct Seg {
    /// Position on the cross axis (y for horizontal, x for vertical).
    pos: f32,
    lo: f32,
    hi: f32,
}

fn merge_segments(mut segs: Vec<Seg>) -> Vec<Seg> {
    segs.sort_by(|a, b| a.pos.total_cmp(&b.pos).then(a.lo.total_cmp(&b.lo)));
    let mut out: Vec<Seg> = Vec::new();
    for s in segs {
        if let Some(l) = out.last_mut()
            && (l.pos - s.pos).abs() <= 1.5
            && s.lo <= l.hi + TOL
        {
            l.hi = l.hi.max(s.hi);
            continue;
        }
        out.push(s);
    }
    out
}

fn cluster_positions(mut vals: Vec<f32>) -> Vec<f32> {
    vals.sort_by(f32::total_cmp);
    let mut out: Vec<f32> = Vec::new();
    for v in vals {
        match out.last_mut() {
            Some(l) if (v - *l).abs() <= TOL => *l = (*l + v) / 2.0,
            _ => out.push(v),
        }
    }
    out
}

struct Dsu(Vec<usize>);
impl Dsu {
    fn new(n: usize) -> Self {
        Dsu((0..n).collect())
    }
    fn find(&mut self, x: usize) -> usize {
        if self.0[x] != x {
            let r = self.find(self.0[x]);
            self.0[x] = r;
        }
        self.0[x]
    }
    fn union(&mut self, a: usize, b: usize) {
        let (a, b) = (self.find(a), self.find(b));
        if a != b {
            self.0[b] = a;
        }
    }
}

/// Detect tables from ruling lines. Returns the tables and a per-word flag
/// marking words that were absorbed into a table.
pub fn ruled_tables(words: &[Word], rules: &[Rule]) -> (Vec<Table>, Vec<bool>) {
    let mut consumed = vec![false; words.len()];
    let mut hs = Vec::new();
    let mut vs = Vec::new();
    for r in rules {
        if (r.y1 - r.y0).abs() < 1.0 && r.x1 - r.x0 >= 4.0 {
            hs.push(Seg {
                pos: r.y0,
                lo: r.x0,
                hi: r.x1,
            });
        } else if (r.x1 - r.x0).abs() < 1.0 && r.y1 - r.y0 >= 4.0 {
            vs.push(Seg {
                pos: r.x0,
                lo: r.y0,
                hi: r.y1,
            });
        }
    }
    let hs = merge_segments(hs);
    let vs = merge_segments(vs);
    if hs.len() < 2 || vs.len() < 2 {
        return (Vec::new(), consumed);
    }

    // Connected components of intersecting horizontal/vertical segments.
    let mut dsu = Dsu::new(hs.len() + vs.len());
    for (i, h) in hs.iter().enumerate() {
        for (j, v) in vs.iter().enumerate() {
            if v.pos >= h.lo - TOL
                && v.pos <= h.hi + TOL
                && h.pos >= v.lo - TOL
                && h.pos <= v.hi + TOL
            {
                dsu.union(i, hs.len() + j);
            }
        }
    }
    let mut comps: std::collections::BTreeMap<usize, (Vec<Seg>, Vec<Seg>)> = Default::default();
    for (i, h) in hs.iter().enumerate() {
        comps.entry(dsu.find(i)).or_default().0.push(*h);
    }
    for (j, v) in vs.iter().enumerate() {
        comps.entry(dsu.find(hs.len() + j)).or_default().1.push(*v);
    }

    let mut tables = Vec::new();
    for (_, (ch, cv)) in comps {
        if ch.len() < 2 || cv.len() < 2 {
            continue;
        }
        if let Some(t) = grid_table(words, &ch, &cv, &mut consumed) {
            tables.push(t);
        }
    }
    (tables, consumed)
}

fn grid_table(words: &[Word], hs: &[Seg], vs: &[Seg], consumed: &mut [bool]) -> Option<Table> {
    let ys = cluster_positions(hs.iter().map(|h| h.pos).collect());
    let xs = cluster_positions(vs.iter().map(|v| v.pos).collect());
    if ys.len() < 2 || xs.len() < 2 {
        return None;
    }
    let (rows, cols) = (ys.len() - 1, xs.len() - 1);
    if rows * cols < 2 || rows > 200 || cols > 40 {
        return None;
    }

    // Is there a vertical rule at xs[j] covering the row band i (and vice versa)?
    let v_edge = |j: usize, i: usize| {
        vs.iter()
            .any(|v| (v.pos - xs[j]).abs() <= TOL && v.lo <= ys[i] + TOL && v.hi >= ys[i + 1] - TOL)
    };
    let h_edge = |i: usize, j: usize| {
        hs.iter()
            .any(|h| (h.pos - ys[i]).abs() <= TOL && h.lo <= xs[j] + TOL && h.hi >= xs[j + 1] - TOL)
    };

    let mut dsu = Dsu::new(rows * cols);
    for i in 0..rows {
        for j in 0..cols {
            if j + 1 < cols && !v_edge(j + 1, i) {
                dsu.union(i * cols + j, i * cols + j + 1);
            }
            if i + 1 < rows && !h_edge(i + 1, j) {
                dsu.union(i * cols + j, (i + 1) * cols + j);
            }
        }
    }

    let (bx0, bx1, by0, by1) = (xs[0], xs[cols], ys[0], ys[rows]);
    let mut cell_words: Vec<Vec<usize>> = vec![Vec::new(); rows * cols];
    let mut assigned: Vec<usize> = Vec::new();
    for (wi, w) in words.iter().enumerate() {
        let (cx, cy) = ((w.x0 + w.x1) / 2.0, w.y - 0.3 * w.size);
        if cx < bx0 - 1.0 || cx > bx1 + 1.0 || cy < by0 - 1.0 || cy > by1 + 1.0 {
            continue;
        }
        let j = (0..cols).find(|&j| cx >= xs[j] - 1.0 && cx < xs[j + 1] + 1.0);
        let i = (0..rows).find(|&i| cy >= ys[i] - 1.0 && cy < ys[i + 1] + 1.0);
        if let (Some(i), Some(j)) = (i, j) {
            let root = dsu.find(i * cols + j);
            cell_words[root].push(wi);
            assigned.push(wi);
        }
    }

    let mut grid: Vec<Vec<String>> = vec![vec![String::new(); cols]; rows];
    for (i, row) in grid.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            let idx = i * cols + j;
            if dsu.find(idx) != idx {
                continue;
            }
            let ws: Vec<Word> = cell_words[idx].iter().map(|&k| words[k].clone()).collect();
            *cell = build_lines(&ws)
                .iter()
                .map(|l| plain(&l.words))
                .collect::<Vec<_>>()
                .join(" ");
        }
    }

    // Diagram boxes and page frames are not tables: need a real grid whose
    // cells are mostly filled.
    let origins = (0..rows * cols).filter(|&k| dsu.find(k) == k).count();
    let filled = grid.iter().flatten().filter(|c| !c.is_empty()).count();
    let rows = finish_rows(grid)?;
    if rows.len() < 2 || rows[0].len() < 2 || filled * 2 < origins {
        return None;
    }
    for wi in assigned {
        consumed[wi] = true;
    }
    Some(Table {
        x0: bx0,
        y0: by0,
        x1: bx1,
        y1: by1,
        rows,
    })
}

/// Drop empty rows/columns; reject tables that carry no text.
fn finish_rows(mut grid: Vec<Vec<String>>) -> Option<Vec<Vec<String>>> {
    grid.retain(|r| r.iter().any(|c| !c.trim().is_empty()));
    if grid.is_empty() {
        return None;
    }
    let cols = grid[0].len();
    let keep: Vec<usize> = (0..cols)
        .filter(|&j| grid.iter().any(|r| !r[j].trim().is_empty()))
        .collect();
    if keep.is_empty() {
        return None;
    }
    let grid: Vec<Vec<String>> = grid
        .into_iter()
        .map(|r| keep.iter().map(|&j| r[j].trim().to_string()).collect())
        .collect();
    Some(grid)
}

fn is_list_marker(s: &str) -> bool {
    let t = s.trim();
    matches!(
        t,
        "•" | "●" | "○" | "◦" | "▪" | "■" | "□" | "‣" | "-" | "–" | "・" | "*"
    ) || (t.len() <= 4
        && (t.ends_with('.') || t.ends_with(')'))
        && t[..t.len() - 1].chars().all(|c| c.is_ascii_digit())
        && t.len() > 1)
}

/// Detect borderless tables from aligned columns of whitespace-separated
/// segments. Operates on words not already consumed by a ruled table.
pub fn aligned_tables(words: &[Word], consumed: &mut [bool]) -> Vec<Table> {
    let free: Vec<usize> = (0..words.len()).filter(|&i| !consumed[i]).collect();
    let free_words: Vec<Word> = free.iter().map(|&i| words[i].clone()).collect();
    let lines = build_lines(&free_words);

    // Split each line into segments separated by wide gaps.
    struct Segment {
        x0: f32,
        x1: f32,
        text: String,
        word_ids: Vec<usize>,
    }
    let segs_of_line: Vec<Vec<Segment>> = lines
        .iter()
        .map(|l| {
            let col_gap = l.size.max(7.0);
            let mut segs: Vec<Segment> = Vec::new();
            for (w, &id) in l.words.iter().zip(&l.ids) {
                match segs.last_mut() {
                    Some(s) if w.x0 - s.x1 <= col_gap => {
                        s.text.push(' ');
                        s.text.push_str(&w.text);
                        s.x1 = w.x1;
                        s.word_ids.push(id);
                    }
                    _ => segs.push(Segment {
                        x0: w.x0,
                        x1: w.x1,
                        text: w.text.clone(),
                        word_ids: vec![id],
                    }),
                }
            }
            segs
        })
        .collect();

    let mut tables = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if segs_of_line[i].len() < 2 {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        while j < lines.len()
            && segs_of_line[j].len() >= 2
            && lines[j].y - lines[j - 1].y <= 2.6 * lines[j].size
        {
            j += 1;
        }
        if j - i >= 3
            && let Some(t) = build_aligned(&lines[i..j], &segs_of_line[i..j], |s: &Segment| {
                (s.x0, s.x1, s.text.clone())
            })
        {
            for line_segs in &segs_of_line[i..j] {
                for s in line_segs {
                    for &id in &s.word_ids {
                        consumed[free[id]] = true;
                    }
                }
            }
            tables.push(t);
        }
        i = j.max(i + 1);
    }
    tables
}

fn build_aligned<S>(
    lines: &[super::words::Line],
    segs: &[Vec<S>],
    info: impl Fn(&S) -> (f32, f32, String),
) -> Option<Table> {
    // Column clusters from overlapping segment x-ranges.
    let mut spans: Vec<(f32, f32)> = segs
        .iter()
        .flatten()
        .map(|s| {
            let (a, b, _) = info(s);
            (a, b)
        })
        .collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut cols: Vec<(f32, f32)> = Vec::new();
    for (a, b) in spans {
        match cols.last_mut() {
            Some(c) if a <= c.1 + 1.0 => c.1 = c.1.max(b),
            _ => cols.push((a, b)),
        }
    }
    if cols.len() < 2 || cols.len() > 24 {
        return None;
    }

    let mut grid: Vec<Vec<String>> = Vec::new();
    for line_segs in segs {
        let mut row = vec![String::new(); cols.len()];
        for s in line_segs {
            let (a, b, text) = info(s);
            let c = (a + b) / 2.0;
            let k = cols.iter().position(|(lo, hi)| c >= *lo && c <= *hi)?;
            if !row[k].is_empty() {
                return None;
            }
            row[k] = text;
        }
        grid.push(row);
    }

    // Prose in several columns is not a table: require short cells.
    let lens: Vec<usize> = grid
        .iter()
        .flatten()
        .filter(|c| !c.is_empty())
        .map(|c| text_width(c))
        .collect();
    let mut sorted = lens.clone();
    sorted.sort_unstable();
    if sorted.get(sorted.len() / 2).copied().unwrap_or(0) > 28 {
        return None;
    }
    if cols.len() == 2 {
        let avg = |k: usize| {
            let v: Vec<usize> = grid
                .iter()
                .filter(|r| !r[k].is_empty())
                .map(|r| text_width(&r[k]))
                .collect();
            v.iter().sum::<usize>() as f32 / v.len().max(1) as f32
        };
        if avg(0).min(avg(1)) > 24.0 {
            return None;
        }
    }
    // A list (bullets / numbers in the first column) is not a table.
    if grid.iter().all(|r| is_list_marker(&r[0])) {
        return None;
    }
    // Most rows must populate most columns.
    let full = grid
        .iter()
        .filter(|r| r.iter().all(|c| !c.is_empty()))
        .count();
    if full * 2 < grid.len() {
        return None;
    }

    let rows = finish_rows(grid)?;
    Some(Table {
        x0: cols[0].0,
        x1: cols[cols.len() - 1].1,
        y0: lines.first()?.y - lines.first()?.size,
        y1: lines.last()?.y + 0.3 * lines.last()?.size,
        rows,
    })
}
