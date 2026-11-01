//! AE2's `PriorityScreen` (`priority.json`) over the priority editor a
//! machine's priority tab opens. The editor's menu (`lan_send_priority`)
//! keeps its entries: 0..3 add 1, 10, 100 and 1000 (1, 16, 32 and 64 on a
//! shift-click), 4..7 take as much away, 8 goes back to the machine. The
//! screen shows them as `NumberEntryWidget`'s buttons and the back tab, and
//! the editor's text field as the widget's field; a press on a button is a
//! click on its entry.
use super::{
    Game,
    ae2_screens::{icon, inside},
};
use horizon_client::{
    ae2_screen_style::{Image, ScreenStyle},
    hud::{HudBatch, rgba},
};
use std::sync::OnceLock;

/// `NumberEntryWidget`'s buttons from the widget's top left, in entry
/// order: x, y and width (each 20 tall).
const BUTTONS: [(i32, i32, i32); 8] = [
    (0, 0, 22),
    (28, 0, 28),
    (62, 0, 32),
    (100, 0, 38),
    (0, 42, 22),
    (28, 42, 28),
    (62, 42, 32),
    (100, 42, 38),
];
/// The buttons' steps, and with shift or control down (`STEPS_64`).
const STEPS_1000: [i32; 4] = [1, 10, 100, 1000];
const STEPS_64: [i32; 4] = [1, 16, 32, 64];
/// The menu entry the back tab presses.
const BACK: usize = 8;
/// `ValidationIcon` from the widget's top left.
const INVALID_AT: (i32, i32) = (104, 27);
/// `AETextField.PADDING`, and the room it keeps for the cursor (`width("_")`).
const FIELD_PADDING: i32 = 2;
const CURSOR_ROOM: i32 = 6;

fn widget(style: &ScreenStyle, name: &str) -> Option<[i32; 4]> {
    let (w, h) = style.size();
    let s = style.widgets.get(name)?;
    let (x, y) = s.position.resolve(w, h);
    Some([x, y, s.width, s.height])
}

/// The help button, alone on the left toolbar.
fn help(style: &ScreenStyle) -> [i32; 4] {
    let (w, h) = style.size();
    let (x, y) = style.widgets.get("verticalToolbar").map_or((3, 1), |s| s.position.resolve(w, h));
    [x - 2 - 16, y + 2, 16, 16]
}

/// Each button's entry and box.
fn buttons(style: &ScreenStyle) -> impl Iterator<Item = (usize, [i32; 4])> {
    let [x, y, _, _] = widget(style, "priority").unwrap_or([20, 30, 138, 62]);
    BUTTONS.into_iter().enumerate().map(move |(entry, (bx, by, w))| (entry, [x + bx, y + by, w, 20]))
}

fn back(style: &ScreenStyle) -> [i32; 4] {
    let [x, y, _, _] = widget(style, "back").unwrap_or([152, -5, 20, 20]);
    [x, y, 20, 20]
}

impl Game<'_> {
    /// Whether the open menu is an AE2 machine's priority editor, which
    /// this screen shows.
    pub(super) fn ae2_priority_ui(&self) -> bool {
        self.remote.as_ref().is_some_and(|r| r.ae2_priority_screen && r.ae2_stock_spec.is_none())
            && self.open_native_screen_extra().is_some_and(|(_, extra)| extra.is_some())
    }

    fn ae2_priority_style(&self) -> Option<&'static ScreenStyle> {
        static STYLE: OnceLock<Option<ScreenStyle>> = OnceLock::new();
        STYLE
            .get_or_init(|| {
                let screens = self.options.resources.join("assets/ae2/screens");
                ScreenStyle::load(&screens, "priority.json")
                    .map_err(|error| log::warn!("AE2 priority screen style: {error}"))
                    .ok()
            })
            .as_ref()
    }

    /// The screen's size.
    pub(super) fn ae2_priority_size(&self) -> (f32, f32) {
        let (w, h) = self.ae2_priority_style().map_or((176, 125), ScreenStyle::size);
        (w as f32, h as f32)
    }

    /// The menu entry a press at `at` (from the screen's top left) clicks.
    pub(super) fn ae2_priority_entry_at(&self, at: [f32; 2]) -> Option<usize> {
        let style = self.ae2_priority_style()?;
        if inside(back(style), at) {
            return Some(BACK);
        }
        buttons(style).find(|(_, rect)| inside(*rect, at)).map(|(entry, _)| entry)
    }

    /// Whether `at` is off the screen and its toolbar and tab.
    pub(super) fn ae2_priority_outside(&self, at: [f32; 2]) -> bool {
        let (w, h) = self.ae2_priority_size();
        self.ae2_priority_entry_at(at).is_none()
            && !self.ae2_priority_style().is_some_and(|s| inside(help(s), at))
            && (at[0] < 0.0 || at[0] >= w || at[1] < 0.0 || at[1] >= h)
    }

    /// The text field: where a press focuses it, and where its text goes
    /// (`AETextField`, its padding in), from the screen's top left.
    pub(super) fn ae2_priority_field(&self) -> Option<([f32; 4], [f32; 4])> {
        if !self.ae2_priority_ui() {
            return None;
        }
        let [x, y, w, h] = widget(self.ae2_priority_style()?, "priorityInput")?;
        let text = [x + FIELD_PADDING, y + FIELD_PADDING, w - 2 * FIELD_PADDING - CURSOR_ROOM, h - 2 * FIELD_PADDING];
        Some(([x, y, w, h].map(|v| v as f32), text.map(|v| v as f32)))
    }

    /// A press at `at` the screen handles before the menu: the help button,
    /// and a right press on a button, which buttons ignore (true then). A
    /// left press on a button clicks; its entry takes the click.
    pub(super) fn ae2_priority_press(&mut self, at: [f32; 2], right: bool) -> bool {
        let Some(style) = self.ae2_priority_style().filter(|_| self.ae2_priority_ui()) else {
            return false;
        };
        if !right && inside(help(style), at) {
            let block = self.open_native_screen_extra().map(|(block, _)| block.to_owned());
            if let Some(block) = block {
                self.audio.play_button_click();
                self.ae2_open_guide(&block);
            }
            return true;
        }
        if self.ae2_priority_entry_at(at).is_some() {
            if right {
                return true;
            }
            self.audio.play_button_click();
        }
        false
    }

    /// Whether the field's text is a priority (`NumberEntryWidget.validate`).
    fn ae2_priority_valid(&self, text: &str) -> bool {
        super::ae2_number_entry::number_value(text, 1, i64::from(i32::MIN), i64::MAX, '.')
            .is_some_and(|n| i32::try_from(n).is_ok())
    }

    pub(super) fn ae2_draw_priority(&self, batch: &mut HudBatch, ox: f32, oy: f32) {
        let Some(style) = self.ae2_priority_style() else {
            return;
        };
        let atlas = &self.gui.atlas;
        let mouse = [self.cursor.0 - ox, self.cursor.1 - oy];
        if let Some(background) = style.background.as_ref() {
            let (w, h) = background.size();
            let [sx, sy] = background.src.map_or([0, 0], |[x, y, _, _]| [x, y]);
            self.ae2_blit(batch, background, [sx, sy, w, h], ox, oy);
        }
        let help = help(style);
        self.ae2_draw_toolbar(batch, ox, oy, help, help);
        self.ae2_draw_icon_button(batch, ox, oy, help, icon::HELP, mouse);
        self.ae2_draw_tab(batch, ox, oy, back(style), icon::BACK);
        let steps = if self.has_shift_down() || self.has_control_down() { STEPS_64 } else { STEPS_1000 };
        for (entry, rect @ [x, y, w, h]) in buttons(style) {
            let hover = inside(rect, mouse);
            // AE2Button: its sprite, a 200 x 20 nine-slice with 3 pixel
            // borders, the label centred without a shadow, a pixel up
            // unless hovered.
            let sprite = if hover { "gui/button_highlighted" } else { "gui/button" };
            let image = Image { texture: sprite.into(), texture_size: (200, 20), src: None };
            self.ae2_blit(batch, &image, [0, 0, w - 3, h], ox + x as f32, oy + y as f32);
            self.ae2_blit(batch, &image, [197, 0, 3, h], ox + (x + w - 3) as f32, oy + y as f32);
            let sign = if entry < 4 { '+' } else { '-' };
            let label = format!("{sign}{}", steps[entry % 4]);
            let width = HudBatch::text_width(atlas, &label) as i32;
            let (min, max) = (x + 2, x + w - 2);
            let centre = ((min + max) / 2).clamp(min + width / 2, max - width / 2);
            let top = y + (h - 9) / 2 + 1 - if hover { 0 } else { 1 };
            let color = if hover { rgba(0x51, 0x74, 0x97, 255) } else { rgba(0xF2, 0xF2, 0xF2, 255) };
            batch.text(atlas, &label, ox + (centre - width / 2) as f32, oy + top as f32, color, false);
        }
        if let (Some((rect, text)), Some(mut field)) =
            (self.ae2_priority_field(), self.remote.as_ref().and_then(|r| r.ae2_search.clone()))
        {
            // AETextField: its background in three slices, the focused row
            // while focused.
            let row = if field.focused { 24 } else { 0 };
            let image = Image { texture: "guis/text_field".into(), texture_size: (128, 128), src: None };
            let (x, y, w) = (ox + rect[0], oy + rect[1], rect[2]);
            let middle = (w - 2.0).min(126.0);
            self.ae2_blit(batch, &image, [0, row, 1, 12], x, y);
            self.ae2_blit(batch, &image, [1, row, middle as i32, 12], x + 1.0, y);
            self.ae2_blit(batch, &image, [127, row, 1, 12], x + w - 1.0, y);
            let valid = self.ae2_priority_valid(&field.value);
            field.bordered = false;
            field.text_color = super::ae2_screens::argb(style.color(if valid { "TEXTFIELD_TEXT" } else { "TEXTFIELD_ERROR" }));
            [field.x, field.y, field.width, field.height] = [ox + text[0], oy + text[1], text[2], text[3]];
            field.draw(batch, atlas, None);
            if !valid {
                let [px, py, _, _] = widget(style, "priority").unwrap_or([20, 30, 138, 62]);
                self.ae2_icon(batch, icon::INVALID, (16, 16), ox + (px + INVALID_AT.0) as f32, oy + (py + INVALID_AT.1) as f32);
            }
        }
        self.ae2_draw_texts(batch, ox, oy, style, &[]);
    }

    /// The tooltip under the cursor: the help button's, and the back tab's
    /// (the machine's name, as `AESubScreen.addBackButton` labels it).
    pub(super) fn ae2_priority_tooltip(&self, batch: &mut HudBatch) {
        let Some(style) = self.ae2_priority_style() else {
            return;
        };
        let (ox, oy) = self.inventory_origin(batch);
        let at = [self.cursor.0 - ox, self.cursor.1 - oy];
        let mut lines = Vec::new();
        if inside(help(style), at) {
            self.ae2_help_tooltip(&mut lines);
        } else if inside(back(style), at)
            && let Some((block, _)) = self.open_native_screen_extra()
        {
            let key = format!("block.{}", block.replace(':', "."));
            lines.push(self.translate(&key, block));
        }
        if lines.is_empty() {
            return;
        }
        let lines: Vec<(horizon_client::text::Text, u32)> =
            lines.into_iter().map(|l| (horizon_client::text::Text::literal(l), u32::MAX)).collect();
        self.draw_tooltip_lines(batch, &lines, self.cursor.0, self.cursor.1);
    }

    /// The priority screen for the control socket's `screen`: its buttons'
    /// entries and boxes in GUI pixels, the back tab, the help button, the
    /// field and its text.
    pub(super) fn ae2_priority_agent_value(&self, origin: (f32, f32)) -> Option<serde_json::Value> {
        if !self.ae2_priority_ui() {
            return None;
        }
        let style = self.ae2_priority_style()?;
        let rect = |[x, y, w, h]: [i32; 4]| [origin.0 + x as f32, origin.1 + y as f32, w as f32, h as f32];
        let buttons: Vec<serde_json::Value> =
            buttons(style).map(|(entry, r)| serde_json::json!({"entry": entry, "rect": rect(r)})).collect();
        let text = self.remote.as_ref().and_then(|r| r.ae2_search.as_ref()).map(|f| f.value.clone());
        Some(serde_json::json!({
            "style": "priority.json",
            "buttons": buttons,
            "back": rect(back(style)),
            "help": rect(help(style)),
            "field": self.ae2_priority_field().map(|(r, _)| [origin.0 + r[0], origin.1 + r[1], r[2], r[3]]),
            "text": text,
            "valid": text.as_deref().is_some_and(|t| self.ae2_priority_valid(t)),
        }))
    }
}
