//! Fixed two-column wrist geometry and whole-row pagination, independent of the renderer.

use std::ops::Range;

use crate::controllers::summary::{
    content::{Metrics, metrics},
    current::{CellKind, CurrentSummary},
};

#[derive(Debug, Copy, Clone, PartialEq)]
pub(crate) struct Bounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CellPage {
    pub kind: CellKind,
    pub bounds: Bounds,
    pub rows: Vec<(usize, Bounds)>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Page {
    pub cells: Vec<CellPage>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Grid {
    pub metrics: Metrics,
    pub size: (u32, u32),
    pub heading: Bounds,
    pub footer: Bounds,
    pub pages: Vec<Page>,
}

/// Measure labels once at a fixed readable size. Every page retains physical left/right columns;
/// an empty pair consumes no height. A long indivisible row receives its own page without shrinking.
pub(crate) fn layout(
    summary: &CurrentSummary,
    width: u32,
    max_height: u32,
    row_px: f32,
    measure: &dyn Fn(&crate::controllers::summary::current::Hint, f32, Metrics) -> f32,
) -> Grid {
    let m = metrics(row_px);
    let margin = m.margin;
    let width = (width as f32).max(m.row_px * 14.0);
    let cell_width = (width - margin * 3.0) * 0.5;
    let heading = Bounds {
        x: margin,
        y: margin,
        width: width - margin * 2.0,
        height: m.heading_px,
    };
    let content_y = heading.y + heading.height + margin;
    let pair_height = m.group_px + m.row_px * ROWS_PER_CELL as f32;
    let minimum_height = content_y + pair_height + m.row_px + margin * 2.0;
    let height_limit = (max_height as f32).max(minimum_height);
    let pair_budget = ((height_limit - content_y - m.row_px - margin * 2.0) * 0.5 - margin)
        .min(pair_height)
        .max(m.group_px + m.row_px);
    let row_budget = pair_budget - m.group_px;
    let measured: [Vec<f32>; 4] = std::array::from_fn(|cell| {
        summary.cells[cell]
            .iter()
            .map(|hint| measure(hint, cell_width, m).max(m.row_px))
            .collect()
    });
    let chunks: [Vec<Range<usize>>; 4] =
        std::array::from_fn(|cell| chunks(&measured[cell], row_budget));
    let count = chunks.iter().map(Vec::len).max().unwrap_or(0).max(1);
    let mut pages = Vec::new();
    let mut tallest = content_y;
    for page in 0..count {
        let mut cells = Vec::new();
        let mut y = content_y;
        for pair in [0..2, 2..4] {
            let pair_used = pair.clone().any(|index| page < chunks[index].len());
            if !pair_used {
                continue;
            }
            let mut used_height = 0.0f32;
            for index in pair {
                let Some(range) = chunks[index].get(page) else {
                    continue;
                };
                let kind = CellKind::ALL[index];
                let x = margin + (index % 2) as f32 * (cell_width + margin);
                let mut row_y = y + m.group_px;
                let mut rows = Vec::new();
                for row in range.clone() {
                    let height = measured[index][row];
                    rows.push((
                        row,
                        Bounds {
                            x,
                            y: row_y,
                            width: cell_width,
                            height,
                        },
                    ));
                    row_y += height;
                }
                let height = row_y - y;
                used_height = used_height.max(height);
                cells.push(CellPage {
                    kind,
                    bounds: Bounds {
                        x,
                        y,
                        width: cell_width,
                        height,
                    },
                    rows,
                });
            }
            y += used_height + margin;
        }
        tallest = tallest.max(y);
        pages.push(Page { cells });
    }
    let footer = Bounds {
        x: margin,
        y: tallest,
        width: width - margin * 2.0,
        height: m.row_px,
    };
    Grid {
        metrics: m,
        size: (
            width.ceil() as u32,
            (footer.y + footer.height + margin).ceil() as u32,
        ),
        heading,
        footer,
        pages,
    }
}

fn chunks(heights: &[f32], budget: f32) -> Vec<Range<usize>> {
    let mut result = Vec::new();
    let mut start = 0;
    while start < heights.len() {
        let mut end = start;
        let mut height = 0.0;
        while end < heights.len() && end - start < ROWS_PER_CELL {
            if end > start && height + heights[end] > budget {
                break;
            }
            height += heights[end];
            end += 1;
        }
        result.push(start..end);
        start = end;
    }
    result
}

const ROWS_PER_CELL: usize = 4;
