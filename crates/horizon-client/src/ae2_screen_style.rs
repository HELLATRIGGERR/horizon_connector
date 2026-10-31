//! AE2's screen styles (`assets/ae2/screens/*.json`, AE2 19.2.18
//! `client/gui/style`): what a screen draws and where. `StyleManager`
//! merges a document over its `includes` in order, later layers winning, and
//! merges the `slots`, `text`, `palette`, `images` and `widgets` objects key
//! by key; `Position.resolve` measures from the left and top edges, else
//! from the right and bottom ones; `SlotGridLayout` spreads the slots of one
//! semantic.
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
};

/// `Position`: offsets from the screen's edges, as the style gives them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Position {
    pub left: Option<i32>,
    pub top: Option<i32>,
    pub right: Option<i32>,
    pub bottom: Option<i32>,
}

impl Position {
    fn read(v: &Value) -> Self {
        let int = |key: &str| v.get(key).and_then(Value::as_f64).map(|n| n as i32);
        Self { left: int("left"), top: int("top"), right: int("right"), bottom: int("bottom") }
    }

    /// `Position.resolve` in a screen of `width` by `height`: the point from
    /// its top left.
    pub fn resolve(&self, width: i32, height: i32) -> (i32, i32) {
        let x = self.left.or(self.right.map(|r| width - r)).unwrap_or(0);
        let y = self.top.or(self.bottom.map(|b| height - b)).unwrap_or(0);
        (x, y)
    }
}

/// `SlotGridLayout`: where the `index`th slot of a semantic goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grid {
    IoBusConfig,
    BreakAfter(i32),
    Horizontal,
    Vertical,
}

impl Grid {
    fn read(name: &str) -> Option<Self> {
        Some(match name {
            "IO_BUS_CONFIG" => Self::IoBusConfig,
            "BREAK_AFTER_9COLS" => Self::BreakAfter(9),
            "BREAK_AFTER_3COLS" => Self::BreakAfter(3),
            "BREAK_AFTER_2COLS" => Self::BreakAfter(2),
            "HORIZONTAL" => Self::Horizontal,
            "VERTICAL" => Self::Vertical,
            _ => return None,
        })
    }

    pub fn offset(self, index: usize) -> (i32, i32) {
        let i = index as i32;
        match self {
            Self::IoBusConfig => {
                const OFFSETS: [(i32, i32); 9] =
                    [(0, 0), (-18, 0), (18, 0), (0, -18), (0, 18), (-18, -18), (18, -18), (-18, 18), (18, 18)];
                OFFSETS[index % OFFSETS.len()]
            }
            Self::BreakAfter(cols) => (i % cols * 18, i / cols * 18),
            Self::Horizontal => (i * 18, 0),
            Self::Vertical => (0, i * 18),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SlotStyle {
    pub position: Position,
    pub grid: Option<Grid>,
    pub hidden: bool,
}

/// `Blitter` from a style: a region of a texture under `assets/ae2/textures/`.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    /// The texture's path below `textures/`, without `.png` (`guis/inscriber`).
    pub texture: String,
    /// The size the source rectangle is measured in (`textureWidth`, 256 by default).
    pub texture_size: (i32, i32),
    /// `[x, y, width, height]`; none means the whole texture.
    pub src: Option<[i32; 4]>,
}

impl Image {
    fn read(v: &Value) -> Option<Self> {
        let texture = v.get("texture")?.as_str()?;
        let texture = texture.strip_prefix("ae2:textures/").unwrap_or(texture);
        let size = |key: &str| v.get(key).and_then(Value::as_i64).map_or(256, |n| n as i32);
        let src = v.get("srcRect").and_then(Value::as_array).and_then(|r| {
            let n: Vec<i32> = r.iter().filter_map(Value::as_i64).map(|n| n as i32).collect();
            (n.len() == 4).then(|| [n[0], n[1], n[2], n[3]])
        });
        Some(Self {
            texture: texture.trim_end_matches(".png").to_owned(),
            texture_size: (size("textureWidth"), size("textureHeight")),
            src,
        })
    }

    /// The size it draws at when no destination size is given.
    pub fn size(&self) -> (i32, i32) {
        self.src.map_or(self.texture_size, |[_, _, w, h]| (w, h))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
}

/// `Text`: a label, its colour from the palette and where it goes.
#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    /// The text component as the style writes it (`{"translate": ...}`).
    pub text: Value,
    pub color: String,
    pub position: Position,
    pub align: Align,
    pub scale: f32,
    pub max_width: i32,
}

/// `WidgetStyle`: a widget's position and size.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WidgetStyle {
    pub position: Position,
    pub width: i32,
    pub height: i32,
    pub hide_edge: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScreenStyle {
    pub background: Option<Image>,
    pub slots: BTreeMap<String, SlotStyle>,
    pub text: BTreeMap<String, TextStyle>,
    /// `PaletteColor` name to `0xAARRGGBB`.
    pub palette: BTreeMap<String, u32>,
    pub images: BTreeMap<String, Image>,
    pub widgets: BTreeMap<String, WidgetStyle>,
}

/// The keys merged across layers rather than replaced (`combineLayers`).
const MERGED: [&str; 6] = ["slots", "text", "palette", "images", "terminalStyle", "widgets"];

impl ScreenStyle {
    /// Loads `screens/<path>` from `screens`, the folder of the styles.
    pub fn load(screens: &Path, path: &str) -> Result<Self, String> {
        let tree = merged(screens, path, &mut HashSet::new())?;
        Ok(Self::from_tree(&tree))
    }

    pub fn from_tree(tree: &Map<String, Value>) -> Self {
        let object = |key: &str| tree.get(key).and_then(Value::as_object);
        let mut style = Self { background: tree.get("background").and_then(Image::read), ..Self::default() };
        for (name, v) in object("palette").into_iter().flatten() {
            if let Some(color) = v.as_str().and_then(parse_color) {
                style.palette.insert(name.clone(), color);
            }
        }
        for (name, v) in object("slots").into_iter().flatten() {
            style.slots.insert(
                name.clone(),
                SlotStyle {
                    position: Position::read(v),
                    grid: v.get("grid").and_then(Value::as_str).and_then(Grid::read),
                    hidden: v.get("hidden").and_then(Value::as_bool).unwrap_or(false),
                },
            );
        }
        for (name, v) in object("text").into_iter().flatten() {
            let align = match v.get("align").and_then(Value::as_str) {
                Some("CENTER") => Align::Center,
                Some("RIGHT") => Align::Right,
                _ => Align::Left,
            };
            style.text.insert(
                name.clone(),
                TextStyle {
                    text: v.get("text").cloned().unwrap_or(Value::Null),
                    color: v.get("color").and_then(Value::as_str).unwrap_or("DEFAULT_TEXT_COLOR").to_owned(),
                    position: v.get("position").map(Position::read).unwrap_or_default(),
                    align,
                    scale: v.get("scale").and_then(Value::as_f64).unwrap_or(1.0) as f32,
                    max_width: v.get("maxWidth").and_then(Value::as_i64).unwrap_or(0) as i32,
                },
            );
        }
        for (name, v) in object("images").into_iter().flatten() {
            if let Some(image) = Image::read(v) {
                style.images.insert(name.clone(), image);
            }
        }
        for (name, v) in object("widgets").into_iter().flatten() {
            let int = |key: &str| v.get(key).and_then(Value::as_i64).unwrap_or(0) as i32;
            style.widgets.insert(
                name.clone(),
                WidgetStyle {
                    position: Position::read(v),
                    width: int("width"),
                    height: int("height"),
                    hide_edge: v.get("hideEdge").and_then(Value::as_bool).unwrap_or(false),
                },
            );
        }
        style
    }

    /// The screen's size: its background's (`AEBaseScreen` sets
    /// `imageWidth` and `imageHeight` from it).
    pub fn size(&self) -> (i32, i32) {
        self.background.as_ref().map_or((176, 166), Image::size)
    }

    /// Where the `index`th slot of `semantic` goes, from the screen's top
    /// left; none when the style has no place for it or hides it.
    pub fn slot(&self, semantic: &str, index: usize) -> Option<(i32, i32)> {
        let slot = self.slots.get(semantic).filter(|s| !s.hidden)?;
        let (w, h) = self.size();
        let (x, y) = slot.position.resolve(w, h);
        let (dx, dy) = slot.grid.map_or((0, 0), |g| g.offset(index));
        Some((x + dx, y + dy))
    }

    pub fn color(&self, name: &str) -> u32 {
        self.palette.get(name).copied().unwrap_or(0xFF40_4040)
    }
}

/// `#RRGGBB` or `#AARRGGBB` (`ColorDeserializer`) as `0xAARRGGBB`.
fn parse_color(text: &str) -> Option<u32> {
    let hex = text.strip_prefix('#')?;
    let n = u32::from_str_radix(hex, 16).ok()?;
    match hex.len() {
        6 => Some(0xFF00_0000 | n),
        8 => Some(n),
        _ => None,
    }
}

/// `loadMergedJsonTree`: the document over its includes, which resolve
/// relative to its own folder.
fn merged(screens: &Path, path: &str, loaded: &mut HashSet<String>) -> Result<Map<String, Value>, String> {
    let path = normalize(path);
    if !loaded.insert(path.clone()) {
        return Err(format!("recursive style includes at {path}"));
    }
    let bytes = std::fs::read(screens.join(&path)).map_err(|e| format!("screens/{path}: {e}"))?;
    let document: Map<String, Value> =
        serde_json::from_slice(&bytes).map_err(|e| format!("screens/{path}: {e}"))?;
    let Some(includes) = document.get("includes").and_then(Value::as_array) else {
        return Ok(document);
    };
    let base = path.rfind('/').map_or("", |i| &path[..=i]);
    let mut layers = Vec::new();
    for include in includes.iter().filter_map(Value::as_str) {
        layers.push(merged(screens, &format!("{base}{include}"), loaded)?);
    }
    layers.push(document);
    Ok(combine(&layers))
}

fn combine(layers: &[Map<String, Value>]) -> Map<String, Value> {
    let mut result = Map::new();
    for layer in layers {
        for (k, v) in layer {
            result.insert(k.clone(), v.clone());
        }
    }
    for key in MERGED {
        let mut merged: Option<Map<String, Value>> = None;
        for layer in layers {
            if let Some(object) = layer.get(key).and_then(Value::as_object) {
                let target = merged.get_or_insert_with(Map::new);
                for (k, v) in object {
                    target.insert(k.clone(), v.clone());
                }
            }
        }
        if let Some(merged) = merged {
            result.insert(key.to_owned(), Value::Object(merged));
        }
    }
    result
}

/// `a/b/../c.json` as `a/c.json`.
fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn layer(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn includes_merge_slots_and_later_layers_win() {
        let common = layer(json!({
            "palette": {"DEFAULT_TEXT_COLOR": "#413f54"},
            "slots": {"PLAYER_INVENTORY": {"left": 8, "bottom": 84, "grid": "BREAK_AFTER_9COLS"},
                      "PLAYER_HOTBAR": {"left": 8, "bottom": 26, "grid": "HORIZONTAL"}},
            "background": {"texture": "guis/other.png"}
        }));
        let screen = layer(json!({
            "background": {"texture": "guis/inscriber.png", "srcRect": [0, 0, 176, 178]},
            "slots": {"MACHINE_INPUT": {"left": 63, "top": 39}},
            "images": {"progressBar": {"texture": "guis/inscriber.png", "srcRect": [176, 0, 6, 18]}},
            "widgets": {"progressBar": {"left": 135, "top": 39}}
        }));
        let style = ScreenStyle::from_tree(&combine(&[common, screen]));
        assert_eq!(style.size(), (176, 178));
        assert_eq!(style.background.as_ref().unwrap().texture, "guis/inscriber");
        assert_eq!(style.slot("MACHINE_INPUT", 0), Some((63, 39)));
        // Player inventory: 84 above the bottom, nine to a row.
        assert_eq!(style.slot("PLAYER_INVENTORY", 0), Some((8, 94)));
        assert_eq!(style.slot("PLAYER_INVENTORY", 10), Some((26, 112)));
        assert_eq!(style.slot("PLAYER_HOTBAR", 8), Some((152, 152)));
        assert_eq!(style.color("DEFAULT_TEXT_COLOR"), 0xFF41_3F54);
        assert_eq!(style.images["progressBar"].size(), (6, 18));
        assert_eq!(style.widgets["progressBar"].position.resolve(176, 178), (135, 39));
    }

    #[test]
    fn grids_and_paths() {
        assert_eq!(Grid::BreakAfter(2).offset(3), (18, 18));
        assert_eq!(Grid::IoBusConfig.offset(4), (0, 18));
        assert_eq!(Grid::Vertical.offset(2), (0, 36));
        assert_eq!(normalize("terminals/../common/common.json"), "common/common.json");
        assert_eq!(parse_color("#80ffffff"), Some(0x80FF_FFFF));
    }

    /// Every style AE2 ships loads, when the prepared content is there.
    #[test]
    fn the_source_styles_load() {
        let screens = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content/ae2/assets/ae2/screens");
        if !screens.join("inscriber.json").exists() {
            return;
        }
        let inscriber = ScreenStyle::load(&screens, "inscriber.json").unwrap();
        assert_eq!(inscriber.size(), (176, 178));
        assert_eq!(inscriber.slot("INSCRIBER_PLATE_TOP", 0), Some((39, 23)));
        assert_eq!(inscriber.slot("PLAYER_HOTBAR", 0), Some((8, 152)));
        assert!(inscriber.palette.len() >= 8);
        let drive = ScreenStyle::load(&screens, "drive.json").unwrap();
        assert_eq!(drive.slot("STORAGE_CELL", 9), Some((89, 86)));
        for entry in std::fs::read_dir(&screens).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.ends_with(".json") && name != "schema.json" {
                ScreenStyle::load(&screens, &name).unwrap();
            }
        }
        ScreenStyle::load(&screens, "terminals/crafting_terminal.json").unwrap();
    }

    #[test]
    fn hidden_slots_have_no_place() {
        let style = ScreenStyle::from_tree(&layer(json!({"slots": {"X": {"left": 1, "top": 1, "hidden": true}}})));
        assert_eq!(style.slot("X", 0), None);
    }
}
