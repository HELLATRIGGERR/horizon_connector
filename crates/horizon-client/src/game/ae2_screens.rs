//! AE2's machine screens as AE2 19.2.18 draws them (`AEBaseScreen`,
//! `UpgradeableScreen`, `UpgradesPanel`, `VerticalButtonBar`), laid out by its
//! screen styles (`horizon_client::ae2_screen_style`) on Horizon's mod-screen
//! path (`native_hooks`: `NativeScreen`, menu buttons, data slots).
//!
//! The server's menu keeps its slots. The screen puts each where the style
//! puts its slot semantic, hides the menu entries that hold settings and
//! shows those as buttons: a press is a menu button whose id is the entry's
//! index (`REVERSE` for a right press), which the server handles as a press
//! on the entry (`lan_ae2_machine_button`). What the screen shows (progress,
//! the settings' values) comes as the menu's data slots, which
//! `ae2_screen_data` makes on the server.
use super::{Game, native_hooks::NativeScreen};
use horizon_client::{
    ae2_screen_style::{Align, Image, ScreenStyle},
    ae2_world::{Machine, MachineKind},
    hud::{HudBatch, rgba},
    items::ItemStack,
};
use horizon_world::nbt::Tag;
use std::ops::Range;

/// Set on a button's menu-button id for a right press, which steps the
/// setting back (`SettingToggleButton`).
pub(super) const REVERSE: i32 = 0x1000;
/// Set on a menu-button id for a press that acts as a shift-click on its
/// entry (the Interface's stock cogs open the stock editor that way).
pub(super) const QUICK: i32 = 0x2000;

/// `Icon`s in `guis/states.png`: (x, y), 16 x 16 unless noted.
mod icon {
    pub const HELP: (i32, i32) = (176, 0);
    pub const REDSTONE_PULSE: (i32, i32) = (32, 0);
    pub const REDSTONE_OFF: (i32, i32) = (64, 0);
    pub const REDSTONE_ON: (i32, i32) = (80, 0);
    pub const ENTER: (i32, i32) = (112, 0);
    pub const LOCKED: (i32, i32) = (144, 0);
    pub const UNLOCKED: (i32, i32) = (160, 0);
    pub const BLOCKING_MODE_NO: (i32, i32) = (64, 16);
    pub const BLOCKING_MODE_YES: (i32, i32) = (80, 16);
    pub const COG: (i32, i32) = (32, 64);
    pub const COG_DISABLED: (i32, i32) = (48, 64);
    pub const PATTERN_ACCESS_SHOW: (i32, i32) = (64, 80);
    pub const PATTERN_ACCESS_HIDE: (i32, i32) = (80, 80);
    pub const FUZZY_PERCENT_25: (i32, i32) = (0, 96);
    pub const FUZZY_PERCENT_50: (i32, i32) = (16, 96);
    pub const FUZZY_PERCENT_75: (i32, i32) = (32, 96);
    pub const FUZZY_PERCENT_99: (i32, i32) = (48, 96);
    pub const FUZZY_IGNORE: (i32, i32) = (64, 96);
    pub const REDSTONE_LOW: (i32, i32) = (0, 0);
    pub const REDSTONE_HIGH: (i32, i32) = (16, 0);
    pub const REDSTONE_IGNORE: (i32, i32) = (48, 0);
    pub const BACKGROUND_STORAGE_CELL: (i32, i32) = (240, 0);
    pub const BACKGROUND_PLATE: (i32, i32) = (224, 32);
    pub const BACKGROUND_INGOT: (i32, i32) = (224, 48);
    pub const BACKGROUND_STORAGE_COMPONENT: (i32, i32) = (240, 48);
    pub const ARROW_RIGHT: (i32, i32) = (32, 48);
    pub const ARROW_LEFT: (i32, i32) = (48, 48);
    pub const PRIORITY: (i32, i32) = (144, 64);
    pub const FULLNESS_EMPTY: (i32, i32) = (0, 80);
    pub const FULLNESS_HALF: (i32, i32) = (16, 80);
    pub const FULLNESS_FULL: (i32, i32) = (32, 80);
    pub const BACKGROUND_TRASH: (i32, i32) = (240, 80);
    pub const INSCRIBER_SEPARATE_SIDES: (i32, i32) = (80, 96);
    pub const INSCRIBER_COMBINED_SIDES: (i32, i32) = (96, 96);
    pub const AUTO_EXPORT_OFF: (i32, i32) = (112, 96);
    pub const AUTO_EXPORT_ON: (i32, i32) = (128, 96);
    pub const INSCRIBER_BUFFER_4: (i32, i32) = (144, 96);
    pub const INSCRIBER_BUFFER_64: (i32, i32) = (160, 96);
    pub const INSCRIBER_BUFFER_1: (i32, i32) = (176, 96);
    pub const CONDENSER_OUTPUT_TRASH: (i32, i32) = (0, 112);
    pub const CONDENSER_OUTPUT_MATTER_BALL: (i32, i32) = (16, 112);
    pub const CONDENSER_OUTPUT_SINGULARITY: (i32, i32) = (32, 112);
    pub const BACKGROUND_BLANK_PATTERN: (i32, i32) = (240, 128);
    pub const BACKGROUND_FUEL: (i32, i32) = (240, 192);
    pub const BACKGROUND_UPGRADE: (i32, i32) = (240, 208);
    /// 18 x 20 each.
    pub const TOOLBAR_BUTTON_BACKGROUND: (i32, i32) = (176, 128);
    pub const TOOLBAR_BUTTON_BACKGROUND_HOVER: (i32, i32) = (212, 128);
    /// 20 x 20.
    pub const TAB_BUTTON_BACKGROUND: (i32, i32) = (160, 192);
    /// 18 x 18.
    pub const SLOT_BACKGROUND: (i32, i32) = (192, 192);
}

/// How a toggle's value sits in the machine's config.
#[derive(Clone, Copy)]
enum Stored {
    /// A string, one of the values' names.
    Text,
    /// A byte, 1 for the second value; the flag when it is absent.
    Flag(bool),
    /// An int, the value's index.
    Index,
}

/// A `ServerSettingToggleButton`.
struct Toggle {
    /// The menu entry a press on the button acts as a press on.
    entry: i32,
    /// The machine config key it sets, how, and the index of its default value.
    key: &'static str,
    stored: Stored,
    default: usize,
    /// The data slot with the index of the value set.
    data: usize,
    /// `ButtonToolTips`: the tooltip's title.
    title: &'static str,
    /// Per value, in cycle order: its config value, icon and tooltip text.
    values: &'static [(&'static str, (i32, i32), &'static str)],
    /// Its own place on the screen; none puts it on the left toolbar.
    place: Option<(i32, i32)>,
    /// A data slot that must not be 0 for it to show (an installed card).
    shown_by: Option<usize>,
}

/// A `ProgressBar`, filling upwards (`Direction.VERTICAL`); its values are
/// wide data slots (two each).
struct Progress {
    /// The style's image and widget of that name.
    name: &'static str,
    current: usize,
    max: usize,
}

struct Layout {
    kind: MachineKind,
    style: &'static str,
    /// Menu entry to slot semantic; the n-th entry of a semantic takes its
    /// n-th place.
    slots: &'static [(usize, &'static str)],
    /// The icons empty slots show (`RestrictedInputSlot`'s, `setIcon`).
    icons: &'static [(usize, (i32, i32))],
    /// The menu entries of the upgrade cards, in the `upgrades` panel.
    upgrades: Range<usize>,
    toggles: &'static [Toggle],
    progress: Option<Progress>,
    /// `addOpenPriorityButton`: the menu entry that opens the priority editor.
    priority: Option<i32>,
    /// Items drawn on the background (`drawBG`), at their places.
    decorations: &'static [(&'static str, (i32, i32))],
    /// Crafting grid entries, dimmed unless the data slot's bit for them is
    /// set (`MolecularAssemblerPatternSlot`).
    grid: Option<(Range<usize>, usize)>,
    /// `SetAmountButton`s: a cog above each filled entry of the range, at the
    /// first one's place and 18 apart; a press opens its stock editor.
    cogs: Option<(Range<usize>, (i32, i32))>,
    /// The Pattern Provider's lock line (`lockReason`): data slots with the
    /// lock mode, whether it is locked, and why.
    lock_line: Option<[usize; 3]>,
}

const NO_YES: [&str; 2] = ["NO", "YES"];

const LAYOUTS: &[Layout] = &[
    Layout {
        kind: MachineKind::Inscriber,
        style: "inscriber.json",
        slots: &[(0, "INSCRIBER_PLATE_TOP"), (1, "INSCRIBER_PLATE_BOTTOM"), (2, "MACHINE_INPUT"), (3, "MACHINE_OUTPUT")],
        icons: &[(0, icon::BACKGROUND_PLATE), (1, icon::BACKGROUND_PLATE), (2, icon::BACKGROUND_INGOT)],
        upgrades: 4..8,
        toggles: &[
            Toggle {
                entry: 25,
                key: "inscriber_separate_sides",
                stored: Stored::Text,
                default: 0,
                data: 4,
                title: "gui.tooltips.ae2.InscriberSideness",
                values: &[
                    (NO_YES[0], icon::INSCRIBER_COMBINED_SIDES, "gui.tooltips.ae2.InscriberSidenessCombined"),
                    (NO_YES[1], icon::INSCRIBER_SEPARATE_SIDES, "gui.tooltips.ae2.InscriberSidenessSeparate"),
                ],
                place: None,
                shown_by: None,
            },
            Toggle {
                entry: 24,
                key: "auto_export",
                stored: Stored::Text,
                default: 0,
                data: 5,
                title: "gui.tooltips.ae2.AutoExport",
                values: &[
                    (NO_YES[0], icon::AUTO_EXPORT_OFF, "gui.tooltips.ae2.AutoExportOff"),
                    (NO_YES[1], icon::AUTO_EXPORT_ON, "gui.tooltips.ae2.AutoExportOn"),
                ],
                place: None,
                shown_by: None,
            },
            // The port cycles SIXTY_FOUR, ONE, FOUR: AE2's ONE, FOUR,
            // SIXTY_FOUR from another start.
            Toggle {
                entry: 26,
                key: "inscriber_input_capacity",
                stored: Stored::Text,
                default: 0,
                data: 6,
                title: "gui.tooltips.ae2.InscriberBufferSize",
                values: &[
                    ("SIXTY_FOUR", icon::INSCRIBER_BUFFER_64, "gui.tooltips.ae2.InscriberBufferHigh"),
                    ("ONE", icon::INSCRIBER_BUFFER_1, "gui.tooltips.ae2.InscriberBufferVeryLow"),
                    ("FOUR", icon::INSCRIBER_BUFFER_4, "gui.tooltips.ae2.InscriberBufferLow"),
                ],
                place: None,
                shown_by: None,
            },
        ],
        progress: Some(Progress { name: "progressBar", current: 0, max: 2 }),
        priority: None,
        decorations: &[],
        grid: None,
        cogs: None,
        lock_line: None,
    },
    Layout {
        kind: MachineKind::Drive,
        style: "drive.json",
        slots: &[
            (0, "STORAGE_CELL"),
            (1, "STORAGE_CELL"),
            (2, "STORAGE_CELL"),
            (3, "STORAGE_CELL"),
            (4, "STORAGE_CELL"),
            (5, "STORAGE_CELL"),
            (6, "STORAGE_CELL"),
            (7, "STORAGE_CELL"),
            (8, "STORAGE_CELL"),
            (9, "STORAGE_CELL"),
        ],
        icons: &[
            (0, icon::BACKGROUND_STORAGE_CELL),
            (1, icon::BACKGROUND_STORAGE_CELL),
            (2, icon::BACKGROUND_STORAGE_CELL),
            (3, icon::BACKGROUND_STORAGE_CELL),
            (4, icon::BACKGROUND_STORAGE_CELL),
            (5, icon::BACKGROUND_STORAGE_CELL),
            (6, icon::BACKGROUND_STORAGE_CELL),
            (7, icon::BACKGROUND_STORAGE_CELL),
            (8, icon::BACKGROUND_STORAGE_CELL),
            (9, icon::BACKGROUND_STORAGE_CELL),
        ],
        upgrades: 0..0,
        toggles: &[],
        progress: None,
        priority: Some(26),
        decorations: &[],
        grid: None,
        cogs: None,
        lock_line: None,
    },
    Layout {
        kind: MachineKind::VibrationChamber,
        style: "vibration_chamber.json",
        slots: &[(0, "MACHINE_INPUT")],
        icons: &[(0, icon::BACKGROUND_FUEL)],
        upgrades: 1..4,
        toggles: &[],
        progress: Some(Progress { name: "generationRateBar", current: 0, max: 2 }),
        priority: None,
        decorations: &[],
        grid: None,
        cogs: None,
        lock_line: None,
    },
    Layout {
        kind: MachineKind::IoPort,
        style: "io_port.json",
        slots: &[
            (0, "MACHINE_INPUT"),
            (1, "MACHINE_INPUT"),
            (2, "MACHINE_INPUT"),
            (3, "MACHINE_INPUT"),
            (4, "MACHINE_INPUT"),
            (5, "MACHINE_INPUT"),
            (6, "MACHINE_OUTPUT"),
            (7, "MACHINE_OUTPUT"),
            (8, "MACHINE_OUTPUT"),
            (9, "MACHINE_OUTPUT"),
            (10, "MACHINE_OUTPUT"),
            (11, "MACHINE_OUTPUT"),
        ],
        icons: &[
            (0, icon::BACKGROUND_STORAGE_CELL),
            (1, icon::BACKGROUND_STORAGE_CELL),
            (2, icon::BACKGROUND_STORAGE_CELL),
            (3, icon::BACKGROUND_STORAGE_CELL),
            (4, icon::BACKGROUND_STORAGE_CELL),
            (5, icon::BACKGROUND_STORAGE_CELL),
            (6, icon::BACKGROUND_STORAGE_CELL),
            (7, icon::BACKGROUND_STORAGE_CELL),
            (8, icon::BACKGROUND_STORAGE_CELL),
            (9, icon::BACKGROUND_STORAGE_CELL),
            (10, icon::BACKGROUND_STORAGE_CELL),
            (11, icon::BACKGROUND_STORAGE_CELL),
        ],
        upgrades: 12..15,
        toggles: &[
            Toggle {
                entry: 24,
                key: "io_fullness",
                stored: Stored::Text,
                default: 0,
                data: 0,
                title: "gui.tooltips.ae2.OperationMode",
                values: &[
                    ("empty", icon::FULLNESS_EMPTY, "gui.tooltips.ae2.MoveWhenEmpty"),
                    ("half", icon::FULLNESS_HALF, "gui.tooltips.ae2.MoveWhenWorkIsDone"),
                    ("full", icon::FULLNESS_FULL, "gui.tooltips.ae2.MoveWhenFull"),
                ],
                place: None,
                shown_by: None,
            },
            // AE2's fourth value, SIGNAL_PULSE, is not in the port.
            Toggle {
                entry: 25,
                key: "io_redstone",
                stored: Stored::Text,
                default: 0,
                data: 1,
                title: "gui.tooltips.ae2.RedstoneMode",
                values: &[
                    ("ignore", icon::REDSTONE_IGNORE, "gui.tooltips.ae2.AlwaysActive"),
                    ("low", icon::REDSTONE_LOW, "gui.tooltips.ae2.ActiveWithoutSignal"),
                    ("high", icon::REDSTONE_HIGH, "gui.tooltips.ae2.ActiveWithSignal"),
                ],
                place: None,
                shown_by: Some(3),
            },
            Toggle {
                entry: 26,
                key: "io_operation",
                stored: Stored::Text,
                default: 1,
                data: 2,
                title: "gui.tooltips.ae2.TransferDirection",
                values: &[
                    ("fill", icon::ARROW_LEFT, "gui.tooltips.ae2.TransferToStorageCell"),
                    ("empty", icon::ARROW_RIGHT, "gui.tooltips.ae2.TransferToNetwork"),
                ],
                place: Some((80, 17)),
                shown_by: None,
            },
        ],
        progress: None,
        priority: None,
        decorations: &[("ae2:item_storage_cell_1k", (58, 17)), ("ae2:drive", (102, 17))],
        grid: None,
        cogs: None,
        lock_line: None,
    },
    Layout {
        kind: MachineKind::Condenser,
        style: "condenser.json",
        slots: &[(0, "MACHINE_INPUT"), (1, "MACHINE_OUTPUT"), (2, "STORAGE_CELL")],
        icons: &[(0, icon::BACKGROUND_TRASH), (2, icon::BACKGROUND_STORAGE_COMPONENT)],
        upgrades: 0..0,
        toggles: &[Toggle {
            entry: 26,
            key: "condenser_output",
            stored: Stored::Text,
            default: 0,
            data: 4,
            title: "gui.tooltips.ae2.CondenserOutput",
            values: &[
                ("trash", icon::CONDENSER_OUTPUT_TRASH, "gui.tooltips.ae2.Trash"),
                ("matter_balls", icon::CONDENSER_OUTPUT_MATTER_BALL, "gui.tooltips.ae2.MatterBalls"),
                ("singularity", icon::CONDENSER_OUTPUT_SINGULARITY, "gui.tooltips.ae2.Singularity"),
            ],
            place: Some((129, 53)),
            shown_by: None,
        }],
        progress: Some(Progress { name: "progressBar", current: 0, max: 2 }),
        priority: None,
        decorations: &[],
        grid: None,
        cogs: None,
        lock_line: None,
    },
    Layout {
        kind: MachineKind::MolecularAssembler,
        style: "molecular_assembler.json",
        slots: &[
            (0, "MACHINE_CRAFTING_GRID"),
            (1, "MACHINE_CRAFTING_GRID"),
            (2, "MACHINE_CRAFTING_GRID"),
            (3, "MACHINE_CRAFTING_GRID"),
            (4, "MACHINE_CRAFTING_GRID"),
            (5, "MACHINE_CRAFTING_GRID"),
            (6, "MACHINE_CRAFTING_GRID"),
            (7, "MACHINE_CRAFTING_GRID"),
            (8, "MACHINE_CRAFTING_GRID"),
            (9, "MACHINE_OUTPUT"),
            (10, "ENCODED_PATTERN"),
        ],
        icons: &[(10, icon::BACKGROUND_BLANK_PATTERN)],
        upgrades: 11..16,
        toggles: &[],
        progress: Some(Progress { name: "progressBar", current: 0, max: 2 }),
        priority: None,
        decorations: &[],
        grid: Some((0..9, 4)),
        cogs: None,
        lock_line: None,
    },
    Layout {
        kind: MachineKind::PatternProvider,
        style: "pattern_provider.json",
        slots: &[
            (0, "ENCODED_PATTERN"),
            (1, "ENCODED_PATTERN"),
            (2, "ENCODED_PATTERN"),
            (3, "ENCODED_PATTERN"),
            (4, "ENCODED_PATTERN"),
            (5, "ENCODED_PATTERN"),
            (6, "ENCODED_PATTERN"),
            (7, "ENCODED_PATTERN"),
            (8, "ENCODED_PATTERN"),
            (9, "STORAGE"),
            (10, "STORAGE"),
            (11, "STORAGE"),
            (12, "STORAGE"),
            (13, "STORAGE"),
            (14, "STORAGE"),
            (15, "STORAGE"),
            (16, "STORAGE"),
            (17, "STORAGE"),
        ],
        icons: &[
            (0, icon::BACKGROUND_BLANK_PATTERN),
            (1, icon::BACKGROUND_BLANK_PATTERN),
            (2, icon::BACKGROUND_BLANK_PATTERN),
            (3, icon::BACKGROUND_BLANK_PATTERN),
            (4, icon::BACKGROUND_BLANK_PATTERN),
            (5, icon::BACKGROUND_BLANK_PATTERN),
            (6, icon::BACKGROUND_BLANK_PATTERN),
            (7, icon::BACKGROUND_BLANK_PATTERN),
            (8, icon::BACKGROUND_BLANK_PATTERN),
        ],
        upgrades: 0..0,
        toggles: &[
            Toggle {
                entry: 18,
                key: "ae2_blocking",
                stored: Stored::Flag(false),
                default: 0,
                data: 0,
                title: "gui.tooltips.ae2.InterfaceBlockingMode",
                values: &[
                    ("NO", icon::BLOCKING_MODE_NO, "gui.tooltips.ae2.NonBlocking"),
                    ("YES", icon::BLOCKING_MODE_YES, "gui.tooltips.ae2.Blocking"),
                ],
                place: None,
                shown_by: None,
            },
            Toggle {
                entry: 19,
                key: "ae2_lock_mode",
                stored: Stored::Index,
                default: 0,
                data: 1,
                title: "gui.tooltips.ae2.LockCraftingMode",
                values: &[
                    ("NONE", icon::UNLOCKED, "gui.tooltips.ae2.LockCraftingModeNone"),
                    ("LOCK_UNTIL_PULSE", icon::REDSTONE_PULSE, "gui.tooltips.ae2.LockCraftingUntilRedstonePulse"),
                    ("LOCK_WHILE_HIGH", icon::REDSTONE_ON, "gui.tooltips.ae2.LockCraftingWhileRedstoneHigh"),
                    ("LOCK_WHILE_LOW", icon::REDSTONE_OFF, "gui.tooltips.ae2.LockCraftingWhileRedstoneLow"),
                    ("LOCK_UNTIL_RESULT", icon::ENTER, "gui.tooltips.ae2.LockCraftingUntilResultReturned"),
                ],
                place: None,
                shown_by: None,
            },
            // The port keeps "visible" as the absence of the flag.
            Toggle {
                entry: 20,
                key: "terminal_visible",
                stored: Stored::Flag(true),
                default: 1,
                data: 2,
                title: "gui.ae2.PatternAccessTerminal",
                values: &[
                    ("NO", icon::PATTERN_ACCESS_HIDE, "gui.ae2.PatternAccessTerminalHint"),
                    ("YES", icon::PATTERN_ACCESS_SHOW, "gui.ae2.PatternAccessTerminalHint"),
                ],
                place: None,
                shown_by: None,
            },
        ],
        progress: None,
        priority: Some(26),
        decorations: &[],
        grid: None,
        cogs: None,
        lock_line: Some([1, 3, 4]),
    },
    Layout {
        kind: MachineKind::Interface,
        style: "interface.json",
        slots: &[
            (9, "CONFIG"),
            (10, "CONFIG"),
            (11, "CONFIG"),
            (12, "CONFIG"),
            (13, "CONFIG"),
            (14, "CONFIG"),
            (15, "CONFIG"),
            (16, "CONFIG"),
            (17, "CONFIG"),
            (0, "STORAGE"),
            (1, "STORAGE"),
            (2, "STORAGE"),
            (3, "STORAGE"),
            (4, "STORAGE"),
            (5, "STORAGE"),
            (6, "STORAGE"),
            (7, "STORAGE"),
            (8, "STORAGE"),
        ],
        icons: &[],
        upgrades: 18..19,
        toggles: &[Toggle {
            entry: 23,
            key: "ae2_fuzzy_mode",
            stored: Stored::Text,
            default: 0,
            data: 0,
            title: "gui.tooltips.ae2.FuzzyMode",
            values: &[
                ("ignore_all", icon::FUZZY_IGNORE, "gui.tooltips.ae2.FZIgnoreAll"),
                ("percent_99", icon::FUZZY_PERCENT_99, "gui.tooltips.ae2.FZPercent_99"),
                ("percent_75", icon::FUZZY_PERCENT_75, "gui.tooltips.ae2.FZPercent_75"),
                ("percent_50", icon::FUZZY_PERCENT_50, "gui.tooltips.ae2.FZPercent_50"),
                ("percent_25", icon::FUZZY_PERCENT_25, "gui.tooltips.ae2.FZPercent_25"),
            ],
            place: None,
            shown_by: Some(1),
        }],
        progress: None,
        priority: Some(26),
        decorations: &[],
        grid: None,
        cogs: Some((9..18, (8, 35))),
        lock_line: None,
    },
];

/// The burn flame over a Vibration Chamber's fuel (`BURN_PROGRESS`): its
/// source in `guis/vibchamber.png` and where it goes.
const BURN_SOURCE: [i32; 4] = [176, 0, 14, 13];
const BURN_AT: (i32, i32) = (80, 20);

/// `UpgradesPanel`: slots 18 apart, the panel's padding.
const PANEL_SLOT: i32 = 18;
const PANEL_PADDING: i32 = 5;

/// An open AE2 machine screen: its layout, style, and where its menu's
/// entries are (`NativeScreen::slots`).
pub(super) struct Ae2Screen {
    layout: &'static Layout,
    style: ScreenStyle,
    slots: Vec<Option<[f32; 2]>>,
    /// The machine's block, whose guide page the help button opens.
    block: String,
}

/// A button the screen shows: the help button (no toggle) or a toggle,
/// and its 16 x 16 box from the screen's top left.
struct Button {
    rect: [i32; 4],
    toggle: Option<&'static Toggle>,
}

impl Ae2Screen {
    fn size(&self) -> (i32, i32) {
        self.style.size()
    }

    /// `UpgradesPanel`'s top left (the style's `upgrades` widget).
    fn panel(&self) -> (i32, i32) {
        let (w, h) = self.size();
        self.style.widgets.get("upgrades").map_or((w - 2, 0), |s| s.position.resolve(w, h))
    }

    /// The buttons shown with `data`: the left toolbar's first
    /// (`VerticalButtonBar`: the help button, then the toggles on it, the
    /// hidden ones leaving no gap), then the toggles with their own places.
    fn buttons(&self, data: &[i32]) -> Vec<Button> {
        let (w, h) = self.size();
        let (x, y) = self
            .style
            .widgets
            .get("verticalToolbar")
            .map_or((3, 1), |s| s.position.resolve(w, h));
        let shown = |t: &Toggle| t.shown_by.is_none_or(|i| value(data, i) != 0);
        let on_toolbar = self.layout.toggles.iter().filter(|t| t.place.is_none() && shown(t));
        // MARGIN 2, VERTICAL_SPACING 6.
        let mut buttons: Vec<Button> = std::iter::once(None)
            .chain(on_toolbar.map(Some))
            .enumerate()
            .map(|(i, toggle)| Button { rect: [x - 2 - 16, y + 2 + i as i32 * 22, 16, 16], toggle })
            .collect();
        for t in self.layout.toggles.iter().filter(|t| shown(t)) {
            if let Some((bx, by)) = t.place {
                buttons.push(Button { rect: [bx, by, 16, 16], toggle: Some(t) });
            }
        }
        buttons
    }

    /// The left toolbar's buttons alone (the first ones of `buttons`).
    fn toolbar_len(&self, data: &[i32]) -> usize {
        1 + self.layout.toggles.iter().filter(|t| t.place.is_none() && t.shown_by.is_none_or(|i| value(data, i) != 0)).count()
    }

    /// The stock cogs shown: each one's 16 x 16 box and its entry, above the
    /// entries `filled` says hold something.
    fn cogs(&self, filled: impl Fn(usize) -> bool) -> Vec<([i32; 4], usize)> {
        let Some((entries, (x, y))) = self.layout.cogs.clone() else {
            return Vec::new();
        };
        let first = entries.start;
        entries
            .filter(|e| filled(*e))
            .map(|e| ([x + 18 * (e - first) as i32, y, 16, 16], e))
            .collect()
    }

    /// The priority tab's 20 x 20 box (`openPriority`).
    fn priority_tab(&self) -> Option<[i32; 4]> {
        self.layout.priority?;
        let (w, h) = self.size();
        let s = self.style.widgets.get("openPriority")?;
        let (x, y) = s.position.resolve(w, h);
        Some([x, y, 20, 20])
    }
}

fn inside([x, y, w, h]: [i32; 4], at: [f32; 2]) -> bool {
    at[0] >= x as f32 && at[0] < (x + w) as f32 && at[1] >= y as f32 && at[1] < (y + h) as f32
}

/// A style's `0xAARRGGBB` as `rgba` packs it.
fn argb(c: u32) -> u32 {
    rgba((c >> 16) as u8, (c >> 8) as u8, c as u8, (c >> 24) as u8)
}

fn value(data: &[i32], i: usize) -> i32 {
    data.get(i).copied().unwrap_or(0)
}

/// A value too big for one 16-bit data slot, in two (low half first).
fn wide(v: f64) -> [i16; 2] {
    let v = v.clamp(0.0, f64::from(u32::MAX)) as u32;
    [(v & 0xFFFF) as u16 as i16, (v >> 16) as u16 as i16]
}

fn wide_value(data: &[i32], i: usize) -> i64 {
    i64::from(value(data, i) as u16) | i64::from(value(data, i + 1) as u16) << 16
}

fn short(v: f64) -> i16 {
    v.clamp(0.0, f64::from(i16::MAX)) as i16
}

/// The index of `t`'s value in `m`'s config (its default when unset).
fn setting(m: &Machine, t: &Toggle) -> i16 {
    let at = match (t.stored, m.config.get(&t.key.into())) {
        (Stored::Text, Some(Tag::String(s))) => {
            s.to_string().ok().and_then(|s| t.values.iter().position(|v| v.0 == s))
        }
        (Stored::Flag(_), Some(Tag::Byte(b))) => Some(usize::from(*b != 0)),
        (Stored::Flag(absent), None) => Some(usize::from(absent)),
        (Stored::Index, Some(Tag::Int(n))) => usize::try_from(*n).ok().filter(|n| *n < t.values.len()),
        _ => None,
    };
    at.unwrap_or(t.default) as i16
}

fn layout(kind: MachineKind) -> Option<&'static Layout> {
    LAYOUTS.iter().find(|l| l.kind == kind)
}

/// A press on toggle entry `slot` (300 + entry) of `m`'s menu where the
/// machine has no handler of its own: its config value steps on (back with
/// `reverse`). False when no toggle has that entry.
pub(super) fn step_setting(m: &mut Machine, slot: usize, reverse: bool) -> bool {
    let Some(t) = layout(m.kind).and_then(|l| l.toggles.iter().find(|t| t.entry as usize + 300 == slot)) else {
        return false;
    };
    let n = t.values.len();
    let at = setting(m, t) as usize;
    let next = if reverse { (at + n - 1) % n } else { (at + 1) % n };
    let value = match t.stored {
        Stored::Text => Tag::String(t.values[next].0.into()),
        Stored::Flag(_) => Tag::Byte(i8::from(next != 0)),
        Stored::Index => Tag::Int(next as i32),
    };
    m.config.insert(t.key.into(), value);
    true
}

impl Game<'_> {
    /// The data slots of the screen for `m`'s menu, on the server.
    pub(super) fn ae2_screen_data(&self, m: &Machine) -> Vec<i16> {
        let Some(l) = layout(m.kind) else {
            return Vec::new();
        };
        let mut data = Vec::new();
        match m.kind {
            // `InscriberMenu`: processingTime of MAX_PROCESSING_STEPS (200).
            MachineKind::Inscriber => {
                data.extend(wide(m.progress.min(200.0)));
                data.extend(wide(200.0));
            }
            // `MolecularAssemblerMenu`: craftProgress of 100.
            MachineKind::MolecularAssembler => {
                data.extend(wide(m.progress.min(100.0)));
                data.extend(wide(100.0));
            }
            // `VibrationChamberMenu.broadcastChanges`: fuel ticks per tick
            // (x100) of the most (x100), the burn left in percent of the fuel
            // item's, the fuel efficiency in percent, AE per tick (x100).
            MachineKind::VibrationChamber => {
                let settings = horizon_client::ae2_common_config::processing();
                let energy_cards = m
                    .upgrades
                    .iter()
                    .flatten()
                    .filter(|s| s.count == 1 && s.item.as_ref() == "ae2:energy_card")
                    .count() as f64;
                let per_fuel = settings.fuel_energy * (1.0 + energy_cards / 2.0);
                let min = settings.vibration_min / per_fuel;
                let max = (settings.vibration_max * (1.0 + m.installed_speed_cards() as f64 / 2.0) / per_fuel).max(min);
                let fuel = match m.config.get(&"ae2_fuel_max".into()) {
                    Some(Tag::Double(bits)) => f64::from_bits(*bits),
                    _ => 0.0,
                };
                let left = if fuel > 0.0 { (100.0 * m.burn_remaining / fuel).floor() } else { 0.0 };
                let rate = if left > 0.0 { m.burn_rate } else { 0.0 };
                data.extend(wide(rate * 100.0));
                data.extend(wide(max * 100.0));
                data.push(short(left));
                data.push(short(100.0 + 50.0 * energy_cards));
                data.extend(wide(rate * per_fuel * 100.0));
            }
            // `CondenserMenu`: stored matter of the most it holds for its
            // output (`requiredEnergy`).
            MachineKind::Condenser => {
                let (stored, required) = super::ae2_storage_devices::condenser_progress(m);
                data.extend(wide(stored));
                data.extend(wide(required));
            }
            MachineKind::IoPort => data.resize(4, 0),
            // PatternProviderMenu: the settings, then whether crafting is
            // locked now and why (`craftingLockedReason`): 1 until a pulse,
            // 2 by a signal, 3 by its lack, 4 until a result returns.
            MachineKind::PatternProvider => {
                let mode = match m.config.get(&"ae2_lock_mode".into()) {
                    Some(Tag::Int(n)) => *n,
                    _ => 0,
                };
                let high = m.config.get(&"ae2_redstone_high".into()) == Some(&Tag::Byte(1));
                let event = match m.config.get(&"ae2_unlock_event".into()) {
                    Some(Tag::Int(n)) => *n,
                    _ => 0,
                };
                let reason = if mode == 2 && high {
                    2
                } else if mode == 3 && !high {
                    3
                } else if event == 3 {
                    4
                } else if event != 0 {
                    1
                } else {
                    0
                };
                data.resize(5, 0);
                data[3] = i16::from(reason != 0);
                data[4] = reason;
            }
            // InterfaceMenu: the fuzzy button shows with a fuzzy card.
            MachineKind::Interface => {
                data.resize(2, 0);
                data[1] = i16::from(m.upgrades.iter().flatten().any(|s| s.item.as_ref() == "ae2:fuzzy_card"));
            }
            _ => {}
        }
        for t in l.toggles {
            if data.len() <= t.data {
                data.resize(t.data + 1, 0);
            }
            data[t.data] = setting(m, t);
        }
        if m.kind == MachineKind::IoPort {
            data[3] = i16::from(m.upgrades.iter().flatten().any(|s| s.item.as_ref() == "ae2:redstone_card"));
        }
        if let Some((grid, at)) = l.grid.clone() {
            // An assembler slot takes items when it holds one or its pattern
            // uses it (`MolecularAssemblerPatternSlot.isSlotEnabled`).
            let pattern = m.slots.get(10).cloned().flatten();
            let mut mask = 0i16;
            for i in grid {
                let used = m.slots.get(i).is_some_and(Option::is_some)
                    || pattern.as_ref().is_some_and(|p| {
                        super::ae2_processing::assembler_pattern_slot_enabled(
                            p,
                            i,
                            self.res.items(),
                            self.res.recipes(),
                            self.res.stations(),
                        )
                    });
                if used {
                    mask |= 1 << i;
                }
            }
            if data.len() <= at {
                data.resize(at + 1, 0);
            }
            data[at] = mask;
        }
        data
    }

    /// The screen of block `block`'s menu when it is an AE2 machine with an
    /// AE2 screen: where its slots go, and its style.
    pub(super) fn ae2_native_screen(&self, block: &str) -> Option<(NativeScreen, Ae2Screen)> {
        let layout = layout(MachineKind::from_id(block)?)?;
        let screens = self.options.resources.join("assets/ae2/screens");
        let style = match ScreenStyle::load(&screens, layout.style) {
            Ok(style) => style,
            Err(error) => {
                log::warn!("AE2 screen style for {block}: {error}");
                return None;
            }
        };
        let mut screen = Ae2Screen { layout, style, slots: Vec::new(), block: block.to_owned() };
        let (w, h) = screen.size();
        let mut slots = vec![None; 27];
        let mut nth = std::collections::HashMap::<&str, usize>::new();
        for &(entry, semantic) in layout.slots {
            let n = nth.entry(semantic).or_default();
            if let Some((x, y)) = screen.style.slot(semantic, *n) {
                slots[entry] = Some([x as f32, y as f32]);
            }
            *n += 1;
        }
        // `UpgradesPanel.updateBeforeRender`: one column of slots down the
        // panel, a pixel in from its left edge (up to 8 a column).
        let (px, py) = screen.panel();
        for (i, entry) in layout.upgrades.clone().enumerate() {
            let (col, row) = (i as i32 / 8, i as i32 % 8);
            slots[entry] = Some([(px + 1 + col * PANEL_SLOT) as f32, (py + PANEL_PADDING + 1 + row * PANEL_SLOT) as f32]);
        }
        let (ix, iy) = screen.style.slot("PLAYER_INVENTORY", 0).unwrap_or((8, h - 84));
        screen.slots = slots.clone();
        let laid_out = NativeScreen {
            width: w as f32,
            height: h as f32,
            slots,
            inventory_y: iy as f32,
            inventory_x: ix as f32,
            own_labels: true,
        };
        Some((laid_out, screen))
    }

    /// Blits `image`'s region `[sx, sy, w, h]` at `x`, `y` (`Blitter.blit`).
    fn ae2_blit(&self, batch: &mut HudBatch, image: &Image, src: [i32; 4], x: f32, y: f32) {
        self.ae2_blit_to(batch, image, src, [x, y, src[2] as f32, src[3] as f32], u32::MAX);
    }

    /// Blits `image`'s region `[sx, sy, sw, sh]`, measured in the image's
    /// reference size, over `dest`, stretched to it, tinted by `color`.
    fn ae2_blit_to(&self, batch: &mut HudBatch, image: &Image, [sx, sy, sw, sh]: [i32; 4], dest: [f32; 4], color: u32) {
        if sw <= 0 || sh <= 0 {
            return;
        }
        let Some(mut r) = self.gui.atlas.sprites.get(&format!("ae2:{}", image.texture)).copied() else {
            return;
        };
        let (tw, th) = (image.texture_size.0 as f32, image.texture_size.1 as f32);
        let (u, v, du, dv) = (r.u0, r.v0, r.u1 - r.u0, r.v1 - r.v0);
        r.u0 = u + du * sx as f32 / tw;
        r.u1 = u + du * (sx + sw) as f32 / tw;
        r.v0 = v + dv * sy as f32 / th;
        r.v1 = v + dv * (sy + sh) as f32 / th;
        let [x, y, w, h] = dest;
        batch.rect_uv(x, y, w, h, r, color);
    }

    /// An `Icon` of `guis/states.png`.
    fn ae2_icon(&self, batch: &mut HudBatch, (sx, sy): (i32, i32), (w, h): (i32, i32), x: f32, y: f32) {
        let states = Image { texture: "guis/states".into(), texture_size: (256, 256), src: None };
        self.ae2_blit(batch, &states, [sx, sy, w, h], x, y);
    }

    pub(super) fn ae2_draw_screen(&self, batch: &mut HudBatch, ox: f32, oy: f32, screen: &Ae2Screen, data: &[i32]) {
        let style = &screen.style;
        let layout = screen.layout;
        let mouse = [self.cursor.0 - ox, self.cursor.1 - oy];
        // The background first: the widgets' backgrounds go over it
        // (`AEBaseScreen.renderBg`), the toolbar's over its left edge and the
        // upgrades panel's over its right edge, so both join the frame.
        if let Some(background) = style.background.as_ref() {
            let (w, h) = background.size();
            let [sx, sy] = background.src.map_or([0, 0], |[x, y, _, _]| [x, y]);
            self.ae2_blit(batch, background, [sx, sy, w, h], ox, oy);
        }
        let buttons = screen.buttons(data);
        let toolbar = &buttons[..screen.toolbar_len(data)];
        if let (Some(first), Some(last)) = (toolbar.first(), toolbar.last()) {
            // VerticalButtonBar.drawBackgroundLayer: `vertical_buttons_bg`, a
            // 21 x 26 nine-slice with 2 rows on top and 4 below.
            let (x, y) = (first.rect[0] - 2 - 2, first.rect[1] - 2 - 1);
            let height = last.rect[1] + 16 + 6 - first.rect[1] + 2 + 4;
            let bg = Image { texture: "gui/vertical_buttons_bg".into(), texture_size: (21, 26), src: None };
            let (x, y) = (ox + x as f32, oy + y as f32);
            let middle = (height - 6).max(0) as f32;
            self.ae2_blit(batch, &bg, [0, 0, 21, 2], x, y);
            self.ae2_blit_to(batch, &bg, [0, 2, 21, 20], [x, y + 2.0, 21.0, middle], u32::MAX);
            self.ae2_blit(batch, &bg, [0, 22, 21, 4], x, y + 2.0 + middle);
        }
        let upgrades = layout.upgrades.len() as i32;
        if upgrades > 0 {
            self.ae2_draw_upgrades(batch, ox, oy, screen.panel(), upgrades);
        }
        for &(id, (x, y)) in layout.decorations {
            self.draw_item(batch, &ItemStack::new(id, 1), ox + x as f32, oy + y as f32);
        }
        // Crafting grid slots: SLOT_BACKGROUND, faint where the pattern
        // leaves the slot unused.
        if let Some((grid, at)) = layout.grid.clone() {
            let mask = value(data, at);
            let states = Image { texture: "guis/states".into(), texture_size: (256, 256), src: None };
            for entry in grid {
                if let Some([x, y]) = screen.slots.get(entry).copied().flatten() {
                    let alpha = if mask & (1 << entry) != 0 { 255 } else { 51 };
                    let (sx, sy) = icon::SLOT_BACKGROUND;
                    self.ae2_blit_to(batch, &states, [sx, sy, 18, 18], [ox + x - 1.0, oy + y - 1.0, 18.0, 18.0], rgba(255, 255, 255, alpha));
                }
            }
        }
        // Empty slots' icons (`AEBaseScreen.drawSlotIcons`).
        let upgrade_icons = layout.upgrades.clone().map(|e| (e, icon::BACKGROUND_UPGRADE));
        for (entry, place) in layout.icons.iter().copied().chain(upgrade_icons) {
            if let Some([x, y]) = screen.slots.get(entry).copied().flatten()
                && self.container_kind().is_none_or(|e| e.slots().get(entry).is_none_or(Option::is_none))
            {
                self.ae2_icon(batch, place, (16, 16), ox + x, oy + y);
            }
        }
        for b in &buttons {
            let shown = match b.toggle {
                None => icon::HELP,
                Some(t) => t.values[value(data, t.data).clamp(0, t.values.len() as i32 - 1) as usize].1,
            };
            let hover = inside(b.rect, mouse);
            // IconButton.renderWidget: the backdrop a pixel left, both a
            // pixel lower while hovered.
            let lift = if hover { 1.0 } else { 0.0 };
            let backdrop = if hover { icon::TOOLBAR_BUTTON_BACKGROUND_HOVER } else { icon::TOOLBAR_BUTTON_BACKGROUND };
            let (x, y) = (ox + b.rect[0] as f32, oy + b.rect[1] as f32);
            self.ae2_icon(batch, backdrop, (18, 20), x - 1.0, y + lift);
            self.ae2_icon(batch, shown, (16, 16), x, y + 1.0 + lift);
        }
        if let Some(tab) = screen.priority_tab() {
            let (x, y) = (ox + tab[0] as f32, oy + tab[1] as f32);
            self.ae2_icon(batch, icon::TAB_BUTTON_BACKGROUND, (20, 20), x, y);
            self.ae2_icon(batch, icon::PRIORITY, (16, 16), x + 2.0, y + 1.0);
        }
        // SetAmountButton: no backdrop, the cog lit while hovered.
        for (rect, _) in screen.cogs(|e| self.ae2_screen_entry_filled(e)) {
            let cog = if inside(rect, mouse) { icon::COG } else { icon::COG_DISABLED };
            self.ae2_icon(batch, cog, (16, 16), ox + rect[0] as f32, oy + rect[1] as f32);
        }
        // PatternProviderLockReason: shown while a lock mode is set.
        if let Some([mode, locked, _]) = layout.lock_line
            && value(data, mode) != 0
        {
            let locked = value(data, locked) != 0;
            let (mark, key, color) = if locked {
                (icon::LOCKED, "gui.ae2.CraftingLockIsLocked", rgba(193, 66, 75, 255))
            } else {
                (icon::UNLOCKED, "gui.ae2.CraftingLockIsUnlocked", rgba(125, 169, 210, 255))
            };
            self.ae2_icon(batch, mark, (16, 16), ox + 5.0, oy + 15.0);
            batch.text(&self.gui.atlas, &self.translate(key, key), ox + 20.0, oy + 20.0, color, false);
        }
        if let Some(progress) = layout.progress.as_ref()
            && let (Some(image), Some(widget)) = (style.images.get(progress.name), style.widgets.get(progress.name))
        {
            let (w, h) = screen.size();
            let (x, y) = widget.position.resolve(w, h);
            let [sx, sy, sw, sh] = image.src.unwrap_or([0, 0, 0, 0]);
            let max = wide_value(data, progress.max);
            let current = wide_value(data, progress.current).min(max);
            // ProgressBar.renderWidget, VERTICAL.
            let diff = sh - if max > 0 { (i64::from(sh) * current / max) as i32 } else { 0 };
            self.ae2_blit(batch, image, [sx, sy + diff, sw, sh - diff], ox + x as f32, oy + (y + diff) as f32);
        }
        if layout.kind == MachineKind::VibrationChamber && value(data, 4) > 0 {
            // VibrationChamberScreen.drawFG: the flame burns down.
            let [sx, sy, sw, sh] = BURN_SOURCE;
            let f = value(data, 4) * sh / 100;
            let image = Image { texture: "guis/vibchamber".into(), texture_size: (256, 256), src: None };
            let (x, y) = (ox + BURN_AT.0 as f32, oy + (BURN_AT.1 + sh - f) as f32);
            self.ae2_blit(batch, &image, [sx, sy + sh - f, sw, f], x, y);
        }
        // The style's labels (`dialog_title`, `player_inventory_title`).
        let (w, h) = screen.size();
        for text in style.text.values() {
            let label = match (text.text.get("translate").and_then(|v| v.as_str()), text.text.get("text")) {
                (Some(key), _) => self.translate(key, key),
                (None, Some(v)) => v.as_str().unwrap_or("").to_owned(),
                _ => continue,
            };
            let (x, y) = text.position.resolve(w, h);
            let width = HudBatch::text_width(&self.gui.atlas, &label);
            let x = match text.align {
                Align::Left => x as f32,
                Align::Center => x as f32 - width / 2.0,
                Align::Right => x as f32 - width,
            };
            batch.text(&self.gui.atlas, &label, ox + x, oy + y as f32, argb(style.color(&text.color)), false);
        }
    }

    /// `UpgradesPanel.drawBackgroundLayer` for one column of `count` slots
    /// (machines have 8 at most), panel top left at `at`.
    fn ae2_draw_upgrades(&self, batch: &mut HudBatch, ox: f32, oy: f32, at: (i32, i32), count: i32) {
        let panel = Image { texture: "guis/extra_panels".into(), texture_size: (128, 128), src: None };
        let (x0, y0) = (at.0 + PANEL_PADDING, at.1 + PANEL_PADDING);
        for i in 0..count {
            let (row, col) = (i % 8, i / 8);
            let (mut x, mut y) = (x0 + col * PANEL_SLOT, y0 + row * PANEL_SLOT);
            let (mut sx, mut sy, mut sw, mut sh) = (PANEL_PADDING, PANEL_PADDING, PANEL_SLOT, PANEL_SLOT);
            // drawSlot: borders on the outer sides.
            if col == 0 {
                x -= PANEL_PADDING;
                sx = 0;
                sw += PANEL_PADDING;
            }
            if i >= count - 8 {
                sw += PANEL_PADDING;
            }
            if row == 0 {
                y -= PANEL_PADDING;
                sy = 0;
                sh += PANEL_PADDING;
            }
            if row + 1 >= 8 || i + 1 >= count {
                sh += PANEL_PADDING + 2;
            }
            self.ae2_blit(batch, &panel, [sx, sy, sw, sh], ox + x as f32, oy + y as f32);
        }
        // The highlights along the column (hLine and vLine, inclusive ends).
        let light = rgba(0xF2, 0xF2, 0xF2, 0xFF);
        let (x, y) = (ox + x0 as f32, oy + y0 as f32);
        let tall = (PANEL_SLOT * count) as f32;
        batch.fill(&self.gui.atlas, x - 4.0, y, 16.0, 1.0, light);
        batch.fill(&self.gui.atlas, x - 4.0, y + tall - 1.0, 16.0, 1.0, light);
        batch.fill(&self.gui.atlas, x - 5.0, y - 1.0, 1.0, tall + 2.0, light);
        batch.fill(&self.gui.atlas, x + 12.0, y - 1.0, 1.0, tall + 2.0, light);
    }

    /// The open AE2 screen for the control socket's `screen`: its buttons in
    /// GUI pixels (the help button first), each toggle's menu entry and
    /// value, the priority tab, and the data slots.
    pub(super) fn ae2_screen_agent_value(&self, origin: (f32, f32)) -> Option<serde_json::Value> {
        let s = self.mod_screen_extra()?.as_ref()?;
        let data = self.mod_screen_data();
        let rect = |[x, y, w, h]: [i32; 4]| [origin.0 + x as f32, origin.1 + y as f32, w as f32, h as f32];
        let toolbar: Vec<serde_json::Value> = s
            .buttons(&data)
            .into_iter()
            .map(|b| match b.toggle {
                None => serde_json::json!({"button": "help", "rect": rect(b.rect)}),
                Some(t) => {
                    let n = value(&data, t.data).clamp(0, t.values.len() as i32 - 1) as usize;
                    serde_json::json!({"button": "setting", "rect": rect(b.rect), "entry": t.entry, "value": t.values[n].0})
                }
            })
            .collect();
        Some(serde_json::json!({
            "style": s.layout.style,
            "toolbar": toolbar,
            "priority": s.priority_tab().map(rect),
            "data": data,
        }))
    }

    /// A press at `at` on the help button (`OpenGuideButton`): the menu
    /// closes and the guide opens at the machine's page. False elsewhere.
    pub(super) fn ae2_screen_help(&mut self, at: [f32; 2]) -> bool {
        let data = self.mod_screen_data();
        let Some(block) = self
            .mod_screen_extra()
            .and_then(|e| e.as_ref())
            .filter(|s| s.buttons(&data).first().is_some_and(|b| inside(b.rect, at)))
            .map(|s| s.block.clone())
        else {
            return false;
        };
        self.close_container();
        if self.ae2_guide_open()
            && let Some(guide) = self.ae2_guide.as_mut()
        {
            guide.show_item(&block);
        }
        true
    }

    /// A press at `at` on the screen: the menu button of the toggle or tab
    /// under it.
    pub(super) fn ae2_screen_button(&self, screen: &Ae2Screen, data: &[i32], at: [f32; 2], right: bool) -> Option<i32> {
        for b in screen.buttons(data) {
            if let Some(t) = b.toggle
                && inside(b.rect, at)
            {
                return Some(if right { t.entry | REVERSE } else { t.entry });
            }
        }
        if let Some(tab) = screen.priority_tab()
            && inside(tab, at)
            && !right
        {
            return screen.layout.priority;
        }
        if !right
            && let Some((_, entry)) = screen.cogs(|e| self.ae2_screen_entry_filled(e)).into_iter().find(|(r, _)| inside(*r, at))
        {
            return Some(entry as i32 | QUICK);
        }
        None
    }

    /// Whether menu entry `entry` of the open container holds something.
    fn ae2_screen_entry_filled(&self, entry: usize) -> bool {
        self.container_kind().is_some_and(|e| e.slots().get(entry).is_some_and(Option::is_some))
    }

    /// The tooltip at `at`: a button's setting and value, the priority tab,
    /// a progress bar's value.
    pub(super) fn ae2_screen_tooltip(&self, screen: &Ae2Screen, data: &[i32], at: [f32; 2], lines: &mut Vec<String>) {
        for b in screen.buttons(data) {
            if !inside(b.rect, at) {
                continue;
            }
            match b.toggle {
                None => {
                    lines.push(self.translate("gui.tooltips.ae2.OpenGuide", "Open Guide"));
                    lines.push(format!("\u{a7}7{}", self.translate("gui.tooltips.ae2.OpenGuideDetail", "")));
                }
                Some(t) => {
                    let (name, _, key) = t.values[value(data, t.data).clamp(0, t.values.len() as i32 - 1) as usize];
                    lines.push(self.translate(t.title, t.title));
                    // The condenser's outputs say how much matter one takes.
                    let amount = match name {
                        "matter_balls" => horizon_client::ae2_common_config::condenser().matter_balls,
                        "singularity" => horizon_client::ae2_common_config::condenser().singularity,
                        _ => 0.0,
                    };
                    let text = self.translate(key, key).replace("%s", &format!("{amount}"));
                    lines.extend(text.split('\n').map(|l| format!("\u{a7}7{l}")));
                }
            }
            return;
        }
        if screen.priority_tab().is_some_and(|tab| inside(tab, at)) {
            lines.push(self.translate("gui.ae2.Priority", "Priority"));
            return;
        }
        if screen.cogs(|e| self.ae2_screen_entry_filled(e)).iter().any(|(r, _)| inside(*r, at)) {
            lines.push(self.translate("gui.tooltips.ae2.InterfaceSetStockAmount", "Set Stock Amount"));
            return;
        }
        if let Some([mode, locked, reason]) = screen.layout.lock_line
            && value(data, mode) != 0
            && value(data, locked) != 0
            && inside([5, 15, 126, 16], at)
        {
            let key = match value(data, reason) {
                2 => "waila.ae2.CraftingLockedByRedstoneSignal",
                3 => "waila.ae2.CraftingLockedByLackOfRedstoneSignal",
                4 => "waila.ae2.CraftingLockedUntilResult",
                _ => "waila.ae2.CraftingLockedUntilPulse",
            };
            lines.push(self.translate(key, key).replace("%s", "?"));
            return;
        }
        let (w, h) = screen.size();
        if let Some(progress) = screen.layout.progress.as_ref()
            && let (Some(image), Some(widget)) = (screen.style.images.get(progress.name), screen.style.widgets.get(progress.name))
        {
            let (x, y) = widget.position.resolve(w, h);
            let (iw, ih) = image.size();
            if inside([x, y, iw, ih], at) {
                let (current, max) = (wide_value(data, progress.current), wide_value(data, progress.max));
                match screen.layout.kind {
                    // VibrationChamberScreen: power per tick and efficiency.
                    MachineKind::VibrationChamber => {
                        lines.push(format!("{} AE/t", trimmed(wide_value(data, 6) as f64 / 100.0)));
                        lines.push(format!("Eff: {}%", value(data, 5)));
                    }
                    // CondenserScreen: "Stored Energy", then "<n> of <max>".
                    MachineKind::Condenser => {
                        lines.push(self.translate("gui.ae2.StoredEnergy", "Stored Energy"));
                        lines.push(format!("{current} {} {max}", self.translate("gui.ae2.Of", "of")));
                    }
                    // Inscriber and Molecular Assembler: "progress%".
                    _ => lines.push(format!("{}%", current * 100 / max.max(1))),
                }
            }
        }
    }
}

/// `DecimalFormat("#.##")`: at most two decimals, none when whole.
fn trimmed(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}
