//! Storage Level Emitter face state shared by its menu, tick and Memory Card.
use ae2_cell_core::ItemKey;
use horizon_client::{items::ItemStack, save};
use horizon_world::nbt::{Compound, Tag};

#[derive(Clone, Debug, PartialEq)]
pub(super) enum EmitterFilter {
    Item(ItemStack),
    Fluid(ItemKey),
}
#[derive(Clone, Debug, PartialEq)]
pub(super) struct EmitterState {
    pub threshold: i64,
    pub low: bool,
    pub filter: Option<EmitterFilter>,
    pub fuzzy_mode: String,
    pub craft_via_redstone: bool,
    pub upgrades: Vec<Option<ItemStack>>,
}
const FUZZY: &[&str] = &["ignore_all", "percent_99", "percent_75", "percent_50", "percent_25"];
fn bool_field(part: &Compound, key: &str) -> Result<bool, String> {
    match part.get(&key.into()) {
        None | Some(Tag::Byte(0)) => Ok(false),
        Some(Tag::Byte(1)) => Ok(true),
        _ => Err(format!("invalid emitter {key}")),
    }
}
fn validate(state: &EmitterState) -> Result<(), String> {
    if state.upgrades.len()!=1 || state.upgrades.iter().flatten().any(|card| card.count!=1 || !matches!(card.item.as_ref(),"ae2:fuzzy_card"|"ae2:crafting_card")) {
        return Err("invalid emitter upgrades".into());
    }
    if !FUZZY.contains(&state.fuzzy_mode.as_str()) { return Err("invalid emitter fuzzy mode".into()); }
    match state.filter.as_ref() {
        Some(EmitterFilter::Item(item)) if item.count!=1 => return Err("invalid emitter item filter".into()),
        Some(EmitterFilter::Fluid(key)) if !super::ae2_fluids::valid_fluid_id(&key.item_id) => return Err("invalid emitter fluid filter".into()),
        _ => {}
    }
    Ok(())
}
pub(super) fn emitter_read(part: &Compound) -> Result<EmitterState, String> {
    if part.get(&"id".into()) != Some(&Tag::String("ae2:level_emitter".into())) { return Err("not a storage level emitter".into()); }
    let threshold=match part.get(&"emitter_threshold".into()) {None=>0,Some(Tag::Long(v))=>*v,_=>return Err("invalid emitter threshold".into())};
    let fuzzy_mode=match part.get(&"fuzzy_mode".into()) {None=>"ignore_all".to_owned(),Some(Tag::String(v))=>v.to_string().map_err(|_|"invalid emitter fuzzy text")?,_=>return Err("invalid emitter fuzzy mode".into())};
    let upgrades=match part.get(&"upgrades".into()) {None=>vec![None],Some(Tag::List(v))=>horizon_client::ae2_upgrade_inventory::decode(v,1)?,_=>return Err("invalid emitter upgrades".into())};
    let filter=match (part.get(&"emitter_filter".into()),part.get(&"emitter_fluid_filter".into())) {
        (None,None)=>None,
        (Some(item),None)=>Some(EmitterFilter::Item(save::item_from_nbt(item).ok_or("invalid emitter item filter")?)),
        (None,Some(Tag::Compound(fluid)))=>{
            let (Some(Tag::String(id)),Some(Tag::ByteArray(components)))=(fluid.get(&"fluid".into()),fluid.get(&"components".into())) else{return Err("invalid emitter fluid filter".into())};
            Some(EmitterFilter::Fluid(ItemKey{item_id:id.to_string().map_err(|_|"invalid emitter fluid id")?,components:components.clone()}))
        },
        _=>return Err("conflicting or invalid emitter filters".into()),
    };
    let state=EmitterState{threshold,low:bool_field(part,"emitter_low")?,filter,fuzzy_mode,craft_via_redstone:bool_field(part,"emitter_craft_via_redstone")?,upgrades};
    validate(&state)?;Ok(state)
}
pub(super) fn emitter_store(part: &mut Compound, state: &EmitterState) -> Result<(), String> {
    if part.get(&"id".into()) != Some(&Tag::String("ae2:level_emitter".into())) { return Err("not a storage level emitter".into()); }
    validate(state)?;
    let upgrades=horizon_client::ae2_upgrade_inventory::encode(&state.upgrades)?;
    part.insert("emitter_threshold".into(),Tag::Long(state.threshold));
    part.insert("emitter_low".into(),Tag::Byte(i8::from(state.low)));
    part.insert("fuzzy_mode".into(),Tag::String(state.fuzzy_mode.as_str().into()));
    part.insert("emitter_craft_via_redstone".into(),Tag::Byte(i8::from(state.craft_via_redstone)));
    part.insert("upgrades".into(),Tag::List(upgrades));
    part.remove(&"emitter_filter".into());part.remove(&"emitter_fluid_filter".into());
    match state.filter.as_ref() {
        Some(EmitterFilter::Item(item))=>{part.insert("emitter_filter".into(),save::item_to_nbt(item));},
        Some(EmitterFilter::Fluid(key))=>{part.insert("emitter_fluid_filter".into(),Tag::compound([("fluid",Tag::String(key.item_id.as_str().into())),("components",Tag::ByteArray(key.components.clone()))]));},
        None=>{},
    }
    Ok(())
}


/// Source level emitters compare against a virtual AE key. A Fuzzy Card uses
/// AEKey fuzzy matching; item durability partitions reuse the bus implementation.
pub(super) fn emitter_matches_item(state:&EmitterState,key:&ItemKey,max_damage:u32)->bool {
    match state.filter.as_ref() {
        None=>true,
        Some(EmitterFilter::Item(stack))=>{
            let Ok(wanted)=super::ae2_lab::key(stack) else{return false};
            if state.upgrades.iter().flatten().any(|s|s.item.as_ref()=="ae2:fuzzy_card") {
                super::ae2_buses::fuzzy_key_match(&ae2_machines_core::Key::from(&wanted),&ae2_machines_core::Key::from(key),&state.fuzzy_mode,max_damage)
            }else{wanted==*key}
        },
        Some(EmitterFilter::Fluid(_))=>false,
    }
}
pub(super) fn emitter_matches_fluid(state:&EmitterState,key:&ItemKey)->bool {
    match state.filter.as_ref() {
        None=>true,
        Some(EmitterFilter::Fluid(wanted))=>if state.upgrades.iter().flatten().any(|s|s.item.as_ref()=="ae2:fuzzy_card"){wanted.item_id==key.item_id}else{wanted==key},
        Some(EmitterFilter::Item(_))=>false,
    }
}
/// AbstractLevelEmitterPart compares reportingValue with lastReportedValue + 1.
pub(super) fn emitter_threshold_signal(state:&EmitterState,count:i64)->bool {
    let boundary=count.wrapping_add(1);
    if state.low {state.threshold>=boundary} else {state.threshold<boundary}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn emitter_matches_exact_and_fuzzy_item_and_fluid_keys() {
        let mut part=match Tag::compound([("id",Tag::String("ae2:level_emitter".into()))]){Tag::Compound(v)=>v,_=>unreachable!()};
        let mut state=emitter_read(&part).unwrap();let mut marked=ItemStack::new("minecraft:diamond",1);marked.data_mut().custom_name=Some("marked".into());
        let plain=super::super::ae2_lab::key(&ItemStack::new("minecraft:diamond",1)).unwrap();state.filter=Some(EmitterFilter::Item(marked));assert!(!emitter_matches_item(&state,&plain,0));
        state.upgrades[0]=Some(ItemStack::new("ae2:fuzzy_card",1));assert!(emitter_matches_item(&state,&plain,0));
        let water=ItemKey{item_id:"minecraft:water".into(),components:vec![1]};let other_water=ItemKey{item_id:"minecraft:water".into(),components:vec![2]};state.filter=Some(EmitterFilter::Fluid(water));
        assert!(emitter_matches_fluid(&state,&other_water));state.upgrades[0]=None;assert!(!emitter_matches_fluid(&state,&other_water));
        emitter_store(&mut part,&state).unwrap();assert_eq!(emitter_read(&part).unwrap(),state);
    }
    #[test]
    fn emitter_threshold_follows_signed_source_comparison() {
        let part=match Tag::compound([("id",Tag::String("ae2:level_emitter".into()))]){Tag::Compound(v)=>v,_=>unreachable!()};let mut state=emitter_read(&part).unwrap();
        assert!(emitter_threshold_signal(&state,0));state.low=true;assert!(!emitter_threshold_signal(&state,0));
        state.threshold=i64::MAX;assert!(emitter_threshold_signal(&state,i64::MAX));state.low=false;assert!(!emitter_threshold_signal(&state,i64::MAX));
    }
    #[test]
    fn emitter_state_preserves_signed_threshold_and_one_physical_card_and_fluid_key() {
        let mut part=match Tag::compound([("id",Tag::String("ae2:level_emitter".into())),("other_face_state",Tag::Int(19))]){Tag::Compound(v)=>v,_=>unreachable!()};
        let mut s=emitter_read(&part).unwrap();assert_eq!(s.threshold,0);assert_eq!(s.upgrades,vec![None]);
        s.threshold=-4;s.low=true;s.fuzzy_mode="percent_75".into();s.filter=Some(EmitterFilter::Fluid(ItemKey{item_id:"minecraft:water".into(),components:vec![1,2,3]}));s.upgrades[0]=Some(ItemStack::new("ae2:fuzzy_card",1));
        emitter_store(&mut part,&s).unwrap();assert_eq!(emitter_read(&part).unwrap(),s);assert_eq!(part.get(&"other_face_state".into()),Some(&Tag::Int(19)));
        let before=part.clone();s.upgrades[0]=Some(ItemStack::new("ae2:speed_card",1));assert!(emitter_store(&mut part,&s).is_err());assert_eq!(part,before);
    }
}
// Staged for inclusion in game/ae2_emitter.rs after the Stage 68 build.
// Runtime state is intentionally separate from the persisted face settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct EnergyEmitterState {
    pub threshold: i64,
    pub low: bool,
}

pub(super) fn energy_emitter_read(part: &Compound) -> Result<EnergyEmitterState, String> {
    if part.get(&"id".into()) != Some(&Tag::String("ae2:energy_level_emitter".into())) {
        return Err("not an energy level emitter".into());
    }
    let threshold = match part.get(&"emitter_threshold".into()) {
        None => 0,
        Some(Tag::Long(value)) => *value,
        _ => return Err("invalid energy emitter threshold".into()),
    };
    Ok(EnergyEmitterState { threshold, low: bool_field(part, "emitter_low")? })
}

pub(super) fn energy_emitter_store(part: &mut Compound, state: &EnergyEmitterState) -> Result<(), String> {
    if part.get(&"id".into()) != Some(&Tag::String("ae2:energy_level_emitter".into())) {
        return Err("not an energy level emitter".into());
    }
    part.insert("emitter_threshold".into(), Tag::Long(state.threshold));
    part.insert("emitter_low".into(), Tag::Byte(i8::from(state.low)));
    Ok(())
}

/// One source EnergyWatcher, scoped to the real part and the service that
/// owns it. `reported`/`output` represent the source's persisted
/// lastReportedValue/prevState; the watch itself is reconstructed on attach.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct EnergyEmitterWatch {
    pub watched: Option<f64>,
    pub previous_power: f64,
    pub reported: i64,
    pub output: bool,
    pub online: bool,
}
impl EnergyEmitterWatch {
    pub fn loaded(reported: i64, output: bool, online: bool, power: f64) -> Self {
        Self { watched: None, previous_power: power, reported, output, online }
    }
    fn update_state(&mut self, state: &EnergyEmitterState) {
        let boundary = self.reported.wrapping_add(1);
        self.output = self.online && if state.low { state.threshold >= boundary } else { state.threshold < boundary };
    }
    /// EnergyLevelEmitterPart.configureWatchers resets then registers the
    /// current threshold, and only afterward refreshes the reported amount.
    /// Its source EnergyWatcher rejects a negative registration; on failure
    /// the previous source output and reported amount remain in place.
    pub fn configure(&mut self, state: &EnergyEmitterState, power: f64) -> Result<(), &'static str> {
        self.watched = None;
        if state.threshold < 0 { return Err("negative energy watch threshold"); }
        self.watched = Some(state.threshold as f64);
        self.previous_power = power;
        self.reported = power as i64;
        self.update_state(state);
        Ok(())
    }
    /// AbstractLevelEmitterPart.setReportingValue changes output but does not
    /// re-register EnergyLevelEmitterPart's existing threshold watcher.
    pub fn threshold_set(&mut self, state: &EnergyEmitterState) { self.update_state(state); }
    pub fn set_online(&mut self, state: &EnergyEmitterState, online: bool) {
        if self.online != online { self.online = online; self.update_state(state); }
    }
    /// Source EnergyService checks the inclusive old/new interval on its
    /// end-tick callback, including an unchanged value exactly at the watch.
    pub fn observe_power(&mut self, state: &EnergyEmitterState, power: f64) {
        let lo = self.previous_power.min(power);
        let hi = self.previous_power.max(power);
        self.previous_power = power;
        if self.watched.is_some_and(|w| lo <= w && w <= hi) {
            self.reported = power as i64;
            self.update_state(state);
        }
    }
}

#[cfg(test)]
mod energy_emitter_state_tests {
    use super::*;
    fn state(threshold: i64, low: bool) -> EnergyEmitterState { EnergyEmitterState { threshold, low } }
    #[test]
    fn threshold_setter_does_not_replace_the_existing_watch() {
        let mut w = EnergyEmitterWatch::loaded(0, false, true, 100.0);
        w.configure(&state(0,false), 100.0).unwrap();
        assert!(w.output);
        w.threshold_set(&state(150,false));
        assert_eq!(w.watched, Some(0.0));
        assert!(!w.output);
        w.observe_power(&state(150,false), 50.0);
        assert_eq!(w.reported,100); // 0 was not crossed
        w.configure(&state(150,true),50.0).unwrap();
        assert_eq!(w.watched,Some(150.0));
        assert_eq!(w.reported,50);
        assert!(w.output);
    }
    #[test]
    fn negative_registration_clears_the_watch_without_changing_output() {
        let mut w=EnergyEmitterWatch::loaded(10,true,true,10.0);
        w.configure(&state(5,false),10.0).unwrap();
        assert!(w.output);
        w.threshold_set(&state(-1,false));
        assert!(w.output);
        assert!(w.configure(&state(-1,true),10.0).is_err());
        assert_eq!(w.watched,None);
        assert!(w.output);
        assert_eq!(w.reported,10);
    }
    #[test]
    fn inclusive_crossing_and_signed_overflow_match_source_comparison() {
        let mut w=EnergyEmitterWatch::loaded(0,false,true,12.0);
        w.configure(&state(10,false),12.0).unwrap();
        w.observe_power(&state(10,false),10.0);
        assert_eq!(w.reported,10);
        assert!(w.output);
        w.reported=i64::MAX;
        w.threshold_set(&state(0,false));
        assert!(!w.output);
    }
}
