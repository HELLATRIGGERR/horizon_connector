//! Local ME terminal: virtual views of authoritative drive-cell storage.
//! No display stack is ever stored in a block entity or treated as an item.
use super::{Game, ae2_lab};
use horizon_client::{items::ItemStack, menu::Input, text::Text};
use horizon_protocol::packets::play::{self, Clientbound as C};
use horizon_world::nbt::Tag;
use std::sync::Arc;

const PAGE_ITEMS: usize = 45;
const PREV: usize = 45;
const NEXT: usize = 53;

fn navigation(label: &str) -> ItemStack {
    let mut stack = ItemStack {
        item: Arc::from("minecraft:arrow"),
        count: 1,
        damage: 0,
        extra: None,
    };
    stack.data_mut().custom_name = Some(label.into());
    stack
}
fn encoding_item_icon(item:&ItemStack,max:u32)->Option<ItemStack> {
    if item.count<=max.max(1) {return Some(item.clone());}
    let key=horizon_client::ae2_cell_codec::key(item).ok()?;
    horizon_client::ae2_generic_stack::wrap(&horizon_client::ae2_generic_stack::GenericStack{key,fluid:false,amount:i64::from(item.count)}).ok()
}
fn encoding_fluid_icon(fluid:&super::ae2_processing::PatternFluid)->Option<ItemStack> {
    let value=horizon_client::ae2_generic_stack::GenericStack{key:fluid.key.clone(),fluid:true,amount:i64::try_from(fluid.amount).ok()?};
    horizon_client::ae2_generic_stack::wrap(&value).ok().or_else(||{
        let id=super::ae2_fluids::fluid_bucket(&fluid.key.item_id).unwrap_or("minecraft:bucket");
        let mut item=ItemStack::new(id,1);item.data_mut().custom_name=Some(format!("{} mB {}",fluid.amount,fluid.key.item_id));Some(item)
    })
}
/// AEBaseScreen's right-click EMPTY_ITEM action sets the fluid represented by
/// a supported physical bucket without consuming the carried container.
fn encoding_emptying_action(held:Option<&ItemStack>,button:i32)->Option<super::ae2_processing::PatternFluid> {
    // The filter action is a simulation and never mutates the carrier, so
    // custom names or unrelated item components do not change a vanilla
    // water/lava bucket's contained fluid key.
    let held=held.filter(|_|button==1)?;
    let id=super::ae2_fluids::bucket_fluid(&held.item)?;
    Some(super::ae2_processing::PatternFluid{key:super::ae2_fluids::fluid_key(id),amount:1000})
}
fn encoding_selected_fluid(held:Option<&ItemStack>,amount:u64,selected:usize,fluids:&[(ae2_cell_core::ItemKey,u64)])->Option<super::ae2_processing::PatternFluid> {
    if amount==0 {return None;}
    let key=held.and_then(|s|super::ae2_fluids::bucket_fluid(&s.item))
        .map(super::ae2_fluids::fluid_key)
        .or_else(||fluids.get(selected%fluids.len().max(1)).map(|(key,_)|key.clone()))?;
    Some(super::ae2_processing::PatternFluid{key,amount})
}
fn fluid_marker(key: &ae2_cell_core::ItemKey) -> ae2_cell_core::ItemKey {
    ae2_cell_core::ItemKey {
        item_id: format!("horizon:crafting_fluid/{}", key.item_id),
        components: key.components.clone(),
    }
}
fn marker_fluid(key: &ae2_cell_core::ItemKey) -> Option<ae2_cell_core::ItemKey> {
    Some(ae2_cell_core::ItemKey {
        item_id: key
            .item_id
            .strip_prefix("horizon:crafting_fluid/")?
            .to_owned(),
        components: key.components.clone(),
    })
}
/// RepoSearch splits on `|`, then ANDs whitespace-separated typed terms.
/// These facts are built from a real item/fluid key, never its display icon.
struct SearchFacts<'a> {
    id:&'a str,
    name:&'a str,
    mod_id:&'a str,
    mod_name:&'a str,
    tooltip:&'a str,
    tags:&'a [String],
}
fn terminal_search_matches(query:&str,facts:&SearchFacts<'_>)->bool {
    let query=query.to_lowercase();
    if query.trim().is_empty() {return true;}
    let mut branches:Vec<_>=query.split('|').collect();
    // Java String.split discards trailing empty branches.
    while branches.last().is_some_and(|branch|branch.is_empty()) {branches.pop();}
    let id=facts.id.to_lowercase();let name=facts.name.to_lowercase();
    let mod_id=facts.mod_id.to_lowercase();let mod_name=facts.mod_name.to_lowercase();
    let tooltip=facts.tooltip.to_lowercase().replace(' ',"");
    branches.into_iter().any(|branch| branch.split_whitespace().all(|term| {
        if let Some(term)=term.strip_prefix('@') {mod_id.contains(term)||mod_name.contains(term)}
        else if let Some(term)=term.strip_prefix('$') {tooltip.contains(&term.replace(' ',""))}
        else if let Some(term)=term.strip_prefix('#') {facts.tags.iter().any(|tag|tag.to_lowercase().contains(term))}
        else if let Some(term)=term.strip_prefix('*') {id.contains(term)}
        else {name.contains(term)}
    }))
}
fn search_language_code(code:&str)->&str {
    if !code.is_empty() && code.len()<=16 && code.bytes().all(|c|c.is_ascii_lowercase()||c.is_ascii_digit()||c==b'_') {code} else {"en_us"}
}
fn terminal_filter_entries(
    entries: &[(ae2_cell_core::ItemKey, u64)],
    craftable: &[ae2_cell_core::ItemKey],
    sort: u8,
    descending: bool,
    filter: u8,
    search: &str,
    display_name:&dyn Fn(&ae2_cell_core::ItemKey)->String,
    matches_search:&dyn Fn(&ae2_cell_core::ItemKey,&str)->bool,
) -> Vec<(ae2_cell_core::ItemKey, u64)> {
    let mut out: Vec<_> = entries
        .iter()
        .filter(|(key, amount)| {
            matches_search(key,search)
                && match filter {
                    1 => *amount > 0,
                    2 => craftable.contains(key),
                    _ => true,
                }
        })
        .cloned()
        .collect();
    if sort == 2 {
        // AE2's MOD mode groups by mod id, then case-insensitive display name.
        out.sort_by_cached_key(|(key, _)| {
            let id = key.item_id.strip_prefix("horizon:crafting_fluid/").unwrap_or(&key.item_id);
            let namespace = id.split_once(':').map_or("minecraft", |(namespace, _)| namespace);
            let name = display_name(key);
            (namespace.to_lowercase(), name.to_lowercase())
        });
    } else if sort == 1 {
        let units=|(key,amount):&(ae2_cell_core::ItemKey,u64)|*amount as f64 / if marker_fluid(key).is_some(){1000.0}else{1.0};
        out.sort_by(|a,b|units(a).total_cmp(&units(b)));
    } else {
        out.sort_by_cached_key(|(key,_)|display_name(key).to_lowercase());
    }
    if descending {
        out.reverse();
    }
    out
}
fn terminal_key_types(
    entries:&[(ae2_cell_core::ItemKey,u64)], enabled:(bool,bool),
) -> Vec<(ae2_cell_core::ItemKey,u64)> {
    entries.iter().filter(|(key,_)|if marker_fluid(key).is_some(){enabled.1}else{enabled.0}).cloned().collect()
}
fn wireless_key_types(host:&super::lan::LanHost,conn:u32,ctx:Option<&super::ae2_wireless::WirelessContext>)->(bool,bool) {
    let Some(ctx)=ctx else{return (true,true);};
    host.hub.guests.iter().find(|guest|guest.conn==conn)
        .and_then(|guest|toolbox_stack(guest,ctx.source))
        .and_then(|stack|super::ae2_wireless::enabled_key_types(stack).ok())
        .unwrap_or((false,false))
}
/// Repo.addEntriesToView places pinned keys in a separate first row, ahead of
/// search and view filters. Repeat that row on each native page and reserve
/// empty positions so slot clicks keep the same server-side key mapping.
fn terminal_pinned_entries(
    all:&[(ae2_cell_core::ItemKey,u64)], filtered:Vec<(ae2_cell_core::ItemKey,u64)>,
    pins:&[ae2_cell_core::ItemKey], page_size:usize,
) -> Vec<(ae2_cell_core::ItemKey,u64)> {
    if pins.is_empty() || page_size<=9 {return filtered;}
    let mut chosen=Vec::new();
    for pin in pins.iter().take(9) {
        if !chosen.iter().any(|(key,_)|key==pin) {
            if let Some(entry)=all.iter().find(|(key,_)|key==pin) {chosen.push(entry.clone());}
        }
    }
    if chosen.is_empty(){return filtered;}
    let rest:Vec<_>=filtered.into_iter().filter(|(key,_)|!chosen.iter().any(|(pinned,_)|pinned==key)).collect();
    let gap=(ae2_cell_core::ItemKey{item_id:String::new(),components:Vec::new()},0);
    let mut out=Vec::new();
    let pages=rest.len().max(1).div_ceil(page_size-9);
    for page in 0..pages {
        out.extend(chosen.iter().cloned());
        out.extend(std::iter::repeat_n(gap.clone(),9-chosen.len()));
        out.extend(rest.iter().skip(page*(page_size-9)).take(page_size-9).cloned());
    }
    out
}
fn same(a: &ItemStack, b: &ItemStack) -> bool {
    a.item == b.item && a.damage == b.damage && a.extra == b.extra
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ChestTerminalSettings {
    sort: u8,
    view: u8,
    descending: bool,
}

#[derive(Clone, Debug)]
struct PriorityEdit { value: i32, text: String, token:u64 }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct InterfaceMenuOwner { host:u64, part:Option<[i32;4]> }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MachineMenuOwner { host:u64, part:Option<[i32;4]>, kind:horizon_client::ae2_world::MachineKind }
#[derive(Clone, Debug, PartialEq, Eq)]
enum StockKey { Item(ae2_cell_core::ItemKey), Fluid(ae2_cell_core::ItemKey) }
#[derive(Clone, Debug)]
struct StockEdit { slot:usize, owner:InterfaceMenuOwner, key:StockKey, max:i64, unit:u32, text:String, token:u64, display:ItemStack }
#[derive(Clone,Debug)]
enum ProcessingAmountKey { Item(ItemStack), Fluid(ae2_cell_core::ItemKey) }
#[derive(Clone,Debug)]
struct ProcessingAmountEdit { part:super::ae2_parts::PartAddress, identity:(u64,[i32;4]), index:usize, output:bool,
    key:ProcessingAmountKey, max:i64, unit:u32, text:String, token:u64, display:ItemStack }
static NEXT_NUMBER_EDITOR:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(1);
// EditBox strips this control character, so ordinary terminal searches and
// anvil names cannot collide with the internal RenameItem envelope.
const PRIORITY_WIRE_PREFIX:&str="\u{1f}ae2p:";
pub(super) fn priority_transport_value(token:u64,text:&str)->String {format!("{PRIORITY_WIRE_PREFIX}{token}:{text}")}
fn priority_transport_parse(value:&str)->Option<(u64,&str)> {
    let (token,text)=value.strip_prefix(PRIORITY_WIRE_PREFIX)?.split_once(':')?;Some((token.parse().ok()?,text))
}
fn priority_transport_for(value:&str,expected:u64)->Option<&str> {
    let (token,text)=priority_transport_parse(value)?;(token==expected).then_some(text)
}
pub(super) fn priority_confirmable(text:&str)->bool {
    super::ae2_number_entry::number_value(text,1,i64::from(i32::MIN),i64::MAX,'.').is_some()
}
fn priority_step(text:&str,slot:i16,shift:bool)->Option<String> {
    if !(0..8).contains(&slot){return None;}
    let steps=if shift {[1i64,16,32,64]}else{[1i64,10,100,1000]};
    let delta=if slot<4 {steps[slot as usize]}else{-steps[(slot as usize)%4]};
    super::ae2_number_entry::number_step(text,delta,i64::from(i32::MIN),i64::MAX,'.')
}

fn chest_terminal_settings(machine: &horizon_client::ae2_world::Machine) -> ChestTerminalSettings {
    let setting = |key: &str| match machine.config.get(&key.into()) {
        Some(Tag::String(value)) => value.to_string().ok(),
        _ => None,
    };
    ChestTerminalSettings {
        sort: match setting("sort_by").as_deref() { Some("amount") => 1, Some("mod") => 2, _ => 0 },
        view: match setting("view_mode").as_deref() { Some("stored") => 1, Some("craftable") => 2, _ => 0 },
        descending: setting("sort_direction").as_deref() == Some("descending"),
    }
}

fn chest_terminal_setting_click(machine: &mut horizon_client::ae2_world::Machine, slot: i16, button: i8) -> bool {
    if machine.kind != horizon_client::ae2_world::MachineKind::Chest || !matches!(button, 0 | 1) { return false; }
    let current = chest_terminal_settings(machine);
    let step = if button == 0 { 1 } else { 2 };
    let (key, value) = match slot {
        46 => ("sort_by", ["name", "amount", "mod"][((current.sort + step) % 3) as usize]),
        47 => ("view_mode", ["all", "stored", "craftable"][((current.view + step) % 3) as usize]),
        51 => ("sort_direction", if current.descending { "ascending" } else { "descending" }),
        _ => return false,
    };
    machine.config.insert(key.into(), Tag::String(value.into()));
    true
}

pub(super) fn ae2_hand_menu_kind(kind:horizon_client::ae2_world::MachineKind)->bool {
    use horizon_client::ae2_world::MachineKind as K;
    matches!(kind,K::Drive|K::Chest|K::Terminal|K::VibrationChamber|K::Inscriber|K::Interface|K::PatternProvider|K::MolecularAssembler|K::CellWorkbench|K::QuantumLink|K::SpatialIoPort|K::IoPort|K::Condenser|K::WirelessAccessPoint|K::SkyStoneChest|K::SmoothSkyStoneChest)
}
impl Game<'_> {
    fn ae2_terminal_display_name(&self,key:&ae2_cell_core::ItemKey,language:&horizon_client::lang::Language)->String {
        let fluid=marker_fluid(key);
        let id=fluid.as_ref().map_or(key.item_id.as_str(),|fluid|fluid.item_id.as_str());
        let (namespace,path)=id.split_once(':').unwrap_or(("minecraft",id));
        if let Some(stack)=fluid.is_none().then(||ae2_lab::prototype(key).ok()).flatten() {
            let text=stack.custom_name_component().map(|tag|horizon_protocol::text::plain(tag,language.map()))
                .or_else(||stack.custom_name().map(str::to_owned))
                .or_else(||stack.data().and_then(|data|data.other("minecraft:item_name")).map(|tag|horizon_protocol::text::plain(tag,language.map())))
                .or_else(||language.get(&format!("item.{namespace}.{path}")).or_else(||language.get(&format!("block.{namespace}.{path}"))).map(str::to_owned))
                .unwrap_or_else(||self.pack.names.item(id));
            self.ae2_item_display_name(&stack,&text)
        } else {
            language.get(&format!("fluid.{namespace}.{path}")).or_else(||language.get(&format!("block.{namespace}.{path}")))
                .map(str::to_owned).unwrap_or_else(||path.replace('_'," "))
        }
    }
    fn ae2_terminal_search_matches(&self,key:&ae2_cell_core::ItemKey,query:&str,include_mod:bool,language:&horizon_client::lang::Language)->bool {
        if query.trim().is_empty() {return true;}
        let fluid=marker_fluid(key);
        let id=fluid.as_ref().map_or(key.item_id.as_str(),|fluid|fluid.item_id.as_str());
        let (namespace,_path)=id.split_once(':').unwrap_or(("minecraft",id));
        let mod_name=match namespace {"minecraft"=>"Minecraft","ae2"=>"Applied Energistics 2",other=>other};
        let stack=if fluid.is_none() {ae2_lab::prototype(key).ok()}else{None};
        let display=self.ae2_terminal_display_name(key,language);
        let mut tooltip=display.clone();
        if query.contains('$') {
            if let Some(stack)=stack.as_ref() {
                // The native tooltip renderer is process-local. Its other
                // generated lines are safe to use when it is in the same
                // language; component lore is resolved for each guest below.
                let same_language=horizon_client::lang::current().map_or(language.code=="en_us",|current|current.code==language.code);
                if same_language {
                    for (line,_) in self.stack_tooltip(stack).into_iter().skip(1) {tooltip.push('\n');tooltip.push_str(&line.plain());}
                }
                if let Some(Tag::List(lines))=stack.data().and_then(|data|data.other("minecraft:lore")) {
                    for line in lines {tooltip.push('\n');tooltip.push_str(&horizon_protocol::text::plain(line,language.map()));}
                }
            }
            if include_mod {tooltip.push('\n');tooltip.push_str(mod_name);}
        }
        let tags=if query.contains('#') {
            let all=if fluid.is_some() {&self.res.recipes().fluid_tags}else{&self.res.items().item_tags};
            all.iter().filter(|(_,values)|values.contains(id)).map(|(tag,_)|tag.clone()).collect::<Vec<_>>()
        }else{Vec::new()};
        terminal_search_matches(query,&SearchFacts{id,name:&display,mod_id:namespace,mod_name,tooltip:&tooltip,tags:&tags})
    }
    fn ae2_view_filtered(&self,entries:&[(ae2_cell_core::ItemKey,u64)],part:Option<super::ae2_parts::PartAddress>)->Vec<(ae2_cell_core::ItemKey,u64)> {
        entries.iter().filter(|(key,_)|self.ae2_view_permits(key,part,horizon_client::ae2_cell_codec::ViewCellScope::All)).cloned().collect()
    }
    fn ae2_view_permits(&self,key:&ae2_cell_core::ItemKey,part:Option<super::ae2_parts::PartAddress>,scope:horizon_client::ae2_cell_codec::ViewCellScope)->bool {
        let Some(part)=part else{return true;};let Some(machine)=self.ae2_part_machine(part).ok().flatten() else{return false;};
        let Ok(cells)=super::ae2_parts::ae2_terminal_view_cells(&machine) else{return false;};
        let fluid=marker_fluid(key);let key=fluid.as_ref().unwrap_or(key);let mut positive=false;let mut matched=false;
        for cell in cells.iter().flatten() {
            let Ok(m)=self.ae2_view_cell_match(cell,key,fluid.is_some(),scope) else{return false;};
            if !m.configured {continue;}if m.inverted && m.matches {return false;}if !m.inverted {positive=true;matched|=m.matches;}
        }
        !positive || matched
    }
    fn ae2_local_chest(&self, pos: (i32, i32, i32)) -> bool {
        matches!(self.world.block_entities.get(&pos), Some(horizon_client::containers::BlockEntity::Ae2(m)) if m.kind == horizon_client::ae2_world::MachineKind::Chest)
    }
    fn chest_terminal_read(&self, pos: (i32, i32, i32)) -> Option<(u64, ChestTerminalSettings)> {
        let identity = self.world.block_entities.incarnation(&pos)?;
        let horizon_client::containers::BlockEntity::Ae2(machine) = self.world.block_entities.get(&pos)? else { return None; };
        (machine.kind == horizon_client::ae2_world::MachineKind::Chest).then(|| (identity, chest_terminal_settings(machine)))
    }
    fn chest_terminal_write(&mut self, pos: (i32, i32, i32), identity: u64, slot: i16, button: i8) -> Option<ChestTerminalSettings> {
        if self.world.block_entities.incarnation(&pos) != Some(identity) { return None; }
        let horizon_client::containers::BlockEntity::Ae2(machine) = self.world.block_entities.get_mut(&pos)? else { return None; };
        if !chest_terminal_setting_click(machine, slot, button) { return None; }
        let settings = chest_terminal_settings(machine);
        self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
        Some(settings)
    }
    fn ae2_terminal_powered(&self, pos: (i32, i32, i32)) -> bool {
        self.ae2_terminal_online(pos)
            || matches!(self.world.block_entities.get(&pos), Some(horizon_client::containers::BlockEntity::Ae2(m)) if m.kind == horizon_client::ae2_world::MachineKind::Chest && m.energy > 1.0)
    }
    /// ME chest has its own cell, even when connected to a larger grid.
    /// Stage cell and power changes before committing either of them.
    fn ae2_chest_transfer(
        &mut self,
        pos: (i32, i32, i32),
        key: &ae2_cell_core::ItemKey,
        limit: u64,
        insert: bool,
    ) -> Result<u64, String> {
        use ae2_cell_core::{Action,Channel};
        use horizon_client::containers::BlockEntity;
        let Some(BlockEntity::Ae2(m)) = self.world.block_entities.get(&pos) else {
            return Err("missing ME chest".into());
        };
        let mut stack = match m.slots.first().and_then(Option::as_ref) {
            Some(s) => s.clone(),
            None => return Ok(0),
        };
        let usage = horizon_client::ae2_common_config::gameplay().usage_multiplier;
        let battery = m.energy.max(0.0) / usage;
        let fluid = marker_fluid(key);
        let physical_key = fluid.as_ref().unwrap_or(key);
        let creative = self.ae2_creative_cell_keys(&stack).ok();
        let creative_accepts = creative.as_ref().is_some_and(|keys| {
            keys.iter().any(|entry| match (entry, fluid.is_some()) {
                (super::ae2_storage_devices::CreativeCellKey::Item(k), false) => k == physical_key,
                (super::ae2_storage_devices::CreativeCellKey::Fluid(k), true) => k == physical_key,
                _ => false,
            })
        });
        let is_fluid = fluid.is_some();
        let channel = if is_fluid { Channel::Fluid } else { Channel::Item };
        let mut cell = if creative.is_some() { None } else { Some(self.ae2_read_cell(&stack)?) };
        // The physical cell only accepts its own channel. A foreign-channel
        // key restored from source data is nevertheless recoverable.
        if insert && cell.as_ref().is_some_and(|cell|cell.config().channel!=channel) {return Ok(0);}
        let possible = if let Some(cell) = cell.as_mut() {
            if insert {
                self.ae2_cell_insert(cell, physical_key, limit, Action::Simulate)
            } else {
                if cell.config().channel==channel {cell.extract(physical_key,limit,Action::Simulate)}
                else {cell.extract_other(physical_key,limit,Action::Simulate)}
            }
        } else if creative_accepts {
            limit
        } else {
            0
        };
        let per_unit = if is_fluid { 1.0 / 125.0 } else { 1.0 };
        let grid = self.ae2_take_power(pos, possible as f64 * per_unit, true);
        if is_fluid && (possible < limit || grid + battery < limit as f64 * per_unit) {
            return Ok(0);
        }
        let amount = if is_fluid {
            limit
        } else {
            possible.min(((grid + battery + 0.9) / per_unit).floor() as u64)
        };
        if amount == 0 {
            return Ok(0);
        }
        let moved = if let Some(cell) = cell.as_mut() {
            if insert {
                self.ae2_cell_insert(cell, physical_key, amount, Action::Modulate)
            } else {
                if cell.config().channel==channel {cell.extract(physical_key,amount,Action::Modulate)}
                else {cell.extract_other(physical_key,amount,Action::Modulate)}
            }
        } else {
            amount
        };
        if let Some(cell) = cell.as_ref() {
            if cell.config().channel==Channel::Fluid {
                super::ae2_fluids::write_fluid_cell(&mut stack, cell)?;
            } else {
                ae2_lab::write_cell(&mut stack, cell)?;
            }
        }
        let paid_grid = self.ae2_take_power(pos, moved as f64 * per_unit, false);
        let Some(BlockEntity::Ae2(m)) = self.world.block_entities.get_mut(&pos) else {
            return Err("ME chest disappeared".into());
        };
        m.private_energy_extract((moved as f64 * per_unit - paid_grid).max(0.0) * usage, true);
        m.slots[0] = Some(stack);
        self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
        Ok(moved)
    }
    fn ae2_terminal_insert(
        &mut self,
        pos: (i32, i32, i32),
        stack: &ItemStack,
        amount: u64,
    ) -> Result<u64, String> {
        if self.ae2_local_chest(pos) {
            self.ae2_chest_transfer(
                pos,
                &ae2_lab::key(stack)?,
                amount.min(stack.count as u64),
                true,
            )
        } else {
            self.ae2_grid_insert(pos, stack, amount)
        }
    }
    fn ae2_terminal_extract(
        &mut self,
        pos: (i32, i32, i32),
        key: &ae2_cell_core::ItemKey,
        amount: u64,
    ) -> Result<Option<ItemStack>, String> {
        if !self.ae2_local_chest(pos) {
            return self.ae2_grid_extract(pos, key, amount);
        }
        let mut item = ae2_lab::prototype(key)?;
        let moved = self.ae2_chest_transfer(pos, key, amount, false)?;
        if moved == 0 {
            return Ok(None);
        }
        item.count = moved as u32;
        Ok(Some(item))
    }
    fn ae2_terminal_entries(
        &self,
        pos: (i32, i32, i32),
    ) -> Result<Vec<(ae2_cell_core::ItemKey, u64)>, String> {
        if !self.ae2_terminal_powered(pos) {
            return Err("terminal is offline: check power and channels".into());
        }
        if self.ae2_local_chest(pos) {
            let Some(horizon_client::containers::BlockEntity::Ae2(m)) =
                self.world.block_entities.get(&pos)
            else {
                return Ok(Vec::new());
            };
            let Some(stack) = m.slots.first().and_then(Option::as_ref) else {
                return Ok(Vec::new());
            };
            if let Ok(keys) = self.ae2_creative_cell_keys(stack) {
                return Ok(keys
                    .into_iter()
                    .map(|entry| match entry {
                        super::ae2_storage_devices::CreativeCellKey::Item(key) => {
                            (key, i32::MAX as u64)
                        }
                        super::ae2_storage_devices::CreativeCellKey::Fluid(key) => {
                            (fluid_marker(&key), i32::MAX as u64)
                        }
                    })
                    .collect());
            }
            use ae2_cell_core::Channel;
            let cell=self.ae2_read_cell(stack)?;
            let mut entries:Vec<_>=cell.entries_for(Channel::Item).map(|(k,n)|(k.clone(),*n)).collect();
            entries.extend(cell.entries_for(Channel::Fluid).map(|(k,n)|(fluid_marker(k),*n)));
            return Ok(entries);
        }
        let mut entries = self.ae2_grid_inventory(pos)?;
        entries.extend(self.ae2_grid_fluids(pos)?.into_iter().map(|(key,amount)|(fluid_marker(&key),amount)));
        for key in self.ae2_craftable_outputs(pos) {
            if !entries.iter().any(|(existing, _)| *existing == key) {
                entries.push((key, 0));
            }
        }
        for key in self.ae2_craftable_fluids(pos) {
            let key = fluid_marker(&key);
            if !entries.iter().any(|(existing, _)| *existing == key) {
                entries.push((key, 0));
            }
        }
        Ok(entries)
    }
    pub(super) fn ae2_cpu_label(&self, address: super::ae2_global::GridAddress) -> String {
        self.ae2_address_parts(address)
            .map(|(dim, p)| format!("{} ({},{},{})", dim.id(), p.0, p.1, p.2))
            .unwrap_or_else(|| "unknown CPU".into())
    }
    fn ae2_job_button(&self, pos: (i32, i32, i32), selected: usize) -> Option<ItemStack> {
        let cpus=self.ae2_terminal_status_cpus(pos);
        let jobs = self.ae2_recursive_jobs(pos);
        let label=if cpus.is_empty() {"Crafting status: no CPUs".to_owned()} else {format!("Crafting {} {}/{}: click to view",if jobs.is_empty() {"status"} else {"job"},selected % cpus.len()+1,cpus.len())};
        let mut icon = navigation(&label);
        let lines: Vec<_> = jobs
            .iter()
            .map(|(p, pending, flight, cancelled)| {
                Tag::String(
                    format!(
                        "CPU {}: {} pending, {} in flight{}",
                        self.ae2_cpu_label(*p),
                        pending,
                        flight,
                        if *cancelled { " (cancelling)" } else { "" }
                    )
                    .as_str()
                    .into(),
                )
            })
            .collect();
        icon.data_mut()
            .set_other("minecraft:lore", Some(Tag::List(lines)));
        Some(icon)
    }
    pub(super) fn ae2_terminal_open(&self) -> bool {
        self.ae2_local_terminal_open() && self.ae2_local_craft.is_none()
    }
    pub(super) fn ae2_local_terminal_open(&self) -> bool {
        self.remote.is_none() && self.ae2_local_terminal.as_ref()
            .is_some_and(|session| self.container == Some(session.pos))
    }
    pub(super) fn ae2_local_terminal_valid(&self) -> bool {
        self.ae2_local_terminal_open() && self.ae2_local_terminal.as_ref().is_some_and(|s| {
            if s.dim != self.dim.id() || self.world.block_entities.incarnation(&s.pos)!=Some(s.host) {return false;}
            match s.part {
                Some(part)=>self.ae2_terminal_part_identity(part)==s.part_identity,
                None=>matches!(self.world.block_entities.get(&s.pos),Some(horizon_client::containers::BlockEntity::Ae2(m))
                    if matches!(m.kind,horizon_client::ae2_world::MachineKind::Terminal|horizon_client::ae2_world::MachineKind::CraftingTerminal|horizon_client::ae2_world::MachineKind::Chest)),
            }
        })
    }
    pub(super) fn ae2_local_terminal_part(&self)->Option<super::ae2_parts::PartAddress> {
        self.ae2_local_terminal.as_ref().and_then(|session|session.part)
    }
    pub(super) fn ae2_open_terminal(&mut self, pos: (i32, i32, i32), face: Option<[i32;3]>) -> bool {
        use horizon_client::{ae2_world::MachineKind,containers::BlockEntity};
        if self.remote.is_some() {return false;}
        let Some(host)=self.world.block_entities.incarnation(&pos) else {return false;};
        let direct=matches!(self.world.block_entities.get(&pos),Some(BlockEntity::Ae2(m))
            if matches!(m.kind,MachineKind::Terminal|MachineKind::CraftingTerminal|MachineKind::Chest));
        let (part,part_identity)=if direct {(None,None)} else {
            let Some(face)=face.and_then(|f|match f {[1,0,0]=>Some(0),[-1,0,0]=>Some(1),[0,1,0]=>Some(2),[0,-1,0]=>Some(3),[0,0,1]=>Some(4),[0,0,-1]=>Some(5),_=>None}) else {return false;};
            let Some(address)=self.ae2_local_address(pos) else {return false;};
            let part=super::ae2_parts::PartAddress{host:address,face};
            let Some(identity)=self.ae2_terminal_part_identity(part) else {return false;};
            (Some(part),Some(identity))
        };
        let crafting=part.and_then(|part|self.ae2_part_machine(part).ok().flatten()).is_some_and(|machine|machine.kind==MachineKind::CraftingTerminal)
            || direct && matches!(self.world.block_entities.get(&pos),Some(BlockEntity::Ae2(machine)) if machine.kind==MachineKind::CraftingTerminal);
        let encoding=part.and_then(|part|self.ae2_part_machine(part).ok().flatten()).is_some_and(|machine|machine.kind==MachineKind::PatternEncodingTerminal);
        let client=super::ae2_client_config::settings();
        let mut search=horizon_client::ui::EditBox::new(0.0,0.0,90.0,12.0);
        search.max_length=128;
        if client.remember_last_search && !self.ae2_remembered_search.is_empty() {search.set_value(&self.ae2_remembered_search);search.highlight=0;}
        search.focused=client.auto_focus_search;
        let chest=self.chest_terminal_read(pos).map(|(_,settings)|settings).unwrap_or(ChestTerminalSettings{sort:0,view:0,descending:false});
        self.ae2_job_ui.terminal_opened();
        self.ae2_terminal_page = 0;
        self.container = Some(pos);
        self.ae2_local_terminal=Some(LocalTerminalSession{pos,host,part,part_identity,dim:self.dim.id().to_owned(),crafting,encoding,encoding_open:false,encoding_page:0,encoding_fluid_mode:false,encoding_fluid_amount:1000,encoding_selected_fluid:0,search,sort:chest.sort,view:chest.view,descending:chest.descending,search_mod_name:client.search_mod_name_in_tooltips,cache:std::cell::RefCell::new(None)});
        true
    }
    pub(super) fn ae2_invalidate_local_terminal(&self) {
        if let Some(session)=self.ae2_local_terminal.as_ref() {*session.cache.borrow_mut()=None;}
    }
    pub(super) fn ae2_remember_local_terminal_search(&mut self) {
        if let Some(session)=self.ae2_local_terminal.as_ref() {self.ae2_remembered_search=session.search.value.clone();}
    }
    pub(super) fn ae2_local_terminal_search_focused(&self)->bool {
        self.ae2_terminal_open() && self.ae2_local_terminal.as_ref().is_some_and(|session|session.search.focused)
    }
    pub(super) fn ae2_local_crafting_terminal_open(&self)->bool {
        self.ae2_terminal_open() && self.ae2_local_terminal.as_ref().is_some_and(|session|session.crafting)
    }
    pub(super) fn ae2_local_crafting_session_open(&self)->bool {
        self.ae2_local_terminal_open() && self.ae2_local_terminal.as_ref().is_some_and(|session|session.crafting)
    }
    fn ae2_local_craft_matrix(&self)->Option<horizon_client::ae2_world::Machine> {
        if !self.ae2_local_terminal_valid() {return None;}
        let session=self.ae2_local_terminal.as_ref().filter(|session|session.crafting)?;
        let machine=match session.part {
            Some(part)=>self.ae2_part_machine(part).ok().flatten()?,
            None=>match self.world.block_entities.get(&session.pos)? {horizon_client::containers::BlockEntity::Ae2(machine)=>(**machine).clone(),_=>return None},
        };
        (machine.kind==horizon_client::ae2_world::MachineKind::CraftingTerminal && machine.slots.len()==9).then_some(machine)
    }
    fn ae2_store_local_craft_matrix(&mut self,machine:&horizon_client::ae2_world::Machine)->Result<(),String> {
        if !self.ae2_local_terminal_valid() || machine.kind!=horizon_client::ae2_world::MachineKind::CraftingTerminal || machine.slots.len()!=9 {return Err("crafting terminal owner changed".into());}
        let session=self.ae2_local_terminal.as_ref().ok_or("terminal closed")?;
        if let Some(part)=session.part {return self.ae2_store_part_machine(part,machine);}
        let pos=session.pos;
        let Some(horizon_client::containers::BlockEntity::Ae2(target))=self.world.block_entities.get_mut(&pos) else{return Err("crafting terminal disappeared".into());};
        **target=machine.clone();self.world.modified.insert((pos.0>>4,pos.2>>4));Ok(())
    }
    /// AEItemKey.getMaxStackSize uses the effective item component, not just
    /// the registry's default max stack size.
    fn ae2_config_item_max(&self,item:&ItemStack)->u32 {
        self.res.items().stack_max(item)
    }
    fn ae2_local_encoding_machine(&self)->Option<(super::ae2_parts::PartAddress,horizon_client::ae2_world::Machine)> {
        if !self.ae2_local_terminal_valid() {return None;}
        let session=self.ae2_local_terminal.as_ref().filter(|s|s.encoding)?;
        let part=session.part?;
        let machine=self.ae2_part_machine(part).ok().flatten()?;
        (machine.kind==horizon_client::ae2_world::MachineKind::PatternEncodingTerminal && machine.slots.len()==2).then_some((part,machine))
    }
    fn ae2_store_local_encoding(&mut self,part:super::ae2_parts::PartAddress,machine:&horizon_client::ae2_world::Machine)->Result<(),String> {
        if !self.ae2_local_terminal_valid() || self.ae2_local_terminal_part()!=Some(part)
            || machine.kind!=horizon_client::ae2_world::MachineKind::PatternEncodingTerminal || machine.slots.len()!=2 {
            return Err("encoding terminal owner changed".into());
        }
        self.ae2_store_part_machine(part,machine)
    }
    pub(super) fn ae2_local_encoding_session_open(&self)->bool {
        self.ae2_local_terminal_open() && self.ae2_local_terminal.as_ref().is_some_and(|s|s.encoding)
    }
    pub(super) fn ae2_clear_local_encoding(&mut self) {
        let Some((part,mut machine))=self.ae2_local_encoding_machine() else{return;};
        let Ok(mut state)=super::ae2_encoding::EncodingState::load(&machine) else{return;};
        state.inputs.fill(None);state.outputs.fill(None);state.fluid_inputs.fill(None);state.fluid_outputs.fill(None);state.stone_recipe=None;
        if let Err(error)=state.save(&mut machine).and_then(|_|self.ae2_store_local_encoding(part,&machine)) {
            self.system_message(Text::literal(format!("Encoding: {error}")));
        }
    }
    fn ae2_local_encoding_preview(&self,state:&super::ae2_encoding::EncodingState)->Option<ItemStack> {
        if state.processing {
            return (state.inputs.iter().any(Option::is_some) || state.fluid_inputs.iter().any(Option::is_some))
                .then_some(())
                .filter(|_|state.outputs.iter().any(Option::is_some) || state.fluid_outputs.iter().any(Option::is_some))
                .map(|_|{let mut item=ItemStack::new("ae2:processing_pattern",1);item.data_mut().custom_name=Some("Encode item processing pattern".into());item});
        }
        if state.special_mode==1 {
            return self.res.stations().recipes.smithing_recipe_result(&self.res.stations().enchantments,state.inputs[0].as_ref(),state.inputs[1].as_ref(),state.inputs[2].as_ref()).map(|(_,result)|result);
        }
        if state.special_mode==2 {
            let input=state.inputs[0].as_ref()?;
            return state.stone_recipe.as_ref().and_then(|id|self.res.stations().recipes.stonecutter(input).iter().find(|r|&r.id==id).map(|r|r.result.clone()));
        }
        self.res.recipes().find(&state.inputs[..9],3).map(|(_,mut result)|{result.data_mut().custom_name=Some("Click to encode pattern".into());result})
    }
    pub(super) fn ae2_local_encoding_fake_slot(&self,slot:usize)->bool {
        if !self.ae2_terminal_open() || !self.ae2_local_terminal.as_ref().is_some_and(|session|session.encoding && session.encoding_open) {return false;}
        let Some((_,machine))=self.ae2_local_encoding_machine() else{return false};
        let Ok(state)=super::ae2_encoding::EncodingState::load(&machine) else{return false};
        (300..=308).contains(&slot) && (state.special_mode==0 || state.special_mode==1 && slot<303 || state.special_mode==2 && slot==300)
            || state.processing && (318..=320).contains(&slot)
    }
    pub(super) fn ae2_local_encoding_drag(&mut self,slot:usize,single:bool) {
        if !self.ae2_local_encoding_fake_slot(slot) || self.carried.is_none() {return;}
        let physical=self.carried.clone();
        if single {if let Some(item)=self.carried.as_mut(){item.count=1;}}
        let mode=self.ae2_local_terminal.as_ref().map(|s|s.encoding_fluid_mode).unwrap_or(false);
        if let Some(session)=self.ae2_local_terminal.as_mut(){session.encoding_fluid_mode=false;}
        self.ae2_local_encoding_click(Some(slot),0,Input::Pickup);
        if let Some(session)=self.ae2_local_terminal.as_mut(){session.encoding_fluid_mode=mode;}
        self.carried=physical;
    }
    fn ae2_local_encoding_slot_view(&self,slot:usize)->Option<Option<ItemStack>> {
        let session=self.ae2_local_terminal.as_ref().filter(|s|s.encoding)?;
        if slot==348 {return Some(Some(navigation(if session.encoding_open {"Back to storage"}else{"Encode crafting pattern"})));}
        if !session.encoding_open || !(300..354).contains(&slot) {return None;}
        let (_,machine)=self.ae2_local_encoding_machine()?;
        let state=super::ae2_encoding::EncodingState::load(&machine).ok()?;
        let page=if state.processing {session.encoding_page.min(8)}else{0};
        let item=match slot {
            300..=308 if state.special_mode==0 || state.special_mode==1 && slot<303 || state.special_mode==2 && slot==300=>{
                let i=page*9+slot-300;
                if let Some(fluid)=state.fluid_inputs[i].as_ref() {
                    encoding_fluid_icon(fluid)
                } else {state.inputs[i].as_ref().and_then(|item|encoding_item_icon(item,self.ae2_config_item_max(item)))}
            },
            313=>self.ae2_local_encoding_preview(&state),
            318..=320 if state.processing=>{
                let i=page*3+slot-318;
                if let Some(fluid)=state.fluid_outputs[i].as_ref() {
                    encoding_fluid_icon(fluid)
                } else {state.outputs[i].as_ref().and_then(|item|encoding_item_icon(item,self.ae2_config_item_max(item)))}
            },
            330..=339 if state.processing=>Some(navigation(&format!("Fluid amount digit {}",slot-330))),
            340 if state.processing=>Some(navigation("Clear fluid amount")),
            341 if state.processing=>Some(navigation(&format!("Fluid amount: {} mB",session.encoding_fluid_amount))),
            342 if state.processing=>Some(navigation("Previous processing page")),
            343 if state.processing=>Some(navigation(&format!("Processing page {}/9; click for next",page+1))),
            342 if state.special_mode==2=>Some(navigation("Previous stonecutting recipe")),
            343 if state.special_mode==2=>Some(navigation("Next stonecutting recipe")),
            344 if state.processing=>{
                let fluids=self.ae2_grid_fluids(session.pos).unwrap_or_default();let selected=fluids.get(session.encoding_selected_fluid%fluids.len().max(1));
                Some(navigation(&selected.map_or_else(||"No network fluid selected".into(),|(key,_)|format!("Selected fluid: {} ({} component bytes); click to cycle",key.item_id,key.components.len()))))
            },
            344 if state.special_mode==0=>Some(navigation(if state.fluid_substitutes {"Fluid substitutions: on"}else{"Fluid substitutions: off"})),
            345 if state.processing=>Some(navigation(if session.encoding_fluid_mode {"Ghost type: fluid bucket"}else{"Ghost type: item"})),
            346=>Some(navigation(match (state.processing,state.special_mode){(true,_)=>"Mode: processing",(_,1)=>"Mode: smithing",(_,2)=>"Mode: stonecutting",_=>"Mode: crafting"})),
            347=>Some(navigation(if state.substitutes {"Substitutions: on"}else{"Substitutions: off"})),
            349=>machine.slots[0].clone(),350=>machine.slots[1].clone(),
            _=>None,
        };
        Some(item)
    }
    fn ae2_local_encoding_physical_click(&mut self,index:usize,button:i32,input:Input)->Result<(),String> {
        if index>=2 {return Err("invalid encoding slot".into());}
        let (part,mut machine)=self.ae2_local_encoding_machine().ok_or("encoding terminal unavailable")?;
        let original=machine.slots[index].clone();let mut inventory=self.inventory.clone();let mut carried=self.carried.clone();
        let accepts=|item:&ItemStack|if index==0 {item.item.as_ref()=="ae2:blank_pattern"}else{item.item.as_ref()=="ae2:blank_pattern" || super::ae2_encoding::EncodingState::from_pattern(item).is_some()};
        if input==Input::QuickMove {
            if let Some(mut item)=machine.slots[index].take() {
                let max=self.res.items().stack_max(&item);
                for target in &mut inventory {
                    if let Some(existing)=target.as_mut().filter(|s|same(s,&item) && s.count<max) {
                        let n=item.count.min(max-existing.count);existing.count+=n;item.count-=n;
                    }
                    if item.count==0 {break;}
                }
                if item.count>0 {for target in &mut inventory {
                    if target.is_none() {let n=item.count.min(max);let mut moved=item.clone();moved.count=n;*target=Some(moved);item.count-=n;}
                    if item.count==0 {break;}
                }}
                if item.count>0 {machine.slots[index]=Some(item);}
            }
        } else if input==Input::Pickup && matches!(button,0|1) {
            let held=carried.take();let stored=machine.slots[index].take();
            match (held,stored) {
                (None,Some(mut item))=>{let n=if button==1 {item.count.div_ceil(2)}else{item.count};let mut taken=item.clone();taken.count=n;item.count-=n;carried=Some(taken);if item.count>0 {machine.slots[index]=Some(item);}},
                (Some(mut held),None) if accepts(&held)=>{let limit=self.res.items().stack_max(&held).min(if index==1 {1}else{64});let n=if button==1 {1.min(limit)}else{held.count.min(limit)};let mut put=held.clone();put.count=n;held.count-=n;machine.slots[index]=Some(put);if held.count>0 {carried=Some(held);}},
                (Some(mut held),Some(mut item)) if index==0 && accepts(&held) && same(&held,&item)=>{let n=self.res.items().stack_max(&held).min(64).saturating_sub(item.count).min(if button==1 {1}else{held.count});held.count-=n;item.count+=n;machine.slots[index]=Some(item);if held.count>0 {carried=Some(held);}},
                (Some(held),Some(item)) if button==0 && accepts(&held) && held.count<=self.res.items().stack_max(&held).min(if index==1 {1}else{64})=>{machine.slots[index]=Some(held);carried=Some(item);},
                (held,item)=>{carried=held;machine.slots[index]=item;}
            }
        } else {return Ok(());}
        if index==1 && machine.slots[1]!=original {
            if let Some(state)=machine.slots[1].as_ref().and_then(super::ae2_encoding::EncodingState::from_pattern) {state.save(&mut machine)?;}
        }
        self.ae2_store_local_encoding(part,&machine)?;self.inventory=inventory;self.carried=carried;
        Ok(())
    }
    fn ae2_local_encoding_quick_insert(&mut self,index:usize)->Result<bool,String> {
        let Some(mut item)=self.inventory.get(index).cloned().flatten() else{return Ok(false);};
        let target=if item.item.as_ref()=="ae2:blank_pattern" {0}else if super::ae2_encoding::EncodingState::from_pattern(&item).is_some(){1}else{return Ok(false);};
        let (part,mut machine)=self.ae2_local_encoding_machine().ok_or("encoding terminal unavailable")?;
        let limit=self.res.items().stack_max(&item).min(if target==0 {64}else{1});
        let room=match machine.slots[target].as_ref(){None=>limit,Some(existing) if target==0 && same(existing,&item)=>limit.saturating_sub(existing.count),_=>0};
        let moved=item.count.min(room);if moved==0 {return Ok(true);}
        if let Some(existing)=machine.slots[target].as_mut(){existing.count+=moved;}else{let mut placed=item.clone();placed.count=moved;machine.slots[target]=Some(placed);}
        item.count-=moved;
        if target==1 {if let Some(state)=machine.slots[1].as_ref().and_then(super::ae2_encoding::EncodingState::from_pattern){state.save(&mut machine)?;}}
        self.ae2_store_local_encoding(part,&machine)?;self.inventory[index]=(item.count>0).then_some(item);Ok(true)
    }
    fn ae2_local_encoding_click(&mut self,slot:Option<usize>,button:i32,input:Input)->bool {
        let Some(session)=self.ae2_local_terminal.as_ref().filter(|s|s.encoding) else{return false;};
        if slot==Some(348) && input==Input::Pickup && matches!(button,0|1) {
            if let Some(session)=self.ae2_local_terminal.as_mut(){session.encoding_open=!session.encoding_open;}
            return true;
        }
        if !session.encoding_open {return false;}
        let Some(slot)=slot else{return true;};
        if slot<36 {
            if input==Input::QuickMove {
                match self.ae2_local_encoding_quick_insert(slot) {
                    Ok(true)=>return true,Ok(false)=>return false,
                    Err(error)=>{self.system_message(Text::literal(format!("Encoding: {error}")));return true;}
                }
            }
            return false;
        }
        if !(300..354).contains(&slot) {return true;}
        if matches!(slot,349|350) {
            if let Err(error)=self.ae2_local_encoding_physical_click(slot-349,button,input){self.system_message(Text::literal(format!("Encoding: {error}")));}
            return true;
        }
        let fake=self.ae2_local_encoding_fake_slot(slot);
        // AEBaseScreen permits inventory hotkeys only with an empty hand. Its
        // overstacking Encoding slots cannot be picked up by SwapSlotsPacket.
        if fake && matches!(input,Input::Swap|Input::Throw) && self.carried.is_some() {return true;}
        if fake && input==Input::Swap && button!=40 {return true;}
        if fake && input==Input::Clone {
            let processing=self.ae2_local_encoding_machine().and_then(|(_,machine)|super::ae2_encoding::EncodingState::load(&machine).ok()).is_some_and(|state|state.processing);
            if processing || self.ae2_local_encoding_slot_view(slot).flatten().is_some_and(|item|item.item.as_ref()=="ae2:wrapped_generic_stack") {return true;}
        }
        let (input,button)=if fake && !matches!(input,Input::Pickup|Input::QuickCraft) {
            (Input::Pickup,i32::from(button==1))
        } else {(input,button)};
        if input!=Input::Pickup || !matches!(button,0|1) {return true;}
        let Some((part,mut machine))=self.ae2_local_encoding_machine() else{return true;};
        let Ok(mut state)=super::ae2_encoding::EncodingState::load(&machine) else{return true;};
        let Some(session)=self.ae2_local_terminal.as_ref() else{return true;};
        let page=if state.processing {session.encoding_page.min(8)}else{0};
        let fluid_mode=session.encoding_fluid_mode;let fluid_amount=session.encoding_fluid_amount;
        let selected_fluid=session.encoding_selected_fluid;let pos=session.pos;
        let cursor=super::ae2_encoding::cursor_ghost(self.carried.as_ref(),&|id,fluid|fluid||self.res.items().get(id).is_some());
        let fluids=if state.processing && fluid_mode {self.ae2_grid_fluids(pos).unwrap_or_default()}else{Vec::new()};
        let selected=encoding_selected_fluid(self.carried.as_ref(),fluid_amount,selected_fluid,&fluids);
        match slot {
            330..=340 if state.processing=>{
                if let Some(session)=self.ae2_local_terminal.as_mut() {
                    if slot==340 {session.encoding_fluid_amount=0;}
                    else if let Some(next)=session.encoding_fluid_amount.checked_mul(10).and_then(|v|v.checked_add((slot-330) as u64)).filter(|v|*v<=i64::MAX as u64) {session.encoding_fluid_amount=next;}
                }
            },
            342|343 if state.special_mode==2=>{
                if let Some(item)=state.inputs[0].as_ref() {
                    let recipes=self.res.stations().recipes.stonecutter(item);
                    if !recipes.is_empty(){
                        let current=state.stone_recipe.as_ref().and_then(|id|recipes.iter().position(|r|&r.id==id)).unwrap_or(0);
                        let next=if slot==342 {(current+recipes.len()-1)%recipes.len()}else{(current+1)%recipes.len()};state.stone_recipe=Some(recipes[next].id.clone());
                    }
                }
            },
            342|343 if state.processing=>if let Some(session)=self.ae2_local_terminal.as_mut(){session.encoding_page=if slot==342 {(page+8)%9}else{(page+1)%9};},
            344 if state.processing=>if let Some(session)=self.ae2_local_terminal.as_mut(){session.encoding_selected_fluid=session.encoding_selected_fluid.wrapping_add(1);},
            344 if state.special_mode==0=>state.fluid_substitutes=!state.fluid_substitutes,
            345 if state.processing=>if let Some(session)=self.ae2_local_terminal.as_mut(){session.encoding_fluid_mode=!session.encoding_fluid_mode;},
            346=>match (state.processing,state.special_mode){(false,0)=>state.processing=true,(true,_)=>{state.processing=false;state.special_mode=1;},(false,1)=>state.special_mode=2,_=>state.special_mode=0},
            347=>state.substitutes=!state.substitutes,
            300..=308 if state.special_mode==0 || state.special_mode==1 && slot<303 || state.special_mode==2 && slot==300=>{
                let index=page*9+slot-300;
                if let Some(fluid)=state.processing.then(||encoding_emptying_action(self.carried.as_ref(),button)).flatten() {
                    state.inputs[index]=None;state.fluid_inputs[index]=Some(fluid);
                } else if let Some(value)=super::ae2_encoding::select_ghost(&cursor,state.processing && fluid_mode,selected.clone(),button==1) {
                    let max=state.inputs[index].as_ref().map_or(64,|item|self.ae2_config_item_max(item));
                    super::ae2_encoding::click_ghost(&mut state.inputs[index],&mut state.fluid_inputs[index],value,button==1,max,i32::MAX as u32,i64::MAX as u64);
                }
                if state.special_mode==2 && slot==300 {state.stone_recipe=state.inputs[0].as_ref().and_then(|item|self.res.stations().recipes.stonecutter(item).first().map(|r|r.id.clone()));}
            },
            318..=320 if state.processing=>{
                let index=page*3+slot-318;
                if let Some(fluid)=encoding_emptying_action(self.carried.as_ref(),button) {
                    state.outputs[index]=None;state.fluid_outputs[index]=Some(fluid);
                } else if let Some(value)=super::ae2_encoding::select_ghost(&cursor,fluid_mode,selected.clone(),button==1) {
                    let max=state.outputs[index].as_ref().map_or(64,|item|self.ae2_config_item_max(item));
                    super::ae2_encoding::click_ghost(&mut state.outputs[index],&mut state.fluid_outputs[index],value,button==1,max,i32::MAX as u32,i64::MAX as u64);
                }
            },
            313=>{
                let pattern=if state.processing {
                    super::ae2_processing::ae2_encode_processing_positioned(&state.inputs,&state.outputs,&state.fluid_inputs,&state.fluid_outputs).ok()
                } else if state.special_mode!=0 {
                    let count=if state.special_mode==1 {3}else{1};let mut grid=vec![None;9];grid[..count].clone_from_slice(&state.inputs[..count]);
                    let recipe=if state.special_mode==1 {self.res.stations().recipes.smithing_recipe_result(&self.res.stations().enchantments,grid[0].as_ref(),grid[1].as_ref(),grid[2].as_ref()).map(|(id,_)|id.to_owned())}else{state.stone_recipe.clone()};
                    recipe.and_then(|id|{let kind=if state.special_mode==1 {"smithing"}else{"stonecutting"};let output=super::ae2_processing::special_recipe_result(self.res.stations(),kind,&id,&grid)?;super::ae2_processing::ae2_encode_special_pattern(kind,&id,&grid,&output,state.substitutes).ok()})
                } else {
                    self.res.recipes().find(&state.inputs[..9],3).and_then(|(recipe,out)|super::ae2_processing::ae2_encode_pattern_source(&state.inputs[..9],&out,recipe,state.substitutes,state.fluid_substitutes).ok())
                };
                if let Some(pattern)=pattern {
                    let eligible=machine.slots[1].as_ref().is_some_and(|item|item.item.as_ref()=="ae2:blank_pattern" || super::ae2_encoding::EncodingState::from_pattern(item).is_some());
                    let empty=machine.slots[1].is_none();let blank=machine.slots[0].as_ref().is_some_and(|item|item.item.as_ref()=="ae2:blank_pattern" && item.count>0);
                    if eligible || empty && blank {
                        if empty {if let Some(item)=machine.slots[0].as_mut(){item.count-=1;if item.count==0 {machine.slots[0]=None;}}}
                        machine.slots[1]=Some(pattern);
                    } else {self.system_message(Text::literal("Insert a blank pattern into the terminal"));}
                }
            },
            _=>{}
        }
        state.normalize_crafting_inputs();
        if let Err(error)=state.save(&mut machine).and_then(|_|self.ae2_store_local_encoding(part,&machine)) {
            self.system_message(Text::literal(format!("Encoding: {error}")));
        }
        true
    }
    /// CraftingTermMenu.clearGridToNetwork: stage cell changes and the
    /// face-owned matrix as one transaction; leftovers stay in their slots.
    pub(super) fn ae2_clear_local_crafting_grid(&mut self) {
        let Some(mut machine)=self.ae2_local_craft_matrix() else{return;};
        let Some(pos)=self.container else{return;};
        let Some(address)=self.ae2_local_terminal_part().map(|part|part.host).or_else(||self.ae2_local_address(pos)) else{return;};
        let before=self.ae2_snapshot_blocks();
        let result=(||->Result<(),String> {
            let mut formation=super::ae2_buses::FormationPlan::default();
            for slot in &mut machine.slots {
                let Some(mut stack)=slot.take() else{continue;};
                let moved=self.ae2_grid_insert_staged_encoded(address,&stack,u64::from(stack.count),&mut formation)?;
                if moved>u64::from(stack.count) {return Err("invalid grid insertion count".into());}
                stack.count-=moved as u32;if stack.count>0 {*slot=Some(stack);}
            }
            self.ae2_store_local_craft_matrix(&machine)?;self.ae2_commit_formation(formation)
        })();
        if let Err(error)=result {self.ae2_restore_blocks(before);self.system_message(Text::literal(format!("Crafting terminal: {error}")));}
        self.ae2_invalidate_local_terminal();
    }
    fn ae2_clear_local_crafting_to_inventory(&mut self)->Result<(),String> {
        let Some(mut machine)=self.ae2_local_craft_matrix() else{return Err("crafting terminal unavailable".into());};
        let original=machine.slots.clone();let mut inventory=self.inventory.clone();
        let order:Vec<usize>=(0..9).rev().chain(9..36).collect();
        for slot in &mut machine.slots {
            let Some(mut stack)=slot.take() else{continue;};
            let max=self.res.items().stack_max(&stack);
            for &index in &order {
                if let Some(target)=inventory[index].as_mut().filter(|target|target.same_item_same_components(&stack) && target.count<max) {
                    let moved=stack.count.min(max-target.count);target.count+=moved;stack.count-=moved;if stack.count==0 {break;}
                }
            }
            if stack.count>0 {
                for &index in &order {
                    if inventory[index].is_none() {let moved=stack.count.min(max);let mut target=stack.clone();target.count=moved;inventory[index]=Some(target);stack.count-=moved;if stack.count==0 {break;}}
                }
            }
            if stack.count>0 {*slot=Some(stack);}
        }
        if let Err(error)=self.ae2_store_local_craft_matrix(&machine) {
            for stack in original.iter().chain(self.inventory.iter()).flatten() {horizon_client::ae2_stack_identity::synchronize_tracked(stack);}
            return Err(error);
        }
        self.inventory=inventory;self.ae2_invalidate_local_terminal();Ok(())
    }
    /// Use the same native CraftingMenu engine as the server adapter, staging
    /// player state until the original physical part accepts the matrix.
    fn ae2_local_craft_matrix_click(&mut self,slot:usize,button:i32,input:Input)->Result<(),String> {
        use horizon_client::menu::{Kind,Menu};
        if !(327..=336).contains(&slot) || !matches!(input,Input::Pickup|Input::QuickMove|Input::Swap|Input::Throw|Input::Clone) {return Ok(());}
        let allowed=match input {Input::Pickup|Input::QuickMove|Input::Throw=>(0..=1).contains(&button),Input::Swap=>slot!=336 && ((0..=8).contains(&button)||button==40),Input::Clone=>slot!=336 && button==2,_=>false};
        if !allowed {return Ok(());}
        if slot==336 {
            if matches!(input,Input::Pickup|Input::QuickMove) {return self.ae2_local_craft_result(button,input);}
            return Ok(());
        }
        let mut machine=self.ae2_local_craft_matrix().ok_or("crafting terminal unavailable")?;
        let original_matrix=machine.slots.clone();
        if machine.slots.iter().chain(self.inventory.iter()).chain(std::iter::once(&self.offhand)).chain(std::iter::once(&self.carried)).flatten().any(|stack| {
            let max=self.res.items().stack_max(&stack).min(99);
            stack.count==0 || stack.count>max
        }) {return Err("oversized crafting inventory".into());}
        let order:Vec<usize>=(9..36).chain(0..9).collect();
        let (clicked,next,inventory,offhand,carried,crafted,dropped,sounds,random)={
            let env=self.menu_env();let mut menu=Menu::new(Kind::Crafting);
            menu.creative=self.creative;menu.selected=self.selected;menu.recipe_durability_rng=Some(self.world.random.clone());
            for (i,stack) in machine.slots.iter().cloned().enumerate() {menu.set_slot(&env,1+i,stack);}
            for (i,&index) in order.iter().enumerate() {menu.set_slot(&env,10+i,self.inventory[index].clone());}
            menu.set_player_item(40,self.offhand.clone());menu.carried=self.carried.clone();menu.settle(&env);
            let menu_slot=if slot==336 {0}else{1+slot-327};
            let clicked=menu.click(&env,menu_slot as i32,button,input);
            let next=(1..10).map(|i|menu.slot(i).cloned()).collect::<Vec<_>>();
            let inventory=(10..46).map(|i|menu.slot(i).cloned()).collect::<Vec<_>>();
            let offhand=menu.player_item(40).cloned();let carried=menu.carried.take();
            (clicked,next,inventory,offhand,carried,std::mem::take(&mut menu.crafted),std::mem::take(&mut menu.dropped),std::mem::take(&mut menu.player_sounds),menu.recipe_durability_rng.take())
        };
        if let Err(error)=clicked {
            for stack in original_matrix.iter().chain(self.inventory.iter()).chain(std::iter::once(&self.offhand)).chain(std::iter::once(&self.carried)).flatten() {horizon_client::ae2_stack_identity::synchronize_tracked(stack);}
            return Err(error.to_owned());
        }
        machine.slots=next;
        if let Err(error)=self.ae2_store_local_craft_matrix(&machine) {
            for stack in original_matrix.iter().chain(self.inventory.iter()).chain(std::iter::once(&self.offhand)).chain(std::iter::once(&self.carried)).flatten() {
                horizon_client::ae2_stack_identity::synchronize_tracked(stack);
            }
            return Err(error);
        }
        for (index,stack) in order.into_iter().zip(inventory) {self.inventory[index]=stack;}
        self.offhand=offhand;self.carried=carried;if let Some(random)=random {self.world.random=random;}
        self.award_crafted(&crafted);for stack in dropped {self.player_drop(stack);}for sound in sounds {self.player_menu_sound(sound);}
        self.ae2_invalidate_local_terminal();Ok(())
    }
    /// CraftingTermSlot crafts at most one output stack on shift-click and
    /// refills consumed ingredients from permitted network storage. Stage the
    /// physical face, cells, player inventory and recipe effects together.
    fn ae2_local_craft_result(&mut self,button:i32,input:Input)->Result<(),String> {
        use horizon_client::menu::{Kind,Menu};
        let mut machine=self.ae2_local_craft_matrix().ok_or("crafting terminal unavailable")?;
        let pos=self.container.ok_or("terminal closed")?;let part=self.ae2_local_terminal_part();
        let initial=self.res.recipes().find(&machine.slots,3).map(|(_,item)|item);
        let Some(expected)=initial else{return Ok(());};
        let max=self.res.items().stack_max(&expected);
        let passes=if input==Input::QuickMove {max/expected.count.max(1)}else{1};
        let before=self.ae2_snapshot_blocks();let original_matrix=machine.slots.clone();
        let mut inventory=self.inventory.clone();let mut offhand=self.offhand.clone();let mut carried=self.carried.clone();let mut random=self.world.random.clone();
        let mut crafted=Vec::new();let mut dropped=Vec::new();let mut sounds=Vec::new();
        let order:Vec<usize>=(9..36).chain(0..9).collect();
        let result=(||->Result<(),String>{
            let mut formation=super::ae2_buses::FormationPlan::default();
            for _ in 0..passes {
                let previous=machine.slots.clone();
                let Some((_,next_result))=self.res.recipes().find(&previous,3) else{break;};
                if next_result.count!=expected.count || !next_result.same_item_same_components(&expected){break;}
                if input==Input::QuickMove {
                    let room=inventory.iter().map(|slot|match slot {None=>max,Some(item) if item.same_item_same_components(&expected)=>max.saturating_sub(item.count),_=>0}).fold(0u64,|sum,n|sum.saturating_add(u64::from(n)));
                    if room<u64::from(expected.count){break;}
                }
                let (next,next_inventory,next_offhand,next_carried,effects,drops,player_sounds,next_random)={
                    let env=self.menu_env();let mut menu=Menu::new(Kind::Crafting);menu.creative=self.creative;menu.selected=self.selected;menu.recipe_durability_rng=Some(random.clone());
                    for (i,stack) in previous.iter().cloned().enumerate(){menu.set_slot(&env,1+i,stack);}
                    for (i,&index) in order.iter().enumerate(){menu.set_slot(&env,10+i,inventory[index].clone());}
                    menu.set_player_item(40,offhand.clone());menu.carried=carried.clone();menu.settle(&env);
                    if input==Input::QuickMove {
                        let cursor=menu.carried.take();menu.click(&env,0,0,Input::Pickup).map_err(|e|e.to_owned())?;
                        if let Some(mut output)=menu.carried.take(){menu.give(&env,&mut output);if output.count>0{return Err("craft result exceeds player capacity".into());}}
                        menu.carried=cursor;
                    } else {menu.click(&env,0,button,Input::Pickup).map_err(|e|e.to_owned())?;}
                    let next=(1..10).map(|i|menu.slot(i).cloned()).collect::<Vec<_>>();
                    let next_inventory=(10..46).map(|i|menu.slot(i).cloned()).collect::<Vec<_>>();
                    (next,next_inventory,menu.player_item(40).cloned(),menu.carried.take(),std::mem::take(&mut menu.crafted),std::mem::take(&mut menu.dropped),std::mem::take(&mut menu.player_sounds),menu.recipe_durability_rng.take())
                };
                if next==previous {break;}
                for (&index,stack) in order.iter().zip(next_inventory){inventory[index]=stack;}
                offhand=next_offhand;carried=next_carried;if let Some(rng)=next_random{random=rng;}
                crafted.extend(effects);dropped.extend(drops);sounds.extend(player_sounds);machine.slots=next;
                for i in 0..9 {
                    if machine.slots[i].is_some(){continue;}
                    let Some(old)=previous[i].as_ref() else{continue;};
                    let Ok(key)=ae2_lab::key(old) else{continue;};
                    if self.ae2_view_permits(&key,part,horizon_client::ae2_cell_codec::ViewCellScope::ItemsOnly) {
                        if let Ok(Some(item))=self.ae2_grid_extract_staged(pos,&key,1,&mut formation){machine.slots[i]=Some(item);}
                    }
                    let damageable=self.res.items().get(&old.item).is_some_and(|v|v.max_damage>0);
                    if machine.slots[i].is_none() && (old.extra.is_some() || damageable) {
                        if let Ok(entries)=self.ae2_grid_inventory(pos) {
                            for (candidate,_) in entries {
                                if !self.ae2_view_permits(&candidate,part,horizon_client::ae2_cell_codec::ViewCellScope::ItemsOnly) || candidate.item_id!=key.item_id || candidate==key {continue;}
                                let Ok(sample)=ae2_lab::prototype(&candidate) else{continue;};let mut grid=previous.clone();grid[i]=Some(sample);
                                let valid=self.res.recipes().find(&grid,3).is_some_and(|(_,output)|output.count==expected.count && output.same_item_same_components(&expected));
                                if valid {if let Ok(Some(item))=self.ae2_grid_extract_staged(pos,&candidate,1,&mut formation){machine.slots[i]=Some(item);break;}}
                            }
                        }
                    }
                }
                if input!=Input::QuickMove {break;}
            }
            self.ae2_store_local_craft_matrix(&machine)?;self.ae2_commit_formation(formation)
        })();
        if let Err(error)=result {
            self.ae2_restore_blocks(before);
            for item in original_matrix.iter().chain(self.inventory.iter()).chain(std::iter::once(&self.offhand)).chain(std::iter::once(&self.carried)).flatten(){horizon_client::ae2_stack_identity::synchronize_tracked(item);}
            return Err(error);
        }
        self.inventory=inventory;self.offhand=offhand;self.carried=carried;self.world.random=random;
        self.award_crafted(&crafted);for item in dropped {self.player_drop(item);}for sound in sounds {self.player_menu_sound(sound);}
        self.ae2_invalidate_local_terminal();Ok(())
    }
    /// Run a complete drag on one CraftingMenu instance so its quick-craft
    /// state spans all three phases, then commit the face and player together.
    pub(super) fn ae2_local_craft_matrix_drag(&mut self,ops:&[(Option<usize>,i32,Input)])->bool {
        use horizon_client::menu::{Kind,Menu};
        if !self.ae2_local_crafting_terminal_open() || ops.is_empty() || ops.iter().any(|(_,_,input)|*input!=Input::QuickCraft) {return false;}
        let Some(mut machine)=self.ae2_local_craft_matrix() else{return true;};
        let original=machine.slots.clone();let order:Vec<usize>=(9..36).chain(0..9).collect();
        let clicks=ops.iter().map(|(slot,button,input)|{
            let index=match slot {None=>-999,Some(i @ 327..=335)=>(1+i-327) as i32,Some(i @ 9..=35)=>(10+i-9) as i32,Some(i @ 0..=8)=>(37+i) as i32,_=>return None};
            Some((index,*button,*input))
        }).collect::<Option<Vec<_>>>();
        let Some(clicks)=clicks else{return true;};
        let staged=(||->Result<_,String>{
            let env=self.menu_env();let mut menu=Menu::new(Kind::Crafting);
            menu.creative=self.creative;menu.selected=self.selected;menu.recipe_durability_rng=Some(self.world.random.clone());
            for (i,item) in machine.slots.iter().cloned().enumerate(){menu.set_slot(&env,1+i,item);}
            for (i,&index) in order.iter().enumerate(){menu.set_slot(&env,10+i,self.inventory[index].clone());}
            menu.set_player_item(40,self.offhand.clone());menu.carried=self.carried.clone();menu.settle(&env);
            for (index,button,input) in clicks {menu.click(&env,index,button,input).map_err(|e|e.to_owned())?;}
            let matrix=(1..10).map(|i|menu.slot(i).cloned()).collect::<Vec<_>>();
            let inventory=(10..46).map(|i|menu.slot(i).cloned()).collect::<Vec<_>>();
            Ok((matrix,inventory,menu.player_item(40).cloned(),menu.carried.take()))
        })();
        let result=staged.and_then(|(matrix,inventory,offhand,carried)|{
            machine.slots=matrix;self.ae2_store_local_craft_matrix(&machine)?;
            for (index,item) in order.into_iter().zip(inventory){self.inventory[index]=item;}
            self.offhand=offhand;self.carried=carried;self.ae2_invalidate_local_terminal();Ok(())
        });
        if let Err(error)=result {
            for item in original.iter().chain(self.inventory.iter()).chain(std::iter::once(&self.offhand)).chain(std::iter::once(&self.carried)).flatten(){horizon_client::ae2_stack_identity::synchronize_tracked(item);}
            self.system_message(Text::literal(format!("Crafting terminal: {error}")));
        }
        true
    }
    pub(super) fn ae2_local_terminal_search_input(&mut self,text:Option<&str>,key:Option<winit::keyboard::KeyCode>) {
        let ctrl=self.has_control_down();let shift=self.has_shift_down();
        if let Some(session)=self.ae2_local_terminal.as_mut() {
            let old=session.search.value.clone();
            if !key.is_some_and(|key|session.search.key(key,ctrl,shift)) {if let Some(text)=text {session.search.insert_text(text);}}
            if old!=session.search.value {self.ae2_terminal_page=0;*session.cache.borrow_mut()=None;}
        }
    }
    pub(super) fn ae2_local_terminal_search_shortcut(&mut self,shortcut:horizon_client::clipboard::EditShortcut) {
        if let Some(session)=self.ae2_local_terminal.as_mut() {
            if session.search.shortcut(shortcut,&mut self.clipboard) {self.ae2_terminal_page=0;*session.cache.borrow_mut()=None;}
        }
    }
    pub(super) fn ae2_local_terminal_search_click(&mut self,ox:f32,oy:f32,right:bool)->bool {
        let (mx,my)=self.cursor;let hit=mx>=ox+80.0 && mx<ox+170.0 && my>=oy+3.0 && my<oy+15.0;
        let shift=self.has_shift_down();let Some(session)=self.ae2_local_terminal.as_mut() else{return false;};
        session.search.focused=hit;
        if hit {
            session.search.x=ox+80.0;session.search.y=oy+3.0;session.search.width=90.0;session.search.height=12.0;
            if right {session.search.set_value("");self.ae2_terminal_page=0;*session.cache.borrow_mut()=None;}
            session.search.click(&self.gui.atlas,mx,shift,false);
        }
        hit
    }
    pub(super) fn ae2_local_terminal_search_draw(&self,batch:&mut horizon_client::hud::HudBatch,ox:f32,oy:f32) {
        if let Some(mut field)=self.ae2_local_terminal.as_ref().map(|session|session.search.clone()) {
            field.x=ox+80.0;field.y=oy+3.0;field.width=90.0;field.height=12.0;field.draw(batch,&self.gui.atlas,None);
        }
    }
    fn ae2_local_terminal_entries(&self,pos:(i32,i32,i32))->Result<Vec<(ae2_cell_core::ItemKey,u64)>,String> {
        let session=self.ae2_local_terminal.as_ref().ok_or("terminal closed")?;
        if let Some((tick,entries))=session.cache.borrow().as_ref() {
            if *tick==self.ticks {return Ok(entries.clone());}
        }
        let all=self.ae2_terminal_entries(pos)?;
        let visible=self.ae2_view_filtered(&all,session.part);
        let mut craftable=self.ae2_craftable_outputs(pos);
        craftable.extend(self.ae2_craftable_fluids(pos).iter().map(fluid_marker));
        let language=horizon_client::lang::current().unwrap_or_else(||Arc::new(horizon_client::lang::Language::load(&[&self.options.resources],"en_us")));
        let filtered=terminal_filter_entries(&visible,&craftable,session.sort,session.descending,session.view,&session.search.value,
            &|key|self.ae2_terminal_display_name(key,&language),
            &|key,query|self.ae2_terminal_search_matches(key,query,session.search_mod_name,&language));
        let page_size=if session.crafting {27}else{PAGE_ITEMS};
        let entries=terminal_pinned_entries(&all,filtered,&self.ae2_job_ui.keys(),page_size);
        *session.cache.borrow_mut()=Some((self.ticks,entries.clone()));
        Ok(entries)
    }
    /// The displayed stack is derived fresh from cells and used solely for rendering.
    pub(super) fn ae2_terminal_slot_view(&self, slot: usize) -> Option<ItemStack> {
        if !self.ae2_terminal_open() || !(300..354).contains(&slot) {
            return None;
        }
        let pos = self.container?;
        if let Some(item)=self.ae2_local_encoding_slot_view(slot) {return item;}
        if self.ae2_local_crafting_terminal_open() {
            if (327..=335).contains(&slot) {return self.ae2_local_craft_matrix()?.slots.get(slot-327)?.clone();}
            if slot==336 {return self.ae2_local_craft_matrix().and_then(|machine|self.res.recipes().find(&machine.slots,3).map(|(_,result)|result));}
            if slot==337 {return Some(navigation("Clear crafting grid to network"));}
            if slot==338 {return Some(navigation("Clear crafting grid to inventory"));}
        }
        if slot == 349 {
            return self.ae2_job_button(pos, 0);
        }
        let session=self.ae2_local_terminal.as_ref()?;
        if slot==346 {return Some(navigation(&format!("Sort: {}",["name","amount","mod"][usize::from(session.sort)])));}
        if slot==347 {return Some(navigation(&format!("View: {}",["all","stored","craftable"][usize::from(session.view)])));}
        if slot==350 {return Some(navigation("Search"));}
        if slot==351 {return Some(navigation(if session.descending {"Descending"}else{"Ascending"}));}
        let entries=self.ae2_local_terminal_entries(pos).ok()?;
        self.ae2_terminal_display(slot - 300, self.ae2_terminal_page, &entries, if session.crafting {27}else{PAGE_ITEMS})
    }
    fn ae2_terminal_display(
        &self,
        index: usize,
        page: usize,
        entries: &[(ae2_cell_core::ItemKey, u64)],
        page_size: usize,
    ) -> Option<ItemStack> {
        let page = page.min(entries.len().saturating_sub(1) / page_size);
        if index == PREV && page > 0 {
            return Some(navigation("Previous page"));
        }
        if index == NEXT && (page + 1) * page_size < entries.len() {
            return Some(navigation("Next page"));
        }
        if index >= page_size {
            return None;
        }
        let (key, amount) = entries.get(page * page_size + index)?;
        if key.item_id.is_empty() {return None;}
        if let Some(fluid) = marker_fluid(key) {
            let id = super::ae2_fluids::fluid_bucket(&fluid.item_id).unwrap_or("minecraft:bucket");
            let mut icon = ItemStack::new(id, 1);
            icon.data_mut().custom_name = Some(if *amount == 0 {
                format!("Craft 1000 mB {}", fluid.item_id)
            } else {
                format!("{}: {} mB", fluid.item_id, amount)
            });
            return Some(icon);
        }
        let mut stack = ae2_lab::prototype(key).ok()?;
        let max = self.res.items().stack_max(&stack);
        stack.count = (*amount).max(1).min(u64::from(max)) as u32;
        let mut lore = match stack.data().and_then(|d| d.other("minecraft:lore")) {
            Some(Tag::List(v)) => v.clone(),
            _ => Vec::new(),
        };
        let label = if *amount == 0 {
            "Craftable: click to request".to_owned()
        } else {
            format!("Stored: {amount}")
        };
        lore.push(Tag::String(label.as_str().into()));
        stack
            .data_mut()
            .set_other("minecraft:lore", Some(Tag::List(lore)));
        Some(stack)
    }
    /// Consume all menu inputs when terminal is open: the generic chest menu
    /// must never see virtual slots, even for unsupported click gestures.
    pub(super) fn ae2_terminal_click(
        &mut self,
        slot: Option<usize>,
        button: i32,
        input: Input,
    ) -> bool {
        if !self.ae2_terminal_open() {
            return false;
        }
        if !self.ae2_local_terminal_valid() {return true;}
        if self.ae2_local_encoding_click(slot,button,input) {return true;}
        if self.ae2_local_crafting_terminal_open() && slot.is_some_and(|slot|(327..=338).contains(&slot)) {
            match slot.unwrap() {
                337 if input==Input::Pickup && button==0=>self.ae2_clear_local_crafting_grid(),
                338 if input==Input::Pickup && button==0=>if let Err(error)=self.ae2_clear_local_crafting_to_inventory() {self.system_message(Text::literal(format!("Crafting terminal: {error}")));},
                327..=336=>if let Err(error)=self.ae2_local_craft_matrix_click(slot.unwrap(),button,input) {self.system_message(Text::literal(format!("Crafting terminal: {error}")));},
                _=>{}
            }
            return true;
        }
        if slot==Some(349) && input==Input::Pickup && (0..=1).contains(&button) {
            if let Some(pos)=self.container {self.ae2_open_local_craft_status(pos);}
            return true;
        }
        if let (Some(slot @ (346|347|351)),Some(pos))=(slot,self.container) {
            if input==Input::Pickup && (0..=1).contains(&button) {
                let chest=self.chest_terminal_read(pos).and_then(|(identity,_)|self.chest_terminal_write(pos,identity,(slot-300) as i16,button as i8));
                if let Some(session)=self.ae2_local_terminal.as_mut() {
                    if let Some(settings)=chest {session.sort=settings.sort;session.view=settings.view;session.descending=settings.descending;}
                    else {let step=if button==0 {1}else{2};match slot {346=>session.sort=(session.sort+step)%3,347=>session.view=(session.view+step)%3,351=>session.descending=!session.descending,_=>{}}}
                    self.ae2_terminal_page=0;
                    *session.cache.borrow_mut()=None;
                }
            }
            return true;
        }
        let page_size=if self.ae2_local_crafting_terminal_open(){27}else{PAGE_ITEMS};
        if let (Some(slot),Some(pos))=(slot,self.container) {
            if (300..300+page_size).contains(&slot) && matches!(input,Input::Pickup|Input::QuickMove) && (0..=1).contains(&button) {
                if let Ok(entries)=self.ae2_local_terminal_entries(pos) {
                    let page=self.ae2_terminal_page.min(entries.len().saturating_sub(1)/page_size);
                    if let Some((key,count))=entries.get(page*page_size+slot-300) {
                        let fluid=marker_fluid(key);
                        let craftable=if let Some(fluid)=fluid.as_ref() {self.ae2_craftable_fluids(pos).contains(fluid)} else {self.ae2_craftable_outputs(pos).contains(key)};
                        if craftable && (input==Input::QuickMove || self.carried.is_none() && *count==0) {
                            self.ae2_open_local_craft_confirm(pos,fluid.clone().unwrap_or_else(||key.clone()),fluid.is_some());
                            return true;
                        }
                    }
                }
            }
        }
        let displayed=self.container.and_then(|pos|self.ae2_local_terminal_entries(pos).ok());
        let result = self.ae2_terminal_click_inner(slot, button, input, displayed.as_deref(), false, 1, page_size);
        self.ae2_invalidate_local_terminal();
        if let Err(message) = result {
            self.system_message(Text::literal(format!("ME terminal: {message}")));
        }
        true
    }
    fn ae2_terminal_click_inner(
        &mut self,
        slot: Option<usize>,
        button: i32,
        input: Input,
        entries_override: Option<&[(ae2_cell_core::ItemKey, u64)]>,
        wireless: bool,
        craft_amount: u64,
        page_size: usize,
    ) -> Result<(), String> {
        let pos = self.container.ok_or("terminal closed")?;
        if !wireless && !self.ae2_has_terminal(pos) {
            return Err("terminal disconnected".into());
        }
        let Some(slot) = slot else {
            return Ok(());
        };
        if (300..354).contains(&slot) {
            if !matches!(input, Input::Pickup | Input::QuickMove) || !(0..=1).contains(&button) {
                return Ok(());
            }
            let index = slot - 300;
            if index == 49 {
                let jobs = self.ae2_recursive_jobs(pos);
                if let Some((cpu, pending, flight, cancelled)) = jobs.first().copied() {
                    if button == 1 {
                        self.ae2_recursive_cancel(cpu)?;
                        self.system_message(Text::literal(
                            "Crafting cancellation requested; in-flight work will be reconciled",
                        ));
                    } else {
                        self.system_message(Text::literal(format!(
                            "CPU {}: {} pending, {} in flight{}",
                            self.ae2_cpu_label(cpu),
                            pending,
                            flight,
                            if cancelled { " (cancelling)" } else { "" }
                        )));
                    }
                }
                return Ok(());
            }
            let entries = match entries_override {
                Some(v) => v.to_vec(),
                None => {let all=self.ae2_terminal_entries(pos)?;terminal_pinned_entries(&all,all.clone(),&self.ae2_job_ui.keys(),page_size)},
            };
            self.ae2_terminal_page = self
                .ae2_terminal_page
                .min(entries.len().saturating_sub(1) / page_size);
            if index < page_size && entries.get(self.ae2_terminal_page*page_size+index).is_some_and(|(key,_)|key.item_id.is_empty()) {return Ok(());}
            if index == PREV {
                self.ae2_terminal_page = self.ae2_terminal_page.saturating_sub(1);
                return Ok(());
            }
            if index == NEXT {
                if (self.ae2_terminal_page + 1) * page_size < entries.len() {
                    self.ae2_terminal_page += 1;
                }
                return Ok(());
            }
            if index >= page_size {
                return Ok(());
            }
            if self.ae2_local_chest(pos) {
                let selected = entries.get(self.ae2_terminal_page * page_size + index);
                let supports_fluid = matches!(self.world.block_entities.get(&pos), Some(horizon_client::containers::BlockEntity::Ae2(m)) if m.slots.first().and_then(Option::as_ref).is_some_and(|stack|
                    self.ae2_read_cell(stack).is_ok_and(|cell|cell.config().channel==ae2_cell_core::Channel::Fluid || cell.entries_for(ae2_cell_core::Channel::Fluid).next().is_some())
                    || self.ae2_creative_cell_keys(stack).is_ok_and(|keys| keys.iter().any(|key| matches!(key, super::ae2_storage_devices::CreativeCellKey::Fluid(_))))));
                if input == Input::Pickup && supports_fluid {
                    if let Some(cursor) = self
                        .carried
                        .clone()
                        .filter(|s| s.count == 1 && s.damage == 0 && s.data().is_none())
                    {
                        if let Some(fluid) = super::ae2_fluids::bucket_fluid(&cursor.item) {
                            let marker = fluid_marker(&super::ae2_fluids::fluid_key(fluid));
                            if self.ae2_chest_transfer(pos, &marker, 1000, true)? == 1000 {
                                self.carried = Some(ItemStack::new("minecraft:bucket", 1));
                            }
                            return Ok(());
                        }
                        if cursor.item.as_ref() == "minecraft:bucket" {
                            if let Some((key, amount)) = selected {
                                if let Some(fluid) = marker_fluid(key) {
                                    if *amount >= 1000
                                        && fluid == super::ae2_fluids::fluid_key(&fluid.item_id)
                                    {
                                        if let Some(bucket) =
                                            super::ae2_fluids::fluid_bucket(&fluid.item_id)
                                        {
                                            if self.ae2_chest_transfer(pos, key, 1000, false)?
                                                == 1000
                                            {
                                                self.carried = Some(ItemStack::new(bucket, 1));
                                            }
                                        }
                                    }
                                    return Ok(());
                                }
                            }
                        }
                    }
                }
                if selected.is_some_and(|(key, _)| marker_fluid(key).is_some()) {
                    return Ok(());
                }
            }
            if input == Input::QuickMove {
                if let Some((key, _)) = entries.get(self.ae2_terminal_page * page_size + index) {
                    if marker_fluid(key).is_some()
                        || self.ae2_craftable_outputs(pos).iter().any(|k| k == key)
                    {
                        let cpu = if let Some(fluid) = marker_fluid(key) {
                            self.ae2_begin_recursive_fluid_craft(
                                pos,
                                &fluid,
                                craft_amount
                                    .checked_mul(1000)
                                    .ok_or("craft amount overflow")?,
                            )?
                        } else {
                            self.ae2_begin_recursive_craft(pos, key, craft_amount)?
                        };
                        self.system_message(Text::literal(format!(
                            "Crafting started on CPU {}",
                            self.ae2_cpu_label(cpu)
                        )));
                    }
                }
                return Ok(());
            }
            if let Some(carried) = self.carried.clone() {
                let requested = if button == 1 {
                    1
                } else {
                    u64::from(carried.count)
                };
                let moved = self.ae2_terminal_insert(pos, &carried, requested)?;
                if moved > requested || moved > u64::from(carried.count) {
                    return Err("invalid grid insert count".into());
                }
                let left = carried.count - moved as u32;
                self.carried = (left > 0).then_some(ItemStack {
                    count: left,
                    ..carried
                });
            } else if let Some((key, count)) =
                entries.get(self.ae2_terminal_page * page_size + index)
            {
                if *count == 0 {
                    let cpu = if let Some(fluid) = marker_fluid(key) {
                        self.ae2_begin_recursive_fluid_craft(
                            pos,
                            &fluid,
                            craft_amount
                                .checked_mul(1000)
                                .ok_or("craft amount overflow")?,
                        )?
                    } else {
                        self.ae2_begin_recursive_craft(pos, key, craft_amount)?
                    };
                    self.system_message(Text::literal(format!(
                        "Crafting started on CPU {}",
                        self.ae2_cpu_label(cpu)
                    )));
                    return Ok(());
                }
                let exemplar = ae2_lab::prototype(key)?;
                let max = self.res.items().stack_max(&exemplar);
                let amount = if button == 1 {
                    u64::from(max).min(*count).div_ceil(2)
                } else {
                    u64::from(max).min(*count)
                };
                if amount > 0 {
                    self.carried = self.ae2_terminal_extract(pos, key, amount)?;
                }
            }
            return Ok(());
        }
        if slot >= 36 {
            return Ok(());
        }
        if input == Input::QuickMove {
            if !self.ae2_terminal_powered(pos) {
                return Err("terminal is offline: check power and channels".into());
            }
            if let Some(stack) = self.inventory[slot].clone() {
                let moved = self.ae2_terminal_insert(pos, &stack, u64::from(stack.count))?;
                if moved > u64::from(stack.count) {
                    return Err("invalid grid insert count".into());
                }
                let left = stack.count - moved as u32;
                self.inventory[slot] = (left > 0).then_some(ItemStack {
                    count: left,
                    ..stack
                });
            }
            return Ok(());
        }
        if input != Input::Pickup || !(0..=1).contains(&button) {
            return Ok(());
        }
        // The source item's own click override still applies to player slots
        // when the surrounding menu is an ME terminal.
        if self.inventory[slot].as_ref().is_some_and(|item|item.item.as_ref()=="ae2:wrapped_generic_stack") {
            let (inventory,carried,drops)={
                let env=self.menu_env();let mut menu=horizon_client::menu::Menu::new(horizon_client::menu::Kind::Inventory);
                for (i,item) in self.inventory.iter().enumerate(){menu.set_player_item(i,item.clone());}
                menu.carried=self.carried.clone();
                let index=if slot<9 {36+slot}else{slot};
                menu.click(&env,index as i32,button,input).map_err(str::to_owned)?;
                let inventory=(0..36).map(|i|menu.player_item(i).cloned()).collect();
                (inventory,menu.carried.clone(),std::mem::take(&mut menu.dropped))
            };
            self.inventory=inventory;self.carried=carried;
            for stack in drops {self.player_drop(stack);}
            return Ok(());
        }
        let held = self.carried.take();
        let item = self.inventory[slot].take();
        match (held, item, button) {
            (None, None, _) => {}
            (None, Some(item), 0) => {
                self.carried = Some(item);
            }
            (None, Some(mut item), 1) => {
                let n = item.count.div_ceil(2);
                item.count -= n;
                self.carried = Some(ItemStack {
                    count: n,
                    ..item.clone()
                });
                if item.count > 0 {
                    self.inventory[slot] = Some(item);
                }
            }
            (Some(mut held), None, b) => {
                let limit = self.res.items().stack_max(&held);
                let n = held.count.min(if b == 1 { 1 } else { limit });
                self.inventory[slot] = Some(ItemStack { count: n, ..held.clone() });
                held.count -= n;
                if held.count > 0 { self.carried = Some(held); }
            }
            (Some(mut held), Some(mut item), b) if same(&held, &item) => {
                let max = self.res.items().stack_max(&item);
                let n = max
                    .saturating_sub(item.count)
                    .min(if b == 1 { 1 } else { held.count });
                item.count += n;
                held.count -= n;
                self.inventory[slot] = Some(item);
                if held.count > 0 {
                    self.carried = Some(held);
                }
            }
            (Some(held), Some(item), _) if held.count <= self.res.items().stack_max(&held) => {
                self.inventory[slot] = Some(held);
                self.carried = Some(item);
            }
            (held, item, _) => {
                self.carried = held;
                self.inventory[slot] = item;
            }
        }
        Ok(())
    }
}

/// Ephemeral per-connection menu state. It contains no item storage.
struct ViewCellDrag { carried:ItemStack, slots:Vec<usize> }

// Inventory.getItem uses 0..35 for the main inventory, 36..39 for armor
// (feet through head), and 40 for the off hand. The locator remains tied
// to this slot even if another Network Tool replaces the original stack.
fn toolbox_owner(g: &horizon_client::lan::guest::Guest) -> Option<usize> {
    (0..=40).find(|&i| toolbox_stack(g, i).is_some_and(|s|
        s.item.as_ref() == horizon_client::ae2_network_tool::TOOL))
}
fn toolbox_stack(g: &horizon_client::lan::guest::Guest, i: usize) -> Option<&ItemStack> {
    match i {
        0..=35 => g.inventory.get(i)?.as_ref(),
        36..=39 => g.armor.get(39 - i)?.as_ref(),
        40 => g.offhand.as_ref(),
        _ => None,
    }
}
fn toolbox_stack_mut(g: &mut horizon_client::lan::guest::Guest, i: usize) -> Option<&mut ItemStack> {
    match i {
        0..=35 => g.inventory.get_mut(i)?.as_mut(),
        36..=39 => g.armor.get_mut(39 - i)?.as_mut(),
        40 => g.offhand.as_mut(),
        _ => None,
    }
}
// Menu::set_slot caps main-container entries. Refuse an invalid pre-existing
// overstack before staging any of the nine slots so an unrelated click cannot
// silently discard the excess.
fn toolbox_staging_safe(
    pocket: &horizon_client::ae2_network_tool::Inventory,
    max: impl Fn(&ItemStack) -> u32,
) -> bool {
    pocket.slots.iter().flatten().all(|s| s.count > 0 && s.count <= max(s).max(1).min(99))
}

/// A quick-craft gesture is pending input, never an owner of the stacks.
struct CraftMatrixDrag {
    identity: (u64, [i32; 4]),
    mode: i32,
    creative: bool,
    carried: ItemStack,
    inventory: Vec<Option<ItemStack>>,
    matrix: Vec<Option<ItemStack>>,
    slots: Vec<i32>,
}

/// A wireless drag reserves no items: it is replayed only after the source,
/// cursor and other player slots still match this pending gesture.
struct WirelessDrag {
    source: usize,
    identity: horizon_client::ae2_stack_identity::StackRef,
    mode: i32,
    creative: bool,
    carried: ItemStack,
    inventory: Vec<Option<ItemStack>>,
    armor: [Option<ItemStack>; 4],
    offhand: Option<ItemStack>,
    matrix: Option<Vec<Option<ItemStack>>>,
    slots: Vec<i32>,
}
fn wireless_unchanged_inventory(
    source: usize, before: &[Option<ItemStack>], after: &[Option<ItemStack>],
    old_offhand: &Option<ItemStack>, new_offhand: &Option<ItemStack>,
) -> bool {
    before.len() == after.len()
        && before.iter().zip(after).enumerate().all(|(i, (a, b))| i == source || a == b)
        && (source == super::shield::OFFHAND_SLOT || old_offhand == new_offhand)
}

#[derive(Clone)]
enum CraftOwner {
    Part(super::ae2_parts::PartAddress),
    Wireless(super::ae2_wireless::WirelessContext),
}
fn craft_owner(
    part: Option<super::ae2_parts::PartAddress>,
    wireless: Option<&super::ae2_wireless::WirelessContext>,
) -> Option<CraftOwner> {
    part.map(CraftOwner::Part).or_else(|| wireless
        .filter(|ctx| ctx.item == "ae2:wireless_crafting_terminal")
        .cloned().map(CraftOwner::Wireless))
}

struct ToolboxDrag {
    owner: usize,
    tool: ItemStack,
    wireless: Option<(usize, horizon_client::ae2_stack_identity::StackRef)>,
    identity: Option<(u64, [i32; 4])>,
    mode: i32,
    creative: bool,
    carried: ItemStack,
    inventory: Vec<Option<ItemStack>>,
    armor: [Option<ItemStack>; 4],
    offhand: Option<ItemStack>,
    matrix: Vec<Option<ItemStack>>,
    slots: Vec<i32>,
}

/// A remote wireless terminal can expose a toolbox without a view-cell panel.
pub(super) fn terminal_visible_slots(view_cells: bool, toolbox: bool) -> Vec<usize> {
    let mut slots: Vec<usize> = (0..36).chain(300..354).collect();
    if view_cells { slots.extend(360..365); }
    if toolbox { slots.extend(365..374); }
    slots
}

// Two independent item hosts may both be protected by one terminal menu.
fn wireless_toolbox_swap_refused(source: usize, toolbox: Option<usize>, button: i8) -> bool {
    source == 40 || usize::try_from(button).ok() == Some(source)
        || toolbox.is_some_and(|owner| owner == 40 || (owner < 9 && usize::try_from(button).ok() == Some(owner)))
}
fn toolbox_pickup_locked(index: usize, owner: usize, wireless_source: Option<usize>) -> bool {
    (9..45).contains(&index) && (index == 9 + owner || wireless_source.is_some_and(|source| index == 9 + source))
}
fn toolbox_drag_slot(slot: i16, matrix: bool, owner: usize, wireless_source: Option<usize>) -> Option<i32> {
    match slot {
        95..=103 => Some(i32::from(slot - 95)),
        27..=35 if matrix => Some(i32::from(slot - 27 + 9)),
        54..=80 => {let source=(slot-54+9) as usize;(source != owner && wireless_source != Some(source)).then_some(i32::from(slot-54+18))},
        81..=89 => {let source=(slot-81) as usize;(source != owner && wireless_source != Some(source)).then_some(i32::from(slot-81+45))},
        _ => None,
    }
}

/// Convert only physical terminal slots to the native crafting menu.
fn craft_drag_menu_slot(slot: i16) -> Option<i32> {
    match slot {
        27..=35 => Some(i32::from(slot - 27 + 1)),
        54..=80 => Some(i32::from(slot - 54 + 10)),
        81..=89 => Some(i32::from(slot - 81 + 37)),
        _ => None,
    }
}

pub(super) struct GuestTerminal {
    window: i32,
    state: i32,
    pos: (i32, i32, i32),
    wireless: Option<super::ae2_wireless::WirelessContext>,
    wireless_connected: bool,
    key_type_edit: bool,
    owner_dim: String,
    craft_part: Option<super::ae2_parts::PartAddress>,
    encoding_part: Option<super::ae2_parts::PartAddress>,
    view_part: Option<super::ae2_parts::PartAddress>,
    view_identity: Option<(u64,[i32;4])>,
    chest_identity: Option<u64>,
    chest_revisions: std::collections::VecDeque<(i32, ChestTerminalSettings)>,
    priority_edit: Option<PriorityEdit>,
    processing_amount_edit: Option<ProcessingAmountEdit>,
    toolbox_owner: Option<usize>,
    toolbox_snapshot: Option<ItemStack>,
    player_snapshot: Vec<Option<ItemStack>>,
    view_drag: Option<ViewCellDrag>,
    craft_drag: Option<CraftMatrixDrag>,
    wireless_drag: Option<WirelessDrag>,
    toolbox_drag: Option<ToolboxDrag>,
    encoding_identity: Option<(u64,[i32;4])>,
    encoding_snapshot: Option<horizon_client::ae2_world::Machine>,
    encoding_revisions: std::collections::VecDeque<(i32, Option<horizon_client::ae2_world::Machine>, usize)>,
    craft_snapshot: Vec<Option<ItemStack>>,
    page: usize,
    job_index: usize,
    craft_amount: u64,
    craft_amount_edit: bool,
    encoding: bool,
    pattern_grid: Vec<Option<ItemStack>>,
    substitutes: bool,
    fluid_substitutes: bool,
    processing: bool,
    special_mode:u8,
    stone_recipe:Option<String>,
    processing_outputs: Vec<Option<ItemStack>>,
    fluid_mode: bool,
    processing_page: usize,
    selected_fluid: usize,
    sort_mode: u8,
    sort_descending: bool,
    filter_mode: u8,
    search: String,
    search_mod_name: bool,
    search_language: Arc<horizon_client::lang::Language>,
    search_edit: bool,
    pinned: Vec<ae2_cell_core::ItemKey>,
    fluid_amount: u64,
    fluid_inputs: Vec<Option<super::ae2_processing::PatternFluid>>,
    fluid_outputs: Vec<Option<super::ae2_processing::PatternFluid>>,
    snapshot: Vec<(ae2_cell_core::ItemKey, u64)>,
    displayed_snapshot: Vec<(ae2_cell_core::ItemKey, u64)>,
    // A source MEInteractionPacket identifies a displayed key, not a vanilla
    // inventory state ID. Retain bounded per-slot identities for queued clicks.
    storage_revisions: std::collections::VecDeque<(i32,Vec<Option<ae2_cell_core::ItemKey>>)>,
}
fn terminal_view_keys(entries:&[(ae2_cell_core::ItemKey,u64)],page:usize,page_size:usize)->Vec<Option<ae2_cell_core::ItemKey>> {
    let page=page.min(entries.len().saturating_sub(1)/page_size);
    (0..page_size).map(|slot|entries.get(page*page_size+slot)
        .filter(|(key,_)|!key.item_id.is_empty()).map(|(key,_)|key.clone())).collect()
}
fn terminal_view_revision_matches(
    revisions:&std::collections::VecDeque<(i32,Vec<Option<ae2_cell_core::ItemKey>>)>,
    state:i32,current:&[(ae2_cell_core::ItemKey,u64)],page:usize,page_size:usize,slot:usize,
)->bool {
    let visible=terminal_view_keys(current,page,page_size);
    revisions.iter().rev().find(|(revision,_)|*revision==state)
        .is_some_and(|(_,shown)|shown.get(slot).zip(visible.get(slot)).is_some_and(|(old,new)|old==new))
}
fn encoding_revision_matches(
    revisions:&std::collections::VecDeque<(i32,Option<horizon_client::ae2_world::Machine>,usize)>,
    state:i32,current:Option<&horizon_client::ae2_world::Machine>,page:usize,ghost_slot:Option<i16>,
)->bool {
    revisions.iter().rev().find(|(revision,_,_)|*revision==state)
        .and_then(|(_,shown,shown_page)|Some((super::ae2_encoding::EncodingState::load(shown.as_ref()?).ok()?,super::ae2_encoding::EncodingState::load(current?).ok()?,*shown_page)))
        .is_some_and(|(shown,current,shown_page)|{
            if shown_page!=page {return false;}
            if shown==current {return true;}
            if shown.processing!=current.processing || shown.special_mode!=current.special_mode
                || shown.substitutes!=current.substitutes || shown.fluid_substitutes!=current.fluid_substitutes || shown.stone_recipe!=current.stone_recipe {return false;}
            match ghost_slot {
                Some(slot @ 0..=8)=>{let index=usize::try_from(slot).unwrap_or(0)+if current.processing {page.min(8)*9}else{0};shown.inputs[index]==current.inputs[index] && shown.fluid_inputs[index]==current.fluid_inputs[index]},
                Some(slot @ 18..=20) if current.processing=>{let index=usize::try_from(slot-18).unwrap_or(0)+page.min(8)*3;shown.outputs[index]==current.outputs[index] && shown.fluid_outputs[index]==current.fluid_outputs[index]},
                _=>false,
            }
        })
}
// Fake-slot packets, like source InventoryActionPacket, act on the current
// authoritative ghost; retain only the visible page and mode constraints.
fn encoding_drag_revision_matches(
    revisions:&std::collections::VecDeque<(i32,Option<horizon_client::ae2_world::Machine>,usize)>,
    state:i32,current:Option<&horizon_client::ae2_world::Machine>,page:usize,
)->bool {
    revisions.iter().rev().find(|(revision,_,_)|*revision==state)
        .and_then(|(_,shown,shown_page)|Some((super::ae2_encoding::EncodingState::load(shown.as_ref()?).ok()?,super::ae2_encoding::EncodingState::load(current?).ok()?,*shown_page)))
        .is_some_and(|(shown,current,shown_page)|shown_page==page && shown.processing==current.processing && shown.special_mode==current.special_mode)
}
fn chest_revision_matches(revisions:&std::collections::VecDeque<(i32,ChestTerminalSettings)>, state:i32, current:ChestTerminalSettings)->bool {
    revisions.iter().rev().find(|(revision,_)|*revision==state).is_some_and(|(_,shown)|*shown==current)
}

impl Game<'_> {
    pub(super) fn lan_ae2_wireless_source_open(
        &self, host: &super::lan::LanHost, conn: u32, source: usize,
    ) -> bool {
        host.play.get(&conn).and_then(|p| p.ae2_terminal.as_ref())
            .and_then(|t| t.wireless.as_ref()).is_some_and(|ctx| ctx.source == source)
    }
    fn lan_craft_matrix_read(
        &self, host: &super::lan::LanHost, conn: u32, owner: &CraftOwner,
    ) -> Result<horizon_client::ae2_world::Machine, String> {
        use horizon_client::ae2_world::{Machine, MachineKind};
        match owner {
            CraftOwner::Part(part) => self.ae2_part_machine(*part)?.ok_or("missing crafting terminal".into()),
            CraftOwner::Wireless(ctx) => {
                if !self.ae2_wireless_source_valid(host, conn, ctx) { return Err("wireless crafting terminal moved".into()); }
                let stack = host.hub.guests.iter().find(|g| g.conn == conn)
                    .and_then(|g| toolbox_stack(g, ctx.source)).ok_or("wireless crafting terminal moved")?;
                let mut machine = Machine::new(MachineKind::CraftingTerminal);
                machine.slots = super::ae2_wireless::crafting_grid(stack)?;
                Ok(machine)
            }
        }
    }
    fn lan_craft_matrix_store(
        &mut self, host: &mut super::lan::LanHost, conn: u32, owner: &CraftOwner,
        machine: &horizon_client::ae2_world::Machine,
    ) -> Result<(), String> {
        match owner {
            CraftOwner::Part(part) => self.ae2_store_part_machine(*part, machine),
            CraftOwner::Wireless(ctx) => {
                if !self.ae2_wireless_source_valid(host, conn, ctx) { return Err("wireless crafting terminal moved".into()); }
                let stack = host.hub.guests.iter_mut().find(|g| g.conn == conn)
                    .and_then(|g| toolbox_stack_mut(g, ctx.source)).ok_or("wireless crafting terminal moved")?;
                super::ae2_wireless::store_crafting_grid(stack, &machine.slots)
            }
        }
    }
    fn lan_craft_grid_extract(
        &mut self, host: &mut super::lan::LanHost, conn: u32, owner: &CraftOwner,
        pos: (i32, i32, i32), key: &ae2_cell_core::ItemKey,
        plan: &mut super::ae2_buses::FormationPlan,
    ) -> Result<Option<ItemStack>, String> {
        match owner {
            CraftOwner::Part(_) => self.ae2_grid_extract_staged(pos, key, 1, plan),
            CraftOwner::Wireless(ctx) => self.as_guest(host, conn, |game| {
                let previous = game.ae2_wireless_power.replace(ctx.source);
                let result = game.ae2_grid_extract_staged(pos, key, 1, plan);
                game.ae2_wireless_power = previous;
                result
            }).ok_or_else(|| "missing wireless player".to_string())?,
        }
    }
    fn lan_craft_grid_insert(
        &mut self, host: &mut super::lan::LanHost, conn: u32, owner: &CraftOwner,
        address: super::ae2_global::GridAddress, item: &ItemStack,
        plan: &mut super::ae2_buses::FormationPlan,
    ) -> Result<u64, String> {
        match owner {
            CraftOwner::Part(_) => self.ae2_grid_insert_staged_encoded(address, item, item.count as u64, plan),
            CraftOwner::Wireless(ctx) => self.as_guest(host, conn, |game| {
                let previous = game.ae2_wireless_power.replace(ctx.source);
                let result = game.ae2_grid_insert_staged_encoded(address, item, item.count as u64, plan);
                game.ae2_wireless_power = previous;
                result
            }).ok_or_else(|| "missing wireless player".to_string())?,
        }
    }
    fn lan_load_encoding_state(&self,host:&mut super::lan::LanHost,conn:u32)->bool {
        let Some(part)=host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref()).and_then(|t|t.encoding_part) else{return false;};
        let Some(machine)=self.ae2_part_machine(part).ok().flatten() else{return false;};
        let identity=host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref()).and_then(|t|t.encoding_identity);
        if self.ae2_encoding_identity(part)!=identity || identity.is_none(){return false;}
        let Ok(state)=super::ae2_encoding::EncodingState::load(&machine) else{return false;};
        let Some(t)=host.play.get_mut(&conn).and_then(|p|p.ae2_terminal.as_mut()) else{return false;};
        t.pattern_grid=state.inputs;t.processing_outputs=state.outputs;t.fluid_inputs=state.fluid_inputs;t.fluid_outputs=state.fluid_outputs;
        t.substitutes=state.substitutes;t.fluid_substitutes=state.fluid_substitutes;t.processing=state.processing;t.special_mode=state.special_mode;t.stone_recipe=state.stone_recipe;t.encoding_snapshot=Some(machine);true
    }
    fn lan_store_encoding_state(&mut self,host:&super::lan::LanHost,conn:u32)->Result<(),String> {
        let t=host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref()).ok_or("encoding session closed")?;
        let part=t.encoding_part.ok_or("encoding part missing")?;
        let mut state=super::ae2_encoding::EncodingState{inputs:t.pattern_grid.clone(),outputs:t.processing_outputs.clone(),fluid_inputs:t.fluid_inputs.clone(),fluid_outputs:t.fluid_outputs.clone(),substitutes:t.substitutes,fluid_substitutes:t.fluid_substitutes,processing:t.processing,special_mode:t.special_mode,stone_recipe:t.stone_recipe.clone()};
        state.normalize_crafting_inputs();
        let mut machine=self.ae2_part_machine(part)?.ok_or("encoding part missing")?;
        state.save(&mut machine)?;self.ae2_store_part_machine(part,&machine)
    }
    fn lan_encoding_slot_click(&mut self,host:&mut super::lan::LanHost,conn:u32,part:super::ae2_parts::PartAddress,slot:usize,button:i8,input:horizon_client::menu::Input)->Result<(),String> {
        let Some(gi)=host.hub.guests.iter().position(|g|g.conn==conn) else{return Err("player missing".into())};
        let mut machine=self.ae2_part_machine(part)?.ok_or("encoding part missing")?;
        if slot>=2 || machine.slots.len()!=2{return Err("invalid encoding slot".into());}
        let before_inventory=host.hub.guests[gi].inventory.clone();let before_cursor=host.hub.guests[gi].carried.clone();
        let old=machine.slots[slot].clone();
        let accepts=|s:&ItemStack|if slot==0 {s.item.as_ref()=="ae2:blank_pattern"} else {s.item.as_ref()=="ae2:blank_pattern" || super::ae2_processing::decode_pattern(s).is_some() || super::ae2_processing::ae2_decode_processing_pattern(s).is_some() || super::ae2_processing::ae2_decode_special_pattern(s).is_some()};
        if input==horizon_client::menu::Input::QuickMove {
            if let Some(item)=machine.slots[slot].take(){
                let Some(remaining)=self.as_guest(host,conn,|g|g.give(item)) else{return Err("player unavailable".into())};
                machine.slots[slot]=remaining;
            }
        } else if input==horizon_client::menu::Input::Pickup && matches!(button,0|1) {
            let cursor=host.hub.guests[gi].carried.take();let stored=machine.slots[slot].take();
            match (cursor,stored) {
                (None,Some(mut item))=>{let n=if button==1 {(item.count+1)/2}else{item.count};let mut taken=item.clone();taken.count=n;item.count-=n;host.hub.guests[gi].carried=Some(taken);if item.count>0 {machine.slots[slot]=Some(item);}}
                (Some(mut held),None) if accepts(&held)=>{let limit=self.res.items().stack_max(&held).min(if slot==1 {1}else{64});let n=if button==1 {1.min(limit)}else{held.count.min(limit)};let mut put=held.clone();put.count=n;held.count-=n;machine.slots[slot]=Some(put);if held.count>0 {host.hub.guests[gi].carried=Some(held);}}
                (Some(mut held),Some(mut item)) if slot==0 && accepts(&held) && same(&held,&item)=>{let n=self.res.items().stack_max(&held).min(64).saturating_sub(item.count).min(if button==1 {1}else{held.count});held.count-=n;item.count+=n;machine.slots[slot]=Some(item);if held.count>0 {host.hub.guests[gi].carried=Some(held);}}
                (Some(held),Some(item)) if button==0 && accepts(&held) && held.count<=self.res.items().stack_max(&held).min(if slot==1 {1}else{64})=>{machine.slots[slot]=Some(held);host.hub.guests[gi].carried=Some(item);}
                (held,item)=>{host.hub.guests[gi].carried=held;machine.slots[slot]=item;}
            }
        }
        if slot==1 && machine.slots[1]!=old {if let Some(pattern)=machine.slots[1].as_ref().and_then(super::ae2_encoding::EncodingState::from_pattern){
            if let Err(e)=pattern.save(&mut machine){host.hub.guests[gi].inventory=before_inventory;host.hub.guests[gi].carried=before_cursor;return Err(e);}
        }}
        if let Err(e)=self.ae2_store_part_machine(part,&machine){host.hub.guests[gi].inventory=before_inventory;host.hub.guests[gi].carried=before_cursor;return Err(e);}
        Ok(())
    }
    fn lan_encoding_quick_insert(&mut self,host:&mut super::lan::LanHost,conn:u32,part:super::ae2_parts::PartAddress,index:usize)->Result<bool,String> {
        let gi=host.hub.guests.iter().position(|g|g.conn==conn).ok_or("player missing")?;
        let Some(mut item)=host.hub.guests[gi].inventory.get(index).cloned().flatten() else{return Ok(false)};
        let slot=if item.item.as_ref()=="ae2:blank_pattern" {0} else if super::ae2_encoding::EncodingState::from_pattern(&item).is_some() {1} else{return Ok(false)};
        let mut machine=self.ae2_part_machine(part)?.ok_or("encoding part missing")?;
        let capacity=self.res.items().stack_max(&item).min(if slot==0 {64}else{1});
        let room=match machine.slots[slot].as_ref() {None=>capacity,Some(existing) if slot==0 && same(existing,&item)=>capacity.saturating_sub(existing.count),_=>0};
        let moved=item.count.min(room);if moved==0{return Ok(true)};
        if let Some(existing)=machine.slots[slot].as_mut(){existing.count+=moved;}else{let mut placed=item.clone();placed.count=moved;machine.slots[slot]=Some(placed);}
        item.count-=moved;
        if slot==1 {if let Some(state)=machine.slots[1].as_ref().and_then(super::ae2_encoding::EncodingState::from_pattern){state.save(&mut machine)?;}}
        self.ae2_store_part_machine(part,&machine)?;
        host.hub.guests[gi].inventory[index]=(item.count>0).then_some(item);Ok(true)
    }
    pub(super) fn lan_open_ae2_terminal(
        &mut self,
        host: &mut super::lan::LanHost,
        conn: u32,
        pos: (i32, i32, i32),
    ) -> bool {
        self.lan_open_ae2_terminal_inner(host, conn, pos, None, None, None, None)
    }
    pub(super) fn lan_open_ae2_view_terminal(&mut self,host:&mut super::lan::LanHost,conn:u32,pos:(i32,i32,i32),part:super::ae2_parts::PartAddress)->bool {
        if !self.ae2_part_machine(part).ok().flatten().is_some_and(|m|m.kind==horizon_client::ae2_world::MachineKind::Terminal) {return false;}
        self.lan_open_ae2_terminal_inner(host,conn,pos,None,None,None,Some(part))
    }
    pub(super) fn lan_open_ae2_wireless_terminal(
        &mut self,
        host: &mut super::lan::LanHost,
        conn: u32,
        pos: (i32, i32, i32),
        context: super::ae2_wireless::WirelessContext,
    ) -> bool {
        self.lan_open_ae2_terminal_inner(host, conn, pos, Some(context), None, None, None)
    }
    pub(super) fn lan_open_ae2_crafting_terminal(
        &mut self,
        host: &mut super::lan::LanHost,
        conn: u32,
        pos: (i32, i32, i32),
        part: super::ae2_parts::PartAddress,
    ) -> bool {
        if !self
            .ae2_part_machine(part)
            .ok()
            .flatten()
            .is_some_and(|m| m.kind == horizon_client::ae2_world::MachineKind::CraftingTerminal)
        {
            return false;
        }
        self.lan_open_ae2_terminal_inner(host, conn, pos, None, Some(part), None, Some(part))
    }
    pub(super) fn lan_open_ae2_encoding_terminal(&mut self,host:&mut super::lan::LanHost,conn:u32,pos:(i32,i32,i32),part:super::ae2_parts::PartAddress)->bool {
        let Some(machine)=self.ae2_part_machine(part).ok().flatten() else{return false;};
        if machine.kind!=horizon_client::ae2_world::MachineKind::PatternEncodingTerminal || super::ae2_encoding::EncodingState::load(&machine).is_err(){return false;}
        self.lan_open_ae2_terminal_inner(host,conn,pos,None,None,Some(part),Some(part))
    }
    fn lan_open_ae2_terminal_inner(
        &mut self,
        host: &mut super::lan::LanHost,
        conn: u32,
        pos: (i32, i32, i32),
        wireless: Option<super::ae2_wireless::WirelessContext>,
        craft_part: Option<super::ae2_parts::PartAddress>,
        encoding_part: Option<super::ae2_parts::PartAddress>,
        view_part: Option<super::ae2_parts::PartAddress>,
    ) -> bool {
        if wireless.as_ref().map_or_else(
            || !self.ae2_has_terminal(pos),
            |ctx| !self.ae2_wireless_validate(host, conn, ctx),
        ) {
            return false;
        }
        let view_identity=view_part.and_then(|part|self.ae2_terminal_part_identity(part));
        if view_part.is_some() && view_identity.is_none(){return false;}
        let encoding_identity=encoding_part.and_then(|part|self.ae2_encoding_identity(part));
        if encoding_part.is_some() && encoding_identity.is_none(){return false;}
        let chest = if wireless.is_none() && view_part.is_none() && self.ae2_local_chest(pos) {
            let Some(chest) = self.chest_terminal_read(pos) else { return false; };
            Some(chest)
        } else { None };
        self.lan_ae2_portable_closed(host, conn, None);
        self.lan_ae2_bus_closed(host, conn, None);
        self.lan_ae2_network_tool_closed(host, conn, None);
        self.lan_ae2_anchor_closed(host, conn, None);
        self.lan_craft_confirm_closed(host, conn, None);
        self.lan_pattern_access_closed(host, conn, None);
        self.lan_ae2_terminal_closed(host, conn, None);
        self.lan_ae2_machine_closed(host, conn, None);
        self.lan_station_closed(host, conn, None);
        self.lan_crafting_closed(host, conn, None);
        self.guest_container_closed(host, conn);
        let toolbox_owner = host.hub.guests.iter().find(|g| g.conn == conn).and_then(toolbox_owner);
        let Some(window) = host.hub.open_window(
            conn,
            "minecraft:generic_9x6",
            horizon_protocol::text::literal(if craft_owner(craft_part, wireless.as_ref()).is_some() {
                "AE2 Crafting Terminal"
            } else {
                "AE2 ME Terminal"
            }),
        ) else {
            return true;
        };
        let locale=host.hub.guests.iter().find(|guest|guest.conn==conn).map(|guest|search_language_code(&guest.info.language)).unwrap_or("en_us");
        let search_language=Arc::new(horizon_client::lang::Language::load(&[&self.options.resources],locale));
        host.play.entry(conn).or_default().ae2_terminal = Some(GuestTerminal {
            window,
            state: 0,
            pos,
            wireless,
            wireless_connected: true,
            key_type_edit: false,
            owner_dim: self.dim.id().to_owned(),
            craft_part,
            encoding_part,
            view_part,
            view_identity,
            chest_identity: chest.map(|(identity, _)| identity),
            chest_revisions: std::collections::VecDeque::new(),
            priority_edit: None,
            processing_amount_edit: None,
            toolbox_owner,
            toolbox_snapshot: None,
            player_snapshot: Vec::new(),
            view_drag: None,
            craft_drag: None,
            wireless_drag: None,
            toolbox_drag: None,
            encoding_identity,
            encoding_snapshot:None,
            encoding_revisions:std::collections::VecDeque::new(),
            craft_snapshot: Vec::new(),
            page: 0,
            job_index: 0,
            craft_amount: 1,
            craft_amount_edit: false,
            encoding: false,
            pattern_grid: vec![None; 81],
            substitutes: false,
            fluid_substitutes: false,
            processing: false,
            special_mode:0,
            stone_recipe:None,
            processing_outputs: vec![None; 27],
            fluid_mode: false,
            processing_page: 0,
            selected_fluid: 0,
            sort_mode: chest.map_or(0, |(_, settings)| settings.sort),
            sort_descending: chest.is_some_and(|(_, settings)| settings.descending),
            filter_mode: chest.map_or(0, |(_, settings)| settings.view),
            search: String::new(),
            search_mod_name: false,
            search_language,
            search_edit: false,
            pinned: Vec::new(),
            fluid_amount: 1000,
            fluid_inputs: vec![None; 81],
            fluid_outputs: vec![None; 27],
            snapshot: Vec::new(),
            displayed_snapshot: Vec::new(),
            storage_revisions: std::collections::VecDeque::new(),
        });
        if !self.lan_load_encoding_state(host,conn) && encoding_part.is_some() {self.lan_ae2_terminal_closed(host,conn,None);return false;}
        if encoding_part.is_some() {
            host.hub.send_to(conn,&C::CustomPayload(horizon_protocol::packets::configuration::CustomPayload {
                channel:"ae2:encoding_menu".into(),data:window.to_be_bytes().to_vec(),
            }));
        }
        self.lan_send_ae2_terminal(host, conn);
        true
    }
    /// PatternEncodingTermMenu.clear: only the exact, in-range encoding
    /// owner may clear its ghost inputs/outputs. Physical patterns remain.
    pub(super) fn lan_clear_encoding(&mut self,host:&mut super::lan::LanHost,conn:u32,data:&[u8]) {
        let Ok(bytes)=<[u8;4]>::try_from(data) else{return};
        let window=i32::from_be_bytes(bytes);
        let Some((pos,dimension))=host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref())
            .filter(|t|t.window==window && t.encoding_part.is_some())
            .map(|t|(t.pos,t.owner_dim.clone())) else{return};
        let valid=dimension==self.dim.id() && self.ae2_has_terminal(pos)
            && host.hub.guests.iter().find(|g|g.conn==conn).is_some_and(|g|
                !g.spectator() && g.within_block_range([pos.0,pos.1,pos.2],4.0));
        if !valid || !self.lan_load_encoding_state(host,conn) {return;}
        if let Some(t)=host.play.get_mut(&conn).and_then(|p|p.ae2_terminal.as_mut()) {
            t.pattern_grid.fill(None);t.processing_outputs.fill(None);
            t.fluid_inputs.fill(None);t.fluid_outputs.fill(None);t.stone_recipe=None;
        }
        if self.lan_store_encoding_state(host,conn).is_err() {self.lan_load_encoding_state(host,conn);}
        self.lan_send_ae2_terminal(host,conn);
    }
    pub(super) fn lan_ae2_terminal_search(
        &mut self,
        host: &mut super::lan::LanHost,
        conn: u32,
        value: &str,
    ) -> bool {
        let Some(t) = host
            .play
            .get_mut(&conn)
            .and_then(|p| p.ae2_terminal.as_mut())
        else {
            return false;
        };
        t.search = value.chars().take(128).collect();
        t.page = 0;
        t.search_edit = false;
        self.lan_send_ae2_terminal(host, conn);
        true
    }
    pub(super) fn lan_ae2_pinned_keys(&mut self,host:&mut super::lan::LanHost,conn:u32,data:&[u8]) {
        if data.len()>131_072{return;}
        let Ok(value)=serde_json::from_slice::<serde_json::Value>(data) else{return};
        let (Some(window),Some(values))=(value.get("window").and_then(|v|v.as_i64()),value.get("pins").and_then(|v|v.as_array())) else{return};
        if values.len()>9{return;}
        let mut pins=Vec::new();
        for value in values {
            let Some(key)=value.as_str().and_then(super::ae2_job_ui::parse_key) else{return};
            if !pins.contains(&key){pins.push(key);}
        }
        if !host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref()).is_some_and(|t|i64::from(t.window)==window && t.owner_dim==self.dim.id()) {return;}
        let include_mod=value.get("search_mod_name").and_then(|v|v.as_bool()).unwrap_or(false);
        let locale=value.get("language").and_then(|v|v.as_str()).map(search_language_code).map(str::to_owned);
        let language=locale.as_deref().filter(|code|host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref()).is_some_and(|t|t.search_language.code!=*code))
            .map(|code|Arc::new(horizon_client::lang::Language::load(&[&self.options.resources],code)));
        let Some(t)=host.play.get_mut(&conn).and_then(|p|p.ae2_terminal.as_mut())
            .filter(|t|i64::from(t.window)==window && t.owner_dim==self.dim.id()) else{return};
        if t.pinned==pins && t.search_mod_name==include_mod && language.is_none(){return;}
        t.pinned=pins;t.search_mod_name=include_mod;if let Some(language)=language {t.search_language=language;}t.page=0;
        self.lan_send_ae2_terminal(host,conn);
    }
    fn lan_send_ae2_terminal(&mut self, host: &mut super::lan::LanHost, conn: u32) {
        if host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref()).is_some_and(|t|t.processing_amount_edit.is_some()){self.lan_send_processing_amount(host,conn);return;}
        if host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref()).is_some_and(|t|t.priority_edit.is_some()){self.lan_send_priority(host,conn);return;}
        if let Some((window,encoding,processing,special_mode))=host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref()).filter(|t|t.encoding_part.is_some()).map(|t|(t.window,t.encoding,t.processing,t.special_mode)) {
            let mut data=window.to_be_bytes().to_vec();data.push(u8::from(encoding && processing));
            host.hub.send_to(conn,&C::CustomPayload(horizon_protocol::packets::configuration::CustomPayload{channel:"ae2:encoding_state".into(),data}));
            let mut slots=Vec::new();
            if encoding {
                let inputs=match special_mode {1=>3,2=>1,_=>9};
                slots.extend(0..inputs);
                if processing {slots.extend(18..21);}
            }
            Self::lan_ae2_fake_slots(host,conn,window,&slots);
        }
        let chest = host.play.get(&conn).and_then(|p| p.ae2_terminal.as_ref())
            .and_then(|t| t.chest_identity.map(|identity| (t.pos, identity)));
        if let Some((pos, identity)) = chest {
            if let Some((_, settings)) = self.chest_terminal_read(pos).filter(|(now, _)| *now == identity) {
                if let Some(t) = host.play.get_mut(&conn).and_then(|p| p.ae2_terminal.as_mut()) {
                    if (t.sort_mode, t.filter_mode, t.sort_descending) != (settings.sort, settings.view, settings.descending) { t.page = 0; }
                    t.sort_mode = settings.sort; t.filter_mode = settings.view; t.sort_descending = settings.descending;
                }
            }
        }
        let toolbox = host.play.get(&conn).and_then(|p| p.ae2_terminal.as_ref())
            .and_then(|t| Some((t.window, t.toolbox_owner?)))
            .and_then(|(window, owner)| {
                let g = host.hub.guests.iter().find(|g| g.conn == conn)?;
                let stack = toolbox_stack(g, owner)?.clone();
                let inventory = horizon_client::ae2_network_tool::Inventory::read(&stack).ok()?;
                Some((window, owner, stack, inventory))
            });
        if let Some((window, owner, stack, inventory)) = toolbox {
            let slots: Vec<_> = inventory.slots.iter()
                .map(|v| horizon_client::item_data::canon_stack(v.as_ref())).collect();
            if let Ok(data) = serde_json::to_vec(&serde_json::json!({
                "window": window, "owner": owner, "slots": slots
            })) {
                host.hub.send_to(conn, &C::CustomPayload(
                    horizon_protocol::packets::configuration::CustomPayload {
                        channel: "ae2:terminal_toolbox".into(), data,
                    },
                ));
            }
            if let Some(t) = host.play.get_mut(&conn).and_then(|p| p.ae2_terminal.as_mut()) {
                t.toolbox_snapshot = Some(stack);
            }
        }
        if let Some((window,part,identity))=host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref()).and_then(|t|Some((t.window,t.view_part?,t.view_identity?))) {
            if self.ae2_terminal_part_identity(part)==Some(identity) {
                if let Some(machine)=self.ae2_part_machine(part).ok().flatten() {
                    if let Ok(cells)=super::ae2_parts::ae2_terminal_view_cells(&machine) {
                        let cells:Vec<_>=cells.iter().map(|v|horizon_client::item_data::canon_stack(v.as_ref())).collect();
                        if let Ok(data)=serde_json::to_vec(&serde_json::json!({"window":window,"cells":cells})) {
                            host.hub.send_to(conn,&C::CustomPayload(horizon_protocol::packets::configuration::CustomPayload{channel:"ae2:view_cells".into(),data}));
                        }
                    }
                }
            }
        }
        if host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref()).is_some_and(|t|t.encoding_part.is_some()) {self.lan_load_encoding_state(host,conn);}
        use horizon_protocol::packets::play::{self, Clientbound as C};
        let Some((
            pos,
            page,
            craft_amount,
            craft_amount_edit,
            encoding,
            pattern_grid,
            substitutes,
            fluid_substitutes,
            processing,
            special_mode,
            stone_recipe,
            processing_outputs,
            fluid_mode,
            processing_page,
            selected_fluid,
            sort_mode,
            sort_descending,
            filter_mode,
            search,
            search_edit,
            fluid_amount,
            fluid_inputs,
            fluid_outputs,
        )) = host
            .play
            .get(&conn)
            .and_then(|p| p.ae2_terminal.as_ref())
            .map(|t| {
                (
                    t.pos,
                    t.page,
                    t.craft_amount,
                    t.craft_amount_edit,
                    t.encoding,
                    t.pattern_grid.clone(),
                    t.substitutes,
                    t.fluid_substitutes,
                    t.processing,
                    t.special_mode,
                    t.stone_recipe.clone(),
                    t.processing_outputs.clone(),
                    t.fluid_mode,
                    t.processing_page,
                    t.selected_fluid,
                    t.sort_mode,
                    t.sort_descending,
                    t.filter_mode,
                    t.search.clone(),
                    t.search_edit,
                    t.fluid_amount,
                    t.fluid_inputs.clone(),
                    t.fluid_outputs.clone(),
                )
            })
        else {
            return;
        };
        let wireless = host
            .play
            .get(&conn)
            .and_then(|p| p.ae2_terminal.as_ref())
            .and_then(|t| t.wireless.clone());
        if wireless.as_ref().map_or_else(
            || !self.ae2_has_terminal(pos),
            |ctx| !self.ae2_wireless_source_valid(host, conn, ctx),
        ) {
            let window = host
                .play
                .get(&conn)
                .and_then(|p| p.ae2_terminal.as_ref())
                .map(|t| t.window);
            self.lan_ae2_terminal_closed(host, conn, None);
            if let Some(container) = window {
                host.hub
                    .send_to(conn, &C::ContainerClose(play::ContainerClose { container }));
            }
            return;
        }
        // An offline network has no available entries; the next click will
        // still be checked against the authoritative grid before any transfer.
        let connected = host
            .play
            .get(&conn)
            .and_then(|p| p.ae2_terminal.as_ref())
            .is_none_or(|t| t.wireless.is_none() || t.wireless_connected);
        let snapshot = if connected {
            self.ae2_terminal_entries(pos).unwrap_or_default()
        } else {
            Vec::new()
        };
        let mut craftable = self.ae2_craftable_outputs(pos);
        craftable.extend(self.ae2_craftable_fluids(pos).iter().map(fluid_marker));
        let view_part=host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref()).and_then(|t|t.view_part);
        let enabled_types=wireless_key_types(host,conn,wireless.as_ref());
        let view_entries=terminal_key_types(&self.ae2_view_filtered(&snapshot,view_part),enabled_types);
        let craft_part = host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref()).and_then(|t|t.craft_part);
        let owner = craft_owner(craft_part, wireless.as_ref());
        let page_size = if owner.is_some() { 27 } else { PAGE_ITEMS };
        let pins=host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref()).map(|t|t.pinned.clone()).unwrap_or_default();
        let Some((search_mod_name,search_language))=host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref()).map(|t|(t.search_mod_name,t.search_language.clone())) else{return;};
        let filtered = terminal_filter_entries(
            &view_entries,
            &craftable,
            sort_mode,
            sort_descending,
            filter_mode,
            &search,
            &|key|self.ae2_terminal_display_name(key,&search_language),
            &|key,query|self.ae2_terminal_search_matches(key,query,search_mod_name,&search_language),
        );
        let displayed=terminal_pinned_entries(&snapshot,filtered,&pins,page_size);
        let mut items: Vec<_> = (0..54)
            .map(|i| self.ae2_terminal_display(i, page, &displayed, page_size))
            .collect();
        let craft_grid = owner.as_ref()
            .and_then(|owner| self.lan_craft_matrix_read(host, conn, owner).ok())
            .map(|m| m.slots)
            .unwrap_or_default();
        if owner.is_some() && craft_grid.len() == 9 {
            items[27..36].clone_from_slice(&craft_grid);
            items[36] = self.res.recipes().find(&craft_grid, 3).map(|(_, out)| out);
            items[37] = Some(navigation("Clear crafting grid to network"));
            items[38] = Some(navigation("Clear crafting grid to inventory"));
        }
        items[46] = Some(navigation(&format!(
            "Sort: {}",
            match sort_mode { 1 => "amount", 2 => "mod", _ => "name" }
        )));
        items[51] = Some(navigation(if sort_descending {
            "Sort direction: descending"
        } else {
            "Sort direction: ascending"
        }));
        items[47] = Some(navigation(&format!(
            "Filter: {}",
            match filter_mode {
                1 => "stored",
                2 => "craftable",
                _ => "all",
            }
        )));
        items[50] = Some(navigation(&format!(
            "Craft amount: {craft_amount}; click to edit"
        )));
        if wireless.is_some() {
            let label = match enabled_types {
                (true,true)=>"Items and fluids",(true,false)=>"Items",(false,true)=>"Fluids",(false,false)=>"None",
            };
            items[52] = Some(navigation(&format!("Visible types: {label}; click to configure, shift-click to cycle")));
        }
        if host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref()).is_some_and(|t|t.chest_identity.is_some()) {
            items[52]=Some(navigation("Priority: click to edit"));
        }
        let job_index = host
            .play
            .get(&conn)
            .and_then(|p| p.ae2_terminal.as_ref())
            .map_or(0, |t| t.job_index);
        items[49] = if connected {
            self.ae2_job_button(pos, job_index)
        } else {
            None
        };
        let encoding_part=host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref()).and_then(|t|t.encoding_part);
        if craft_part.is_none() && encoding_part.is_some() {
            items[48] = Some(navigation(if encoding {
                "Back to storage"
            } else {
                "Encode crafting pattern"
            }));
            if encoding {
                items[..48].fill(None);
                if let Some(machine)=encoding_part.and_then(|part|self.ae2_part_machine(part).ok().flatten()) {
                    items[49]=machine.slots.first().cloned().flatten();items[50]=machine.slots.get(1).cloned().flatten();
                }
                let pattern_page = if processing {
                    processing_page.min(8)
                } else {
                    0
                };
                let input_offset = pattern_page * 9;
                let output_offset = pattern_page * 3;
                for (i,item) in pattern_grid[input_offset..input_offset+9].iter().enumerate() {
                    items[i]=item.as_ref().and_then(|item|encoding_item_icon(item,self.ae2_config_item_max(item)));
                }
                items[46] = Some(navigation(match (processing,special_mode) {(true,_)=>"Mode: processing",(_,1)=>"Mode: smithing",(_,2)=>"Mode: stonecutting",_=>"Mode: crafting"}));
                if processing {
                    items[45] = Some(navigation(if fluid_mode {
                        "Ghost type: fluid bucket"
                    } else {
                        "Ghost type: item"
                    }));
                    for digit in 0..10 {
                        items[30 + digit] =
                            Some(navigation(&format!("Fluid amount digit {digit}")));
                    }
                    items[40] = Some(navigation("Clear fluid amount"));
                    items[41] = Some(navigation(&format!("Fluid amount: {fluid_amount} mB")));
                    items[42] = Some(navigation("Previous processing page"));
                    items[43] = Some(navigation(&format!(
                        "Processing page {}/9; click for next",
                        pattern_page + 1
                    )));
                    let available_fluids = self.ae2_grid_fluids(pos).unwrap_or_default();
                    let selected =
                        available_fluids.get(selected_fluid % available_fluids.len().max(1));
                    items[44] = Some(navigation(&selected.map_or_else(
                        || "No network fluid selected".into(),
                        |(k, _)| {
                            format!(
                                "Selected fluid: {} ({} component bytes); click to cycle",
                                k.item_id,
                                k.components.len()
                            )
                        },
                    )));
                    for (i,item) in processing_outputs[output_offset..output_offset+3].iter().enumerate() {
                        items[18+i]=item.as_ref().and_then(|item|encoding_item_icon(item,self.ae2_config_item_max(item)));
                    }
                    for (i, f) in fluid_inputs[input_offset..input_offset + 9]
                        .iter()
                        .enumerate()
                    {
                        if let Some(f) = f {
                            items[i] = encoding_fluid_icon(f);
                        }
                    }
                    for (i, f) in fluid_outputs[output_offset..output_offset + 3]
                        .iter()
                        .enumerate()
                    {
                        if let Some(f) = f {
                            items[18 + i] = encoding_fluid_icon(f);
                        }
                    }
                    if (pattern_grid.iter().any(Option::is_some)
                        || fluid_inputs.iter().any(Option::is_some))
                        && (fluid_outputs.iter().any(Option::is_some)
                            || processing_outputs.iter().any(Option::is_some))
                    {
                        let mut preview = ItemStack::new("ae2:processing_pattern", 1);
                        preview.data_mut().custom_name =
                            Some("Encode item processing pattern".into());
                        items[13] = Some(preview);
                    }
                } else if special_mode!=0 {
                    items[47]=Some(navigation(if substitutes {"Substitutions: on"}else{"Substitutions: off"}));
                    let mut special_grid=vec![None;9];let count=if special_mode==1 {3}else{1};special_grid[..count].clone_from_slice(&pattern_grid[..count]);items[..9].fill(None);items[..count].clone_from_slice(&special_grid[..count]);
                    if special_mode==1 {
                        if let Some((_,result))=self.res.stations().recipes.smithing_recipe_result(&self.res.stations().enchantments,special_grid[0].as_ref(),special_grid[1].as_ref(),special_grid[2].as_ref()){items[13]=Some(result);}
                    } else if let Some(input)=special_grid[0].as_ref() {
                        let recipes=self.res.stations().recipes.stonecutter(input);
                        items[42]=Some(navigation("Previous stonecutting recipe"));items[43]=Some(navigation("Next stonecutting recipe"));
                        if let Some(recipe)=stone_recipe.as_ref().and_then(|id|recipes.iter().find(|r|&r.id==id)) {items[13]=Some(recipe.result.clone());}
                    }
                } else {
                    items[44] = Some(navigation(if fluid_substitutes {
                        "Fluid substitutions: on"
                    } else {
                        "Fluid substitutions: off"
                    }));
                    items[47] = Some(navigation(if substitutes {
                        "Substitutions: on"
                    } else {
                        "Substitutions: off"
                    }));
                    if let Some((_, result)) = self.res.recipes().find(&pattern_grid[..9], 3) {
                        let mut preview = result;
                        preview.data_mut().custom_name = Some("Click to encode pattern".into());
                        items[13] = Some(preview);
                    }
                }
            }
        }
        if encoding && craft_part.is_none() && encoding_part.is_some() && !processing {
            for (i,fluid) in fluid_inputs.iter().take(9).enumerate() {
                if (special_mode==1 && i>=3) || (special_mode==2 && i>=1) {continue;}
                if let Some(fluid)=fluid {items[i]=encoding_fluid_icon(fluid);}
            }
        }
        if craft_amount_edit && !encoding {
            items[..49].fill(None);
            for digit in 0..10 {
                items[digit] = Some(navigation(&format!("Craft amount digit {digit}")));
            }
            items[10] = Some(navigation("Clear craft amount"));
            items[11] = Some(navigation(&format!("Apply craft amount: {craft_amount}")));
        }
        if search_edit && !encoding {
            items[..49].fill(None);
            for (i, ch) in "abcdefghijklmnopqrstuvwxyz0123456789:_-./"
                .chars()
                .enumerate()
            {
                items[i] = Some(navigation(&format!("Search character: {ch}")));
            }
            items[41] = Some(navigation("Search: backspace"));
            items[42] = Some(navigation("Search: clear"));
            items[43] = Some(navigation(&format!("Apply search: {search}")));
        }
        if host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref()).is_some_and(|t|t.key_type_edit) {
            items[..54].fill(None);
            items[0]=Some(navigation(if enabled_types.0 {"Items: enabled"}else{"Items: disabled"}));
            items[1]=Some(navigation(if enabled_types.1 {"Fluids: enabled"}else{"Fluids: disabled"}));
            items[2]=Some(navigation("Back to storage"));
        }
        let Some(g) = host.hub.guests.iter().find(|g| g.conn == conn) else {
            return;
        };
        items.extend((9..36).chain(0..9).map(|i| g.inventory[i].clone()));
        let player_snapshot = g.inventory.clone();
        let carried = g.carried.clone();
        let Some(t) = host
            .play
            .get_mut(&conn)
            .and_then(|p| p.ae2_terminal.as_mut())
        else {
            return;
        };
        t.craft_snapshot = craft_grid;
        t.player_snapshot = player_snapshot;
        t.snapshot = snapshot;
        t.displayed_snapshot = displayed;
        t.state = (t.state + 1) & 0x7fff;
        if !t.encoding && !t.craft_amount_edit && !t.search_edit && !t.key_type_edit {
            t.storage_revisions.push_back((t.state,terminal_view_keys(&t.displayed_snapshot,page,page_size)));
            while t.storage_revisions.len()>64 {t.storage_revisions.pop_front();}
        }
        if t.chest_identity.is_some() {
            t.chest_revisions.push_back((t.state,ChestTerminalSettings{sort:t.sort_mode,view:t.filter_mode,descending:t.sort_descending}));
            while t.chest_revisions.len()>64 {t.chest_revisions.pop_front();}
        }
        if t.encoding_part.is_some() {
            t.encoding_revisions.push_back((t.state,t.encoding_snapshot.clone(),t.processing_page));
            while t.encoding_revisions.len()>64 {t.encoding_revisions.pop_front();}
        }
        let (window, state) = (t.window, t.state);
        let repo_slots:Vec<u8>=if !t.encoding && !t.craft_amount_edit && !t.search_edit && !t.key_type_edit {
            (0..page_size as u8).collect()
        } else {Vec::new()};
        Self::lan_ae2_repo_slots(host,conn,window,&repo_slots);
        let packet = C::ContainerSetContent(play::ContainerSetContent {
            container: window,
            state,
            items: items.iter().map(|s| host.hub.stack(s)).collect(),
            carried: host.hub.stack(&carried),
        });
        host.hub.send_to(conn, &packet);
    }
    /// Refresh only when the backing inventory changes, or close a removed part.
    pub(super) fn lan_refresh_ae2_terminals(&mut self, host: &mut super::lan::LanHost) {
        let viewers: Vec<_> = host
            .play
            .iter()
            .filter_map(|(&conn, p)| {
                if !host
                    .hub
                    .guests
                    .iter()
                    .any(|g| g.conn == conn && host.hub.here(g))
                {
                    return None;
                }
                p.ae2_terminal.as_ref().map(|t| {
                    (
                        conn,
                        t.pos,
                        t.snapshot.clone(),
                        t.wireless.clone(),
                        t.wireless_connected,
                        t.owner_dim.clone(),
                        t.craft_part,
                        t.craft_snapshot.clone(),
                        t.encoding_part,
                        t.encoding_identity,
                        t.encoding_snapshot.clone(),
                        t.view_part,
                        t.view_identity,
                        t.chest_identity,
                        ChestTerminalSettings { sort: t.sort_mode, view: t.filter_mode, descending: t.sort_descending },
                        t.toolbox_owner,
                        t.toolbox_snapshot.clone(),
                        t.player_snapshot.clone(),
                    )
                })
            })
            .collect();
        for (
            conn,
            mut pos,
            previous,
            mut wireless,
            was_connected,
            owner_dim,
            craft_part,
            craft_snapshot,
            encoding_part,
            encoding_identity,
            encoding_snapshot,
            view_part,
            view_identity,
            chest_identity,
            chest_settings,
            toolbox_owner,
            toolbox_snapshot,
            player_snapshot,
        ) in viewers
        {
            let source_valid = (wireless.is_some() || owner_dim == self.dim.id())
                && toolbox_owner.is_none_or(|owner| host.hub.guests.iter()
                    .find(|g| g.conn == conn)
                    .and_then(|g| toolbox_stack(g, owner))
                    .is_some_and(|s| horizon_client::ae2_network_tool::Inventory::read(s).is_ok()))
                && view_part.is_none_or(|part|view_identity.is_some() && self.ae2_terminal_part_identity(part)==view_identity)
                && chest_identity.is_none_or(|identity|self.chest_terminal_read(pos).is_some_and(|(current,_)|current==identity))
                && encoding_part.is_none_or(|part|encoding_identity.is_some() && self.ae2_encoding_identity(part)==encoding_identity && self.ae2_part_machine(part).ok().flatten().is_some_and(|m|m.kind==horizon_client::ae2_world::MachineKind::PatternEncodingTerminal && super::ae2_encoding::EncodingState::load(&m).is_ok()))
                && craft_part.is_none_or(|part| {
                    self.ae2_part_machine(part).ok().flatten().is_some_and(|m| {
                        m.kind == horizon_client::ae2_world::MachineKind::CraftingTerminal
                    })
                })
                && wireless.as_ref().map_or_else(
                    || {
                        host.hub
                            .guests
                            .iter()
                            .find(|g| g.conn == conn)
                            .is_some_and(|g| {
                                !g.spectator() && g.within_block_range([pos.0, pos.1, pos.2], 4.0)
                            })
                    },
                    |ctx| self.ae2_wireless_source_valid(host, conn, ctx),
                );
            let connected = source_valid
                && wireless
                    .as_mut()
                    .is_none_or(|ctx| self.ae2_wireless_tick(host, conn, ctx));
            let old_pos = pos;
            if connected {
                if let Some((nearest, _)) = wireless
                    .as_ref()
                    .and_then(|ctx| self.ae2_wireless_nearest(host, conn, ctx))
                {
                    pos = nearest;
                }
            }
            if let Some(t) = host
                .play
                .get_mut(&conn)
                .and_then(|p| p.ae2_terminal.as_mut())
            {
                t.wireless = wireless.clone();
                t.wireless_connected = connected;
                t.pos = pos;
            }
            if !source_valid {
                let window = host
                    .play
                    .get(&conn)
                    .and_then(|p| p.ae2_terminal.as_ref())
                    .map(|t| t.window);
                self.lan_ae2_terminal_closed(host, conn, None);
                if let Some(container) = window {
                    host.hub.send_to(
                        conn,
                        &horizon_protocol::packets::play::Clientbound::ContainerClose(
                            horizon_protocol::packets::play::ContainerClose { container },
                        ),
                    );
                }
                continue;
            }
            if (wireless.is_none() && !self.ae2_has_terminal(pos))
                || toolbox_owner.is_some_and(|owner| host.hub.guests.iter()
                    .find(|g| g.conn == conn)
                    .and_then(|g| toolbox_stack(g, owner)) != toolbox_snapshot.as_ref())
                || host.hub.guests.iter().find(|g| g.conn == conn)
                    .is_some_and(|g| g.inventory != player_snapshot)
                || connected != was_connected
                || pos != old_pos
                || chest_identity.is_some_and(|identity|self.chest_terminal_read(pos).is_some_and(|(current,settings)|current==identity && settings!=chest_settings))
                || craft_part.is_some_and(|part| {
                    self.ae2_part_machine(part)
                        .ok()
                        .flatten()
                        .is_none_or(|m| m.slots != craft_snapshot)
                })
                || craft_owner(None, wireless.as_ref()).is_some_and(|owner|
                    self.lan_craft_matrix_read(host, conn, &owner)
                        .map_or(true, |m| m.slots != craft_snapshot))
                || encoding_part.is_some_and(|part|self.ae2_part_machine(part).ok().flatten()!=encoding_snapshot)
                || (connected
                    && self
                        .ae2_terminal_entries(pos)
                        .map_or(!previous.is_empty(), |now| now != previous))
            {
                self.lan_send_ae2_terminal(host, conn);
            }
        }
    }
    /// A pending drag is a proposal only. Replay across the actual toolbox,
    /// player inventory and optional matrix after checking all their owners.
    fn lan_terminal_toolbox_drag(
        &mut self, host: &mut super::lan::LanHost, conn: u32,
        owner: usize, part: Option<super::ae2_parts::PartAddress>,
        wireless: Option<&super::ae2_wireless::WirelessContext>, slot: i16, button: i8,
    ) {
        use horizon_client::{ae2_network_tool::Inventory, menu::{Kind, Menu}};
        let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn) else { return; };
        let button = i32::from(button);
        let mode = button >> 2;
        let stage = button & 3;
        let old = host.play.get_mut(&conn).and_then(|p| p.ae2_terminal.as_mut())
            .and_then(|t| t.toolbox_drag.take());
        let matrix_owner = craft_owner(part, wireless);
        let machine = matrix_owner.as_ref().and_then(|o| self.lan_craft_matrix_read(host, conn, o).ok());
        let identity = part.and_then(|p| self.ae2_terminal_part_identity(p));
        if stage == 0 {
            let g = &host.hub.guests[gi];
            let tool = toolbox_stack(g, owner).cloned();
            let matrix = if matrix_owner.is_some() {
                machine.as_ref().filter(|m| m.kind == horizon_client::ae2_world::MachineKind::CraftingTerminal && m.slots.len() == 9).map(|m| m.slots.clone())
            } else { Some(vec![None; 9]) };
            let next = match (old, tool, g.carried.clone(), matrix) {
                (None, Some(tool), Some(carried), Some(matrix))
                    if slot == -999 && (0..=2).contains(&mode) && (mode != 2 || g.creative())
                        && (part.is_none() || identity.is_some()) && Inventory::read(&tool).is_ok() =>
                    Some(ToolboxDrag { owner, tool, identity, mode, creative: g.creative(), carried,
                        wireless: wireless.map(|ctx| (ctx.source, ctx.source_owner.clone())),
                        inventory: g.inventory.clone(), armor: g.armor.clone(), offhand: g.offhand.clone(),
                        matrix, slots: Vec::new() }),
                _ => None,
            };
            let started = next.is_some();
            if let Some(t) = host.play.get_mut(&conn).and_then(|p| p.ae2_terminal.as_mut()) { t.toolbox_drag = next; }
            if !started { self.lan_send_ae2_terminal(host, conn); }
            return;
        }
        let Some(mut drag) = old else { self.lan_send_ae2_terminal(host, conn); return; };
        let g = &host.hub.guests[gi];
        let valid = drag.owner == owner && drag.mode == mode && drag.creative == g.creative()
            && g.carried.as_ref().is_some_and(|s| s == &drag.carried && Arc::ptr_eq(&s.item, &drag.carried.item))
            && match (wireless, &drag.wireless) {
                (None, None) => g.inventory == drag.inventory && g.offhand == drag.offhand,
                (Some(ctx), Some((source, identity))) => *source == ctx.source && identity.same_object(&ctx.source_owner)
                    && self.ae2_wireless_source_valid(host, conn, ctx)
                    && wireless_unchanged_inventory(ctx.source, &drag.inventory, &g.inventory, &drag.offhand, &g.offhand),
                _ => false,
            }
            && g.armor == drag.armor
            && toolbox_stack(g, owner).is_some_and(|s| s == &drag.tool && Arc::ptr_eq(&s.item, &drag.tool.item))
            && part.is_none_or(|_| identity == drag.identity && identity.is_some())
            && (matrix_owner.is_none() || machine.as_ref().is_some_and(|m| m.slots == drag.matrix));
        if !valid { self.lan_send_ae2_terminal(host, conn); return; }
        if stage == 1 {
            let index = toolbox_drag_slot(slot, matrix_owner.is_some(), owner, wireless.map(|ctx| ctx.source));
            if let Some(index) = index {
                if !drag.slots.contains(&index) { drag.slots.push(index); }
            } else if !(0..=53).contains(&slot) && !(54..=89).contains(&slot) {
                self.lan_send_ae2_terminal(host, conn); return;
            }
            if let Some(t) = host.play.get_mut(&conn).and_then(|p| p.ae2_terminal.as_mut()) { t.toolbox_drag = Some(drag); }
            return;
        }
        if stage != 2 || slot != -999 { self.lan_send_ae2_terminal(host, conn); return; }
        let Ok(pocket) = Inventory::read(&drag.tool) else { self.lan_send_ae2_terminal(host, conn); return; };
        if !toolbox_staging_safe(&pocket, |s| self.res.items().stack_max(&s))
            || drag.matrix.iter().flatten().any(|s|
                s.count == 0 || s.count > self.res.items().stack_max(&s).min(99))
        { self.lan_send_ae2_terminal(host, conn); return; }
        let before = self.ae2_snapshot_blocks();
        let random = self.world.random.clone();
        let inventory = g.inventory.clone(); let armor = g.armor.clone();
        let offhand = g.offhand.clone(); let carried = g.carried.clone();
        let mut menu = Menu::new(Kind::Chest(2));
        menu.network_tool_slot = Some(owner);
        menu.network_tool_main_limit = Some(9);
        let (result, effects) = self.guest_menu_op_deferred(host, gi, &mut menu, |menu, env| {
            for (i, stack) in pocket.slots.iter().chain(drag.matrix.iter()).cloned().enumerate() { menu.set_slot(env, i, stack); }
            let mask = drag.mode << 2;
            menu.click(env, -999, mask, Input::QuickCraft)?;
            for &i in &drag.slots { menu.click(env, i, mask | 1, Input::QuickCraft)?; }
            menu.click(env, -999, mask | 2, Input::QuickCraft)?;
            Ok::<_, &'static str>((0..18).map(|i| menu.slot(i).cloned()).collect::<Vec<_>>())
        });
        let result = result.map_err(str::to_owned).and_then(|next| {
            if toolbox_stack(&host.hub.guests[gi], owner) != Some(&drag.tool) { return Err("network tool moved".into()); }
            if wireless.is_some_and(|ctx| !self.ae2_wireless_source_valid(host, conn, ctx)) { return Err("wireless terminal moved".into()); }
            let mut tool = drag.tool.clone();
            Inventory { slots: std::array::from_fn(|i| next[i].clone()) }.store(&mut tool)?;
            if let (Some(matrix_owner), Some(mut machine)) = (matrix_owner.as_ref(), machine) {
                if machine.slots != next[9..18] {
                    machine.slots = next[9..18].to_vec();
                    self.lan_craft_matrix_store(host, conn, matrix_owner, &machine)?;
                }
            }
            *toolbox_stack_mut(&mut host.hub.guests[gi], owner).ok_or("network tool moved")? = tool;
            Ok(())
        });
        if let Err(message) = result {
            self.ae2_restore_blocks(before);
            let g = &mut host.hub.guests[gi];
            g.inventory = inventory; g.armor = armor; g.offhand = offhand;
            g.carried = carried; self.world.random = random;
            for stack in pocket.slots.iter().chain(drag.matrix.iter()).chain(g.inventory.iter())
                .chain(g.armor.iter()).chain(std::iter::once(&g.offhand)).chain(std::iter::once(&g.carried)).flatten()
            { horizon_client::ae2_stack_identity::synchronize_tracked(stack); }
            self.as_guest(host, conn, |g| g.system_message(Text::literal(format!("Terminal: {message}"))));
        } else { self.apply_guest_menu_effects(host, gi, effects); }
        if wireless.is_some() { host.hub.send_inventory(gi); }
        if let Some(part) = part {
            let viewers: Vec<_> = host.play.iter().filter_map(|(&id, p)|
                p.ae2_terminal.as_ref().filter(|t| t.craft_part == Some(part)).map(|_| id)).collect();
            for viewer in viewers { self.lan_send_ae2_terminal(host, viewer); }
        } else { self.lan_send_ae2_terminal(host, conn); }
    }

    /// Replay wireless player swaps only after excluding the held source.
    fn lan_wireless_player_swap(
        &mut self, host: &mut super::lan::LanHost, conn: u32,
        ctx: &super::ae2_wireless::WirelessContext, slot: i16, button: i8,
    ) {
        use horizon_client::menu::{Kind, Menu};
        let target = match slot {
            54..=80 => Some((slot - 54 + 9) as usize),
            81..=89 => Some((slot - 81) as usize),
            _ => None,
        };
        let source = match button { 0..=8 => Some(button as usize), 40 => Some(40), _ => None };
        let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn) else { return; };
        if let (Some(target), Some(source)) = (target, source) {
            let g = &host.hub.guests[gi];
            let safe = target != ctx.source && source != ctx.source
                && [toolbox_stack(g, target), toolbox_stack(g, source)].into_iter().flatten()
                    .all(|s| s.count > 0 && s.count <= self.res.items().stack_max(&s));
            if safe {
                let inventory = g.inventory.clone(); let armor = g.armor.clone();
                let offhand = g.offhand.clone(); let carried = g.carried.clone();
                let random = self.world.random.clone();
                let mut menu = Menu::new(Kind::Chest(6));
                let (result, effects) = self.guest_menu_op_deferred(host, gi, &mut menu, |menu, env|
                    menu.click(env, i32::from(slot), i32::from(button), Input::Swap));
                if result.is_ok() && self.ae2_wireless_source_valid(host, conn, ctx) {
                    self.apply_guest_menu_effects(host, gi, effects);
                } else {
                    let g = &mut host.hub.guests[gi];
                    g.inventory = inventory; g.armor = armor; g.offhand = offhand; g.carried = carried;
                    self.world.random = random;
                    for stack in g.inventory.iter().chain(g.armor.iter()).chain(std::iter::once(&g.offhand))
                        .chain(std::iter::once(&g.carried)).flatten() {
                        horizon_client::ae2_stack_identity::synchronize_tracked(stack);
                    }
                }
            }
        }
        host.hub.send_inventory(gi);
        self.lan_send_ae2_terminal(host, conn);
    }

    /// Accumulate an actual gesture, then commit player and optional item grid
    /// together. A changing battery does not invalidate an otherwise stable
    /// owner; the current battery is retained when the grid is written.
    fn lan_wireless_drag(
        &mut self, host: &mut super::lan::LanHost, conn: u32,
        ctx: &super::ae2_wireless::WirelessContext, slot: i16, button: i8,
    ) {
        use horizon_client::menu::{Kind, Menu};
        let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn) else { return; };
        let mode = i32::from(button) >> 2; let stage = i32::from(button) & 3;
        let old = host.play.get_mut(&conn).and_then(|p| p.ae2_terminal.as_mut())
            .and_then(|t| t.wireless_drag.take());
        let owner = craft_owner(None, Some(ctx));
        let machine = owner.as_ref().and_then(|owner| self.lan_craft_matrix_read(host, conn, owner).ok());
        let matrix = machine.as_ref().map(|m| m.slots.clone());
        if stage == 0 {
            let g = &host.hub.guests[gi];
            let next = match (old, g.carried.clone()) {
                (None, Some(carried)) if slot == -999 && (0..=2).contains(&mode)
                    && (mode != 2 || g.creative()) && (owner.is_none() || matrix.as_ref().is_some_and(|v| v.len() == 9)) =>
                    Some(WirelessDrag { source: ctx.source, identity: ctx.source_owner.clone(), mode,
                        creative: g.creative(), carried, inventory: g.inventory.clone(), armor: g.armor.clone(),
                        offhand: g.offhand.clone(), matrix, slots: Vec::new() }),
                _ => None,
            };
            let started = next.is_some();
            if let Some(t) = host.play.get_mut(&conn).and_then(|p| p.ae2_terminal.as_mut()) { t.wireless_drag = next; }
            if !started { self.lan_send_ae2_terminal(host, conn); }
            return;
        }
        let Some(mut drag) = old else { self.lan_send_ae2_terminal(host, conn); return; };
        let g = &host.hub.guests[gi];
        let valid = drag.source == ctx.source && drag.identity.same_object(&ctx.source_owner)
            && self.ae2_wireless_source_valid(host, conn, ctx)
            && drag.mode == mode && drag.creative == g.creative()
            && g.carried.as_ref().is_some_and(|s| s == &drag.carried && Arc::ptr_eq(&s.item, &drag.carried.item))
            && wireless_unchanged_inventory(ctx.source, &drag.inventory, &g.inventory, &drag.offhand, &g.offhand)
            && g.armor == drag.armor && matrix == drag.matrix;
        if !valid { self.lan_send_ae2_terminal(host, conn); return; }
        if stage == 1 {
            let index = if owner.is_some() { craft_drag_menu_slot(slot) } else {
                match slot { 54..=89 => Some(i32::from(slot)), _ => None }
            };
            if let Some(index) = index { if !drag.slots.contains(&index) { drag.slots.push(index); } }
            else if !(0..=53).contains(&slot) { self.lan_send_ae2_terminal(host, conn); return; }
            if let Some(t) = host.play.get_mut(&conn).and_then(|p| p.ae2_terminal.as_mut()) { t.wireless_drag = Some(drag); }
            return;
        }
        if stage != 2 || slot != -999 || drag.matrix.iter().flatten().flatten().any(|s|
            s.count == 0 || s.count > self.res.items().stack_max(&s).min(99))
        { self.lan_send_ae2_terminal(host, conn); return; }
        let before = self.ae2_snapshot_blocks(); let random = self.world.random.clone();
        let inventory = g.inventory.clone(); let armor = g.armor.clone();
        let offhand = g.offhand.clone(); let carried = g.carried.clone();
        let mut menu = Menu::new(if owner.is_some() { Kind::Crafting } else { Kind::Chest(6) });
        menu.network_tool_slot = Some(ctx.source); menu.network_tool_main_limit = Some(0);
        let (result, effects) = self.guest_menu_op_deferred(host, gi, &mut menu, |menu, env| {
            if let Some(matrix) = &drag.matrix {
                for (i, stack) in matrix.iter().cloned().enumerate() { menu.set_slot(env, 1 + i, stack); }
            }
            let mask = drag.mode << 2;
            menu.click(env, -999, mask, Input::QuickCraft)?;
            for &index in &drag.slots { menu.click(env, index, mask | 1, Input::QuickCraft)?; }
            menu.click(env, -999, mask | 2, Input::QuickCraft)?;
            Ok::<_, &'static str>(drag.matrix.as_ref().map(|_| (1..10).map(|i| menu.slot(i).cloned()).collect::<Vec<_>>()))
        });
        let result = result.map_err(str::to_owned).and_then(|next| {
            if !self.ae2_wireless_source_valid(host, conn, ctx) { return Err("wireless terminal moved".into()); }
            if let (Some(owner), Some(mut machine), Some(next)) = (owner.as_ref(), machine, next) {
                machine.slots = next;
                self.lan_craft_matrix_store(host, conn, owner, &machine)?;
            }
            Ok(())
        });
        if result.is_err() {
            self.ae2_restore_blocks(before); self.world.random = random;
            let g = &mut host.hub.guests[gi];
            g.inventory = inventory; g.armor = armor; g.offhand = offhand; g.carried = carried;
            for stack in g.inventory.iter().chain(g.armor.iter()).chain(std::iter::once(&g.offhand))
                .chain(std::iter::once(&g.carried)).flatten() {
                horizon_client::ae2_stack_identity::synchronize_tracked(stack);
            }
        } else { self.apply_guest_menu_effects(host, gi, effects); }
        host.hub.send_inventory(gi);
        self.lan_send_ae2_terminal(host, conn);
    }

    fn lan_wireless_pickup_all(
        &mut self, host: &mut super::lan::LanHost, conn: u32,
        ctx: &super::ae2_wireless::WirelessContext, slot: i16, button: i8,
    ) -> Result<(), String> {
        use horizon_client::menu::{Kind, Menu};
        if !(0..=1).contains(&button) { return Ok(()); }
        let owner = craft_owner(None, Some(ctx));
        let target = match slot {
            27..=35 if owner.is_some() => (slot - 27) as usize + 36,
            54..=80 => (slot - 54 + 9) as usize,
            81..=89 => (slot - 81) as usize,
            _ => return Ok(()),
        };
        let gi = host.hub.guests.iter().position(|g| g.conn == conn).ok_or("missing guest")?;
        let g = &host.hub.guests[gi];
        let Some(carried) = g.carried.clone() else { return Ok(()); };
        let mut machine = owner.as_ref().map(|owner| self.lan_craft_matrix_read(host, conn, owner)).transpose()?;
        if g.inventory.len() != 36 || machine.as_ref().is_some_and(|m| m.slots.len() != 9) {
            return Err("invalid wireless crafting inventory".into());
        }
        let original: Vec<_> = g.inventory.iter().cloned().chain(machine.iter().flat_map(|m| m.slots.iter().cloned())).collect();
        if original.get(target).is_none_or(|stack| stack.is_some() && target != ctx.source) { return Ok(()); }
        let max = self.res.items().stack_max(&carried);
        if carried.count >= max { return Ok(()); }
        if !original.iter().enumerate().any(|(i, s)| i != ctx.source && s.as_ref().is_some_and(|s|
            s.same_item_same_components(&carried))) { return Ok(()); }
        if original.iter().enumerate().any(|(i, s)| i != ctx.source && s.as_ref().is_some_and(|s|
            s.same_item_same_components(&carried) && (s.count == 0 || s.count > max))) {
            return Err("oversized pickup-all source".into());
        }
        let (result, next, cursor) = {
            let env = self.menu_env();
            let mut menu = Menu::new(Kind::Chest(if owner.is_some() { 5 } else { 4 }));
            for (i, stack) in original.iter().enumerate() {
                if i != ctx.source && stack.as_ref().is_some_and(|s| s.same_item_same_components(&carried)) {
                    menu.set_slot(&env, i, stack.clone());
                }
            }
            menu.carried = Some(carried.clone());
            let result = menu.click(&env, target as i32, i32::from(button), Input::PickupAll);
            let next = original.iter().enumerate().map(|(i, stack)| {
                if i != ctx.source && stack.as_ref().is_some_and(|s| s.same_item_same_components(&carried)) {
                    menu.slot(i).cloned()
                } else { stack.clone() }
            }).collect::<Vec<_>>();
            (result, next, menu.carried)
        };
        if result.is_ok() && next == original && cursor.as_ref() == Some(&carried) { return Ok(()); }
        let commit = result.map_err(str::to_owned).and_then(|()| {
            if !self.ae2_wireless_source_valid(host, conn, ctx) { return Err("wireless terminal moved".into()); }
            if let (Some(owner), Some(machine)) = (owner.as_ref(), machine.as_mut()) {
                machine.slots = next[36..45].to_vec();
                self.lan_craft_matrix_store(host, conn, owner, machine)?;
            }
            Ok(())
        });
        if let Err(message) = commit {
            for stack in original.iter().flatten().chain(std::iter::once(&carried)) {
                horizon_client::ae2_stack_identity::synchronize_tracked(stack);
            }
            return Err(message);
        }
        // A grid update changed the item source in place. Preserve that current
        // value rather than overwriting it with the pre-gesture snapshot.
        let mut inventory = next[..36].to_vec();
        if ctx.source < 36 { inventory[ctx.source] = host.hub.guests[gi].inventory[ctx.source].clone(); }
        host.hub.guests[gi].inventory = inventory;
        host.hub.guests[gi].carried = cursor;
        Ok(())
    }

    /// Collect the real slots of one quick-craft gesture, then replay it on a
    /// native menu only if the player and physical face are still unchanged.
    fn lan_crafting_terminal_drag(
        &mut self,
        host: &mut super::lan::LanHost,
        conn: u32,
        part: super::ae2_parts::PartAddress,
        slot: i16,
        button: i8,
    ) {
        use horizon_client::menu::{Kind, Menu};
        let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn) else { return; };
        let button = i32::from(button);
        let mode = button >> 2;
        let stage = button & 3;
        let old = host.play.get_mut(&conn).and_then(|p| p.ae2_terminal.as_mut())
            .and_then(|t| t.craft_drag.take());
        let machine = self.ae2_part_machine(part).ok().flatten();
        if stage == 0 {
            let guest = &host.hub.guests[gi];
            let identity = host.play.get(&conn).and_then(|p| p.ae2_terminal.as_ref())
                .filter(|t| t.view_part == Some(part)).and_then(|t| t.view_identity);
            let next = match (old, machine, guest.carried.clone(), identity) {
                (None, Some(machine), Some(carried), Some(identity))
                    if slot == -999 && (0..=2).contains(&mode)
                        && (mode != 2 || guest.creative()) && machine.slots.len() == 9 =>
                {
                    Some(CraftMatrixDrag {
                        identity, mode, creative: guest.creative(), carried,
                        inventory: guest.inventory.clone(), matrix: machine.slots, slots: Vec::new(),
                    })
                }
                _ => None,
            };
            let started = next.is_some();
            if let Some(t) = host.play.get_mut(&conn).and_then(|p| p.ae2_terminal.as_mut()) {
                t.craft_drag = next;
            }
            if !started { self.lan_send_ae2_terminal(host, conn); }
            return;
        }

        let Some(mut drag) = old else {
            self.lan_send_ae2_terminal(host, conn);
            return;
        };
        let guest = &host.hub.guests[gi];
        let valid = drag.mode == mode && guest.creative() == drag.creative
            && guest.carried.as_ref().is_some_and(|s| s == &drag.carried
                && std::sync::Arc::ptr_eq(&s.item, &drag.carried.item))
            && guest.inventory == drag.inventory
            && machine.as_ref().is_some_and(|m| m.slots == drag.matrix)
            && self.ae2_terminal_part_identity(part) == Some(drag.identity);
        if !valid {
            self.lan_send_ae2_terminal(host, conn);
            return;
        }
        if stage == 1 {
            if let Some(index) = craft_drag_menu_slot(slot) {
                if !drag.slots.contains(&index) { drag.slots.push(index); }
            } else if !(0..=53).contains(&slot) {
                self.lan_send_ae2_terminal(host, conn);
                return;
            }
            if let Some(t) = host.play.get_mut(&conn).and_then(|p| p.ae2_terminal.as_mut()) {
                t.craft_drag = Some(drag);
            }
            return;
        }
        if stage != 2 || slot != -999 {
            self.lan_send_ae2_terminal(host, conn);
            return;
        }

        let mut machine = machine.expect("validated machine");
        let before = self.ae2_snapshot_blocks();
        let inventory = guest.inventory.clone();
        let armor = guest.armor.clone();
        let offhand = guest.offhand.clone();
        let carried = guest.carried.clone();
        let random = self.world.random.clone();
        let mut menu = Menu::new(Kind::Crafting);
        let (next, effects) = self.guest_menu_op_deferred(host, gi, &mut menu, |menu, env| {
            for (i, stack) in drag.matrix.iter().cloned().enumerate() {
                menu.set_slot(env, 1 + i, stack);
            }
            let mask = drag.mode << 2;
            menu.click(env, -999, mask, Input::QuickCraft)?;
            for &index in &drag.slots {
                menu.click(env, index, mask | 1, Input::QuickCraft)?;
            }
            menu.click(env, -999, mask | 2, Input::QuickCraft)?;
            Ok::<_, &'static str>((1..10).map(|i| menu.slot(i).cloned()).collect::<Vec<_>>())
        });
        let result = next.map_err(str::to_owned).and_then(|next| {
            machine.slots = next;
            self.ae2_store_part_machine(part, &machine)
        });
        if let Err(message) = result {
            self.ae2_restore_blocks(before);
            host.hub.guests[gi].inventory = inventory;
            host.hub.guests[gi].armor = armor;
            host.hub.guests[gi].offhand = offhand;
            host.hub.guests[gi].carried = carried;
            self.world.random = random;
            let guest = &host.hub.guests[gi];
            for stack in drag.matrix.iter().chain(guest.inventory.iter()).chain(guest.armor.iter())
                .chain(std::iter::once(&guest.offhand)).chain(std::iter::once(&guest.carried)).flatten()
            {
                horizon_client::ae2_stack_identity::synchronize_tracked(stack);
            }
            self.as_guest(host, conn, |g| {
                g.system_message(Text::literal(format!("Crafting terminal: {message}")))
            });
        } else {
            self.apply_guest_menu_effects(host, gi, effects);
        }
        let viewers: Vec<_> = host.play.iter().filter_map(|(&id, p)| {
            p.ae2_terminal.as_ref().filter(|t| t.craft_part == Some(part)).map(|_| id)
        }).collect();
        for viewer in viewers { self.lan_send_ae2_terminal(host, viewer); }
    }

    /// The nine toolbox slots are owned by the located player stack. Stage
    /// the native menu operation before writing that stack and its effects.
    fn lan_terminal_toolbox_click(
        &mut self, host: &mut super::lan::LanHost, conn: u32, pos: (i32, i32, i32),
        owner: usize, slot: usize, button: i8, input: Input,
    ) -> Result<(), String> {
        use horizon_client::{ae2_network_tool::Inventory, menu::{Kind, Menu}};
        let gi = host.hub.guests.iter().position(|g| g.conn == conn).ok_or("missing guest")?;
        let tool = toolbox_stack(&host.hub.guests[gi], owner).cloned().ok_or("network tool moved")?;
        let mut pocket = Inventory::read(&tool)?;
        if !toolbox_staging_safe(&pocket, |s| self.res.items().stack_max(&s)) {
            return Err("oversized toolbox source".into());
        }
        if slot >= pocket.slots.len() { return Ok(()); }
        if input == Input::QuickMove {
            if !(0..=1).contains(&button) || !self.ae2_terminal_powered(pos) { return Ok(()); }
            let Some(stack) = pocket.slots[slot].clone() else { return Ok(()); };
            let before = self.ae2_snapshot_blocks();
            let moved = match self.ae2_terminal_insert(pos, &stack, u64::from(stack.count)) {
                Ok(moved) if moved <= u64::from(stack.count) => moved as u32,
                Ok(_) => { self.ae2_restore_blocks(before); return Err("invalid grid insert count".into()); }
                Err(e) => { self.ae2_restore_blocks(before); return Err(e); }
            };
            if moved == 0 { return Ok(()); }
            let mut remaining = stack;
            remaining.count -= moved;
            pocket.slots[slot] = (remaining.count > 0).then_some(remaining);
            let mut updated = tool.clone();
            if let Err(e) = pocket.store(&mut updated) {
                self.ae2_restore_blocks(before);
                return Err(e);
            }
            *toolbox_stack_mut(&mut host.hub.guests[gi], owner).ok_or("network tool moved")? = updated;
            return Ok(());
        }
        let allowed = match input {
            Input::Pickup | Input::Throw => (0..=1).contains(&button),
            Input::Swap => (0..=8).contains(&button) || button == 40,
            Input::Clone => button == 2,
            _ => false,
        };
        if !allowed { return Ok(()); }
        if input == Input::Swap {
            let g = &host.hub.guests[gi];
            let source = if button == 40 { g.offhand.as_ref() } else { g.inventory[button as usize].as_ref() };
            if source.is_some_and(|s| s.count > self.res.items().stack_max(&s)) {
                return Ok(());
            }
        }
        let inventory = host.hub.guests[gi].inventory.clone();
        let armor = host.hub.guests[gi].armor.clone();
        let offhand = host.hub.guests[gi].offhand.clone();
        let carried = host.hub.guests[gi].carried.clone();
        let random = self.world.random.clone();
        let mut menu = Menu::new(Kind::Chest(1));
        menu.network_tool_slot = Some(owner);
        let (result, effects) = self.guest_menu_op_deferred(host, gi, &mut menu, |menu, env| {
            for (i, stack) in pocket.slots.iter().cloned().enumerate() { menu.set_slot(env, i, stack); }
            menu.click(env, slot as i32, i32::from(button), input)?;
            Ok::<_, &'static str>(std::array::from_fn(|i| menu.slot(i).cloned()))
        });
        let result = result.map_err(str::to_owned).and_then(|slots| {
            if toolbox_stack(&host.hub.guests[gi], owner) != Some(&tool) {
                return Err("network tool moved".into());
            }
            let mut updated = tool.clone();
            Inventory { slots }.store(&mut updated)?;
            *toolbox_stack_mut(&mut host.hub.guests[gi], owner).ok_or("network tool moved")? = updated;
            Ok(())
        });
        if let Err(e) = result {
            let guest = &mut host.hub.guests[gi];
            guest.inventory = inventory; guest.armor = armor; guest.offhand = offhand; guest.carried = carried;
            self.world.random = random;
            for stack in pocket.slots.iter().chain(guest.inventory.iter()).chain(guest.armor.iter())
                .chain(std::iter::once(&guest.offhand)).chain(std::iter::once(&guest.carried)).flatten()
            { horizon_client::ae2_stack_identity::synchronize_tracked(stack); }
            return Err(e);
        }
        self.apply_guest_menu_effects(host, gi, effects);
        if input == Input::Swap && button == 40 { host.hub.send_inventory(gi); }
        Ok(())
    }

    /// Double-click in the source menu visits toolbox, then player inventory
    /// (hotbar before main), then the crafting matrix.
    fn lan_terminal_toolbox_pickup_all(
        &mut self, host: &mut super::lan::LanHost, conn: u32,
        owner: usize, part: Option<super::ae2_parts::PartAddress>,
        wireless: Option<&super::ae2_wireless::WirelessContext>, slot: i16, button: i8,
    ) -> Result<(), String> {
        use horizon_client::{ae2_network_tool::Inventory, menu::{Kind, Menu}};
        if !(0..=1).contains(&button) { return Ok(()); }
        let matrix_owner = craft_owner(part, wireless);
        let target = match slot {
            95..=103 => (slot - 95) as usize,
            54..=80 => 18 + (slot - 54) as usize,
            81..=89 => 9 + (slot - 81) as usize,
            27..=35 if matrix_owner.is_some() => 45 + (slot - 27) as usize,
            _ => return Ok(()),
        };
        let gi = host.hub.guests.iter().position(|g| g.conn == conn).ok_or("missing guest")?;
        let g = &host.hub.guests[gi];
        let tool = toolbox_stack(g, owner).cloned().ok_or("network tool moved")?;
        let pocket = Inventory::read(&tool)?;
        if g.inventory.len() != 36 || !toolbox_staging_safe(&pocket, |s| self.res.items().stack_max(&s)) {
            return Err("invalid toolbox inventory".into());
        }
        if wireless.is_some_and(|ctx| !self.ae2_wireless_source_valid(host, conn, ctx)) { return Err("wireless terminal moved".into()); }
        let Some(carried) = g.carried.clone() else { return Ok(()); };
        let mut machine = matrix_owner.as_ref().map(|o| self.lan_craft_matrix_read(host, conn, o)).transpose()?;
        if machine.as_ref().is_some_and(|m| m.kind != horizon_client::ae2_world::MachineKind::CraftingTerminal || m.slots.len() != 9) {
            return Err("invalid crafting matrix".into());
        }
        let mut original: Vec<_> = pocket.slots.iter().chain(g.inventory.iter()).cloned().collect();
        if let Some(machine) = machine.as_ref() { original.extend(machine.slots.iter().cloned()); }
        let locked = |index: usize| toolbox_pickup_locked(index, owner, wireless.map(|ctx| ctx.source));
        // Both DisabledSlot owners are valid double-click targets but cannot be sources.
        if original.get(target).is_none_or(|v| v.is_some() && !locked(target)) { return Ok(()); }
        let max = self.res.items().stack_max(&carried);
        if carried.count >= max { return Ok(()); }
        if original.iter().enumerate().any(|(i,s)| !locked(i) && s.as_ref().is_some_and(|s|
            s.same_item_same_components(&carried) && (s.count == 0 || s.count > max))) {
            return Err("oversized pickup-all source".into());
        }
        let inventory_before = g.inventory.clone(); let armor_before = g.armor.clone();
        let offhand_before = g.offhand.clone(); let carried_before = g.carried.clone();
        let before = self.ae2_snapshot_blocks();
        let (result, next, cursor) = {
            let env = self.menu_env();
            let mut menu = Menu::new(Kind::Chest(if matrix_owner.is_some() {6} else {5}));
            for (i, stack) in original.iter().enumerate() {
                if !locked(i) && stack.as_ref().is_some_and(|s| s.same_item_same_components(&carried)) {
                    menu.set_slot(&env, i, stack.clone());
                }
            }
            menu.carried = Some(carried.clone());
            let result = menu.click(&env, target as i32, i32::from(button), Input::PickupAll);
            let next = original.iter().enumerate().map(|(i, stack)| {
                if !locked(i) && stack.as_ref().is_some_and(|s| s.same_item_same_components(&carried)) {
                    menu.slot(i).cloned()
                } else { stack.clone() }
            }).collect::<Vec<_>>();
            (result, next, menu.carried)
        };
        if result.is_ok() && next == original && cursor.as_ref() == Some(&carried) { return Ok(()); }
        let result = result.map_err(str::to_owned).and_then(|()| {
            if toolbox_stack(&host.hub.guests[gi], owner) != Some(&tool) { return Err("network tool moved".into()); }
            if wireless.is_some_and(|ctx| !self.ae2_wireless_source_valid(host, conn, ctx)) { return Err("wireless terminal moved".into()); }
            let mut updated_tool = tool.clone();
            Inventory { slots: std::array::from_fn(|i| next[i].clone()) }.store(&mut updated_tool)?;
            if let (Some(matrix_owner), Some(machine)) = (matrix_owner.as_ref(), machine.as_mut()) {
                if machine.slots != next[45..54] {
                    machine.slots = next[45..54].to_vec();
                    self.lan_craft_matrix_store(host, conn, matrix_owner, machine)?;
                }
            }
            let mut inventory = next[9..45].to_vec();
            if let Some(ctx) = wireless.filter(|ctx| ctx.source < 36) {
                inventory[ctx.source] = host.hub.guests[gi].inventory[ctx.source].clone();
            }
            if owner < 36 { inventory[owner] = Some(updated_tool.clone()); }
            host.hub.guests[gi].inventory = inventory;
            if owner >= 36 { *toolbox_stack_mut(&mut host.hub.guests[gi], owner).ok_or("network tool moved")? = updated_tool; }
            host.hub.guests[gi].carried = cursor;
            Ok(())
        });
        if let Err(message) = result {
            self.ae2_restore_blocks(before);
            let g = &mut host.hub.guests[gi];
            g.inventory = inventory_before; g.armor = armor_before; g.offhand = offhand_before; g.carried = carried_before;
            for stack in original.iter().chain(g.inventory.iter()).chain(g.armor.iter())
                .chain(std::iter::once(&g.offhand)).chain(std::iter::once(&g.carried)).flatten() {
                horizon_client::ae2_stack_identity::synchronize_tracked(stack);
            }
            return Err(message);
        }
        Ok(())
    }

    fn lan_crafting_terminal_pickup_all(
        &mut self,
        host: &mut super::lan::LanHost,
        conn: u32,
        part: super::ae2_parts::PartAddress,
        slot: i16,
        button: i8,
    ) -> Result<(), String> {
        use horizon_client::menu::{Kind, Menu};
        if !(0..=1).contains(&button) { return Ok(()); }
        let target = match slot {
            27..=35 => usize::from((slot - 27) as u16) + 36,
            54..=80 => usize::from((slot - 54) as u16) + 9,
            81..=89 => usize::from((slot - 81) as u16),
            _ => return Ok(()),
        };
        let gi = host.hub.guests.iter().position(|g| g.conn == conn).ok_or("missing guest")?;
        let guest = &host.hub.guests[gi];
        let Some(carried) = guest.carried.clone() else { return Ok(()); };
        let mut machine = self.ae2_part_machine(part)?.ok_or("missing crafting terminal")?;
        if machine.kind != horizon_client::ae2_world::MachineKind::CraftingTerminal
            || machine.slots.len() != 9 || guest.inventory.len() != 36
        {
            return Err("invalid crafting inventory".into());
        }
        let original: Vec<_> = guest.inventory.iter().chain(machine.slots.iter()).cloned().collect();
        if original[target].is_some() { return Ok(()); }
        let max = self.res.items().stack_max(&carried);
        if carried.count >= max { return Ok(()); }
        if !original.iter().flatten().any(|s| s.same_item_same_components(&carried)) {
            return Ok(());
        }
        // Menu's chest setter caps a stack. Never truncate a malformed source
        // while staging it; unrelated stacks are not copied into the menu.
        if original.iter().flatten().any(|s| s.same_item_same_components(&carried)
            && (s.count == 0 || s.count > max))
        {
            return Err("oversized pickup-all source".into());
        }

        let before = self.ae2_snapshot_blocks();
        let (result, next, cursor) = {
            let env = self.menu_env();
            let mut menu = Menu::new(Kind::Chest(5));
            for (i, stack) in original.iter().enumerate() {
                if stack.as_ref().is_some_and(|s| s.same_item_same_components(&carried)) {
                    menu.set_slot(&env, i, stack.clone());
                }
            }
            menu.carried = Some(carried.clone());
            let result = menu.click(&env, target as i32, i32::from(button), Input::PickupAll);
            let next = original.iter().enumerate().map(|(i, stack)| {
                if stack.as_ref().is_some_and(|s| s.same_item_same_components(&carried)) {
                    menu.slot(i).cloned()
                } else {
                    stack.clone()
                }
            }).collect::<Vec<_>>();
            (result, next, menu.carried)
        };
        if result.is_ok() && next == original && cursor.as_ref() == Some(&carried) {
            return Ok(());
        }
        let result = result.map_err(str::to_owned).and_then(|()| {
            machine.slots = next[36..45].to_vec();
            self.ae2_store_part_machine(part, &machine)
        });
        if let Err(message) = result {
            self.ae2_restore_blocks(before);
            for stack in original.iter().flatten().chain(std::iter::once(&carried)) {
                horizon_client::ae2_stack_identity::synchronize_tracked(stack);
            }
            return Err(message);
        }
        host.hub.guests[gi].inventory = next[..36].to_vec();
        host.hub.guests[gi].carried = cursor;
        Ok(())
    }

    fn lan_crafting_terminal_click(
        &mut self,
        host: &mut super::lan::LanHost,
        conn: u32,
        owner: &CraftOwner,
        pos: (i32, i32, i32),
        slot: i16,
        button: i8,
        input: Input,
    ) -> Result<(), String> {
        use horizon_client::menu::{Kind, Menu};
        let allowed = match input {
            Input::Pickup | Input::QuickMove => (0..=1).contains(&button),
            Input::Swap => (27..=35).contains(&slot) && ((0..=8).contains(&button) || button == 40),
            Input::Clone => (27..=35).contains(&slot) && button == 2,
            Input::Throw => (27..=35).contains(&slot) && (0..=1).contains(&button),
            _ => false,
        };
        if !allowed {
            return Ok(());
        }
        let gi = host
            .hub
            .guests
            .iter()
            .position(|g| g.conn == conn)
            .ok_or("missing guest")?;
        if input == Input::Swap {
            let guest = &host.hub.guests[gi];
            let source = if button == 40 { guest.offhand.as_ref() } else { guest.inventory.get(button as usize).and_then(Option::as_ref) };
            // Menu::swap can drop a displaced stack when splitting an invalid
            // oversized source; defer that case rather than emit a world item
            // before the face-owned transaction has committed.
            if source.is_some_and(|s| s.count > self.res.items().stack_max(&s)) {
                return Ok(());
            }
        }
        let original = self.lan_craft_matrix_read(host, conn, owner)?;
        if original.kind != horizon_client::ae2_world::MachineKind::CraftingTerminal
            || original.slots.len() != 9
        {
            return Err("invalid crafting matrix".into());
        }
        if matches!(owner, CraftOwner::Wireless(_)) && original.slots.iter().flatten().any(|s|
            s.count == 0 || s.count > self.res.items().stack_max(&s).min(99))
        {
            return Err("oversized wireless crafting matrix".into());
        }
        let (address, view_part) = match owner {
            CraftOwner::Part(part) => (part.host, Some(*part)),
            CraftOwner::Wireless(_) => (self.ae2_local_address(pos).ok_or("invalid wireless network")?, None),
        };
        if host.play.get(&conn).and_then(|p| p.ae2_terminal.as_ref()).is_none() {
            return Err("terminal closed".into());
        }
        let before = self.ae2_snapshot_blocks();
        let inventory = host.hub.guests[gi].inventory.clone();
        let armor = host.hub.guests[gi].armor.clone();
        let offhand = host.hub.guests[gi].offhand.clone();
        let carried = host.hub.guests[gi].carried.clone();
        let matrix = original.slots.clone();
        let random = self.world.random.clone();
        let mut effects = super::lan_crafting::GuestMenuEffects::default();
        let result = (|| -> Result<(), String> {
            let mut machine = original;
            let mut formation = super::ae2_buses::FormationPlan::default();
            if slot == 37 || slot == 38 {
                for i in 0..9 {
                    let Some(mut stack) = machine.slots[i].take() else {
                        continue;
                    };
                    if slot == 37 {
                        let moved = self.lan_craft_grid_insert(host, conn, owner, address, &stack, &mut formation)?;
                        stack.count -= moved as u32;
                    } else {
                        let max = self.res.items().stack_max(&stack);
                        let inv = &mut host.hub.guests[gi].inventory;
                        // CraftingTermMenu clears into hotbar 8..0, then the
                        // remaining 27 slots, first merging and then filling.
                        let order = (0..9).rev().chain(9..36);
                        for index in order.clone() {
                            let Some(dest) = inv.get_mut(index).and_then(Option::as_mut) else { continue; };
                            if dest.same_item_same_components(&stack) && dest.count < max {
                                let n = stack.count.min(max - dest.count);
                                dest.count += n;
                                stack.count -= n;
                                if stack.count == 0 {
                                    break;
                                }
                            }
                        }
                        if stack.count > 0 {
                            for index in order {
                                let Some(dest) = inv.get_mut(index) else { continue; };
                                if dest.is_none() {
                                    let n = stack.count.min(max);
                                    *dest = Some(ItemStack {
                                        count: n,
                                        ..stack.clone()
                                    });
                                    stack.count -= n;
                                    if stack.count == 0 {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    if stack.count > 0 {
                        machine.slots[i] = Some(stack);
                    }
                }
                self.lan_craft_matrix_store(host, conn, owner, &machine)?;
                self.ae2_commit_formation(formation)?;
                return Ok(());
            }
            let menu_slot = if slot == 36 {
                0
            } else {
                (slot - 27 + 1) as i32
            };
            let shift_result = slot == 36 && input == Input::QuickMove;
            let initial_result = if shift_result {
                self.res
                    .recipes()
                    .find(&machine.slots, 3)
                    .map(|(_, item)| item)
            } else {
                None
            };
            let max_passes = if let Some(output) = &initial_result {
                let max = self.res.items().stack_max(&output);
                max / output.count.max(1)
            } else if shift_result {
                0
            } else {
                1
            };
            // CraftingTermSlot CRAFT_SHIFT transfers at most one output stack,
            // checking real player capacity before consuming each recipe.
            for _ in 0..max_passes {
                let previous = machine.slots.clone();
                if let Some(expected) = &initial_result {
                    let Some((_, next_result)) = self.res.recipes().find(&previous, 3) else {
                        break;
                    };
                    if next_result.count != expected.count
                        || !next_result.same_item_same_components(expected)
                    {
                        break;
                    }
                    let max = self.res.items().stack_max(&expected);
                    let room = host.hub.guests[gi]
                        .inventory
                        .iter()
                        .map(|slot| match slot {
                            None => max,
                            Some(item) if item.same_item_same_components(expected) => {
                                max.saturating_sub(item.count)
                            }
                            _ => 0,
                        })
                        .fold(0u64, |sum, n| sum.saturating_add(u64::from(n)));
                    if room < u64::from(expected.count) {
                        break;
                    }
                }
                let mut menu = Menu::new(Kind::Crafting);
                if let CraftOwner::Wireless(ctx) = owner {
                    if input != Input::Swap { menu.network_tool_slot = Some(ctx.source); }
                }
                let (next, pass_effects) = self.guest_menu_op_deferred(host, gi, &mut menu, |menu, env| {
                    for (i, stack) in previous.iter().cloned().enumerate() {
                        menu.set_slot(env, 1 + i, stack);
                    }
                    if shift_result {
                        let original_cursor = menu.carried.take();
                        let _ = menu.click(env, menu_slot, 0, Input::Pickup);
                        if let Some(mut output) = menu.carried.take() {
                            menu.give(env, &mut output);
                            if output.count > 0 {
                                menu.dropped.push(output);
                            }
                        }
                        menu.carried = original_cursor;
                    } else {
                        let _ = menu.click(env, menu_slot, i32::from(button), input);
                    }
                    (1..10).map(|i| menu.slot(i).cloned()).collect::<Vec<_>>()
                });
                effects.append(pass_effects);
                let changed = next != previous;
                machine.slots = next;
                self.lan_craft_matrix_store(host, conn, owner, &machine)?;
                if slot != 36 || !changed {
                    break;
                }
                for i in 0..9 {
                    if machine.slots[i].is_none() {
                        if let Some(old) = previous[i].as_ref() {
                            if let (Ok(key), Some((dim, pos))) =
                                (ae2_lab::key(old), self.ae2_address_parts(address))
                            {
                                if dim == self.dim {
                                    // A disconnected or unpowered network leaves the
                                    // physical matrix empty; it must not undo an
                                    // already completed craft or its remainders.
                                    if self.ae2_view_permits(&key,view_part,horizon_client::ae2_cell_codec::ViewCellScope::ItemsOnly)
                                        && let Ok(Some(item)) = self.lan_craft_grid_extract(host, conn, owner, pos, &key, &mut formation) {
                                        machine.slots[i] = Some(item);
                                    }
                                    // CraftingTermSlot permits a different component
                                    // or damage variant of the same item only when
                                    // the recipe still produces the identical result.
                                    let damageable = self
                                        .res
                                        .items()
                                        .get(&old.item)
                                        .is_some_and(|v| v.max_damage > 0);
                                    if machine.slots[i].is_none()
                                        && (old.extra.is_some() || damageable)
                                    {
                                        let expected = self
                                            .res
                                            .recipes()
                                            .find(&previous, 3)
                                            .map(|(_, out)| out);
                                        if let (Some(expected), Ok(entries)) =
                                            (expected, self.ae2_grid_inventory(pos))
                                        {
                                            for (candidate, _) in entries {
                                                if !self.ae2_view_permits(&candidate,view_part,horizon_client::ae2_cell_codec::ViewCellScope::ItemsOnly) || candidate.item_id != key.item_id
                                                    || candidate == key
                                                {
                                                    continue;
                                                }
                                                let Ok(sample) = ae2_lab::prototype(&candidate)
                                                else {
                                                    continue;
                                                };
                                                let mut grid = previous.clone();
                                                grid[i] = Some(sample);
                                                let valid =
                                                    self.res.recipes().find(&grid, 3).is_some_and(
                                                        |(_, out)| {
                                                            out.count == expected.count
                                                                && out.same_item_same_components(
                                                                    &expected,
                                                                )
                                                        },
                                                    );
                                                if valid {
                                                    if let Ok(Some(item)) =
                                                        self.lan_craft_grid_extract(host, conn, owner, pos, &candidate, &mut formation)
                                                    {
                                                        machine.slots[i] = Some(item);
                                                        break;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                self.lan_craft_matrix_store(host, conn, owner, &machine)?;
                if input != Input::QuickMove {
                    break;
                }
            }
            self.ae2_commit_formation(formation)?;
            Ok(())
        })();
        if result.is_err() {
            self.ae2_restore_blocks(before);
            host.hub.guests[gi].inventory = inventory;
            host.hub.guests[gi].armor = armor;
            host.hub.guests[gi].offhand = offhand;
            host.hub.guests[gi].carried = carried;
            self.world.random = random;
            // Menu operations can split tracked live stacks before a storage
            // error. Restore the retained values with the owned slots.
            let guest = &host.hub.guests[gi];
            for stack in matrix.iter().chain(guest.inventory.iter()).chain(guest.armor.iter())
                .chain(std::iter::once(&guest.offhand)).chain(std::iter::once(&guest.carried)).flatten()
            {
                horizon_client::ae2_stack_identity::synchronize_tracked(stack);
            }
        } else {
            self.apply_guest_menu_effects(host, gi, effects);
        }
        result
    }
    pub(super) fn lan_ae2_terminal_click(
        &mut self,
        host: &mut super::lan::LanHost,
        conn: u32,
        p: &horizon_protocol::packets::play::ContainerClick,
    ) -> bool {
        let Some((pos, page, wireless, connected, craft_amount, craft_amount_edit)) = host
            .play
            .get(&conn)
            .and_then(|x| x.ae2_terminal.as_ref())
            .filter(|t| t.window == p.container)
            .map(|t| {
                (
                    t.pos,
                    t.page,
                    t.wireless.clone(),
                    t.wireless_connected,
                    t.craft_amount,
                    t.craft_amount_edit,
                )
            })
        else {
            return false;
        };
        if let Some((identity, shown)) = host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref())
            .and_then(|t| t.chest_identity.map(|identity| (identity, ChestTerminalSettings { sort: t.sort_mode, view: t.filter_mode, descending: t.sort_descending }))) {
            match self.chest_terminal_read(pos) {
                Some((current, settings)) if current == identity && settings == shown => {}
                Some((current, _)) if current == identity => { self.lan_send_ae2_terminal(host, conn); return true; }
                _ => {
                    self.lan_ae2_terminal_closed(host, conn, None);
                    host.hub.send_to(conn, &C::ContainerClose(play::ContainerClose { container: p.container }));
                    return true;
                }
            }
            if matches!(p.slot, 46 | 47 | 51) && super::lan_crafting::input(p.input) == Input::Pickup
                && host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref()).is_none_or(|t|!chest_revision_matches(&t.chest_revisions,p.state,shown)) {
                self.lan_send_ae2_terminal(host, conn);
                return true;
            }
        }
        if host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref()).is_some_and(|t|t.processing_amount_edit.is_some()){return self.lan_processing_amount_click(host,conn,p);}
        if host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref()).is_some_and(|t|t.priority_edit.is_some()){return self.lan_ae2_priority_click(host,conn,p);}
        let craft_part = host
            .play
            .get(&conn)
            .and_then(|p| p.ae2_terminal.as_ref())
            .and_then(|t| t.craft_part);
        let encoding_part=host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref()).and_then(|t|t.encoding_part);
        if let Some(part)=encoding_part {
            let current=self.ae2_part_machine(part).ok().flatten();
            // A broadcast can update the session before an already queued
            // click arrives. Compare the recipe actually displayed at that
            // revision, not merely the latest counter (which also advances on
            // harmless inventory refreshes).
            let stale=host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref()).is_none_or(|t|{
                let fake=t.encoding && matches!(super::lan_crafting::input(p.input),Input::Pickup|Input::QuickMove)
                    && ((0..=8).contains(&p.slot)||(18..=20).contains(&p.slot));
                if fake {!encoding_drag_revision_matches(&t.encoding_revisions,p.state,current.as_ref(),t.processing_page)} else {
                    let ghost=(t.encoding && super::lan_crafting::input(p.input)==Input::Clone).then_some(p.slot);
                    !encoding_revision_matches(&t.encoding_revisions,p.state,current.as_ref(),t.processing_page,ghost)
                }
            });
            if stale && (0..=53).contains(&p.slot) {
                if self.lan_load_encoding_state(host,conn){self.lan_send_ae2_terminal(host,conn);return true;}
            }
        }
        if encoding_part.is_some() && !self.lan_load_encoding_state(host,conn) {
            self.lan_ae2_terminal_closed(host,conn,None);
            host.hub.send_to(conn,&horizon_protocol::packets::play::Clientbound::ContainerClose(horizon_protocol::packets::play::ContainerClose{container:p.container}));
            return true;
        }
        let owner = craft_owner(craft_part, wireless.as_ref());
        let page_size = if owner.is_some() { 27 } else { PAGE_ITEMS };
        let valid = wireless.as_ref().map_or_else(
            || {
                host.hub
                    .guests
                    .iter()
                    .find(|g| g.conn == conn)
                    .is_some_and(|g| {
                        !g.spectator() && g.within_block_range([pos.0, pos.1, pos.2], 4.0)
                    })
                    && self.ae2_has_terminal(pos)
                    && host
                        .play
                        .get(&conn)
                        .and_then(|p| p.ae2_terminal.as_ref())
                        .is_some_and(|t| t.owner_dim == self.dim.id())
            },
            |ctx| self.ae2_wireless_source_valid(host, conn, ctx),
        );
        let valid = valid
            && craft_part.is_none_or(|part| {
                self.ae2_part_machine(part).ok().flatten().is_some_and(|m| {
                    m.kind == horizon_client::ae2_world::MachineKind::CraftingTerminal
                })
            })
            && host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref()).is_some_and(|t|
                t.view_part.is_none_or(|part|t.view_identity.is_some() && self.ae2_terminal_part_identity(part)==t.view_identity));
        if !valid {
            self.lan_ae2_terminal_closed(host, conn, None);
            host.hub.send_to(
                conn,
                &horizon_protocol::packets::play::Clientbound::ContainerClose(
                    horizon_protocol::packets::play::ContainerClose {
                        container: p.container,
                    },
                ),
            );
            return true;
        }
        if let Some(ctx)=wireless.as_ref() {
            let editing=host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref()).is_some_and(|t|t.key_type_edit);
            let input=super::lan_crafting::input(p.input);
            let type_control=p.slot==52 && matches!(input,Input::Pickup|Input::QuickMove);
            if editing || type_control {
                let mut selection=None;
                if let Some(guest)=host.hub.guests.iter_mut().find(|guest|guest.conn==conn) {
                    if let Some(stack)=toolbox_stack_mut(guest,ctx.source) {
                        if let Ok(current)=super::ae2_wireless::enabled_key_types(stack) {
                            let next=if editing && input==Input::Pickup && matches!(p.slot,0|1) {
                                Some(super::ae2_wireless::toggle_key_type(current,p.slot==1))
                            } else if !editing && type_control && input==Input::QuickMove {
                                Some(super::ae2_wireless::cycle_key_types(current))
                            } else {None};
                            if let Some(next)=next { if super::ae2_wireless::store_enabled_key_types(stack,next).is_ok(){selection=Some(next);} }
                        }
                    }
                }
                if let Some(t)=host.play.get_mut(&conn).and_then(|p|p.ae2_terminal.as_mut()) {
                    if !editing && type_control && input==Input::Pickup {t.key_type_edit=true;t.search_edit=false;t.craft_amount_edit=false;}
                    if editing && input==Input::Pickup && p.slot==2 {t.key_type_edit=false;}
                    if selection.is_some(){t.page=0;}
                    t.craft_drag=None;t.wireless_drag=None;t.toolbox_drag=None;
                }
                if selection.is_some() {if let Some(gi)=host.hub.guests.iter().position(|guest|guest.conn==conn){host.hub.send_inventory(gi);}}
                self.lan_send_ae2_terminal(host,conn);
                return true;
            }
        }
        let toolbox_owner = host.play.get(&conn).and_then(|p| p.ae2_terminal.as_ref())
            .and_then(|t| t.toolbox_owner);
        // Apply the wireless source lock before any toolbox or matrix route.
        // In particular a predicted offhand swap needs an inventory update as
        // well as a container update to put the real owner back on the client.
        if let Some(ctx) = &wireless {
            let player_slot = match p.slot {
                54..=80 => Some((p.slot - 54 + 9) as usize),
                81..=89 => Some((p.slot - 81) as usize),
                _ => None,
            };
            let input = super::lan_crafting::input(p.input);
            if (player_slot == Some(ctx.source) && !matches!(input, Input::QuickCraft | Input::PickupAll))
                || (input == Input::Swap && wireless_toolbox_swap_refused(ctx.source, toolbox_owner, p.button))
            {
                if let Some(t) = host.play.get_mut(&conn).and_then(|p| p.ae2_terminal.as_mut()) {
                    t.wireless_drag = None; t.toolbox_drag = None; t.craft_drag = None;
                }
                if let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn) {
                    host.hub.send_inventory(gi);
                }
                self.lan_send_ae2_terminal(host, conn);
                return true;
            }
        }
        if let Some(owner) = toolbox_owner {
            let exists = host.hub.guests.iter().find(|g| g.conn == conn)
                .and_then(|g| toolbox_stack(g, owner))
                .is_some_and(|s| horizon_client::ae2_network_tool::Inventory::read(s).is_ok());
            if !exists {
                self.lan_ae2_terminal_closed(host, conn, None);
                host.hub.send_to(conn, &C::ContainerClose(play::ContainerClose { container: p.container }));
                return true;
            }
            let player_slot = match p.slot {
                54..=80 => Some((p.slot - 54 + 9) as usize),
                81..=89 => Some((p.slot - 81) as usize),
                _ => None,
            };
            let input = super::lan_crafting::input(p.input);
            if player_slot == Some(owner) && input == Input::Clone {
                let gi = host.hub.guests.iter().position(|g| g.conn == conn);
                if let Some(gi) = gi {
                    let g = &mut host.hub.guests[gi];
                    if g.creative() && g.carried.is_none() {
                        if let Some(mut copy) = toolbox_stack(g, owner).cloned() {
                            horizon_client::ae2_stack_identity::detach_if_tracked(&mut copy);
                            copy.count = self.res.items().stack_max(&copy);
                            g.carried = Some(copy);
                        }
                    }
                }
                self.lan_send_ae2_terminal(host, conn);
                return true;
            }
            if (player_slot == Some(owner) && input != Input::PickupAll)
                || (input == Input::Swap && owner == 40)
            {
                // A rejected drag-add is simply omitted from the pending set.
                if input != Input::QuickCraft {
                    if let Some(t) = host.play.get_mut(&conn).and_then(|p| p.ae2_terminal.as_mut()) {
                        t.craft_drag = None; t.toolbox_drag = None;
                    }
                    if input == Input::Swap && owner == 40 {
                        if let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn) {
                            host.hub.send_inventory(gi);
                        }
                    }
                    self.lan_send_ae2_terminal(host, conn);
                }
                return true;
            }
            // Vanilla SWAP accesses the hotbar directly, bypassing the
            // disabled slot. If the target accepts the tool, the locator is
            // checked again after the exchange.
            if input == Input::Swap && owner < 9 && usize::try_from(p.button).ok() == Some(owner)
                && player_slot.is_some()
            {
                if let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn) {
                    let mut menu = horizon_client::menu::Menu::new(horizon_client::menu::Kind::Chest(6));
                    self.guest_menu_op(host, gi, &mut menu, |menu, env| {
                        let _ = menu.click(env, i32::from(p.slot), i32::from(p.button), Input::Swap);
                    });
                }
                self.lan_send_ae2_terminal(host, conn);
                return true;
            }
        }
        let panel_drag=super::lan_crafting::input(p.input)==Input::QuickCraft && (p.slot==-990 || p.slot==-991 || (90..95).contains(&p.slot));
        if super::lan_crafting::input(p.input) != Input::QuickCraft || panel_drag {
            if let Some(t) = host.play.get_mut(&conn).and_then(|p| p.ae2_terminal.as_mut()) {
                t.craft_drag = None;
                t.toolbox_drag = None;
                t.wireless_drag = None;
            }
        }
        if panel_drag {
            let Some(gi)=host.hub.guests.iter().position(|g|g.conn==conn) else{return true;};
            let mode=(i32::from(p.button)>>2)&3;
            let stage=i32::from(p.button)&3;
            if p.slot==-990 {
                let carried=host.hub.guests[gi].carried.clone().filter(|s|s.item.as_ref()=="ae2:view_cell" && s.count==1 && host.hub.guests[gi].creative() && mode==2 && stage==0);
                if let Some(t)=host.play.get_mut(&conn).and_then(|v|v.ae2_terminal.as_mut()) {t.view_drag=carried.map(|carried|ViewCellDrag{carried,slots:Vec::new()});}
            } else if (90..95).contains(&p.slot) {
                if let Some(drag)=host.play.get_mut(&conn).and_then(|v|v.ae2_terminal.as_mut()).and_then(|t|t.view_drag.as_mut()) {
                    let i=(p.slot-90) as usize;if mode==2 && stage==1 && host.hub.guests[gi].carried.as_ref()==Some(&drag.carried) && !drag.slots.contains(&i){drag.slots.push(i);}
                }
            } else {
                let session=host.play.get_mut(&conn).and_then(|v|v.ae2_terminal.as_mut());
                let part=session.as_ref().and_then(|t|t.view_part);let drag=session.and_then(|t|t.view_drag.take());
                if let (Some(part),Some(drag))=(part,drag) {
                    if mode==2 && stage==2 && host.hub.guests[gi].creative() && host.hub.guests[gi].carried.as_ref()==Some(&drag.carried) {
                        if let Some(mut machine)=self.ae2_part_machine(part).ok().flatten() {
                            if let Ok(mut cells)=super::ae2_parts::ae2_terminal_view_cells(&machine) {
                                for i in drag.slots {if cells[i].is_none(){cells[i]=Some(drag.carried.clone());}}
                                if super::ae2_parts::ae2_set_terminal_view_cells(&mut machine,&cells).is_ok() && self.ae2_store_part_machine(part,&machine).is_ok() {
                                    let viewers:Vec<_>=host.play.iter().filter_map(|(&c,v)|v.ae2_terminal.as_ref().filter(|t|t.view_part==Some(part)).map(|_|c)).collect();
                                    for viewer in viewers {self.lan_send_ae2_terminal(host,viewer);}return true;
                                }
                            }
                        }
                    }
                }
                self.lan_send_ae2_terminal(host,conn);
            }
            return true;
        }
        if (54..90).contains(&p.slot) && super::lan_crafting::input(p.input)==Input::QuickMove {
            let part=host.play.get(&conn).and_then(|v|v.ae2_terminal.as_ref()).and_then(|t|t.view_part);
            if let (Some(part),Some(gi))=(part,host.hub.guests.iter().position(|g|g.conn==conn)) {
                let slot=if p.slot<81 {(p.slot-54+9) as usize}else{(p.slot-81) as usize};
                if let Some(mut held)=host.hub.guests[gi].inventory[slot].clone().filter(|s|s.item.as_ref()=="ae2:view_cell" && s.count>0) {
                    if let Some(mut machine)=self.ae2_part_machine(part).ok().flatten() {
                        if let Ok(mut cells)=super::ae2_parts::ae2_terminal_view_cells(&machine) {
                            if let Some(i)=cells.iter().position(Option::is_none) {
                                let mut one=held.clone();one.count=1;cells[i]=Some(one);held.count-=1;
                                if super::ae2_parts::ae2_set_terminal_view_cells(&mut machine,&cells).is_ok() && self.ae2_store_part_machine(part,&machine).is_ok() {
                                    host.hub.guests[gi].inventory[slot]=(held.count>0).then_some(held);
                                    let viewers:Vec<_>=host.play.iter().filter_map(|(&c,v)|v.ae2_terminal.as_ref().filter(|t|t.view_part==Some(part)).map(|_|c)).collect();
                                    for viewer in viewers {self.lan_send_ae2_terminal(host,viewer);}return true;
                                }
                            }
                        }
                    }
                    self.lan_send_ae2_terminal(host,conn);return true;
                }
            }
        }
        // Five physical side-panel slots use dedicated packet indices beyond
        // the generic chest/player range; they never alias network entries.
        if (90..95).contains(&p.slot) {
            let part=host.play.get(&conn).and_then(|v|v.ae2_terminal.as_ref()).and_then(|t|t.view_part);
            if let (Some(part),Some(gi))=(part,host.hub.guests.iter().position(|g|g.conn==conn)) {
                if let Some(mut machine)=self.ae2_part_machine(part).ok().flatten() {
                    if let Ok(mut cells)=super::ae2_parts::ae2_terminal_view_cells(&machine) {
                        let i=(p.slot-90) as usize;let input=super::lan_crafting::input(p.input);
                        let mut carried=host.hub.guests[gi].carried.clone();let mut inventory=host.hub.guests[gi].inventory.clone();let mut offhand=host.hub.guests[gi].offhand.clone();let mut changed=false;let mut drops=Vec::new();
                        if input==Input::Pickup && p.button==2 && host.hub.guests[gi].creative() && cells[i].is_none() {
                            if let Some(held)=carried.as_ref().filter(|s|s.item.as_ref()=="ae2:view_cell" && s.count==1) {cells[i]=Some(held.clone());changed=true;}
                        } else if input==Input::Pickup && (0..=1).contains(&p.button) {
                            match (cells[i].clone(),carried.clone()) {
                                (Some(cell),None)=>{cells[i]=None;carried=Some(cell);changed=true;}
                                (None,Some(mut held)) if held.item.as_ref()=="ae2:view_cell" && held.count>0=>{let mut one=held.clone();one.count=1;cells[i]=Some(one);held.count-=1;carried=(held.count>0).then_some(held);changed=true;}
                                (Some(cell),Some(held)) if held.item.as_ref()=="ae2:view_cell" && held.count==1=>{cells[i]=Some(held);carried=Some(cell);changed=true;}
                                _=>{}
                            }
                        } else if input==Input::QuickMove {
                            if let Some(cell)=cells[i].clone() {if let Some(index)=inventory.iter().position(Option::is_none){inventory[index]=Some(cell);cells[i]=None;changed=true;}}
                        } else if input==Input::Swap && (0..=8).contains(&p.button) {
                            let hotbar=p.button as usize;
                            if inventory[hotbar].as_ref().is_none_or(|s|s.item.as_ref()=="ae2:view_cell" && s.count==1) {
                                std::mem::swap(&mut inventory[hotbar],&mut cells[i]);changed=true;
                            }
                        } else if input==Input::Swap && p.button==40 {
                            if offhand.as_ref().is_none_or(|s|s.item.as_ref()=="ae2:view_cell" && s.count==1) {
                                std::mem::swap(&mut offhand,&mut cells[i]);changed=true;
                            }
                        } else if input==Input::Clone && host.hub.guests[gi].creative() && carried.is_none() {
                            if let Some(cell)=cells[i].clone() {carried=Some(cell);changed=true;}
                        } else if input==Input::Throw && (0..=1).contains(&p.button) {
                            if let Some(cell)=cells[i].take() {drops.push(cell);changed=true;}
                        }
                        if changed && super::ae2_parts::ae2_set_terminal_view_cells(&mut machine,&cells).is_ok() && self.ae2_store_part_machine(part,&machine).is_ok() {
                            host.hub.guests[gi].carried=carried;host.hub.guests[gi].inventory=inventory;host.hub.guests[gi].offhand=offhand;
                            if input==Input::Swap && p.button==40 {host.hub.send_inventory(gi);}
                            self.drop_guest_items(host,gi,drops);
                            let viewers:Vec<_>=host.play.iter().filter_map(|(&c,v)|v.ae2_terminal.as_ref().filter(|t|t.view_part==Some(part)).map(|_|c)).collect();
                            for viewer in viewers {self.lan_send_ae2_terminal(host,viewer);}
                            return true;
                        }
                    }
                }
            }
            self.lan_send_ae2_terminal(host,conn);return true;
        }
        let input = super::lan_crafting::input(p.input);
        let has_crafting_matrix = owner.is_some();
        if let Some(owner) = toolbox_owner {
            if input == Input::PickupAll && ((95..=103).contains(&p.slot)
                || (54..=89).contains(&p.slot) || (has_crafting_matrix && (27..=35).contains(&p.slot)))
            {
                if let Err(message) = self.lan_terminal_toolbox_pickup_all(host, conn, owner, craft_part, wireless.as_ref(), p.slot, p.button) {
                    self.as_guest(host, conn, |g| g.system_message(Text::literal(format!("Terminal: {message}"))));
                }
                if wireless.is_some() { if let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn) { host.hub.send_inventory(gi); } }
                if let Some(part) = craft_part {
                    let viewers: Vec<_> = host.play.iter().filter_map(|(&id, p)|
                        p.ae2_terminal.as_ref().filter(|t| t.craft_part == Some(part)).map(|_| id)).collect();
                    for viewer in viewers { self.lan_send_ae2_terminal(host, viewer); }
                } else { self.lan_send_ae2_terminal(host, conn); }
                return true;
            }
            if (95..=103).contains(&p.slot) && input != Input::QuickCraft {
                if let Err(message) = self.lan_terminal_toolbox_click(host, conn, pos, owner, (p.slot - 95) as usize, p.button, input) {
                    self.as_guest(host, conn, |g| g.system_message(Text::literal(format!("Terminal: {message}"))));
                }
                self.lan_send_ae2_terminal(host, conn);
                return true;
            }
        }
        let connected = connected
            && wireless
                .as_ref()
                .is_none_or(|ctx| self.ae2_wireless_validate(host, conn, ctx));
        if !connected {
            if let Some(t) = host
                .play
                .get_mut(&conn)
                .and_then(|p| p.ae2_terminal.as_mut())
            {
                t.wireless_connected = false;
            }
        }
        if !connected
            && ((0..=53).contains(&p.slot) || super::lan_crafting::input(p.input) != Input::Pickup)
        {
            if let Some(t) = host.play.get_mut(&conn).and_then(|p| p.ae2_terminal.as_mut()) {
                t.craft_drag = None;
                t.wireless_drag = None;
            }
            self.lan_send_ae2_terminal(host, conn);
            return true;
        }
        if let Some(ctx) = &wireless {
            let input = super::lan_crafting::input(p.input);
            let overlay = host.play.get(&conn).and_then(|p| p.ae2_terminal.as_ref())
                .is_some_and(|t| t.craft_amount_edit || t.search_edit || t.encoding);
            if input == Input::QuickCraft && toolbox_owner.is_none() {
                if !overlay { self.lan_wireless_drag(host, conn, ctx, p.slot, p.button); }
                else if let Some(t) = host.play.get_mut(&conn).and_then(|p| p.ae2_terminal.as_mut()) { t.wireless_drag = None; }
                return true;
            }
            if input == Input::PickupAll {
                if !overlay {
                    if let Err(message) = self.lan_wireless_pickup_all(host, conn, ctx, p.slot, p.button) {
                        self.as_guest(host, conn, |g| g.system_message(Text::literal(format!("Wireless terminal: {message}"))));
                    }
                }
                if let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn) { host.hub.send_inventory(gi); }
                self.lan_send_ae2_terminal(host, conn);
                return true;
            }
            if input == Input::Swap && !(owner.is_some() && (27..=35).contains(&p.slot) && !overlay) {
                self.lan_wireless_player_swap(host, conn, ctx, p.slot, p.button);
                return true;
            }
        }
        if let Some(owner) = toolbox_owner {
            if super::lan_crafting::input(p.input) == Input::QuickCraft {
                let card_cursor = host.hub.guests.iter().find(|g| g.conn == conn)
                    .and_then(|g| g.carried.as_ref())
                    .is_some_and(|s| horizon_client::ae2_network_tool::is_card(&s.item));
                let overlay = host.play.get(&conn).and_then(|p| p.ae2_terminal.as_ref())
                    .is_some_and(|t| t.craft_amount_edit || t.search_edit || (t.encoding && !card_cursor));
                if !overlay {
                    self.lan_terminal_toolbox_drag(host, conn, owner, craft_part, wireless.as_ref(), p.slot, p.button);
                } else if let Some(t) = host.play.get_mut(&conn).and_then(|p| p.ae2_terminal.as_mut()) {
                    t.toolbox_drag = None;
                }
                return true;
            }
        }
        if let Some(part) = craft_part {
            let overlay = host.play.get(&conn).and_then(|p| p.ae2_terminal.as_ref())
                .is_some_and(|t| t.craft_amount_edit || t.search_edit || t.encoding);
            if !overlay && super::lan_crafting::input(p.input) == Input::QuickCraft {
                self.lan_crafting_terminal_drag(host, conn, part, p.slot, p.button);
                return true;
            }
            if overlay {
                if let Some(t) = host.play.get_mut(&conn).and_then(|p| p.ae2_terminal.as_mut()) {
                    t.craft_drag = None;
                }
            }
        }
        let current = if connected {
            self.ae2_terminal_entries(pos)
        } else {
            Ok(Vec::new())
        };
        let craftable_now = {
            let mut keys = self.ae2_craftable_outputs(pos);
            keys.extend(self.ae2_craftable_fluids(pos).iter().map(fluid_marker));
            keys
        };
        let matches_snapshot = host
            .play
            .get(&conn)
            .and_then(|x| x.ae2_terminal.as_ref())
            .is_some_and(|t| {
                current.as_ref().is_ok_and(|v| {
                    *v == t.snapshot
                        && terminal_pinned_entries(v,terminal_filter_entries(
                            &terminal_key_types(&self.ae2_view_filtered(v,t.view_part),wireless_key_types(host,conn,t.wireless.as_ref())),
                            &craftable_now,
                            t.sort_mode,
                            t.sort_descending,
                            t.filter_mode,
                            &t.search,
                            &|key|self.ae2_terminal_display_name(key,&t.search_language),
                            &|key,query|self.ae2_terminal_search_matches(key,query,t.search_mod_name,&t.search_language),
                        ),&t.pinned,page_size) == t.displayed_snapshot
                })
            });
        // An indexed storage view can be reordered between updates. Only
        // those virtual-key clicks require the exact displayed revision;
        // physical matrix, player and control slots use current server state.
        let virtual_key_click = host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref()).is_some_and(|t|
            !t.encoding && !t.craft_amount_edit && !t.search_edit && p.slot>=0 && (p.slot as usize)<page_size);
        let same_key=host.play.get(&conn).and_then(|x|x.ae2_terminal.as_ref()).is_some_and(|t|
            terminal_view_revision_matches(&t.storage_revisions,p.state,&t.displayed_snapshot,page,page_size,p.slot as usize));
        if virtual_key_click && (!matches_snapshot || !same_key) {
            self.lan_send_ae2_terminal(host, conn);
            return true;
        }
        if let Some(part) = craft_part {
            let overlay = host
                .play
                .get(&conn)
                .and_then(|p| p.ae2_terminal.as_ref())
                .is_some_and(|t| t.craft_amount_edit || t.search_edit || t.encoding);
            if !overlay && super::lan_crafting::input(p.input) == Input::PickupAll
                && ((27..=35).contains(&p.slot) || (54..=89).contains(&p.slot))
            {
                if let Err(message) = self.lan_crafting_terminal_pickup_all(host, conn, part, p.slot, p.button) {
                    self.as_guest(host, conn, |g| {
                        g.system_message(Text::literal(format!("Crafting terminal: {message}")))
                    });
                }
                let viewers: Vec<_> = host.play.iter().filter_map(|(&id, p)| {
                    p.ae2_terminal.as_ref().filter(|t| t.craft_part == Some(part)).map(|_| id)
                }).collect();
                for viewer in viewers { self.lan_send_ae2_terminal(host, viewer); }
                return true;
            }
        }
        if let Some(owner) = owner.as_ref() {
            let overlay = host.play.get(&conn).and_then(|p| p.ae2_terminal.as_ref())
                .is_some_and(|t| t.craft_amount_edit || t.search_edit || t.encoding);
            if !overlay && (27..=38).contains(&p.slot) {
                let result = self.lan_crafting_terminal_click(
                    host,
                    conn,
                    owner,
                    pos,
                    p.slot,
                    p.button,
                    super::lan_crafting::input(p.input),
                );
                if let Err(message) = result {
                    self.as_guest(host, conn, |g| {
                        g.system_message(Text::literal(format!("Crafting terminal: {message}")))
                    });
                }
                if matches!(owner, CraftOwner::Wireless(ctx) if ctx.source == super::shield::OFFHAND_SLOT
                    || (super::lan_crafting::input(p.input) == Input::Swap && p.button == 40)) {
                    if let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn) {
                        host.hub.send_inventory(gi);
                    }
                }
                let viewers: Vec<_> = match owner {
                    CraftOwner::Wireless(_) => vec![conn],
                    CraftOwner::Part(part) => host
                    .play
                    .iter()
                    .filter_map(|(&id, p)| {
                        p.ae2_terminal
                            .as_ref()
                            .filter(|t| t.craft_part == Some(*part))
                            .map(|_| id)
                    })
                    .collect(),
                };
                for viewer in viewers {
                    self.lan_send_ae2_terminal(host, viewer);
                }
                return true;
            }
        }
        if p.slot == 48
            && craft_part.is_none()
            && encoding_part.is_some()
            && !host
                .play
                .get(&conn)
                .and_then(|p| p.ae2_terminal.as_ref())
                .is_some_and(|t| t.search_edit)
            && super::lan_crafting::input(p.input) == Input::Pickup
        {
            if let Some(t) = host
                .play
                .get_mut(&conn)
                .and_then(|p| p.ae2_terminal.as_mut())
            {
                t.encoding = !t.encoding;
            }
            self.lan_send_ae2_terminal(host, conn);
            return true;
        }
        let encoding = host
            .play
            .get(&conn)
            .and_then(|p| p.ae2_terminal.as_ref())
            .is_some_and(|t| t.encoding);
        if encoding && super::lan_crafting::input(p.input)==Input::Clone && p.button==2 && ((0..=8).contains(&p.slot)||(18..=20).contains(&p.slot)) {
            return self.lan_open_processing_amount(host,conn,p.container,p.slot);
        }
        if encoding && (54..90).contains(&p.slot) && super::lan_crafting::input(p.input)==horizon_client::menu::Input::QuickMove {
            if let Some(part)=encoding_part {
                let index=if p.slot<81 {9+(p.slot as usize-54)} else {p.slot as usize-81};
                match self.lan_encoding_quick_insert(host,conn,part,index) {
                    Ok(true)=>{self.lan_load_encoding_state(host,conn);self.lan_send_ae2_terminal(host,conn);return true;}
                    Err(message)=>{self.as_guest(host,conn,|g|g.system_message(Text::literal(format!("Encoding: {message}"))));self.lan_send_ae2_terminal(host,conn);return true;}
                    Ok(false)=>{}
                }
            }
        }
        if encoding && (p.slot==49 || p.slot==50) {
            if let Some(part)=encoding_part {
                if let Err(message)=self.lan_encoding_slot_click(host,conn,part,(p.slot-49) as usize,p.button,super::lan_crafting::input(p.input)) {
                    self.as_guest(host,conn,|g|g.system_message(Text::literal(format!("Encoding: {message}"))));
                }
                self.lan_load_encoding_state(host,conn);
            }
            self.lan_send_ae2_terminal(host,conn);return true;
        }
        if encoding && matches!(p.button,2|3) && super::lan_crafting::input(p.input)==Input::Pickup
            && ((0..=8).contains(&p.slot)||(18..=20).contains(&p.slot))
            && host.hub.guests.iter().find(|g|g.conn==conn).is_none_or(|g|g.carried.is_none()) {
            self.lan_send_ae2_terminal(host,conn);return true;
        }
        if encoding && (0..=53).contains(&p.slot) {
            if (30..=40).contains(&p.slot) && super::lan_crafting::input(p.input) == Input::Pickup {
                if let Some(t) = host
                    .play
                    .get_mut(&conn)
                    .and_then(|p| p.ae2_terminal.as_mut())
                {
                    if t.processing {
                        if p.slot == 40 {
                            t.fluid_amount = 0;
                        } else {
                            let digit = (p.slot - 30) as u64;
                            if let Some(n) = t
                                .fluid_amount
                                .checked_mul(10)
                                .and_then(|n| n.checked_add(digit))
                                .filter(|n| *n <= i64::MAX as u64)
                            {
                                t.fluid_amount = n;
                            }
                        }
                    }
                }
            } else if (42..=44).contains(&p.slot)
                && super::lan_crafting::input(p.input) == Input::Pickup
            {
                let special=host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref()).filter(|t|t.special_mode==2 && (p.slot==42 || p.slot==43)).and_then(|t|t.pattern_grid.first().and_then(Option::as_ref).map(|item|(item.clone(),t.stone_recipe.clone())));
                if let Some((item,current))=special {
                    let recipes=self.res.stations().recipes.stonecutter(&item);if !recipes.is_empty(){
                        let index=current.as_ref().and_then(|id|recipes.iter().position(|r|&r.id==id)).unwrap_or(0);
                        let next=if p.slot==42 {(index+recipes.len()-1)%recipes.len()}else{(index+1)%recipes.len()};
                        let id=recipes[next].id.clone();if let Some(t)=host.play.get_mut(&conn).and_then(|p|p.ae2_terminal.as_mut()){t.stone_recipe=Some(id);}
                    }
                }
                if let Some(t) = host
                    .play
                    .get_mut(&conn)
                    .and_then(|p| p.ae2_terminal.as_mut())
                {
                    if !t.processing && p.slot == 44 {
                        t.fluid_substitutes = !t.fluid_substitutes;
                    }
                    if t.processing {
                        match p.slot {
                            42 => t.processing_page = (t.processing_page + 8) % 9,
                            43 => t.processing_page = (t.processing_page + 1) % 9,
                            44 => t.selected_fluid = t.selected_fluid.wrapping_add(1),
                            _ => {}
                        }
                    }
                }
            } else if p.slot == 45 && super::lan_crafting::input(p.input) == Input::Pickup {
                if let Some(t) = host
                    .play
                    .get_mut(&conn)
                    .and_then(|p| p.ae2_terminal.as_mut())
                {
                    if t.processing {
                        t.fluid_mode = !t.fluid_mode;
                    }
                }
            } else if p.slot == 46 && super::lan_crafting::input(p.input) == Input::Pickup {
                if let Some(t) = host
                    .play
                    .get_mut(&conn)
                    .and_then(|p| p.ae2_terminal.as_mut())
                {
                    match (t.processing,t.special_mode) {(false,0)=>t.processing=true,(true,_)=>{t.processing=false;t.special_mode=1;},(false,1)=>t.special_mode=2,_=>t.special_mode=0}
                }
            } else if p.slot == 47 && super::lan_crafting::input(p.input) == Input::Pickup {
                if let Some(t) = host
                    .play
                    .get_mut(&conn)
                    .and_then(|p| p.ae2_terminal.as_mut())
                {
                    t.substitutes = !t.substitutes;
                }
            } else if (0..=8).contains(&p.slot)
                && matches!(super::lan_crafting::input(p.input),Input::Pickup|Input::QuickMove)
            {
                let mut held = host
                    .hub
                    .guests
                    .iter()
                    .find(|g| g.conn == conn)
                    .and_then(|g| g.carried.clone());
                if p.button==2 {if let Some(item)=held.as_mut(){item.count=1;}}
                let drag=matches!(p.button,2|3);
                let available_fluids = self.ae2_grid_fluids(pos).unwrap_or_default();
                let cursor=super::ae2_encoding::cursor_ghost(held.as_ref(),&|id,fluid|fluid||self.res.items().get(id).is_some());
                let allowed=host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref()).is_some_and(|t|t.special_mode==0 || (t.special_mode==1 && p.slot<3) || (t.special_mode==2 && p.slot==0));
                if allowed {if let Some(t) = host
                    .play
                    .get_mut(&conn)
                    .and_then(|p| p.ae2_terminal.as_mut())
                {
                    let index = p.slot as usize
                        + if t.processing {
                            t.processing_page.min(8) * 9
                        } else {
                            0
                        };
                    let selected=encoding_selected_fluid(held.as_ref(),t.fluid_amount,t.selected_fluid,&available_fluids);
                    if let Some(fluid)=t.processing.then(||encoding_emptying_action(held.as_ref(),i32::from(p.button))).flatten() {
                        t.pattern_grid[index]=None;t.fluid_inputs[index]=Some(fluid);
                    } else if let Some(value)=super::ae2_encoding::select_ghost(&cursor,!drag && t.processing && t.fluid_mode,selected,p.button==1) {
                        let max=t.pattern_grid[index].as_ref().map_or(64,|item|self.ae2_config_item_max(item));
                        super::ae2_encoding::click_ghost(&mut t.pattern_grid[index],&mut t.fluid_inputs[index],value,p.button==1,max,i32::MAX as u32,i64::MAX as u64);
                    }
                }}
                let special_input=host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref()).filter(|t|t.special_mode==2 && p.slot==0).and_then(|t|t.pattern_grid[0].clone());
                if let Some(item)=special_input {let selected=self.res.stations().recipes.stonecutter(&item).first().map(|r|r.id.clone());if let Some(t)=host.play.get_mut(&conn).and_then(|p|p.ae2_terminal.as_mut()){t.stone_recipe=selected;}}
            } else if (18..=20).contains(&p.slot)
                && matches!(super::lan_crafting::input(p.input),Input::Pickup|Input::QuickMove)
            {
                let mut held = host
                    .hub
                    .guests
                    .iter()
                    .find(|g| g.conn == conn)
                    .and_then(|g| g.carried.clone());
                if p.button==2 {if let Some(item)=held.as_mut(){item.count=1;}}
                let drag=matches!(p.button,2|3);
                let available_fluids = self.ae2_grid_fluids(pos).unwrap_or_default();
                let cursor=super::ae2_encoding::cursor_ghost(held.as_ref(),&|id,fluid|fluid||self.res.items().get(id).is_some());
                if let Some(t) = host
                    .play
                    .get_mut(&conn)
                    .and_then(|p| p.ae2_terminal.as_mut())
                {
                    if t.processing {
                        let index = p.slot as usize - 18 + t.processing_page.min(8) * 3;
                        let selected=encoding_selected_fluid(held.as_ref(),t.fluid_amount,t.selected_fluid,&available_fluids);
                        if let Some(fluid)=encoding_emptying_action(held.as_ref(),i32::from(p.button)) {
                            t.processing_outputs[index]=None;t.fluid_outputs[index]=Some(fluid);
                        } else if let Some(value)=super::ae2_encoding::select_ghost(&cursor,!drag && t.fluid_mode,selected,p.button==1) {
                            let max=t.processing_outputs[index].as_ref().map_or(64,|item|self.ae2_config_item_max(item));
                            super::ae2_encoding::click_ghost(&mut t.processing_outputs[index],&mut t.fluid_outputs[index],value,p.button==1,max,i32::MAX as u32,i64::MAX as u64);
                        }
                    }
                }
            } else if p.slot == 13 && super::lan_crafting::input(p.input) == Input::Pickup {
                let (
                    grid,
                    substitutes,
                    fluid_substitutes,
                    processing,
                    special_mode,
                    stone_recipe,
                    outputs,
                    fluid_inputs,
                    fluid_outputs,
                ) = host
                    .play
                    .get(&conn)
                    .and_then(|p| p.ae2_terminal.as_ref())
                    .map(|t| {
                        (
                            t.pattern_grid.clone(),
                            t.substitutes,
                            t.fluid_substitutes,
                            t.processing,
                            t.special_mode,
                            t.stone_recipe.clone(),
                            t.processing_outputs.clone(),
                            t.fluid_inputs.clone(),
                            t.fluid_outputs.clone(),
                        )
                    })
                    .unwrap_or_default();
                let pattern = if processing {
                    if self.res.items().get("ae2:processing_pattern").is_some() {
                        // Preserve the source GenericStack ghost-slot order,
                        // including whether slot zero is an item or a fluid.
                        super::ae2_processing::ae2_encode_processing_positioned(
                            &grid,
                            &outputs,
                            &fluid_inputs,
                            &fluid_outputs,
                        )
                        .ok()
                    } else {
                        None
                    }
                } else if special_mode!=0 {
                    let count=if special_mode==1 {3}else{1};let mut special_grid=vec![None;9];special_grid[..count].clone_from_slice(&grid[..count]);
                    let recipe=if special_mode==1 {self.res.stations().recipes.smithing_recipe_result(&self.res.stations().enchantments,special_grid[0].as_ref(),special_grid[1].as_ref(),special_grid[2].as_ref()).map(|(id,_)|id.to_owned())} else {stone_recipe};
                    recipe.and_then(|id| {let kind=if special_mode==1 {"smithing"}else{"stonecutting"};let output=super::ae2_processing::special_recipe_result(self.res.stations(),kind,&id,&special_grid)?;super::ae2_processing::ae2_encode_special_pattern(kind,&id,&special_grid,&output,substitutes).ok()})
                } else {
                    self.res.recipes().find(&grid[..9], 3).and_then(|(recipe, out)| {
                        self.res.items().get("ae2:crafting_pattern").and_then(|_| {
                            super::ae2_processing::ae2_encode_pattern_source(
                                &grid[..9],
                                &out,
                                recipe,
                                substitutes,
                                fluid_substitutes,
                            )
                            .ok()
                        })
                    })
                };
                if let (Some(pattern),Some(part))=(pattern,encoding_part) {
                    if let Ok(Some(mut machine))=self.ae2_part_machine(part) {
                        let eligible=machine.slots.get(1).and_then(Option::as_ref).is_some_and(|item|
                            item.item.as_ref()=="ae2:blank_pattern" || super::ae2_encoding::EncodingState::from_pattern(item).is_some());
                        let empty=machine.slots.get(1).is_some_and(Option::is_none);
                        let blank=machine.slots.first().and_then(Option::as_ref).is_some_and(|item|item.item.as_ref()=="ae2:blank_pattern" && item.count>0);
                        if eligible || (empty && blank) {
                            if empty {if let Some(item)=machine.slots[0].as_mut(){item.count-=1;if item.count==0 {machine.slots[0]=None;}}}
                            machine.slots[1]=Some(pattern);
                            if let Err(message)=self.ae2_store_part_machine(part,&machine) {self.as_guest(host,conn,|g|g.system_message(Text::literal(format!("Encoding: {message}"))));}
                        } else {self.as_guest(host,conn,|g|g.system_message(Text::literal("Insert a blank pattern into the terminal")));}
                    }
                }
            }
            if let Err(message)=self.lan_store_encoding_state(host,conn) {
                self.as_guest(host,conn,|g|g.system_message(Text::literal(format!("Encoding: {message}"))));
                self.lan_load_encoding_state(host,conn);
            }
            self.lan_send_ae2_terminal(host, conn);
            return true;
        }
        if craft_amount_edit && (0..=53).contains(&p.slot) {
            if super::lan_crafting::input(p.input) == Input::Pickup {
                if let Some(t) = host
                    .play
                    .get_mut(&conn)
                    .and_then(|p| p.ae2_terminal.as_mut())
                {
                    match p.slot {
                        0..=9 => {
                            if let Some(n) = t
                                .craft_amount
                                .checked_mul(10)
                                .and_then(|n| n.checked_add(p.slot as u64))
                                .filter(|n| *n <= 1_000_000)
                            {
                                t.craft_amount = n;
                            }
                        }
                        10 => t.craft_amount = 0,
                        11 => {
                            if t.craft_amount > 0 {
                                t.craft_amount_edit = false;
                            }
                        }
                        _ => {}
                    }
                }
            }
            self.lan_send_ae2_terminal(host, conn);
            return true;
        }
        if p.slot == 50 && super::lan_crafting::input(p.input) == Input::Pickup {
            if let Some(t) = host
                .play
                .get_mut(&conn)
                .and_then(|p| p.ae2_terminal.as_mut())
            {
                t.craft_amount_edit = true;
            }
            self.lan_send_ae2_terminal(host, conn);
            return true;
        }
        if p.slot == 49 && super::lan_crafting::input(p.input) == Input::Pickup {
            let cpus = self.ae2_terminal_status_cpus(pos);
            let selected = host
                    .play
                    .get(&conn)
                    .and_then(|p| p.ae2_terminal.as_ref())
                    .map_or(0, |t| t.job_index);
                let cpu=(!cpus.is_empty()).then(||cpus[selected % cpus.len()]);
                let (part,wireless,owner)=host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref()).map(|t|(t.craft_part,t.wireless.clone(),t.view_part.zip(t.view_identity))).unwrap_or((None,None,None));
            return self.lan_open_terminal_craft_status(host,conn,pos,cpu,part,wireless,owner);
        }
        let search_edit = host
            .play
            .get(&conn)
            .and_then(|p| p.ae2_terminal.as_ref())
            .is_some_and(|t| t.search_edit);
        if search_edit && (0..=53).contains(&p.slot) {
            if super::lan_crafting::input(p.input) == Input::Pickup {
                if let Some(t) = host
                    .play
                    .get_mut(&conn)
                    .and_then(|p| p.ae2_terminal.as_mut())
                {
                    if (0..=40).contains(&p.slot) {
                        if let Some(ch) = "abcdefghijklmnopqrstuvwxyz0123456789:_-./"
                            .chars()
                            .nth(p.slot as usize)
                        {
                            if t.search.len() < 256 {
                                t.search.push(ch);
                            }
                        }
                    } else if p.slot == 41 {
                        t.search.pop();
                    } else if p.slot == 42 {
                        t.search.clear();
                    } else if p.slot == 43 {
                        t.search_edit = false;
                    }
                    t.page = 0;
                }
            }
            self.lan_send_ae2_terminal(host, conn);
            return true;
        }
        if p.slot==52 && super::lan_crafting::input(p.input)==Input::Pickup {
            if let Some(identity)=host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref()).and_then(|t|t.chest_identity) {
                return self.lan_priority_open(host,conn,p.container,PriorityTarget::Chest{pos,identity});
            }
        }
        if matches!(p.slot, 46 | 47 | 51) && super::lan_crafting::input(p.input) == Input::Pickup {
            if let Some(identity) = host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref()).and_then(|t|t.chest_identity) {
                if matches!(p.button, 0 | 1) && self.chest_terminal_write(pos, identity, p.slot, p.button).is_none() {
                    self.lan_ae2_terminal_closed(host, conn, None);
                    host.hub.send_to(conn, &C::ContainerClose(play::ContainerClose { container: p.container }));
                    return true;
                }
                self.lan_send_ae2_terminal(host, conn);
                return true;
            }
        }
        if (p.slot == 46 || p.slot == 47 || p.slot == 50 || p.slot == 51)
            && super::lan_crafting::input(p.input) == Input::Pickup
        {
            let held = host
                .hub
                .guests
                .iter()
                .find(|g| g.conn == conn)
                .and_then(|g| g.carried.as_ref())
                .map(|s| s.item.to_string());
            if let Some(t) = host
                .play
                .get_mut(&conn)
                .and_then(|p| p.ae2_terminal.as_mut())
            {
                match p.slot {
                    46 => t.sort_mode = (t.sort_mode + 1) % 2,
                    47 => t.filter_mode = (t.filter_mode + 1) % 3,
                    51 => t.sort_descending = !t.sort_descending,
                    50 => {
                        if p.button == 1 {
                            t.search.clear();
                        } else if let Some(id) = held {
                            t.search = id;
                        } else {
                            t.search_edit = true;
                        }
                    }
                    _ => {}
                }
                t.page = 0;
            }
            self.lan_send_ae2_terminal(host, conn);
            return true;
        }
        let mut craftable = self.ae2_craftable_outputs(pos);
        craftable.extend(self.ae2_craftable_fluids(pos).iter().map(fluid_marker));
        let displayed = host
            .play
            .get(&conn)
            .and_then(|p| p.ae2_terminal.as_ref())
            .map(|t| {
                terminal_pinned_entries(&t.snapshot,terminal_filter_entries(
                    &terminal_key_types(&self.ae2_view_filtered(&t.snapshot,t.view_part),wireless_key_types(host,conn,t.wireless.as_ref())),
                    &craftable,
                    t.sort_mode,
                    t.sort_descending,
                    t.filter_mode,
                    &t.search,
                    &|key|self.ae2_terminal_display_name(key,&t.search_language),
                    &|key,query|self.ae2_terminal_search_matches(key,query,t.search_mod_name,&t.search_language),
                ),&t.pinned,page_size)
            })
            .unwrap_or_default();
        let slot = match p.slot {
            0..=53 => Some(300 + p.slot as usize),
            54..=80 => Some(9 + (p.slot as usize - 54)),
            81..=89 => Some(p.slot as usize - 81),
            _ => None,
        };
        let button = i32::from(p.button);
        let input = super::lan_crafting::input(p.input);
        // A craftable virtual entry opens confirmation without reserving inputs.
        if p.slot>=0 && (p.slot as usize)<page_size {
            let empty_cursor=host.hub.guests.iter().find(|g|g.conn==conn).is_some_and(|g|g.carried.is_none());
            if let Some((key,count))=displayed.get(page*page_size+p.slot as usize) {
                if craftable.contains(key) && (input==Input::QuickMove || (empty_cursor && *count==0)) {
                    let fluid=marker_fluid(key);
                    let is_fluid=fluid.is_some();
                    let selected=fluid.unwrap_or_else(||key.clone());
                    return self.lan_open_terminal_craft_confirm(host,conn,pos,selected,is_fluid,craft_part,wireless.clone());
                }
            }
        }
        if let Some(ctx) = &wireless {
            if slot == Some(ctx.source)
                || matches!(input, Input::Swap | Input::QuickCraft | Input::PickupAll)
            {
                self.lan_send_ae2_terminal(host, conn);
                return true;
            }
        }
        let next = self.as_guest(host, conn, |game| {
            let old_container = game.container;
            let old_page = game.ae2_terminal_page;
            game.container = Some(pos);
            game.ae2_terminal_page = page;
            let old_power = game.ae2_wireless_power;
            game.ae2_wireless_power = wireless.as_ref().map(|ctx| ctx.source);
            if let Err(message) = game.ae2_terminal_click_inner(
                slot,
                button,
                input,
                Some(&displayed),
                wireless.is_some(),
                craft_amount,
                page_size,
            ) {
                game.system_message(Text::literal(format!("ME terminal: {message}")));
            }
            let next = game.ae2_terminal_page;
            game.ae2_wireless_power = old_power;
            game.container = old_container;
            game.ae2_terminal_page = old_page;
            next
        });
        if let (Some(page), Some(t)) = (
            next,
            host.play
                .get_mut(&conn)
                .and_then(|p| p.ae2_terminal.as_mut()),
        ) {
            t.page = page;
        }
        let viewers: Vec<u32> = host
            .play
            .iter()
            .filter_map(|(&id, p)| {
                (host
                    .hub
                    .guests
                    .iter()
                    .any(|g| g.conn == id && host.hub.here(g)))
                .then(|| p.ae2_terminal.as_ref().map(|_| id))
                .flatten()
            })
            .collect();
        for viewer in viewers {
            self.lan_send_ae2_terminal(host, viewer);
        }
        true
    }
    pub(super) fn lan_ae2_terminal_closed(
        &mut self,
        host: &mut super::lan::LanHost,
        conn: u32,
        window: Option<i32>,
    ) -> bool {
        let Some(_) = host.play.get_mut(&conn).and_then(|p| {
            p.ae2_terminal
                .take_if(|t| window.is_none_or(|w| t.window == w))
        }) else {
            return false;
        };
        if let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn) {
            self.guest_carried_back(host, gi);
        }
        true
    }
}

/// A physical-machine window: snapshots are for stale-click detection only.
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct ProviderMenuOwner {host:u64,part:Option<[i32;4]>}

/// PatternProviderReturnInventory and ConfigMenuInventory expose one shared
/// nine-slot store for items and fluids. These values are authoritative; the
/// fluid display stack below is only an icon and is never given to a player.
#[derive(Clone, Debug, PartialEq)]
enum ProviderReturnSlot {
    Item(ItemStack),
    Fluid(ae2_cell_core::ItemKey, u64),
}
/// A queued quick-move may arrive before the client receives the previous
/// click's update. It is safe only if the exact physical source slot is still
/// the one displayed at that revision, including item/fluid components.
fn provider_return_revision_matches(
    revisions:&std::collections::VecDeque<(i32,Vec<Option<ProviderReturnSlot>>)>,
    state:i32,index:usize,current:&[Option<ProviderReturnSlot>],
) -> bool {
    index<9 && current.len()==9 && revisions.iter().rev().find(|(revision,_)|*revision==state)
        .is_some_and(|(_,slots)|slots.len()==9 && slots[index]==current[index])
}
fn provider_return_slots(machine:&horizon_client::ae2_world::Machine) -> Result<Vec<Option<ProviderReturnSlot>>,()> {
    if machine.kind != horizon_client::ae2_world::MachineKind::PatternProvider { return Err(()); }
    let mut slots = vec![None;9];
    for (key, fluid) in [("ae2_provider_return_items",false),("ae2_provider_return_fluids",true)] {
        let Some(tag) = machine.config.get(&key.into()) else { continue; };
        let Tag::List(entries) = tag else { return Err(()); };
        for entry in entries {
            let Some(Tag::Byte(slot)) = entry.get("Slot") else { return Err(()); };
            let index = *slot as usize;
            if index >= 9 || slots[index].is_some() { return Err(()); }
            let value = if fluid {
                let (Some(Tag::String(id)),Some(Tag::ByteArray(components)),Some(Tag::Long(amount))) =
                    (entry.get("fluid"),entry.get("components"),entry.get("amount")) else { return Err(()); };
                let id=id.to_string().map_err(|_|())?;
                if *amount <= 0 || *amount > 4000 || !super::ae2_fluids::valid_fluid_id(&id) { return Err(()); }
                ProviderReturnSlot::Fluid(ae2_cell_core::ItemKey {item_id:id,components:components.clone()}, *amount as u64)
            } else {
                if !matches!(entry.get("count"),Some(Tag::Int(n)) if *n>0) { return Err(()); }
                ProviderReturnSlot::Item(horizon_client::save::item_from_nbt(entry).ok_or(())?)
            };
            slots[index] = Some(value);
        }
    }
    Ok(slots)
}
fn provider_return_store(machine:&mut horizon_client::ae2_world::Machine, slots:&[Option<ProviderReturnSlot>]) -> Result<(),()> {
    if machine.kind != horizon_client::ae2_world::MachineKind::PatternProvider || slots.len()!=9 { return Err(()); }
    if slots.iter().flatten().any(|slot|match slot {
        ProviderReturnSlot::Item(item)=>item.count==0,
        ProviderReturnSlot::Fluid(key,amount)=>*amount==0 || *amount>4000 || !super::ae2_fluids::valid_fluid_id(&key.item_id),
    }) { return Err(()); }
    let items:Vec<_> = slots.iter().map(|s|match s {Some(ProviderReturnSlot::Item(item))=>Some(item.clone()),_=>None}).collect();
    let fluids = slots.iter().enumerate().filter_map(|(i,slot)| match slot {
        Some(ProviderReturnSlot::Fluid(key,amount)) => Some(Tag::compound([
            ("Slot",Tag::Byte(i as i8)), ("fluid",Tag::String(key.item_id.as_str().into())),
            ("components",Tag::ByteArray(key.components.clone())), ("amount",Tag::Long(*amount as i64)),
        ])), _ => None,
    }).collect();
    machine.config.insert("ae2_provider_return_items".into(),horizon_client::save::encode_items(&items));
    machine.config.insert("ae2_provider_return_fluids".into(),Tag::List(fluids));
    Ok(())
}
fn provider_return_icon(slot:&ProviderReturnSlot) -> ItemStack {
    match slot {
        ProviderReturnSlot::Item(item) => item.clone(),
        ProviderReturnSlot::Fluid(key,amount) => {
            let mut icon = ItemStack::new("ae2:wrapped_generic_stack",1);
            let data=icon.data_mut();
            data.custom_name=Some(format!("{}: {} mB",key.item_id,amount));
            // Native item packets carry vanilla custom_data, so include a
            // render-only GenericStack envelope there. The real fluid remains
            // in the provider's return inventory and is never this icon.
            data.set_other("minecraft:custom_data",Some(Tag::compound([("ae2:wrapped_stack",Tag::compound([
                ("#t",Tag::String("ae2:f".into())),("id",Tag::String(key.item_id.as_str().into())),
                ("#",Tag::Long(*amount as i64)),("native_components",Tag::ByteArray(key.components.clone())),
            ]))])));
            icon
        }
    }
}
/// The in-frame player owns a real Provider block or part, not a copied menu
/// inventory. The incarnation also rejects a replacement at the same position.
#[derive(Clone, Debug)]
pub(super) struct LocalProviderSession {
    pos: (i32, i32, i32),
    part: Option<super::ae2_parts::PartAddress>,
    owner: ProviderMenuOwner,
    dim: String,
    priority: Option<horizon_client::ui::EditBox>,
}
#[derive(Clone, Debug)]
pub(super) struct LocalTerminalSession {
    pos:(i32,i32,i32),
    host:u64,
    part:Option<super::ae2_parts::PartAddress>,
    part_identity:Option<(u64,[i32;4])>,
    dim:String,
    crafting:bool,
    encoding:bool,
    encoding_open:bool,
    encoding_page:usize,
    encoding_fluid_mode:bool,
    encoding_fluid_amount:u64,
    encoding_selected_fluid:usize,
    search:horizon_client::ui::EditBox,
    sort:u8,
    view:u8,
    descending:bool,
    search_mod_name:bool,
    cache:std::cell::RefCell<Option<(u64,Vec<(ae2_cell_core::ItemKey,u64)>)>>,
}
#[derive(Clone,Copy,Debug)]
enum PriorityTarget {
    Chest { pos:(i32,i32,i32), identity:u64 },
    Drive { pos:(i32,i32,i32), identity:u64 },
    Provider { pos:(i32,i32,i32), part:Option<super::ae2_parts::PartAddress>, identity:ProviderMenuOwner },
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct WorkbenchRevision { state:i32, page:usize, keep:bool }
fn workbench_revision_matches(history:&std::collections::VecDeque<WorkbenchRevision>,state:i32,page:usize,keep:bool)->bool {
    history.iter().rev().find(|revision|revision.state==state).is_some_and(|revision|revision.page==page && revision.keep==keep)
}
fn ae2_link_slot_click(dim:&str,pos:(i32,i32,i32),slot:usize,button:i32,input:Input,
    carried:&mut Option<ItemStack>,link_input:&mut Option<ItemStack>,link_output:&mut Option<ItemStack>) {
    if input!=Input::Pickup || !(0..=1).contains(&button) || !matches!(slot,1|2) {return;}
    if slot==1 {
        if carried.is_none() {*carried=link_input.take();}
        else if link_input.is_none() && carried.as_ref().is_some_and(|item|item.count==1 && matches!(item.item.as_ref(),"ae2:wireless_terminal"|"ae2:wireless_crafting_terminal")) {
            *link_input=carried.take();
        }
    } else if carried.is_none() {*carried=link_output.take();}
    if link_output.is_none() {
        if let Some(mut stack)=link_input.take() {
            if super::ae2_wireless::link(&mut stack,dim,pos).is_ok() {*link_output=Some(stack);} else {*link_input=Some(stack);}
        }
    }
}
fn workbench_copy_kept(machine:&horizon_client::ae2_world::Machine)->bool {
    matches!(machine.config.get(&"ae2_workbench_copy_mode".into()),Some(Tag::String(value)) if value.to_string().ok().as_deref()==Some("keep_on_remove"))
}
#[derive(Clone,Debug)]
enum LocalMachineEdit {
    Priority { target:PriorityTarget, field:horizon_client::ui::EditBox },
    Stock { edit:StockEdit, field:horizon_client::ui::EditBox },
}
/// In-frame menus use the same physical owner and click logic as GuestMachine.
#[derive(Clone,Debug)]
pub(super) struct LocalMachineSession {
    pos:(i32,i32,i32), part:Option<super::ae2_parts::PartAddress>, owner:MachineMenuOwner,
    dim:String, page:usize, edit:Option<LocalMachineEdit>,
    link_input:Option<ItemStack>, link_output:Option<ItemStack>,
}
pub(super) struct GuestMachine {
    window: i32,
    state: i32,
    pos: (i32, i32, i32),
    part: Option<super::ae2_parts::PartAddress>,
    snapshot: Vec<Option<ItemStack>>,
    provider_return_revisions: std::collections::VecDeque<(i32,Vec<Option<ProviderReturnSlot>>)>,
    workbench_page: usize,
    owner: MachineMenuOwner,
    workbench_revisions: std::collections::VecDeque<WorkbenchRevision>,
    owner_dim: String,
    provider_owner: Option<ProviderMenuOwner>,
    drive_owner: Option<u64>,
    priority_edit: Option<PriorityEdit>,
    stock_edit: Option<StockEdit>,
    link_input: Option<ItemStack>,
    link_output: Option<ItemStack>,
}
impl Game<'_> {
    /// A provider window belongs to the original physical block entity and
    /// mounted face, including when a replacement has identical slot values.
    fn ae2_provider_menu_owner(&self,pos:(i32,i32,i32),part:Option<super::ae2_parts::PartAddress>)->Result<Option<ProviderMenuOwner>,()> {
        use horizon_client::{ae2_world::MachineKind,containers::BlockEntity};
        let machine=if let Some(part)=part {self.ae2_part_machine(part).map_err(|_|())?} else {
            match self.world.block_entities.get(&pos) {Some(BlockEntity::Ae2(machine))=>Some((**machine).clone()),_=>None}
        }.ok_or(())?;
        if machine.kind!=MachineKind::PatternProvider {return Ok(None)}
        let host=self.world.block_entities.incarnation(&pos).ok_or(())?;
        let identity=if let Some(part)=part {
            let Some(BlockEntity::Ae2(machine))=self.world.block_entities.get(&pos) else{return Err(())};
            let Some(Tag::List(parts))=machine.config.get(&"parts".into()) else{return Err(())};
            let face=["east","west","up","down","south","north"].get(usize::from(part.face)).ok_or(())?;
            let mut records=parts.iter().filter(|record|record.get("face")==Some(&Tag::String((*face).into())));
            let record=records.next().ok_or(())?;if records.next().is_some(){return Err(())}
            Some(super::ae2_channels::part_identity(record).ok_or(())?)
        }else{None};
        Ok(Some(ProviderMenuOwner{host,part:identity}))
    }
    fn ae2_provider_window_valid(&self,pos:(i32,i32,i32),part:Option<super::ae2_parts::PartAddress>,original:Option<ProviderMenuOwner>)->bool {
        original.is_none() || self.ae2_provider_menu_owner(pos,part).ok().flatten()==original
    }
    fn ae2_drive_window_valid(&self,pos:(i32,i32,i32),original:Option<u64>)->bool {
        original.is_none() || (self.world.block_entities.incarnation(&pos)==original
            && matches!(self.world.block_entities.get(&pos),Some(horizon_client::containers::BlockEntity::Ae2(m)) if m.kind==horizon_client::ae2_world::MachineKind::Drive))
    }
    /// PatternProviderBlock.useWithoutItem and PatternProviderPart's equivalent
    /// open the same menu, addressed to the actual block or the clicked face.
    pub(super) fn ae2_open_local_provider(&mut self, pos:(i32,i32,i32), face:Option<[i32;3]>) -> bool {
        use horizon_client::{ae2_world::MachineKind, containers::BlockEntity};
        if self.remote.is_some() { return false; }
        let part = if matches!(self.world.block_entities.get(&pos), Some(BlockEntity::Ae2(m)) if m.kind == MachineKind::PatternProvider) {
            None
        } else {
            let Some(face) = face.and_then(|face| match face {
                [1,0,0] => Some(0), [-1,0,0] => Some(1), [0,1,0] => Some(2),
                [0,-1,0] => Some(3), [0,0,1] => Some(4), [0,0,-1] => Some(5), _ => None,
            }) else { return false; };
            let Some(host) = self.ae2_local_address(pos) else { return false; };
            Some(super::ae2_parts::PartAddress { host, face })
        };
        let Ok(Some(owner)) = self.ae2_provider_menu_owner(pos, part) else { return false; };
        self.container = Some(pos);
        self.ae2_local_provider = Some(LocalProviderSession {
            pos, part, owner, dim: self.dim.id().to_owned(), priority: None,
        });
        self.screen = super::Screen::Inventory;
        self.capture(false);
        self.right = false;
        true
    }
    pub(super) fn ae2_local_provider_open(&self) -> bool {
        self.remote.is_none() && self.ae2_local_provider.as_ref()
            .is_some_and(|session| self.container == Some(session.pos))
    }
    pub(super) fn ae2_local_provider_valid(&self) -> bool {
        self.ae2_local_provider_open() && self.ae2_local_provider.as_ref().is_some_and(|session|
            session.dim == self.dim.id()
                && self.ae2_provider_window_valid(session.pos, session.part, Some(session.owner)))
    }
    pub(super) fn ae2_local_provider_priority_open(&self) -> bool {
        self.ae2_local_provider_open() && self.ae2_local_provider.as_ref().is_some_and(|s| s.priority.is_some())
    }
    pub(super) fn ae2_local_priority_focused(&self) -> bool {
        self.ae2_local_provider_priority_open() && self.ae2_local_provider.as_ref()
            .and_then(|s| s.priority.as_ref()).is_some_and(|field| field.focused)
    }
    fn ae2_local_priority_apply(&mut self) {
        if !self.ae2_local_provider_valid() { return; }
        let Some(session) = self.ae2_local_provider.clone() else { return; };
        let Some(text) = session.priority.as_ref().map(|field| field.value.as_str()) else { return; };
        let Some(value) = super::ae2_number_entry::number_value(text,1,i64::from(i32::MIN),i64::MAX,'.')
            .and_then(|n| i32::try_from(n).ok()) else { return; };
        let target = PriorityTarget::Provider {pos:session.pos, part:session.part, identity:session.owner};
        if let Some(old) = self.priority_target_read(target) { self.priority_target_set(target,old,value); }
    }
    pub(super) fn ae2_local_priority_input(&mut self, text:Option<&str>, key:Option<winit::keyboard::KeyCode>) {
        if !self.ae2_local_provider_valid() { return; }
        if key == Some(winit::keyboard::KeyCode::Enter) {
            let valid = self.ae2_local_provider.as_ref().and_then(|s| s.priority.as_ref())
                .is_some_and(|field| priority_confirmable(&field.value));
            if valid {
                self.ae2_local_priority_apply();
                if let Some(session) = self.ae2_local_provider.as_mut() { session.priority = None; }
            }
            return;
        }
        let ctrl = self.has_control_down();
        let shift = self.has_shift_down();
        if let Some(field) = self.ae2_local_provider.as_mut().and_then(|s| s.priority.as_mut()) {
            if !key.is_some_and(|key| field.key(key,ctrl,shift)) {
                if let Some(text) = text { field.insert_text(text); }
            }
        }
        self.ae2_local_priority_apply();
    }
    pub(super) fn ae2_local_priority_shortcut(&mut self, shortcut:horizon_client::clipboard::EditShortcut) {
        if let Some(field) = self.ae2_local_provider.as_mut().and_then(|s| s.priority.as_mut()) {
            field.shortcut(shortcut, &mut self.clipboard);
        }
        self.ae2_local_priority_apply();
    }
    pub(super) fn ae2_local_priority_click_field(&mut self, ox:f32, oy:f32, right:bool) -> bool {
        let (mx,my) = self.cursor;
        let hit = !right && mx >= ox+48.0 && mx < ox+114.0 && my >= oy+55.0 && my < oy+67.0;
        let shift = self.has_shift_down();
        let Some(field) = self.ae2_local_provider.as_mut().and_then(|s| s.priority.as_mut()) else { return false; };
        field.focused = hit;
        if hit {
            field.x=ox+48.0; field.y=oy+55.0; field.width=66.0; field.height=12.0;
            field.click(&self.gui.atlas,mx,shift,false);
        }
        hit
    }
    pub(super) fn ae2_local_priority_draw(&self, batch:&mut horizon_client::hud::HudBatch, ox:f32, oy:f32) {
        if let Some(mut field) = self.ae2_local_provider.as_ref().and_then(|s| s.priority.clone()) {
            field.x=ox+48.0; field.y=oy+55.0; field.width=66.0; field.height=12.0;
            field.draw(batch,&self.gui.atlas,None);
        }
    }
    fn local_provider_machine(&self, session:&LocalProviderSession) -> Option<horizon_client::ae2_world::Machine> {
        self.provider_menu_machine(session.pos, session.part)
    }
    fn provider_menu_machine(&self, pos:(i32,i32,i32), part:Option<super::ae2_parts::PartAddress>) -> Option<horizon_client::ae2_world::Machine> {
        use horizon_client::containers::BlockEntity;
        let machine = if let Some(part) = part { self.ae2_part_machine(part).ok().flatten() } else {
            match self.world.block_entities.get(&pos) {
                Some(BlockEntity::Ae2(machine)) => Some((**machine).clone()), _ => None,
            }
        }?;
        (machine.kind == horizon_client::ae2_world::MachineKind::PatternProvider).then_some(machine)
    }
    fn provider_menu_store(&mut self,pos:(i32,i32,i32),part:Option<super::ae2_parts::PartAddress>,machine:&horizon_client::ae2_world::Machine) -> bool {
        use horizon_client::{ae2_world::MachineKind,containers::BlockEntity};
        if let Some(part) = part { return self.ae2_store_part_machine(part,machine).is_ok(); }
        let Some(BlockEntity::Ae2(current)) = self.world.block_entities.get_mut(&pos) else { return false; };
        if current.kind != MachineKind::PatternProvider { return false; }
        **current=machine.clone();self.world.modified.insert((pos.0>>4,pos.2>>4));true
    }
    fn provider_return_item_limit(&self,stack:&ItemStack) -> u32 {
        self.res.items().stack_max(stack)
    }
    /// A menu slot is allowed to change the return inventory directly even
    /// though the external capability is insertion-only in the source.
    fn ae2_provider_return_click(&mut self,pos:(i32,i32,i32),part:Option<super::ae2_parts::PartAddress>,index:usize,button:i32,input:Input) {
        if index>=9 { return; }
        let Some(mut machine)=self.provider_menu_machine(pos,part) else { return; };
        let Ok(mut slots)=provider_return_slots(&machine) else { return; };
        let before_slots=slots.clone();
        let before_inventory=self.inventory.clone();let before_carried=self.carried.clone();let before_offhand=self.offhand.clone();
        if input == Input::QuickMove {
            if let Some(ProviderReturnSlot::Item(item))=slots[index].take() {
                if item.count<=self.provider_return_item_limit(&item) {
                    slots[index]=self.give(item).map(ProviderReturnSlot::Item);
                } else { slots[index]=Some(ProviderReturnSlot::Item(item)); }
            }
        } else if input == Input::Swap && ((0..=8).contains(&button) || button==40) {
            let source=if button==40 {self.offhand.clone()}else{self.inventory[button as usize].clone()};
            let target=match &slots[index] {None=>None,Some(ProviderReturnSlot::Item(item)) if item.count<=self.provider_return_item_limit(item)=>Some(item.clone()),_=>return};
            if source.as_ref().is_some_and(|item|item.count==0 || item.count>self.provider_return_item_limit(item) || item.item.as_ref()=="ae2:wrapped_generic_stack") {return;}
            slots[index]=source.map(ProviderReturnSlot::Item);
            if button==40 {self.offhand=target;}else{self.inventory[button as usize]=target;}
        } else if input == Input::Pickup && (0..=1).contains(&button) {
            if let Some(ProviderReturnSlot::Fluid(key,amount))=&slots[index] {
                // WrappedGenericStack.overrideOtherStackedOnMe: a plain bucket
                // exchanges one complete, component-free bucket on primary use.
                if button!=0 { return; }
                let Some(cursor)=self.carried.as_ref().filter(|s|s.count==1 && super::ae2_fluids::plain_bucket(s)) else { return; };
                if cursor.item.as_ref()=="minecraft:bucket" && *amount>=1000 && *key==super::ae2_fluids::fluid_key(&key.item_id) {
                    let Some(bucket)=super::ae2_fluids::fluid_bucket(&key.item_id) else {return;};
                    let remain=*amount-1000;
                    slots[index]=(remain>0).then(||ProviderReturnSlot::Fluid(key.clone(),remain));
                    self.carried=Some(ItemStack::new(bucket,1));
                } else if super::ae2_fluids::bucket_fluid(&cursor.item).is_some_and(|id|*key==super::ae2_fluids::fluid_key(id)) && *amount<=3000 {
                    slots[index]=Some(ProviderReturnSlot::Fluid(key.clone(),*amount+1000));
                    self.carried=Some(ItemStack::new("minecraft:bucket",1));
                }
            } else {
                let current=match slots[index].take() {Some(ProviderReturnSlot::Item(item))=>Some(item),None=>None,_=>unreachable!()};
                let held=self.carried.take();
                if current.as_ref().is_some_and(|item|item.count>self.provider_return_item_limit(item)) || held.as_ref().is_some_and(|item|item.item.as_ref()=="ae2:wrapped_generic_stack") {
                    slots[index]=current.map(ProviderReturnSlot::Item);self.carried=held;return;
                }
                let (next,cursor)=match (held,current,button) {
                    (None,None,_) => (None,None),
                    (None,Some(item),0) => (None,Some(item)),
                    (None,Some(mut item),1) => {let n=item.count.div_ceil(2);item.count-=n;( (item.count>0).then_some(item.clone()),Some(ItemStack{count:n,..item}) )},
                    (Some(mut held),None,b) => {let n=self.provider_return_item_limit(&held).min(if b==1 {1}else{held.count});held.count-=n;(Some(ItemStack{count:n,..held.clone()}),(held.count>0).then_some(held))},
                    (Some(mut held),Some(mut item),b) if same(&held,&item) => {let n=self.provider_return_item_limit(&item).saturating_sub(item.count).min(if b==1 {1}else{held.count});item.count+=n;held.count-=n;(Some(item),(held.count>0).then_some(held))},
                    (Some(held),Some(item),_) if held.count<=self.provider_return_item_limit(&held) => (Some(held),Some(item)),
                    (held,item,_) => (item,held),
                };
                slots[index]=next.map(ProviderReturnSlot::Item);self.carried=cursor;
            }
        }
        if slots==before_slots { return; }
        if provider_return_store(&mut machine,&slots).is_err() || !self.provider_menu_store(pos,part,&machine) {
            self.inventory=before_inventory;self.carried=before_carried;self.offhand=before_offhand;
        }
    }
    /// AEBaseMenu.quickMoveStack tries encoded-pattern slots first, then the
    /// storage slots; preserve the player's remainder if a commit fails.
    fn ae2_provider_return_quick_insert(&mut self,pos:(i32,i32,i32),part:Option<super::ae2_parts::PartAddress>,player_slot:usize) {
        let Some(mut source)=self.inventory.get(player_slot).cloned().flatten() else{return;};
        if source.item.as_ref()=="ae2:wrapped_generic_stack" {return;}
        let Some(mut machine)=self.provider_menu_machine(pos,part) else{return;};
        let Ok(mut slots)=provider_return_slots(&machine) else{return;};
        let before=source.count;
        let max=self.res.items().stack_max(&source);
        for slot in &mut slots {
            if let Some(ProviderReturnSlot::Item(item))=slot {
                if same(item,&source) {let n=max.saturating_sub(item.count).min(source.count);item.count+=n;source.count-=n;}
            }
        }
        for slot in &mut slots {
            if source.count==0 {break;}
            if slot.is_none() {let n=max.min(source.count);*slot=Some(ProviderReturnSlot::Item(ItemStack{count:n,..source.clone()}));source.count-=n;}
        }
        if source.count==before {return;}
        if provider_return_store(&mut machine,&slots).is_ok() && self.provider_menu_store(pos,part,&machine) {
            self.inventory[player_slot]=(source.count>0).then_some(source);
        }
    }
    pub(super) fn ae2_local_provider_slot_view(&self, slot:usize) -> Option<ItemStack> {
        if !self.ae2_local_provider_valid() { return None; }
        let session = self.ae2_local_provider.as_ref()?;
        let index = slot.checked_sub(300)?;
        if session.priority.is_some() {
            return match index {
                0..=3 => Some(navigation(&format!("Priority +{}; shift uses 1/16/32/64", [1,10,100,1000][index]))),
                4..=7 => Some(navigation(&format!("Priority -{}; shift uses 1/16/32/64", [1,10,100,1000][index-4]))),
                8 => Some(navigation("Back to provider")),
                9 => self.priority_target_read(PriorityTarget::Provider { pos:session.pos, part:session.part, identity:session.owner })
                    .map(|n| navigation(&format!("Priority: {n}"))),
                _ => None,
            };
        }
        let machine = self.local_provider_machine(session)?;
        match index {
            0..=8 => machine.slots.get(index).cloned().flatten(),
            9..=17 => provider_return_slots(&machine).ok()?.get(index-9)?.as_ref().map(provider_return_icon),
            18 => Some(navigation(if machine.config.get(&"ae2_blocking".into()) == Some(&Tag::Byte(1)) { "Blocking mode: on" } else { "Blocking mode: off" })),
            19 => Some(navigation(match machine.config.get(&"ae2_lock_mode".into()) {
                Some(Tag::Int(1)) => "Craft lock: until redstone pulse",
                Some(Tag::Int(2)) => "Craft lock: while redstone high",
                Some(Tag::Int(3)) => "Craft lock: while redstone low",
                Some(Tag::Int(4)) => "Craft lock: until result", _ => "Craft lock: none",
            })),
            20 => Some(navigation(if machine.config.get(&"terminal_visible".into()) != Some(&Tag::Byte(0)) { "Pattern Access: visible" } else { "Pattern Access: hidden" })),
            26 => Some(navigation("Priority: click to edit")),
            _ => None,
        }
    }
    /// Keep the machine and player inventory authoritative for each gesture.
    /// Unsupported gestures are consumed, never handed to the generic menu.
    pub(super) fn ae2_local_provider_click(&mut self, slot:Option<usize>, button:i32, input:Input) {
        use horizon_client::containers::BlockEntity;
        if !self.ae2_local_provider_valid() {
            self.close_container();
            self.screen = super::Screen::Playing;
            self.capture(true);
            return;
        }
        let Some(session) = self.ae2_local_provider.clone() else { return; };
        if session.priority.is_some() {
            match slot {
                Some(308) if input == Input::Pickup => {
                    if let Some(current) = self.ae2_local_provider.as_mut() { current.priority = None; }
                }
                Some(index @ 300..=307) if matches!(input, Input::Pickup | Input::QuickMove) => {
                    let target = PriorityTarget::Provider { pos:session.pos, part:session.part, identity:session.owner };
                    if let Some(old) = self.priority_target_read(target) {
                        let text = session.priority.as_ref().map(|field| field.value.as_str()).unwrap_or("");
                        if let Some(next) = priority_step(text, (index - 300) as i16, input == Input::QuickMove)
                            .and_then(|text| super::ae2_number_entry::number_value(&text,1,i64::from(i32::MIN),i64::MAX,'.'))
                            .and_then(|n| i32::try_from(n).ok()) {
                            self.priority_target_set(target, old, next);
                            if let Some(field) = self.ae2_local_provider.as_mut().and_then(|s| s.priority.as_mut()) {
                                field.set_value(&next.to_string()); field.focused = true;
                            }
                        }
                    }
                }
                _ => {}
            }
            return;
        }
        if slot == Some(326) && input == Input::Pickup && (0..=1).contains(&button) {
            let target = PriorityTarget::Provider {pos:session.pos,part:session.part,identity:session.owner};
            if let Some(value) = self.priority_target_read(target) {
                let mut field = horizon_client::ui::EditBox::new(0.0,0.0,66.0,12.0);
                field.max_length=16; field.set_value(&value.to_string()); field.highlight=0; field.focused=true;
                if let Some(current) = self.ae2_local_provider.as_mut() { current.priority = Some(field); }
            }
            return;
        }
        if let Some(index @ 318..=320) = slot.filter(|_| input == Input::Pickup && (0..=1).contains(&button)) {
            let Some(mut machine) = self.local_provider_machine(&session) else { return; };
            match index {
                318 => {
                    let old = machine.config.get(&"ae2_blocking".into()) == Some(&Tag::Byte(1));
                    machine.config.insert("ae2_blocking".into(), Tag::Byte(i8::from(!old)));
                }
                319 => {
                    let old = match machine.config.get(&"ae2_lock_mode".into()) { Some(Tag::Int(n)) => *n, _ => 0 };
                    machine.config.insert("ae2_lock_mode".into(), Tag::Int((old + 1).rem_euclid(5)));
                    machine.config.remove(&"ae2_unlock_event".into());
                    machine.config.remove(&"ae2_unlock_stack".into());
                }
                320 => {
                    let visible = machine.config.get(&"terminal_visible".into()) != Some(&Tag::Byte(0));
                    machine.config.insert("terminal_visible".into(), Tag::Byte(i8::from(!visible)));
                }
                _ => unreachable!(),
            }
            if let Some(part) = session.part { let _ = self.ae2_store_part_machine(part, &machine); }
            else if let Some(BlockEntity::Ae2(target)) = self.world.block_entities.get_mut(&session.pos) {
                **target = machine;
                self.world.modified.insert((session.pos.0 >> 4, session.pos.2 >> 4));
            }
            return;
        }
        if let Some(index @ 309..=317) = slot {
            self.ae2_provider_return_click(session.pos, session.part, index-309, button, input);
            return;
        }
        let before = self.local_provider_machine(&session).map(|m| m.slots);
        let setter = if input == Input::Swap && slot.is_some_and(|slot|(300..309).contains(&slot)) {
            self.ae2_provider_slot_swap(session.pos, session.part, slot, button)
        } else {
            if let Some(part) = session.part { self.ae2_part_machine_click(part, slot, button, input); }
            else { self.ae2_machine_click(session.pos, slot, button, input); }
            false
        };
        if input == Input::QuickMove {
            if let Some(index @ 0..=35) = slot { self.ae2_provider_return_quick_insert(session.pos,session.part,index); }
        }
        let after = self.local_provider_machine(&session).map(|m| m.slots);
        if before.is_some() && (before != after || setter) {
            self.ae2_provider_patterns_changed_local(session.pos, session.part.map(|p| p.face));
        }
    }
    fn priority_target_read(&self,target:PriorityTarget)->Option<i32> {
        use horizon_client::containers::BlockEntity;
        match target {
            PriorityTarget::Chest{pos,identity} => {
                if self.chest_terminal_read(pos)?.0!=identity {return None;}
                self.ae2_storage_priority(self.ae2_local_address(pos)?).ok()
            },
            PriorityTarget::Drive{pos,identity} => {
                if !self.ae2_drive_window_valid(pos,Some(identity)){return None;}
                self.ae2_storage_priority(self.ae2_local_address(pos)?).ok()
            },
            PriorityTarget::Provider{pos,part,identity} => {
                if !self.ae2_provider_window_valid(pos,part,Some(identity)){return None;}
                let machine=if let Some(part)=part {self.ae2_part_machine(part).ok().flatten()?} else {
                    let BlockEntity::Ae2(machine)=self.world.block_entities.get(&pos)? else{return None};(**machine).clone()
                };
                match machine.config.get(&"priority".into()){None=>Some(0),Some(Tag::Int(value))=>Some(*value),_=>None}
            }
        }
    }
    fn priority_target_set(&mut self,target:PriorityTarget,old:i32,value:i32)->bool {
        if self.priority_target_read(target)!=Some(old){return false;}
        if old==value{return true;}
        use horizon_client::containers::BlockEntity;
        match target {
            PriorityTarget::Chest{pos,..}|PriorityTarget::Drive{pos,..} => self.ae2_set_storage_priority_local(pos,value).is_ok(),
            PriorityTarget::Provider{pos,part,identity:_} => {
                if let Some(part)=part {
                    let Some(mut machine)=self.ae2_part_machine(part).ok().flatten() else{return false};
                    machine.config.insert("priority".into(),Tag::Int(value));
                    if self.ae2_store_part_machine(part,&machine).is_err(){return false;}
                } else {
                    let Some(BlockEntity::Ae2(machine))=self.world.block_entities.get_mut(&pos) else{return false};
                    machine.config.insert("priority".into(),Tag::Int(value));self.world.modified.insert((pos.0>>4,pos.2>>4));
                }
                self.ae2_provider_priority_setter_local(pos,part.map(|p|p.face));true
            }
        }
    }
    fn priority_session(&self,host:&super::lan::LanHost,conn:u32)->Option<(i32,PriorityTarget,PriorityEdit)> {
        let player=host.hub.guests.iter().find(|g|g.conn==conn && host.hub.here(g) && !g.spectator())?;
        let play=host.play.get(&conn)?;
        if let Some(t)=play.ae2_terminal.as_ref() {
            let edit=t.priority_edit.clone()?;let identity=t.chest_identity?;
            if t.owner_dim!=self.dim.id() || !player.within_block_range([t.pos.0,t.pos.1,t.pos.2],4.0){return None;}
            return Some((t.window,PriorityTarget::Chest{pos:t.pos,identity},edit));
        }
        let m=play.ae2_machine.as_ref()?;let edit=m.priority_edit.clone()?;
        if m.owner_dim!=self.dim.id() || !player.within_block_range([m.pos.0,m.pos.1,m.pos.2],4.0){return None;}
        let target=if let Some(identity)=m.provider_owner {PriorityTarget::Provider{pos:m.pos,part:m.part,identity}}
            else {PriorityTarget::Drive{pos:m.pos,identity:m.drive_owner?}};
        Some((m.window,target,edit))
    }
    fn priority_payload(host:&mut super::lan::LanHost,conn:u32,window:i32,token:u64,mode:&str,value:&str) {
        if let Ok(data)=serde_json::to_vec(&serde_json::json!({"window":window,"token":token,"mode":mode,"value":value})) {
            host.hub.send_to(conn,&C::CustomPayload(horizon_protocol::packets::configuration::CustomPayload{channel:"ae2:priority_editor".into(),data}));
        }
    }
    fn lan_priority_invalid(&mut self,host:&mut super::lan::LanHost,conn:u32) {
        let window=host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref().filter(|t|t.priority_edit.is_some()).map(|t|t.window)
            .or_else(||p.ae2_machine.as_ref().filter(|m|m.priority_edit.is_some()).map(|m|m.window)));
        if let Some(window)=window {
            if !self.lan_ae2_terminal_closed(host,conn,Some(window)){self.lan_ae2_machine_closed(host,conn,Some(window));}
            host.hub.send_to(conn,&C::ContainerClose(play::ContainerClose{container:window}));
        }
    }
    fn lan_priority_open(&mut self,host:&mut super::lan::LanHost,conn:u32,window:i32,target:PriorityTarget)->bool {
        let Some(value)=self.priority_target_read(target) else{
            if !self.lan_ae2_terminal_closed(host,conn,Some(window)){self.lan_ae2_machine_closed(host,conn,Some(window));}
            host.hub.send_to(conn,&C::ContainerClose(play::ContainerClose{container:window}));return true;
        };
        if let Some(gi)=host.hub.guests.iter().position(|g|g.conn==conn){self.guest_carried_back(host,gi);}
        let token=NEXT_NUMBER_EDITOR.fetch_add(1,std::sync::atomic::Ordering::Relaxed);
        let edit=PriorityEdit{value,text:value.to_string(),token};
        if let Some(t)=host.play.get_mut(&conn).and_then(|p|p.ae2_terminal.as_mut()).filter(|t|t.window==window){t.priority_edit=Some(edit.clone());}
        else if let Some(m)=host.play.get_mut(&conn).and_then(|p|p.ae2_machine.as_mut()).filter(|m|m.window==window){m.priority_edit=Some(edit.clone());m.provider_return_revisions.clear();}
        else{return true}
        Self::priority_payload(host,conn,window,edit.token,"open",&edit.text);self.lan_send_priority(host,conn);true
    }
    fn lan_send_priority(&mut self,host:&mut super::lan::LanHost,conn:u32) {
        let Some((window,target,edit))=self.priority_session(host,conn) else{self.lan_priority_invalid(host,conn);return};
        if self.priority_target_read(target).is_none(){self.lan_priority_invalid(host,conn);return;}
        let is_machine=host.play.get(&conn).and_then(|p|p.ae2_machine.as_ref()).is_some_and(|m|m.window==window);
        let Some(g)=host.hub.guests.iter().find(|g|g.conn==conn) else{return};
        let mut items=vec![None;if is_machine {27}else{54}];
        for (i,n) in [1,10,100,1000].into_iter().enumerate(){items[i]=Some(navigation(&format!("Priority +{n}; shift uses 1/16/32/64")));items[i+4]=Some(navigation(&format!("Priority -{n}; shift uses 1/16/32/64")));}
        items[8]=Some(navigation("Back to owner"));items[9]=Some(navigation(&format!("Priority: {}",edit.text)));
        items.extend((9..36).chain(0..9).map(|i|g.inventory[i].clone()));let carried=g.carried.clone();
        let snapshot=if is_machine {match target {PriorityTarget::Drive{pos,..}=>self.ae2_owned_machine_slots(pos,None),PriorityTarget::Provider{pos,part,..}=>self.ae2_owned_machine_slots(pos,part),_=>None}}else{None};
        let chest_settings=match target {PriorityTarget::Chest{pos,identity}=>self.chest_terminal_read(pos).filter(|(current,_)|*current==identity).map(|(_,settings)|settings),_=>None};
        let state=if is_machine {
            let Some(m)=host.play.get_mut(&conn).and_then(|p|p.ae2_machine.as_mut()) else{return};
            if let Some(snapshot)=snapshot {m.snapshot=snapshot;}m.state=(m.state+1)&0x7fff;m.state
        } else {
            let Some(t)=host.play.get_mut(&conn).and_then(|p|p.ae2_terminal.as_mut()) else{return};
            if let Some(settings)=chest_settings {if (t.sort_mode,t.filter_mode,t.sort_descending)!=(settings.sort,settings.view,settings.descending){t.page=0;}t.sort_mode=settings.sort;t.filter_mode=settings.view;t.sort_descending=settings.descending;}
            t.state=(t.state+1)&0x7fff;t.state
        };
        let items=items.iter().map(|item|host.hub.stack(item)).collect();let carried=host.hub.stack(&carried);
        host.hub.send_to(conn,&C::ContainerSetContent(play::ContainerSetContent{container:window,state,items,carried}));
    }
    fn priority_update(&mut self,host:&mut super::lan::LanHost,conn:u32,text:&str,echo:bool)->bool {
        let active=host.play.get(&conn).is_some_and(|p|p.ae2_terminal.as_ref().is_some_and(|t|t.priority_edit.is_some()) || p.ae2_machine.as_ref().is_some_and(|m|m.priority_edit.is_some()));
        if !active{return false;}
        let Some((window,target,mut edit))=self.priority_session(host,conn) else{self.lan_priority_invalid(host,conn);return true};
        let Some(actual)=self.priority_target_read(target) else{self.lan_priority_invalid(host,conn);return true};
        if actual!=edit.value {
            edit.value=actual;edit.text=actual.to_string();Self::priority_payload(host,conn,window,edit.token,"set",&edit.text);
        } else {
            edit.text=text.chars().take(16).collect();
            if let Some(value)=super::ae2_number_entry::number_value(&edit.text,1,i64::from(i32::MIN),i64::MAX,'.').and_then(|n|i32::try_from(n).ok()) {
                if value!=edit.value {
                    if !self.priority_target_set(target,edit.value,value){self.lan_priority_invalid(host,conn);return true;}
                    edit.value=value;
                }
            }
            if echo {Self::priority_payload(host,conn,window,edit.token,"set",&edit.text);}
        }
        if let Some(t)=host.play.get_mut(&conn).and_then(|p|p.ae2_terminal.as_mut()).filter(|t|t.window==window){t.priority_edit=Some(edit);}
        else if let Some(m)=host.play.get_mut(&conn).and_then(|p|p.ae2_machine.as_mut()).filter(|m|m.window==window){m.priority_edit=Some(edit);}
        self.lan_send_priority(host,conn);true
    }
    pub(super) fn lan_ae2_priority_text(&mut self,host:&mut super::lan::LanHost,conn:u32,value:&str)->bool {
        if host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref()).is_some_and(|t|t.processing_amount_edit.is_some()) {
            return self.lan_processing_amount_text(host,conn,value);
        }
        if host.play.get(&conn).and_then(|p|p.ae2_machine.as_ref()).is_some_and(|m|m.stock_edit.is_some()) {
            return self.lan_ae2_stock_text(host,conn,value);
        }
        if !value.starts_with(PRIORITY_WIRE_PREFIX) {
            return host.play.get(&conn).is_some_and(|p|p.ae2_terminal.as_ref().is_some_and(|t|t.priority_edit.is_some()) || p.ae2_machine.as_ref().is_some_and(|m|m.priority_edit.is_some()));
        }
        let Some((_,_,edit))=self.priority_session(host,conn) else{self.lan_priority_invalid(host,conn);return true};
        if let Some(text)=priority_transport_for(value,edit.token) {self.priority_update(host,conn,text,false);}true
    }
    fn lan_ae2_priority_click(&mut self,host:&mut super::lan::LanHost,conn:u32,p:&play::ContainerClick)->bool {
        let Some((window,_,edit))=self.priority_session(host,conn) else{self.lan_priority_invalid(host,conn);return true};
        let input=super::lan_crafting::input(p.input);
        if p.slot==8 && input==Input::Pickup {
            if let Some(t)=host.play.get_mut(&conn).and_then(|p|p.ae2_terminal.as_mut()).filter(|t|t.window==window){t.priority_edit=None;Self::priority_payload(host,conn,window,edit.token,"close","");self.lan_send_ae2_terminal(host,conn);}
            else if let Some(m)=host.play.get_mut(&conn).and_then(|p|p.ae2_machine.as_mut()).filter(|m|m.window==window){m.priority_edit=None;Self::priority_payload(host,conn,window,edit.token,"close","");self.lan_send_ae2_machine(host,conn);}
        } else if (0..8).contains(&p.slot) && matches!(input,Input::Pickup|Input::QuickMove) {
            if let Some(next)=priority_step(&edit.text,p.slot,input==Input::QuickMove){self.priority_update(host,conn,&next,true);}
        } else {self.lan_send_priority(host,conn);}
        true
    }
    fn processing_amount_payload(host:&mut super::lan::LanHost,conn:u32,window:i32,edit:&ProcessingAmountEdit,mode:&str) {
        if let Ok(data)=serde_json::to_vec(&serde_json::json!({"window":window,"token":edit.token,"mode":mode,"value":edit.text,"kind":"stock","unit":edit.unit,"max":edit.max})) {
            host.hub.send_to(conn,&C::CustomPayload(horizon_protocol::packets::configuration::CustomPayload{channel:"ae2:priority_editor".into(),data}));
        }
    }
    fn processing_amount_session(&self,host:&super::lan::LanHost,conn:u32)->Option<(i32,ProcessingAmountEdit)> {
        let guest=host.hub.guests.iter().find(|guest|guest.conn==conn && host.hub.here(guest) && !guest.spectator())?;
        let terminal=host.play.get(&conn)?.ae2_terminal.as_ref()?;let edit=terminal.processing_amount_edit.clone()?;
        if terminal.owner_dim!=self.dim.id() || !terminal.encoding || !terminal.processing
            || terminal.encoding_part!=Some(edit.part) || terminal.encoding_identity!=Some(edit.identity)
            || self.ae2_encoding_identity(edit.part)!=Some(edit.identity)
            || !guest.within_block_range([terminal.pos.0,terminal.pos.1,terminal.pos.2],4.0) {return None;}
        Some((terminal.window,edit))
    }
    fn lan_processing_amount_invalid(&mut self,host:&mut super::lan::LanHost,conn:u32) {
        if let Some(window)=host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref()).filter(|t|t.processing_amount_edit.is_some()).map(|t|t.window) {
            self.lan_ae2_terminal_closed(host,conn,Some(window));host.hub.send_to(conn,&C::ContainerClose(play::ContainerClose{container:window}));
        }
    }
    fn lan_open_processing_amount(&mut self,host:&mut super::lan::LanHost,conn:u32,window:i32,slot:i16)->bool {
        let Some(terminal)=host.play.get(&conn).and_then(|p|p.ae2_terminal.as_ref()).filter(|t|t.window==window && t.encoding && t.processing) else{return true};
        let (Some(part),Some(identity))=(terminal.encoding_part,terminal.encoding_identity) else{return true};
        let output=slot>=18;
        let index=if output {(slot-18) as usize+terminal.processing_page.min(8)*3}else{slot as usize+terminal.processing_page.min(8)*9};
        let (item,fluid)=if output {(terminal.processing_outputs.get(index),terminal.fluid_outputs.get(index))} else {(terminal.pattern_grid.get(index),terminal.fluid_inputs.get(index))};
        let (key,amount,unit,display)=match (item.and_then(Option::as_ref),fluid.and_then(Option::as_ref)) {
            (Some(item),None)=>{
                let max=self.ae2_config_item_max(item);
                let Some(display)=encoding_item_icon(item,max) else{return true};
                (ProcessingAmountKey::Item(item.clone()),u64::from(item.count),1,display)
            },
            (None,Some(fluid))=>{
                let Some(display)=encoding_fluid_icon(fluid) else{return true};
                (ProcessingAmountKey::Fluid(fluid.key.clone()),fluid.amount,1000,display)
            },
            _=>return true,
        };
        let max=999_999i64*i64::from(unit);
        let text=if unit==1 {amount.to_string()} else {
            let value=format!("{}.{:03}",amount/u64::from(unit),amount%u64::from(unit));value.trim_end_matches('0').trim_end_matches('.').to_owned()
        };
        let edit=ProcessingAmountEdit{part,identity,index,output,key,max,unit,text,token:NEXT_NUMBER_EDITOR.fetch_add(1,std::sync::atomic::Ordering::Relaxed),display};
        let Some(terminal)=host.play.get_mut(&conn).and_then(|p|p.ae2_terminal.as_mut()).filter(|t|t.window==window) else{return true};
        terminal.processing_amount_edit=Some(edit.clone());Self::processing_amount_payload(host,conn,window,&edit,"open");self.lan_send_processing_amount(host,conn);true
    }
    fn lan_send_processing_amount(&mut self,host:&mut super::lan::LanHost,conn:u32) {
        let Some((window,edit))=self.processing_amount_session(host,conn) else{self.lan_processing_amount_invalid(host,conn);return};
        let Some(guest)=host.hub.guests.iter().find(|guest|guest.conn==conn) else{return};
        let mut items=vec![None;54];
        for (i,n) in [1,10,100,1000].into_iter().enumerate(){items[i]=Some(navigation(&format!("Amount +{n}; shift uses 1/16/32/64")));items[i+4]=Some(navigation(&format!("Amount -{n}; shift uses 1/16/32/64")));}
        items[8]=Some(navigation("Set processing amount"));items[9]=Some(navigation("Back to encoding"));items[10]=Some(edit.display.clone());
        items[11]=Some(navigation(&format!("Processing amount: {}{}",edit.text,if edit.unit==1000 {" buckets"}else{""})));
        items.extend((9..36).chain(0..9).map(|i|guest.inventory[i].clone()));let carried=guest.carried.clone();
        let Some(terminal)=host.play.get_mut(&conn).and_then(|p|p.ae2_terminal.as_mut()) else{return};terminal.state=(terminal.state+1)&0x7fff;let state=terminal.state;
        let items=items.iter().map(|item|host.hub.stack(item)).collect();let carried=host.hub.stack(&carried);
        host.hub.send_to(conn,&C::ContainerSetContent(play::ContainerSetContent{container:window,state,items,carried}));
    }
    fn lan_processing_amount_update(&mut self,host:&mut super::lan::LanHost,conn:u32,text:&str,echo:bool)->bool {
        let Some((window,mut edit))=self.processing_amount_session(host,conn) else{self.lan_processing_amount_invalid(host,conn);return true};
        edit.text=text.chars().take(16).collect();
        if let Some(terminal)=host.play.get_mut(&conn).and_then(|p|p.ae2_terminal.as_mut()).filter(|t|t.window==window){terminal.processing_amount_edit=Some(edit.clone());}
        if echo {Self::processing_amount_payload(host,conn,window,&edit,"set");}self.lan_send_processing_amount(host,conn);true
    }
    fn lan_processing_amount_text(&mut self,host:&mut super::lan::LanHost,conn:u32,value:&str)->bool {
        let Some((_,edit))=self.processing_amount_session(host,conn) else{self.lan_processing_amount_invalid(host,conn);return true};
        if let Some(text)=priority_transport_for(value,edit.token){self.lan_processing_amount_update(host,conn,text,false);}true
    }
    fn lan_processing_amount_close(&mut self,host:&mut super::lan::LanHost,conn:u32,window:i32,edit:&ProcessingAmountEdit) {
        if let Some(t)=host.play.get_mut(&conn).and_then(|p|p.ae2_terminal.as_mut()).filter(|t|t.window==window){t.processing_amount_edit=None;}
        Self::processing_amount_payload(host,conn,window,edit,"close");self.lan_load_encoding_state(host,conn);self.lan_send_ae2_terminal(host,conn);
    }
    fn lan_processing_amount_click(&mut self,host:&mut super::lan::LanHost,conn:u32,p:&play::ContainerClick)->bool {
        let Some((window,edit))=self.processing_amount_session(host,conn) else{self.lan_processing_amount_invalid(host,conn);return true};
        let input=super::lan_crafting::input(p.input);
        if p.slot==9 && input==Input::Pickup {self.lan_processing_amount_close(host,conn,window,&edit);return true;}
        if p.slot==8 && input==Input::Pickup {
            let Some(amount)=super::ae2_number_entry::number_value(&edit.text,edit.unit,0,edit.max,'.') else{self.lan_send_processing_amount(host,conn);return true};
            let Some(mut machine)=self.ae2_part_machine(edit.part).ok().flatten() else{self.lan_processing_amount_invalid(host,conn);return true};
            let Ok(mut state)=super::ae2_encoding::EncodingState::load(&machine) else{self.lan_processing_amount_invalid(host,conn);return true};
            if state.processing {
                let (items,fluids)=if edit.output {(&mut state.outputs,&mut state.fluid_outputs)}else{(&mut state.inputs,&mut state.fluid_inputs)};
                if edit.index<items.len() && edit.index<fluids.len() {
                    match &edit.key {
                        ProcessingAmountKey::Item(item)=>{let mut item=item.clone();item.count=amount as u32;items[edit.index]=(amount>0).then_some(item);fluids[edit.index]=None;},
                        ProcessingAmountKey::Fluid(key)=>{items[edit.index]=None;fluids[edit.index]=(amount>0).then(||super::ae2_processing::PatternFluid{key:key.clone(),amount:amount as u64});},
                    }
                    if state.save(&mut machine).is_ok(){let _=self.ae2_store_part_machine(edit.part,&machine);}
                }
            }
            self.lan_processing_amount_close(host,conn,window,&edit);return true;
        }
        if (0..8).contains(&p.slot) && matches!(input,Input::Pickup|Input::QuickMove) {
            let steps=if input==Input::QuickMove {[1i64,16,32,64]} else {[1i64,10,100,1000]};
            let delta=if p.slot<4 {steps[p.slot as usize]}else{-steps[(p.slot as usize)%4]};
            if let Some(next)=super::ae2_number_entry::number_step(&edit.text,delta,0,edit.max/i64::from(edit.unit),'.'){self.lan_processing_amount_update(host,conn,&next,true);}
        } else {self.lan_send_processing_amount(host,conn);}true
    }
    /// Window-scoped fake-slot metadata lets the client send source actions
    /// without predicting physical movement in a vanilla generic container.
    pub(super) fn lan_ae2_fake_slots(host:&mut super::lan::LanHost,conn:u32,window:i32,slots:&[u8]) {
        let mut data=window.to_be_bytes().to_vec();data.extend_from_slice(slots);
        host.hub.send_to(conn,&C::CustomPayload(horizon_protocol::packets::configuration::CustomPayload{channel:"ae2:fake_slots".into(),data}));
    }
    /// Source repository entries are client-only slots, not physical menu
    /// slots. Publish the current mode's exact wire indices to the client.
    pub(super) fn lan_ae2_repo_slots(host:&mut super::lan::LanHost,conn:u32,window:i32,slots:&[u8]) {
        let mut data=window.to_be_bytes().to_vec();data.extend_from_slice(slots);
        host.hub.send_to(conn,&C::CustomPayload(horizon_protocol::packets::configuration::CustomPayload{channel:"ae2:repo_slots".into(),data}));
    }
    /// Tell the local renderer this generic container is an AE2 base screen.
    pub(super) fn lan_mark_ae2_base_screen(host:&mut super::lan::LanHost,conn:u32,window:i32) {
        host.hub.send_to(conn,&C::CustomPayload(horizon_protocol::packets::configuration::CustomPayload{channel:"ae2:base_screen".into(),data:window.to_be_bytes().to_vec()}));
    }
    /// Every machine menu belongs to the original block entity and, for a
    /// cable host, the original face part. Replacing a same-kind machine must
    /// close the old menu rather than bind it to the new instance.
    fn ae2_machine_menu_owner(&self,pos:(i32,i32,i32),part:Option<super::ae2_parts::PartAddress>)->Option<MachineMenuOwner> {
        use horizon_client::containers::BlockEntity;
        let machine=if let Some(part)=part {self.ae2_part_machine(part).ok().flatten()?} else {
            let BlockEntity::Ae2(machine)=self.world.block_entities.get(&pos)? else{return None};(**machine).clone()
        };
        let host=self.world.block_entities.incarnation(&pos)?;
        let identity=if let Some(part)=part {
            let BlockEntity::Ae2(host_machine)=self.world.block_entities.get(&pos)? else{return None};
            let Tag::List(parts)=host_machine.config.get(&"parts".into())? else{return None};
            let face=["east","west","up","down","south","north"].get(usize::from(part.face))?;
            let mut matches=parts.iter().filter(|record|record.get("face")==Some(&Tag::String((*face).into())));
            let record=matches.next()?;if matches.next().is_some(){return None;}
            Some(super::ae2_channels::part_identity(record)?)
        } else {None};
        Some(MachineMenuOwner{host,part:identity,kind:machine.kind})
    }
    fn ae2_interface_menu_owner(&self,pos:(i32,i32,i32),part:Option<super::ae2_parts::PartAddress>)->Option<InterfaceMenuOwner> {
        let owner=self.ae2_machine_menu_owner(pos,part)?;
        (owner.kind==horizon_client::ae2_world::MachineKind::Interface).then_some(InterfaceMenuOwner{host:owner.host,part:owner.part})
    }
    fn ae2_stock_machine(&self,pos:(i32,i32,i32),part:Option<super::ae2_parts::PartAddress>,owner:InterfaceMenuOwner)->Option<horizon_client::ae2_world::Machine> {
        use horizon_client::containers::BlockEntity;
        if self.ae2_interface_menu_owner(pos,part)!=Some(owner){return None;}
        if let Some(part)=part {self.ae2_part_machine(part).ok().flatten()} else {
            let BlockEntity::Ae2(machine)=self.world.block_entities.get(&pos)? else{return None};Some((**machine).clone())
        }
    }
    fn ae2_stock_entry(&self,machine:&horizon_client::ae2_world::Machine,index:usize)->Option<(StockKey,u64,i64,u32,ItemStack)> {
        if index>=9 {return None;}
        let item=super::ae2_interface::interface_config(machine).ok()?.get(index)?.clone();
        let fluid=super::ae2_interface::interface_fluid_config(machine).ok()?.get(index)?.clone();
        match (item,fluid) {
            (Some(mut item),None)=>{
                let amount=u64::from(item.count);let key=super::ae2_lab::key(&item).ok()?;
                self.res.items().get(&item.item)?;
                let max=i64::from(self.ae2_config_item_max(&item));
                item.count=1;Some((StockKey::Item(key),amount,max,1,item))
            },
            (None,Some((key,amount)))=>{
                let display=encoding_fluid_icon(&super::ae2_processing::PatternFluid{key:key.clone(),amount})?;
                Some((StockKey::Fluid(key),amount,super::ae2_interface::INTERFACE_FLUID_CAPACITY as i64,1000,display))
            },
            _=>None,
        }
    }
    fn stock_payload(host:&mut super::lan::LanHost,conn:u32,window:i32,edit:&StockEdit,mode:&str) {
        if let Ok(data)=serde_json::to_vec(&serde_json::json!({"window":window,"token":edit.token,"mode":mode,"value":edit.text,"kind":"stock","unit":edit.unit,"max":edit.max})) {
            host.hub.send_to(conn,&C::CustomPayload(horizon_protocol::packets::configuration::CustomPayload{channel:"ae2:priority_editor".into(),data}));
        }
    }
    fn stock_session(&self,host:&super::lan::LanHost,conn:u32)->Option<(i32,(i32,i32,i32),Option<super::ae2_parts::PartAddress>,StockEdit)> {
        let guest=host.hub.guests.iter().find(|guest|guest.conn==conn && host.hub.here(guest) && !guest.spectator())?;
        let menu=host.play.get(&conn)?.ae2_machine.as_ref()?;let edit=menu.stock_edit.clone()?;
        if menu.owner_dim!=self.dim.id() || !guest.within_block_range([menu.pos.0,menu.pos.1,menu.pos.2],4.0)
            || self.ae2_interface_menu_owner(menu.pos,menu.part)!=Some(edit.owner){return None;}
        Some((menu.window,menu.pos,menu.part,edit))
    }
    fn lan_stock_invalid(&mut self,host:&mut super::lan::LanHost,conn:u32) {
        if let Some(window)=host.play.get(&conn).and_then(|p|p.ae2_machine.as_ref()).filter(|m|m.stock_edit.is_some()).map(|m|m.window) {
            self.lan_ae2_machine_closed(host,conn,Some(window));
            host.hub.send_to(conn,&C::ContainerClose(play::ContainerClose{container:window}));
        }
    }
    fn lan_open_stock(&mut self,host:&mut super::lan::LanHost,conn:u32,window:i32,pos:(i32,i32,i32),part:Option<super::ae2_parts::PartAddress>,slot:usize)->bool {
        let Some(owner)=self.ae2_interface_menu_owner(pos,part) else{return false};
        let Some(machine)=self.ae2_stock_machine(pos,part,owner) else{return false};
        let Some((key,amount,max,unit,display))=self.ae2_stock_entry(&machine,slot) else {self.lan_send_ae2_machine(host,conn);return true};
        if let Some(gi)=host.hub.guests.iter().position(|g|g.conn==conn){self.guest_carried_back(host,gi);}
        let text=if unit==1 {amount.to_string()} else {
            let value=format!("{}.{:03}",amount/u64::from(unit),amount%u64::from(unit));
            value.trim_end_matches('0').trim_end_matches('.').to_owned()
        };
        let edit=StockEdit{slot,owner,key,max,unit,text,token:NEXT_NUMBER_EDITOR.fetch_add(1,std::sync::atomic::Ordering::Relaxed),display};
        let Some(menu)=host.play.get_mut(&conn).and_then(|p|p.ae2_machine.as_mut()).filter(|m|m.window==window) else{return true};
        menu.stock_edit=Some(edit.clone());Self::stock_payload(host,conn,window,&edit,"open");self.lan_send_stock(host,conn);true
    }
    fn lan_send_stock(&mut self,host:&mut super::lan::LanHost,conn:u32) {
        let Some((window,pos,part,edit))=self.stock_session(host,conn) else{self.lan_stock_invalid(host,conn);return};
        let Some(snapshot)=self.ae2_owned_machine_slots(pos,part) else{self.lan_stock_invalid(host,conn);return};
        let Some(g)=host.hub.guests.iter().find(|g|g.conn==conn) else{return};
        let mut items=vec![None;27];
        for (i,n) in [1,10,100,1000].into_iter().enumerate(){items[i]=Some(navigation(&format!("Stock +{n}; shift uses 1/16/32/64")));items[i+4]=Some(navigation(&format!("Stock -{n}; shift uses 1/16/32/64")));}
        items[8]=Some(navigation("Set stock amount"));items[9]=Some(navigation("Back to Interface"));items[10]=Some(edit.display.clone());
        items[11]=Some(navigation(&format!("Stock amount: {}{}",edit.text,if edit.unit==1000 {" buckets"}else{""})));
        items.extend((9..36).chain(0..9).map(|i|g.inventory[i].clone()));let carried=g.carried.clone();
        let Some(menu)=host.play.get_mut(&conn).and_then(|p|p.ae2_machine.as_mut()) else{return};
        menu.snapshot=snapshot;menu.state=(menu.state+1)&0x7fff;let state=menu.state;
        let items=items.iter().map(|item|host.hub.stack(item)).collect();let carried=host.hub.stack(&carried);
        host.hub.send_to(conn,&C::ContainerSetContent(play::ContainerSetContent{container:window,state,items,carried}));
    }
    fn lan_stock_text(&mut self,host:&mut super::lan::LanHost,conn:u32,text:&str,echo:bool)->bool {
        let Some((window,_,_,mut edit))=self.stock_session(host,conn) else{self.lan_stock_invalid(host,conn);return true};
        edit.text=text.chars().take(16).collect();
        if let Some(menu)=host.play.get_mut(&conn).and_then(|p|p.ae2_machine.as_mut()).filter(|m|m.window==window){menu.stock_edit=Some(edit.clone());}
        if echo {Self::stock_payload(host,conn,window,&edit,"set");}self.lan_send_stock(host,conn);true
    }
    fn lan_ae2_stock_text(&mut self,host:&mut super::lan::LanHost,conn:u32,value:&str)->bool {
        let Some((_,_,_,edit))=self.stock_session(host,conn) else{self.lan_stock_invalid(host,conn);return true};
        if let Some(text)=priority_transport_for(value,edit.token){self.lan_stock_text(host,conn,text,false);}true
    }
    fn lan_stock_close(&mut self,host:&mut super::lan::LanHost,conn:u32,window:i32,edit:&StockEdit) {
        if let Some(menu)=host.play.get_mut(&conn).and_then(|p|p.ae2_machine.as_mut()).filter(|m|m.window==window){menu.stock_edit=None;}
        Self::stock_payload(host,conn,window,edit,"close");self.lan_send_ae2_machine(host,conn);
    }
    /// SetStockAmountMenu.confirm rechecks both the original owner and key.
    /// Used by the server and in-frame editors after numeric validation.
    fn ae2_commit_stock_amount(&mut self,pos:(i32,i32,i32),part:Option<super::ae2_parts::PartAddress>,edit:&StockEdit,amount:i64)->bool {
        let Some(mut machine)=self.ae2_stock_machine(pos,part,edit.owner) else{return false};
        if !self.ae2_stock_entry(&machine,edit.slot).is_some_and(|(key,_,_,_,_)|key==edit.key) {return false;}
        let result=match &edit.key {
            StockKey::Item(_) => {
                let mut current=super::ae2_interface::interface_config(&machine).ok().and_then(|v|v.get(edit.slot).cloned()).flatten();
                if let Some(stack)=current.as_mut(){stack.count=amount as u32;}
                super::ae2_interface::set_interface_config(&mut machine,edit.slot,current.as_ref())
            },
            StockKey::Fluid(key)=>super::ae2_interface::set_interface_fluid_config_amount(&mut machine,edit.slot,(amount>0).then_some((key,amount as u64))),
        };
        result.is_ok() && self.ae2_store_interface_menu_machine(pos,part,&machine)
    }
    fn lan_ae2_stock_click(&mut self,host:&mut super::lan::LanHost,conn:u32,p:&play::ContainerClick)->bool {
        let Some((window,pos,part,edit))=self.stock_session(host,conn) else{self.lan_stock_invalid(host,conn);return true};
        let input=super::lan_crafting::input(p.input);
        if p.slot==9 && input==Input::Pickup {self.lan_stock_close(host,conn,window,&edit);return true;}
        if p.slot==8 && input==Input::Pickup {
            let Some(amount)=super::ae2_number_entry::number_value(&edit.text,edit.unit,0,edit.max,'.') else{self.lan_send_stock(host,conn);return true};
            if self.ae2_stock_machine(pos,part,edit.owner).is_none(){self.lan_stock_invalid(host,conn);return true;}
            self.ae2_commit_stock_amount(pos,part,&edit,amount);
            self.lan_stock_close(host,conn,window,&edit);
            let viewers:Vec<_>=host.play.iter().filter_map(|(&c,p)|p.ae2_machine.as_ref().filter(|m|m.pos==pos && m.part==part && m.owner_dim==self.dim.id() && c!=conn).map(|_|c)).collect();
            for viewer in viewers {self.lan_send_ae2_machine(host,viewer);}return true;
        }
        if (0..8).contains(&p.slot) && matches!(input,Input::Pickup|Input::QuickMove) {
            let steps=if input==Input::QuickMove {[1i64,16,32,64]} else {[1i64,10,100,1000]};
            let delta=if p.slot<4 {steps[p.slot as usize]}else{-steps[(p.slot as usize)%4]};
            if let Some(next)=super::ae2_number_entry::number_step(&edit.text,delta,0,edit.max/i64::from(edit.unit),'.'){self.lan_stock_text(host,conn,&next,true);}
        } else {self.lan_send_stock(host,conn);}true
    }
    pub(super) fn ae2_has_machine_menu(&self, pos: (i32, i32, i32)) -> bool {
        use horizon_client::{ae2_world::MachineKind as K, containers::BlockEntity};
        matches!(self.world.block_entities.get(&pos),Some(BlockEntity::Ae2(m)) if matches!(m.kind,K::Drive|K::VibrationChamber|K::Inscriber|K::Interface|K::PatternProvider|K::MolecularAssembler|K::CellWorkbench|K::QuantumLink|K::SpatialIoPort|K::IoPort|K::Condenser|K::WirelessAccessPoint))
    }
    fn ae2_machine_slots(&self, pos: (i32, i32, i32)) -> Option<Vec<Option<ItemStack>>> {
        use horizon_client::containers::BlockEntity;
        match self.world.block_entities.get(&pos) {
            Some(BlockEntity::Ae2(m)) if self.ae2_has_machine_menu(pos) => {
                let mut slots = m.slots.clone();
                slots.extend(m.upgrades.clone());
                Some(slots)
            }
            _ => None,
        }
    }
    fn ae2_owned_machine_slots(
        &self,
        pos: (i32, i32, i32),
        part: Option<super::ae2_parts::PartAddress>,
    ) -> Option<Vec<Option<ItemStack>>> {
        if part.is_none() {if let Some(slots)=self.ae2_condenser_menu(pos) {return Some(slots);}}
        let machine = if let Some(part) = part {
            self.ae2_part_machine(part).ok().flatten()?
        } else {
            match self.world.block_entities.get(&pos) {
                Some(horizon_client::containers::BlockEntity::Ae2(m))
                    if self.ae2_has_machine_menu(pos) =>
                {
                    (**m).clone()
                }
                _ => return None,
            }
        };
        let mut slots = machine.slots.clone();
        if machine.kind == horizon_client::ae2_world::MachineKind::Interface {
            for (i, entry) in super::ae2_interface::interface_fluid_stock(&machine)
                .ok()?
                .into_iter()
                .enumerate()
            {
                if let Some((key, n)) = entry {
                    if slots[i].is_some() {
                        return None;
                    }
                    let id =
                        super::ae2_fluids::fluid_bucket(&key.item_id).unwrap_or("minecraft:bucket");
                    let mut icon = ItemStack::new(id, 1);
                    icon.data_mut().custom_name = Some(format!("{}: {} mB", key.item_id, n));
                    slots[i] = Some(icon);
                }
            }
        }
        if machine.kind == horizon_client::ae2_world::MachineKind::PatternProvider {
            slots.extend(provider_return_slots(&machine).ok()?.iter().map(|slot|slot.as_ref().map(provider_return_icon)));
        }
        slots.extend(machine.upgrades);
        Some(slots)
    }
    pub(super) fn lan_open_ae2_part_machine(
        &mut self,
        host: &mut super::lan::LanHost,
        conn: u32,
        pos: (i32, i32, i32),
        part: super::ae2_parts::PartAddress,
    ) -> bool {
        let Ok(Some(machine)) = self.ae2_part_machine(part) else {
            return false;
        };
        if !matches!(
            machine.kind,
            horizon_client::ae2_world::MachineKind::Interface
                | horizon_client::ae2_world::MachineKind::PatternProvider
        ) {
            return false;
        }
        let Ok(provider_owner)=self.ae2_provider_menu_owner(pos,Some(part)) else{return false};
        let Some(owner)=self.ae2_machine_menu_owner(pos,Some(part)) else{return false};
        self.lan_ae2_portable_closed(host, conn, None);
        self.lan_ae2_bus_closed(host, conn, None);
        self.lan_ae2_network_tool_closed(host, conn, None);
        self.lan_ae2_anchor_closed(host, conn, None);
        self.lan_craft_confirm_closed(host, conn, None);
        self.lan_pattern_access_closed(host, conn, None);
        self.lan_ae2_terminal_closed(host, conn, None);
        self.lan_ae2_machine_closed(host, conn, None);
        self.lan_station_closed(host, conn, None);
        self.lan_crafting_closed(host, conn, None);
        self.guest_container_closed(host, conn);
        let title = if machine.kind == horizon_client::ae2_world::MachineKind::PatternProvider {
            "ME Pattern Provider"
        } else {
            "ME Interface"
        };
        let Some(window) = host.hub.open_window(
            conn,
            "minecraft:generic_9x3",
            horizon_protocol::text::literal(title),
        ) else {
            return true;
        };
        host.play.entry(conn).or_default().ae2_machine = Some(GuestMachine {
            window,
            state: 0,
            pos,
            part: Some(part),
            snapshot: vec![],
            provider_return_revisions: std::collections::VecDeque::new(),
            workbench_page: 0,
            owner,
            workbench_revisions: std::collections::VecDeque::new(),
            owner_dim: self.dim.id().to_owned(),
            provider_owner,
            drive_owner: None,
            priority_edit: None,
            stock_edit: None,
            link_input: None,
            link_output: None,
        });
        Self::lan_mark_ae2_base_screen(host,conn,window);
        self.lan_send_ae2_machine(host, conn);
        true
    }
    fn ae2_machine_may_place(&self, pos: (i32, i32, i32), slot: usize, item: &ItemStack) -> bool {
        use horizon_client::{ae2_world::MachineKind as K, containers::BlockEntity};
        let Some(BlockEntity::Ae2(m)) = self.world.block_entities.get(&pos) else {
            return false;
        };
        if slot >= m.slots.len() {
            return super::ae2_machine_ui::machine_upgrade_allowed(m, slot - m.slots.len(), item);
        }
        match m.kind {
            // AE2 cells are unstackable; splitting a multi-count cell carrying
            // content components would clone its inventory.
            K::Drive => super::ae2_storage_devices::is_valid_storage_cell(item),
            K::CellWorkbench => super::ae2_storage_devices::is_valid_workbench_cell(item),
            K::VibrationChamber => {
                self.res
                    .providers()
                    .burn_time(self.res.items(), item, false)
                    > 0
            }
            K::Inscriber => self.ae2_inscriber_accepts(m,slot,item),
            K::Charger => true,
            K::QuantumLink => {
                slot == 0 && item.count > 0 && super::ae2_quantum::frequency(item).is_some()
            }
            K::SpatialIoPort => {
                slot == 0
                    && item.count == 1
                    && matches!(
                        item.item.as_ref(),
                        "ae2:spatial_storage_cell_2"
                            | "ae2:spatial_storage_cell_16"
                            | "ae2:spatial_storage_cell_128"
                    )
            }
            K::IoPort => slot < 6 && super::ae2_storage_devices::is_valid_storage_cell(item),
            K::Condenser => {
                slot == 1
                    && matches!(
                        item.item.as_ref(),
                        "ae2:cell_component_1k"
                            | "ae2:cell_component_4k"
                            | "ae2:cell_component_16k"
                            | "ae2:cell_component_64k"
                            | "ae2:cell_component_256k"
                    )
            }
            K::WirelessAccessPoint => slot == 0 && item.item.as_ref() == "ae2:wireless_booster",
            K::PatternProvider => super::ae2_pattern_access::encoded_pattern_item(item),
            K::Interface => true,
            K::MolecularAssembler => match slot {
                0..=8 => self.ae2_assembler_menu_slot_enabled(m,slot) && self.ae2_assembler_menu_accepts(m,slot,item),
                9 => false,
                10 => {let mut candidate=item.clone();candidate.count=1;self.ae2_assembler_pattern_valid(&candidate)},
                _ => false,
            },
            _ => false,
        }
    }
    fn ae2_machine_limit(&self, pos: (i32, i32, i32), slot: usize, item: &ItemStack) -> u32 {
        use horizon_client::containers::BlockEntity;
        let item_max = self.res.items().stack_max(item);
        match self.world.block_entities.get(&pos) {
            Some(BlockEntity::Ae2(m)) => machine_slot_limit(m, slot, item_max),
            _ => item_max.min(64),
        }
    }
    pub(super) fn lan_open_ae2_machine(
        &mut self,
        host: &mut super::lan::LanHost,
        conn: u32,
        pos: (i32, i32, i32),
    ) -> bool {
        if !self.ae2_has_machine_menu(pos) {
            return false;
        }
        let Ok(provider_owner)=self.ae2_provider_menu_owner(pos,None) else{return false};
        let drive_owner = if matches!(self.world.block_entities.get(&pos),Some(horizon_client::containers::BlockEntity::Ae2(m)) if m.kind==horizon_client::ae2_world::MachineKind::Drive) {
            let Some(identity)=self.world.block_entities.incarnation(&pos) else{return false};Some(identity)
        } else {None};
        let Some(owner)=self.ae2_machine_menu_owner(pos,None) else{return false};
        self.lan_ae2_portable_closed(host, conn, None);
        self.lan_ae2_bus_closed(host, conn, None);
        self.lan_ae2_network_tool_closed(host, conn, None);
        self.lan_ae2_anchor_closed(host, conn, None);
        self.lan_craft_confirm_closed(host, conn, None);
        self.lan_pattern_access_closed(host, conn, None);
        self.lan_ae2_terminal_closed(host, conn, None);
        self.lan_ae2_machine_closed(host, conn, None);
        self.lan_station_closed(host, conn, None);
        self.lan_crafting_closed(host, conn, None);
        self.guest_container_closed(host, conn);
        let title = match self.world.block_entities.get(&pos) {
            Some(horizon_client::containers::BlockEntity::Ae2(m)) => match m.kind {
                horizon_client::ae2_world::MachineKind::Drive => "ME Drive (cells)",
                horizon_client::ae2_world::MachineKind::Inscriber => {
                    "Inscriber: top, bottom, input, output"
                }
                horizon_client::ae2_world::MachineKind::VibrationChamber => {
                    "Vibration Chamber: fuel"
                }
                horizon_client::ae2_world::MachineKind::PatternProvider => "ME Pattern Provider",
                horizon_client::ae2_world::MachineKind::Charger => "Charger: input",
                _ => "AE2 Machine",
            },
            _ => "AE2 Machine",
        };
        let Some(window) = host.hub.open_window(
            conn,
            "minecraft:generic_9x3",
            horizon_protocol::text::literal(title),
        ) else {
            return true;
        };
        host.play.entry(conn).or_default().ae2_machine = Some(GuestMachine {
            window,
            state: 0,
            pos,
            part: None,
            snapshot: vec![],
            provider_return_revisions: std::collections::VecDeque::new(),
            workbench_page: 0,
            owner,
            workbench_revisions: std::collections::VecDeque::new(),
            owner_dim: self.dim.id().to_owned(),
            provider_owner,
            drive_owner,
            priority_edit: None,
            stock_edit: None,
            link_input: None,
            link_output: None,
        });
        Self::lan_mark_ae2_base_screen(host,conn,window);
        self.lan_send_ae2_machine(host, conn);
        true
    }
    fn ae2_interface_menu_machine(&self,pos:(i32,i32,i32),part:Option<super::ae2_parts::PartAddress>)->Option<horizon_client::ae2_world::Machine> {
        use horizon_client::{ae2_world::MachineKind,containers::BlockEntity};
        let machine=if let Some(part)=part {self.ae2_part_machine(part).ok().flatten()?} else {
            let BlockEntity::Ae2(machine)=self.world.block_entities.get(&pos)? else{return None};(**machine).clone()
        };
        (machine.kind==MachineKind::Interface).then_some(machine)
    }
    fn ae2_store_interface_menu_machine(&mut self,pos:(i32,i32,i32),part:Option<super::ae2_parts::PartAddress>,machine:&horizon_client::ae2_world::Machine)->bool {
        if let Some(part)=part {return self.ae2_store_part_machine(part,machine).is_ok();}
        if let Some(horizon_client::containers::BlockEntity::Ae2(target))=self.world.block_entities.get_mut(&pos) {
            if target.kind!=horizon_client::ae2_world::MachineKind::Interface {return false;}
            **target=machine.clone();self.world.modified.insert((pos.0>>4,pos.2>>4));true
        }else{false}
    }
    /// Shared Interface settings, fake filters and fluid-container actions.
    /// The caller supplies the authoritative player's actual carried stack.
    fn ae2_interface_menu_click(&mut self,pos:(i32,i32,i32),part:Option<super::ae2_parts::PartAddress>,slot:usize,button:i32,input:Input)->bool {
        let Some(mut machine)=self.ae2_interface_menu_machine(pos,part) else{return false};
        if input!=Input::Pickup {return false;}
        if (19..=23).contains(&slot) && matches!(button,0|1) {
            if super::ae2_interface::interface_setting_click(&mut machine,slot,button) {self.ae2_store_interface_menu_machine(pos,part,&machine);return true;}
            return false;
        }
        if slot<9 && matches!(button,0|1) {
            let fluid=super::ae2_interface::interface_fluid_config(&machine).is_ok_and(|v|v[slot].is_some())
                || super::ae2_interface::interface_fluid_stock(&machine).is_ok_and(|v|v[slot].is_some());
            if !fluid {return false;}
            if let Some(cursor)=self.carried.clone() {
                if let Ok(Some(result))=super::ae2_interface::interface_bucket_exchange(&mut machine,slot,&cursor) {
                    if self.ae2_store_interface_menu_machine(pos,part,&machine){self.carried=Some(result);}
                }
            }
            return true;
        }
        if !(9..18).contains(&slot) || !(0..=3).contains(&button) {return false;}
        let mut cursor=self.carried.clone();
        if matches!(button,2|3) && cursor.is_none() {return true;}
        if button==2 {if let Some(item)=cursor.as_mut(){item.count=1;}}
        let configured=(||->Result<(),String>{
            let index=slot-9;
            if let Some(fluid)=encoding_emptying_action(cursor.as_ref(),button) {
                return super::ae2_interface::set_interface_fluid_config_amount(&mut machine,index,Some((&fluid.key,fluid.amount)));
            }
            let mut item=super::ae2_interface::interface_config(&machine)?[index].clone();
            let mut fluid=super::ae2_interface::interface_fluid_config(&machine)?[index].as_ref().map(|(key,amount)|super::ae2_processing::PatternFluid{key:key.clone(),amount:*amount});
            let held=super::ae2_encoding::cursor_ghost(cursor.as_ref(),&|id,fluid|fluid||self.res.items().get(id).is_some())?;
            let max_for=|item:&ItemStack|self.ae2_config_item_max(item);
            let displayed_max=item.as_ref().map_or(64,&max_for);
            let item_capacity=held.as_ref().and_then(|(value,_)|if let super::ae2_encoding::WrappedGhost::Item(item)=value {Some(max_for(item))}else{None}).or_else(||item.as_ref().map(&max_for)).unwrap_or(64);
            super::ae2_encoding::click_ghost(&mut item,&mut fluid,held,button==1,displayed_max,item_capacity,super::ae2_interface::INTERFACE_FLUID_CAPACITY);
            if let Some(fluid)=fluid {super::ae2_interface::set_interface_fluid_config_amount(&mut machine,index,Some((&fluid.key,fluid.amount)))}
            else {super::ae2_interface::set_interface_config(&mut machine,index,item.as_ref())}
        })();
        if configured.is_ok(){self.ae2_store_interface_menu_machine(pos,part,&machine);}
        true
    }
    /// Open a block machine or the Interface on the clicked cable face.
    pub(super) fn ae2_open_local_machine(&mut self,pos:(i32,i32,i32),face:Option<[i32;3]>)->bool {
        if self.remote.is_some(){return false;}
        let part=if self.ae2_has_machine_menu(pos) {None} else {
            let Some(face)=face.and_then(|face|match face {
                [1,0,0]=>Some(0),[-1,0,0]=>Some(1),[0,1,0]=>Some(2),[0,-1,0]=>Some(3),[0,0,1]=>Some(4),[0,0,-1]=>Some(5),_=>None,
            }) else{return false};
            let Some(host)=self.ae2_local_address(pos) else{return false};
            Some(super::ae2_parts::PartAddress{host,face})
        };
        let Some(owner)=self.ae2_machine_menu_owner(pos,part) else{return false};
        if owner.kind==horizon_client::ae2_world::MachineKind::PatternProvider
            || part.is_some() && owner.kind!=horizon_client::ae2_world::MachineKind::Interface {return false;}
        self.container=Some(pos);self.ae2_local_machine=Some(LocalMachineSession{
            pos,part,owner,dim:self.dim.id().to_owned(),page:0,edit:None,link_input:None,link_output:None,
        });
        self.screen=super::Screen::Inventory;self.capture(false);self.right=false;true
    }
    pub(super) fn ae2_local_machine_open(&self)->bool {
        self.remote.is_none() && self.ae2_local_machine.as_ref().is_some_and(|session|self.container==Some(session.pos))
    }
    pub(super) fn ae2_local_machine_valid(&self)->bool {
        self.ae2_local_machine_open() && self.ae2_local_machine.as_ref().is_some_and(|session|
            session.dim==self.dim.id() && self.ae2_machine_menu_owner(session.pos,session.part)==Some(session.owner)
                && self.eye_position().is_some_and(|eye|horizon_client::lan::guest::within_block_range(eye,self.reach()+4.0,[session.pos.0,session.pos.1,session.pos.2])))
    }
    pub(super) fn ae2_local_machine_close(&mut self)->Vec<ItemStack> {
        let Some(session)=self.ae2_local_machine.take() else{return Vec::new()};
        [session.link_input,session.link_output].into_iter().flatten().collect()
    }
    pub(super) fn ae2_local_machine_title(&self)->&'static str {
        use horizon_client::ae2_world::MachineKind as K;
        let Some(session)=self.ae2_local_machine.as_ref() else{return "AE2 Machine"};
        if let Some(edit)=session.edit.as_ref(){return match edit {LocalMachineEdit::Priority{..}=>"Priority",LocalMachineEdit::Stock{..}=>"Set Stock Amount"};}
        match session.owner.kind {
            K::Drive=>"ME Drive: 10 cell slots",K::Inscriber=>"Inscriber: top, bottom, input, output",
            K::VibrationChamber=>"Vibration Chamber: fuel",K::Interface if session.part.is_some()=>"ME Interface",
            _=>"AE2 Machine",
        }
    }
    pub(super) fn ae2_local_machine_editor_open(&self)->bool {
        self.ae2_local_machine_open() && self.ae2_local_machine.as_ref().is_some_and(|s|s.edit.is_some())
    }
    fn ae2_local_machine_field(&self)->Option<&horizon_client::ui::EditBox> {
        match self.ae2_local_machine.as_ref()?.edit.as_ref()? {LocalMachineEdit::Priority{field,..}|LocalMachineEdit::Stock{field,..}=>Some(field)}
    }
    fn ae2_local_machine_field_mut(&mut self)->Option<&mut horizon_client::ui::EditBox> {
        match self.ae2_local_machine.as_mut()?.edit.as_mut()? {LocalMachineEdit::Priority{field,..}|LocalMachineEdit::Stock{field,..}=>Some(field)}
    }
    pub(super) fn ae2_local_machine_editor_focused(&self)->bool {
        self.ae2_local_machine_editor_open() && self.ae2_local_machine_field().is_some_and(|field|field.focused)
    }
    fn ae2_local_machine_apply_priority(&mut self) {
        if !self.ae2_local_machine_valid(){return;}
        let Some(LocalMachineEdit::Priority{target,field})=self.ae2_local_machine.as_ref().and_then(|s|s.edit.as_ref()).cloned() else{return};
        let Some(next)=super::ae2_number_entry::number_value(&field.value,1,i64::from(i32::MIN),i64::MAX,'.').and_then(|v|i32::try_from(v).ok()) else{return};
        if let Some(old)=self.priority_target_read(target){self.priority_target_set(target,old,next);}
    }
    fn ae2_local_machine_confirm_editor(&mut self) {
        if !self.ae2_local_machine_valid(){return;}
        let Some(session)=self.ae2_local_machine.clone() else{return};let Some(edit)=session.edit else{return};
        match edit {
            LocalMachineEdit::Priority{field,..}=>if !priority_confirmable(&field.value){return},
            LocalMachineEdit::Stock{edit,field}=>{
                let Some(amount)=super::ae2_number_entry::number_value(&field.value,edit.unit,0,edit.max,'.') else{return};
                self.ae2_commit_stock_amount(session.pos,session.part,&edit,amount);
            }
        }
        if let Some(session)=self.ae2_local_machine.as_mut(){session.edit=None;}
    }
    pub(super) fn ae2_local_machine_editor_input(&mut self,text:Option<&str>,key:Option<winit::keyboard::KeyCode>) {
        if !self.ae2_local_machine_valid(){return;}
        if key==Some(winit::keyboard::KeyCode::Enter){self.ae2_local_machine_confirm_editor();return;}
        let ctrl=self.has_control_down();let shift=self.has_shift_down();
        if let Some(field)=self.ae2_local_machine_field_mut(){if !key.is_some_and(|key|field.key(key,ctrl,shift)){if let Some(text)=text{field.insert_text(text);}}}
        self.ae2_local_machine_apply_priority();
    }
    pub(super) fn ae2_local_machine_editor_shortcut(&mut self,shortcut:horizon_client::clipboard::EditShortcut) {
        if let Some(session)=self.ae2_local_machine.as_mut(){if let Some(edit)=session.edit.as_mut(){
            let field=match edit {LocalMachineEdit::Priority{field,..}|LocalMachineEdit::Stock{field,..}=>field};field.shortcut(shortcut,&mut self.clipboard);
        }}
        self.ae2_local_machine_apply_priority();
    }
    pub(super) fn ae2_local_machine_editor_click_field(&mut self,ox:f32,oy:f32,right:bool)->bool {
        let (mx,my)=self.cursor;let hit=!right && mx>=ox+48.0 && mx<ox+114.0 && my>=oy+55.0 && my<oy+67.0;let shift=self.has_shift_down();
        // Split the independent atlas and session borrows.
        let Some(session)=self.ae2_local_machine.as_mut() else{return false};
        let Some(edit)=session.edit.as_mut() else{return false};
        let field=match edit {LocalMachineEdit::Priority{field,..}|LocalMachineEdit::Stock{field,..}=>field};field.focused=hit;
        if hit {field.x=ox+48.0;field.y=oy+55.0;field.width=66.0;field.height=12.0;field.click(&self.gui.atlas,mx,shift,false);}hit
    }
    pub(super) fn ae2_local_machine_editor_draw(&self,batch:&mut horizon_client::hud::HudBatch,ox:f32,oy:f32) {
        if let Some(mut field)=self.ae2_local_machine_field().cloned(){field.x=ox+48.0;field.y=oy+55.0;field.width=66.0;field.height=12.0;field.draw(batch,&self.gui.atlas,None);}
    }
    fn ae2_local_machine_return_carried(&mut self) {
        if let Some(stack)=self.carried.take(){if let Some(rest)=self.give(stack){self.player_drop(rest);}}
    }
    fn ae2_local_machine_open_priority(&mut self,session:&LocalMachineSession) {
        let target=PriorityTarget::Drive{pos:session.pos,identity:session.owner.host};let Some(value)=self.priority_target_read(target) else{return};
        self.ae2_local_machine_return_carried();let mut field=horizon_client::ui::EditBox::new(0.0,0.0,66.0,12.0);
        field.max_length=16;field.set_value(&value.to_string());field.highlight=0;field.focused=true;
        if let Some(session)=self.ae2_local_machine.as_mut(){session.edit=Some(LocalMachineEdit::Priority{target,field});}
    }
    fn ae2_local_machine_open_stock(&mut self,session:&LocalMachineSession,index:usize) {
        let Some(owner)=self.ae2_interface_menu_owner(session.pos,session.part) else{return};
        let Some(machine)=self.ae2_stock_machine(session.pos,session.part,owner) else{return};
        let Some((key,amount,max,unit,display))=self.ae2_stock_entry(&machine,index) else{return};
        self.ae2_local_machine_return_carried();let text=if unit==1 {amount.to_string()}else{
            let value=format!("{}.{:03}",amount/u64::from(unit),amount%u64::from(unit));value.trim_end_matches('0').trim_end_matches('.').to_owned()
        };
        let mut field=horizon_client::ui::EditBox::new(0.0,0.0,66.0,12.0);field.max_length=16;field.set_value(&text);field.highlight=0;field.focused=true;
        let edit=StockEdit{slot:index,owner,key,max,unit,text,token:0,display};
        if let Some(session)=self.ae2_local_machine.as_mut(){session.edit=Some(LocalMachineEdit::Stock{edit,field});}
    }
    pub(super) fn ae2_local_machine_slot_view(&self,slot:usize)->Option<ItemStack> {
        if !self.ae2_local_machine_valid(){return None;}let session=self.ae2_local_machine.as_ref()?;let index=slot.checked_sub(300)?;
        if let Some(edit)=session.edit.as_ref(){return match (edit,index) {
            (LocalMachineEdit::Priority{..},0..=3)=>Some(navigation(&format!("Priority +{}; shift uses 1/16/32/64",[1,10,100,1000][index]))),
            (LocalMachineEdit::Priority{..},4..=7)=>Some(navigation(&format!("Priority -{}; shift uses 1/16/32/64",[1,10,100,1000][index-4]))),
            (LocalMachineEdit::Priority{..},8)=>Some(navigation("Back to owner")),
            (LocalMachineEdit::Priority{field,..},9)=>Some(navigation(&format!("Priority: {}",field.value))),
            (LocalMachineEdit::Stock{..},0..=3)=>Some(navigation(&format!("Stock +{}; shift uses 1/16/32/64",[1,10,100,1000][index]))),
            (LocalMachineEdit::Stock{..},4..=7)=>Some(navigation(&format!("Stock -{}; shift uses 1/16/32/64",[1,10,100,1000][index-4]))),
            (LocalMachineEdit::Stock{..},8)=>Some(navigation("Set stock amount")),
            (LocalMachineEdit::Stock{..},9)=>Some(navigation("Back to Interface")),
            (LocalMachineEdit::Stock{edit,..},10)=>Some(edit.display.clone()),
            (LocalMachineEdit::Stock{edit,field},11)=>Some(navigation(&format!("Stock amount: {}{}",field.value,if edit.unit==1000 {" buckets"}else{""}))),
            _=>None,
        };}
        let snapshot=self.ae2_owned_machine_slots(session.pos,session.part)?;
        self.ae2_machine_menu_items(session.pos,session.part,session.page,&snapshot,session.link_input.as_ref(),session.link_output.as_ref()).get(index).cloned().flatten()
    }
    pub(super) fn ae2_local_machine_fake_slot(&self,slot:usize)->bool {
        if !self.ae2_local_machine_valid(){return false;}let Some(session)=self.ae2_local_machine.as_ref().filter(|s|s.edit.is_none()) else{return false};
        match session.owner.kind {
            horizon_client::ae2_world::MachineKind::CellWorkbench=>(305..323).contains(&slot) && session.page*18+(slot-305)<63,
            horizon_client::ae2_world::MachineKind::Interface=>(309..318).contains(&slot),
            _=>false,
        }
    }
    pub(super) fn ae2_local_machine_drag(&mut self,slot:usize,single:bool) {
        if !self.ae2_local_machine_fake_slot(slot) || self.carried.is_none(){return;}
        let physical=self.carried.clone();if single {if let Some(item)=self.carried.as_mut(){item.count=1;}}
        self.ae2_local_machine_click(Some(slot),0,Input::Pickup);self.carried=physical;
    }
    pub(super) fn ae2_local_machine_click(&mut self,slot:Option<usize>,button:i32,input:Input) {
        if !self.ae2_local_machine_valid(){self.close_container();self.screen=super::Screen::Playing;self.capture(true);return;}
        let Some(mut session)=self.ae2_local_machine.clone() else{return};
        // AEBaseScreen refuses creative cloning of wrapped generic stacks in
        // both the player inventory and the host's visible slots.
        if input==Input::Clone && slot.is_some_and(|slot|self.slot_view(slot).is_some_and(|item|item.item.as_ref()=="ae2:wrapped_generic_stack")) {return;}
        if let Some(edit)=session.edit.clone(){
            match (edit,slot) {
                (LocalMachineEdit::Priority{..},Some(308)) if input==Input::Pickup=>if let Some(s)=self.ae2_local_machine.as_mut(){s.edit=None;},
                (LocalMachineEdit::Stock{..},Some(309)) if input==Input::Pickup=>if let Some(s)=self.ae2_local_machine.as_mut(){s.edit=None;},
                (LocalMachineEdit::Stock{..},Some(308)) if input==Input::Pickup=>self.ae2_local_machine_confirm_editor(),
                (LocalMachineEdit::Priority{field,..},Some(index @ 300..=307)) if matches!(input,Input::Pickup|Input::QuickMove)=>{
                    if let Some(next)=priority_step(&field.value,(index-300) as i16,input==Input::QuickMove){if let Some(field)=self.ae2_local_machine_field_mut(){field.set_value(&next);field.focused=true;}self.ae2_local_machine_apply_priority();}
                },
                (LocalMachineEdit::Stock{edit,field},Some(index @ 300..=307)) if matches!(input,Input::Pickup|Input::QuickMove)=>{
                    let steps=if input==Input::QuickMove {[1i64,16,32,64]}else{[1i64,10,100,1000]};let i=index-300;
                    let delta=if i<4 {steps[i]}else{-steps[i%4]};
                    if let Some(next)=super::ae2_number_entry::number_step(&field.value,delta,0,edit.max/i64::from(edit.unit),'.'){if let Some(field)=self.ae2_local_machine_field_mut(){field.set_value(&next);field.focused=true;}}
                },
                _=>{}
            } return;
        }
        let (button,input)=if slot.is_some_and(|slot|self.ae2_local_machine_fake_slot(slot)) {
            // The source screen intercepts fake-slot hotkeys before they can
            // swap or drop physical player items. A hotbar swap on Workbench's
            // unlimited config slots is rejected by FakeSlot.mayPickup; the
            // Interface's registered 64-limit slots use the fake click path.
            if matches!(input,Input::Swap|Input::Throw) && self.carried.is_some() {return;}
            if input==Input::Swap && button!=40 && session.owner.kind!=horizon_client::ae2_world::MachineKind::Interface {return;}
            if input==Input::QuickMove && session.owner.kind!=horizon_client::ae2_world::MachineKind::Interface {(button,Input::Pickup)}
            else if matches!(input,Input::Pickup|Input::QuickMove) {(button,input)}
            else {(i32::from(button==1),Input::Pickup)}
        } else {(button,input)};
        if session.owner.kind==horizon_client::ae2_world::MachineKind::Drive && slot==Some(326) && input==Input::Pickup {self.ae2_local_machine_open_priority(&session);return;}
        if session.owner.kind==horizon_client::ae2_world::MachineKind::Interface && slot.is_some_and(|s|(309..318).contains(&s)) && input==Input::QuickMove {
            self.ae2_local_machine_open_stock(&session,slot.unwrap()-309);return;
        }
        if let Some(slot)=slot.filter(|s|(300..324).contains(s)) {
            if self.ae2_interface_menu_click(session.pos,session.part,slot-300,button,input){return;}
        }
        if session.owner.kind==horizon_client::ae2_world::MachineKind::WirelessAccessPoint && slot.is_some_and(|s|matches!(s,301|302)) {
            ae2_link_slot_click(self.dim.id(),session.pos,slot.unwrap()-300,button,input,&mut self.carried,&mut session.link_input,&mut session.link_output);
            if let Some(current)=self.ae2_local_machine.as_mut(){current.link_input=session.link_input;current.link_output=session.link_output;}return;
        }
        let slot=if session.owner.kind==horizon_client::ae2_world::MachineKind::Interface && slot==Some(318) {Some(309)}else{slot};
        if let Some(part)=session.part {self.ae2_part_machine_click(part,slot,button,input);}
        else if !self.ae2_workbench_menu_click(session.pos,&mut session.page,slot,button,input){self.ae2_machine_click(session.pos,slot,button,input);}
        if let Some(current)=self.ae2_local_machine.as_mut(){current.page=session.page;}
    }
    /// The server and in-frame screens use the same virtual layout and
    /// physical host snapshot; only their player/session transport differs.
    fn ae2_machine_menu_items(&self,pos:(i32,i32,i32),part:Option<super::ae2_parts::PartAddress>,page:usize,
        snapshot:&[Option<ItemStack>],link_input:Option<&ItemStack>,link_output:Option<&ItemStack>)->Vec<Option<ItemStack>> {
        let mut items = self
            .ae2_workbench_menu_slots(pos, page)
            .unwrap_or_else(|| snapshot.to_vec());
        items.resize(27, None);
        if part.is_none() {if let Some(condenser)=self.ae2_condenser_menu(pos) {items=condenser;}}
        let displayed_machine = if let Some(part) = part {
            self.ae2_part_machine(part).ok().flatten()
        } else {
            match self.world.block_entities.get(&pos) {
                Some(horizon_client::containers::BlockEntity::Ae2(machine)) => {
                    Some((**machine).clone())
                }
                _ => None,
            }
        };
        if let Some(machine) = displayed_machine {
            if matches!(machine.kind,horizon_client::ae2_world::MachineKind::Drive|horizon_client::ae2_world::MachineKind::PatternProvider) {
                items[26]=Some(navigation("Priority: click to edit"));
            }
            if machine.kind == horizon_client::ae2_world::MachineKind::Inscriber {
                let value=|key:&str,default:&str|match machine.config.get(&key.into()) {Some(Tag::String(s))=>s.to_string().unwrap_or_else(|_|default.to_owned()),_=>default.to_owned()};
                items[24]=Some(navigation(&format!("Auto export: {}; click to change",value("auto_export","NO"))));
                items[25]=Some(navigation(&format!("Separate sides: {}; click to change",value("inscriber_separate_sides","NO"))));
                items[26]=Some(navigation(&format!("Input capacity: {}; click to change",value("inscriber_input_capacity","SIXTY_FOUR"))));
            }
            if machine.kind == horizon_client::ae2_world::MachineKind::Interface {
                items[24] = Some(navigation("Shift-click a configured slot to set its stock amount"));
                items[18] = machine.upgrades.first().cloned().flatten();
                let priority = match machine.config.get(&"priority".into()) {
                    Some(Tag::Int(n)) => *n,
                    _ => 0,
                };
                for (index, amount) in [1, 10, 100, 1000].into_iter().enumerate() {
                    items[19 + index] = Some(navigation(&format!(
                        "Priority: {priority}; left +{amount}, right -{amount}"
                    )));
                }
                let fuzzy = match machine.config.get(&"ae2_fuzzy_mode".into()) {
                    Some(Tag::String(s)) => s.to_string().unwrap_or_else(|_| "ignore_all".into()),
                    _ => "ignore_all".into(),
                };
                items[23] = Some(navigation(&format!("Fuzzy mode: {fuzzy}; click to change")));
                if let Ok(config) = super::ae2_interface::interface_config(&machine) {
                    for (index, entry) in config.into_iter().enumerate() {
                        items[9 + index] = entry;
                    }
                }
                for (offset, values) in [
                    (9, super::ae2_interface::interface_fluid_config(&machine)),
                    (0, super::ae2_interface::interface_fluid_stock(&machine)),
                ] {
                    if let Ok(values) = values {
                        for (index, entry) in values.into_iter().enumerate() {
                            if let Some((key, n)) = entry {
                                let id = super::ae2_fluids::fluid_bucket(&key.item_id)
                                    .unwrap_or("minecraft:bucket");
                                let mut icon = ItemStack::new(id, 1);
                                icon.data_mut().custom_name =
                                    Some(format!("{}: {} mB", key.item_id, n));
                                items[offset + index] = Some(icon);
                            }
                        }
                    }
                }
            }
            if machine.kind == horizon_client::ae2_world::MachineKind::PatternProvider {
                let visible=machine.config.get(&"terminal_visible".into())!=Some(&Tag::Byte(0));
                items[20]=Some(navigation(if visible {"Pattern Access: visible"} else {"Pattern Access: hidden"}));
                let enabled = machine.config.get(&"ae2_blocking".into()) == Some(&Tag::Byte(1));
                items[18] = Some(navigation(if enabled {
                    "Blocking mode: on"
                } else {
                    "Blocking mode: off"
                }));
                let mode = match machine.config.get(&"ae2_lock_mode".into()) {
                    Some(Tag::Int(n)) => *n,
                    _ => 0,
                };
                items[19] = Some(navigation(match mode {
                    1 => "Craft lock: until redstone pulse",
                    2 => "Craft lock: while redstone high",
                    3 => "Craft lock: while redstone low",
                    4 => "Craft lock: until result",
                    _ => "Craft lock: none",
                }));
            }
        }
        if matches!(self.world.block_entities.get(&pos), Some(horizon_client::containers::BlockEntity::Ae2(m)) if m.kind == horizon_client::ae2_world::MachineKind::WirelessAccessPoint)
        {
            items[1] = link_input.cloned();
            items[2] = link_output.cloned();
        }
        items
    }
    fn lan_send_ae2_machine(&mut self, host: &mut super::lan::LanHost, conn: u32) {
        if host.play.get(&conn).and_then(|p|p.ae2_machine.as_ref()).is_some_and(|m|m.stock_edit.is_some()){self.lan_send_stock(host,conn);return;}
        if host.play.get(&conn).and_then(|p|p.ae2_machine.as_ref()).is_some_and(|m|m.priority_edit.is_some()){self.lan_send_priority(host,conn);return;}
        use horizon_protocol::packets::play::{self, Clientbound as C};
        let Some((pos, window, part, owner)) = host
            .play
            .get(&conn)
            .and_then(|p| p.ae2_machine.as_ref())
            .map(|m| (m.pos, m.window, m.part, m.owner))
        else {
            return;
        };
        let snapshot=self.ae2_owned_machine_slots(pos,part)
            .filter(|_|self.ae2_machine_menu_owner(pos,part)==Some(owner));
        let Some(snapshot) = snapshot else {
            self.lan_ae2_machine_closed(host, conn, None);
            host.hub.send_to(
                conn,
                &C::ContainerClose(play::ContainerClose { container: window }),
            );
            return;
        };
        let Some(g) = host.hub.guests.iter().find(|g| g.conn == conn) else {
            return;
        };
        let page = host
            .play
            .get(&conn)
            .and_then(|p| p.ae2_machine.as_ref())
            .map_or(0, |m| m.workbench_page);
        let (link_input,link_output)=host.play.get(&conn).and_then(|p|p.ae2_machine.as_ref())
            .map(|m|(m.link_input.clone(),m.link_output.clone())).unwrap_or_default();
        let mut items=self.ae2_machine_menu_items(pos,part,page,&snapshot,link_input.as_ref(),link_output.as_ref());
        let displayed_machine = if let Some(part) = part {
            self.ae2_part_machine(part).ok().flatten()
        } else {
            match self.world.block_entities.get(&pos) {
                Some(horizon_client::containers::BlockEntity::Ae2(machine)) => Some((**machine).clone()),
                _ => None,
            }
        };
        let fake_slots:Vec<u8>=match displayed_machine.as_ref().map(|machine|machine.kind) {
            Some(horizon_client::ae2_world::MachineKind::CellWorkbench)=>(0..63usize.saturating_sub(page*18).min(18)).map(|i|(5+i) as u8).collect(),
            // The Interface config wrapper's registered item capacity is 64;
            // unlike overstacking fake slots, its hotbar key is a fake click.
            Some(horizon_client::ae2_world::MachineKind::Interface)=>(9..18).map(|slot|slot|0x80).collect(),
            _=>Vec::new(),
        };
        let provider_return_snapshot=displayed_machine.as_ref().and_then(|machine|provider_return_slots(machine).ok());
        let workbench_keep=displayed_machine.as_ref().filter(|machine|machine.kind==horizon_client::ae2_world::MachineKind::CellWorkbench).map(workbench_copy_kept);
        items.extend((9..36).chain(0..9).map(|i| g.inventory[i].clone()));
        let carried = g.carried.clone();
        Self::lan_ae2_fake_slots(host,conn,window,&fake_slots);
        let Some(m) = host
            .play
            .get_mut(&conn)
            .and_then(|p| p.ae2_machine.as_mut())
        else {
            return;
        };
        m.snapshot = snapshot;
        m.state = (m.state + 1) & 0x7fff;
        let state = m.state;
        if let Some(returns)=provider_return_snapshot {
            m.provider_return_revisions.push_back((state,returns));
            while m.provider_return_revisions.len()>32 {m.provider_return_revisions.pop_front();}
        }
        if let Some(keep)=workbench_keep {
            m.workbench_revisions.push_back(WorkbenchRevision{state,page,keep});
            while m.workbench_revisions.len()>64 {m.workbench_revisions.pop_front();}
        }
        let packet = C::ContainerSetContent(play::ContainerSetContent {
            container: window,
            state,
            items: items.iter().map(|s| host.hub.stack(s)).collect(),
            carried: host.hub.stack(&carried),
        });
        host.hub.send_to(conn, &packet);
    }
    pub(super) fn lan_refresh_ae2_machines(&mut self, host: &mut super::lan::LanHost) {
        let windows: Vec<_> = host
            .play
            .iter()
            .filter_map(|(&c, p)| {
                if !host
                    .hub
                    .guests
                    .iter()
                    .any(|g| g.conn == c && host.hub.here(g))
                {
                    return None;
                }
                p.ae2_machine.as_ref().map(|m| {
                    (
                        c,
                        m.pos,
                        m.window,
                        m.snapshot.clone(),
                        m.owner_dim.clone(),
                        m.part,
                        m.owner,
                    )
                })
            })
            .collect();
        for (conn, pos, window, old, owner_dim, part, owner) in windows {
            let valid = host
                .hub
                .guests
                .iter()
                .find(|g| g.conn == conn)
                .is_some_and(|g| {
                    !g.spectator() && g.within_block_range([pos.0, pos.1, pos.2], 4.0)
                });
            if !valid || owner_dim != self.dim.id() || self.ae2_machine_menu_owner(pos,part)!=Some(owner) {
                self.lan_ae2_machine_closed(host, conn, None);
                host.hub.send_to(
                    conn,
                    &horizon_protocol::packets::play::Clientbound::ContainerClose(
                        horizon_protocol::packets::play::ContainerClose { container: window },
                    ),
                );
                continue;
            }
            if self.ae2_owned_machine_slots(pos, part).as_ref() != Some(&old) {
                self.lan_send_ae2_machine(host, conn);
            }
        }
    }
    pub(super) fn lan_ae2_machine_click(
        &mut self,
        host: &mut super::lan::LanHost,
        conn: u32,
        p: &horizon_protocol::packets::play::ContainerClick,
    ) -> bool {
        let Some((pos, state, snapshot, part)) = host
            .play
            .get(&conn)
            .and_then(|p| p.ae2_machine.as_ref())
            .filter(|m| m.window == p.container)
            .map(|m| (m.pos, m.state, m.snapshot.clone(), m.part))
        else {
            return false;
        };
        let valid = host
            .hub
            .guests
            .iter()
            .find(|g| g.conn == conn)
            .is_some_and(|g| !g.spectator() && g.within_block_range([pos.0, pos.1, pos.2], 4.0));
        let owner_valid = host
            .play
            .get(&conn)
            .and_then(|p| p.ae2_machine.as_ref())
            .is_some_and(|m| m.owner_dim == self.dim.id() && self.ae2_machine_menu_owner(pos,part)==Some(m.owner));
        if !valid || !owner_valid || self.ae2_owned_machine_slots(pos, part).is_none() {
            self.lan_ae2_machine_closed(host, conn, None);
            host.hub.send_to(
                conn,
                &horizon_protocol::packets::play::Clientbound::ContainerClose(
                    horizon_protocol::packets::play::ContainerClose {
                        container: p.container,
                    },
                ),
            );
            return true;
        }
        if host.play.get(&conn).and_then(|p|p.ae2_machine.as_ref()).is_some_and(|m|m.stock_edit.is_some()){return self.lan_ae2_stock_click(host,conn,p);}
        if host.play.get(&conn).and_then(|p|p.ae2_machine.as_ref()).is_some_and(|m|m.priority_edit.is_some()){return self.lan_ae2_priority_click(host,conn,p);}
        if (9..18).contains(&p.slot) && super::lan_crafting::input(p.input)==Input::QuickMove && self.ae2_interface_menu_owner(pos,part).is_some() {
            return self.lan_open_stock(host,conn,p.container,pos,part,(p.slot-9) as usize);
        }
        if p.slot==26 && super::lan_crafting::input(p.input)==Input::Pickup {
            let target=host.play.get(&conn).and_then(|p|p.ae2_machine.as_ref()).and_then(|m|
                m.provider_owner.map(|identity|PriorityTarget::Provider{pos,part,identity})
                    .or_else(||m.drive_owner.map(|identity|PriorityTarget::Drive{pos,identity})));
            if let Some(target)=target {return self.lan_priority_open(host,conn,p.container,target);}
        }
        let paged_ghost = part.is_none()
            && matches!(self.world.block_entities.get(&pos),Some(horizon_client::containers::BlockEntity::Ae2(m)) if m.kind==horizon_client::ae2_world::MachineKind::CellWorkbench)
            && (5..23).contains(&p.slot);
        if paged_ghost && matches!(p.button,2|3) && super::lan_crafting::input(p.input)==Input::Pickup
            && host.hub.guests.iter().find(|g|g.conn==conn).is_none_or(|g|g.carried.is_none()) {
            self.lan_send_ae2_machine(host,conn);return true;
        }
        if paged_ghost {
            // AEBaseMenu receives a separate fake-slot action without a vanilla
            // state ID. A cursor-only refresh must not drop an ordered action,
            // but the earlier visible page and copy mode must still match.
            let keep=match self.world.block_entities.get(&pos) {Some(horizon_client::containers::BlockEntity::Ae2(machine))=>workbench_copy_kept(machine),_=>false};
            let compatible=host.play.get(&conn).and_then(|p|p.ae2_machine.as_ref()).is_some_and(|m|
                workbench_revision_matches(&m.workbench_revisions,p.state,m.workbench_page,keep));
            if !compatible {self.lan_send_ae2_machine(host,conn);return true;}
        }
        let provider=host.play.get(&conn).and_then(|p|p.ae2_machine.as_ref()).is_some_and(|m|m.provider_owner.is_some());
        if provider && (9..18).contains(&p.slot) {
            if state != p.state || self.ae2_owned_machine_slots(pos,part).as_ref()!=Some(&snapshot) {
                let source=self.provider_menu_machine(pos,part).and_then(|machine|provider_return_slots(&machine).ok());
                let safe=super::lan_crafting::input(p.input)==Input::QuickMove && source.as_ref().is_some_and(|current|
                    host.play.get(&conn).and_then(|player|player.ae2_machine.as_ref()).is_some_and(|menu|
                        provider_return_revision_matches(&menu.provider_return_revisions,p.state,(p.slot-9) as usize,current)));
                if !safe {self.lan_send_ae2_machine(host,conn);return true;}
            }
            let index=(p.slot-9) as usize;let button=i32::from(p.button);let input=super::lan_crafting::input(p.input);
            self.as_guest(host,conn,|g|g.ae2_provider_return_click(pos,part,index,button,input));
            let viewers:Vec<_>=host.play.iter().filter_map(|(&c,p)|p.ae2_machine.as_ref().filter(|m|m.pos==pos && m.part==part && m.owner_dim==self.dim.id()).map(|_|c)).collect();
            for viewer in viewers {self.lan_send_ae2_machine(host,viewer);}
            return true;
        }
        let menu_input=super::lan_crafting::input(p.input);
        let interface=host.play.get(&conn).and_then(|p|p.ae2_machine.as_ref()).is_some_and(|m|m.owner.kind==horizon_client::ae2_world::MachineKind::Interface);
        if interface && (0..=23).contains(&p.slot) && self.as_guest(host,conn,|game|
            game.ae2_interface_menu_click(pos,part,p.slot as usize,i32::from(p.button),menu_input)).unwrap_or(false) {
            let viewers:Vec<_>=host.play.iter().filter_map(|(&c,p)|
                (p.ae2_machine.as_ref().is_some_and(|m|m.pos==pos && m.part==part && m.owner_dim==self.dim.id())
                    && host.hub.guests.iter().any(|g|g.conn==c && host.hub.here(g))).then_some(c)).collect();
            for viewer in viewers {self.lan_send_ae2_machine(host,viewer);}
            return true;
        }
        if let Some(part) = part {
            if (p.slot == 18 || p.slot == 19 || p.slot == 20)
                && super::lan_crafting::input(p.input) == Input::Pickup
            {
                if let Ok(Some(mut machine)) = self.ae2_part_machine(part) {
                    if machine.kind == horizon_client::ae2_world::MachineKind::PatternProvider {
                        if p.slot == 18 {
                            let old =
                                machine.config.get(&"ae2_blocking".into()) == Some(&Tag::Byte(1));
                            machine
                                .config
                                .insert("ae2_blocking".into(), Tag::Byte(i8::from(!old)));
                        } else if p.slot==20 {
                            let visible=machine.config.get(&"terminal_visible".into())!=Some(&Tag::Byte(0));
                            machine.config.insert("terminal_visible".into(),Tag::Byte(i8::from(!visible)));
                        } else {
                            let mode = match machine.config.get(&"ae2_lock_mode".into()) {
                                Some(Tag::Int(n)) => *n,
                                _ => 0,
                            };
                            machine
                                .config
                                .insert("ae2_lock_mode".into(), Tag::Int((mode + 1).rem_euclid(5)));
                            machine.config.remove(&"ae2_unlock_event".into());
                            machine.config.remove(&"ae2_unlock_stack".into());
                        }
                        let _ = self.ae2_store_part_machine(part, &machine);
                        self.lan_send_ae2_machine(host, conn);
                        return true;
                    }
                }
            }
        }
        if (p.slot == 18 || p.slot == 19 || p.slot == 20)
            && super::lan_crafting::input(p.input) == Input::Pickup
            && (0..=1).contains(&p.button)
        {
            if let Some(horizon_client::containers::BlockEntity::Ae2(machine)) =
                self.world.block_entities.get_mut(&pos)
            {
                if machine.kind == horizon_client::ae2_world::MachineKind::PatternProvider {
                    if p.slot == 18 {
                        let old = machine.config.get(&"ae2_blocking".into()) == Some(&Tag::Byte(1));
                        machine
                            .config
                            .insert("ae2_blocking".into(), Tag::Byte(i8::from(!old)));
                    } else if p.slot==20 {
                        let visible=machine.config.get(&"terminal_visible".into())!=Some(&Tag::Byte(0));
                        machine.config.insert("terminal_visible".into(),Tag::Byte(i8::from(!visible)));
                    } else {
                        let mode = match machine.config.get(&"ae2_lock_mode".into()) {
                            Some(Tag::Int(n)) => *n,
                            _ => 0,
                        };
                        machine
                            .config
                            .insert("ae2_lock_mode".into(), Tag::Int((mode + 1).rem_euclid(5)));
                        machine.config.remove(&"ae2_unlock_event".into());
                        machine.config.remove(&"ae2_unlock_stack".into());
                    }
                    self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
                    let viewers: Vec<_> = host
                        .play
                        .iter()
                        .filter_map(|(&c, p)| {
                            (p.ae2_machine
                                .as_ref()
                                .is_some_and(|m| m.pos == pos && m.owner_dim == self.dim.id())
                                && host
                                    .hub
                                    .guests
                                    .iter()
                                    .any(|g| g.conn == c && host.hub.here(g)))
                            .then_some(c)
                        })
                        .collect();
                    for viewer in viewers {
                        self.lan_send_ae2_machine(host, viewer);
                    }
                    return true;
                }
            }
        }
        let wireless_access = matches!(self.world.block_entities.get(&pos), Some(horizon_client::containers::BlockEntity::Ae2(m)) if m.kind == horizon_client::ae2_world::MachineKind::WirelessAccessPoint);
        if wireless_access && (p.slot == 1 || p.slot == 2) {
            let input = super::lan_crafting::input(p.input);
            if let (Some(menu), Some(guest)) = (
                host.play.get_mut(&conn).and_then(|p|p.ae2_machine.as_mut()),
                host.hub.guests.iter_mut().find(|g|g.conn==conn),
            ) {
                ae2_link_slot_click(self.dim.id(),pos,p.slot as usize,i32::from(p.button),input,
                    &mut guest.carried,&mut menu.link_input,&mut menu.link_output);
            }
            self.lan_send_ae2_machine(host, conn);
            return true;
        }
        let is_interface = if let Some(part) = part {
            self.ae2_part_machine(part)
                .ok()
                .flatten()
                .is_some_and(|m| m.kind == horizon_client::ae2_world::MachineKind::Interface)
        } else {
            matches!(self.world.block_entities.get(&pos),Some(horizon_client::containers::BlockEntity::Ae2(m)) if m.kind==horizon_client::ae2_world::MachineKind::Interface)
        };
        let slot = match p.slot {
            18 if is_interface => Some(309),
            0..=26 => Some(300 + p.slot as usize),
            27..=53 => Some(9 + (p.slot as usize - 27)),
            54..=62 => Some(p.slot as usize - 54),
            _ => None,
        };
        let raw_input=super::lan_crafting::input(p.input);
        let input=if paged_ghost && raw_input==Input::QuickMove {Input::Pickup}else{raw_input};
        let button = if paged_ghost && matches!(p.button,2|3) {0} else {i32::from(p.button)};
        let mut workbench_page = host
            .play
            .get(&conn)
            .and_then(|p| p.ae2_machine.as_ref())
            .map_or(0, |m| m.workbench_page);
        let provider_slots = if let Some(part) = part {
            self.ae2_part_machine(part).ok().flatten()
        } else {
            match self.world.block_entities.get(&pos) {
                Some(horizon_client::containers::BlockEntity::Ae2(machine)) => Some((**machine).clone()),
                _ => None,
            }
        }.filter(|machine|machine.kind==horizon_client::ae2_world::MachineKind::PatternProvider)
            .map(|machine|machine.slots);
        let is_provider=provider_slots.is_some();
        let provider_setter=self.as_guest(host, conn, |g| {
            if is_provider && input==Input::Swap && slot.is_some_and(|slot|(300..309).contains(&slot)) {
                return g.ae2_provider_slot_swap(pos,part,slot,button);
            }
            if let Some(part) = part {
                g.ae2_part_machine_click(part, slot, button, input);
            } else if !g.ae2_workbench_menu_click(pos, &mut workbench_page, slot, button, input) {
                g.ae2_machine_click(pos, slot, button, input);
            }
            if is_provider && input==Input::QuickMove {
                if let Some(index @ 0..=35)=slot {g.ae2_provider_return_quick_insert(pos,part,index);}
            }
            false
        }).unwrap_or(false);
        if let Some(before)=provider_slots {
            let after=if let Some(part)=part {
                self.ae2_part_machine(part).ok().flatten()
            } else {
                match self.world.block_entities.get(&pos) {
                    Some(horizon_client::containers::BlockEntity::Ae2(machine))=>Some((**machine).clone()),
                    _=>None,
                }
            };
            if after.is_some_and(|machine|machine.kind==horizon_client::ae2_world::MachineKind::PatternProvider && (machine.slots!=before || provider_setter)) {
                if let Some(part)=part {
                    self.ae2_provider_patterns_changed(part.host,Some(part.face));
                } else {
                    self.ae2_provider_patterns_changed_local(pos,None);
                }
            }
        }
        if is_provider && input==Input::Swap {
            if let Some(gi)=host.hub.guests.iter().position(|guest|guest.conn==conn) {host.hub.send_inventory(gi);}
        }
        if let Some(m) = host
            .play
            .get_mut(&conn)
            .and_then(|p| p.ae2_machine.as_mut())
        {
            m.workbench_page = workbench_page;
        }
        let viewers: Vec<_> = host
            .play
            .iter()
            .filter_map(|(&c, p)| {
                (host
                    .hub
                    .guests
                    .iter()
                    .any(|g| g.conn == c && host.hub.here(g)))
                .then(|| p.ae2_machine.as_ref().map(|_| c))
                .flatten()
            })
            .collect();
        for c in viewers {
            self.lan_send_ae2_machine(host, c);
        }
        true
    }
    /// Source SWAP writes the physical slot even when two encoded stacks
    /// compare equal. Report that setter separately from value differences so
    /// the provider registry receives exactly one post-commit refresh.
    fn ae2_provider_slot_swap(&mut self,pos:(i32,i32,i32),part:Option<super::ae2_parts::PartAddress>,slot:Option<usize>,button:i32)->bool {
        use horizon_client::{ae2_world::MachineKind,containers::BlockEntity};
        let Some(index)=slot.and_then(|slot|slot.checked_sub(300)).filter(|index|*index<9) else{return false};
        if !(0..=8).contains(&button) && button!=40 {return false}
        let mut machine=if let Some(part)=part {self.ae2_part_machine(part).ok().flatten()} else {
            match self.world.block_entities.get(&pos) {Some(BlockEntity::Ae2(machine))=>Some((**machine).clone()),_=>None}
        };
        let Some(machine)=machine.as_mut().filter(|machine|machine.kind==MachineKind::PatternProvider && machine.slots.len()==9) else{return false};
        let source=if button==40 {self.offhand.clone()} else {self.inventory[button as usize].clone()};
        let target=machine.slots[index].clone();
        if source.is_none() && target.is_none() {return false}
        // Real encoded items stack to one. Reject malformed overstacks before
        // mutation; the general oversized-stack/drop path remains separate.
        if source.as_ref().is_some_and(|stack|stack.count!=1 || !super::ae2_pattern_access::encoded_pattern_item(stack))
            || target.as_ref().is_some_and(|stack|stack.count!=1) {return false}
        machine.slots[index]=source;
        if let Some(part)=part {
            if self.ae2_store_part_machine(part,machine).is_err() {return false}
        } else {
            let Some(BlockEntity::Ae2(current))=self.world.block_entities.get_mut(&pos) else{return false};
            if current.kind!=MachineKind::PatternProvider {return false}
            **current=machine.clone();self.world.modified.insert((pos.0>>4,pos.2>>4));
        }
        if button==40 {self.offhand=target;} else {self.inventory[button as usize]=target;}
        true
    }
    /// Machine screens own their physical slots; replay player-slot gestures
    /// through the native menu so item overrides and creative actions apply.
    pub(super) fn ae2_machine_player_menu_click(&mut self,slot:usize,button:i32,input:Input)->bool {
        use horizon_client::menu::{Kind,Menu};
        if slot>=36 {return false;}
        // AEBaseScreen.slotClicked suppresses creative cloning of every
        // WrappedGenericStack in an AE2 menu, including malformed wrappers.
        if input==Input::Clone && self.inventory[slot].as_ref().is_some_and(|item|item.item.as_ref()=="ae2:wrapped_generic_stack") {return true;}
        let supported=match input {
            Input::Pickup|Input::Throw=>(0..=1).contains(&button),
            Input::Clone=>button==2,
            Input::Swap=>(0..=8).contains(&button)||button==40,
            _=>false,
        };
        if !supported {return false;}
        let result={
            let env=self.menu_env();let mut menu=Menu::new(Kind::Inventory);
            menu.creative=self.creative;menu.selected=self.selected;
            for (i,item) in self.inventory.iter().enumerate(){menu.set_player_item(i,item.clone());}
            menu.set_player_item(40,self.offhand.clone());menu.carried=self.carried.clone();
            let index=if slot<9 {36+slot}else{slot};
            menu.click(&env,index as i32,button,input).map(|()|{
                let inventory=(0..36).map(|i|menu.player_item(i).cloned()).collect();
                (inventory,menu.player_item(40).cloned(),menu.carried.clone(),std::mem::take(&mut menu.dropped))
            })
        };
        if let Ok((inventory,offhand,carried,drops))=result {
            self.inventory=inventory;self.offhand=offhand;self.carried=carried;
            for stack in drops {self.player_drop(stack);}
        }
        true
    }
    /// Per-face part inventory transaction. Stale menu snapshots are checked
    /// before this runs; the player and owned part roll back together on error.
    fn ae2_part_machine_click(
        &mut self,
        part: super::ae2_parts::PartAddress,
        slot: Option<usize>,
        button: i32,
        input: Input,
    ) {
        let Some(slot) = slot else { return };
        if self.ae2_machine_player_menu_click(slot,button,input) {return;}
        let Ok(Some(mut machine)) = self.ae2_part_machine(part) else {
            return;
        };
        let before_inventory = self.inventory.clone();
        let before_carried = self.carried.clone();
        let normal = machine.slots.len();
        let mut slots = machine.slots.clone();
        slots.extend(machine.upgrades.clone());
        if (300..327).contains(&slot) {
            let i = slot - 300;
            if i >= slots.len() {
                return;
            }
            if input == Input::QuickMove {
                if let Some(item) = slots[i].take() {
                    slots[i] = self.give(item);
                }
            } else if input == Input::Pickup && (0..=1).contains(&button) {
                let held = self.carried.take();
                let item = slots[i].take();
                let allows = |s: &ItemStack| {
                    if i >= normal {
                        super::ae2_machine_ui::machine_upgrade_allowed(&machine, i - normal, s)
                    } else {
                        machine.kind != horizon_client::ae2_world::MachineKind::PatternProvider
                            || super::ae2_pattern_access::encoded_pattern_item(s)
                    }
                };
                let max = |s: &ItemStack| {
                    if i >= normal {
                        1
                    } else {
                        self.res.items().stack_max(&s)
                    }
                };
                match (held, item, button) {
                    (None, None, _) => {}
                    (None, Some(v), 0) => self.carried = Some(v),
                    (None, Some(mut v), 1) => {
                        let n = v.count.div_ceil(2);
                        v.count -= n;
                        self.carried = Some(ItemStack {
                            count: n,
                            ..v.clone()
                        });
                        if v.count > 0 {
                            slots[i] = Some(v);
                        }
                    }
                    (Some(mut h), None, b) if allows(&h) => {
                        let n = max(&h).min(if b == 1 { 1 } else { h.count });
                        slots[i] = Some(ItemStack {
                            count: n,
                            ..h.clone()
                        });
                        h.count -= n;
                        if h.count > 0 {
                            self.carried = Some(h);
                        }
                    }
                    (Some(mut h), Some(mut v), b) if same(&h, &v) && allows(&h) => {
                        let n =
                            max(&v)
                                .saturating_sub(v.count)
                                .min(if b == 1 { 1 } else { h.count });
                        v.count += n;
                        h.count -= n;
                        slots[i] = Some(v);
                        if h.count > 0 {
                            self.carried = Some(h);
                        }
                    }
                    (Some(h), Some(v), _) if allows(&h) && h.count <= max(&h) => {
                        slots[i] = Some(h);
                        self.carried = Some(v);
                    }
                    (h, v, _) => {
                        self.carried = h;
                        slots[i] = v;
                    }
                }
            } else {
                return;
            }
        } else if slot < 36 {
            if input == Input::QuickMove {
                if let Some(mut held) = self.inventory[slot].take() {
                    for i in (normal..slots.len()).chain(0..normal) {
                        let allowed = if i >= normal {
                            super::ae2_machine_ui::machine_upgrade_allowed(
                                &machine,
                                i - normal,
                                &held,
                            )
                        } else {
                            machine.kind != horizon_client::ae2_world::MachineKind::PatternProvider
                                || super::ae2_pattern_access::encoded_pattern_item(&held)
                        };
                        if !allowed {
                            continue;
                        }
                        let max = if i >= normal {
                            1
                        } else {
                            self.res.items().stack_max(&held)
                        };
                        match &mut slots[i] {
                            Some(v) if same(v, &held) => {
                                let n = max.saturating_sub(v.count).min(held.count);
                                v.count += n;
                                held.count -= n
                            }
                            None => {
                                let n = max.min(held.count);
                                slots[i] = Some(ItemStack {
                                    count: n,
                                    ..held.clone()
                                });
                                held.count -= n
                            }
                            _ => {}
                        }
                        if held.count == 0 {
                            break;
                        }
                    }
                    if held.count > 0 {
                        self.inventory[slot] = Some(held)
                    }
                }
            } else if input == Input::Pickup && (0..=1).contains(&button) {
                let held = self.carried.take();
                let item = self.inventory[slot].take();
                match (held, item, button) {
                    (None, None, _) => {}
                    (None, Some(v), 0) => self.carried = Some(v),
                    (None, Some(mut v), 1) => {
                        let n = v.count.div_ceil(2);
                        v.count -= n;
                        self.carried = Some(ItemStack {
                            count: n,
                            ..v.clone()
                        });
                        if v.count > 0 {
                            self.inventory[slot] = Some(v)
                        }
                    }
                    (Some(h), None, 0) => self.inventory[slot] = Some(h),
                    (Some(mut h), None, 1) => {
                        self.inventory[slot] = Some(ItemStack {
                            count: 1,
                            ..h.clone()
                        });
                        h.count -= 1;
                        if h.count > 0 {
                            self.carried = Some(h)
                        }
                    }
                    (Some(mut h), Some(mut v), b) if same(&h, &v) => {
                        let max = self.res.items().stack_max(&v);
                        let n = max
                            .saturating_sub(v.count)
                            .min(if b == 1 { 1 } else { h.count });
                        v.count += n;
                        h.count -= n;
                        self.inventory[slot] = Some(v);
                        if h.count > 0 {
                            self.carried = Some(h)
                        }
                    }
                    (Some(h), Some(v), _) => {
                        self.inventory[slot] = Some(h);
                        self.carried = Some(v)
                    }
                    (h, v, _) => {
                        self.carried = h;
                        self.inventory[slot] = v
                    }
                }
            } else {
                return;
            }
        } else {
            return;
        }
        let had_crafting_card = machine
            .upgrades
            .iter()
            .flatten()
            .any(|s| s.item.as_ref() == "ae2:crafting_card");
        machine.slots = slots[..normal].to_vec();
        machine.upgrades = slots[normal..].to_vec();
        if had_crafting_card
            && !machine
                .upgrades
                .iter()
                .flatten()
                .any(|s| s.item.as_ref() == "ae2:crafting_card")
        {
            super::ae2_interface::mark_card_removed(&mut machine);
        }
        super::ae2_interface::wake_interface(&mut machine);
        if self.ae2_store_part_machine(part, &machine).is_err() {
            self.inventory = before_inventory;
            self.carried = before_carried;
        }
    }
    fn ae2_machine_click(&mut self,pos:(i32,i32,i32),slot:Option<usize>,button:i32,input:Input) {
        if matches!(slot,Some(324|325|326)) && input==Input::Pickup && (0..=1).contains(&button)
            && self.ae2_inscriber_setting(pos,slot.unwrap(),button==1) {return;}
        let condenser=matches!(self.world.block_entities.get(&pos),Some(horizon_client::containers::BlockEntity::Ae2(m)) if m.kind==horizon_client::ae2_world::MachineKind::Condenser);
        if !condenser {
            let before=match self.world.block_entities.get(&pos) {Some(horizon_client::containers::BlockEntity::Ae2(m)) if m.kind==horizon_client::ae2_world::MachineKind::QuantumLink=>Some(m.slots.first().cloned()),_=>None};
            let workbench=match self.world.block_entities.get(&pos) {
                Some(horizon_client::containers::BlockEntity::Ae2(m)) if m.kind==horizon_client::ae2_world::MachineKind::CellWorkbench=>Some((**m).clone()),
                _=>None,
            };
            let player=workbench.as_ref().map(|_|(self.inventory.clone(),self.carried.clone()));
            self.ae2_machine_click_physical(pos,slot,button,input);
            if let Some(previous)=workbench {
                let after=match self.world.block_entities.get(&pos) {
                    Some(horizon_client::containers::BlockEntity::Ae2(m)) if m.kind==horizon_client::ae2_world::MachineKind::CellWorkbench=>Some((**m).clone()),
                    _=>None,
                };
                if let Some(mut next)=after.filter(|m|m.slots.first()!=previous.slots.first()) {
                    if self.ae2_workbench_cell_changed(&previous,&mut next).is_ok() {
                        if let Some(horizon_client::containers::BlockEntity::Ae2(m))=self.world.block_entities.get_mut(&pos) {
                            **m=next;
                            if let Some(cell)=m.slots.first().and_then(Option::as_ref){horizon_client::ae2_stack_identity::synchronize_tracked(cell);}
                        }
                    } else {
                        if let Some(horizon_client::containers::BlockEntity::Ae2(m))=self.world.block_entities.get_mut(&pos){**m=previous;}
                        if let Some((inventory,carried))=player {self.inventory=inventory;self.carried=carried;}
                    }
                }
            }
            if let Some(before)=before {
                let after=match self.world.block_entities.get(&pos) {Some(horizon_client::containers::BlockEntity::Ae2(m))=>m.slots.first().cloned(),_=>None};
                if before!=after {self.ae2_quantum_inventory_changed(pos);}
            }
            return;
        }
        let Some(slot)=slot else {return;};
        match slot {
            300 if input==Input::Pickup && (0..=1).contains(&button)=>self.ae2_condenser_cursor_input(pos,button==1),
            301=>{
                let before=match self.world.block_entities.get(&pos) {Some(horizon_client::containers::BlockEntity::Ae2(m))=>m.slots[0].clone(),_=>None};
                self.ae2_machine_click_physical(pos,Some(300),button,input);
                let after=match self.world.block_entities.get(&pos) {Some(horizon_client::containers::BlockEntity::Ae2(m))=>m.slots[0].clone(),_=>None};
                if before!=after {self.ae2_condenser_output_changed(pos);}
            }
            302=>self.ae2_machine_click_physical(pos,Some(301),button,input),
            326 if input==Input::Pickup && (0..=1).contains(&button)=>self.ae2_condenser_cycle(pos,button==1),
            0..36=>{
                if input!=Input::QuickMove || !self.ae2_condenser_shift_input(pos,slot) {self.ae2_machine_click_physical(pos,Some(slot),button,input);}
            }
            _=>{}
        }
    }
    fn ae2_machine_click_physical(
        &mut self,
        pos: (i32, i32, i32),
        slot: Option<usize>,
        button: i32,
        input: Input,
    ) {
        use horizon_client::containers::BlockEntity;
        let Some(slot) = slot else { return };
        if self.ae2_machine_player_menu_click(slot,button,input) {return;}
        let Some(mut slots) = self.ae2_machine_slots(pos) else {
            return;
        };
        if (300..327).contains(&slot) {
            let i = slot - 300;
            if i >= slots.len() {
                return;
            }
            if input == Input::QuickMove {
                if let Some(item) = slots[i].take() {
                    let rest = self.give(item);
                    slots[i] = rest;
                }
            } else if input == Input::Pickup && (0..=1).contains(&button) {
                let held = self.carried.take();
                let item = slots[i].take();
                match (held, item, button) {
                    (None, None, _) => {}
                    (None, Some(item), 0) => self.carried = Some(item),
                    (None, Some(mut item), 1) => {
                        let n = item.count.div_ceil(2);
                        item.count -= n;
                        self.carried = Some(ItemStack {
                            count: n,
                            ..item.clone()
                        });
                        if item.count > 0 {
                            slots[i] = Some(item)
                        }
                    }
                    (Some(mut held), None, b) if self.ae2_machine_may_place(pos, i, &held) => {
                        let n = self.ae2_machine_limit(pos, i, &held).min(if b == 1 {
                            1
                        } else {
                            held.count
                        });
                        slots[i] = Some(ItemStack {
                            count: n,
                            ..held.clone()
                        });
                        held.count -= n;
                        if held.count > 0 {
                            self.carried = Some(held)
                        }
                    }
                    (Some(mut held), Some(mut item), b)
                        if same(&held, &item) && self.ae2_machine_may_place(pos, i, &held) =>
                    {
                        let n = self
                            .ae2_machine_limit(pos, i, &held)
                            .saturating_sub(item.count)
                            .min(if b == 1 { 1 } else { held.count });
                        item.count += n;
                        held.count -= n;
                        slots[i] = Some(item);
                        if held.count > 0 {
                            self.carried = Some(held)
                        }
                    }
                    (Some(held), Some(item), _)
                        if self.ae2_machine_may_place(pos, i, &held)
                            && held.count <= self.ae2_machine_limit(pos, i, &held) =>
                    {
                        slots[i] = Some(held);
                        self.carried = Some(item)
                    }
                    (held, item, _) => {
                        self.carried = held;
                        slots[i] = item;
                    }
                }
            } else {
                return;
            }
            if let Some(BlockEntity::Ae2(m)) = self.world.block_entities.get_mut(&pos) {
                let n = m.slots.len();
                if slots.len() != n + m.upgrades.len() {
                    return;
                }
                let had_crafting_card = m
                    .upgrades
                    .iter()
                    .flatten()
                    .any(|s| s.item.as_ref() == "ae2:crafting_card");
                m.slots = slots[..n].to_vec();
                m.upgrades = slots[n..].to_vec();
                if had_crafting_card
                    && !m
                        .upgrades
                        .iter()
                        .flatten()
                        .any(|s| s.item.as_ref() == "ae2:crafting_card")
                {
                    super::ae2_interface::mark_card_removed(m);
                }
                super::ae2_interface::wake_interface(m);
                self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
            }
            return;
        }
        if slot >= 36 {
            return;
        }
        if input == Input::QuickMove {
            if let Some(mut held) = self.inventory[slot].take() {
                if let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get_mut(&pos) {
                    if super::ae2_machine_ui::machine_upgrade_quick_insert(machine, &mut held) > 0 {
                        self.inventory[slot] = (held.count > 0).then_some(held);
                        self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
                        return;
                    }
                }
                for i in 0..slots.len() {
                    if !self.ae2_machine_may_place(pos, i, &held) {
                        continue;
                    }
                    let max = self.ae2_machine_limit(pos, i, &held);
                    match &mut slots[i] {
                        Some(item) if same(item, &held) => {
                            let n = max.saturating_sub(item.count).min(held.count);
                            item.count += n;
                            held.count -= n
                        }
                        None => {
                            let n = max.min(held.count);
                            slots[i] = Some(ItemStack {
                                count: n,
                                ..held.clone()
                            });
                            held.count -= n
                        }
                        _ => {}
                    }
                    if held.count == 0 {
                        break;
                    }
                }
                if held.count > 0 {
                    self.inventory[slot] = Some(held);
                }
                if let Some(BlockEntity::Ae2(m)) = self.world.block_entities.get_mut(&pos) {
                    let n = m.slots.len();
                    if slots.len() != n + m.upgrades.len() {
                        return;
                    }
                    let had_crafting_card = m
                        .upgrades
                        .iter()
                        .flatten()
                        .any(|s| s.item.as_ref() == "ae2:crafting_card");
                    m.slots = slots[..n].to_vec();
                    m.upgrades = slots[n..].to_vec();
                    if had_crafting_card
                        && !m
                            .upgrades
                            .iter()
                            .flatten()
                            .any(|s| s.item.as_ref() == "ae2:crafting_card")
                    {
                        super::ae2_interface::mark_card_removed(m);
                    }
                    super::ae2_interface::wake_interface(m);
                    self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
                }
            }
            return;
        }
        if input != Input::Pickup || !(0..=1).contains(&button) {
            return;
        }
        let held = self.carried.take();
        let item = self.inventory[slot].take();
        match (held, item, button) {
            (None, None, _) => {}
            (None, Some(item), 0) => self.carried = Some(item),
            (None, Some(mut item), 1) => {
                let n = item.count.div_ceil(2);
                item.count -= n;
                self.carried = Some(ItemStack {
                    count: n,
                    ..item.clone()
                });
                if item.count > 0 {
                    self.inventory[slot] = Some(item)
                }
            }
            (Some(held), None, 0) => self.inventory[slot] = Some(held),
            (Some(mut held), None, 1) => {
                self.inventory[slot] = Some(ItemStack {
                    count: 1,
                    ..held.clone()
                });
                held.count -= 1;
                if held.count > 0 {
                    self.carried = Some(held)
                }
            }
            (Some(mut held), Some(mut item), b) if same(&held, &item) => {
                let max = self.res.items().stack_max(&item);
                let n = max
                    .saturating_sub(item.count)
                    .min(if b == 1 { 1 } else { held.count });
                item.count += n;
                held.count -= n;
                self.inventory[slot] = Some(item);
                if held.count > 0 {
                    self.carried = Some(held)
                }
            }
            (Some(held), Some(item), _) => {
                self.inventory[slot] = Some(held);
                self.carried = Some(item)
            }
            (held, item, _) => {
                self.carried = held;
                self.inventory[slot] = item
            }
        }
    }
    pub(super) fn lan_ae2_machine_closed(
        &mut self,
        host: &mut super::lan::LanHost,
        conn: u32,
        window: Option<i32>,
    ) -> bool {
        let Some(menu) = host.play.get_mut(&conn).and_then(|p| {
            p.ae2_machine
                .take_if(|m| window.is_none_or(|w| m.window == w))
        }) else {
            return false;
        };
        if let Some(gi) = host.hub.guests.iter().position(|g| g.conn == conn) {
            self.guest_carried_back(host, gi);
            let items: Vec<_> = [menu.link_input, menu.link_output]
                .into_iter()
                .flatten()
                .collect();
            self.drop_guest_items(host, gi, items);
            // A rejected stale SWAP may have predicted an inventory edit even
            // with an empty cursor. Restore the actual player slots on close.
            host.hub.send_inventory(gi);
        }
        true
    }
}

#[cfg(test)]
mod terminal_filter_tests {
    use super::*;
    #[test]
    fn queued_virtual_click_keeps_displayed_key_across_unrelated_revisions() {
        let water=fluid_marker(&ae2_cell_core::ItemKey{item_id:"minecraft:water".into(),components:vec![]});
        let iron=ae2_cell_core::ItemKey{item_id:"minecraft:iron_ingot".into(),components:vec![]};
        let displayed=vec![(iron.clone(),2),(water.clone(),2000)];
        let mut history=std::collections::VecDeque::new();
        history.push_back((7,terminal_view_keys(&displayed,0,45)));
        assert!(terminal_view_revision_matches(&history,7,&[(iron.clone(),1),(water.clone(),1000)],0,45,1));
        assert!(terminal_view_revision_matches(&history,7,&displayed,0,45,10));
        assert!(!terminal_view_revision_matches(&history,7,&[(water,2000),(iron,2)],0,45,1));
        assert!(!terminal_view_revision_matches(&history,8,&displayed,0,45,1));
        assert!(!terminal_view_revision_matches(&history,7,&displayed,0,45,45));
    }
    #[test]
    fn pinned_row_ignores_search_keeps_slots_stable_and_repeats_on_pages() {
        let key=|id:&str|ae2_cell_core::ItemKey{item_id:id.into(),components:Vec::new()};
        let pin=key("minecraft:diamond");
        let mut all=vec![(pin.clone(),3)];
        all.extend((0..20).map(|n|(key(&format!("minecraft:item_{n}")),1)));
        let filtered=all.iter().skip(1).cloned().collect();
        let visible=terminal_pinned_entries(&all,filtered,&[pin.clone()],27);
        assert_eq!(visible[0],(pin.clone(),3));
        assert!(visible[1..9].iter().all(|(key,_)|key.item_id.is_empty()));
        assert_eq!(visible[9].0.item_id,"minecraft:item_0");
        assert_eq!(visible[27],(pin,3));
        assert_eq!(visible[36].0.item_id,"minecraft:item_18");
    }
    #[test]
    fn queued_provider_quick_move_requires_the_same_physical_return_slot() {
        let mut observed=vec![None;9];
        observed[0]=Some(ProviderReturnSlot::Item(ItemStack::new("minecraft:diamond",7)));
        let mut fluid=super::super::ae2_fluids::fluid_key("minecraft:water");
        fluid.components=vec![1,2,3];
        observed[1]=Some(ProviderReturnSlot::Fluid(fluid.clone(),1000));
        let history=std::collections::VecDeque::from([(11,observed.clone())]);
        // A preceding pattern-slot move leaves the requested return slot alone.
        assert!(provider_return_revision_matches(&history,11,0,&observed));
        let mut changed=observed.clone();
        if let Some(ProviderReturnSlot::Item(item))=&mut changed[0] {item.count-=1;}
        assert!(!provider_return_revision_matches(&history,11,0,&changed));
        fluid.components.push(4);changed[1]=Some(ProviderReturnSlot::Fluid(fluid,1000));
        assert!(!provider_return_revision_matches(&history,11,1,&changed));
        assert!(!provider_return_revision_matches(&history,10,0,&observed));
        assert!(!provider_return_revision_matches(&history,11,9,&observed));
    }
    #[test]
    fn provider_return_inventory_keeps_item_components_and_fluid_slots_distinct() {
        use horizon_client::ae2_world::{Machine,MachineKind};
        let mut machine=Machine::new(MachineKind::PatternProvider);
        let mut item=ItemStack::new("minecraft:diamond",3);
        item.data_mut().custom_name=Some("return item".into());
        let mut slots=vec![None;9];
        slots[1]=Some(ProviderReturnSlot::Item(item));
        slots[7]=Some(ProviderReturnSlot::Fluid(super::super::ae2_fluids::fluid_key("minecraft:water"),2500));
        provider_return_store(&mut machine,&slots).unwrap();
        assert_eq!(provider_return_slots(&machine).unwrap(),slots);
        let Some(Tag::List(fluids))=machine.config.get_mut(&"ae2_provider_return_fluids".into()) else {panic!("missing fluid")};
        let Some(Tag::Compound(fluid))=fluids.first_mut() else {panic!("missing fluid entry")};
        fluid.insert("Slot".into(),Tag::Byte(1));
        assert!(provider_return_slots(&machine).is_err(),"an item and fluid cannot claim the same physical slot");
        let mut invalid=slots;invalid[7]=Some(ProviderReturnSlot::Fluid(super::super::ae2_fluids::fluid_key("minecraft:water"),4001));
        assert!(provider_return_store(&mut machine,&invalid).is_err());
    }
    #[test]
    fn wireless_gesture_snapshot_allows_battery_ticks_but_rejects_other_changes() {
        let mut inventory = vec![None; 36];
        inventory[0] = Some(ItemStack::new("ae2:wireless_crafting_terminal", 1));
        inventory[1] = Some(ItemStack::new("minecraft:stone", 3));
        let mut after = inventory.clone();
        after[0].as_mut().unwrap().data_mut().set_other("minecraft:custom_data", Some(Tag::compound([
            ("ae2_wireless_energy", Tag::Double(42.0f64.to_bits()))
        ])));
        assert!(wireless_unchanged_inventory(0, &inventory, &after, &None, &None));
        after[1].as_mut().unwrap().count = 2;
        assert!(!wireless_unchanged_inventory(0, &inventory, &after, &None, &None));
        let offhand = Some(ItemStack::new("ae2:wireless_terminal", 1));
        let mut changed = offhand.clone(); changed.as_mut().unwrap().damage = 1;
        assert!(wireless_unchanged_inventory(40, &inventory, &inventory, &offhand, &changed));
        assert!(!wireless_unchanged_inventory(0, &inventory, &inventory, &offhand, &changed));
    }
    #[test]
    fn number_entry_priority_expression_and_steps() {
        assert!(priority_confirmable("=2^3^4"));assert!(!priority_confirmable("1/3"));
        assert_eq!(priority_step("1+2",1,false).as_deref(),Some("13"));
        assert_eq!(super::super::ae2_number_entry::number_value("=3*8",1,i64::from(i32::MIN),i64::MAX,'.').and_then(|v|i32::try_from(v).ok()),Some(24));
        assert_eq!(super::super::ae2_number_entry::number_value("2147483648",1,i64::from(i32::MIN),i64::MAX,'.').and_then(|v|i32::try_from(v).ok()),None);
    }
    #[test]
    fn encoding_icon_uses_effective_component_stack_limit() {
        let mut item=ItemStack::new("minecraft:diamond",6);
        item.data_mut().set_other("minecraft:max_stack_size",Some(Tag::Int(5)));
        let max=super::super::ae2_formation::max_stack(&item,64);
        assert_eq!(max,5);
        assert_eq!(encoding_item_icon(&item,max).unwrap().item.as_ref(),"ae2:wrapped_generic_stack");
    }
    #[test]
    fn workbench_ghost_revision_ignores_cursor_refresh_but_checks_page_and_mode() {
        let history=std::collections::VecDeque::from([WorkbenchRevision{state:1,page:0,keep:false},WorkbenchRevision{state:2,page:0,keep:false},WorkbenchRevision{state:3,page:1,keep:false}]);
        assert!(workbench_revision_matches(&history,1,0,false));
        assert!(!workbench_revision_matches(&history,1,1,false));
        assert!(!workbench_revision_matches(&history,1,0,true));
        assert!(!workbench_revision_matches(&history,0,0,false));
    }
    #[test]
    fn encoding_bucket_filter_reads_the_fluid_without_mutating_a_named_carrier() {
        let mut bucket=ItemStack::new("minecraft:water_bucket",1);
        bucket.data_mut().custom_name=Some("Marked bucket".into());
        let before=bucket.clone();
        let value=encoding_emptying_action(Some(&bucket),1).unwrap();
        assert_eq!(value.key,super::super::ae2_fluids::fluid_key("minecraft:water"));
        assert_eq!(value.amount,1000);assert_eq!(bucket,before);
        assert!(encoding_emptying_action(Some(&bucket),0).is_none());
    }
    #[test]
    fn priority_editor_steps_follow_source_boundaries_and_special_one() {
        assert_eq!(priority_step("1",1,false).as_deref(),Some("10"));
        assert_eq!(priority_step("1",1,true).as_deref(),Some("16"));
        assert_eq!(priority_step("-2147483648",4,false).as_deref(),Some("-2147483648"));
        assert_eq!(priority_step("2147483647",0,false).as_deref(),Some("2147483648"));
        assert_eq!(priority_step("9999999999999999",3,false).unwrap().len(),16);
        assert!("2147483648".parse::<i32>().is_err());assert_eq!("-2147483648".parse::<i32>(),Ok(i32::MIN));
        assert!(priority_confirmable("2147483648"));assert!(priority_confirmable("-2147483648"));
        assert!(!priority_confirmable("-2147483649"));assert!(!priority_confirmable("-"));
        let old=priority_transport_value(7,"-37");assert_eq!(priority_transport_for(&old,7),Some("-37"));
        assert_eq!(priority_transport_for(&old,8),None,"a prior editor cannot update its replacement");
        assert!(priority_transport_parse(&horizon_client::clipboard::filter_text(&old,false)).is_none(),"ordinary editable text cannot use the internal envelope");
        assert!(priority_transport_parse("\u{1f}ae2p:not-a-token:4").is_none());assert!(priority_transport_value(u64::MAX,"-2147483648").len()<=50);
        assert_eq!(priority_step("-",0,false).as_deref(),Some("1"));assert_eq!(priority_step("0",8,false),None);
        assert!(priority_confirmable("=2^3^4"));assert!(!priority_confirmable("1/3"));
        assert_eq!(priority_step("1+2",1,false).as_deref(),Some("13"));
    }
    #[test]
    fn chest_terminal_settings_preserve_other_state_and_cycle_both_directions() {
        use horizon_client::ae2_world::{Machine, MachineKind};
        let mut chest = Machine::new(MachineKind::Chest);
        chest.config.insert("priority".into(), Tag::Int(17));
        chest.config.insert("sort_by".into(), Tag::String("invalid".into()));
        assert_eq!(chest_terminal_settings(&chest), ChestTerminalSettings { sort: 0, view: 0, descending: false });
        assert!(chest_terminal_setting_click(&mut chest, 46, 1));
        assert_eq!(chest.config.get(&"sort_by".into()), Some(&Tag::String("mod".into())));
        assert!(chest_terminal_setting_click(&mut chest, 47, 1));
        assert_eq!(chest.config.get(&"view_mode".into()), Some(&Tag::String("craftable".into())));
        assert!(chest_terminal_setting_click(&mut chest, 51, 0));
        assert_eq!(chest_terminal_settings(&chest), ChestTerminalSettings { sort: 2, view: 2, descending: true });
        assert_eq!(chest.config.get(&"priority".into()), Some(&Tag::Int(17)));
        let unchanged=chest.clone();assert!(!chest_terminal_setting_click(&mut chest, 46, 2));assert_eq!(chest, unchanged);
        let mut other=Machine::new(MachineKind::Drive);assert!(!chest_terminal_setting_click(&mut other, 46, 0));
        let original=ChestTerminalSettings{sort:0,view:0,descending:false};let updated=chest_terminal_settings(&chest);
        let revisions=std::collections::VecDeque::from([(4,original),(5,original),(6,updated)]);
        assert!(chest_revision_matches(&revisions,4,original),"unchanged older views stay usable");
        assert!(!chest_revision_matches(&revisions,4,updated),"queued settings click cannot overwrite a newer Card import");
        assert!(chest_revision_matches(&revisions,6,updated));assert!(!chest_revision_matches(&revisions,3,updated));
    }
    #[test]
    fn terminal_toolbox_rejects_preexisting_overstack_before_staging() {
        use horizon_client::ae2_network_tool::Inventory;
        let mut pocket = Inventory { slots: std::array::from_fn(|_| None) };
        pocket.slots[0] = Some(ItemStack::new("ae2:speed_card",64));
        // Existing non-card contents remain readable and extractable.
        pocket.slots[1] = Some(ItemStack::new("minecraft:diamond",7));
        assert!(toolbox_staging_safe(&pocket, |_| 64));
        pocket.slots[0].as_mut().unwrap().count = 65;
        assert!(!toolbox_staging_safe(&pocket, |_| 64));
        pocket.slots[0].as_mut().unwrap().count = 64;
        pocket.slots[1].as_mut().unwrap().count = 100;
        assert!(!toolbox_staging_safe(&pocket, |_| 200));
    }
    #[test]
    fn toolbox_wireless_slots_do_not_require_a_view_cell_panel() {
        let wireless=terminal_visible_slots(false,true);
        assert!((365..374).all(|slot|wireless.contains(&slot)));
        assert!((360..365).all(|slot|!wireless.contains(&slot)));
        let physical=terminal_visible_slots(true,true);
        assert!((360..374).all(|slot|physical.contains(&slot)));
        let no_toolbox=terminal_visible_slots(true,false);
        assert!((360..365).all(|slot|no_toolbox.contains(&slot)));
        assert!((365..374).all(|slot|!no_toolbox.contains(&slot)));
    }
    #[test]
    fn toolbox_wireless_owners_and_mixed_drag_are_protected() {
        use horizon_client::{banner::Banners, brewing::Brewing, crafting::Recipes,
            items::Items, menu::{Env, Kind, Menu}};
        assert!(wireless_toolbox_swap_refused(40,None,1));
        assert!(wireless_toolbox_swap_refused(0,Some(40),1));
        assert!(wireless_toolbox_swap_refused(0,Some(8),8));
        assert!(wireless_toolbox_swap_refused(0,Some(8),0));
        assert!(!wireless_toolbox_swap_refused(0,Some(8),3));
        assert_eq!(toolbox_drag_slot(95,true,8,Some(0)),Some(0));
        assert_eq!(toolbox_drag_slot(27,true,8,Some(0)),Some(9));
        assert_eq!(toolbox_drag_slot(81,true,8,Some(0)),None);
        assert_eq!(toolbox_drag_slot(89,true,8,Some(0)),None);
        assert_eq!(toolbox_drag_slot(0,true,8,Some(0)),None);
        assert!(toolbox_pickup_locked(9,8,Some(0)));
        assert!(toolbox_pickup_locked(17,8,Some(0)));
        assert!(!toolbox_pickup_locked(0,8,Some(0)));
        assert!(!toolbox_pickup_locked(45,8,Some(0)));
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../.local/resources/26.3");
        assert!(root.join("items.json").exists(), "native test resources are required");
        let items=Items::catalog(&root).unwrap(); let recipes=Recipes::load(&root,&items).unwrap();
        let brewing=Brewing::load(&root,&items); let banners=Banners::load(&root);
        let stations=horizon_client::stations::StationRecipes::load(&root,&items);
        let enchantments=horizon_client::enchant::Enchantments::default();
        let names=horizon_client::creative::Names::default();
        let env=Env{items:&items,recipes:&recipes,brewing:&brewing,banners:&banners,
            station_recipes:&stations,enchantments:&enchantments,names:&names,trade:None,maps:None};
        let mut menu=Menu::new(Kind::Chest(2));menu.network_tool_slot=Some(8);menu.network_tool_main_limit=Some(9);
        menu.set_player_item(0,Some(ItemStack::new("ae2:wireless_crafting_terminal",1)));
        menu.set_player_item(8,Some(ItemStack::new("ae2:network_tool",1)));
        menu.carried=Some(ItemStack::new("ae2:speed_card",6));
        menu.click(&env,-999,0,Input::QuickCraft).unwrap();
        for slot in [95,27,81,89,0] {if let Some(index)=toolbox_drag_slot(slot,true,8,Some(0)) {menu.click(&env,index,1,Input::QuickCraft).unwrap();}}
        menu.click(&env,-999,2,Input::QuickCraft).unwrap();
        assert_eq!(menu.slot(0).map(|s|s.count),Some(3));assert_eq!(menu.slot(9).map(|s|s.count),Some(3));
        assert!(menu.carried.is_none());assert_eq!(menu.player_item(0).map(|s|s.item.as_ref()),Some("ae2:wireless_crafting_terminal"));
        assert_eq!(menu.player_item(8).map(|s|s.item.as_ref()),Some("ae2:network_tool"));
    }
    #[test]
    fn terminal_toolbox_menu_respects_source_slot_rules() {
        use horizon_client::{banner::Banners, brewing::Brewing, crafting::Recipes,
            items::Items, menu::{Env, Kind, Menu}};
        let root=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../.local/resources/26.3");
        assert!(root.join("items.json").exists(), "native test resources are required");
        let items=Items::catalog(&root).unwrap(); let recipes=Recipes::load(&root,&items).unwrap();
        let brewing=Brewing::load(&root,&items); let banners=Banners::load(&root);
        let stations=horizon_client::stations::StationRecipes::load(&root,&items);
        let enchantments=horizon_client::enchant::Enchantments::default();
        let names=horizon_client::creative::Names::default();
        let env=Env{items:&items,recipes:&recipes,brewing:&brewing,banners:&banners,
            station_recipes:&stations,enchantments:&enchantments,names:&names,trade:None,maps:None};
        let mut menu=Menu::new(Kind::Chest(2));
        menu.network_tool_slot=Some(8);menu.network_tool_main_limit=Some(9);
        menu.set_player_item(8,Some(ItemStack::new("ae2:network_tool",1)));
        // Chest(2) has 18 main slots, 27 main-inventory slots, then hotbar.
        menu.click(&env,53,0,Input::Pickup).unwrap();
        assert!(menu.carried.is_none());assert!(menu.player_item(8).is_some());
        menu.creative=true;menu.click(&env,53,2,Input::Clone).unwrap();
        assert_eq!(menu.carried.as_ref().map(|s|s.item.as_ref()),Some("ae2:network_tool"));
        assert!(menu.player_item(8).is_some());menu.carried=None;menu.creative=false;
        menu.click(&env,9,8,Input::Swap).unwrap();
        assert!(menu.player_item(8).is_none());
        assert_eq!(menu.slot(9).map(|s|s.item.as_ref()),Some("ae2:network_tool"));
        menu.set_slot(&env,9,None);menu.carried=Some(ItemStack::new("minecraft:diamond",3));
        menu.click(&env,0,0,Input::Pickup).unwrap();
        assert!(menu.slot(0).is_none());assert_eq!(menu.carried.as_ref().map(|s|s.count),Some(3));
        menu.click(&env,9,0,Input::Pickup).unwrap();
        assert_eq!(menu.slot(9).map(|s|s.count),Some(3));assert!(menu.carried.is_none());
        menu.network_tool_slot=Some(40);
        menu.set_player_item(40,Some(ItemStack::new("ae2:network_tool",1)));
        menu.set_player_item(0,Some(ItemStack::new("minecraft:cobblestone",2)));
        menu.set_slot(&env,10,None);menu.click(&env,10,0,Input::Swap).unwrap();
        assert!(menu.slot(10).is_none());assert_eq!(menu.player_item(0).map(|s|s.count),Some(2));
    }
    #[test]
    fn hand_menu_dispatch_skips_non_menu_blocks_and_keeps_real_menus() {
        use horizon_client::ae2_world::MachineKind as K;
        for kind in [K::QuantumRing,K::EnergyCell,K::Controller,K::Charger] {assert!(!ae2_hand_menu_kind(kind));}
        for kind in [K::QuantumLink,K::Chest,K::Drive,K::SkyStoneChest,K::SmoothSkyStoneChest] {assert!(ae2_hand_menu_kind(kind));}
    }
    #[test]
    fn encoding_revisions_distinguish_refreshes_from_changed_recipe() {
        use horizon_client::ae2_world::{Machine,MachineKind};
        let original=Machine::new(MachineKind::PatternEncodingTerminal);
        let mut changed=original.clone();let mut recipe=super::super::ae2_encoding::EncodingState::default();recipe.inputs[0]=Some(ItemStack::new("minecraft:stone",1));recipe.save(&mut changed).unwrap();
        let mut blank=original.clone();blank.slots[0]=Some(ItemStack::new("ae2:blank_pattern",1));
        let history=std::collections::VecDeque::from([(4,Some(original.clone()),0),(5,Some(original.clone()),0),(6,Some(changed.clone()),0)]);
        assert!(encoding_revision_matches(&history,4,Some(&original),0,None),"older unchanged views remain usable");
        assert!(encoding_revision_matches(&history,4,Some(&blank),0,None),"physical blank insertion does not change the viewed recipe");
        assert!(!encoding_revision_matches(&history,4,Some(&changed),0,Some(0)),"a queued editor open cannot bind a different ghost");
        assert!(encoding_revision_matches(&history,4,Some(&changed),0,Some(3)),"an editor open tolerates a different slot changing");
        assert!(!encoding_revision_matches(&history,4,Some(&changed),0,Some(13)),"recipe controls require a current recipe");
        assert!(!encoding_revision_matches(&history,4,Some(&changed),1,Some(3)),"queued clicks cannot cross processing pages");
        assert!(encoding_revision_matches(&history,6,Some(&changed),0,None));
        assert!(!encoding_revision_matches(&history,3,Some(&changed),0,Some(3)),"expired revisions fail closed");
        assert!(encoding_drag_revision_matches(&history,4,Some(&changed),0),"ordered fake drags apply to the current ghost");
        assert!(!encoding_drag_revision_matches(&history,4,Some(&changed),1),"fake drags cannot cross pages");
        let mut other_mode=changed.clone();let mut processing=recipe.clone();processing.processing=true;processing.save(&mut other_mode).unwrap();
        assert!(!encoding_drag_revision_matches(&history,4,Some(&other_mode),0),"fake drags cannot cross modes");
        assert!(!encoding_drag_revision_matches(&history,3,Some(&changed),0),"expired drag revisions fail closed");

    }
    #[test]
    fn search_sort_and_craftable_filter_keep_the_right_item_keys() {
        let key = |id: &str| ae2_cell_core::ItemKey {
            item_id: id.into(),
            components: vec![],
        };
        let entries = vec![
            (key("minecraft:oak_planks"), 20),
            (key("minecraft:oak_log"), 5),
            (key("minecraft:stick"), 0),
        ];
        let craftable = vec![key("minecraft:oak_log"), key("minecraft:stick")];
        let names = horizon_client::creative::Names::default();
        let matches=|key:&ae2_cell_core::ItemKey,query:&str| {
            let (mod_id,path)=key.item_id.split_once(':').unwrap_or(("minecraft",key.item_id.as_str()));
            terminal_search_matches(query,&SearchFacts{id:&key.item_id,name:path,mod_id,mod_name:mod_id,tooltip:path,tags:&[]})
        };
        let display=|key:&ae2_cell_core::ItemKey|names.display_item(&key.item_id);
        let result = terminal_filter_entries(&entries, &craftable, 1, true, 2, "*minecraft:", &display, &matches);
        assert_eq!(
            result,
            vec![(key("minecraft:oak_log"), 5), (key("minecraft:stick"), 0)]
        );
        let result = terminal_filter_entries(&entries, &craftable, 0, false, 1, "planks", &display, &matches);
        assert_eq!(result, vec![(key("minecraft:oak_planks"), 20)]);
        let mixed=vec![(key("minecraft:apple"),1),(key("ae2:zeta"),1),(key("ae2:alpha"),1)];
        let result=terminal_filter_entries(&mixed,&[],2,false,0,"",&display,&matches);
        assert_eq!(result,vec![(key("ae2:alpha"),1),(key("ae2:zeta"),1),(key("minecraft:apple"),1)]);
    }
    #[test]
    fn wireless_type_filter_keeps_item_and_fluid_keys_distinct() {
        let item=ae2_cell_core::ItemKey{item_id:"minecraft:oak_log".into(),components:vec![]};
        let fluid=fluid_marker(&ae2_cell_core::ItemKey{item_id:"minecraft:water".into(),components:vec![]});
        let all=vec![(item.clone(),2),(fluid.clone(),1000)];
        assert_eq!(terminal_key_types(&all,(true,false)),vec![(item.clone(),2)]);
        assert_eq!(terminal_key_types(&all,(false,true)),vec![(fluid.clone(),1000)]);
        assert!(terminal_key_types(&all,(false,false)).is_empty());
        let pinned=terminal_pinned_entries(&all,terminal_key_types(&all,(true,false)),&[fluid.clone()],45);
        assert_eq!(pinned[0],(fluid,1000),"pinned keys remain visible independent of ordinary filters");
    }
    #[test]
    fn source_search_operators_use_key_name_tags_and_tooltip() {
        let tags=vec!["minecraft:logs".to_owned(),"c:wooden_logs".to_owned()];
        let facts=SearchFacts{id:"minecraft:oak_log",name:"Ancient Timber",mod_id:"minecraft",mod_name:"Minecraft",tooltip:"Ancient Timber\nWood Grain",tags:&tags};
        for query in ["ancient timber","@mine timber","*oak_log","#minecraft:logs","$woodgrain","missing | timber","@minecraft #wooden"] {
            assert!(terminal_search_matches(query,&facts),"{query}");
        }
        for query in ["oak_log","@ae2","#minecraft:planks","$unknown","ancient missing","missing|unknown","|"] {
            assert!(!terminal_search_matches(query,&facts),"{query}");
        }
        assert!(terminal_search_matches("",&facts));
        assert!(terminal_search_matches("timber|",&facts));
        assert_eq!(search_language_code("pt_br"),"pt_br");
        assert_eq!(search_language_code("../en_us"),"en_us");
    }
    #[test]
    fn terminal_sort_uses_display_names_and_fluid_units() {
        let item=ae2_cell_core::ItemKey{item_id:"minecraft:apple".into(),components:vec![]};
        let fluid=fluid_marker(&ae2_cell_core::ItemKey{item_id:"minecraft:water".into(),components:vec![]});
        let entries=vec![(item.clone(),2),(fluid.clone(),1500)];
        let name=|key:&ae2_cell_core::ItemKey|if *key==item {"Zulu".into()} else {"Alpha".into()};
        let matches=|_:&ae2_cell_core::ItemKey,_:&str|true;
        assert_eq!(terminal_filter_entries(&entries,&[],0,false,0,"",&name,&matches),vec![(fluid.clone(),1500),(item.clone(),2)]);
        assert_eq!(terminal_filter_entries(&entries,&[],1,false,0,"",&name,&matches),vec![(fluid,1500),(item,2)]);
    }
}

// Source Settings.INSCRIBER_SEPARATE_SIDES and INSCRIBER_INPUT_CAPACITY.
/// Source AppEngSlot limits combine each inventory's capacity with the
/// item's effective maximum. Inscriber's three input capacities are shared
/// by its configurable setting; its output retains the default capacity.
fn machine_slot_limit(m:&horizon_client::ae2_world::Machine,slot:usize,item_max:u32)->u32 {
    use horizon_client::ae2_world::MachineKind as K;
    let capacity=if slot>=m.slots.len() {1} else {match m.kind {
        K::Drive|K::CellWorkbench|K::QuantumLink|K::SpatialIoPort|K::IoPort|K::Condenser|K::MolecularAssembler=>1,
        K::Inscriber if slot<3=>match m.config.get(&"inscriber_input_capacity".into()) {
            Some(Tag::String(value)) if value.to_string().ok().as_deref()==Some("ONE")=>1,
            Some(Tag::String(value)) if value.to_string().ok().as_deref()==Some("FOUR")=>4,
            _=>64,
        },
        _=>64,
    }};
    item_max.min(capacity)
}
fn inscriber_setting_click(m:&mut horizon_client::ae2_world::Machine, slot:usize, reverse:bool)->bool {
    if m.kind!=horizon_client::ae2_world::MachineKind::Inscriber{return false}
    let (key,values):(&str,&[&str])=match slot {
        324=>("auto_export",&["NO","YES"]),
        325=>("inscriber_separate_sides",&["NO","YES"]),
        326=>("inscriber_input_capacity",&["SIXTY_FOUR","ONE","FOUR"]),
        _=>return false,
    };
    let current=match m.config.get(&key.into()) {
        None=>values[0],
        Some(Tag::String(s))=>match s.to_string().ok().and_then(|s|values.iter().copied().find(|v|*v==s)) {Some(value)=>value,None=>return false},
        _=>return false,
    };
    let index=values.iter().position(|v|*v==current).unwrap();
    let next=values[(index+if reverse {values.len()-1}else{1})%values.len()];
    m.config.insert(key.into(),Tag::String(next.into()));true
}
impl Game<'_> {
    fn ae2_inscriber_setting(&mut self, pos:(i32,i32,i32), slot:usize, reverse:bool)->bool {
        let Some(horizon_client::containers::BlockEntity::Ae2(m))=self.world.block_entities.get_mut(&pos) else{return false};
        if !inscriber_setting_click(m,slot,reverse){return false}
        self.world.modified.insert((pos.0>>4,pos.2>>4));true
    }
}
#[cfg(test)]
mod inscriber_setting_tests {
    use super::*;
    #[test]
    fn source_machine_slot_limits_follow_inscriber_setting_and_assembler_inventory() {
        use horizon_client::ae2_world::{Machine,MachineKind};
        let mut inscriber=Machine::new(MachineKind::Inscriber);
        for slot in 0..3 {assert_eq!(machine_slot_limit(&inscriber,slot,99),64);}
        inscriber.config.insert("inscriber_input_capacity".into(),Tag::String("ONE".into()));
        for slot in 0..3 {assert_eq!(machine_slot_limit(&inscriber,slot,64),1);}
        assert_eq!(machine_slot_limit(&inscriber,3,64),64);
        inscriber.config.insert("inscriber_input_capacity".into(),Tag::String("FOUR".into()));
        for slot in 0..3 {assert_eq!(machine_slot_limit(&inscriber,slot,64),4);}
        assert_eq!(machine_slot_limit(&inscriber,2,2),2);
        let assembler=Machine::new(MachineKind::MolecularAssembler);
        for slot in 0..assembler.slots.len() {assert_eq!(machine_slot_limit(&assembler,slot,64),1);}
    }
    #[test]
    fn physical_inscriber_settings_cycle_source_values_and_reject_corrupt_state() {
        use horizon_client::ae2_world::{Machine,MachineKind};
        let mut machine=Machine::new(MachineKind::Inscriber);
        assert!(inscriber_setting_click(&mut machine,324,false));
        assert_eq!(machine.config.get(&"auto_export".into()),Some(&Tag::String("YES".into())));
        assert!(inscriber_setting_click(&mut machine,325,false));
        assert_eq!(machine.config.get(&"inscriber_separate_sides".into()),Some(&Tag::String("YES".into())));
        assert!(inscriber_setting_click(&mut machine,326,false));
        assert_eq!(machine.config.get(&"inscriber_input_capacity".into()),Some(&Tag::String("ONE".into())));
        assert!(inscriber_setting_click(&mut machine,326,true));
        assert_eq!(machine.config.get(&"inscriber_input_capacity".into()),Some(&Tag::String("SIXTY_FOUR".into())));
        machine.config.insert("inscriber_input_capacity".into(),Tag::Int(999));
        assert!(!inscriber_setting_click(&mut machine,326,false));
        assert_eq!(machine.config.get(&"inscriber_input_capacity".into()),Some(&Tag::Int(999)));
        assert!(!inscriber_setting_click(&mut Machine::new(MachineKind::Drive),325,false));
    }
}
