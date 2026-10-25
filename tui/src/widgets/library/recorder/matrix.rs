//! The TKL matrix and its geometry. One pure `layout` feeds both the
//! render path and mouse hit-testing, so the two never disagree.

use ratatui::layout::Rect;

use crate::widgets::util::{self, OverlayChrome};

/// Total cells the keyboard matrix needs: main block, gap, right cluster.
pub(crate) const TKL_WIDTH: u16 = 77;
const MAIN_WIDTH: u16 = 57;
const CLUSTER_GAP: u16 = 3;
const CANCEL_WIDTH: u16 = 12;
const DONE_WIDTH: u16 = 8;
const CHIP_GAP: u16 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cell {
    Key {
        id: &'static str,
        label: &'static str,
        width: u16,
    },
    Gap(u16),
}

const fn key(id: &'static str, label: &'static str, width: u16) -> Cell {
    Cell::Key { id, label, width }
}

const fn gap(width: u16) -> Cell {
    Cell::Gap(width)
}

pub(crate) struct Row {
    pub(crate) main: &'static [Cell],
    /// Blank cells between the main block and this row's cluster keys.
    pub(crate) cluster_offset: u16,
    pub(crate) cluster: &'static [Cell],
}

const F_ROW: &[Cell] = &[
    key("esc", "Esc", 3),
    gap(2),
    key("f1", "F1", 3),
    gap(1),
    key("f2", "F2", 3),
    gap(1),
    key("f3", "F3", 3),
    gap(1),
    key("f4", "F4", 3),
    gap(2),
    key("f5", "F5", 3),
    gap(1),
    key("f6", "F6", 3),
    gap(1),
    key("f7", "F7", 3),
    gap(1),
    key("f8", "F8", 3),
    gap(2),
    key("f9", "F9", 3),
    gap(1),
    key("f10", "F10", 4),
    gap(1),
    key("f11", "F11", 4),
    gap(1),
    key("f12", "F12", 4),
];

const NUMBER_ROW: &[Cell] = &[
    key("`", "`", 3),
    gap(1),
    key("1", "1", 3),
    gap(1),
    key("2", "2", 3),
    gap(1),
    key("3", "3", 3),
    gap(1),
    key("4", "4", 3),
    gap(1),
    key("5", "5", 3),
    gap(1),
    key("6", "6", 3),
    gap(1),
    key("7", "7", 3),
    gap(1),
    key("8", "8", 3),
    gap(1),
    key("9", "9", 3),
    gap(1),
    key("0", "0", 3),
    gap(1),
    key("-", "-", 3),
    gap(1),
    key("=", "=", 3),
    gap(1),
    key("bksp", "Bksp", 5),
];

const QWERTY_ROW: &[Cell] = &[
    key("tab", "Tab", 5),
    gap(1),
    key("q", "Q", 3),
    gap(1),
    key("w", "W", 3),
    gap(1),
    key("e", "E", 3),
    gap(1),
    key("r", "R", 3),
    gap(1),
    key("t", "T", 3),
    gap(1),
    key("y", "Y", 3),
    gap(1),
    key("u", "U", 3),
    gap(1),
    key("i", "I", 3),
    gap(1),
    key("o", "O", 3),
    gap(1),
    key("p", "P", 3),
    gap(1),
    key("[", "[", 3),
    gap(1),
    key("]", "]", 3),
    gap(1),
    key("\\", "\\", 3),
];

const HOME_ROW: &[Cell] = &[
    key("caps", "Caps", 6),
    gap(1),
    key("a", "A", 3),
    gap(1),
    key("s", "S", 3),
    gap(1),
    key("d", "D", 3),
    gap(1),
    key("f", "F", 3),
    gap(1),
    key("g", "G", 3),
    gap(1),
    key("h", "H", 3),
    gap(1),
    key("j", "J", 3),
    gap(1),
    key("k", "K", 3),
    gap(1),
    key("l", "L", 3),
    gap(1),
    key(";", ";", 3),
    gap(1),
    key("'", "'", 3),
    gap(1),
    key("enter", "Enter", 6),
];

const SHIFT_ROW: &[Cell] = &[
    key("lshift", "Shift", 8),
    gap(1),
    key("z", "Z", 3),
    gap(1),
    key("x", "X", 3),
    gap(1),
    key("c", "C", 3),
    gap(1),
    key("v", "V", 3),
    gap(1),
    key("b", "B", 3),
    gap(1),
    key("n", "N", 3),
    gap(1),
    key("m", "M", 3),
    gap(1),
    key(",", ",", 3),
    gap(1),
    key(".", ".", 3),
    gap(1),
    key("/", "/", 3),
    gap(1),
    key("rshift", "Shift", 8),
];

const BOTTOM_ROW: &[Cell] = &[
    key("lctrl", "Ctrl", 5),
    gap(1),
    key("lmeta", "Win", 5),
    gap(1),
    key("lalt", "Alt", 5),
    gap(1),
    key("space", "", 18),
    gap(1),
    key("ralt", "Alt", 5),
    gap(1),
    key("rmeta", "Win", 5),
    gap(1),
    key("menu", "Menu", 4),
    gap(1),
    key("rctrl", "Ctl", 3),
];

const PRINT_CLUSTER: &[Cell] = &[
    key("prtsc", "PrtSc", 5),
    gap(1),
    key("scrlk", "ScrLk", 5),
    gap(1),
    key("pause", "Pause", 5),
];

const NAV_TOP: &[Cell] = &[
    key("ins", "Ins", 5),
    gap(1),
    key("home", "Home", 5),
    gap(1),
    key("pgup", "PgUp", 5),
];

const NAV_BOTTOM: &[Cell] = &[
    key("del", "Del", 5),
    gap(1),
    key("end", "End", 5),
    gap(1),
    key("pgdn", "PgDn", 5),
];

const ARROW_UP: &[Cell] = &[key("up", "↑", 5)];
const ARROW_BOTTOM: &[Cell] = &[
    key("left", "←", 5),
    gap(1),
    key("down", "↓", 5),
    gap(1),
    key("right", "→", 5),
];

/// The complete TKL matrix: six rows of main-block cells plus the right
/// cluster. Every row's main block sums to `MAIN_WIDTH`; the cluster
/// starts `CLUSTER_GAP` cells past it.
pub(crate) const ROWS: [Row; 6] = [
    Row {
        main: F_ROW,
        cluster_offset: 0,
        cluster: PRINT_CLUSTER,
    },
    Row {
        main: NUMBER_ROW,
        cluster_offset: 0,
        cluster: NAV_TOP,
    },
    Row {
        main: QWERTY_ROW,
        cluster_offset: 0,
        cluster: NAV_BOTTOM,
    },
    Row {
        main: HOME_ROW,
        cluster_offset: 0,
        cluster: &[],
    },
    Row {
        main: SHIFT_ROW,
        // honey: six blank cells park ↑ directly above ↓.
        cluster_offset: 6,
        cluster: ARROW_UP,
    },
    Row {
        main: BOTTOM_ROW,
        cluster_offset: 0,
        cluster: ARROW_BOTTOM,
    },
];

/// Popup rect: 90% of the terminal in both axes, so the keyboard sits in
/// a panel rather than a cramped box.
pub(crate) fn popup(area: Rect) -> Rect {
    let width = (u32::from(area.width) * 9 / 10) as u16;
    let height = (u32::from(area.height) * 9 / 10) as u16;
    util::overlay_popup_sized(width, height, area)
}

pub(crate) struct Layout {
    pub(crate) content: Rect,
    pub(crate) instructions: Rect,
    pub(crate) cancel: Rect,
    pub(crate) done: Rect,
    pub(crate) divider_top: u16,
    pub(crate) keycaps: Vec<(&'static str, &'static str, Rect)>,
}

/// Every rect the overlay draws. The panel is 90% of the terminal, so the
/// keyboard stretches across the content width instead of leaving empty
/// gutters on either side.
pub(crate) fn layout(area: Rect) -> Layout {
    let chrome = OverlayChrome::new(popup(area));
    let content = chrome.content;
    let keycap_height = keycap_height(content.height);
    let right = content.x.saturating_add(content.width);
    let chips_x = right
        .saturating_sub(CANCEL_WIDTH + CHIP_GAP + DONE_WIDTH)
        .max(content.x);
    let cancel = Rect::new(chips_x, content.y, CANCEL_WIDTH, 1);
    let done = Rect::new(
        chips_x.saturating_add(CANCEL_WIDTH + CHIP_GAP),
        content.y,
        DONE_WIDTH,
        1,
    );
    let instructions = Rect::new(content.x, content.y, content.width, 2);
    let divider_top = content.y.saturating_add(3);
    let keycap_top = divider_top.saturating_add(2);

    let mut keycaps = Vec::new();
    for (index, row) in ROWS.iter().enumerate() {
        let y = keycap_top.saturating_add((index as u16).saturating_mul(keycap_height + 1));
        place_row(&mut keycaps, content, row.main, 0, y, keycap_height);
        let cluster_x = MAIN_WIDTH + CLUSTER_GAP + row.cluster_offset;
        place_row(
            &mut keycaps,
            content,
            row.cluster,
            cluster_x,
            y,
            keycap_height,
        );
    }

    Layout {
        content,
        instructions,
        cancel,
        done,
        divider_top,
        keycaps,
    }
}

/// Keycap rows for the content height: full caps when the keyboard fits,
/// flat caps otherwise.
const fn keycap_height(content_height: u16) -> u16 {
    // honey: five rows of header, divider, and gap sit above the matrix.
    if content_height >= 28 { 3 } else { 1 }
}

/// Stretch a base matrix column to the drawn width. With the keyboard
/// filling the panel, letters and word keys gain the padding they need.
fn scale_x(x: u16, width: u16) -> u16 {
    let scaled = u32::from(x) * u32::from(width) + u32::from(TKL_WIDTH) / 2;
    (scaled / u32::from(TKL_WIDTH)) as u16
}

fn place_row(
    out: &mut Vec<(&'static str, &'static str, Rect)>,
    content: Rect,
    cells: &'static [Cell],
    start_x: u16,
    y: u16,
    height: u16,
) {
    let right = content.x.saturating_add(content.width);
    let width = TKL_WIDTH.max(content.width);
    let mut x = start_x;
    for cell in cells {
        match cell {
            Cell::Gap(cells) => x = x.saturating_add(*cells),
            Cell::Key {
                id,
                label,
                width: cells,
            } => {
                let start = content.x.saturating_add(scale_x(x, width));
                let end = content
                    .x
                    .saturating_add(scale_x(x.saturating_add(*cells), width));
                // honey: narrow terminals clip the far columns instead
                // of scrolling the keyboard.
                if start < right {
                    let cell_width = end.saturating_sub(start).min(right - start);
                    out.push((*id, *label, Rect::new(start, y, cell_width, height)));
                }
                x = x.saturating_add(*cells);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cells_width(cells: &[Cell]) -> u16 {
        cells
            .iter()
            .map(|cell| match cell {
                Cell::Key { width, .. } => *width,
                Cell::Gap(width) => *width,
            })
            .sum()
    }

    #[test]
    fn every_row_aligns_on_the_main_block_edge() {
        for (index, row) in ROWS.iter().enumerate() {
            assert_eq!(cells_width(row.main), MAIN_WIDTH, "row {index} main block");
            let cluster = row.cluster_offset + cells_width(row.cluster);
            assert!(
                cluster <= TKL_WIDTH - MAIN_WIDTH - CLUSTER_GAP,
                "row {index} cluster overflows: {cluster}"
            );
        }
    }

    #[test]
    fn cluster_rows_are_seventeen_cells() {
        for index in [0, 1, 2, 5] {
            assert_eq!(cells_width(ROWS[index].cluster), 17, "row {index} cluster");
        }
        // ↑ is parked directly above ↓, six cells into the cluster.
        assert_eq!(ROWS[4].cluster_offset, 6);
        assert_eq!(cells_width(ROWS[4].cluster), 5);
    }

    #[test]
    fn popup_takes_ninety_percent_of_the_terminal() {
        let area = Rect::new(0, 0, 100, 45);
        let popup = popup(area);
        assert_eq!(popup.width, 90);
        assert_eq!(popup.height, 40);
        // Centered: five cells of terminal on each side, two rows above
        // and three below (integer centering).
        assert_eq!(popup.x, 5);
        assert_eq!(popup.y, 2);
    }

    #[test]
    fn layout_draws_the_whole_keyboard() {
        let layout = layout(Rect::new(0, 0, 100, 45));
        let left = layout
            .keycaps
            .iter()
            .map(|(_, _, rect)| rect.x)
            .min()
            .expect("keycaps");
        let right = layout
            .keycaps
            .iter()
            .map(|(_, _, rect)| rect.x + rect.width)
            .max()
            .expect("keycaps");
        // The keyboard stretches across the whole panel, so no gutters
        // sit beside it.
        assert_eq!(left, layout.content.x);
        assert_eq!(right, layout.content.x + layout.content.width);
        assert!(right - left > TKL_WIDTH);
    }

    #[test]
    fn keyboard_sits_directly_under_the_instructions() {
        let layout = layout(Rect::new(0, 0, 100, 60));
        let top = layout
            .keycaps
            .iter()
            .map(|(_, _, rect)| rect.y)
            .min()
            .expect("keycaps");
        assert_eq!(layout.instructions.y, layout.content.y);
        assert_eq!(top, layout.divider_top + 2);
    }

    #[test]
    fn arrow_column_lines_up() {
        let layout = layout(Rect::new(0, 0, 100, 45));
        let rect_of = |id: &str| {
            layout
                .keycaps
                .iter()
                .find(|(key, _, _)| *key == id)
                .map(|(_, _, rect)| *rect)
                .expect(id)
        };
        assert_eq!(rect_of("up").x, rect_of("down").x);
    }

    #[test]
    fn short_terminals_flatten_the_keycaps() {
        let tall = layout(Rect::new(0, 0, 100, 45));
        assert!(tall.keycaps.iter().all(|(_, _, rect)| rect.height == 3));
        let short = layout(Rect::new(0, 0, 100, 30));
        assert!(short.keycaps.iter().all(|(_, _, rect)| rect.height == 1));
    }

    #[test]
    fn short_terminals_still_fit_every_row() {
        let layout = layout(Rect::new(0, 0, 100, 30));
        let bottom = layout
            .keycaps
            .iter()
            .map(|(_, _, rect)| rect.y + rect.height)
            .max()
            .expect("keycaps");
        assert!(bottom <= layout.content.y + layout.content.height);
    }

    #[test]
    fn mid_height_terminals_keep_three_row_keycaps() {
        let layout = layout(Rect::new(0, 0, 100, 40));
        assert!(layout.keycaps.iter().all(|(_, _, rect)| rect.height == 3));
        let bottom = layout
            .keycaps
            .iter()
            .map(|(_, _, rect)| rect.y + rect.height)
            .max()
            .expect("keycaps");
        assert!(bottom <= layout.content.y + layout.content.height);
    }

    #[test]
    fn narrow_terminals_clip_instead_of_overflowing() {
        let layout = layout(Rect::new(0, 0, 60, 45));
        let right = layout.content.x + layout.content.width;
        assert!(
            layout
                .keycaps
                .iter()
                .all(|(_, _, rect)| rect.x + rect.width <= right)
        );
    }
}
