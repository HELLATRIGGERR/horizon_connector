//! AE2 storage devices; operates only on persisted machine slots and grid cells.
use super::{Game, ae2_fluids, ae2_lab};
use ae2_cell_core::{Action, Cell};
use horizon_client::{ae2_world::MachineKind, containers::BlockEntity, items::ItemStack};
use horizon_world::nbt::{Compound, Tag};
use serde_json::{Value, json};
type Pos = (i32, i32, i32);
fn text(c: &Compound, key: &str) -> Option<String> {
    match c.get(&key.into()) {
        Some(Tag::String(s)) => s.to_string().ok(),
        _ => None,
    }
}
fn is_fluid(s: &ItemStack) -> bool {
    horizon_client::ae2_cell_codec::is_fluid_cell(&s.item)
}
fn read(s: &ItemStack) -> Result<Cell, String> {
    if is_fluid(s) {
        ae2_fluids::read_fluid_cell(s)
    } else {
        ae2_lab::read_cell(s)
    }
}
#[cfg(test)]
use horizon_client::ae2_cell_codec::apply_cell_settings;
/// CreativeCellInventory configured keys have infinite extraction and void
/// insertion; amounts are displayed as Integer.MAX_VALUE by upstream AE2.
pub(super) use horizon_client::ae2_cell_codec::{
    CreativeCellKey, cell_settings, creative_cell_keys,
};

pub(super) fn is_valid_storage_cell(stack: &ItemStack) -> bool {
    stack.count == 1 && (read(stack).is_ok() || creative_cell_keys(stack).is_ok())
}
pub(super) fn is_valid_workbench_cell(stack: &ItemStack) -> bool {
    is_valid_storage_cell(stack) || horizon_client::ae2_cell_codec::read_view_cell(stack).is_ok()
}
/// StorageCell.getIdleDrain of a cell in a Drive or ME Chest, from the cell
/// definitions (AE2's and its addons'): BasicStorageCell idleDrain by size;
/// 0.5 for every PortableCellItem, MatterCannonItem and ColorApplicatorItem.
pub(super) fn cell_idle_drain(stack: &ItemStack) -> Option<f64> {
    horizon_client::ae2_cell_codec::idle_drain(&stack.item)
}
fn write(s: &mut ItemStack, c: &Cell) -> Result<(), String> {
    if is_fluid(s) {
        ae2_fluids::write_fluid_cell(s, c)
    } else {
        ae2_lab::write_cell(s, c)
    }
}
/// FuzzyPriorityList queries KeyCounter with the incoming key. Damageable
/// items use FuzzySearch's integer bounds based on that query's max damage;
/// unordered keys (including all fluids) ignore their components.
fn storage_fuzzy_match(
    filter:&ae2_cell_core::ItemKey,candidate:&ae2_cell_core::ItemKey,
    mode:&str,fluid:bool,base_max:u32,
)->bool {
    if filter.item_id!=candidate.item_id {return false;}
    if fluid {return true;}
    let Ok(filter)=ae2_lab::prototype(filter) else {return false;};
    let Ok(candidate)=ae2_lab::prototype(candidate) else {return false;};
    let max=|stack:&ItemStack| match stack.data().and_then(|d|d.other("minecraft:max_damage")) {
        Some(Tag::Int(n)) if *n>=0=>*n as u32,
        _=>base_max,
    };
    // KeyCounter selected an unordered variant map when the configured key
    // did not support a positive fuzzy range.
    if max(&filter)==0 {return true;}
    let query_max=max(&candidate);
    if query_max==0 {return false;}
    let breakpoint=match mode {
        "ignore_all"=>None,
        "percent_99"=>Some(0),
        "percent_75"=>Some((25f32*query_max as f32/100.0) as u32),
        "percent_50"=>Some((50f32*query_max as f32/100.0) as u32),
        "percent_25"=>Some((75f32*query_max as f32/100.0) as u32),
        _=>return false,
    };
    match breakpoint {
        None=>filter.damage<=query_max,
        Some(point) if candidate.damage<=point=>filter.damage<=point,
        Some(point)=>filter.damage>point && filter.damage<=query_max,
    }
}
impl Game<'_> {
    // Horizon exposes an installed item registry, but not a mod-fluid
    // registry. Preserve all syntactically valid fluid IDs until one exists.
    fn ae2_known_cell_key(&self, id:&str, fluid:bool)->bool {
        fluid || self.res.items().get(id).is_some()
    }
    pub(super) fn ae2_read_item_cell(&self, stack:&ItemStack)->Result<Cell,String> {
        horizon_client::ae2_cell_codec::read_cell_checked(stack,&|id,fluid|self.ae2_known_cell_key(id,fluid))
    }
    pub(super) fn ae2_read_fluid_cell(&self, stack:&ItemStack)->Result<Cell,String> {
        horizon_client::ae2_cell_codec::read_fluid_cell_checked(stack,&|id,fluid|self.ae2_known_cell_key(id,fluid))
    }
    pub(super) fn ae2_read_cell(&self, stack:&ItemStack)->Result<Cell,String> {
        if is_fluid(stack) {self.ae2_read_fluid_cell(stack)} else {self.ae2_read_item_cell(stack)}
    }
    pub(super) fn ae2_creative_cell_keys(&self, stack:&ItemStack)->Result<Vec<CreativeCellKey>,String> {
        horizon_client::ae2_cell_codec::creative_cell_keys_checked(stack,&|id,fluid|self.ae2_known_cell_key(id,fluid))
    }
    fn ae2_checked_cell_settings(&self, stack:&ItemStack)->Result<Compound,String> {
        horizon_client::ae2_cell_codec::cell_settings_checked(stack,&|id,fluid|self.ae2_known_cell_key(id,fluid))
    }
    fn ae2_checked_view_cell(&self, stack:&ItemStack)->Result<horizon_client::ae2_cell_codec::ViewCellSettings,String> {
        horizon_client::ae2_cell_codec::read_view_cell_checked(stack,&|id,fluid|self.ae2_known_cell_key(id,fluid))
    }
    pub(super) fn ae2_workbench_cell_changed(&self, before:&horizon_client::ae2_world::Machine, after:&mut horizon_client::ae2_world::Machine)->Result<(),String> {
        workbench_cell_changed_checked(before,after,&|id,fluid|self.ae2_known_cell_key(id,fluid))
    }
    /// A single source View Cell contributes its raw match and inversion flag;
    /// callers combine configured cells with MergedPriorityList semantics.
    pub(super) fn ae2_view_cell_match(&self, stack: &ItemStack, key: &ae2_cell_core::ItemKey,
        fluid: bool, scope: horizon_client::ae2_cell_codec::ViewCellScope)
        -> Result<horizon_client::ae2_cell_codec::ViewCellMatch, String> {
        let settings = self.ae2_checked_view_cell(stack)?;
        Ok(settings.evaluate(key, fluid, scope, |filter, candidate, mode| {
            let maximum = self.res.items().get(&filter.item_id).map_or(0, |item| item.max_damage);
            storage_fuzzy_match(filter,candidate,mode,false,maximum)
        }))
    }
    /// Resolve AEKey durability partitions using actual stack components and item registry defaults.
    pub(super) fn ae2_cell_insert(
        &self,
        cell: &mut Cell,
        key: &ae2_cell_core::ItemKey,
        amount: u64,
        action: Action,
    ) -> u64 {
        if let Some(tool) = cell.config().tool {
            let Ok(stack) = ae2_lab::prototype(key) else {
                return 0;
            };
            const COLORS: [&str; 16] = [
                "white",
                "orange",
                "magenta",
                "light_blue",
                "yellow",
                "lime",
                "pink",
                "gray",
                "light_gray",
                "cyan",
                "purple",
                "blue",
                "brown",
                "green",
                "red",
                "black",
            ];
            let paint = COLORS.iter().any(|color| {
                stack.item.as_ref() == format!("ae2:{color}_paint_ball")
                    || stack.item.as_ref() == format!("ae2:{color}_lumen_paint_ball")
            });
            let allowed = match tool {
                ae2_cell_core::ToolCell::MatterCannon => {
                    paint
                        || horizon_client::ae2_powered_items::ammo_weight(
                            self.res.recipes(),
                            self.res.items(),
                            &stack,
                        ) > 0.0
                }
                ae2_cell_core::ToolCell::ColorApplicator => {
                    stack.item.as_ref() == "minecraft:snowball"
                        || paint
                        || COLORS.iter().any(|color| {
                            stack.item.as_ref() == format!("minecraft:{color}_dye")
                                || self
                                    .res
                                    .items()
                                    .item_tags
                                    .get(&format!("c:dyes/{color}"))
                                    .is_some_and(|members| members.contains(stack.item.as_ref()))
                        })
                }
            };
            if !allowed {
                return 0;
            }
        }
        if cell.config().channel == ae2_cell_core::Channel::Item
            && !ae2_lab::prototype(key)
                .is_ok_and(|stack| super::ae2_portable_ui::can_store_item(&stack))
        {
            return 0;
        }
        let fluid=cell.config().channel==ae2_cell_core::Channel::Fluid;
        cell.insert_with_fuzzy_match(key, amount, action, |filter, candidate, mode| {
            use ae2_cell_core::FuzzyMode;
            let name = match mode {
                FuzzyMode::IgnoreAll => "ignore_all",
                FuzzyMode::Percent99 => "percent_99",
                FuzzyMode::Percent75 => "percent_75",
                FuzzyMode::Percent50 => "percent_50",
                FuzzyMode::Percent25 => "percent_25",
            };
            let max = self
                .res
                .items()
                .get(&filter.item_id)
                .map_or(0, |item| item.max_damage);
            storage_fuzzy_match(filter,candidate,name,fluid,max)
        })
    }
}
fn completed(c: &Cell, mode: &str) -> bool {
    match mode {
        "empty" => c.total_items() == 0,
        "full" => !c.can_hold_new_type() && c.remaining_items() == 0,
        "half" => true,
        _ => false,
    }
}
/// What IOPortBlockEntity.tickingRequest finds in one input slot.
enum IoSlot<C> {
    Empty,
    NotCell,
    Cell(C),
}
/// IOPortBlockEntity.tickingRequest over the six input slots, `budget`
/// being itemsToMove. A slot without a storage cell is moved to the output
/// (moveSlot) and leaves the result alone. A cell transfers while budget is
/// left (transferContents) and sets the result: IDLE with budget left,
/// URGENT once it ran out; a cell then matching the fullness mode that moves
/// out makes it URGENT. Running out of budget does not end the loop: later
/// non-cells still move, later cells wait. So the last cell that ran
/// decides, and with no cell the result stays SLEEP.
fn io_port_slots<G, C>(
    g: &mut G,
    mut budget: u64,
    slot: impl Fn(&mut G, usize) -> Result<IoSlot<C>, String>,
    transfer: impl Fn(&mut G, usize, &mut C, u64) -> Result<u64, String>,
    done: impl Fn(&C) -> bool,
    move_slot: impl Fn(&mut G, usize) -> Result<bool, String>,
) -> Result<super::ae2_tick_tracker::Modulation, String> {
    use super::ae2_tick_tracker::Modulation;
    let mut ret = Modulation::Sleep;
    for x in 0..6 {
        let mut cell = match slot(g, x)? {
            IoSlot::Empty => continue,
            IoSlot::NotCell => {
                move_slot(g, x)?;
                continue;
            }
            IoSlot::Cell(cell) => cell,
        };
        if budget > 0 {
            budget = transfer(g, x, &mut cell, budget)?;
            ret = if budget > 0 { Modulation::Idle } else { Modulation::Urgent };
        }
        if budget > 0 && done(&cell) && move_slot(g, x)? {
            ret = Modulation::Urgent;
        }
    }
    Ok(ret)
}
/// One pass of IOPortBlockEntity.transferContents over the listed keys, in
/// order. `transfer` gives None for a key the destination does not take,
/// else the operations its attempt used. The first key taken ends the pass,
/// except that in FILL mode a key the network lists but gives none of
/// (`src.extract` returns 0) is passed over.
fn io_transfer_pass<K>(
    keys: Vec<K>,
    fill: bool,
    mut transfer: impl FnMut(K) -> Result<Option<u64>, String>,
) -> Result<u64, String> {
    for key in keys {
        match transfer(key)? {
            Some(used) if used > 0 || !fill => return Ok(used),
            _ => {}
        }
    }
    Ok(0)
}
impl Game<'_> {
    /// Run on the authoritative world tick; source I/O port cadence 1..5 ticks.
    pub(super) fn tick_ae2_storage_devices(&mut self) {
        if self.remote.is_some() {
            return;
        }
        let mut ports: Vec<_> = self
            .world
            .block_entities
            .iter()
            .filter_map(|(p, b)| {
                matches!(b,BlockEntity::Ae2(m) if m.kind==MachineKind::IoPort).then_some(*p)
            })
            .collect();
        ports.sort_unstable();
        for p in ports {
            let Some(BlockEntity::Ae2(machine))=self.world.block_entities.get(&p) else {continue;};
            let has_input=machine.slots.iter().take(6).any(Option::is_some);
            let rs=text(&machine.config,"io_redstone").unwrap_or_else(||"ignore".into());
            let redstone_card=machine.upgrades.iter().flatten().any(|s|s.item.as_ref()=="ae2:redstone_card");
            let token=format!("{:?}|{:?}|{:?}",machine.slots,machine.upgrades,machine.config.iter().filter(|(key,_)|key.to_string_lossy().starts_with("io_") && key.to_string_lossy()!="io_next_tick").collect::<Vec<_>>());
            let enabled=if redstone_card {let at=horizon_core::position::BlockPos::new(p.0,p.1,p.2);let powered=self.with_level(|b,level,ticks|b.has_neighbor_signal(level,ticks,at));match rs.as_str(){"high"=>powered,"low"|"ignore"=>!powered,_=>false}} else {true};
            let token=format!("{token}|{enabled}");
            let (min,max)=horizon_client::ae2_tick_config::rates(horizon_client::ae2_tick_config::Kind::IoPort);
            if !self.ae2_variable_processor_due(p,min,max,!has_input || !enabled,token) {continue;}
            use super::ae2_tick_tracker::Modulation;
            let online=self.ae2_online(p);
            let ticked=if online {self.ae2_io_port_tick(p).ok().flatten()} else {None};
            let has_input=matches!(self.world.block_entities.get(&p),Some(BlockEntity::Ae2(m)) if m.slots.iter().take(6).any(Option::is_some));
            let modulation=match ticked {Some(modulation)=>modulation,None if !online=>Modulation::Idle,None if has_input=>Modulation::Idle,None=>Modulation::Sleep};
            let Some(BlockEntity::Ae2(machine))=self.world.block_entities.get(&p) else {continue;};
            let token=format!("{:?}|{:?}|{:?}|{enabled}",machine.slots,machine.upgrades,machine.config.iter().filter(|(key,_)|key.to_string_lossy().starts_with("io_") && key.to_string_lossy()!="io_next_tick").collect::<Vec<_>>());
            self.ae2_variable_processor_completed(p,modulation,token);
        }
    }
    fn io_snapshot(&self) -> super::ae2_global::BlockSnapshot {
        self.ae2_snapshot_blocks()
    }
    fn io_restore(&mut self, snapshot: super::ae2_global::BlockSnapshot) {
        self.ae2_restore_blocks(snapshot);
    }
    fn ae2_io_port_tick(&mut self, pos: Pos) -> Result<Option<super::ae2_tick_tracker::Modulation>, String> {
        let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get(&pos) else {
            return Err("I/O port disappeared".into());
        };
        if machine.slots.len() != 12 {
            return Err("invalid I/O port slots".into());
        }
        let mode = text(&machine.config, "io_operation").unwrap_or_else(|| "empty".into());
        let fullness = text(&machine.config, "io_fullness").unwrap_or_else(|| "empty".into());
        if !["empty", "fill"].contains(&mode.as_str())
            || !["empty", "full", "half"].contains(&fullness.as_str())
        {
            return Err("invalid I/O settings".into());
        }
        // Redstone-aware execution needs neighbor signal; do not run cards with
        // an unrecognized setting. A card with high/low uses the live world.
        let redstone_card = machine
            .upgrades
            .iter()
            .flatten()
            .any(|s| &*s.item == "ae2:redstone_card");
        let rs = text(&machine.config, "io_redstone").unwrap_or_else(|| "ignore".into());
        let speed = machine.installed_speed_cards().min(3);
        if redstone_card {
            let at = horizon_core::position::BlockPos::new(pos.0, pos.1, pos.2);
            let powered =
                self.with_level(|b, level, ticks| b.has_neighbor_signal(level, ticks, at));
            let enabled = match rs.as_str() {
                "high" => powered,
                "low" | "ignore" => !powered,
                _ => false,
            };
            if !enabled {
                return Ok(None);
            }
        }
        io_port_slots(
            self,
            256u64 * (1u64 << speed),
            |g: &mut Self, slot| {
                let Some(BlockEntity::Ae2(m)) = g.world.block_entities.get(&pos) else {
                    return Err("I/O port disappeared".into());
                };
                let Some(item) = m.slots[slot].clone() else {
                    return Ok(IoSlot::Empty);
                };
                Ok(match g.ae2_read_cell(&item) {
                    Ok(cell) => IoSlot::Cell((item, cell)),
                    Err(_) => IoSlot::NotCell,
                })
            },
            |g: &mut Self, slot, cell: &mut (ItemStack, Cell), operations| {
                g.ae2_io_transfer(pos, slot, cell, &mode, operations)
            },
            |(_, cell): &(ItemStack, Cell)| completed(cell, &fullness),
            |g: &mut Self, slot| g.io_move_output(pos, slot),
        )
        .map(Some)
    }
    /// IOPortBlockEntity.transferContents for the cell in input `slot`;
    /// gives the operations left.
    fn ae2_io_transfer(
        &mut self,
        pos: Pos,
        slot: usize,
        (item, cell): &mut (ItemStack, Cell),
        mode: &str,
        mut operations: u64,
    ) -> Result<u64, String> {
        while operations > 0 {
            let primary_fluid = is_fluid(item);
            let candidates: Vec<(ae2_cell_core::ItemKey, u64, bool, bool)> = if mode == "empty" {
                // Source scans all available keys, including foreign-channel
                // Missing Content; a blocked first key must not hide a later
                // key that the network can accept.
                cell.entries().map(|(k,v)|(k.clone(),*v,primary_fluid,false))
                    .chain(cell.other_entries().map(|(k,v)|(k.clone(),*v,!primary_fluid,true)))
                    .collect()
            } else {
                let listed = if primary_fluid { self.ae2_grid_fluids(pos)? } else { self.ae2_grid_inventory(pos)? };
                listed.into_iter().filter(|(_, n)| *n > 0).map(|(k, n)| (k, n, primary_fluid, false)).collect()
            };
            let used = io_transfer_pass(candidates, mode == "fill", |(key, available, key_fluid, other)| {
                let accepts = if mode == "empty" {
                    if key_fluid {
                        self.ae2_grid_fluid_insert(pos,&key,available,true)? > 0
                    } else {
                        let mut stack=ae2_lab::prototype(&key)?;
                        stack.count=available.min(u64::from(u32::MAX)) as u32;
                        self.ae2_grid_insert_simulate(pos,&stack,u64::from(stack.count))? > 0
                    }
                } else {
                    self.ae2_cell_insert(cell, &key, 1, Action::Simulate) > 0
                };
                if !accepts {
                    return Ok(None);
                }
                let factor = if key_fluid { 125 } else { 1 };
                let max = operations.saturating_mul(factor).min(available);
                let before = self.io_snapshot();
                let mut formation = super::ae2_buses::FormationPlan::default();
                let transfer = (|| -> Result<u64, String> {
                    let moved = if mode == "empty" {
                        if key_fluid {
                            self.ae2_grid_fluid_insert_staged(pos, &key, max, false, &mut formation)?
                        } else {
                            let mut stack = ae2_lab::prototype(&key)?;
                            stack.count = max.min(u32::MAX as u64) as u32;
                            self.ae2_grid_insert_staged(
                                pos,
                                &stack,
                                u64::from(stack.count),
                                &mut formation,
                            )?
                        }
                    } else {
                        let room = self.ae2_cell_insert(cell, &key, max, Action::Simulate);
                        if is_fluid(item) {
                            self.ae2_grid_fluid_extract(pos, &key, room, false)?
                        } else {
                            self.ae2_grid_extract_staged(pos, &key, room, &mut formation)?
                                .map_or(0, |s| u64::from(s.count))
                        }
                    };
                    if moved == 0 {
                        return Ok(0);
                    }
                    let changed = if mode == "empty" {
                        if other {cell.extract_other(&key,moved,Action::Modulate)} else {cell.extract(&key, moved, Action::Modulate)}
                    } else {
                        self.ae2_cell_insert(cell, &key, moved, Action::Modulate)
                    };
                    if changed != moved {
                        return Err("I/O cell refused staged transfer".into());
                    }
                    write(item, cell)?;
                    let Some(BlockEntity::Ae2(m)) = self.world.block_entities.get_mut(&pos) else {
                        return Err("I/O port disappeared".into());
                    };
                    if m.slots.get(slot).is_none() {
                        return Err("I/O input slot disappeared".into());
                    }
                    m.slots[slot] = Some(item.clone());
                    self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
                    Ok(moved)
                })();
                let transfer =
                    transfer.and_then(|moved| self.ae2_commit_formation(formation).map(|_| moved));
                let moved = match transfer {
                    Ok(n) => n,
                    Err(e) => {
                        self.io_restore(before);
                        return Err(e);
                    }
                };
                Ok(Some(if moved == 0 { 0 } else { (moved / factor).max(1) }))
            })?;
            if used == 0 {
                break;
            }
            operations = operations.saturating_sub(used);
        }
        Ok(operations)
    }
    fn io_move_output(&mut self, pos: Pos, slot: usize) -> Result<bool, String> {
        let Some(BlockEntity::Ae2(m)) = self.world.block_entities.get_mut(&pos) else {
            return Err("I/O port disappeared".into());
        };
        if m.slots.len() != 12 || slot >= 6 {
            return Err("invalid I/O slot".into());
        }
        let Some(out) = (6..12).find(|&i| m.slots[i].is_none()) else {
            return Ok(false);
        };
        m.slots[out] = m.slots[slot].take();
        self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
        Ok(true)
    }
    /// Settings are persisted on the physical I/O port; does not create cells.
    pub(super) fn ae2_io_command(&mut self, args: &[&str]) -> Result<Value, String> {
        if args.len() < 4 {
            return Err("usage: /ae2 io status|operation|fullness|redstone x y z [value]".into());
        }
        let pos = (
            args[1].parse().map_err(|_| "invalid x")?,
            args[2].parse().map_err(|_| "invalid y")?,
            args[3].parse().map_err(|_| "invalid z")?,
        );
        let Some(BlockEntity::Ae2(m)) = self.world.block_entities.get_mut(&pos) else {
            return Err("I/O port missing".into());
        };
        if m.kind != MachineKind::IoPort {
            return Err("not an I/O port".into());
        }
        if args[0] == "status" && args.len() == 4 {
            return Ok(
                json!({"operation":text(&m.config,"io_operation").unwrap_or_else(||"empty".into()),"fullness":text(&m.config,"io_fullness").unwrap_or_else(||"empty".into()),"redstone":text(&m.config,"io_redstone").unwrap_or_else(||"ignore".into())}),
            );
        }
        let value = *args.get(4).ok_or("missing setting")?;
        if args.len() != 5 {
            return Err("unexpected setting arguments".into());
        }
        let key = match args[0] {
            "operation" if ["empty", "fill"].contains(&value) => "io_operation",
            "fullness" if ["empty", "full", "half"].contains(&value) => "io_fullness",
            "redstone" if ["ignore", "high", "low"].contains(&value) => "io_redstone",
            _ => return Err("invalid I/O setting".into()),
        };
        m.config.insert(key.into(), Tag::String(value.into()));
        self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
        Ok(json!({"setting":key,"value":value}))
    }
}

/// AE2 condenser buffer is storage-component bytes times eight; input is
/// consumed directly and is not a physical slot.
fn condenser_capacity(machine: &horizon_client::ae2_world::Machine) -> f64 {
    let Some(Some(component)) = machine.slots.get(1) else {
        return 0.0;
    };
    // IStorageComponent.getBytes, from the definitions registry.
    let kb = horizon_client::ae2x::defs::get()
        .component(&component.item)
        .map_or(0, |tier| tier.kilobytes());
    (kb * 1024 * 8) as f64
}
fn condenser_mode(
    machine: &horizon_client::ae2_world::Machine,
) -> Result<(&'static str, f64), String> {
    match text(&machine.config, "condenser_output")
        .as_deref()
        .unwrap_or("trash")
    {
        "trash" => Ok(("", 0.0)),
        "matter_balls" => Ok(("ae2:matter_ball", horizon_client::ae2_common_config::condenser().matter_balls)),
        "singularity" => Ok(("ae2:singularity", horizon_client::ae2_common_config::condenser().singularity)),
        _ => Err("invalid condenser output mode".into()),
    }
}
/// `CondenserMenu`'s bar: the matter stored, and the most it shows
/// (`requiredEnergy`: the output's cost, capped by the component's room, or
/// the room alone when trashing).
pub(super) fn condenser_progress(machine: &horizon_client::ae2_world::Machine) -> (f64, f64) {
    let required = condenser_mode(machine).map_or(0.0, |(_, cost)| cost);
    let capacity = condenser_capacity(machine);
    let most = if required == 0.0 { capacity } else { required.min(capacity) };
    (condenser_power(machine).unwrap_or(0.0), most)
}
fn condenser_power(machine: &horizon_client::ae2_world::Machine) -> Result<f64, String> {
    let power = match machine.config.get(&"condenser_power".into()) {
        Some(Tag::Double(bits)) => f64::from_bits(*bits),
        None => 0.0,
        _ => return Err("invalid condenser power".into()),
    };
    if power.is_finite() && power >= 0.0 {
        Ok(power)
    } else {
        Err("invalid condenser power".into())
    }
}
fn condenser_apply(
    machine: &mut horizon_client::ae2_world::Machine,
    add: f64,
    max_stack: u32,
) -> Result<(), String> {
    if !add.is_finite() || add < 0.0 || machine.slots.len() != 2 {
        return Err("invalid condenser input".into());
    }
    let power=(condenser_power(machine)?+add).min(condenser_capacity(machine));
    machine.config.insert("condenser_power".into(),Tag::Double(power.to_bits()));
    condenser_fill_output(machine,max_stack)
}
/// CondenserMEStorage.insert checks output capacity, accepts the complete
/// generic-stack amount and uses Java integer division for fluid power. This
/// differs from its separate one-bucket FluidHandler, which uses fractions.
fn condenser_me_fluid_insert(machine:&mut horizon_client::ae2_world::Machine,amount:u64,max_stack:u32,simulate:bool)->Result<u64,String> {
    if machine.kind!=horizon_client::ae2_world::MachineKind::Condenser || machine.slots.len()!=2 || amount>i64::MAX as u64 {return Err("invalid condenser ME input".into());}
    let (output,_)=condenser_mode(machine)?;
    if !output.is_empty() && machine.slots[0].as_ref().is_some_and(|stack|
        stack.item.as_ref()!=output || !stack.same_item_same_components(&ItemStack::new(output,1)) || stack.count>=max_stack) {
        return Ok(0);
    }
    if !simulate && amount>0 {condenser_apply(machine,(amount/125) as f64,max_stack)?;}
    Ok(amount)
}
/// Memory Card setting import invokes the same `addPower(0)` callback as a
/// menu mode change. The caller stages the machine before committing it.
pub(super) fn condenser_apply_memory_mode(
    machine:&mut horizon_client::ae2_world::Machine, mode:&str, max_stack:u32,
)->Result<(),String>{
    if machine.kind!=horizon_client::ae2_world::MachineKind::Condenser
        || !["trash","matter_balls","singularity"].contains(&mode) {return Err("invalid Condenser mode".into())}
    machine.config.insert("condenser_output".into(),Tag::String(mode.into()));
    condenser_apply(machine,0.0,max_stack)
}
fn condenser_fill_output(machine:&mut horizon_client::ae2_world::Machine,max_stack:u32)->Result<(),String> {
    if machine.slots.len()!=2 {return Err("invalid condenser slots".into());}
    let (output,required)=condenser_mode(machine)?;
    let mut power=condenser_power(machine)?;
    if required > 0.0 {
        let existing = machine.slots[0].as_ref();
        let space = match existing {
            None => max_stack,
            Some(s)
                if &*s.item == output
                    && s.same_item_same_components(&ItemStack::new(output, 1)) =>
            {
                max_stack.saturating_sub(s.count)
            }
            _ => 0,
        };
        let produced = (power / required).floor().min(f64::from(space)) as u32;
        if produced > 0 {
            if let Some(stack) = machine.slots[0].as_mut() {
                stack.count += produced;
            } else {
                machine.slots[0] = Some(ItemStack::new(output, produced));
            }
            power -= f64::from(produced) * required;
        }
    }
    machine
        .config
        .insert("condenser_power".into(), Tag::Double(power.to_bits()));
    Ok(())
}
/// Apply a committed Condenser consumption to the original runtime object
/// before its cursor or inventory slot is replaced/emptied.
fn condenser_consumed(mut stack: ItemStack, accepted: u32) -> Option<ItemStack> {
    stack.count -= accepted;
    horizon_client::ae2_stack_identity::synchronize_tracked(&stack);
    (stack.count > 0).then_some(stack)
}
impl Game<'_> {
    pub(super) fn ae2_condenser_me_fluid_insert_at(&mut self,address:super::ae2_global::GridAddress,amount:u64,simulate:bool)->u64 {
        let Some((dim,pos))=self.ae2_address_parts(address) else{return 0;};let back=self.dim;
        if self.enter_level(None,dim).is_err(){return 0;}
        let accepted=self.ae2_condenser_me_fluid_insert(pos,amount,simulate).unwrap_or(0);
        let _=self.enter_level(None,back);accepted
    }
    fn ae2_condenser_me_fluid_insert(&mut self,pos:Pos,amount:u64,simulate:bool)->Result<u64,String> {
        let Some(BlockEntity::Ae2(machine))=self.world.block_entities.get(&pos) else{return Err("condenser missing".into());};
        let (output,_)=condenser_mode(machine)?;
        let max=if output.is_empty(){64}else{self.res.items().get(output).ok_or("condenser output item missing")?.max_stack.max(1)};
        let mut staged=(**machine).clone();
        let accepted=condenser_me_fluid_insert(&mut staged,amount,max,simulate)?;
        if accepted>0 && !simulate {
            let Some(BlockEntity::Ae2(machine))=self.world.block_entities.get_mut(&pos) else{return Err("condenser disappeared".into());};
            **machine=staged;self.world.modified.insert((pos.0>>4,pos.2>>4));
        }
        Ok(accepted)
    }
    pub(super) fn ae2_condenser_insert_at(&mut self,address:super::ae2_global::GridAddress,stack:&ItemStack,limit:u32,simulate:bool)->u32 {
        let Some((dim,pos))=self.ae2_address_parts(address) else {return 0;};let back=self.dim;
        if self.enter_level(None,dim).is_err(){return 0;}
        let accepted=self.ae2_condenser_insert(pos,stack,u64::from(limit),simulate).unwrap_or(0).min(u64::from(limit)) as u32;
        let _=self.enter_level(None,back);accepted
    }
    pub(super) fn ae2_condenser_fill_at(&mut self,address:super::ae2_global::GridAddress,amount:u64,simulate:bool)->u64 {
        let Some((dim,pos))=self.ae2_address_parts(address) else {return 0;};let back=self.dim;
        if self.enter_level(None,dim).is_err(){return 0;}
        let accepted=self.ae2_condenser_fill(pos,amount,simulate).unwrap_or(0);
        let _=self.enter_level(None,back);accepted
    }
    pub(super) fn ae2_condenser_extract_at(&mut self,address:super::ae2_global::GridAddress,limit:u32,filter:Option<&ItemStack>,simulate:bool)->Option<ItemStack> {
        let Some(BlockEntity::Ae2(machine))=self.ae2_block_entity(address) else {return None;};
        if machine.kind!=MachineKind::Condenser || limit==0 {return None;}
        let stack=machine.slots.first()?.as_ref()?.clone();
        if filter.is_some_and(|filter|!filter.same_item_same_components(&stack)){return None;}
        let amount=limit.min(stack.count);if amount==0{return None;}
        let out=ItemStack{count:amount,..stack.clone()};
        if !simulate {
            let (dim,pos)=self.ae2_address_parts(address)?;let back=self.dim;
            self.enter_level(None,dim).ok()?;
            if let Some(BlockEntity::Ae2(machine))=self.world.block_entities.get_mut(&pos) {
                if machine.slots.first().and_then(Option::as_ref)!=Some(&stack){let _=self.enter_level(None,back);return None;}
                machine.slots[0]=(stack.count>amount).then(||ItemStack{count:stack.count-amount,..stack});
            } else {let _=self.enter_level(None,back);return None;}
            self.ae2_condenser_output_changed(pos);self.container_changed(pos);
            let _=self.enter_level(None,back);
        }
        Some(out)
    }
    pub(super) fn ae2_condenser_menu(&self,pos:Pos)->Option<Vec<Option<ItemStack>>> {
        let Some(BlockEntity::Ae2(machine))=self.world.block_entities.get(&pos) else {return None;};
        if machine.kind!=MachineKind::Condenser {return None;}
        let mut slots=vec![None;27];
        slots[1]=machine.slots.first().cloned().flatten();
        slots[2]=machine.slots.get(1).cloned().flatten();
        let label=|text:String| {let mut item=ItemStack::new("minecraft:arrow",1);item.data_mut().custom_name=Some(horizon_client::item_data::text_name(text));Some(item)};
        let mode=text(&machine.config,"condenser_output").unwrap_or_else(||"trash".into());
        slots[24]=label("Trash input: first slot; cells and components are excluded".into());
        let required=condenser_mode(machine).ok()?.1;let capacity=condenser_capacity(machine);
        let maximum=if required==0.0 {capacity}else{required.min(capacity)};
        slots[25]=label(format!("Stored: {} / {} matter",condenser_power(machine).ok()?,maximum));
        slots[26]=label(format!("Output: {mode}; click to change"));
        Some(slots)
    }
    pub(super) fn ae2_condenser_trash_allowed(stack:&ItemStack)->bool {
        !horizon_client::ae2_cell_codec::is_cell(&stack.item)
            && horizon_client::ae2x::defs::get().component(&stack.item).is_none()
            && !matches!(stack.item.as_ref(),"ae2:matter_cannon"|"ae2:color_applicator"|"ae2:creative_storage_cell")
    }
    pub(super) fn ae2_condenser_cursor_input(&mut self,pos:Pos,one:bool) {
        let Some(stack)=self.carried.as_ref().filter(|s|Self::ae2_condenser_trash_allowed(s)).cloned() else {return;};
        let amount=if one {1}else{u64::from(stack.count)};
        let Ok(accepted)=self.ae2_condenser_insert(pos,&stack,amount,false) else {return;};
        if accepted>u64::from(stack.count) {return;}
        self.carried=condenser_consumed(stack,accepted as u32);
    }
    pub(super) fn ae2_condenser_shift_input(&mut self,pos:Pos,slot:usize)->bool {
        let Some(stack)=self.inventory.get(slot).and_then(Option::as_ref).filter(|s|Self::ae2_condenser_trash_allowed(s)).cloned() else {return false;};
        let Ok(accepted)=self.ae2_condenser_insert(pos,&stack,u64::from(stack.count),false) else {return true;};
        if accepted<=u64::from(stack.count) {self.inventory[slot]=condenser_consumed(stack,accepted as u32);}
        true
    }
    pub(super) fn ae2_condenser_cycle(&mut self,pos:Pos,reverse:bool) {
        let modes=["trash","matter_balls","singularity"];
        let Some(BlockEntity::Ae2(machine))=self.world.block_entities.get(&pos) else {return;};
        let current=text(&machine.config,"condenser_output").unwrap_or_else(||"trash".into());
        let index=modes.iter().position(|mode|*mode==current).unwrap_or(0);
        let mode=modes[(index+if reverse {2}else{1})%3];
        let args=["output".to_owned(),pos.0.to_string(),pos.1.to_string(),pos.2.to_string(),mode.to_owned()];
        let words:Vec<_>=args.iter().map(String::as_str).collect();let _=self.ae2_condenser_command(&words);
    }
    pub(super) fn ae2_condenser_output_changed(&mut self,pos:Pos) {
        let Some(BlockEntity::Ae2(machine))=self.world.block_entities.get(&pos) else {return;};
        let mut updated=(**machine).clone();let Ok((output,_))=condenser_mode(&updated) else {return;};
        let max=self.res.items().get(output).map_or(64,|item|item.max_stack);
        if condenser_fill_output(&mut updated,max).is_ok() && updated!=**machine {
            if let Some(BlockEntity::Ae2(machine))=self.world.block_entities.get_mut(&pos) {**machine=updated;self.world.modified.insert((pos.0>>4,pos.2>>4));}
        }
    }
    /// Accept items into the virtual trash input; the caller must remove exactly
    /// the returned count from its real inventory after successful commit.
    pub(super) fn ae2_condenser_insert(
        &mut self,
        pos: Pos,
        input: &ItemStack,
        amount: u64,
        simulate: bool,
    ) -> Result<u64, String> {
        let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get(&pos) else {
            return Err("condenser missing".into());
        };
        if machine.kind != MachineKind::Condenser || machine.slots.len() != 2 {
            return Err("not a condenser".into());
        }
        let (output, _) = condenser_mode(machine)?;
        let max = if output.is_empty() {
            64
        } else {
            self.res
                .items()
                .get(output)
                .ok_or("condenser output item missing")?
                .max_stack
                .max(1)
        };
        if !output.is_empty()
            && machine.slots[0].as_ref().is_some_and(|s| {
                &*s.item != output
                    || !s.same_item_same_components(&ItemStack::new(output, 1))
                    || s.count >= max
            })
        {
            return Ok(0);
        }
        let accepted = amount.min(u64::from(input.count));
        let mut staged = (**machine).clone();
        condenser_apply(&mut staged, accepted as f64, max)?;
        if !simulate && accepted > 0 {
            let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get_mut(&pos) else {
                return Err("condenser disappeared".into());
            };
            **machine = staged;
            self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
        }
        Ok(accepted)
    }
    /// Condenser fluid handler accepts up to one bucket per call and converts
    /// mB to matter at AEFluidKey's 125 mB per operation. The caller must debit
    /// actual fluid; simulated calls do not mutate the machine.
    pub(super) fn ae2_condenser_fill(
        &mut self,
        pos: Pos,
        amount_mb: u64,
        simulate: bool,
    ) -> Result<u64, String> {
        let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get(&pos) else {
            return Err("condenser missing".into());
        };
        if machine.kind != MachineKind::Condenser || machine.slots.len() != 2 {
            return Err("not a condenser".into());
        }
        let (output, _) = condenser_mode(machine)?;
        let max = if output.is_empty() {
            64
        } else {
            self.res
                .items()
                .get(output)
                .ok_or("condenser output item missing")?
                .max_stack
                .max(1)
        };
        let accepted = amount_mb.min(1000);
        let mut staged = (**machine).clone();
        condenser_apply(&mut staged, accepted as f64 / 125.0, max)?;
        if !simulate && accepted > 0 {
            let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get_mut(&pos) else {
                return Err("condenser disappeared".into());
            };
            **machine = staged;
            self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
        }
        Ok(accepted)
    }
    /// /ae2 condenser status|output|insert X Y Z [mode or count]
    pub(super) fn ae2_condenser_command(&mut self, args: &[&str]) -> Result<Value, String> {
        if args.len() < 4 {
            return Err("usage: /ae2 condenser status|output|insert|fluid x y z [value]".into());
        }
        let pos = (
            args[1].parse().map_err(|_| "invalid x")?,
            args[2].parse().map_err(|_| "invalid y")?,
            args[3].parse().map_err(|_| "invalid z")?,
        );
        match args[0] {
            "status" if args.len() == 4 => {
                let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get(&pos) else {
                    return Err("condenser missing".into());
                };
                if machine.kind != MachineKind::Condenser {
                    return Err("not a condenser".into());
                }
                Ok(
                    json!({"mode":text(&machine.config,"condenser_output").unwrap_or_else(||"trash".into()),"stored":condenser_power(machine)?,"capacity":condenser_capacity(machine)}),
                )
            }
            "output" if args.len() == 5 => {
                let value = args[4];
                if !["trash", "matter_balls", "singularity"].contains(&value) {
                    return Err("invalid condenser output".into());
                }
                let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get(&pos) else {
                    return Err("condenser missing".into());
                };
                if machine.kind != MachineKind::Condenser {
                    return Err("not a condenser".into());
                }
                let mut staged = (**machine).clone();
                staged
                    .config
                    .insert("condenser_output".into(), Tag::String(value.into()));
                let (output, _) = condenser_mode(&staged)?;
                let max = if output.is_empty() {
                    64
                } else {
                    self.res
                        .items()
                        .get(output)
                        .ok_or("condenser output item missing")?
                        .max_stack
                        .max(1)
                };
                condenser_apply(&mut staged, 0.0, max)?;
                let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get_mut(&pos)
                else {
                    return Err("condenser disappeared".into());
                };
                **machine = staged;
                self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
                Ok(json!({"output":value}))
            }
            "fluid" if args.len() == 4 => {
                let source = self.inventory[self.selected]
                    .clone()
                    .ok_or("hold a water or lava bucket")?;
                let id = ae2_fluids::bucket_fluid(&source.item)
                    .ok_or("only water and lava buckets are supported")?;
                if !ae2_fluids::plain_bucket(&source) {
                    return Err("modified bucket cannot be safely converted".into());
                }
                if self.ae2_condenser_fill(pos, 1000, true)? != 1000 {
                    return Err("condenser refused fluid".into());
                }
                let before = self.inventory.clone();
                self.exchange_bucket(&source, "minecraft:bucket")?;
                match self.ae2_condenser_fill(pos, 1000, false) {
                    Ok(1000) => Ok(json!({"fluid":id,"consumed_mb":1000})),
                    Ok(_) => {
                        self.inventory = before;
                        Err("condenser changed before transfer".into())
                    }
                    Err(e) => {
                        self.inventory = before;
                        Err(e)
                    }
                }
            }
            "insert" if (4..=5).contains(&args.len()) => {
                let source = self.inventory[self.selected]
                    .clone()
                    .ok_or("hold an item to condense")?;
                let requested = if let Some(raw) = args.get(4) {
                    raw.parse::<u64>().map_err(|_| "invalid item count")?
                } else {
                    u64::from(source.count)
                };
                let accepted = self.ae2_condenser_insert(pos, &source, requested, false)?;
                let remain = source.count - accepted as u32;
                self.inventory[self.selected] = (remain > 0).then_some(ItemStack {
                    count: remain,
                    ..source
                });
                Ok(json!({"consumed":accepted}))
            }
            _ => Err("usage: /ae2 condenser status|output|insert|fluid x y z [value]".into()),
        }
    }
}
#[cfg(test)]
mod storage_tests {
    use super::*;
    use horizon_client::ae2_world::{Machine, MachineKind};
    use super::super::ae2_tick_tracker::Modulation;
    #[test]
    fn drive_and_chest_idle_drain_follows_each_cell_item() {
        let drain = |id: &str| cell_idle_drain(&ItemStack::new(id, 1));
        // BasicStorageCell idleDrain by size (AEItems.ITEM_CELL_*, FLUID_CELL_*).
        assert_eq!(drain("ae2:item_storage_cell_1k"), Some(0.5));
        assert_eq!(drain("ae2:item_storage_cell_256k"), Some(2.5));
        assert_eq!(drain("ae2:fluid_storage_cell_64k"), Some(2.0));
        // PortableCellItem.getIdleDrain is 0.5 for every size, so a Drive
        // with ten 256k portable cells draws 2 + 10 x 0.5 = 7 AE/t.
        for size in ["1k", "4k", "16k", "64k", "256k"] {
            assert_eq!(drain(&format!("ae2:portable_item_cell_{size}")), Some(0.5));
            assert_eq!(drain(&format!("ae2:portable_fluid_cell_{size}")), Some(0.5));
        }
        assert_eq!(drain("ae2:matter_cannon"), Some(0.5));
        // CreativeCellInventory.getIdleDrain is 0.
        assert_eq!(drain("ae2:creative_item_cell"), None);
    }
    /// Six I/O Port input slots for io_port_slots: Some(n) is a cell holding
    /// n items, None a non-cell. The network takes up to `room` items.
    struct Port {
        inputs: [Option<Option<u64>>; 6],
        outputs: Vec<Option<u64>>,
        room: u64,
    }
    fn empty_cells(port: &mut Port, budget: u64) -> Modulation {
        io_port_slots(
            port,
            budget,
            |p: &mut Port, x| Ok(match p.inputs[x] {
                None => IoSlot::Empty,
                Some(None) => IoSlot::NotCell,
                Some(Some(n)) => IoSlot::Cell(n),
            }),
            |p: &mut Port, x, n: &mut u64, budget| {
                let moved = (*n).min(budget).min(p.room);
                p.room -= moved;
                *n -= moved;
                p.inputs[x] = Some(Some(*n));
                Ok(budget - moved)
            },
            |n: &u64| *n == 0,
            |p: &mut Port, x| Ok(p.outputs.len() < 6 && {
                p.outputs.push(p.inputs[x].take().unwrap());
                true
            }),
        )
        .unwrap()
    }
    #[test]
    fn io_port_keeps_moving_non_cells_after_the_budget_runs_out() {
        // IOPortBlockEntity.tickingRequest: once itemsToMove is 0 the loop
        // still visits every slot; a non-cell moves to the output, a cell waits.
        let mut port = Port { inputs: [Some(Some(1000)), Some(None), Some(Some(5)), None, None, None], outputs: Vec::new(), room: u64::MAX };
        assert_eq!(empty_cells(&mut port, 256), Modulation::Urgent);
        assert_eq!(port.inputs[..3], [Some(Some(744)), None, Some(Some(5))]);
        assert_eq!(port.outputs, [None]);
    }
    #[test]
    fn io_port_tick_rate_follows_the_last_cell() {
        // `ret` is overwritten by each cell that still had budget: the first
        // cell finishes and moves out (URGENT), the second stops with budget
        // left because the network is full (IDLE), and IDLE is returned.
        let mut port = Port { inputs: [Some(Some(10)), None, Some(Some(20)), None, None, None], outputs: Vec::new(), room: 15 };
        assert_eq!(empty_cells(&mut port, 256), Modulation::Idle);
        assert_eq!(port.outputs, [Some(0)]);
        assert_eq!(port.inputs[2], Some(Some(15)));
        // Moving a non-cell leaves `ret` at SLEEP, whether or not it fits.
        let mut port = Port { inputs: [Some(None), None, None, None, None, None], outputs: Vec::new(), room: 0 };
        assert_eq!(empty_cells(&mut port, 256), Modulation::Sleep);
        assert_eq!(port.outputs, [None]);
        let mut port = Port { inputs: [Some(None), None, None, None, None, None], outputs: vec![Some(0); 6], room: 0 };
        assert_eq!(empty_cells(&mut port, 256), Modulation::Sleep);
        assert_eq!(port.inputs[0], Some(None));
    }
    #[test]
    fn io_port_fill_passes_over_a_listed_key_the_network_does_not_give_up() {
        // IOPortBlockEntity.transferContents, FILL: when networkInv.extract
        // gives 0 for a listed key (a Storage Bus with filter NONE lists a
        // furnace's fuel that its sided handler keeps), the next key is tried.
        let mut tried = Vec::new();
        let used = io_transfer_pass(vec!["fuel", "cobblestone", "dirt"], true, |key| {
            tried.push(key);
            Ok(Some(if key == "fuel" { 0 } else { 64 }))
        });
        assert_eq!(used, Ok(64));
        assert_eq!(tried, ["fuel", "cobblestone"]);
        // A key the cell does not take is passed over too.
        assert_eq!(io_transfer_pass(vec!["refused", "dirt"], true, |key| Ok((key == "dirt").then_some(3))), Ok(3));
        // EMPTY: the first key the network takes ends the pass even if
        // poweredInsert stored nothing (didStuff stays false).
        let mut tried = Vec::new();
        let used = io_transfer_pass(vec!["refused", "taken", "later"], false, |key| {
            tried.push(key);
            Ok(match key { "refused" => None, "taken" => Some(0), _ => Some(9) })
        });
        assert_eq!(used, Ok(0));
        assert_eq!(tried, ["refused", "taken"]);
    }
    #[test]
    fn condenser_me_fluid_obeys_source_output_and_integer_amount_rules() {
        let mut machine=Machine::new(MachineKind::Condenser);
        machine.slots[1]=Some(ItemStack::new("ae2:cell_component_1k",1));
        machine.config.insert("condenser_output".into(),Tag::String("matter_balls".into()));
        let before=machine.clone();
        assert_eq!(condenser_me_fluid_insert(&mut machine,1377,64,true).unwrap(),1377);assert_eq!(machine,before);
        assert_eq!(condenser_me_fluid_insert(&mut machine,1377,64,false).unwrap(),1377);assert_eq!(condenser_power(&machine).unwrap(),11.0);
        assert_eq!(condenser_me_fluid_insert(&mut machine,124,64,false).unwrap(),124);assert_eq!(condenser_power(&machine).unwrap(),11.0);
        machine.slots[0]=Some(ItemStack::new("ae2:matter_ball",64));let blocked=machine.clone();
        assert_eq!(condenser_me_fluid_insert(&mut machine,1377,64,false).unwrap(),0);assert_eq!(machine,blocked);
        machine.config.insert("condenser_output".into(),Tag::String("trash".into()));
        assert_eq!(condenser_me_fluid_insert(&mut machine,1377,64,false).unwrap(),1377);
        assert_eq!(condenser_power(&machine).unwrap(),22.0);
    }
    #[test]
    fn fuzzy_cell_partitions_follow_candidate_range_and_fluid_identity() {
        let fluid=|components|ae2_cell_core::ItemKey{item_id:"minecraft:water".into(),components};
        for mode in ["ignore_all","percent_99","percent_75","percent_50","percent_25"] {
            assert!(storage_fuzzy_match(&fluid(vec![0,1]),&fluid(vec![2,3]),mode,true,0));
        }
        let mut config=ae2_cell_core::CellConfig::fluid(ae2_cell_core::Tier::K1);
        config.fuzzy=true;
        config.fuzzy_mode=ae2_cell_core::FuzzyMode::Percent50;
        config.filter=Some(ae2_cell_core::Filter {
            mode:ae2_cell_core::FilterMode::Whitelist,
            keys:std::collections::BTreeSet::from([fluid(vec![0,1])]),
        });
        let mut cell=Cell::new(config);
        assert_eq!(cell.insert_with_fuzzy_match(&fluid(vec![2,3]),1000,Action::Modulate,
            |filter,candidate,_|storage_fuzzy_match(filter,candidate,"percent_50",true,0)),1000);
        let item=|damage,max_damage| {
            let mut stack=ItemStack::new("minecraft:test_tool",1);
            stack.damage=damage;
            stack.data_mut().set_other("minecraft:max_damage",Some(Tag::Int(max_damage)));
            ae2_lab::key(&stack).unwrap()
        };
        let filter=item(75,100);
        let candidate=item(120,200);
        assert!(!storage_fuzzy_match(&filter,&candidate,"percent_50",false,100));
        assert!(storage_fuzzy_match(&item(120,100),&candidate,"percent_50",false,100));
        assert!(!storage_fuzzy_match(&item(201,300),&candidate,"ignore_all",false,100));
        assert!(storage_fuzzy_match(&item(0,0),&candidate,"percent_50",false,0));
    }
    #[test]
    fn cell_settings_validate_physical_cards_and_ghost_filter() {
        let mut item = ItemStack::new("ae2:item_storage_cell_1k", 1);
        let card = horizon_client::save::item_to_nbt(&ItemStack::new("ae2:inverter_card", 1));
        let filter = Tag::compound([(
            "item",
            horizon_client::save::item_to_nbt(&ItemStack::new("minecraft:stone", 1)),
        )]);
        let settings = match Tag::compound([
            ("upgrades", Tag::List(vec![card.clone()])),
            ("filters", Tag::List(vec![filter])),
        ]) {
            Tag::Compound(m) => m,
            _ => unreachable!(),
        };
        set_cell_settings(&mut item, settings).unwrap();
        let mut config = ae2_cell_core::CellConfig::item(ae2_cell_core::Tier::K1);
        apply_cell_settings(&item, &mut config).unwrap();
        let mut cell = Cell::new(config);
        assert_eq!(
            cell.insert(
                &ae2_lab::key(&ItemStack::new("minecraft:stone", 1)).unwrap(),
                1,
                Action::Simulate
            ),
            0
        );
        assert_eq!(
            cell.insert(
                &ae2_lab::key(&ItemStack::new("minecraft:dirt", 1)).unwrap(),
                1,
                Action::Simulate
            ),
            1
        );
        let mut bad = cell_settings(&item).unwrap();
        bad.insert("upgrades".into(), Tag::List(vec![card.clone(), card]));
        set_cell_settings(&mut item, bad).unwrap();
        let mut config = ae2_cell_core::CellConfig::item(ae2_cell_core::Tier::K1);
        assert!(apply_cell_settings(&item, &mut config).is_err());
    }
    #[test]
    fn condenser_respects_component_capacity_and_output_backpressure() {
        let mut m = Machine::new(MachineKind::Condenser);
        m.slots[1] = Some(ItemStack::new("ae2:cell_component_1k", 1));
        m.config.insert(
            "condenser_output".into(),
            Tag::String("matter_balls".into()),
        );
        condenser_apply(&mut m, 512.0, 64).unwrap();
        assert_eq!(m.slots[0].as_ref().unwrap().count, 2);
        assert_eq!(condenser_power(&m).unwrap(), 0.0);
        m.slots[0].as_mut().unwrap().count = 64;
        condenser_apply(&mut m, 9000.0, 64).unwrap();
        assert_eq!(condenser_power(&m).unwrap(), 8192.0);
        assert_eq!(m.slots[0].as_ref().unwrap().count, 64);
    }
    #[test]
    fn condenser_consumption_updates_a_retained_original_before_removing_its_slot() {
        let mut stack=ItemStack::new("minecraft:stone",4);
        let retained=horizon_client::ae2_stack_identity::StackRef::capture(&mut stack);
        let remainder=condenser_consumed(stack,1).unwrap();
        assert_eq!(retained.read().count,3);
        assert!(retained.tracks(&remainder));
        assert!(condenser_consumed(remainder,3).is_none());
        assert_eq!(retained.read().count,0);
    }
    #[test]
    fn condenser_uses_actual_runtime_singularity_boundary_and_refills_without_reclamping() {
        let mut m=Machine::new(MachineKind::Condenser);
        m.slots[1]=Some(ItemStack::new("ae2:cell_component_64k",1));
        m.config.insert("condenser_output".into(),Tag::String("singularity".into()));
        condenser_apply(&mut m,255999.0,64).unwrap();assert!(m.slots[0].is_none());
        condenser_apply(&mut m,1.0,64).unwrap();assert_eq!(m.slots[0].as_ref().unwrap().item.as_ref(),"ae2:singularity");assert_eq!(condenser_power(&m).unwrap(),0.0);
        m.config.insert("condenser_output".into(),Tag::String("matter_balls".into()));
        m.config.insert("condenser_power".into(),Tag::Double(512.0f64.to_bits()));m.slots[0]=None;m.slots[1]=None;
        condenser_fill_output(&mut m,64).unwrap();assert_eq!(m.slots[0].as_ref().unwrap().count,2);assert_eq!(condenser_power(&m).unwrap(),0.0);
        for id in ["ae2:item_storage_cell_1k","ae2:portable_item_cell_1k","ae2:creative_storage_cell","ae2:matter_cannon","ae2:cell_component_1k"] {assert!(!Game::ae2_condenser_trash_allowed(&ItemStack::new(id,1)),"{id}");}
        assert!(Game::ae2_condenser_trash_allowed(&ItemStack::new("minecraft:cobblestone",64)));
    }
    /// MEGA Cells' cells go in drives and chests, and its components power a
    /// Condenser as AE2's do (IStorageComponent: bytes × 8).
    #[cfg(feature = "ae2-megacells")]
    #[test]
    fn mega_cells_are_storage_cells_and_mega_components_condenser_storage() {
        for size in ["1m","4m","16m","64m","256m"] {
            for kind in ["item","fluid"] {
                let cell=ItemStack::new(&format!("megacells:{kind}_storage_cell_{size}"),1);
                assert!(is_valid_storage_cell(&cell)&&is_valid_workbench_cell(&cell),"{}",cell.item);
                assert_eq!(is_fluid(&cell),kind=="fluid");
                assert!(!Game::ae2_condenser_trash_allowed(&cell));
            }
            assert!(!Game::ae2_condenser_trash_allowed(&ItemStack::new(&format!("megacells:cell_component_{size}"),1)));
        }
        assert!(!is_valid_storage_cell(&ItemStack::new("megacells:item_storage_cell_1m",2)),"a stack of two is not a cell");
        assert!(!is_valid_storage_cell(&ItemStack::new("megacells:cell_component_1m",1)));
        let mut m=Machine::new(MachineKind::Condenser);
        m.slots[1]=Some(ItemStack::new("megacells:cell_component_256m",1));
        assert_eq!(condenser_capacity(&m),(262_144u64*1024*8) as f64);
        m.slots[1]=Some(ItemStack::new("megacells:cell_component_1m",1));
        assert_eq!(condenser_capacity(&m),8_388_608.0);
    }
}

/// Apply physical cards and ghost partition entries stored on the cell item.
/// A cell item retains its cards in its own NBT when moved or dropped.

fn set_cell_settings(stack: &mut ItemStack, settings: Compound) -> Result<(), String> {
    let Some(Tag::List(cards))=settings.get(&"upgrades".into()) else{return Err("invalid cell upgrades".into());};
    let max=horizon_client::ae2_upgrade_inventory::capacity(&stack.item).ok_or("unsupported upgrade owner")?;
    let slots=horizon_client::ae2_upgrade_inventory::decode(cards,max)?;
    let source_cards=horizon_client::ae2_components::normalize("ae2:upgrades",Tag::List(horizon_client::ae2_upgrade_inventory::encode_source(&slots)?))?;
    let Some(Tag::List(filters))=settings.get(&"filters".into()) else{return Err("invalid cell filters".into());};
    let source_filters=match horizon_client::ae2_cell_codec::encode_source_filters(filters) {
        Ok(filters)=>Some(Tag::List(filters)),
        Err(error) if stack.data().and_then(|data|data.other("ae2:storage_cell_config_inv")).is_some()=>return Err(error),
        Err(_)=>None,
    };
    let source_mode=match settings.get(&"fuzzy_mode".into()) {
        None=>None,
        Some(Tag::String(mode))=>{
            let mode=mode.to_string().map_err(|_|"invalid fuzzy mode")?.to_ascii_uppercase();
            if !["IGNORE_ALL","PERCENT_99","PERCENT_75","PERCENT_50","PERCENT_25"].contains(&mode.as_str()) {return Err("invalid fuzzy mode".into());}
            Some(Tag::String(mode.as_str().into()))
        }
        _=>return Err("invalid fuzzy mode".into()),
    };
    let mut data = match stack.data().and_then(|d| d.other("minecraft:custom_data")) {
        None => Compound::new(),
        Some(Tag::Compound(m)) => m.clone(),
        Some(_) => return Err("invalid cell custom data".into()),
    };
    data.insert("ae2_cell_settings".into(), Tag::Compound(settings));
    stack
        .data_mut()
        .set_other("minecraft:custom_data", Some(Tag::Compound(data)));
    stack.data_mut().set_other("ae2:upgrades",Some(source_cards));
    stack.data_mut().set_other("ae2:storage_cell_fuzzy_mode",source_mode);
    if let Some(filters)=source_filters {stack.data_mut().set_other("ae2:storage_cell_config_inv",Some(filters));}
    Ok(())
}
impl Game<'_> {
    /// Workbench slot zero contains a real cell. Cards are physical; filters are ghost keys.
    pub(super) fn ae2_workbench_command(&mut self, args: &[&str]) -> Result<Value, String> {
        if args.len() < 4 {
            return Err("usage: /ae2 workbench status|install|remove|filter_add|filter_remove|filter_clear x y z [index]".into());
        }
        let pos = (
            args[1].parse().map_err(|_| "invalid x")?,
            args[2].parse().map_err(|_| "invalid y")?,
            args[3].parse().map_err(|_| "invalid z")?,
        );
        let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get(&pos) else {
            return Err("workbench missing".into());
        };
        if machine.kind != MachineKind::CellWorkbench {
            return Err("not a cell workbench".into());
        }
        let mut cell = machine
            .slots
            .first()
            .and_then(Clone::clone)
            .ok_or("put a cell in the workbench")?;
        let fluid = is_fluid(&cell);
        if !is_valid_workbench_cell(&cell) {
            return Err("unsupported workbench cell".into());
        }
        let mut settings = self.ae2_checked_cell_settings(&cell)?;
        if args[0] == "status" && args.len() == 4 {
            let cards = match settings.get(&"upgrades".into()) {
                Some(Tag::List(v)) => v,
                _ => return Err("invalid cell upgrades".into()),
            };
            let filters = match settings.get(&"filters".into()) {
                Some(Tag::List(v)) => v,
                _ => return Err("invalid cell filters".into()),
            };
            let max=horizon_client::ae2_upgrade_inventory::capacity(&cell.item).ok_or("unsupported upgrade owner")?;
            let card_rows=horizon_client::ae2_upgrade_inventory::decode(cards,max)?.iter().enumerate().filter_map(|(i,t)|t.as_ref().map(|t|json!({"index":i,"item":t.item.as_ref()}))).collect::<Vec<_>>();
            let slots = workbench_decode(filters)?;
            let filter_rows=slots.iter().enumerate().filter_map(|(i,t)|t.as_ref().map(|t|json!({"index":i,"item":t.get("item").and_then(|x|x.get("id")).and_then(|x|if let Tag::String(s)=x{s.to_string().ok()}else{None}),"fluid":t.get("fluid").and_then(|x|if let Tag::String(s)=x{s.to_string().ok()}else{None})}))).collect::<Vec<_>>();
            return Ok(
                json!({"cell":cell.item.as_ref(),"upgrades":card_rows,"filters":filter_rows}),
            );
        }
        let mut remove_held = false;
        let mut return_card = None;
        match args[0] {
            "install" if args.len() == 4 => {
                let held = self.inventory[self.selected]
                    .clone()
                    .ok_or("hold a cell upgrade card")?;
                if held.count == 0 {
                    return Err("invalid held count".into());
                };
                let Some(Tag::List(cards)) = settings.get_mut(&"upgrades".into()) else {
                    return Err("invalid cell upgrades".into());
                };
                let max=horizon_client::ae2_upgrade_inventory::capacity(&cell.item).ok_or("unsupported upgrade owner")?;
                let mut slots=horizon_client::ae2_upgrade_inventory::decode(cards,max)?;
                let slot=slots.iter_mut().find(|s|s.is_none()).ok_or("no free cell upgrade slot")?;
                *slot=Some(ItemStack{count:1,..held});
                *cards=horizon_client::ae2_upgrade_inventory::encode(&slots)?;
                remove_held = true;
            }
            "remove" if args.len() == 5 => {
                let index: usize = args[4].parse().map_err(|_| "invalid card index")?;
                let Some(Tag::List(cards)) = settings.get_mut(&"upgrades".into()) else {
                    return Err("invalid cell upgrades".into());
                };
                let max=horizon_client::ae2_upgrade_inventory::capacity(&cell.item).ok_or("unsupported upgrade owner")?;
                let mut slots=horizon_client::ae2_upgrade_inventory::decode(cards,max)?;
                return_card=Some(slots.get_mut(index).and_then(Option::take).ok_or("card slot is empty or out of range")?);
                *cards=horizon_client::ae2_upgrade_inventory::encode(&slots)?;
            }
            "filter_add" | "filter_add_fluid" if args.len() == 4 => {
                let held = self.inventory[self.selected]
                    .as_ref()
                    .ok_or("hold an item or fluid bucket for a filter")?;
                let fluid_mode=fluid || (cell.item.as_ref() == "ae2:view_cell" && args[0] == "filter_add_fluid");
                let filter = workbench_ghost_from_carried(Some(&cell),held,fluid_mode,&|id,fluid|self.ae2_known_cell_key(id,fluid))?;
                let Some(Tag::List(filters)) = settings.get_mut(&"filters".into()) else {
                    return Err("invalid cell filters".into());
                };
                let mut slots=workbench_decode(filters)?;
                let slot=slots.iter_mut().find(|slot|slot.is_none()).ok_or("no free filter slot")?;
                *slot=Some(filter);
                *filters=horizon_client::ae2_cell_codec::encode_view_filter_slots(&slots)?;
            }
            "filter_remove" if args.len() == 5 => {
                let index: usize = args[4].parse().map_err(|_| "invalid filter index")?;
                let Some(Tag::List(filters)) = settings.get_mut(&"filters".into()) else {
                    return Err("invalid cell filters".into());
                };
                let mut slots=workbench_decode(filters)?;
                slots.get_mut(index).and_then(Option::take).ok_or("empty or invalid filter slot")?;
                *filters=horizon_client::ae2_cell_codec::encode_view_filter_slots(&slots)?;
            }
            "fuzzy"
                if args.len() == 5
                    && [
                        "ignore_all",
                        "percent_99",
                        "percent_75",
                        "percent_50",
                        "percent_25",
                    ]
                    .contains(&args[4]) =>
            {
                settings.insert("fuzzy_mode".into(), Tag::String(args[4].into()));
            }
            "filter_clear" if args.len() == 4 => {
                settings.insert("filters".into(), Tag::List(vec![]));
            }
            _ => return Err("invalid workbench operation".into()),
        }
        set_cell_settings(&mut cell, settings)?;
        if remove_held || return_card.is_some() {
            let upgrades=horizon_client::ae2_upgrade_inventory::Inventory::read(&cell)?;
            upgrades.store(&mut cell)?;
        }
        // Validate settings against this exact cell's profile, including tool
        // upgrade slots and card types, before exchanging physical cards.
        if cell.item.as_ref() == "ae2:view_cell" {
            self.ae2_checked_view_cell(&cell)?;
        } else {
            self.ae2_read_cell(&cell)?;
        }
        // Finish validating and encoding the template before moving any
        // physical cards or mutating the workbench.
        let slots = match self.ae2_checked_cell_settings(&cell)?.get(&"filters".into()) {
            Some(Tag::List(filters)) => workbench_decode(filters)?,
            _ => return Err("invalid cell filters".into()),
        };
        let encoded_filters = horizon_client::ae2_cell_codec::encode_view_filter_slots(&slots)?;
        let before = self.inventory.clone();
        if let Some(card) = return_card {
            if self.give(card).is_some() {
                self.inventory = before;
                return Err("inventory full".into());
            }
        }
        if remove_held {
            let held = self.inventory[self.selected]
                .clone()
                .ok_or("held card disappeared")?;
            self.inventory[self.selected] = if held.count == 1 {
                None
            } else {
                Some(ItemStack {
                    count: held.count - 1,
                    ..held
                })
            };
        }
        let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get_mut(&pos) else {
            self.inventory = before;
            return Err("workbench disappeared".into());
        };
        machine.slots[0] = Some(cell);
        machine.config.insert(WORKBENCH_FILTERS.into(), Tag::List(encoded_filters));
        self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
        Ok(json!({"configured":true}))
    }
}

include!("ae2_workbench.rs");

// Staged replacement for the two Cell Workbench menu methods.
impl Game<'_> {
    pub(super) fn ae2_workbench_menu_slots(&self,pos:Pos,page:usize)->Option<Vec<Option<ItemStack>>>{
        let BlockEntity::Ae2(machine)=self.world.block_entities.get(&pos)? else{return None};
        if machine.kind!=MachineKind::CellWorkbench{return None}
        let mut slots=vec![None;27];slots[0]=machine.slots.first()?.clone();
        let cell=slots[0].clone();
        let settings=cell.as_ref().map(|cell|self.ae2_checked_cell_settings(cell)).transpose().ok()?;
        if let (Some(cell),Some(settings))=(cell.as_ref(),settings.as_ref()){
            if let Some(Tag::List(cards))=settings.get(&"upgrades".into()){
                let max=horizon_client::ae2_upgrade_inventory::capacity(&cell.item)?;
                for (i,card) in horizon_client::ae2_upgrade_inventory::decode(cards,max).ok()?.into_iter().enumerate().take(4){slots[1+i]=card;}
            }
        }
        if cell.is_some()||workbench_keep(machine){
            let entries=workbench_filters_checked(machine,&|id,fluid|self.ae2_known_cell_key(id,fluid)).ok()?;
            for (i,entry) in entries.iter().skip(page.min(3)*18).take(18).enumerate(){
                let Some(entry)=entry else{continue};
                slots[5+i]=if let Some(item)=entry.get("item").and_then(horizon_client::save::item_from_nbt){Some(item)}
                else if let (Some(Tag::String(id)),Some(Tag::ByteArray(components)))=(entry.get("fluid"),entry.get("components")) {
                    id.to_string().ok().and_then(|id| {
                        let value=horizon_client::ae2_generic_stack::GenericStack {key:ae2_cell_core::ItemKey{item_id:id.clone(),components:components.clone()},fluid:true,amount:0};
                        horizon_client::ae2_generic_stack::wrap(&value).ok().or_else(||ae2_fluids::fluid_bucket(&id).map(|id|ItemStack::new(id,1)))
                    })
                } else{None};
            }
        }
        let label=|item:&str,name:String|{let mut s=ItemStack::new(item,1);s.data_mut().custom_name=Some(horizon_client::item_data::text_name(name));Some(s)};
        let fuzzy=settings.as_ref().and_then(|s|text(s,"fuzzy_mode")).unwrap_or_else(||"ignore_all".into());
        let mode=if workbench_keep(machine){"keep on remove"}else{"clear on remove"};
        slots[23]=label("minecraft:comparator",format!("Fuzzy: {fuzzy}; left cycle, right copy mode: {mode}"));
        slots[24]=label("minecraft:barrier","Left: clear filters; right: partition from cell".into());
        if page>0{slots[25]=label("minecraft:arrow","Previous filter page".into());}
        if page<3{slots[26]=label("minecraft:arrow","Next filter page".into());}
        Some(slots)
    }
    /// Ghost filters and controls commit to the workbench plus the exact
    /// inserted cell. Physical cell/player/cursor ownership remains external.
    pub(super) fn ae2_workbench_menu_click(&mut self,pos:Pos,page:&mut usize,slot:Option<usize>,button:i32,input:horizon_client::menu::Input)->bool{
        let Some(slot)=slot.filter(|s|(301..327).contains(s))else{return false};
        let Some(BlockEntity::Ae2(machine))=self.world.block_entities.get(&pos)else{return false};
        if machine.kind!=MachineKind::CellWorkbench{return false}
        if input!=horizon_client::menu::Input::Pickup||!(0..=1).contains(&button){return true}
        if slot==325{*page=page.saturating_sub(1);return true}
        if slot==326{*page=(*page+1).min(3);return true}
        let original=(**machine).clone();let mut next=original.clone();let mut next_cursor=self.carried.clone();
        let old_upgrades=next.slots.first().and_then(Option::as_ref).and_then(|cell|horizon_client::ae2_upgrade_inventory::Inventory::read(cell).ok().map(|v|v.slots));
        let result:Result<(),String>=(||{
            match slot {
                323 if button==1=>{
                    let value=if workbench_keep(&next){"clear_on_remove"}else{"keep_on_remove"};
                    next.config.insert(WORKBENCH_MODE.into(),Tag::String(value.into()));
                }
                323=>{
                    let Some(cell)=next.slots.first_mut().and_then(Option::as_mut)else{return Ok(())};
                    let mut settings=self.ae2_checked_cell_settings(cell)?;
                    let modes=["ignore_all","percent_99","percent_75","percent_50","percent_25"];
                    let old=text(&settings,"fuzzy_mode").unwrap_or_else(||"ignore_all".into());
                    let i=modes.iter().position(|v|*v==old).unwrap_or(0);
                    settings.insert("fuzzy_mode".into(),Tag::String(modes[(i+1)%modes.len()].into()));set_cell_settings(cell,settings)?;
                    if !is_valid_workbench_cell(cell){return Err("invalid fuzzy setting".into())}
                }
                324 if button==1=>workbench_partition_checked(&mut next,&|id,fluid|self.ae2_known_cell_key(id,fluid))?,
                324=>{
                    let mut filters=vec![None;63];
                    if let Some(cell)=next.slots.first_mut().and_then(Option::as_mut){filters=workbench_apply_filters_checked(cell,&filters,&|id,fluid|self.ae2_known_cell_key(id,fluid))?;}
                    workbench_store_filters(&mut next,&filters)?;
                }
                301..=304=>{
                    let Some(cell)=next.slots.first_mut().and_then(Option::as_mut)else{return Ok(())};
                    let mut settings=self.ae2_checked_cell_settings(cell)?;
                    let Some(Tag::List(cards))=settings.get_mut(&"upgrades".into())else{return Err("invalid upgrades".into())};
                    let max=horizon_client::ae2_upgrade_inventory::capacity(&cell.item).ok_or("invalid upgrade owner")?;
                    let mut cards=horizon_client::ae2_upgrade_inventory::decode(cards,max)?;
                    let Some(card_slot)=cards.get_mut(slot-301)else{return Ok(())};let old=card_slot.clone();
                    match next_cursor.take(){
                        None if old.is_some()=>{next_cursor=card_slot.take();}
                        Some(mut held) if held.count>0&&(old.is_none()||held.count==1)=>{
                            *card_slot=Some(ItemStack{count:1,..held.clone()});held.count-=1;
                            next_cursor=old.or_else(||(held.count>0).then_some(held));
                        }
                        other=>next_cursor=other,
                    }
                    settings.insert("upgrades".into(),Tag::List(horizon_client::ae2_upgrade_inventory::encode(&cards)?));
                    set_cell_settings(cell,settings)?;
                    if !is_valid_workbench_cell(cell){return Err("invalid upgrade".into())}
                    let upgrades=horizon_client::ae2_upgrade_inventory::Inventory::read(cell)?;
                    if old_upgrades.as_ref()!=Some(&upgrades.slots){upgrades.store(cell)?;}
                }
                305..=322=>{
                    if next.slots.first().and_then(Option::as_ref).is_none()&&!workbench_keep(&next){return Ok(())}
                    let mut filters=workbench_filters_checked(&next,&|id,fluid|self.ae2_known_cell_key(id,fluid))?;let index=(*page).min(3)*18+(slot-305);
                    if index>=63{return Ok(())}
                    let fluid=workbench_emptying_mode(next.slots.first().and_then(Option::as_ref),button);
                    filters[index]=if let Some(held)=next_cursor.as_ref(){
                        match workbench_ghost_from_carried(next.slots.first().and_then(Option::as_ref),held,fluid,&|id,fluid|self.ae2_known_cell_key(id,fluid)) {
                            Ok(ghost)=>Some(ghost),Err(_)=>return Ok(()),
                        }
                    }else{None};
                    if let Some(cell)=next.slots.first_mut().and_then(Option::as_mut){filters=workbench_apply_filters_checked(cell,&filters,&|id,fluid|self.ae2_known_cell_key(id,fluid))?;}
                    workbench_store_filters(&mut next,&filters)?;
                }
                _=>return Ok(()),
            }
            Ok(())
        })();
        if result.is_err(){return true}
        let Some(BlockEntity::Ae2(current))=self.world.block_entities.get_mut(&pos)else{return true};
        if **current!=original{return true}
        **current=next;self.carried=next_cursor;self.world.modified.insert((pos.0>>4,pos.2>>4));true
    }
}
