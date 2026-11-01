// The Energy Cube's screen (support module `crate::game::mek_ui` of the
// `mekanism` native bundle), drawn like Mekanism 10.7's `GuiEnergyCube` with
// Mekanism's own GUI textures (`mekanism:gui/...`, which `mekanism-content`
// copies from the carry): its panel, the wide energy gauge, the discharge (-)
// and charge (+) slots, the side configuration and energy tabs, and the side
// configuration (`GuiSideConfiguration`) with its energy and item pages.
//
// Mekanism draws the side configuration as a floating 156x135 window; here
// the game draws a screen's items over everything the mod draws, so it is a
// page over the cube's upper half, between its two slots and above the
// inventory, with the same buttons, textures and behaviour. Which page shows
// is the screen's own (Mekanism's windows are client-side too): page buttons
// still go to the server, which ignores them, so the press is taken.

use super::{Game, native_hooks::NativeScreen};
use horizon_client::hud::HudBatch;
use mekanism_energy::{
    config::EnergyConfig,
    display,
    menu::{Button, Kind, View},
    sides::{DataType, RelativeSide},
    tier::CubeTier,
};
use std::cell::{Cell, RefCell};

const TITLE: u32 = 0x404040;
const SCREEN_TEXT: u32 = 0x3CFE9A;

thread_local! {
    /// 0: the cube; 1: energy side configuration; 2: item side configuration.
    static PAGE: Cell<u8> = const { Cell::new(0) };
    /// The settings this side reads for the unit it shows (`energyType`).
    static SETTINGS: RefCell<Option<EnergyConfig>> = const { RefCell::new(None) };
}

fn settings() -> EnergyConfig {
    SETTINGS.with(|s| {
        s.borrow_mut()
            .get_or_insert_with(|| Game::native_config_dir().map(|d| EnergyConfig::load(&d)).unwrap_or_default())
            .clone()
    })
}

/// `native_screen`: `GuiEnergyCube`'s size, its two slots, and the labels it
/// draws itself. A screen opening starts on the cube's page.
pub(super) fn screen(block: &str, screen: &mut Option<NativeScreen>) {
    if screen.is_some() || CubeTier::of_block(block).is_none() {
        return;
    }
    PAGE.with(|p| p.set(0));
    *screen = Some(NativeScreen {
        width: 176.0,
        height: 166.0,
        slots: vec![Some([17.0, 35.0]), Some([143.0, 35.0]), None, None, None],
        inventory_y: 84.0,
        inventory_x: 8.0,
        own_labels: true,
    });
}

// ---- Layout -----------------------------------------------------------------

type Rect = [f32; 4];

fn inside(r: Rect, at: [f32; 2]) -> bool {
    at[0] >= r[0] && at[0] < r[0] + r[2] && at[1] >= r[1] && at[1] < r[1] + r[3]
}

const CONFIG_TAB: Rect = [-26.0, 6.0, 26.0, 26.0];
const CONFIG_BUTTON: Rect = [-21.0, 10.0, 18.0, 18.0];
const ENERGY_TAB: Rect = [-26.0, 137.0, 26.0, 26.0];
const GAUGE: Rect = [55.0, 18.0, 66.0, 50.0];
/// The side configuration page.
const WINDOW: Rect = [36.0, 2.0, 104.0, 81.0];
const CLOSE: Rect = [40.0, 6.0, 8.0, 8.0];
const EJECT_SCREEN: Rect = [40.0, 17.0, 78.0, 12.0];
const EJECT: Rect = [122.0, 16.0, 14.0, 14.0];
const CLEAR: Rect = [122.0, 65.0, 14.0, 14.0];
const ENERGY_PAGE: Rect = [40.0, 33.0, 18.0, 18.0];
const ITEMS_PAGE: Rect = [40.0, 54.0, 18.0, 18.0];

/// `GuiSideConfiguration`'s cross of side buttons (top; left, front, right;
/// back, bottom), 16 pixels each here.
fn side_button(side: RelativeSide) -> Rect {
    let (col, row) = match side {
        RelativeSide::Top => (1.0, 0.0),
        RelativeSide::Left => (0.0, 1.0),
        RelativeSide::Front => (1.0, 1.0),
        RelativeSide::Right => (2.0, 1.0),
        RelativeSide::Back => (0.0, 2.0),
        RelativeSide::Bottom => (1.0, 2.0),
    };
    [70.0 + 17.0 * col, 31.0 + 17.0 * row, 16.0, 16.0]
}

fn kind_of(page: u8) -> Kind {
    if page == 2 { Kind::Items } else { Kind::Energy }
}

/// What is under `at` on `page`: the button it presses, and its tooltip.
enum Hit {
    Press(Button),
    Side(Kind, RelativeSide),
    Tip(Vec<String>),
}

fn hit(page: u8, view: &View, settings: &EnergyConfig, at: [f32; 2], right: bool) -> Option<Hit> {
    if page != 0 {
        let kind = kind_of(page);
        for side in RelativeSide::ALL {
            if inside(side_button(side), at) {
                return Some(Hit::Side(kind, side));
            }
        }
        let tip = |s: &str| Some(Hit::Tip(vec![s.to_owned()]));
        if inside(CLOSE, at) {
            return Some(Hit::Press(Button::Page(0)));
        }
        if inside(EJECT, at) {
            return if right { tip("Auto-eject") } else { Some(Hit::Press(Button::Eject(kind))) };
        }
        if inside(CLEAR, at) {
            return Some(Hit::Press(Button::Clear { kind, forward: !right }));
        }
        if inside(ENERGY_PAGE, at) {
            return Some(Hit::Press(Button::Page(1)));
        }
        if inside(ITEMS_PAGE, at) {
            return Some(Hit::Press(Button::Page(2)));
        }
        if inside(WINDOW, at) {
            return None;
        }
    }
    if inside(CONFIG_BUTTON, at) {
        return Some(Hit::Press(Button::Page(if page == 0 { 1 } else { 0 })));
    }
    if inside(ENERGY_TAB, at) {
        return Some(Hit::Tip(vec![
            format!("Input: {}/t", display::energy(view.input, settings)),
            format!("Max Output: {}/t", display::energy(view.output, settings)),
        ]));
    }
    if page == 0 && inside(GAUGE, at) {
        return Some(Hit::Tip(vec![display::fraction(view.stored, view.capacity, settings)]));
    }
    None
}

fn view_of(data: &[i32]) -> View {
    View::unpack(data).unwrap_or(View {
        stored: 0,
        capacity: 0,
        input: 0,
        output: 0,
        energy: [DataType::None; 6],
        eject: false,
        items: [DataType::None; 6],
        items_eject: false,
    })
}

// ---- Clicks and tooltips ----------------------------------------------------

/// `native_screen_click`.
pub(super) fn click(block: &str, data: &[i32], at: [f32; 2], right: bool, button: &mut Option<i32>) {
    if button.is_some() || CubeTier::of_block(block).is_none() {
        return;
    }
    let page = PAGE.with(Cell::get);
    let pressed = match hit(page, &view_of(data), &settings(), at, right) {
        Some(Hit::Press(b)) => b,
        Some(Hit::Side(kind, side)) => Button::Side { kind, side: side.index(), forward: !right },
        _ => return,
    };
    if let Button::Page(p) = pressed {
        PAGE.with(|c| c.set(p));
    }
    *button = Some(pressed.id());
}

/// A side's type in its `EnumColor`, as near as legacy colour codes come
/// (Mekanism's dark red and blue are lighter than `§4` and `§1`).
fn type_name(t: DataType) -> &'static str {
    match t {
        DataType::Input => "§cInput",
        DataType::Output => "§9Output",
        DataType::InputOutput => "§5Input/Output",
        _ => "§7None",
    }
}

fn side_name(side: RelativeSide) -> &'static str {
    match side {
        RelativeSide::Front => "Front",
        RelativeSide::Left => "Left",
        RelativeSide::Right => "Right",
        RelativeSide::Back => "Back",
        RelativeSide::Top => "Top",
        RelativeSide::Bottom => "Bottom",
    }
}

/// `native_screen_tooltip`.
pub(super) fn tooltip(block: &str, data: &[i32], at: [f32; 2], lines: &mut Vec<String>) {
    if CubeTier::of_block(block).is_none() {
        return;
    }
    let view = view_of(data);
    let page = PAGE.with(Cell::get);
    if inside(CONFIG_BUTTON, at) {
        lines.push("Side Configuration".into());
        return;
    }
    match hit(page, &view, &settings(), at, false) {
        Some(Hit::Tip(tip)) => lines.extend(tip),
        Some(Hit::Side(kind, side)) => {
            let sides = if kind == Kind::Items { view.items } else { view.energy };
            lines.push(side_name(side).into());
            lines.push(type_name(sides[side.index()]).into());
        }
        Some(Hit::Press(b)) => lines.push(
            match b {
                Button::Page(0) => "Close",
                Button::Page(1) if page == 0 => "Side Configuration",
                Button::Page(1) => "Energy",
                Button::Page(_) => "Items",
                Button::Eject(_) => "Auto-eject",
                Button::Clear { .. } => "Clear Sides",
                Button::Side { .. } => "",
            }
            .into(),
        ),
        None => {}
    }
}

// ---- Drawing ----------------------------------------------------------------

struct Pen<'a, 'g> {
    game: &'a Game<'g>,
    batch: &'a mut HudBatch,
    origin: [f32; 2],
    cursor: [f32; 2],
}

impl Pen<'_, '_> {
    fn sprite(&mut self, name: &str, src: Rect, dst: Rect) {
        let [x, y, w, h] = dst;
        let at = [self.origin[0] + x, self.origin[1] + y, w, h];
        self.game.native_sprite(self.batch, &format!("mekanism:gui/{name}"), src, at);
    }

    fn whole(&mut self, name: &str, size: [f32; 2], dst: Rect) {
        self.sprite(name, [0.0, 0.0, size[0], size[1]], dst);
    }

    /// `blitNineSlicedSized`: the corners as they are, edges and middle
    /// stretched; corners shrink to half of a smaller target.
    fn nine(&mut self, name: &str, src: Rect, side: [f32; 2], dst: Rect) {
        let cw = side[0].min(dst[2] / 2.0).floor();
        let ch = side[1].min(dst[3] / 2.0).floor();
        let cols = [
            (src[0], cw, dst[0], cw),
            (src[0] + side[0], src[2] - 2.0 * side[0], dst[0] + cw, dst[2] - 2.0 * cw),
            (src[0] + src[2] - cw, cw, dst[0] + dst[2] - cw, cw),
        ];
        let rows = [
            (src[1], ch, dst[1], ch),
            (src[1] + side[1], src[3] - 2.0 * side[1], dst[1] + ch, dst[3] - 2.0 * ch),
            (src[1] + src[3] - ch, ch, dst[1] + dst[3] - ch, ch),
        ];
        for (sy, sh, dy, dh) in rows {
            for (sx, sw, dx, dw) in cols {
                if dw > 0.0 && dh > 0.0 && sw > 0.0 && sh > 0.0 {
                    self.sprite(name, [sx, sy, sw, sh], [dx, dy, dw, dh]);
                }
            }
        }
    }

    /// `drawButton`: `button.png`'s inactive, normal or hovered row.
    fn button(&mut self, r: Rect, active: bool) {
        let row = if !active {
            0.0
        } else if inside(r, self.cursor) {
            2.0
        } else {
            1.0
        };
        self.nine("button", [0.0, row * 20.0, 200.0, 20.0], [20.0, 4.0], r);
    }

    fn image_button(&mut self, icon: &str, icon_size: f32, r: Rect, active: bool) {
        self.button(r, active);
        self.whole(icon, [icon_size, icon_size], r);
    }

    fn fill(&mut self, r: Rect, argb: u32) {
        let at = [self.origin[0] + r[0], self.origin[1] + r[1], r[2], r[3]];
        self.game.native_fill(self.batch, at, argb);
    }

    fn text(&mut self, text: &str, at: [f32; 2], rgb: u32) {
        let at = [self.origin[0] + at[0], self.origin[1] + at[1]];
        self.game.native_text(self.batch, text, at, rgb, false);
    }

    fn centered(&mut self, text: &str, left: f32, width: f32, y: f32, rgb: u32) {
        let w = self.game.native_text_width(text);
        self.text(text, [left + ((width - w) / 2.0).floor(), y], rgb);
    }

    /// `GuiSideHolder`: a tab's holder, 26 wide.
    fn holder(&mut self, r: Rect, left: bool) {
        let name = if left { "holder_left" } else { "holder_right" };
        self.nine(name, [0.0, 0.0, 26.0, 9.0], [4.0, 4.0], r);
    }

    fn slot(&mut self, kind: &str, overlay: Option<&str>, x: f32, y: f32) {
        self.whole(&format!("slot/{kind}"), [18.0, 18.0], [x, y, 18.0, 18.0]);
        if let Some(o) = overlay {
            self.whole(&format!("slot/{o}"), [18.0, 18.0], [x, y, 18.0, 18.0]);
        }
    }
}

/// `EnumColor` of a side's type (`DataType.getColor`), lightened as
/// `BasicColorButton` does, laid over the button.
fn side_color(t: DataType) -> u32 {
    match t {
        DataType::Input => 0x80D51A30,
        DataType::Output => 0x804A7FE0,
        DataType::InputOutput => 0x80B576EE,
        _ => 0x60E6E6E6,
    }
}

/// `native_screen_draw`.
pub(super) fn draw(game: &Game<'_>, block: &str, data: &[i32], batch: &mut HudBatch, origin: [f32; 2]) {
    let Some(tier) = CubeTier::of_block(block) else {
        return;
    };
    let view = view_of(data);
    let page = PAGE.with(Cell::get);
    let cursor = game.native_cursor();
    let cursor = [cursor[0] - origin[0], cursor[1] - origin[1]];
    let mut pen = Pen { game, batch, origin, cursor };

    // GuiMekanism's background and the tabs beside it.
    pen.nine("base", [0.0, 0.0, 256.0, 256.0], [4.0, 4.0], [0.0, 0.0, 176.0, 166.0]);
    pen.holder(CONFIG_TAB, true);
    pen.image_button("configuration", 18.0, CONFIG_BUTTON, true);
    let unit = if settings().show_fe { "fe" } else { "j" };
    pen.whole(&format!("tabs/energy_info_{unit}"), [26.0, 26.0], ENERGY_TAB);

    // The wide energy gauge: its frame, the energy from the bottom, the scale.
    pen.nine("gauge/normal", [0.0, 0.0, 5.0, 5.0], [2.0, 2.0], GAUGE);
    let inner = GAUGE[3] - 2.0;
    let level = if view.stored == 0 || view.capacity == 0 {
        0.0
    } else if view.stored >= i64::MAX as u64 || tier == CubeTier::Creative {
        inner
    } else {
        (inner * (view.stored as f64 / view.capacity as f64) as f32).floor().max(1.0)
    };
    let mut y = GAUGE[1] + 1.0 + inner;
    let mut left = level;
    while left > 0.0 {
        let h = left.min(16.0);
        y -= h;
        for col in 0..4 {
            let x = GAUGE[0] + 1.0 + 16.0 * col as f32;
            pen.sprite("energy_fill", [0.0, 16.0 - h, 16.0, h], [x, y, 16.0, h]);
        }
        left -= h;
    }
    pen.whole("gauge/wide", [64.0, 48.0], [GAUGE[0] + 1.0, GAUGE[1] + 1.0, 64.0, 48.0]);

    // The slots: discharge (an OUTPUT item side) and charge (INPUT), then the inventory's.
    pen.slot("output", Some("overlay_minus"), 16.0, 34.0);
    pen.slot("input", Some("overlay_plus"), 142.0, 34.0);
    for row in 0..4 {
        let y = if row == 3 { 141.0 } else { 83.0 + 18.0 * row as f32 };
        for col in 0..9 {
            pen.slot("normal", None, 7.0 + 18.0 * col as f32, y);
        }
    }
    let title = game.native_screen_title().unwrap_or_default();
    pen.centered(&title, 0.0, 176.0, 6.0, TITLE);
    pen.text("Inventory", [8.0, 72.0], TITLE);
    if page == 0 {
        return;
    }

    // The side configuration.
    let kind = kind_of(page);
    let (sides, eject) = match kind {
        Kind::Energy => (view.energy, Some(view.eject)),
        Kind::Items => (view.items, None),
    };
    let [wx, wy, ww, wh] = WINDOW;
    pen.nine("shadow", [0.0, 0.0, 256.0, 256.0], [4.0, 4.0], [wx - 3.0, wy - 3.0, ww + 6.0, wh + 6.0]);
    pen.nine("base", [0.0, 0.0, 256.0, 256.0], [4.0, 4.0], WINDOW);
    pen.image_button("button/close", 12.0, CLOSE, true);
    let name = if kind == Kind::Energy { "Energy Config" } else { "Items Config" };
    pen.centered(name, wx, ww, wy + 4.0, TITLE);
    pen.nine("inner_screen", [0.0, 0.0, 256.0, 256.0], [32.0, 32.0], EJECT_SCREEN);
    let state = match eject {
        Some(on) => format!("Eject: {}", if on { "On" } else { "Off" }),
        None => "Can't Eject".to_owned(),
    };
    pen.centered(&state, EJECT_SCREEN[0], EJECT_SCREEN[2], EJECT_SCREEN[1] + 2.0, SCREEN_TEXT);
    pen.image_button("button/auto_eject", 14.0, EJECT, eject.is_some());
    pen.image_button("button/clear_sides", 14.0, CLEAR, true);
    for (icon, r, on) in [("energy", ENERGY_PAGE, kind == Kind::Energy), ("items", ITEMS_PAGE, kind == Kind::Items)] {
        pen.button(r, !on);
        pen.whole(icon, [18.0, 18.0], r);
    }
    for side in RelativeSide::ALL {
        let r = side_button(side);
        pen.button(r, true);
        pen.fill([r[0] + 1.0, r[1] + 1.0, r[2] - 2.0, r[3] - 2.0], side_color(sides[side.index()]));
    }
}
