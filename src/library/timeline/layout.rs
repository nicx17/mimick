use crate::config::DateHeadings;
use crate::library::masonry::layout::{LaidItem, LaidRow, LayoutConfig, pack_rows};

pub const HEADING_HEIGHT: f32 = 40.0;
pub const ESTIMATED_ASPECT: f32 = 1.5;
const PHOTOS_PER_DAY: u32 = 8;

pub struct Group<'a> {
    pub heading: Option<String>,
    pub dims: &'a [(u32, u32)],
}

#[derive(Clone, Debug, PartialEq)]
pub struct LaidHeading {
    pub y: f32,
    pub text: String,
}

/// One month, or a run of months without headings, packed on its own:
/// `y` and `asset_index` start at the section; `starts` holds each month's top.
pub struct PackedSection {
    pub rows: Vec<LaidRow>,
    pub headings: Vec<LaidHeading>,
    pub height: f32,
    pub starts: Vec<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LaidSection {
    pub y: f32,
    pub h: f32,
    pub loaded: bool,
}

pub enum Placed<'a> {
    Packed {
        section: &'a PackedSection,
        model_offset: u32,
    },
    Estimated {
        height: f32,
    },
}

#[derive(Default)]
pub struct TimelineLayout {
    pub rows: Vec<LaidRow>,
    pub headings: Vec<LaidHeading>,
    pub sections: Vec<LaidSection>,
    pub height: f32,
}

pub fn pack_section(groups: &[Group<'_>], width: f32, cfg: LayoutConfig) -> PackedSection {
    let mut packed = PackedSection {
        rows: Vec::new(),
        headings: Vec::new(),
        height: 0.0,
        starts: vec![0.0],
    };
    let mut first_index = 0;
    for group in groups {
        if let Some(text) = &group.heading {
            packed.headings.push(LaidHeading {
                y: packed.height,
                text: text.clone(),
            });
            packed.height += HEADING_HEIGHT;
        }
        let (rows, height) = pack_rows(group.dims, width, cfg);
        let top = packed.height;
        packed.rows.extend(rows.into_iter().map(|mut row| {
            row.y += top;
            for item in &mut row.items {
                item.asset_index += first_index;
            }
            row
        }));
        packed.height += height;
        first_index += group.dims.len() as u32;
    }
    packed
}

pub fn pack_run(dims: &[(u32, u32)], lens: &[u32], width: f32, cfg: LayoutConfig) -> PackedSection {
    let mut packed = pack_section(
        &[Group {
            heading: None,
            dims,
        }],
        width,
        cfg,
    );
    packed.starts = month_starts(&packed.rows, lens, packed.height);
    packed
}

fn month_starts(rows: &[LaidRow], lens: &[u32], height: f32) -> Vec<f32> {
    let total: u32 = lens.iter().sum();
    let mut next = 0;
    lens.iter()
        .map(|len| {
            let first = next;
            next += len;
            if first == 0 {
                return 0.0;
            }
            if first >= total {
                return height;
            }
            let row = rows.partition_point(|row| {
                row.items
                    .first()
                    .is_some_and(|item| item.asset_index <= first)
            });
            rows[..row].last().map_or(height, |row| row.y)
        })
        .collect()
}

pub fn compose(placed: &[Placed<'_>]) -> TimelineLayout {
    let mut layout = TimelineLayout::default();
    for place in placed {
        let y = layout.height;
        match place {
            Placed::Packed {
                section,
                model_offset,
            } => {
                layout.rows.extend(section.rows.iter().map(|row| {
                    LaidRow {
                        y: row.y + y,
                        h: row.h,
                        items: row
                            .items
                            .iter()
                            .map(|item| LaidItem {
                                asset_index: item.asset_index + model_offset,
                                ..*item
                            })
                            .collect(),
                    }
                }));
                layout
                    .headings
                    .extend(section.headings.iter().map(|heading| LaidHeading {
                        y: heading.y + y,
                        text: heading.text.clone(),
                    }));
                let ends = section.starts.iter().skip(1).chain([&section.height]);
                for (start, end) in section.starts.iter().zip(ends) {
                    layout.sections.push(LaidSection {
                        y: y + start,
                        h: end - start,
                        loaded: true,
                    });
                }
                layout.height += section.height;
            }
            Placed::Estimated { height } => {
                layout.sections.push(LaidSection {
                    y,
                    h: *height,
                    loaded: false,
                });
                layout.height += height;
            }
        }
    }
    layout
}

pub fn estimated_height(count: u32, width: f32, cfg: LayoutConfig, headings: DateHeadings) -> f32 {
    if count == 0 {
        return 0.0;
    }
    let row_h = (cfg.min_row_height + cfg.max_row_height) / 2.0;
    let per_row = ((width - 2.0 * cfg.gap) / (ESTIMATED_ASPECT * row_h))
        .floor()
        .max(1.0);
    let rows = (count as f32 / per_row).ceil();
    let heading_count = match headings {
        DateHeadings::Day => count.div_ceil(PHOTOS_PER_DAY),
        DateHeadings::Month => 1,
        DateHeadings::Off => 0,
    };
    rows * (row_h + cfg.gap) + heading_count as f32 * HEADING_HEIGHT
}

pub fn section_at(sections: &[LaidSection], y: f32) -> Option<usize> {
    let index = sections.partition_point(|section| section.y + section.h <= y);
    (index < sections.len()).then_some(index)
}

/// Where `top` must move so the same point of the same section stays at the top.
pub fn anchored_scroll(old: &[LaidSection], new: &[LaidSection], top: f32) -> f32 {
    section_at(old, top)
        .and_then(|i| Some(new.get(i)?.y + (top - old[i].y)))
        .unwrap_or(top)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> LayoutConfig {
        LayoutConfig::wide()
    }

    #[test]
    fn estimated_height_per_mode() {
        assert_eq!(
            estimated_height(10, 1000.0, cfg(), DateHeadings::Off),
            1350.0
        );
        assert_eq!(
            estimated_height(10, 1000.0, cfg(), DateHeadings::Month),
            1390.0
        );
        assert_eq!(
            estimated_height(10, 1000.0, cfg(), DateHeadings::Day),
            1430.0
        );
        assert_eq!(estimated_height(0, 1000.0, cfg(), DateHeadings::Day), 0.0);
    }

    #[test]
    fn headings_sit_above_their_rows() {
        let dims = [(4, 3), (4, 3)];
        let packed = pack_section(
            &[Group {
                heading: Some("Mon, Oct 26".into()),
                dims: &dims,
            }],
            1000.0,
            cfg(),
        );
        assert_eq!(packed.headings[0].y, 0.0);
        assert_eq!(packed.rows[0].y, HEADING_HEIGHT);
    }

    #[test]
    fn compose_offsets_rows_and_indices() {
        let dims = [(4, 3)];
        let packed = pack_section(
            &[Group {
                heading: None,
                dims: &dims,
            }],
            1000.0,
            cfg(),
        );
        let layout = compose(&[
            Placed::Estimated { height: 500.0 },
            Placed::Packed {
                section: &packed,
                model_offset: 7,
            },
        ]);
        assert_eq!(layout.sections[1].y, 500.0);
        assert_eq!(layout.rows[0].y, 500.0);
        assert_eq!(layout.rows[0].items[0].asset_index, 7);
        assert!(!layout.sections[0].loaded && layout.sections[1].loaded);
    }

    fn row_width(row: &LaidRow) -> f32 {
        row.items.iter().map(|item| item.w).sum()
    }

    #[test]
    fn months_without_headings_flow_into_full_rows() {
        let dims = [(4, 3); 10];
        let run = pack_run(&dims, &[5, 5], 1000.0, cfg());
        let (last, full) = run.rows.split_last().unwrap();
        assert!(full.iter().all(|row| (row_width(row) - 1000.0).abs() < 0.5));
        assert_eq!(last.items.last().unwrap().asset_index, 9);
        let alone = pack_run(&dims[..5], &[5], 1000.0, cfg());
        assert!(row_width(alone.rows.last().unwrap()) < 999.0);
    }

    #[test]
    fn a_run_keeps_one_section_per_month() {
        let dims = [(4, 3); 10];
        let run = pack_run(&dims, &[5, 5], 1000.0, cfg());
        let layout = compose(&[
            Placed::Estimated { height: 100.0 },
            Placed::Packed {
                section: &run,
                model_offset: 3,
            },
        ]);
        assert_eq!(layout.sections.len(), 3);
        let shared_row = run
            .rows
            .iter()
            .find(|row| row.items.iter().any(|item| item.asset_index == 5));
        assert_eq!(layout.sections[2].y, 100.0 + shared_row.unwrap().y);
        assert_eq!(
            layout.sections[1].y + layout.sections[1].h,
            layout.sections[2].y
        );
        assert_eq!(layout.sections[2].y + layout.sections[2].h, layout.height);
        assert_eq!(layout.rows[0].items[0].asset_index, 3);
    }

    #[test]
    fn empty_months_in_a_run_take_no_space() {
        let dims = [(4, 3); 6];
        let run = pack_run(&dims, &[3, 0, 3, 0], 1000.0, cfg());
        let layout = compose(&[Placed::Packed {
            section: &run,
            model_offset: 0,
        }]);
        let heights: Vec<f32> = layout.sections.iter().map(|s| s.h).collect();
        assert_eq!(layout.sections.len(), 4);
        assert_eq!((heights[1], heights[3]), (0.0, 0.0));
        assert_eq!(heights.iter().sum::<f32>(), layout.height);
    }

    #[test]
    fn anchored_scroll_keeps_the_section_under_the_top() {
        let s = |y, h| LaidSection { y, h, loaded: true };
        let old = [s(0.0, 100.0), s(100.0, 500.0), s(600.0, 300.0)];
        let new = [s(0.0, 300.0), s(300.0, 500.0), s(800.0, 300.0)];
        assert_eq!(anchored_scroll(&old, &new, 700.0), 900.0);
        assert_eq!(anchored_scroll(&old, &new, 50.0), 50.0);
    }
}
