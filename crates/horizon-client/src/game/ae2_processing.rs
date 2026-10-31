//! Experimental AE2 machine processing. Source: AE2 InscriberBlockEntity,
//! ChargerBlockEntity, VibrationChamberBlockEntity and their data generators,
//! commit db17504a86128fdf3dae31f5fb7a112a646e0b93, LGPL-3.0-or-later.
use super::Game;
use horizon_client::{
    ae2_world::{Machine, MachineKind},
    containers::BlockEntity,
    item_data,
    items::{ItemStack, Items},
};
use horizon_core::random::RandomSource;
use horizon_world::nbt::Tag;
use std::sync::Arc;
use super::ae2_tick_tracker::{Tracker, Modulation};

struct ProcessorOwner {
    tracker: Tracker,
    input: String,
    service: Option<u64>,
}
impl ProcessorOwner {
    /// TickManagerService for one processor: a changed inventory wakes a
    /// sleeping device (onChangeInventory, wakeDevice), then its tracker
    /// decides. When due, gives ticksSinceLastCall.
    fn due(&mut self, now: u64, input: String) -> Option<u64> {
        if self.input != input { self.input = input; self.tracker.wake(); }
        self.tracker.due(now).then(|| now.saturating_sub(self.tracker.last()))
    }
}
#[derive(Default)]
pub(super) struct ProcessorTicks {
    owners: std::collections::BTreeMap<u64, ProcessorOwner>,
}

fn processing_input(m: &Machine) -> String {
    // Observe physical inventory changes, including automation and menu paths.
    // Internal progress/power changes must not wake a sleeping device.
    format!("{:?}|{:?}|{:?}", m.slots, m.upgrades, m.config.get(&"auto_export".into()))
}

fn inscriber_auto_export_work(m:&Machine)->bool {
    matches!(m.config.get(&"auto_export".into()),Some(Tag::String(s)) if s.to_string().ok().as_deref()==Some("YES"))
        && m.slots.get(3).and_then(Option::as_ref).is_some_and(|s|s.count>0)
}

/// Private machine buffers are separate from network storage. Source AE2
/// AEBasePoweredBlockEntity and the individual machine constructors.
pub(super) fn ae2_private_power_capacity(machine: &Machine) -> Option<f64> {
    machine.private_power_capacity()
}

pub(super) fn ae2_private_power_face(
    kind: MachineKind,
    state: &horizon_world::chunk::BlockState,
    face: horizon_core::geometry::Direction,
) -> bool {
    use horizon_core::geometry::Direction;
    if kind == MachineKind::Chest { return true; }
    let facing = match state.properties.get("facing").map(String::as_str) {
        Some("down") => Direction::Down, Some("up") => Direction::Up,
        Some("north") => Direction::North, Some("south") => Direction::South,
        Some("west") => Direction::West, Some("east") => Direction::East,
        _ => return false,
    };
    match kind {
        MachineKind::Inscriber | MachineKind::Charger => face != facing,
        MachineKind::GrowthAccelerator => face == facing || face == facing.opposite(),
        _ => false,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Recipe {
    middle: &'static str,
    top: Option<&'static str>,
    bottom: Option<&'static str>,
    result: &'static str,
    consume_plates: bool,
}
const RECIPES: &[Recipe] = &[
    Recipe {
        middle: "minecraft:iron_block",
        top: Some("ae2:silicon_press"),
        bottom: None,
        result: "ae2:silicon_press",
        consume_plates: false,
    },
    Recipe {
        middle: "#c:silicon",
        top: Some("ae2:silicon_press"),
        bottom: None,
        result: "ae2:printed_silicon",
        consume_plates: false,
    },
    Recipe {
        middle: "ae2:certus_quartz_crystal",
        top: Some("ae2:calculation_processor_press"),
        bottom: None,
        result: "ae2:printed_calculation_processor",
        consume_plates: false,
    },
    Recipe {
        middle: "#c:gems/diamond",
        top: Some("ae2:engineering_processor_press"),
        bottom: None,
        result: "ae2:printed_engineering_processor",
        consume_plates: false,
    },
    Recipe {
        middle: "#c:ingots/gold",
        top: Some("ae2:logic_processor_press"),
        bottom: None,
        result: "ae2:printed_logic_processor",
        consume_plates: false,
    },
    Recipe {
        middle: "minecraft:redstone",
        top: Some("ae2:printed_calculation_processor"),
        bottom: Some("ae2:printed_silicon"),
        result: "ae2:calculation_processor",
        consume_plates: true,
    },
    Recipe {
        middle: "minecraft:redstone",
        top: Some("ae2:printed_engineering_processor"),
        bottom: Some("ae2:printed_silicon"),
        result: "ae2:engineering_processor",
        consume_plates: true,
    },
    Recipe {
        middle: "minecraft:redstone",
        top: Some("ae2:printed_logic_processor"),
        bottom: Some("ae2:printed_silicon"),
        result: "ae2:logic_processor",
        consume_plates: true,
    },
    Recipe {
        middle: "minecraft:iron_block",
        top: Some("ae2:calculation_processor_press"),
        bottom: None,
        result: "ae2:calculation_processor_press",
        consume_plates: false,
    },
    Recipe {
        middle: "minecraft:iron_block",
        top: Some("ae2:engineering_processor_press"),
        bottom: None,
        result: "ae2:engineering_processor_press",
        consume_plates: false,
    },
    Recipe {
        middle: "minecraft:iron_block",
        top: Some("ae2:logic_processor_press"),
        bottom: None,
        result: "ae2:logic_processor_press",
        consume_plates: false,
    },
    Recipe {
        middle: "#c:gems/fluix",
        top: None,
        bottom: None,
        result: "ae2:fluix_dust",
        consume_plates: false,
    },
    Recipe {
        middle: "#c:gems/certus_quartz",
        top: None,
        bottom: None,
        result: "ae2:certus_quartz_dust",
        consume_plates: false,
    },
    Recipe {
        middle: "ae2:sky_stone_block",
        top: None,
        bottom: None,
        result: "ae2:sky_dust",
        consume_plates: false,
    },
    Recipe {
        middle: "minecraft:ender_pearl",
        top: None,
        bottom: None,
        result: "ae2:ender_dust",
        consume_plates: false,
    },
];
fn ingredient(items: &Items, expected: Option<&str>, stack: Option<&ItemStack>) -> bool {
    match (expected, stack) {
        (None, None) => true,
        (Some(e), Some(s)) if s.count > 0 => {
            if let Some(tag) = e.strip_prefix('#') {
                items
                    .item_tags
                    .get(tag)
                    .is_some_and(|members| members.contains(s.item.as_ref()))
            } else {
                s.item.as_ref() == e
            }
        }
        _ => false,
    }
}
fn recipe(machine: &Machine, items: &Items) -> Option<Recipe> {
    let [top, bottom, middle, _] = machine.slots.as_slice() else {
        return None;
    };
    RECIPES.iter().copied().find(|r| {
        ingredient(items, Some(r.middle), middle.as_ref())
            && ((ingredient(items, r.top, top.as_ref())
                && ingredient(items, r.bottom, bottom.as_ref()))
                || (ingredient(items, r.top, bottom.as_ref())
                    && ingredient(items, r.bottom, top.as_ref())))
    })
}
// Resolve the active datapack's source-defined inscriber recipes rather than
// baking the shipped recipes into the engine. Unsupported ingredients fail closed.
pub(super) fn json_ingredient(items: &Items, value: Option<&serde_json::Value>, stack: Option<&ItemStack>) -> bool {
    match (value, stack) {
        (None, None) => true,
        (Some(serde_json::Value::Array(values)), Some(s)) => values.iter().any(|v| json_ingredient(items, Some(v), Some(s))),
        (Some(v), Some(s)) if s.count > 0 => {
            if let Some(id) = v.get("item").and_then(|x| x.as_str()).or_else(||v.as_str().filter(|id|!id.starts_with('#'))) { return s.item.as_ref() == id; }
            let tag = v.get("tag").and_then(|x|x.as_str()).or_else(||v.as_str().and_then(|x|x.strip_prefix('#')));
            tag.and_then(|tag|items.item_tags.get(tag)).is_some_and(|members|members.contains(s.item.as_ref()))
        }
        _ => false,
    }
}
/// Source dynamic name-press recipe: preserve the middle item's components,
/// replace its custom name, and retain the physical presses.
fn append_component(root: &mut Tag, sibling: Tag) {
    if let Tag::Compound(map) = root {
        let extra = map.entry("extra".into()).or_insert_with(|| Tag::List(Vec::new()));
        if let Tag::List(parts) = extra {
            parts.push(sibling);
        }
    }
}

fn component_root(component: &Tag) -> Tag {
    match component {
        Tag::Compound(_) => component.clone(),
        // ComponentSerialization decodes a list as its first component with
        // the remaining components appended as siblings. In particular, the
        // first component's style is inherited by those siblings.
        Tag::List(parts) => {
            let Some((first, siblings)) = parts.split_first() else {
                return Tag::compound([("text", Tag::String("".into()))]);
            };
            let mut root = component_root(first);
            for sibling in siblings {
                append_component(&mut root, sibling.clone());
            }
            root
        }
        literal => Tag::compound([("text", literal.clone())]),
    }
}

fn append_press_name(first: &Tag, second: &Tag) -> Tag {
    let mut root = component_root(first);
    append_component(&mut root, Tag::String(" ".into()));
    append_component(&mut root, second.clone());
    root
}
fn name_press_recipe(machine:&Machine)->Option<ItemStack> {
    let [top,bottom,middle,_]=machine.slots.as_slice() else{return None};
    let is_press=|s:&Option<ItemStack>|s.as_ref().is_some_and(|v|v.item.as_ref()=="ae2:name_press");
    if !(is_press(top) && (bottom.is_none() || is_press(bottom)) || top.is_none() && is_press(bottom)){return None;}
    let mut output=middle.as_ref()?.clone();output.count=1;
    let names:Vec<Tag>=[top,bottom].into_iter().filter_map(|slot|slot.as_ref())
        .filter_map(|press|press.data())
        // Older native saves used a custom-data wrapper; keep them readable.
        .filter_map(|data|data.other("ae2:name_press_name").or_else(||data.other("minecraft:custom_data").and_then(|tag|tag.get("ae2:name_press_name"))))
        .filter(|tag|matches!(tag,Tag::String(_) | Tag::Compound(_) | Tag::List(_))).cloned().collect();
    let name=match names.as_slice() {[]=>None,[only]=>Some(only.clone()),[first,second]=>Some(append_press_name(first,second)),_=>unreachable!()};
    output.set_custom_name_component(name);Some(output)
}
/// InscriberRecipe.CODEC uses ItemStack.CODEC, including its component patch.
fn inscriber_result(value: &serde_json::Value) -> Option<ItemStack> {
    value.get("id")?.as_str()?;
    let count = match value.get("count") { Some(count) => count.as_u64()?, None => 1 };
    if count == 0 || count > u32::MAX as u64 || value.get("components").is_some_and(|v| !v.is_object()) {
        return None;
    }
    Some(horizon_client::crafting::Template::parse(value)?.create())
}
fn live_inscriber_recipe(machine:&Machine, items:&Items, recipes:&horizon_client::crafting::Recipes) -> Option<(ItemStack,bool)> {
    if let Some(output)=name_press_recipe(machine){return Some((output,false));}
    let [top,bottom,middle,_] = machine.slots.as_slice() else { return None; };
    recipes.ae2_custom.get("ae2:inscriber")?.iter().find_map(|(_,v)| {
        let ingredients=v.get("ingredients")?;
        if !json_ingredient(items,ingredients.get("middle"),middle.as_ref()) { return None; }
        let t=ingredients.get("top");let b=ingredients.get("bottom");
        if !(json_ingredient(items,t,top.as_ref()) && json_ingredient(items,b,bottom.as_ref()) || json_ingredient(items,t,bottom.as_ref()) && json_ingredient(items,b,top.as_ref())) { return None; }
        // InscriberRecipe.MODE_CODEC reads every mode but "press" as INSCRIBE.
        let mode=v.get("mode")?.as_str()?=="press";
        Some((inscriber_result(v.get("result")?)?,mode))
    })
}
pub(super) fn live_charger_recipe(stack:&ItemStack,items:&Items,recipes:&horizon_client::crafting::Recipes)->Option<ItemStack> {
    recipes.ae2_custom.get("ae2:charger")?.iter().find_map(|(_,v)| {
        if !json_ingredient(items,v.get("ingredient"),Some(stack)) {return None;}
        let result=horizon_client::crafting::Template::parse(v.get("result")?)?;
        Some(result.create())
    })
}
pub(super) fn charger_chargeable(stack:&ItemStack)->bool {
    let mut single=stack.clone();single.count=1;
    super::ae2_wireless::is_wireless(&single)
        || horizon_client::ae2_powered_items::profile(&single).is_some()
        || super::ae2_interactions::portable_energy(&single).is_ok()
        || energy_cell_item(&single).is_some()
}
/// EnergyCellBlockItem, an IAEItemPowerStorage the Charger fills: its
/// block's max power and charge rate per tick (AEBlocks.ENERGY_CELL and
/// DENSE_ENERGY_CELL).
fn energy_cell_item(stack:&ItemStack)->Option<(f64,f64)> {
    if stack.count!=1 {return None;}
    match stack.item.as_ref() {"ae2:energy_cell"=>Some((200_000.0,800.0)),"ae2:dense_energy_cell"=>Some((1_600_000.0,1_600.0)),_=>None}
}
pub(super) fn is_energy_cell_item(stack:&ItemStack)->bool {energy_cell_item(stack).is_some()}
/// EnergyCellBlockItem.getAECurrentPower and getAEMaxPower.
pub(super) fn energy_cell_energy(stack:&ItemStack)->Result<(f64,f64),String> {
    let (max,_)=energy_cell_item(stack).ok_or("not an energy cell")?;
    Ok((horizon_client::ae2_powered_items::component_power_value(stack,"ae2:stored_energy","ae2:stored_energy",0.0)?,max))
}
/// EnergyCellBlockItem.getChargeRate.
fn energy_cell_charge_rate(stack:&ItemStack)->Result<f64,String> {
    energy_cell_item(stack).map(|(_,rate)|rate).ok_or_else(||"not an energy cell".into())
}
/// EnergyCellBlockItem.injectAEPower; returns the accepted AE.
fn energy_cell_insert(stack:&mut ItemStack,offered:f64)->Result<f64,String> {
    if !offered.is_finite() || offered<0.0 {return Err("invalid energy amount".into());}
    let (current,max)=energy_cell_energy(stack)?;
    let accepted=offered.min((max-current).max(0.0));let stored=current+accepted;
    // setAECurrentPower drops the component below 0.00001 AE.
    stack.data_mut().set_other("ae2:stored_energy",(stored>=0.00001).then(||Tag::Double(stored.to_bits())));
    Ok(accepted)
}
/// A standalone Assembler, running the pattern in its own slot, pushes its
/// output and leftovers out itself (MolecularAssemblerBlockEntity.pushOut
/// with no pushDirection). A provider's job leaves that slot empty; its CPU
/// collects output and leftovers, as the provider's return inventory would.
fn assembler_pushes_out(m:&Machine)->bool {
    m.slots.get(10).is_some_and(Option::is_some) && !m.config.contains_key(&"ae2_flight_owner".into())
}
/// MolecularAssemblerBlockEntity.pushOut: each side in Direction order
/// (D, U, N, S, W, E) takes what it can (pushTo); the rest stays.
fn assembler_push_out(mut stack:ItemStack,mut push_to:impl FnMut(horizon_core::geometry::Direction,&ItemStack)->u32)->Option<ItemStack> {
    for face in horizon_core::geometry::Direction::ALL {
        if stack.count==0 {break;}
        stack.count-=push_to(face,&stack).min(stack.count);
    }
    (stack.count>0).then_some(stack)
}
/// MolecularAssemblerBlockEntity.ejectHeldItems: with the output slot free,
/// the first grid item the plan does not take in its slot moves there.
fn assembler_eject_held(m:&mut Machine,valid:impl Fn(&Machine,usize,&ItemStack)->bool)->bool {
    if m.slots.len()!=11 || m.slots[9].is_some() {return false;}
    let grid:&Machine=m;
    let Some(slot)=(0..9).find(|&slot|grid.slots[slot].as_ref().is_some_and(|s|!valid(grid,slot,s))) else{return false};
    m.slots[9]=m.slots[slot].take();true
}
fn consume(slot: &mut Option<ItemStack>) {
    if let Some(s) = slot {
        s.count -= 1;
        if s.count == 0 {
            *slot = None;
        }
    }
}
fn simple_stack(id: &str) -> ItemStack {
    ItemStack {
        item: Arc::from(id),
        count: 1,
        damage: 0,
        extra: None,
    }
}
fn fits(slot: &Option<ItemStack>, result: &ItemStack, items: &Items) -> bool {
    let max = items.stack_max(result);
    match slot {
        None => result.count <= max,
        Some(s) => s.same_item_same_components(result) && s.count.checked_add(result.count).is_some_and(|n|n <= max),
    }
}
fn finish_inscriber(m: &mut Machine, consume_plates: bool, output: ItemStack) {
    if let Some(out) = &mut m.slots[3] {
        out.count += output.count;
    } else {
        m.slots[3] = Some(output);
    }
    consume(&mut m.slots[2]);
    if consume_plates {
        consume(&mut m.slots[0]);
        consume(&mut m.slots[1]);
    }
    m.progress = 0.0;
}
fn count_config(m: &Machine, key: &str, default: i32) -> i32 {
    match m.config.get(&key.into()) {
        Some(Tag::Int(n)) => *n,
        _ => default,
    }
}
fn set_int(m: &mut Machine, key: &str, value: i32) {
    m.config.insert(key.into(), Tag::Int(value));
}
fn identity(m: &Machine) -> String {
    m.slots
        .iter()
        .take(3)
        .map(|v| {
            let mut s = v.clone();
            if let Some(s) = &mut s {
                s.count = 1;
            }
            item_data::canon_stack(s.as_ref())
        })
        .collect::<Vec<_>>()
        .join("|")
}
fn run_inscriber_step(m: &mut Machine, items: &Items, recipes: &horizon_client::crafting::Recipes, powered: bool) -> bool {
    let prior = m.clone();
    let smash=count_config(m,"ae2_smash",0);
    if smash>0 {
        if smash==8 {
            if let Some((output,consume_plates))=live_inscriber_recipe(m,items,recipes) {
                if items.get(&output.item).is_some() && fits(&m.slots[3],&output,items) {finish_inscriber(m,consume_plates,output);}
            }
        }
        set_int(m,"ae2_smash",if smash>=16 {0}else{smash+1});
        return *m!=prior;
    }
    let key = identity(m);
    let old = match m.config.get(&"ae2_input_identity".into()) {
        Some(Tag::String(s)) => s.to_string_lossy(),
        _ => String::new(),
    };
    if key != old {
        m.progress = 0.0;
        set_int(m, "ae2_smash", 0);
        m.config.insert(
            "ae2_input_identity".into(),
            Tag::String(key.as_str().into()),
        );
    }
    let Some((output, consume_plates)) = live_inscriber_recipe(m, items, recipes) else {
        m.progress = 0.0;
        set_int(m, "ae2_smash", 0);
        return *m != prior;
    };
    if items.get(&output.item).is_none() || !fits(&m.slots[3], &output, items) {
        return *m != prior;
    }
    if powered {
        let speed = match m.installed_speed_cards().min(4) {
            0 => 2,
            1 => 3,
            2 => 5,
            3 => 10,
            _ => 50,
        };
        m.progress += f64::from(speed);
        if m.progress > 200.0 {
            m.progress = 200.0;
            set_int(m, "ae2_smash", 1);
        }
    }
    *m != prior
}
/// InscriberBlockEntity.tickingRequest up to pushOutResult: a smash step, or
/// a powered step of 10 x speedFactor AE taken from the Inscriber's own
/// buffer when it holds enough, else from the grid (`grid(amount, simulate)`
/// gives what the grid would give or gave). Returns whether `m` changed.
fn inscriber_tick(m: &mut Machine, items: &Items, recipes: &horizon_client::crafting::Recipes, mut grid: impl FnMut(f64, bool) -> f64) -> bool {
    let smash = count_config(m, "ae2_smash", 0);
    let valid = live_inscriber_recipe(m, items, recipes).is_some_and(|(out, _)| {
        items.get(&out.item).is_some() && fits(&m.slots[3], &out, items)
    });
    let speed = match m.installed_speed_cards().min(4) {
        0 => 2,
        1 => 3,
        2 => 5,
        3 => 10,
        _ => 50,
    };
    let cost = f64::from(speed) * 10.0;
    let usage = horizon_client::ae2_common_config::gameplay().usage_multiplier;
    let power = if smash != 0 || !valid {
        false
    } else if m.private_energy_extract(cost * usage, false) / usage > cost - 0.01 {
        m.private_energy_extract(cost * usage, true);
        true
    } else {
        grid(cost, true) > cost - 0.01 && grid(cost, false) > cost - 0.01
    };
    run_inscriber_step(m, items, recipes, power)
}
/// The TickRateModulation InscriberBlockEntity.tickingRequest returns:
/// URGENT after a push or with craft work, SLOWER with only export work.
fn inscriber_modulation(m: &Machine, items: &Items, recipes: &horizon_client::crafting::Recipes, pushed: bool) -> Modulation {
    if pushed || count_config(m, "ae2_smash", 0) > 0
        || live_inscriber_recipe(m, items, recipes).is_some_and(|(out, _)| fits(&m.slots[3], &out, items)) {
        Modulation::Urgent
    } else if inscriber_auto_export_work(m) {
        Modulation::Slower
    } else {
        Modulation::Sleep
    }
}
/// VibrationChamberBlockEntity.tickingRequest once fuel burns: the fuel
/// burned since the last call at the current rate, then the rate stepped
/// within `range` (setCurrentFuelTicksPerTick clamps). A rate outside the
/// range of the installed cards, as after a reload, burns once as it is.
/// `power` takes the AE made and gives what the grid accepted; when nothing
/// burned it is asked (None) for the grid's free room instead.
fn vibration_burn(m: &mut Machine, elapsed: u64, energy_per_fuel: f64, (min, max): (f64, f64), mut power: impl FnMut(Option<f64>) -> f64) -> Modulation {
    let spent=(elapsed as f64*m.burn_rate).min(m.burn_remaining);
    m.burn_remaining=(m.burn_remaining-spent).max(0.0);
    let step=elapsed as f64*(max-min)/100.0;
    if spent.abs()<0.01 {
        // Source injectPower(..., SIMULATE) returns overflow.
        if power(None)>=1.0 {m.burn_rate=(m.burn_rate+step).clamp(min,max);Modulation::Faster}
        else {Modulation::Idle}
    } else {
        let generated=spent*energy_per_fuel;
        let overflow=generated-power(Some(generated));
        m.burn_rate=(m.burn_rate+if overflow>0.0 {-step}else{step}).clamp(min,max);
        if overflow>0.0 {Modulation::Slower}else{Modulation::Faster}
    }
}
fn assembler_recipe_grid(
    m: &Machine,
    expected_grid: &[Option<ItemStack>],
) -> Option<(Vec<Option<ItemStack>>, Vec<usize>)> {
    if m.slots.len() < 9 || expected_grid.len() != 9 {
        return None;
    }
    let mut recipe_grid = m.slots[..9].to_vec();
    let mut fluid_slots = Vec::new();
    if let Some(tag) = m.config.get(&"ae2_assembler_fluid_escrow".into()) {
        let Tag::List(entries) = tag else {
            return None;
        };
        for entry in entries {
            let (
                Some(Tag::Byte(slot)),
                Some(Tag::String(fluid)),
                Some(Tag::ByteArray(components)),
                Some(Tag::Long(amount)),
            ) = (
                entry.get("slot"),
                entry.get("fluid"),
                entry.get("components"),
                entry.get("amount"),
            )
            else {
                return None;
            };
            let slot = *slot as u8 as usize;
            if slot >= 9
                || fluid_slots.contains(&slot)
                || *amount != 1000
                || !components.is_empty()
                || recipe_grid[slot].is_some()
            {
                return None;
            }
            let Some(expected_item) = expected_grid[slot].as_ref() else {
                return None;
            };
            if !super::ae2_fluids::bucket_fluid(&expected_item.item)
                .is_some_and(|id| *fluid == id.into())
            {
                return None;
            }
            recipe_grid[slot] = Some(expected_item.clone());
            fluid_slots.push(slot);
        }
    }
    Some((recipe_grid, fluid_slots))
}

impl Game<'_> {
    pub(super) fn ae2_invalidate_variable_processor(&mut self, pos:(i32,i32,i32)) {
        if let Some(id)=self.world.block_entities.incarnation(&pos) {
            if let Some(owner)=self.ae2_processor_ticks.owners.get_mut(&id) { owner.input.clear(); }
        }
    }
    pub(super) fn ae2_variable_processor_due(&mut self,pos:(i32,i32,i32),min:u64,max:u64,sleeping:bool,input:String)->bool {
        let Some(id)=self.world.block_entities.incarnation(&pos) else {return false;};
        let now=self.ae2_server_tick();let service=self.ae2_processor_service(pos);
        let owner=self.ae2_processor_ticks.owners.entry(id).or_insert_with(||ProcessorOwner{tracker:Tracker::new(now,min,max,sleeping),input:input.clone(),service});
        if owner.service.zip(service).is_some_and(|(a,b)|a!=b) {owner.tracker=Tracker::new(now,min,max,sleeping);}
        owner.service=service.or(owner.service);
        if owner.input!=input {owner.input=input;if sleeping {owner.tracker.sleep();} else {owner.tracker.wake();}}
        owner.tracker.due(now)
    }
    pub(super) fn ae2_variable_processor_completed(&mut self,pos:(i32,i32,i32),modulation:Modulation,input:String) {
        let now=self.ae2_server_tick();
        if let Some(id)=self.world.block_entities.incarnation(&pos) {
            if let Some(owner)=self.ae2_processor_ticks.owners.get_mut(&id) {owner.tracker.completed(now,modulation);owner.input=input;}
        }
    }
    pub(super) fn ae2_fixed_processor_elapsed(&mut self,pos:(i32,i32,i32),interval:u64)->Option<u64> {
        let id=self.world.block_entities.incarnation(&pos)?;
        let now=self.ae2_server_tick();
        let service=self.ae2_processor_service(pos);
        let owner=self.ae2_processor_ticks.owners.entry(id).or_insert_with(||ProcessorOwner{tracker:Tracker::new(now,interval,interval,false),input:String::new(),service});
        if owner.service.zip(service).is_some_and(|(a,b)|a!=b) {owner.tracker=Tracker::new(now,interval,interval,false);}
        owner.service=service.or(owner.service);
        if !owner.tracker.due(now) {return None;}
        let elapsed=now.saturating_sub(owner.tracker.last());
        owner.tracker.completed(now,Modulation::Same);
        Some(elapsed)
    }
    fn ae2_processor_service(&self,pos:(i32,i32,i32))->Option<u64> {
        let network=self.ae2_network();
        let address=self.ae2_local_address(pos)?;
        let identity=network.node_identities.get(&address.network_pos())?;
        self.ae2_event_service_ids(&network)?.get(identity).copied()
    }
    /// Invoke under with_hand for a permitted, authoritative block use.
    pub(super) fn ae2_try_charger_use(&mut self,pos:(i32,i32,i32))->bool {
        let Some(BlockEntity::Ae2(machine))=self.world.block_entities.get(&pos) else {return false;};
        if machine.kind!=MachineKind::Charger {return false;}
        if machine.slots[0].is_none() {
            let Some(mut held)=self.held().cloned() else {return false;};
            if !charger_chargeable(&held) && live_charger_recipe(&held,self.res.items(),self.res.recipes()).is_none() {return false;}
            let mut one=held.clone();one.count=1;held.count-=1;
            let Some(BlockEntity::Ae2(machine))=self.world.block_entities.get_mut(&pos) else {return false;};
            machine.slots[0]=Some(one);*self.held_mut()=(held.count>0).then_some(held);
        } else {
            let face=self.pack.assets.registry.state(self.world.block(pos.0,pos.1,pos.2)).and_then(|state|state.properties.get("facing")).map(String::as_str).unwrap_or("north");
            let delta=match face {"down"=>(0,-1,0),"up"=>(0,1,0),"south"=>(0,0,1),"west"=>(-1,0,0),"east"=>(1,0,0),_=>(0,0,-1)};
            let Some(BlockEntity::Ae2(machine))=self.world.block_entities.get_mut(&pos) else {return false;};
            let Some(stack)=machine.slots[0].take() else {return false;};
            self.pop_resource([pos.0+delta.0,pos.1+delta.1,pos.2+delta.2],stack);
        }
        self.world.modified.insert((pos.0>>4,pos.2>>4));true
    }
    fn ae2_eat_vibration_fuel(&self,m:&mut Machine)->bool {
        let Some(fuel)=m.slots.first().and_then(Option::as_ref) else {return false;};
        let ticks=self.res.providers().burn_time(self.res.items(),fuel,false);
        if ticks<=0 {return false;}
        let remainder=self.res.items().get(&fuel.item).and_then(|item|item.crafting_remainder.as_ref()).cloned();
        m.burn_remaining+=f64::from(ticks);
        m.config.insert("ae2_fuel_max".into(),Tag::Double(m.burn_remaining.to_bits()));
        consume(&mut m.slots[0]);
        if m.slots[0].is_none() {if let Some(id)=remainder {m.slots[0]=Some(simple_stack(&id));}}
        true
    }
    /// InscriberBlockEntity.pushOutResult: try sides in Java Direction order,
    /// preserving the physical output when a neighbor refuses or changes.
    fn ae2_inscriber_push_output(&mut self,pos:(i32,i32,i32),m:&mut Machine,original:&Machine,plan:&mut super::ae2_buses::FormationPlan)->bool {
        use horizon_core::geometry::Direction;
        if !inscriber_auto_export_work(m){return false}
        let Some(source)=self.ae2_local_address(pos) else{return false};
        let faces=[(Direction::Down,(0,-1,0)),(Direction::Up,(0,1,0)),(Direction::North,(0,0,-1)),(Direction::South,(0,0,1)),(Direction::West,(-1,0,0)),(Direction::East,(1,0,0))];
        for (face,(dx,dy,dz)) in faces {
            let Some(slots)=self.ae2_inscriber_faces(source,face,m) else{return false};
            if slots.len()!=4 && !slots.contains(&3){continue}
            let Some(target)=self.ae2_local_address((pos.0+dx,pos.1+dy,pos.2+dz)) else{continue};
            let Some(stack)=m.slots.get(3).and_then(Option::as_ref).cloned() else{return false};
            let offer=stack.count.min(64);
            let mut offered=stack.clone();offered.count=offer;
            // External handlers receive an extracted copy, not another owner
            // of the source slot's transient object identity.
            horizon_client::ae2_stack_identity::detach_if_tracked(&mut offered);
            let accepted=match self.ae2_item_p2p_insert(target,face.opposite(),&offered,true) {
                Some(n)=>n.min(offer),None=>self.ae2_native_item_insert(target,face.opposite(),&offered,offer,true),
            };
            if accepted==0{continue}
            let before=self.ae2_snapshot_blocks();
            let previous=plan.len();
            let moved=match self.ae2_item_p2p_insert_staged(target,face.opposite(),&offered,false,plan) {
                Some(n)=>n,None=>self.ae2_native_item_insert_staged(target,face.opposite(),&offered,offer,false,plan),
            };
            let source_unchanged=matches!(self.world.block_entities.get(&pos),Some(BlockEntity::Ae2(current)) if **current==*original);
            if moved>offer || !source_unchanged {self.ae2_restore_blocks(before);plan.truncate(previous);continue}
            if moved==0{self.ae2_restore_blocks(before);plan.truncate(previous);continue}
            let mut remain=stack;remain.count-=moved;
            horizon_client::ae2_stack_identity::synchronize_tracked(&remain);
            m.slots[3]=(remain.count>0).then_some(remain);return true
        }
        false
    }
    pub(super) fn tick_ae2_processing(&mut self) {
        self.tick_ae2_processing_pass(false);
    }
    pub(super) fn tick_ae2_grid_processing(&mut self) {
        self.tick_ae2_processing_pass(true);
    }
    fn tick_ae2_processing_pass(&mut self, frozen: bool) {
        let now=self.ae2_server_tick();
        let live: std::collections::BTreeSet<_>=self.ae2_loaded_worlds().into_iter().flat_map(|(_,world)|world.block_entities.iter().filter_map(|(pos,_)|world.block_entities.incarnation(pos))).collect();
        self.ae2_processor_ticks.owners.retain(|id,_|live.contains(id));
        let positions: Vec<_> = self
            .world
            .block_entities
            .iter()
            .filter_map(|(p, e)| match e {
                BlockEntity::Ae2(m)
                    if matches!(
                        m.kind,
                        MachineKind::Inscriber
                            | MachineKind::Charger
                            | MachineKind::VibrationChamber
                            | MachineKind::MolecularAssembler
                            | MachineKind::Chest
                    ) =>
                {
                    Some(*p)
                }
                _ => None,
            })
            .collect();
        for pos in positions {
            let Some(BlockEntity::Ae2(original)) = self.world.block_entities.get(&pos) else {
                continue;
            };
            let mut m = (**original).clone();
            let original_machine=m.clone();
            let mut formation=super::ae2_buses::FormationPlan::default();
            if frozen && m.kind==MachineKind::Chest { continue; }
            let mut elapsed=1;
            let mut tracking=None;
            if matches!(m.kind,MachineKind::Charger|MachineKind::Inscriber|MachineKind::VibrationChamber) {
                let Some(incarnation)=self.world.block_entities.incarnation(&pos) else {continue};
                if m.kind==MachineKind::VibrationChamber && !self.ae2_processor_ticks.owners.contains_key(&incarnation) && m.burn_remaining<=0.0 {
                    if self.ae2_eat_vibration_fuel(&mut m) {
                        if let Some(BlockEntity::Ae2(target))=self.world.block_entities.get_mut(&pos) {**target=m.clone();}
                        self.world.modified.insert((pos.0>>4,pos.2>>4));
                    }
                }
                let input=processing_input(&m);
                let sleeping=match m.kind {
                    MachineKind::Inscriber=>inscriber_modulation(&m,self.res.items(),self.res.recipes(),false)==Modulation::Sleep,
                    MachineKind::VibrationChamber=>m.burn_remaining<=0.0 && !m.slots.first().and_then(Option::as_ref).is_some_and(|fuel|self.res.providers().burn_time(self.res.items(),fuel,false)>0),
                    _=>false,
                };
                let kind=match m.kind {MachineKind::Charger=>horizon_client::ae2_tick_config::Kind::Charger,MachineKind::Inscriber=>horizon_client::ae2_tick_config::Kind::Inscriber,_=>horizon_client::ae2_tick_config::Kind::VibrationChamber};
                let (min,max)=horizon_client::ae2_tick_config::rates(kind);
                let service=self.ae2_processor_service(pos);
                let owner=self.ae2_processor_ticks.owners.entry(incarnation).or_insert_with(||ProcessorOwner{tracker:Tracker::new(now,min,max,sleeping),input:input.clone(),service});
                if owner.service.zip(service).is_some_and(|(a,b)|a!=b) {owner.tracker=Tracker::new(now,min,max,sleeping);}
                owner.service=service.or(owner.service);
                let Some(ticks)=owner.due(now,input) else {continue};
                elapsed=ticks;
                tracking=Some(incarnation);
            }
            let mut modulation=Modulation::Same;
            let changed = match m.kind {
                MachineKind::Inscriber => {
                    let res = self.res.clone();
                    let mut online = None;
                    let changed = inscriber_tick(&mut m, res.items(), res.recipes(), |amount, simulate| {
                        if !*online.get_or_insert_with(|| self.ae2_online(pos)) { return 0.0; }
                        self.ae2_take_power(pos, amount, simulate)
                    });
                    let pushed=self.ae2_inscriber_push_output(pos,&mut m,&original_machine,&mut formation);
                    modulation=inscriber_modulation(&m,res.items(),res.recipes(),pushed);
                    changed||pushed
                }
                MachineKind::Chest => {
                    let old = m.energy;
                    // Source MEChest.serverTick: local idle drain applies only
                    // when network power is unavailable, and only with a cell.
                    if !self.ae2_online(pos) {
                        if let Some(stack) = m.slots.first().and_then(Option::as_ref) {
                            if let Some(idle) = super::ae2_storage_devices::cell_idle_drain(stack) { m.private_energy_extract((1.0 + idle) * horizon_client::ae2_common_config::gameplay().usage_multiplier, true); }
                        }
                    }
                    m.energy != old
                }
                MachineKind::MolecularAssembler => self.ae2_tick_scheduled_assembler(pos, &mut m, &original_machine, &mut formation),
                MachineKind::Charger => {
                    // Source TickRates.Charger is 10 ticks. A chargeable item
                    // takes power from the local buffer first, then the grid.
                    let old = m.clone();
                    {
                        modulation=Modulation::Faster;
                        let mut working=false;
                        if let Some(stack) = m.slots.get_mut(0).and_then(Option::as_mut) {
                            let wireless = super::ae2_wireless::is_wireless(stack);
                            let powered_tool = horizon_client::ae2_powered_items::profile(stack).is_some();
                            let energy_cell = is_energy_cell_item(stack);
                            let energy = if wireless {
                                super::ae2_wireless::energy(stack)
                            } else if powered_tool {
                                horizon_client::ae2_powered_items::energy(stack)
                            } else if energy_cell {
                                energy_cell_energy(stack)
                            } else {
                                super::ae2_interactions::portable_energy(stack)
                            };
                            let rate = if wireless {
                                super::ae2_wireless::charge_rate(stack)
                            } else if powered_tool {
                                horizon_client::ae2_powered_items::charge_rate(stack)
                            } else if energy_cell {
                                energy_cell_charge_rate(stack)
                            } else {
                                super::ae2_interactions::portable_charge_rate(stack)
                            };
                            if let (Ok((current, max)), Ok(rate)) = (energy, rate) {
                                let wanted = (rate * elapsed as f64 * horizon_client::ae2_common_config::processing().charger_rate).min((max - current).max(0.0));
                                if wanted > 0.0 {
                                    let usage = horizon_client::ae2_common_config::gameplay().usage_multiplier;
                                    let mut local_buffer = ae2_machines_core::stored_energy::StoredEnergy::new(m.energy, 1600.0);
                                    let local = local_buffer.extract(wanted * usage, false) / usage;
                                    let grid = self.ae2_take_power_raw(pos, wanted - local, false);
                                    let offered = local + grid;
                                    let charge = if wireless {
                                        super::ae2_wireless::transfer_energy(
                                            stack, offered, true, false,
                                        )
                                    } else if powered_tool {
                                        horizon_client::ae2_powered_items::transfer(stack, offered, true, false)
                                    } else if energy_cell {
                                        energy_cell_insert(stack, offered)
                                    } else {
                                        super::ae2_interactions::portable_insert_energy(
                                            stack, offered, false,
                                        )
                                    };
                                    match charge {
                                        Ok(accepted) if accepted >= 0.0 && accepted <= offered => {
                                            if accepted>0.0 {working=true;}
                                            let used_local = accepted.min(local);
                                            local_buffer.extract(used_local * usage, true);
                                            m.energy = local_buffer.stored();
                                            let unused_grid =
                                                (grid - (accepted - used_local)).max(0.0);
                                            if unused_grid > 0.0 {
                                                let _ = self.ae2_add_power(pos, unused_grid);
                                            }
                                        }
                                        _ => {
                                            if grid > 0.0 {
                                                let _ = self.ae2_add_power(pos, grid);
                                            }
                                        }
                                    }
                                }
                            } else if m.energy >= 1599.0 && stack.count == 1 {
                                if let Some(out)=live_charger_recipe(stack,self.res.items(),self.res.recipes())
                                    && self.res.items().get(&out.item).is_some() {
                                    working=true;
                                    if self.world.random.next_float() > 0.8 {
                                        *stack=out;
                                        let mut local_buffer = ae2_machines_core::stored_energy::StoredEnergy::new(m.energy, 1600.0);
                                        local_buffer.extract(1600.0*horizon_client::ae2_common_config::gameplay().usage_multiplier, true);
                                        m.energy = local_buffer.stored();
                                    }
                                }
                            }
                        }
                        m.config.insert("charger_working".into(),Tag::Byte(i8::from(working)));
                        if m.energy < 1599.0 && self.ae2_online(pos) {
                            let received = self.ae2_take_power_raw(pos, (1600.0 - m.energy).min(800.0), false);
                            m.private_energy_insert(received, true);
                        }
                    }
                    m != old
                }
                MachineKind::VibrationChamber => {
                    let old=m.clone();
                    let settings=horizon_client::ae2_common_config::processing();
                    let energy_cards=m.upgrades.iter().flatten().filter(|s|s.count==1 && s.item.as_ref()=="ae2:energy_card").count();
                    let energy_per_fuel=settings.fuel_energy*(1.0+energy_cards as f64/2.0);
                    let min=settings.vibration_min/energy_per_fuel;
                    let max=settings.vibration_max*(1.0+m.installed_speed_cards() as f64/2.0)/energy_per_fuel;
                    let max=max.max(min);
                    if m.burn_remaining<=0.0 {
                        self.ae2_eat_vibration_fuel(&mut m);
                        if m.burn_remaining>0.0 {modulation=Modulation::Urgent;}
                        else {m.burn_rate=settings.fuel_energy.clamp(settings.vibration_min,settings.vibration_max.max(settings.vibration_min))/energy_per_fuel;modulation=Modulation::Sleep;}
                    } else {
                        modulation=vibration_burn(&mut m,elapsed,energy_per_fuel,(min,max),|generated|match generated {
                            None=>self.ae2_power_room(pos),
                            Some(amount)=>self.ae2_add_power(pos,amount),
                        });
                    }
                    m!=old
                }
                _ => false,
            };
            if let Some(id)=tracking {
                if let Some(owner)=self.ae2_processor_ticks.owners.get_mut(&id) {
                    owner.tracker.completed(now,modulation);
                    owner.input=processing_input(&m);
                }
            }
            if changed {
                if let Some(BlockEntity::Ae2(target)) = self.world.block_entities.get_mut(&pos) {
                    **target = m;
                    self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
                    if let Err(error)=self.ae2_commit_formation(formation) { log::warn!("AE2 processor notification: {error}"); }
                }
            }
            self.ae2_recover_orphan_assembler(pos);
        }
    }
    /// A cancelled CPU no longer owns an already dispatched assembler job.
    /// Once it finishes, return only its actual output and recipe remainders;
    /// an offline/full network leaves those physical stacks in the machine.
    fn ae2_recover_orphan_assembler(&mut self,pos:(i32,i32,i32)) {
        // The physical assembler carries its owner identity across unload. Only
        // classify it as orphan after the CPU's dimension and chunk are loaded.
        if self.ae2_assembler_flight_is_live(pos) == Some(false) {
            if let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get_mut(&pos) {
                if !matches!(machine.config.get(&"ae2_orphaned_flight".into()), Some(Tag::Byte(1))) {
                    machine.config.insert("ae2_orphaned_flight".into(), Tag::Byte(1));
                    self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
                }
            }
        }
        let Some(BlockEntity::Ae2(machine))=self.world.block_entities.get(&pos) else{return;};
        // A persisted marker may race a stale CPU chunk save. Never recover a
        // tokenized flight unless its owner is loaded and verified absent.
        if machine.config.contains_key(&"ae2_flight_owner".into())
            && self.ae2_assembler_flight_is_live(pos) != Some(false) { return; }
        if machine.kind!=MachineKind::MolecularAssembler || machine.slots.len()!=11
            || !matches!(machine.config.get(&"ae2_orphaned_flight".into()),Some(Tag::Byte(1)))
            || machine.config.contains_key(&"ae2_assembler_pattern".into())
            || !self.ae2_online(pos) {return;}
        let Some(address)=self.ae2_local_address(pos) else{return;};
        let mut updated=(**machine).clone();
        let snapshot=self.ae2_snapshot_blocks();
        let mut plan=super::ae2_buses::FormationPlan::default();
        let result=(||->Result<(),String>{
            for slot in &mut updated.slots[..10] {
                let Some(stack)=slot.clone() else{continue;};
                let moved=self.ae2_grid_insert_uncharged_staged_encoded(address,&stack,u64::from(stack.count),&mut plan)?;
                if moved>u64::from(stack.count) {return Err("invalid assembler return count".into());}
                *slot=(moved<u64::from(stack.count)).then_some(ItemStack{count:stack.count-moved as u32,..stack});
            }
            if updated.slots[..10].iter().all(Option::is_none) {
                updated.config.remove(&"ae2_orphaned_flight".into());
                updated.config.remove(&"ae2_flight_owner".into());
            }
            let Some(BlockEntity::Ae2(machine))=self.world.block_entities.get_mut(&pos) else{return Err("assembler disappeared".into());};
            **machine=updated;
            self.world.modified.insert((pos.0>>4,pos.2>>4));
            self.ae2_commit_formation(plan)
        })();
        if result.is_err(){self.ae2_restore_blocks(snapshot);}
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn inscriber_auto_export_only_wakes_for_enabled_physical_output() {
        use super::{inscriber_auto_export_work,processing_input};
        use horizon_client::{ae2_world::{Machine,MachineKind},items::ItemStack};
        use horizon_world::nbt::Tag;
        let mut machine=Machine::new(MachineKind::Inscriber);
        machine.slots[3]=Some(ItemStack::new("ae2:printed_logic_processor",4));
        let before=processing_input(&machine);assert!(!inscriber_auto_export_work(&machine));
        machine.config.insert("auto_export".into(),Tag::String("YES".into()));
        assert_ne!(processing_input(&machine),before);assert!(inscriber_auto_export_work(&machine));
        machine.slots[3]=None;assert!(!inscriber_auto_export_work(&machine));
        machine.config.insert("auto_export".into(),Tag::String("invalid".into()));
        machine.slots[3]=Some(ItemStack::new("ae2:printed_logic_processor",4));
        assert!(!inscriber_auto_export_work(&machine));
    }
    use super::*;
    #[test]
    fn inscriber_output_respects_component_stack_maximum() {
        let items=Items::tags_only(&std::env::temp_dir().join("ae2-empty-item-catalog"), &horizon_world::loot::PackOverlay::NONE).unwrap();
        let mut output=ItemStack::new("minecraft:stone",1);
        output.data_mut().set_other("minecraft:max_stack_size",Some(Tag::Int(16)));
        let mut partial=output.clone();partial.count=15;
        assert!(fits(&Some(partial.clone()),&output,&items));
        partial.count=16;
        assert!(!fits(&Some(partial),&output,&items));
        output.count=17;
        assert!(!fits(&None,&output,&items));
    }
    #[test]
    fn inscriber_output_keeps_recipe_components_in_the_physical_slot() {
        let result = serde_json::json!({"id":"minecraft:diamond", "count":2,
            "components":{"minecraft:custom_data":{"marker":7}}});
        let output = inscriber_result(&result).unwrap();
        assert_eq!(output.count, 2);
        assert_eq!(output.data().unwrap().other("minecraft:custom_data").unwrap().get("marker"), Some(&Tag::Int(7)));
        assert!(!output.same_item_same_components(&simple_stack("minecraft:diamond")));
        let mut machine = Machine::new(MachineKind::Inscriber);
        machine.slots[2] = Some(simple_stack("minecraft:gold_ingot"));
        finish_inscriber(&mut machine, false, output);
        assert!(machine.slots[2].is_none());
        let stored = machine.slots[3].as_ref().unwrap();
        assert_eq!(stored.count, 2);
        assert_eq!(stored.data().unwrap().other("minecraft:custom_data").unwrap().get("marker"), Some(&Tag::Int(7)));
        assert!(inscriber_result(&serde_json::json!({"id":"minecraft:diamond","count":-1})).is_none());
    }
    #[test]
    fn name_press_combines_names_preserves_input_and_retains_plates() {
        let press=|name:&str| {let mut s=simple_stack("ae2:name_press");s.data_mut().set_other("ae2:name_press_name",Some(Tag::String(name.into())));s};
        let mut m=Machine::new(MachineKind::Inscriber);
        m.slots[0]=Some(press("Alpha"));m.slots[1]=Some(press("Beta"));
        let mut input=simple_stack("minecraft:diamond");input.count=2;input.data_mut().set_other("minecraft:custom_data",Some(Tag::compound([("marker",Tag::Int(7))])));m.slots[2]=Some(input);
        let output=name_press_recipe(&m).unwrap();assert_eq!(output.count,1);assert_eq!(output.custom_name().as_deref(),Some("Alpha Beta"));
        assert_eq!(output.data().unwrap().other("minecraft:custom_data").unwrap().get("marker"),Some(&Tag::Int(7)));
        m.slots[1].as_mut().unwrap().data_mut().set_other("ae2:name_press_name",None);
        m.slots[1].as_mut().unwrap().data_mut().set_other("minecraft:custom_data",Some(Tag::compound([("ae2:name_press_name",Tag::String("Legacy".into()))])));
        assert_eq!(name_press_recipe(&m).unwrap().custom_name().as_deref(),Some("Alpha Legacy"));
        finish_inscriber(&mut m,false,output);assert!(m.slots[0].is_some()&&m.slots[1].is_some());assert_eq!(m.slots[2].as_ref().unwrap().count,1);
        m.slots[1]=Some(simple_stack("ae2:logic_processor_press"));assert!(name_press_recipe(&m).is_none());
    }
    #[test]
    fn name_press_preserves_structured_component_trees_in_physical_output() {
        let first=Tag::compound([("text",Tag::String("Red".into())),("color",Tag::String("red".into()))]);
        let second=Tag::compound([("translate",Tag::String("item.minecraft.diamond".into())),("bold",Tag::Byte(1))]);
        let mut top=simple_stack("ae2:name_press");top.data_mut().set_other("ae2:name_press_name",Some(first.clone()));
        let mut bottom=simple_stack("ae2:name_press");bottom.data_mut().set_other("ae2:name_press_name",Some(second.clone()));
        let mut machine=Machine::new(MachineKind::Inscriber);
        machine.slots[0]=Some(top);machine.slots[1]=Some(bottom);machine.slots[2]=Some(simple_stack("minecraft:diamond"));
        let output=name_press_recipe(&machine).unwrap();
        let expected=append_press_name(&first,&second);
        assert_eq!(horizon_client::item_data::encode_components(&output).unwrap().get("minecraft:custom_name"),Some(&expected));
        let saved=horizon_client::save::item_to_nbt(&output);
        let restored=horizon_client::save::item_from_nbt(&saved).unwrap();
        assert_eq!(horizon_client::item_data::encode_components(&restored).unwrap().get("minecraft:custom_name"),Some(&expected));
        assert!(output.same_item_same_components(&restored));
        machine.slots[1]=None;
        let single=name_press_recipe(&machine).unwrap();
        assert_eq!(horizon_client::item_data::encode_components(&single).unwrap().get("minecraft:custom_name"),Some(&first));
    }
    #[test]
    fn name_press_list_inherits_first_component_style_through_nbt() {
        let second = Tag::compound([("text", Tag::String("End".into())), ("color", Tag::String("blue".into()))]);
        let first = Tag::List(vec![
            Tag::compound([
                ("text", Tag::String("Red".into())),
                ("color", Tag::String("red".into())),
                ("extra", Tag::List(vec![Tag::String("!".into())])),
            ]),
            Tag::String("tail".into()),
        ]);
        let mut machine = Machine::new(MachineKind::Inscriber);
        let mut top = simple_stack("ae2:name_press");
        top.data_mut().set_other("ae2:name_press_name", Some(first));
        let mut bottom = simple_stack("ae2:name_press");
        bottom.data_mut().set_other("ae2:name_press_name", Some(second.clone()));
        machine.slots[0] = Some(top);
        machine.slots[1] = Some(bottom);
        machine.slots[2] = Some(simple_stack("minecraft:diamond"));
        let output = name_press_recipe(&machine).unwrap();
        let encoded = horizon_client::save::item_to_nbt(&output);
        let bytes = horizon_world::nbt::write_network(&encoded, Default::default()).unwrap();
        let (decoded, _) = horizon_world::nbt::read_network(&bytes, Default::default()).unwrap();
        let restored = horizon_client::save::item_from_nbt(&decoded).unwrap();
        let name = restored.custom_name_component().unwrap();
        assert_eq!(name.get("color"), Some(&Tag::String("red".into())));
        assert_eq!(name.get("extra"), Some(&Tag::List(vec![
            Tag::String("!".into()), Tag::String("tail".into()), Tag::String(" ".into()), second,
        ])));
        let rendered = horizon_client::entity_nbt::name_text(name);
        assert_eq!(rendered.plain(), "Red!tail End");
        let colors: Vec<_> = rendered.chars().map(|(_, style)| style.color).collect();
        assert!(colors[0].is_some());
        assert!(colors[..9].iter().all(|color| *color == colors[0]), "the first root's red style must carry through the separator");
        assert_ne!(colors[9], colors[0], "the second component's explicit blue style wins");
    }
    #[test]
    fn ae2_private_power_capacities_and_oriented_faces() {
        use horizon_core::geometry::Direction::*;
        let mut state = horizon_world::chunk::BlockState {
            name: "ae2:inscriber".to_owned(),
            properties: [("facing".to_owned(), "north".to_owned())].into_iter().collect(),
        };
        assert_eq!(ae2_private_power_capacity(&Machine::new(MachineKind::Inscriber)), Some(1600.0));
        let mut chest=Machine::new(MachineKind::Chest);
        chest.chest_power_capacity=Some(1234.0);
        assert_eq!(ae2_private_power_capacity(&chest), Some(1234.0));
        assert_eq!(ae2_private_power_capacity(&Machine::new(MachineKind::EnergyAcceptor)), None);
        assert!(!ae2_private_power_face(MachineKind::Inscriber, &state, North));
        assert!(ae2_private_power_face(MachineKind::Inscriber, &state, South));
        assert!(ae2_private_power_face(MachineKind::GrowthAccelerator, &state, North));
        assert!(ae2_private_power_face(MachineKind::GrowthAccelerator, &state, South));
        assert!(!ae2_private_power_face(MachineKind::GrowthAccelerator, &state, East));
        state.properties.clear();
        assert!(!ae2_private_power_face(MachineKind::Charger, &state, North));
        assert!(ae2_private_power_face(MachineKind::Chest, &state, North));
    }
    #[test]
    fn inscribe_retains_press_and_press_recipe_consumes_prints() {
        let mut m = Machine::new(MachineKind::Inscriber);
        m.slots[0] = Some(simple_stack("ae2:logic_processor_press"));
        m.slots[2] = Some(simple_stack("minecraft:gold_ingot"));
        let r = RECIPES
            .iter()
            .copied()
            .find(|r| r.result == "ae2:printed_logic_processor")
            .unwrap();
        finish_inscriber(&mut m, r.consume_plates, simple_stack(r.result));
        assert!(m.slots[0].is_some());
        assert!(m.slots[2].is_none());
        assert_eq!(m.slots[3].as_ref().unwrap().item.as_ref(), r.result);
        m.slots[0] = m.slots[3].take();
        m.slots[1] = Some(simple_stack("ae2:printed_silicon"));
        m.slots[2] = Some(simple_stack("minecraft:redstone"));
        let r = RECIPES
            .iter()
            .copied()
            .find(|r| r.result == "ae2:logic_processor")
            .unwrap();
        finish_inscriber(&mut m, r.consume_plates, simple_stack(r.result));
        assert!(m.slots[..3].iter().all(Option::is_none));
        assert_eq!(m.slots[3].as_ref().unwrap().item.as_ref(), r.result);
    }
}

#[cfg(test)]
mod push_out_and_charge_tests {
    use super::*;
    use horizon_core::geometry::Direction;
    fn assembler(pattern:bool)->Machine {
        let mut m=Machine::new(MachineKind::MolecularAssembler);
        if pattern {m.slots[10]=Some(simple_stack("ae2:crafting_pattern"));}
        m
    }
    #[test]
    fn standalone_assembler_pushes_its_output_to_each_side_in_order() {
        // A CPU's job (no own pattern, or a flight owner) is collected by the CPU.
        assert!(assembler_pushes_out(&assembler(true)));
        assert!(!assembler_pushes_out(&assembler(false)));
        let mut flight=assembler(true);flight.config.insert("ae2_flight_owner".into(),Tag::String("{}".into()));
        assert!(!assembler_pushes_out(&flight));
        // MolecularAssemblerBlockEntity.pushOut: D, U, N, S, W, E, each taking what it can.
        let mut slabs=simple_stack("minecraft:stone_slab");slabs.count=6;
        let mut offers=Vec::new();
        let rest=assembler_push_out(slabs.clone(),|face,offered|{offers.push((face,offered.count));match face {Direction::Up=>2,Direction::West=>9,_=>0}});
        assert_eq!(rest,None);
        assert_eq!(offers,[(Direction::Down,6),(Direction::Up,6),(Direction::North,4),(Direction::South,4),(Direction::West,4)]);
        // With no neighbour taking it, the output stays in its slot.
        assert_eq!(assembler_push_out(slabs.clone(),|_,_|0),Some(slabs));
    }
    #[test]
    fn standalone_assembler_ejects_leftovers_one_at_a_time() {
        // ejectHeldItems after a cake: the plan takes milk, not the empty buckets.
        let mut m=assembler(true);
        for slot in 0..3 {m.slots[slot]=Some(simple_stack("minecraft:bucket"));}
        m.slots[4]=Some(simple_stack("minecraft:egg"));
        let valid=|_:&Machine,_:usize,s:&ItemStack|s.item.as_ref()!="minecraft:bucket";
        m.slots[9]=Some(simple_stack("minecraft:cake"));
        assert!(!assembler_eject_held(&mut m,valid));
        for slot in 0..3 {
            m.slots[9]=None;
            assert!(assembler_eject_held(&mut m,valid));
            assert_eq!(m.slots[9],Some(simple_stack("minecraft:bucket")));
            assert!(m.slots[slot].is_none());
            assert_eq!(m.slots[..3].iter().flatten().count(),2-slot);
        }
        m.slots[9]=None;
        assert!(!assembler_eject_held(&mut m,valid));
        assert_eq!(m.slots[4],Some(simple_stack("minecraft:egg")));
        assert!(m.slots[9].is_none());
    }
    #[test]
    fn charger_fills_energy_cells_at_their_block_rate() {
        // Platform.isChargeable: EnergyCellBlockItem is an IAEItemPowerStorage with WRITE flow.
        let mut cell=simple_stack("ae2:energy_cell");let mut dense=simple_stack("ae2:dense_energy_cell");
        assert!(charger_chargeable(&cell));
        assert!(charger_chargeable(&dense));
        // ChargerBlockEntity.doWork: getChargeRate x 10 ticks x chargerChargeRate 1.0.
        assert_eq!(energy_cell_charge_rate(&cell).unwrap()*10.0,8_000.0);
        assert_eq!(energy_cell_charge_rate(&dense).unwrap()*10.0,16_000.0);
        assert_eq!(energy_cell_energy(&cell).unwrap(),(0.0,200_000.0));
        assert_eq!(energy_cell_insert(&mut cell,8_000.0).unwrap(),8_000.0);
        assert_eq!(energy_cell_energy(&cell).unwrap(),(8_000.0,200_000.0));
        assert_eq!(cell.data().unwrap().other("ae2:stored_energy"),Some(&Tag::Double(8_000f64.to_bits())));
        // injectAEPower keeps only what fits.
        dense.data_mut().set_other("ae2:stored_energy",Some(Tag::Double(1_590_000f64.to_bits())));
        assert_eq!(energy_cell_insert(&mut dense,16_000.0).unwrap(),10_000.0);
        assert_eq!(energy_cell_energy(&dense).unwrap(),(1_600_000.0,1_600_000.0));
    }
}

#[cfg(test)]
mod reload_and_oracle_tests {
    use super::*;
    /// A data root with AE2's inscriber/silicon_print.json, its `mode` set
    /// to `mode` (None leaves the field out), and the items it names. `test`
    /// keeps the temporary roots of tests running at once apart.
    fn silicon_print(test:&str,mode:Option<&str>)->(Items,horizon_client::crafting::Recipes) {
        let root=std::env::temp_dir().join(format!("ae2-silicon-print-{}-{test}-{}",std::process::id(),mode.unwrap_or("none")));
        let _=std::fs::remove_dir_all(&root);
        let recipe_dir=root.join("data/ae2/recipe/inscriber");let tag_dir=root.join("data/c/tags/item");
        for dir in [&recipe_dir,&tag_dir,&root.join("data/minecraft/recipe"),&root.join("data/minecraft/loot_table")] {std::fs::create_dir_all(dir).unwrap();}
        std::fs::write(root.join("horizon-mod-content.json"),br#"["ae2"]"#).unwrap();
        std::fs::write(root.join("items.json"),br#"{"ae2:silicon":{},"ae2:silicon_press":{},"ae2:printed_silicon":{}}"#).unwrap();
        std::fs::write(tag_dir.join("silicon.json"),br#"{"values":["ae2:silicon"]}"#).unwrap();
        let mut recipe=serde_json::json!({"type":"ae2:inscriber","ingredients":{"middle":{"tag":"c:silicon"},"top":{"item":"ae2:silicon_press"}},"result":{"count":1,"id":"ae2:printed_silicon"}});
        if let Some(mode)=mode {recipe["mode"]=mode.into();}
        std::fs::write(recipe_dir.join("silicon_print.json"),recipe.to_string()).unwrap();
        let items=Items::catalog(&root,&horizon_world::loot::PackOverlay::NONE).unwrap();
        let recipes=horizon_client::crafting::Recipes::load(&root,&items,&horizon_world::loot::PackOverlay::NONE).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        (items,recipes)
    }
    fn reload(m:&Machine)->Machine {
        let nbt=horizon_client::save::encode_block_entity((0,64,0),&BlockEntity::Ae2(Box::new(m.clone())),m.kind.id());
        let Some((_,BlockEntity::Ae2(back)))=horizon_client::save::decode_block_entity(&nbt) else {panic!("AE2 machine")};
        *back
    }
    #[test]
    fn reload_restarts_inscriber_and_assembler_progress() {
        // InscriberBlockEntity.loadTag restores neither processingTime nor
        // smash and finalStep; the inventory and settings come back.
        let mut inscriber=Machine::new(MachineKind::Inscriber);
        inscriber.slots[0]=Some(simple_stack("ae2:silicon_press"));inscriber.slots[2]=Some(simple_stack("ae2:silicon"));
        inscriber.progress=160.0;set_int(&mut inscriber,"ae2_smash",5);
        inscriber.config.insert("auto_export".into(),Tag::String("YES".into()));
        let back=reload(&inscriber);
        assert_eq!((back.progress,count_config(&back,"ae2_smash",0)),(0.0,0));
        assert_eq!(back.slots,inscriber.slots);
        assert_eq!(back.config.get(&"auto_export".into()),inscriber.config.get(&"auto_export".into()));
        // MolecularAssemblerBlockEntity.saveAdditional writes myPlan, not progress.
        let mut assembler=Machine::new(MachineKind::MolecularAssembler);assembler.progress=40.0;
        assert_eq!(reload(&assembler).progress,0.0);
    }
    #[test]
    fn reload_turns_vibration_burn_speed_into_a_percent_of_the_uncarded_max() {
        // VibrationChamberBlockEntity.saveAdditional: burnSpeed = (int)
        // (rate x 100 / max of the last tick). loadTag: burnSpeed x max / 100
        // with the constructor's range, built before the upgrades load: 0.8..8.
        let chamber=|card:&str,cards:usize,rate:f64|{
            let mut m=Machine::new(MachineKind::VibrationChamber);
            for slot in &mut m.upgrades[..cards] {*slot=Some(simple_stack(card));}
            m.burn_rate=rate;m.burn_remaining=1600.0;
            let back=reload(&m);
            assert_eq!((back.burn_remaining,&back.upgrades),(1600.0,&m.upgrades));
            back.burn_rate
        };
        // Three Speed Cards at full speed (20, 100 AE/t) come back at 8 (40 AE/t).
        assert_eq!(chamber("ae2:speed_card",3,20.0),8.0);
        // Three Energy Cards at their minimum 0.32: 10 % of 8 is below 0.8.
        assert_eq!(chamber("ae2:energy_card",3,4.0/12.5),0.8);
        // One Speed Card, 7 of 12: 58 %.
        assert_eq!(chamber("ae2:speed_card",1,7.0),58.0*8.0/100.0);
        // No cards, the initial 1 of 8: 12 %.
        assert_eq!(chamber("ae2:speed_card",0,1.0),12.0*8.0/100.0);
    }
    #[test]
    fn vibration_chamber_burns_at_its_rate_before_stepping_into_the_card_range() {
        // tickingRequest burns ticksSinceLastCall x currentFuelTicksPerTick;
        // only setCurrentFuelTicksPerTick afterwards clamps to the range of
        // the installed cards. Three Energy Cards: 12.5 AE per fuel tick,
        // 0.32..3.2 fuel ticks per tick; reloaded at 8 (VC-1).
        let mut m=Machine::new(MachineKind::VibrationChamber);
        m.burn_rate=8.0;m.burn_remaining=1000.0;
        let mut offered=Vec::new();
        let modulation=vibration_burn(&mut m,25,12.5,(4.0/12.5,40.0/12.5),|generated|{offered.push(generated);generated.unwrap_or(1.0)});
        assert_eq!(offered,[Some(2500.0)]);
        assert_eq!((m.burn_remaining,m.burn_rate,modulation),(800.0,40.0/12.5,Modulation::Faster));
    }
    #[test]
    fn inscriber_recipe_mode_other_than_press_inscribes() {
        // InscriberRecipe.MODE_CODEC: "press" is PRESS, any other string is
        // INSCRIBE; the field itself is required.
        let mut m=Machine::new(MachineKind::Inscriber);
        m.slots[0]=Some(simple_stack("ae2:silicon_press"));m.slots[2]=Some(simple_stack("ae2:silicon"));
        for (mode,press) in [(Some("press"),Some(true)),(Some("inscribe"),Some(false)),(Some("Inscribe"),Some(false)),(Some("stamp"),Some(false)),(None,None)] {
            let (items,recipes)=silicon_print("mode",mode);
            let found=live_inscriber_recipe(&m,&items,&recipes);
            assert_eq!(found.map(|(out,press)|(out.item.to_string(),out.count,press)),press.map(|press|("ae2:printed_silicon".to_string(),1,press)),"mode {mode:?}");
        }
    }
    #[test]
    fn inscriber_matches_the_ae2_oracle_run() {
        // oracle/traces/inscriber-summary.txt, AE2 19.2.18: an Inscriber
        // with 1 silicon press on top and 2 silicon in the middle next to a
        // full Energy Cell (200,000 AE); tick k is the k-th server tick after
        // the fixture was placed.
        let (items,recipes)=silicon_print("oracle",Some("inscribe"));
        let mut m=Machine::new(MachineKind::Inscriber);
        m.slots[0]=Some(simple_stack("ae2:silicon_press"));
        let mut silicon=simple_stack("ae2:silicon");silicon.count=2;m.slots[2]=Some(silicon);
        let mut cell=200_000.0f64;
        // Tick 1 creates the grid node and its TickTracker: TickRates.Inscriber
        // 1..20 (ATM10's defaults), so the first call comes 10 ticks later.
        let sleeping=inscriber_modulation(&m,&items,&recipes,false)==Modulation::Sleep;
        let mut owner=ProcessorOwner{tracker:Tracker::new(1,1,20,sleeping),input:processing_input(&m),service:None};
        let (mut powered,mut printed)=(Vec::new(),Vec::new());
        for tick in 1..=300u64 {
            if owner.due(tick,processing_input(&m)).is_none() {continue}
            let (before,output)=(cell,m.slots[3].as_ref().map_or(0,|s|s.count));
            inscriber_tick(&mut m,&items,&recipes,|amount,simulate|{let got=amount.min(cell);if !simulate {cell-=got;}got});
            owner.tracker.completed(tick,inscriber_modulation(&m,&items,&recipes,false));owner.input=processing_input(&m);
            if cell!=before {powered.push((tick,before-cell));}
            if m.slots[3].as_ref().map_or(0,|s|s.count)!=output {printed.push(tick);}
        }
        // 101 powered ticks of 20 AE per item (+2 progress, 202 > 200), the
        // item 8 ticks after the last, the next job 9 ticks after the item.
        let expected:Vec<_>=(11..=111).chain(128..=228).map(|tick|(tick,20.0)).collect();
        assert_eq!(powered,expected);
        assert_eq!(printed,[119,236]);
        assert_eq!(cell,195_960.0);
        assert_eq!(m.energy,0.0);
        assert_eq!(m.slots[0],Some(simple_stack("ae2:silicon_press")));
        assert_eq!((&m.slots[1],&m.slots[2]),(&None,&None));
        assert_eq!(m.slots[3].as_ref().map(|s|(s.item.as_ref(),s.count)),Some(("ae2:printed_silicon",2)));
        // Idle from then on: the device sleeps.
        assert!(!owner.tracker.is_awake());
    }
}

/// Laboratory crafting pattern component. The caller is responsible for
/// consuming one actual blank pattern before giving the returned item.
pub(super) fn ae2_encode_pattern(
    grid: &[Option<ItemStack>],
    output: &ItemStack,
) -> Result<ItemStack, String> {
    if grid.len() != 9
        || grid.iter().all(Option::is_none)
        || grid.iter().flatten().any(|s| s.count != 1)
        || output.count == 0
    {
        return Err("pattern needs a nonempty one-item crafting grid".into());
    }
    ae2_encode_pattern_with_substitutions(grid, output, false)
}
pub(super) fn ae2_encode_pattern_with_substitutions(
    grid: &[Option<ItemStack>],
    output: &ItemStack,
    substitutes: bool,
) -> Result<ItemStack, String> {
    ae2_encode_pattern_options(grid, output, substitutes, false)
}
pub(super) fn ae2_encode_pattern_options(
    grid: &[Option<ItemStack>],
    output: &ItemStack,
    substitutes: bool,
    fluid_substitutes: bool,
) -> Result<ItemStack, String> {
    if grid.len() != 9
        || grid.iter().all(Option::is_none)
        || grid.iter().flatten().any(|s| s.count != 1)
        || output.count == 0
    {
        return Err("pattern needs a nonempty one-item crafting grid".into());
    }
    let mut p = simple_stack("ae2:crafting_pattern");
    p.data_mut().set_other(
        "minecraft:custom_data",
        Some(Tag::compound([(
            "ae2_lab_pattern",
            Tag::compound([
                ("grid", horizon_client::save::encode_items(grid)),
                ("output", horizon_client::save::item_to_nbt(output)),
                ("substitutes", Tag::Byte(i8::from(substitutes))),
                ("fluid_substitutes", Tag::Byte(i8::from(fluid_substitutes))),
            ]),
        )])),
    );
    Ok(p)
}
/// Persist the actual selected recipe using the source pattern component.
pub(super) fn ae2_encode_pattern_source(
    grid: &[Option<ItemStack>], output: &ItemStack, recipe: &str,
    substitutes: bool, fluid_substitutes: bool,
) -> Result<ItemStack, String> {
    let source = super::ae2_source_patterns::encode_crafting(grid, output, recipe, substitutes, fluid_substitutes)?;
    let mut pattern = ae2_encode_pattern_options(grid, output, substitutes, fluid_substitutes)?;
    pattern.data_mut().set_other("ae2:encoded_crafting_pattern", Some(source));
    Ok(pattern)
}

/// Source data is authoritative when present; malformed source data never
/// revives stale legacy options.
pub(super) fn ae2_crafting_pattern_options(pattern: &ItemStack) -> Option<(bool, bool)> {
    if let Some(source) = super::ae2_source_patterns::crafting(pattern) {
        let source = source.ok()?;
        return Some((source.substitutes, source.fluid_substitutes));
    }
    let data = pattern.data()?.other("minecraft:custom_data")?.get("ae2_lab_pattern")?;
    Some((matches!(data.get("substitutes"), Some(Tag::Byte(1))),
          matches!(data.get("fluid_substitutes"), Some(Tag::Byte(1)))))
}
/// Upstream special patterns use the middle row of the Molecular Assembler,
/// while the encoding UI stores its compact logical input slots first.
pub(super) fn special_assembler_grid(kind:&str,logical:&[Option<ItemStack>])->Option<Vec<Option<ItemStack>>> {
    if logical.len()!=9{return None;}let mut physical=vec![None;9];match kind {
        "stonecutting"=>{if logical[0].is_none() || logical.iter().skip(1).any(Option::is_some){return None;}physical[4]=logical[0].clone();}
        "smithing"=>{if logical.iter().take(3).any(Option::is_none)||logical.iter().skip(3).any(Option::is_some){return None;}physical[3..6].clone_from_slice(&logical[..3]);}
        _=>return None,
    }Some(physical)
}
pub(super) fn special_logical_grid(kind:&str,physical:&[Option<ItemStack>])->Option<Vec<Option<ItemStack>>> {
    if physical.len()!=9{return None;}let mut logical=vec![None;9];match kind {
        "stonecutting"=>{if physical[4].is_none() || physical.iter().enumerate().any(|(i,s)|i!=4 && s.is_some()){return None;}logical[0]=physical[4].clone();}
        "smithing"=>{if physical[3..6].iter().any(Option::is_none)||physical.iter().enumerate().any(|(i,s)|!(3..6).contains(&i)&&s.is_some()){return None;}logical[..3].clone_from_slice(&physical[3..6]);}
        _=>return None,
    }Some(logical)
}
pub(super) fn special_recipe_result(data:&horizon_client::stations::StationData,kind:&str,recipe:&str,grid:&[Option<ItemStack>])->Option<ItemStack> {
    if grid.len()!=9{return None;}
    match kind {
        "stonecutting"=>{if grid.iter().skip(1).any(Option::is_some){return None;}let item=grid[0].as_ref()?;data.recipes.stonecutter(item).into_iter().find(|r|r.id==recipe).map(|r|r.result.clone())},
        "smithing"=>{if grid.iter().skip(3).any(Option::is_some){return None;}data.recipes.smithing_recipe_result_selected(&data.enchantments,grid[0].as_ref(),grid[1].as_ref(),grid[2].as_ref(),Some(recipe)).map(|(_,result)|result)},
        _=>None
    }
}
/// `IMolecularAssemblerSupportedPattern.isItemValid`: use the named live
/// recipe for source patterns, and compare each proposed substitution with
/// that recipe's current output rather than its saved display fallback.
pub(in crate::game) fn assembler_pattern_accepts(
    pattern:&ItemStack,slot:usize,stack:&ItemStack,
    items:&Items,recipes:&horizon_client::crafting::Recipes,stations:&horizon_client::stations::StationData,
)->bool {
    let known=|s:&ItemStack|s.count>0 && s.item.as_ref()!="ae2:missing_content" && items.get(&s.item).is_some();
    if slot>=9 || !known(stack) {return false;}
    if let Some(special)=ae2_decode_special_pattern(pattern) {
        if !known(&special.output) || special.grid.iter().flatten().any(|item|!known(item)){return false;}
        let Some(mut physical)=special_assembler_grid(&special.kind,&special.grid) else{return false;};
        let Some(actual)=special_recipe_result(stations,&special.kind,&special.recipe,&special.grid) else{return false;};
        if !known(&actual){return false;}
        if super::ae2_source_patterns::special(pattern).is_none() && actual!=special.output {return false;}
        let Some(expected)=physical[slot].as_ref() else{return false;};
        if expected.same_item_same_components(stack){return true;}
        if !special.substitutes{return false;}
        let mut proposed=stack.clone();proposed.count=1;physical[slot]=Some(proposed);
        let Some(logical)=special_logical_grid(&special.kind,&physical) else{return false;};
        return special_recipe_result(stations,&special.kind,&special.recipe,&logical)==Some(actual);
    }
    let Some((mut grid,display))=decode_pattern(pattern) else{return false;};
    if !known(&display) || grid.iter().flatten().any(|item|!known(item)){return false;}
    let source=super::ae2_source_patterns::crafting(pattern).and_then(Result::ok);
    let (recipe,actual)=if let Some(source)=source {
        let Some(actual)=recipes.assemble_named(&source.recipe,&grid,3) else{return false;};
        (source.recipe,actual)
    } else {
        let Some((name,actual))=recipes.find(&grid,3) else{return false;};
        if actual!=display{return false;}(name.to_owned(),actual)
    };
    if !known(&actual){return false;}
    let Some(expected)=grid[slot].as_ref() else{return false;};
    if expected.same_item_same_components(stack){return true;}
    if !ae2_crafting_pattern_options(pattern).is_some_and(|options|options.0){return false;}
    let mut proposed=stack.clone();proposed.count=1;grid[slot]=Some(proposed);
    recipes.assemble_named(&recipe,&grid,3)==Some(actual)
}
pub(in crate::game) fn assembler_pattern_valid(
    pattern:&ItemStack,items:&Items,recipes:&horizon_client::crafting::Recipes,stations:&horizon_client::stations::StationData,
)->bool {
    let known=|s:&ItemStack|s.count>0 && s.item.as_ref()!="ae2:missing_content" && items.get(&s.item).is_some();
    if let Some(special)=ae2_decode_special_pattern(pattern) {
        if !known(&special.output) || special.grid.iter().flatten().any(|item|!known(item)){return false;}
        return special_recipe_result(stations,&special.kind,&special.recipe,&special.grid)
            .is_some_and(|actual|known(&actual) && (super::ae2_source_patterns::special(pattern).is_some() || actual==special.output));
    }
    let Some((grid,display))=decode_pattern(pattern) else{return false;};
    if !known(&display) || grid.iter().flatten().any(|item|!known(item)){return false;}
    if let Some(Ok(source))=super::ae2_source_patterns::crafting(pattern) {
        recipes.assemble_named(&source.recipe,&grid,3).is_some_and(|actual|known(&actual))
    } else {
        recipes.find(&grid,3).is_some_and(|(_,actual)|known(&actual) && actual==display)
    }
}
pub(in crate::game) fn assembler_pattern_slot_enabled(
    pattern:&ItemStack,slot:usize,items:&Items,recipes:&horizon_client::crafting::Recipes,stations:&horizon_client::stations::StationData,
)->bool {
    if slot>=9 || !assembler_pattern_valid(pattern,items,recipes,stations){return false;}
    if let Some(special)=ae2_decode_special_pattern(pattern) {
        return special_assembler_grid(&special.kind,&special.grid).is_some_and(|grid|grid[slot].is_some());
    }
    decode_pattern(pattern).is_some_and(|(grid,_)|grid[slot].is_some())
}
#[derive(Clone,Debug,PartialEq)]
pub(super) struct SpecialPattern {pub kind:String,pub recipe:String,pub grid:Vec<Option<ItemStack>>,pub output:ItemStack,pub substitutes:bool}
pub(super) fn ae2_encode_special_pattern(kind:&str,recipe:&str,grid:&[Option<ItemStack>],output:&ItemStack,substitutes:bool)->Result<ItemStack,String> {
    let item=match kind {"smithing"=>"ae2:smithing_table_pattern","stonecutting"=>"ae2:stonecutting_pattern",_=>return Err("invalid special pattern kind".into())};
    if recipe.is_empty() || (kind=="stonecutting" && (grid.get(0).is_none_or(Option::is_none) || grid.iter().skip(1).any(Option::is_some))) || (kind=="smithing" && (grid.iter().take(3).any(Option::is_none) || grid.iter().skip(3).any(Option::is_some))) {return Err("invalid special pattern input".into());}
    let mut p=ae2_encode_pattern_options(grid,output,substitutes,false)?;p.item=item.into();
    let Some(Tag::Compound(mut custom))=p.data().and_then(|d|d.other("minecraft:custom_data")).cloned() else{return Err("missing pattern data".into())};
    let Some(Tag::Compound(data))=custom.get_mut(&"ae2_lab_pattern".into()) else{return Err("missing pattern data".into())};
    data.insert("special_kind".into(),Tag::String(kind.into()));data.insert("recipe".into(),Tag::String(recipe.into()));
    p.data_mut().set_other("minecraft:custom_data",Some(Tag::Compound(custom)));
    let source=super::ae2_source_patterns::encode_special(kind,grid,output,recipe,substitutes)?;
    let component=if kind=="smithing" {"ae2:encoded_smithing_table_pattern"} else {"ae2:encoded_stonecutting_pattern"};
    p.data_mut().set_other(component,Some(source));
    Ok(p)
}
pub(super) fn ae2_decode_special_pattern(p:&ItemStack)->Option<SpecialPattern> {
    let kind=match p.item.as_ref() {"ae2:smithing_table_pattern"=>"smithing","ae2:stonecutting_pattern"=>"stonecutting",_=>return None};
    if p.count!=1{return None;}
    if let Some(source)=super::ae2_source_patterns::special(p) {
        let source=source.ok()?;
        return Some(SpecialPattern{kind:kind.into(),recipe:source.recipe,grid:source.grid,output:source.output,substitutes:source.substitutes});
    }
    let data=p.data()?.other("minecraft:custom_data")?.get("ae2_lab_pattern")?;
    let Tag::String(saved_kind)=data.get("special_kind")? else{return None};if saved_kind.to_string().ok()?.as_str()!=kind{return None;}
    let Tag::String(recipe)=data.get("recipe")? else{return None};let recipe=recipe.to_string().ok()?;if recipe.is_empty(){return None;}
    let grid=strict_pattern_slots(data.get("grid")?,9)?;let output=horizon_client::save::item_from_nbt(data.get("output")?)?;
    let substitutes=matches!(data.get("substitutes"),Some(Tag::Byte(1)));
    if grid.iter().flatten().any(|s|s.count!=1) || output.count==0 || (kind=="stonecutting" && (grid[0].is_none() || grid.iter().skip(1).any(Option::is_some))) || (kind=="smithing" && (grid.iter().take(3).any(Option::is_none) || grid.iter().skip(3).any(Option::is_some))) {return None;}
    Some(SpecialPattern{kind:kind.into(),recipe,grid,output,substitutes})
}
pub(super) fn decode_pattern(p: &ItemStack) -> Option<(Vec<Option<ItemStack>>, ItemStack)> {
    if p.item.as_ref() != "ae2:crafting_pattern" || p.count != 1 {
        return None;
    }
    if let Some(source) = super::ae2_source_patterns::crafting(p) {
        let source = source.ok()?;
        return Some((source.grid, source.output));
    }
    let data = p
        .data()?
        .other("minecraft:custom_data")?
        .get("ae2_lab_pattern")?;
    let grid = horizon_client::save::decode_items(Some(data.get("grid")?), 9);
    if grid.iter().all(Option::is_none) || grid.iter().flatten().any(|s| s.count != 1) {
        return None;
    }
    let output = horizon_client::save::item_from_nbt(data.get("output")?)?;
    Some((grid, output))
}
/// Build the assembler's persistent recipe snapshot from either source or
/// legacy pattern data. The pattern itself remains physically owned by the
/// provider or assembler; this record only captures the recipe being run.
pub(super) fn assembler_pattern_data(pattern: &ItemStack) -> Option<Tag> {
    let (grid, output, special, recipe) = if let Some(special) = ae2_decode_special_pattern(pattern) {
        let grid = special_assembler_grid(&special.kind, &special.grid)?;
        (grid, special.output, Some((special.kind, special.recipe)), None)
    } else {
        let (grid, output) = decode_pattern(pattern)?;
        let recipe = super::ae2_source_patterns::crafting(pattern).and_then(Result::ok).map(|source| source.recipe);
        (grid, output, None, recipe)
    };
    let mut data = match Tag::compound([
        ("grid", horizon_client::save::encode_items(&grid)),
        ("output", horizon_client::save::item_to_nbt(&output)),
    ]) { Tag::Compound(values) => values, _ => unreachable!() };
    if let Some((kind, recipe)) = special {
        data.insert("special_kind".into(), Tag::String(kind.as_str().into()));
        data.insert("recipe".into(), Tag::String(recipe.as_str().into()));
    }
    if let Some(recipe) = recipe { data.insert("crafting_recipe".into(), Tag::String(recipe.as_str().into())); }
    // Forced plans are not owned physical patterns, but the menu must still
    // validate against the exact definition, including its substitutions.
    data.insert("definition".into(),horizon_client::save::item_to_nbt(pattern));
    Some(Tag::Compound(data))
}
/// The source constructs a positioned CraftingInput and compares its dimensions,
/// rather than requiring the input rectangle to remain at the encoded offset.
fn assembler_bounds(grid: &[Option<ItemStack>]) -> Option<(usize,usize,usize,usize)> {
    if grid.len()!=9 {return None;}
    let mut left=3;let mut top=3;let mut right=0;let mut bottom=0;let mut any=false;
    for (i,slot) in grid.iter().enumerate() {
        if slot.is_some() {let (x,y)=(i%3,i/3);left=left.min(x);top=top.min(y);right=right.max(x);bottom=bottom.max(y);any=true;}
    }
    if any {Some((left,top,right-left+1,bottom-top+1))} else {None}
}
fn assembler_recipe_is_special(recipe:&horizon_client::crafting::Recipe)->bool {
    match recipe {
        horizon_client::crafting::Recipe::Special(horizon_client::crafting::Special::Transmute{..}
            |horizon_client::crafting::Special::Dye{..}
            |horizon_client::crafting::Special::Imbue{..})=>false,
        horizon_client::crafting::Recipe::Special(_)=>true,
        _=>false,
    }
}
/// AE2's AECraftingPattern.assemble validates ordinary recipes per original
/// slot, whereas an allowed special recipe assembles the combined live grid.
/// Stonecutting and smithing assemble from the compressed live input instead.
fn assembler_pattern_assemble(
    pattern:&ItemStack,grid:&[Option<ItemStack>],items:&Items,
    recipes:&horizon_client::crafting::Recipes,stations:&horizon_client::stations::StationData,
)->Option<(Option<String>,ItemStack)> {
    if !assembler_pattern_valid(pattern,items,recipes,stations) || grid.len()!=9
        || grid.iter().flatten().any(|item|item.count!=1 || items.get(&item.item).is_none()) {return None;}
    let (actual_left,actual_top,actual_width,actual_height)=assembler_bounds(grid)?;
    if let Some(special)=ae2_decode_special_pattern(pattern) {
        let size=actual_width*actual_height;
        if (special.kind=="stonecutting" && size!=1) || (special.kind=="smithing" && size<3) {return None;}
        let mut logical=vec![None;9];
        for i in 0..size.min(3) {
            let x=actual_left+i%actual_width;let y=actual_top+i/actual_width;
            logical[i]=grid[x+y*3].clone();
        }
        return special_recipe_result(stations,&special.kind,&special.recipe,&logical).map(|result|(None,result));
    }
    let (original,_)=decode_pattern(pattern)?;
    let (left,top,width,height)=assembler_bounds(&original)?;
    if (width,height)!=(actual_width,actual_height) {return None;}
    let source=super::ae2_source_patterns::crafting(pattern).and_then(Result::ok);
    let (recipe,original_output)=if let Some(source)=source {
        let output=recipes.assemble_named(&source.recipe,&original,3)?;(source.recipe,output)
    } else {
        let (recipe,output)=recipes.find(&original,3)?;(recipe.to_owned(),output)
    };
    let recipe_name=recipe.strip_prefix("minecraft:").unwrap_or(&recipe);
    // Horizon groups several data-driven normal crafting recipes under its
    // Special enum. Minecraft's Transmute/Dye/Imbue extend NormalCraftingRecipe
    // and inherit Recipe.isSpecial() == false; CustomRecipe returns true.
    let dynamic=recipes.crafting.iter().find(|(name,_)|name.strip_prefix("minecraft:").unwrap_or(name)==recipe_name)
        .map_or(true,|(_,recipe)|assembler_recipe_is_special(recipe));
    if dynamic && ae2_crafting_pattern_options(pattern).is_some_and(|options|options.0) {
        let result=recipes.assemble_named_special_unchecked(&recipe,grid,3)?;
        return Some((Some(recipe_name.to_owned()),result));
    }
    for y in 0..height {for x in 0..width {
        let original_slot=(left+x)+(top+y)*3;let actual_slot=(actual_left+x)+(actual_top+y)*3;
        match (&original[original_slot],&grid[actual_slot]) {
            (None,None)=>{},
            (Some(_),Some(item)) if assembler_pattern_accepts(pattern,original_slot,item,items,recipes,stations)=>{},
            _=>return None,
        }
    }}
    Some((Some(recipe_name.to_owned()),original_output))
}
/// Existing snapshots without a persisted definition retain the old exact-grid
/// check; an invalid explicit definition never falls back to legacy metadata.
fn assembler_recipe_result(
    data:&Tag,expected_grid:&[Option<ItemStack>],grid:&[Option<ItemStack>],
    items:&Items,recipes:&horizon_client::crafting::Recipes,stations:&horizon_client::stations::StationData,
)->Option<(Option<String>,ItemStack)> {
    if let Some(definition)=data.get("definition") {
        let pattern=horizon_client::save::item_from_nbt(definition)?;
        let (recipe,result)=assembler_pattern_assemble(&pattern,grid,items,recipes,stations)?;
        return (result.count>0 && items.get(&result.item).is_some()).then_some((recipe,result));
    }
    if !same_grid(grid,expected_grid) {return None;}
    let expected=data.get("output").and_then(horizon_client::save::item_from_nbt)?;
    let special=match (data.get("special_kind"),data.get("recipe")) {
        (Some(Tag::String(kind)),Some(Tag::String(recipe)))=>Some((kind.to_string().ok()?,recipe.to_string().ok()?)),
        (None,None)=>None,
        _=>return None,
    };
    let (recipe,result)=if let Some((kind,id))=&special {
        let logical=special_logical_grid(kind,grid).unwrap_or_else(||grid.to_vec());
        (None,special_recipe_result(stations,kind,id,&logical)?)
    } else if let Some(Tag::String(recipe))=data.get("crafting_recipe") {
        let recipe=recipe.to_string().ok()?;let result=recipes.assemble_named(&recipe,grid,3)?;
        (Some(recipe.strip_prefix("minecraft:").unwrap_or(&recipe).to_owned()),result)
    } else {
        let (recipe,result)=recipes.find(grid,3)?;(Some(recipe.to_owned()),result)
    };
    if (data.get("crafting_recipe").is_none() && special.is_none() && result!=expected)
        || items.get(&result.item).is_none() {return None;}
    Some((recipe,result))
}
fn adjacent(a: (i32, i32, i32), b: (i32, i32, i32)) -> bool {
    (i64::from(a.0) - i64::from(b.0)).abs()
        + (i64::from(a.1) - i64::from(b.1)).abs()
        + (i64::from(a.2) - i64::from(b.2)).abs()
        == 1
}
fn same_grid(a: &[Option<ItemStack>], b: &[Option<ItemStack>]) -> bool {
    a.len() == 9
        && b.len() == 9
        && a.iter().zip(b).all(|(a, b)| match (a, b) {
            (None, None) => true,
            (Some(a), Some(b)) => a.count == 1 && a.same_item_same_components(b),
            _ => false,
        })
}
impl Game<'_> {
    /// Moves real matching inputs from a vanilla container next to the provider
    /// into the adjacent assembler. Caller must not reserve the same stacks in
    /// a separate CPU escrow. Failed dispatch leaves both inventories unchanged.
    pub(super) fn ae2_dispatch_pattern(
        &mut self,
        provider: (i32, i32, i32),
        assembler: (i32, i32, i32),
        source: (i32, i32, i32),
        pattern_slot: usize,
    ) -> Result<(), String> {
        if !adjacent(provider, assembler) || !adjacent(provider, source) || assembler == source {
            return Err("provider, assembler and source must be adjacent".into());
        }
        if !self.ae2_online(provider) || !self.ae2_online(assembler) {
            return Err("provider or assembler is offline".into());
        }
        let Some(BlockEntity::Ae2(provider_be)) = self.world.block_entities.get(&provider) else {
            return Err("no provider".into());
        };
        if provider_be.kind != MachineKind::PatternProvider {
            return Err("no provider".into());
        }
        let pattern = provider_be
            .slots
            .get(pattern_slot)
            .and_then(Option::as_ref)
            .ok_or("empty pattern slot")?;
        let (grid, expected) = decode_pattern(pattern).ok_or("invalid laboratory pattern")?;
        let source_recipe = super::ae2_source_patterns::crafting(pattern).and_then(Result::ok).map(|source| source.recipe);
        let result = match source_recipe.as_deref() {
            Some(recipe) => self.res.recipes().assemble_named(recipe, &grid, 3),
            None => self.res.recipes().find(&grid, 3).map(|(_, result)| result),
        }.ok_or("recipe unavailable")?;
        if source_recipe.is_none() && result != expected {
            return Err("pattern result does not match recipe".into());
        }
        let Some(BlockEntity::Ae2(a)) = self.world.block_entities.get(&assembler) else {
            return Err("no assembler".into());
        };
        if a.kind != MachineKind::MolecularAssembler
            || a.slots.len() != 11
            || a.slots.iter().any(Option::is_some)
            || a.config.contains_key(&"ae2_assembler_pattern".into())
        {
            return Err("assembler is busy".into());
        }
        let Some(BlockEntity::Chest(source_be)) = self.world.block_entities.get(&source) else {
            return Err("source must be a physical container".into());
        };
        let mut source_next = source_be.clone();
        let mut staged = vec![None; 9];
        for (i, wanted) in grid.iter().enumerate() {
            if let Some(wanted) = wanted {
                let Some(slot) = source_next.iter_mut().find(|s| {
                    s.as_ref()
                        .is_some_and(|s| s.count > 0 && s.same_item_same_components(wanted))
                }) else {
                    return Err("missing input in source container".into());
                };
                let actual = slot.as_ref().unwrap();
                staged[i] = Some(ItemStack {
                    count: 1,
                    ..actual.clone()
                });
                consume(slot);
            }
        }
        let pattern_data = assembler_pattern_data(pattern).ok_or("invalid pattern")?;
        if let Some(BlockEntity::Chest(chest)) = self.world.block_entities.get_mut(&source) {
            *chest = source_next;
        } else {
            return Err("source changed".into());
        }
        if let Some(BlockEntity::Ae2(a)) = self.world.block_entities.get_mut(&assembler) {
            a.slots[..9].clone_from_slice(&staged);
            a.progress = 0.0;
            a.config
                .insert("ae2_assembler_pattern".into(), pattern_data);
        } else {
            unreachable!("world is single-threaded during dispatch");
        }
        self.world.modified.insert((source.0 >> 4, source.2 >> 4));
        self.world
            .modified
            .insert((assembler.0 >> 4, assembler.2 >> 4));
        Ok(())
    }
    fn ae2_tick_scheduled_assembler(&mut self,pos:(i32,i32,i32),m:&mut Machine,original:&Machine,plan:&mut super::ae2_buses::FormationPlan)->bool {
        let Some(address)=self.ae2_local_address(pos) else{return false;};let network=self.ae2_network();
        let Some(identity)=network.node_identities.get(&address.network_pos()).copied() else{return false;};
        let services=self.ae2_event_service_ids(&network);let service=services.as_ref().and_then(|s|s.get(&identity)).copied();
        let now=self.ae2_server_tick();let mut fields=m.slots.iter().map(|s|horizon_client::item_data::canon_stack(s.as_ref())).collect::<Vec<_>>();
        fields.push(m.config.get(&"ae2_assembler_pattern".into()).map(horizon_client::item_data::canon_tag).unwrap_or_default());
        let Ok(token)=serde_json::to_vec(&fields) else{return false;};if token.len()>1_048_576 {return false;}
        let sleeping=m.slots.iter().take(10).all(Option::is_none) && !m.config.contains_key(&"ae2_assembler_pattern".into());
        let due=self.ae2_device_scheduler.borrow_mut().due_with_token(identity,service,now,1,1,sleeping,&token);
        if !due{return false;}let changed=self.ae2_tick_assembler(pos,m,original,plan);
        let modulation=if sleeping {super::ae2_tick_tracker::Modulation::Sleep}else{super::ae2_tick_tracker::Modulation::Same};
        self.ae2_device_scheduler.borrow_mut().completed(identity,now,modulation);changed
    }
    /// MolecularAssemblerBlockEntity.pushTo: the neighbour on `face` takes
    /// what it can through its facing side, staged as the Inscriber's export.
    fn ae2_assembler_push_to(&mut self,pos:(i32,i32,i32),face:horizon_core::geometry::Direction,stack:&ItemStack,original:&Machine,plan:&mut super::ae2_buses::FormationPlan)->u32 {
        let [dx,dy,dz]=face.normal();
        let Some(target)=self.ae2_local_address((pos.0+dx,pos.1+dy,pos.2+dz)) else{return 0};
        let mut offered=stack.clone();let offer=offered.count;
        horizon_client::ae2_stack_identity::detach_if_tracked(&mut offered);
        let accepted=match self.ae2_item_p2p_insert(target,face.opposite(),&offered,true) {
            Some(n)=>n.min(offer),None=>self.ae2_native_item_insert(target,face.opposite(),&offered,offer,true),
        };
        if accepted==0{return 0}
        let before=self.ae2_snapshot_blocks();let previous=plan.len();
        let moved=match self.ae2_item_p2p_insert_staged(target,face.opposite(),&offered,false,plan) {
            Some(n)=>n,None=>self.ae2_native_item_insert_staged(target,face.opposite(),&offered,offer,false,plan),
        };
        let source_unchanged=matches!(self.world.block_entities.get(&pos),Some(BlockEntity::Ae2(current)) if **current==*original);
        if moved==0 || moved>offer || !source_unchanged {self.ae2_restore_blocks(before);plan.truncate(previous);return 0}
        moved
    }
    /// MolecularAssemblerBlockEntity.pushOut of the output slot, then
    /// ejectHeldItems.
    fn ae2_assembler_push_out(&mut self,pos:(i32,i32,i32),m:&mut Machine,original:&Machine,plan:&mut super::ae2_buses::FormationPlan)->bool {
        let mut changed=false;
        if let Some(output)=m.slots[9].clone() {
            let rest=assembler_push_out(output.clone(),|face,stack|self.ae2_assembler_push_to(pos,face,stack,original,plan));
            if rest.as_ref()!=Some(&output) {
                if let Some(rest)=&rest {horizon_client::ae2_stack_identity::synchronize_tracked(rest);}
                m.slots[9]=rest;changed=true;
            }
        }
        assembler_eject_held(m,|m,slot,stack|self.ae2_assembler_accepts(m,slot,stack)) || changed
    }
    fn ae2_tick_assembler(&mut self, pos: (i32, i32, i32), m: &mut Machine, original: &Machine, plan: &mut super::ae2_buses::FormationPlan) -> bool {
        if m.slots.len() != 11 {
            return false;
        }
        if m.slots[9].is_some() {
            // MolecularAssemblerBlockEntity.tickingRequest: a held output is
            // pushed out first, then a held item is ejected, and nothing crafts.
            return assembler_pushes_out(m) && self.ae2_assembler_push_out(pos, m, original, plan);
        }
        let pattern_data = m
            .config
            .get(&"ae2_assembler_pattern".into())
            .cloned()
            .or_else(|| m.slots[10].as_ref().and_then(assembler_pattern_data));
        let Some(data) = pattern_data else {
            return false;
        };
        let expected_grid = horizon_client::save::decode_items(data.get("grid"), 9);
        let Some((recipe_grid, fluid_slots)) = assembler_recipe_grid(m, &expected_grid) else {
            return false;
        };
        let Some((recipe,result))=assembler_recipe_result(&data,&expected_grid,&recipe_grid,
            self.res.items(),self.res.recipes(),self.res.stations()) else{return false;};
        if !self.ae2_online(pos) {
            return false;
        }
        let (speed, tax) = match m.installed_speed_cards().min(5) {
            0 => (10.0, 1.0),
            1 => (13.0, 1.3),
            2 => (17.0, 1.7),
            3 => (20.0, 2.0),
            4 => (25.0, 2.5),
            _ => (50.0, 5.0),
        };
        let extracted = self.ae2_take_power(pos, speed * tax, false);
        let gain = (extracted / tax).floor();
        if gain <= 0.0 {
            return false;
        }
        m.progress += gain;
        if m.progress >= 100.0 {
            let mut remaining = if let Some(recipe)=recipe.as_deref() {
                let items=self.res.items();let enchantments=&self.res.stations().enchantments;let random=&mut self.world.random;
                let mut loss=|stack:&ItemStack,amount:u32|horizon_client::crafting::committed_durability_loss(enchantments,items,stack,random,amount);
                self.res.recipes().remainders_with_durability(recipe,&recipe_grid,3,Some(&mut loss))
            } else {vec![None;9]};
            for slot in fluid_slots {
                remaining[slot] = None;
            }
            m.slots[..9].clone_from_slice(&remaining);
            m.slots[9] = Some(result);
            m.progress = 0.0;
            m.config.remove(&"ae2_assembler_pattern".into());
            m.config.remove(&"ae2_assembler_fluid_escrow".into());
            if assembler_pushes_out(m) {
                self.ae2_assembler_push_out(pos, m, original, plan);
            }
        }
        true
    }
}

#[cfg(test)]
mod pattern_tests {
    use super::*;
    #[test]
    fn assembler_source_pattern_accepts_only_enabled_named_recipe_inputs() {
        let root=std::env::temp_dir().join(format!("ae2-assembler-validator-{}",std::process::id()));
        let dir=root.join("data/minecraft/recipe");std::fs::create_dir_all(&dir).unwrap();
        // Items::catalog scans the vanilla loot table directory even in this
        // minimal recipe fixture.
        std::fs::create_dir_all(root.join("data/minecraft/loot_table")).unwrap();
        std::fs::write(dir.join("a_first.json"),r#"{"type":"minecraft:crafting_shapeless","ingredients":["minecraft:stone"],"result":{"id":"minecraft:dirt","count":1}}"#).unwrap();
        std::fs::write(dir.join("b_named.json"),r#"{"type":"minecraft:crafting_shapeless","ingredients":[["minecraft:stone","minecraft:andesite"]],"result":{"id":"minecraft:diamond","count":1}}"#).unwrap();
        std::fs::write(dir.join("overlap.json"),r#"{"type":"minecraft:crafting_shapeless","ingredients":["minecraft:stone",["minecraft:stone","minecraft:andesite"]],"result":{"id":"minecraft:diamond","count":1}}"#).unwrap();
        std::fs::write(dir.join("transmute.json"),r#"{"type":"minecraft:crafting_transmute","input":"minecraft:stone","material":"minecraft:andesite","result":{"id":"minecraft:diamond"}}"#).unwrap();
        std::fs::write(dir.join("repair.json"),r#"{"type":"minecraft:crafting_special_repairitem"}"#).unwrap();
        std::fs::write(dir.join("cut.json"),r#"{"type":"minecraft:stonecutting","ingredient":["minecraft:stone","minecraft:andesite"],"result":{"id":"minecraft:stone_slab","count":2}}"#).unwrap();
        std::fs::write(dir.join("smith.json"),r#"{"type":"minecraft:smithing_transform","template":"minecraft:netherite_upgrade_smithing_template","base":"minecraft:diamond_sword","addition":"minecraft:netherite_ingot","result":{"id":"minecraft:netherite_sword"}}"#).unwrap();
        std::fs::write(root.join("items.json"),r#"{"minecraft:stone":{},"minecraft:andesite":{},"minecraft:dirt":{},"minecraft:diamond":{},"minecraft:gold_ingot":{},"minecraft:stone_slab":{},"minecraft:netherite_upgrade_smithing_template":{},"minecraft:diamond_sword":{},"minecraft:netherite_ingot":{},"minecraft:netherite_sword":{}}"#).unwrap();
        let items=Items::catalog(&root, &horizon_world::loot::PackOverlay::NONE).unwrap();
        let recipes=horizon_client::crafting::Recipes::load(&root, &items, &horizon_world::loot::PackOverlay::NONE).unwrap();
        let stations=horizon_client::stations::StationData::load(&root, &items, &horizon_world::loot::PackOverlay::NONE);
        let named=|name:&str|&recipes.crafting.iter().find(|(id,_)|id==name).unwrap().1;
        assert!(!assembler_recipe_is_special(named("transmute")));
        assert!(assembler_recipe_is_special(named("repair")));
        let mut grid=vec![None;9];grid[0]=Some(simple_stack("minecraft:stone"));
        let stale=simple_stack("minecraft:gold_ingot");
        let mut pattern=ae2_encode_pattern_source(&grid,&stale,"minecraft:b_named",true,false).unwrap();
        pattern.data_mut().set_other("minecraft:custom_data",None);
        let accepts=|p:&ItemStack,slot,item:&str|assembler_pattern_accepts(p,slot,&simple_stack(item),&items,&recipes,&stations);
        assert!(assembler_pattern_valid(&pattern,&items,&recipes,&stations));
        assert!(assembler_pattern_slot_enabled(&pattern,0,&items,&recipes,&stations));
        assert!(!assembler_pattern_slot_enabled(&pattern,1,&items,&recipes,&stations));
        let snapshot=assembler_pattern_data(&pattern).unwrap();
        assert_eq!(horizon_client::save::item_from_nbt(snapshot.get("definition").unwrap()),Some(pattern.clone()));
        assert!(accepts(&pattern,0,"minecraft:stone"));assert!(accepts(&pattern,0,"minecraft:andesite"));
        assert!(!accepts(&pattern,0,"minecraft:dirt"));assert!(!accepts(&pattern,1,"minecraft:stone"));
        let mut actual=vec![None;9];actual[0]=Some(simple_stack("minecraft:andesite"));
        let expected=horizon_client::save::decode_items(snapshot.get("grid"),9);
        assert_eq!(assembler_recipe_result(&snapshot,&expected,&actual,&items,&recipes,&stations).unwrap().1,simple_stack("minecraft:diamond"));
        // Source AECraftingPattern compares compressed dimensions, so a moved
        // one-slot frame remains valid after it is loaded from an older save.
        actual[4]=actual[0].take();
        assert_eq!(assembler_recipe_result(&snapshot,&expected,&actual,&items,&recipes,&stations).unwrap().1,simple_stack("minecraft:diamond"));
        actual[5]=Some(simple_stack("minecraft:stone"));
        assert!(assembler_recipe_result(&snapshot,&expected,&actual,&items,&recipes,&stations).is_none());
        let mut bad_snapshot=snapshot.clone();
        if let Tag::Compound(values)=&mut bad_snapshot {values.insert("definition".into(),Tag::String("broken".into()));}
        assert!(assembler_recipe_result(&bad_snapshot,&expected,&expected,&items,&recipes,&stations).is_none());
        let mut old_snapshot=snapshot.clone();
        if let Tag::Compound(values)=&mut old_snapshot {values.remove(&"definition".into());}
        assert!(assembler_recipe_result(&old_snapshot,&expected,&expected,&items,&recipes,&stations).is_some());
        actual=vec![None;9];actual[0]=Some(simple_stack("minecraft:andesite"));
        assert!(assembler_recipe_result(&old_snapshot,&expected,&actual,&items,&recipes,&stations).is_none());
        // Each replacement is valid against the original inputs. AE2 returns
        // the original output even when the two replacements together no
        // longer match this overlapping shapeless recipe.
        let mut overlapping=vec![None;9];overlapping[0]=Some(simple_stack("minecraft:stone"));overlapping[1]=Some(simple_stack("minecraft:stone"));
        let mut pattern_overlap=ae2_encode_pattern_source(&overlapping,&stale,"minecraft:overlap",true,false).unwrap();
        pattern_overlap.data_mut().set_other("minecraft:custom_data",None);
        let overlap_snapshot=assembler_pattern_data(&pattern_overlap).unwrap();
        let mut both=overlapping.clone();both[0]=Some(simple_stack("minecraft:andesite"));both[1]=Some(simple_stack("minecraft:andesite"));
        assert!(recipes.assemble_named("minecraft:overlap",&both,3).is_none());
        assert_eq!(assembler_recipe_result(&overlap_snapshot,&overlapping,&both,&items,&recipes,&stations).unwrap().1,simple_stack("minecraft:diamond"));
        let mut strict=ae2_encode_pattern_source(&grid,&stale,"minecraft:b_named",false,false).unwrap();
        strict.data_mut().set_other("minecraft:custom_data",None);
        assert!(!accepts(&strict,0,"minecraft:andesite"));
        let invalid=ae2_encode_pattern_source(&grid,&simple_stack("removed:result"),"minecraft:b_named",true,false).unwrap();
        assert!(!assembler_pattern_valid(&invalid,&items,&recipes,&stations));
        let mut cut=ae2_encode_special_pattern("stonecutting","minecraft:cut",&grid,&stale,true).unwrap();
        cut.data_mut().set_other("minecraft:custom_data",None);
        assert!(assembler_pattern_valid(&cut,&items,&recipes,&stations));
        assert!(assembler_pattern_slot_enabled(&cut,4,&items,&recipes,&stations));
        assert!(!assembler_pattern_slot_enabled(&cut,0,&items,&recipes,&stations));
        assert!(accepts(&cut,4,"minecraft:stone"));assert!(accepts(&cut,4,"minecraft:andesite"));
        assert!(!accepts(&cut,0,"minecraft:stone"));
        let cut_snapshot=assembler_pattern_data(&cut).unwrap();
        let cut_expected=horizon_client::save::decode_items(cut_snapshot.get("grid"),9);
        let mut moved_cut=vec![None;9];moved_cut[0]=Some(simple_stack("minecraft:andesite"));
        assert_eq!(assembler_recipe_result(&cut_snapshot,&cut_expected,&moved_cut,&items,&recipes,&stations).unwrap().1,ItemStack::new("minecraft:stone_slab",2));
        let mut smith=vec![None;9];smith[0]=Some(simple_stack("minecraft:netherite_upgrade_smithing_template"));
        smith[1]=Some(simple_stack("minecraft:diamond_sword"));smith[2]=Some(simple_stack("minecraft:netherite_ingot"));
        let mut pattern=ae2_encode_special_pattern("smithing","minecraft:smith",&smith,&stale,false).unwrap();
        pattern.data_mut().set_other("minecraft:custom_data",None);
        assert!(accepts(&pattern,3,"minecraft:netherite_upgrade_smithing_template"));
        assert!(accepts(&pattern,4,"minecraft:diamond_sword"));assert!(accepts(&pattern,5,"minecraft:netherite_ingot"));
        assert!(!accepts(&pattern,0,"minecraft:diamond_sword"));
    }
    #[test]
    fn pattern_round_trip_preserves_components_and_assembler_physical_slots() {
        let mut input = simple_stack("minecraft:diamond_sword");
        input.damage = 4;
        input.enchant("minecraft:sharpness", 3);
        let mut grid = vec![None; 9];
        grid[4] = Some(input.clone());
        let output = simple_stack("minecraft:stick");
        let pattern = ae2_encode_pattern(&grid, &output).unwrap();
        let (decoded, result) = decode_pattern(&pattern).unwrap();
        assert_eq!(decoded, grid);
        assert_eq!(result, output);
        let m = Machine::new(MachineKind::MolecularAssembler);
        assert_eq!(m.slots.len(), 11);
        assert!(m.slots.iter().all(Option::is_none));
        grid[4].as_mut().unwrap().damage = 5;
        assert!(!same_grid(&grid, &decoded));
    }
    #[test]
    fn source_crafting_and_special_patterns_work_without_legacy_data() {
        let mut grid=vec![None;9]; grid[0]=Some(simple_stack("minecraft:oak_planks"));
        let output=ItemStack::new("minecraft:oak_button",1);
        let mut pattern=ae2_encode_pattern_source(&grid,&output,"oak_button",true,true).unwrap();
        let source=super::super::ae2_source_patterns::crafting(&pattern).unwrap().unwrap();
        assert_eq!(source.recipe,"minecraft:oak_button");
        let mut bad=pattern.clone();
        bad.data_mut().set_other("ae2:encoded_crafting_pattern",Some(Tag::compound([("recipeId",Tag::String("minecraft:bad".into()))])));
        assert!(decode_pattern(&bad).is_none(),"malformed source must not revive stale legacy data");
        pattern.data_mut().set_other("minecraft:custom_data",None);
        assert_eq!(decode_pattern(&pattern),Some((grid.clone(),output.clone())));
        assert_eq!(ae2_crafting_pattern_options(&pattern),Some((true,true)));
        let snapshot=assembler_pattern_data(&pattern).unwrap();
        assert_eq!(snapshot.get("crafting_recipe"),Some(&Tag::String("minecraft:oak_button".into())));
        let mut special=ae2_encode_special_pattern("stonecutting","minecraft:stone_slab",&grid,&output,false).unwrap();
        special.data_mut().set_other("minecraft:custom_data",None);
        let snapshot=assembler_pattern_data(&special).unwrap();
        assert_eq!(snapshot.get("special_kind"),Some(&Tag::String("stonecutting".into())));
        let actual=horizon_client::save::decode_items(snapshot.get("grid"),9);
        assert_eq!(actual[4],grid[0]);
    }
    #[test]
    fn special_pattern_roundtrips_recipe_identity_and_rejects_malformed_grid() {
        let mut grid=vec![None;9];grid[0]=Some(simple_stack("minecraft:stone"));let output=ItemStack::new("minecraft:stone_slab",2);
        let pattern=ae2_encode_special_pattern("stonecutting","minecraft:stone_slab",&grid,&output,true).unwrap();
        let decoded=ae2_decode_special_pattern(&pattern).unwrap();assert_eq!(decoded.recipe,"minecraft:stone_slab");assert_eq!(decoded.grid,grid);assert_eq!(decoded.output,output);assert!(decoded.substitutes);
        let physical=special_assembler_grid("stonecutting",&grid).unwrap();assert_eq!(physical[4],grid[0]);assert!(physical[0].is_none());assert_eq!(special_logical_grid("stonecutting",&physical),Some(grid.clone()));
        let mut smith=vec![None;9];smith[0]=Some(simple_stack("minecraft:paper"));smith[1]=Some(simple_stack("minecraft:diamond_sword"));smith[2]=Some(simple_stack("minecraft:netherite_ingot"));let physical=special_assembler_grid("smithing",&smith).unwrap();assert_eq!(&physical[3..6],&smith[..3]);assert_eq!(special_logical_grid("smithing",&physical),Some(smith));
        grid[1]=Some(simple_stack("minecraft:dirt"));assert!(ae2_encode_special_pattern("stonecutting","minecraft:stone_slab",&grid,&output,false).is_err());
    }
    #[test]
    fn pattern_requires_real_unit_inputs() {
        let mut grid = vec![None; 9];
        grid[0] = Some(ItemStack {
            count: 2,
            ..simple_stack("minecraft:stone")
        });
        assert!(ae2_encode_pattern(&grid, &simple_stack("minecraft:stick")).is_err());
        assert!(decode_pattern(&simple_stack("ae2:crafting_pattern")).is_none());
    }
    #[test]
    fn assembler_fluid_recipe_view_requires_real_valid_escrow() {
        let mut machine = Machine::new(MachineKind::MolecularAssembler);
        let mut expected = vec![None; 9];
        expected[0] = Some(simple_stack("minecraft:water_bucket"));
        let entry = |amount| {
            Tag::compound([
                ("slot", Tag::Byte(0)),
                ("fluid", Tag::String("minecraft:water".into())),
                ("components", Tag::ByteArray(vec![])),
                ("amount", Tag::Long(amount)),
            ])
        };
        machine.config.insert(
            "ae2_assembler_fluid_escrow".into(),
            Tag::List(vec![entry(1000)]),
        );
        let (view, fluid_slots) = assembler_recipe_grid(&machine, &expected).unwrap();
        assert_eq!(view, expected);
        assert_eq!(fluid_slots, vec![0]);
        assert!(machine.slots[0].is_none()); // recipe view never creates a physical bucket
        machine.config.insert(
            "ae2_assembler_fluid_escrow".into(),
            Tag::List(vec![entry(999)]),
        );
        assert!(assembler_recipe_grid(&machine, &expected).is_none());
        machine.config.insert(
            "ae2_assembler_fluid_escrow".into(),
            Tag::List(vec![entry(1000), entry(1000)]),
        );
        assert!(assembler_recipe_grid(&machine, &expected).is_none());
    }
}

// Translate source GenericStack positions without losing their physical order.
// The host item representation is u32; out-of-range source quantities are
// rejected rather than truncated. Missing Content invalidates processing as in
// AEProcessingPattern while its original component remains on the stack.
struct SourceProcessingView {
    inputs: Vec<Option<ItemStack>>,
    outputs: Vec<Option<ItemStack>>,
    fluid_inputs: Vec<Option<PatternFluid>>,
    fluid_outputs: Vec<Option<PatternFluid>>,
}
fn source_processing_view(pattern: &ItemStack) -> Option<Result<SourceProcessingView, String>> {
    let source = super::ae2_source_patterns::processing(pattern)?;
    Some((|| {
        fn slots(values: Vec<Option<horizon_client::ae2_generic_stack::GenericStack>>, size: usize)
            -> Result<(Vec<Option<ItemStack>>, Vec<Option<PatternFluid>>), String> {
            let mut items = vec![None; size];
            let mut fluids = vec![None; size];
            for (index, value) in values.into_iter().enumerate() {
                let Some(value) = value else { continue; };
                if value.amount <= 0 || value.key.item_id == "ae2:missing_content" {
                    return Err("invalid processing ingredient".into());
                }
                if value.fluid {
                    fluids[index] = Some(PatternFluid { key: value.key, amount: value.amount as u64 });
                } else {
                    let mut stack = horizon_client::ae2_cell_codec::prototype(&value.key)?;
                    stack.count = u32::try_from(value.amount).map_err(|_| "processing item amount exceeds host limit")?;
                    items[index] = Some(stack);
                }
            }
            Ok((items, fluids))
        }
        let (input, output) = source?;
        let (inputs, fluid_inputs) = slots(input, 81)?;
        let (outputs, fluid_outputs) = slots(output, 27)?;
        Ok(SourceProcessingView { inputs, outputs, fluid_inputs, fluid_outputs })
    })())
}
fn source_processing_slots(items: &[Option<ItemStack>], fluids: &[Option<PatternFluid>])
    -> Result<Vec<Option<horizon_client::ae2_generic_stack::GenericStack>>, String> {
    use horizon_client::ae2_generic_stack::GenericStack;
    if items.len() != fluids.len() { return Err("processing slot count mismatch".into()); }
    items.iter().zip(fluids).map(|(item, fluid)| match (item, fluid) {
        (None, None) => Ok(None),
        (Some(item), None) if item.count > 0 => Ok(Some(GenericStack {
            key: horizon_client::ae2_cell_codec::key(item)?, fluid: false, amount: i64::from(item.count),
        })),
        (None, Some(fluid)) if fluid.amount > 0 => Ok(Some(GenericStack {
            key: fluid.key.clone(), fluid: true,
            amount: i64::try_from(fluid.amount).map_err(|_| "processing fluid amount exceeds source limit")?,
        })),
        _ => Err("invalid processing slot".into()),
    }).collect()
}
fn write_source_processing(pattern: &mut ItemStack, items_in: &[Option<ItemStack>], items_out: &[Option<ItemStack>],
    fluids_in: &[Option<PatternFluid>], fluids_out: &[Option<PatternFluid>]) -> Result<(), String> {
    let input = source_processing_slots(items_in, fluids_in)?;
    let output = source_processing_slots(items_out, fluids_out)?;
    let source = super::ae2_source_patterns::encode_processing(&input, &output)?;
    pattern.data_mut().set_other("ae2:encoded_processing_pattern", Some(source));
    Ok(())
}
fn has_unrepresentable_fluid(values: &[Option<PatternFluid>]) -> bool {
    values.iter().flatten().any(|fluid| {
        let generic = horizon_client::ae2_generic_stack::GenericStack {
            key: fluid.key.clone(), fluid: true, amount: i64::try_from(fluid.amount).unwrap_or(i64::MAX),
        };
        !fluid.key.components.is_empty() && horizon_client::ae2_generic_stack::encode(&generic).is_err()
    })
}

/// Experimental item-only processing pattern. Fluid ingredients are not yet
/// accepted because CPU fluid ownership is not break-safe.
pub(super) fn ae2_encode_processing_pattern(
    inputs: &[Option<ItemStack>],
    outputs: &[Option<ItemStack>],
) -> Result<ItemStack, String> {
    if inputs.len() > 81
        || outputs.len() > 27
        || inputs.iter().all(Option::is_none)
        || outputs.first().and_then(Option::as_ref).is_none()
        || inputs.iter().chain(outputs).flatten().any(|s| s.count == 0)
    {
        return Err("processing pattern needs inputs and a primary output".into());
    }
    let mut pattern = simple_stack("ae2:processing_pattern");
    pattern.data_mut().set_other(
        "minecraft:custom_data",
        Some(Tag::compound([(
            "ae2_lab_processing",
            Tag::compound([
                ("inputs", horizon_client::save::encode_items(inputs)),
                ("outputs", horizon_client::save::encode_items(outputs)),
            ]),
        )])),
    );
    write_source_processing(&mut pattern, inputs, outputs, &vec![None; inputs.len()], &vec![None; outputs.len()])?;
    Ok(pattern)
}
fn strict_pattern_slots(tag: &Tag, size: usize) -> Option<Vec<Option<ItemStack>>> {
    let Tag::List(entries) = tag else { return None };
    let mut slots = vec![None; size];
    for entry in entries {
        let Some(Tag::Byte(index)) = entry.get("Slot") else {
            return None;
        };
        let index = *index as u8 as usize;
        if index >= size
            || slots[index].is_some()
            || !matches!(entry.get("count"),Some(Tag::Int(n)) if (1..=99).contains(n))
        {
            return None;
        }
        slots[index] = Some(horizon_client::save::item_from_nbt(entry)?);
    }
    Some(slots)
}
pub(super) fn ae2_decode_processing_pattern(
    pattern: &ItemStack,
) -> Option<(Vec<Option<ItemStack>>, Vec<Option<ItemStack>>)> {
    if pattern.item.as_ref() != "ae2:processing_pattern" || pattern.count != 1 {
        return None;
    }
    if let Some(source) = source_processing_view(pattern) {
        let source = source.ok()?;
        return Some((source.inputs, source.outputs));
    }
    let data = pattern
        .data()?
        .other("minecraft:custom_data")?
        .get("ae2_lab_processing")?;
    let inputs = strict_pattern_slots(data.get("inputs")?, 81)?;
    let outputs = strict_pattern_slots(data.get("outputs")?, 27)?;
    let (fluid_inputs, fluid_outputs) = ae2_decode_processing_fluids(pattern)?;
    if (inputs.iter().all(Option::is_none) && fluid_inputs.is_empty())
        || (outputs.first().and_then(Option::as_ref).is_none() && fluid_outputs.is_empty())
    {
        return None;
    }
    validate_processing_positions(data, &inputs, &outputs, &fluid_inputs, &fluid_outputs).ok()?;
    Some((inputs, outputs))
}
#[cfg(test)]
mod processing_pattern_tests {
    use super::*;
    #[test]
    fn processing_pattern_retains_multiple_outputs_and_components() {
        let mut ore = simple_stack("minecraft:iron_ore");
        ore.count = 3;
        let mut named = simple_stack("minecraft:iron_ingot");
        named.data_mut().custom_name = Some(horizon_client::item_data::text_name("Special"));
        let inputs = vec![Some(ore.clone())];
        let outputs = vec![
            Some(named.clone()),
            Some(simple_stack("minecraft:gold_nugget")),
        ];
        let p = ae2_encode_processing_pattern(&inputs, &outputs).unwrap();
        let (a, b) = ae2_decode_processing_pattern(&p).unwrap();
        assert_eq!(a[0], Some(ore));
        assert_eq!(b[0], Some(named));
        assert_eq!(
            b[1].as_ref().unwrap().item.as_ref(),
            "minecraft:gold_nugget"
        );
        assert!(ae2_encode_processing_pattern(&[], &outputs).is_err());
    }

    #[test]
    fn source_processing_preserves_sparse_items_fluids_and_rejects_bad_source() {
        let mut items_in = vec![None; 81]; items_in[80] = Some(simple_stack("minecraft:iron_ore"));
        let mut items_out = vec![None; 27]; items_out[4] = Some(simple_stack("minecraft:iron_ingot"));
        let mut fluids_in = vec![None; 81]; fluids_in[3] = Some(PatternFluid {
            key: ae2_cell_core::ItemKey { item_id: "minecraft:water".into(), components: vec![] }, amount: 1000,
        });
        let mut fluids_out = vec![None; 27]; fluids_out[0] = Some(PatternFluid {
            key: ae2_cell_core::ItemKey { item_id: "minecraft:lava".into(), components: vec![] }, amount: 250,
        });
        let mut pattern = ae2_encode_processing_positioned(&items_in, &items_out, &fluids_in, &fluids_out).unwrap();
        assert!(pattern.data().unwrap().other("ae2:encoded_processing_pattern").is_some());
        pattern.data_mut().set_other("minecraft:custom_data", None);
        assert_eq!(ae2_decode_processing_pattern(&pattern), Some((items_in, items_out)));
        assert_eq!(ae2_decode_processing_positions(&pattern).unwrap(), Some((fluids_in, fluids_out)));
        pattern.data_mut().set_other("ae2:encoded_processing_pattern", Some(Tag::compound([("sparseInputs", Tag::List(vec![]))])));
        assert!(ae2_decode_processing_pattern(&pattern).is_none());
    }

    #[test]
    fn processing_pattern_preserves_last_paged_slots_and_fluid_identity() {
        let mut inputs = vec![None; 81];
        let mut outputs = vec![None; 27];
        inputs[80] = Some(simple_stack("minecraft:iron_ore"));
        outputs[0] = Some(simple_stack("minecraft:iron_ingot"));
        outputs[26] = Some(simple_stack("minecraft:gold_nugget"));
        let fluid = PatternFluid {
            key: ae2_cell_core::ItemKey {
                item_id: "example:opaque_fluid".into(),
                components: vec![1, 2, 3],
            },
            amount: 10,
        };
        let pattern = ae2_encode_processing_with_fluids(
            &inputs,
            &outputs,
            std::slice::from_ref(&fluid),
            std::slice::from_ref(&fluid),
        )
        .unwrap();
        let (actual_inputs, actual_outputs) = ae2_decode_processing_pattern(&pattern).unwrap();
        let (actual_fluid_inputs, actual_fluid_outputs) =
            ae2_decode_processing_fluids(&pattern).unwrap();
        assert_eq!(actual_inputs[80], inputs[80]);
        assert_eq!(actual_outputs[26], outputs[26]);
        assert_eq!(actual_fluid_inputs, vec![fluid.clone()]);
        assert_eq!(actual_fluid_outputs, vec![fluid]);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PatternFluid {
    pub key: ae2_cell_core::ItemKey,
    pub amount: u64,
}
/// Adds source-owned mB descriptions to an item processing pattern. This
/// records recipe data only; no fluid is created or moved by encoding.
pub(super) fn ae2_encode_processing_with_fluids(
    inputs: &[Option<ItemStack>],
    outputs: &[Option<ItemStack>],
    fluid_inputs: &[PatternFluid],
    fluid_outputs: &[PatternFluid],
) -> Result<ItemStack, String> {
    if inputs.len() > 81
        || outputs.len() > 27
        || (inputs.iter().all(Option::is_none) && fluid_inputs.is_empty())
        || (outputs.first().and_then(Option::as_ref).is_none() && fluid_outputs.is_empty())
        || inputs.iter().chain(outputs).flatten().any(|s| s.count == 0)
    {
        return Err("processing pattern needs inputs and a primary output".into());
    }
    let mut pattern = simple_stack("ae2:processing_pattern");
    pattern.data_mut().set_other(
        "minecraft:custom_data",
        Some(Tag::compound([(
            "ae2_lab_processing",
            Tag::compound([
                ("inputs", horizon_client::save::encode_items(inputs)),
                ("outputs", horizon_client::save::encode_items(outputs)),
            ]),
        )])),
    );
    if fluid_inputs.len() > 81
        || fluid_outputs.len() > 27
        || fluid_inputs
            .iter()
            .chain(fluid_outputs)
            .any(|f| f.amount == 0 || f.amount > i64::MAX as u64 || f.key.item_id.is_empty())
    {
        return Err("invalid processing fluids".into());
    }
    let entries = |fluids: &[PatternFluid]| {
        Tag::List(
            fluids
                .iter()
                .map(|f| {
                    Tag::compound([
                        ("fluid", Tag::String(f.key.item_id.as_str().into())),
                        ("components", Tag::ByteArray(f.key.components.clone())),
                        ("amount", Tag::Long(f.amount as i64)),
                    ])
                })
                .collect(),
        )
    };
    let mut custom = match pattern
        .data()
        .and_then(|d| d.other("minecraft:custom_data"))
    {
        Some(Tag::Compound(c)) => c.clone(),
        _ => return Err("invalid encoded pattern".into()),
    };
    let Some(Tag::Compound(data)) = custom.get_mut(&"ae2_lab_processing".into()) else {
        return Err("invalid encoded pattern".into());
    };
    data.insert("fluid_inputs".into(), entries(fluid_inputs));
    data.insert("fluid_outputs".into(), entries(fluid_outputs));
    pattern
        .data_mut()
        .set_other("minecraft:custom_data", Some(Tag::Compound(custom)));
    Ok(pattern)
}
pub(super) fn ae2_decode_processing_fluids(
    pattern: &ItemStack,
) -> Option<(Vec<PatternFluid>, Vec<PatternFluid>)> {
    if pattern.item.as_ref() != "ae2:processing_pattern" || pattern.count != 1 { return None; }
    if let Some(source) = source_processing_view(pattern) {
        let source = source.ok()?;
        return Some((source.fluid_inputs.into_iter().flatten().collect(), source.fluid_outputs.into_iter().flatten().collect()));
    }
    let data = pattern
        .data()?
        .other("minecraft:custom_data")?
        .get("ae2_lab_processing")?;
    fn read(data: &Tag, name: &str, maximum: usize) -> Option<Vec<PatternFluid>> {
        let Some(tag) = data.get(name) else {
            return Some(Vec::new());
        };
        let Tag::List(entries) = tag else { return None };
        if entries.len() > maximum {
            return None;
        }
        entries
            .iter()
            .map(|e| {
                let Some(Tag::String(id)) = e.get("fluid") else {
                    return None;
                };
                let Some(Tag::ByteArray(bytes)) = e.get("components") else {
                    return None;
                };
                let Some(Tag::Long(amount)) = e.get("amount") else {
                    return None;
                };
                let id = id.to_string().ok()?;
                if *amount <= 0 || id.is_empty() {
                    return None;
                }
                Some(PatternFluid {
                    key: ae2_cell_core::ItemKey {
                        item_id: id,
                        components: bytes.clone(),
                    },
                    amount: *amount as u64,
                })
            })
            .collect()
    }
    Some((
        read(data, "fluid_inputs", 81)?,
        read(data, "fluid_outputs", 27)?,
    ))
}
pub(super) fn ae2_encode_processing_positioned(
    inputs: &[Option<ItemStack>],
    outputs: &[Option<ItemStack>],
    fluid_inputs: &[Option<PatternFluid>],
    fluid_outputs: &[Option<PatternFluid>],
) -> Result<ItemStack, String> {
    if inputs.len() != fluid_inputs.len() || outputs.len() != fluid_outputs.len()
        || inputs.len() > 81 || outputs.len() > 27
        || inputs.iter().zip(fluid_inputs).any(|(item, fluid)| item.is_some() && fluid.is_some())
        || outputs.iter().zip(fluid_outputs).any(|(item, fluid)| item.is_some() && fluid.is_some())
        || (outputs.first().and_then(Option::as_ref).is_none()
            && fluid_outputs.first().and_then(Option::as_ref).is_none()) {
        return Err("invalid positioned processing pattern".into());
    }
    let input_values: Vec<_> = fluid_inputs.iter().flatten().cloned().collect();
    let output_values: Vec<_> = fluid_outputs.iter().flatten().cloned().collect();
    let mut pattern = ae2_encode_processing_with_fluids(inputs, outputs, &input_values, &output_values)?;
    let mut custom = match pattern.data().and_then(|data| data.other("minecraft:custom_data")) {
        Some(Tag::Compound(data)) => data.clone(),
        _ => return Err("invalid encoded processing pattern".into()),
    };
    let Some(Tag::Compound(data)) = custom.get_mut(&"ae2_lab_processing".into()) else {
        return Err("invalid encoded processing pattern".into());
    };
    let positions = |values: &[Option<PatternFluid>]| Tag::IntArray(values.iter().enumerate()
        .filter_map(|(index, value)| value.as_ref().map(|_| index as i32)).collect());
    data.insert("fluid_input_slots".into(), positions(fluid_inputs));
    data.insert("fluid_output_slots".into(), positions(fluid_outputs));
    pattern.data_mut().set_other("minecraft:custom_data", Some(Tag::Compound(custom)));
    ae2_decode_processing_positions(&pattern)?.ok_or("missing processing positions")?;
    if let Err(error) = write_source_processing(&mut pattern, inputs, outputs, fluid_inputs, fluid_outputs) {
        // Older native saves can contain opaque fluid-key bytes that do not
        // represent an NBT DataComponentPatch. Preserve those legacy records.
        if !has_unrepresentable_fluid(fluid_inputs) && !has_unrepresentable_fluid(fluid_outputs) {
            return Err(error);
        }
    }
    Ok(pattern)
}

pub(super) fn ae2_decode_processing_positions(
    pattern: &ItemStack,
) -> Result<Option<(Vec<Option<PatternFluid>>, Vec<Option<PatternFluid>>)>, String> {
    if pattern.item.as_ref() != "ae2:processing_pattern" || pattern.count != 1 { return Err("invalid processing pattern".into()); }
    if let Some(source) = source_processing_view(pattern) {
        let source = source?;
        return Ok(Some((source.fluid_inputs, source.fluid_outputs)));
    }
    let data = pattern.data().and_then(|data| data.other("minecraft:custom_data"))
        .and_then(|tag| tag.get("ae2_lab_processing")).ok_or("invalid processing pattern")?;
    let (items_in, items_out) = ae2_decode_processing_pattern(pattern).ok_or("invalid processing pattern")?;
    let (fluids_in, fluids_out) = ae2_decode_processing_fluids(pattern).ok_or("invalid processing fluids")?;
    validate_processing_positions(data, &items_in, &items_out, &fluids_in, &fluids_out)
}

// The ordinary processing decoder also calls this non-recursive validator, so
// malformed metadata cannot pass Memory Card or Pattern Access decoding.
fn validate_processing_positions(
    data: &Tag,
    items_in: &[Option<ItemStack>],
    items_out: &[Option<ItemStack>],
    fluids_in: &[PatternFluid],
    fluids_out: &[PatternFluid],
) -> Result<Option<(Vec<Option<PatternFluid>>, Vec<Option<PatternFluid>>)>, String> {
    let (input_slots, output_slots) = match (data.get("fluid_input_slots"), data.get("fluid_output_slots")) {
        (None, None) => return Ok(None),
        (Some(Tag::IntArray(input)), Some(Tag::IntArray(output))) => (input, output),
        _ => return Err("malformed processing slot metadata".into()),
    };
    fn place(indices: &[i32], fluids: &[PatternFluid], items: &[Option<ItemStack>]) -> Result<Vec<Option<PatternFluid>>, String> {
        if indices.len() != fluids.len() { return Err("processing slot count mismatch".into()); }
        let mut result = vec![None; items.len()];
        for (index, fluid) in indices.iter().zip(fluids) {
            let index = usize::try_from(*index).map_err(|_| "invalid processing fluid slot")?;
            let item = items.get(index).ok_or("invalid processing fluid slot")?;
            let slot = result.get_mut(index).ok_or("invalid processing fluid slot")?;
            if item.is_some() || slot.is_some() { return Err("overlapping processing fluid slot".into()); }
            *slot = Some(fluid.clone());
        }
        Ok(result)
    }
    let input = place(input_slots, fluids_in, items_in)?;
    let output = place(output_slots, fluids_out, items_out)?;
    if items_out.first().and_then(Option::as_ref).is_none()
        && output.first().and_then(Option::as_ref).is_none() {
        return Err("missing processing primary output".into());
    }
    Ok(Some((input, output)))
}

#[cfg(test)]
mod fluid_pattern_tests {
    use super::*;
    #[test]
    fn fluid_pattern_preserves_exact_key_and_amount() {
        let f = PatternFluid {
            key: ae2_cell_core::ItemKey {
                item_id: "minecraft:water".into(),
                components: vec![1, 2],
            },
            amount: 1000,
        };
        let p = ae2_encode_processing_with_fluids(
            &[Some(simple_stack("minecraft:stone"))],
            &[Some(simple_stack("minecraft:diamond"))],
            &[f.clone()],
            &[],
        )
        .unwrap();
        assert_eq!(ae2_decode_processing_fluids(&p).unwrap().0, vec![f]);
    }
    #[test]
    fn positioned_processing_preserves_sparse_slots_and_rejects_bad_metadata() {
        let fluid = |name: &str| PatternFluid {
            key: ae2_cell_core::ItemKey { item_id: name.into(), components: vec![1, 2] }, amount: 1000,
        };
        let mut inputs = vec![None; 81]; inputs[8] = Some(simple_stack("minecraft:iron_ingot"));
        let mut outputs = vec![None; 27]; outputs[4] = Some(simple_stack("minecraft:diamond"));
        let mut fluid_inputs = vec![None; 81]; fluid_inputs[2] = Some(fluid("minecraft:water"));
        let mut fluid_outputs = vec![None; 27]; fluid_outputs[0] = Some(fluid("minecraft:water")); fluid_outputs[7] = Some(fluid("minecraft:lava"));
        let pattern = ae2_encode_processing_positioned(&inputs, &outputs, &fluid_inputs, &fluid_outputs).unwrap();
        assert_eq!(ae2_decode_processing_positions(&pattern).unwrap(), Some((fluid_inputs.clone(), fluid_outputs.clone())));
        assert_eq!(ae2_decode_processing_pattern(&pattern).unwrap(), (inputs.clone(), outputs.clone()));
        let mut missing_primary = fluid_outputs.clone(); missing_primary[0] = None;
        assert!(ae2_encode_processing_positioned(&inputs, &outputs, &fluid_inputs, &missing_primary).is_err());
        for replacement in [None, Some(vec![0, 0]), Some(vec![4, 7]), Some(vec![1, 7]), Some(vec![-1, 7])] {
            let mut damaged = pattern.clone();
            let mut custom = match damaged.data().unwrap().other("minecraft:custom_data").unwrap() {
                Tag::Compound(values) => values.clone(), _ => panic!("custom data"),
            };
            let Some(Tag::Compound(data)) = custom.get_mut(&"ae2_lab_processing".into()) else { panic!("processing data") };
            match replacement {
                Some(slots) => { data.insert("fluid_output_slots".into(), Tag::IntArray(slots)); }
                None => { data.remove(&"fluid_output_slots".into()); }
            }
            damaged.data_mut().set_other("minecraft:custom_data", Some(Tag::Compound(custom)));
            assert!(ae2_decode_processing_pattern(&damaged).is_none());
            assert!(ae2_decode_processing_positions(&damaged).is_err());
        }
        let legacy = ae2_encode_processing_with_fluids(&inputs, &[Some(simple_stack("minecraft:diamond"))], &[], &[]).unwrap();
        assert_eq!(ae2_decode_processing_positions(&legacy).unwrap(), None);
    }
}
