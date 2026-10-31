//! Single-recipe experimental CPU execution. Inventory is always physical.
use super::{Game, ae2_lab, ae2_processing};
use ae2_crafting_core::{Inventory, Job, Pattern, Registry};
use horizon_client::{
    ae2_world::{Machine, MachineKind},
    containers::BlockEntity,
    item_data,
    items::ItemStack,
};
use horizon_world::nbt::{Compound, Tag};
use serde_json::{Value, json};
/// Source PatternProviderBlock.PUSH_DIRECTION; absent means the legacy ALL state.
pub(super) fn provider_push_direction(state:&horizon_world::chunk::BlockState)->Result<Option<horizon_core::geometry::Direction>,()> {
    use horizon_core::geometry::Direction::*;
    Ok(match state.properties.get("push_direction").map(String::as_str).unwrap_or("all") {
        "all"=>None,"down"=>Some(Down),"up"=>Some(Up),"north"=>Some(North),"south"=>Some(South),"west"=>Some(West),"east"=>Some(East),_=>return Err(())
    })
}
type Pos = (i32, i32, i32);
const CARD_GENERATION: &str = "ae2_crafting_card_generation";
/// A pending future belongs to the installed crafting card instance. The
/// generation also distinguishes removal followed by reinsertion between ticks.
pub(super) fn crafting_card_generation(data: &Compound) -> Option<i64> {
    match data.get(&CARD_GENERATION.into()) {
        None => Some(0),
        Some(Tag::Long(value)) if *value >= 0 => Some(*value),
        _ => None,
    }
}
pub(super) fn advance_crafting_card_generation(data: &mut Compound) {
    let next = crafting_card_generation(data).and_then(|value| value.checked_add(1));
    let value = next.map_or_else(|| Tag::String("invalid".into()), Tag::Long);
    data.insert(CARD_GENERATION.into(), value);
}
fn adjacent(a: Pos, b: Pos) -> bool {
    (i64::from(a.0) - i64::from(b.0)).abs()
        + (i64::from(a.1) - i64::from(b.1)).abs()
        + (i64::from(a.2) - i64::from(b.2)).abs()
        == 1
}
fn coords(w: &[&str], i: usize) -> Result<Pos, String> {
    Ok((
        w.get(i).ok_or("missing x")?.parse().map_err(|_| "bad x")?,
        w.get(i + 1)
            .ok_or("missing y")?
            .parse()
            .map_err(|_| "bad y")?,
        w.get(i + 2)
            .ok_or("missing z")?
            .parse()
            .map_err(|_| "bad z")?,
    ))
}
fn canon(s: &ItemStack) -> String {
    let mut s = s.clone();
    s.count = 1;
    item_data::canon_stack(Some(&s))
}
// A storage bus can mutate ordinary containers as well as AE2 cells. A
// synchronous transaction therefore snapshots every loaded block entity.
fn snapshot(g: &Game<'_>) -> super::ae2_global::BlockSnapshot {
    g.ae2_snapshot_blocks()
}
fn restore(g: &mut Game<'_>, saved: super::ae2_global::BlockSnapshot) {
    g.ae2_restore_blocks(saved);
}
fn state(m: &Machine) -> Result<(Job, Pos), String> {
    let s = match m.config.get(&"ae2_cpu_job".into()) {
        Some(Tag::String(s)) => s.to_string_lossy(),
        _ => return Err("CPU has no job".into()),
    };
    let json: Value = serde_json::from_str(&s).map_err(|_| "invalid CPU job")?;
    let job: Job = serde_json::from_value(json.get("job").cloned().ok_or("missing job")?)
        .map_err(|_| "invalid job")?;
    let a = json
        .get("assembler")
        .and_then(Value::as_array)
        .ok_or("missing assembler")?;
    if a.len() != 3 {
        return Err("bad assembler".into());
    }
    let n = |i: usize| {
        a[i].as_i64()
            .and_then(|v| i32::try_from(v).ok())
            .ok_or("bad assembler".to_owned())
    };
    Ok((job, (n(0)?, n(1)?, n(2)?)))
}
fn put_state(m: &mut Machine, job: &Job, assembler: Pos) -> Result<(), String> {
    let s = serde_json::to_string(
        &json!({"job":job,"assembler":[assembler.0,assembler.1,assembler.2]}),
    )
    .map_err(|e| e.to_string())?;
    m.config
        .insert("ae2_cpu_job".into(), Tag::String(s.as_str().into()));
    Ok(())
}
/// Keep earlier single-recipe jobs busy and let their own physical assembler
/// complete normally; do not migrate or reconstruct their owned inventory.
fn legacy_waiting(m:&Machine) -> Result<Option<Inventory>,String> {
    if !m.config.contains_key(&"ae2_cpu_job".into()) {return Ok(None);}
    if m.config.contains_key(&"ae2_recursive_job".into()) {return Err("CPU has both legacy and recursive jobs".into());}
    let (job,_)=state(m)?;
    let mut waiting=Inventory::new();
    if let Some(step)=job.waiting() {
        if step.output.is_empty() || step.count==0 {return Err("invalid legacy CPU waiting output".into());}
        let output=item_data::parse_stack(&step.output)?.ok_or("invalid legacy CPU waiting output")?;
        waiting.insert(canon(&output),step.count);
    }
    Ok(Some(waiting))
}
impl Game<'_> {
    pub(super) fn ae2_crafting_command(&mut self, w: &[&str]) -> Result<Value, String> {
        match w.first().copied().unwrap_or("") {
            "encode" => {
                if w.len() != 10 {
                    return Err("usage: /ae2 crafting encode <nine item IDs or ->".into());
                }
                let blank = self.inventory[self.selected]
                    .as_ref()
                    .ok_or("hold one blank pattern")?;
                if blank.item.as_ref() != "ae2:blank_pattern" || blank.count != 1 {
                    return Err("hold exactly one blank pattern in your main hand".into());
                }
                let mut grid = Vec::with_capacity(9);
                for id in &w[1..] {
                    if *id == "-" {
                        grid.push(None)
                    } else {
                        if self.res.items().get(id).is_none() {
                            return Err(format!("unknown item {id}"));
                        }
                        grid.push(Some(ItemStack::new(id, 1)));
                    }
                }
                let (recipe, out) = self
                    .res
                    .recipes()
                    .find(&grid, 3)
                    .ok_or("no matching crafting recipe")?;
                if self.res.items().get("ae2:crafting_pattern").is_none() {
                    return Err("crafting pattern item not registered".into());
                }
                let pattern = ae2_processing::ae2_encode_pattern_source(&grid, &out, recipe, false, false)?;
                self.inventory[self.selected] = Some(pattern);
                Ok(json!({"encoded":true,"output":out.item.to_string(),"count":out.count}))
            }
            "request" => {
                if w.len() != 11 {
                    return Err("usage: /ae2 crafting request <cpu xyz> <provider xyz> <assembler xyz> <pattern slot>".into());
                }
                let cpu = coords(w, 1)?;
                let provider = coords(w, 4)?;
                let assembler = coords(w, 7)?;
                let slot: usize = w[10].parse().map_err(|_| "bad pattern slot")?;
                if !adjacent(provider, assembler) {
                    return Err("assembler must touch provider".into());
                }
                let n = self.ae2_network();
                let a = n.analyze().map_err(str::to_owned)?;
                let idx = |p: Pos| {
                    a.nodes
                        .get(&self.ae2_local_address(p)?.network_pos())
                        .filter(|s| s.online)
                        .map(|s| s.component)
                };
                let component = idx(cpu).ok_or("CPU offline")?;
                if idx(provider) != Some(component) || idx(assembler) != Some(component) {
                    return Err("machines must be online on the same network".into());
                }
                let Some(BlockEntity::Ae2(c)) = self.world.block_entities.get(&cpu) else {
                    return Err("missing CPU".into());
                };
                if !matches!(
                    c.kind,
                    MachineKind::CraftingStorage1k
                        | MachineKind::CraftingStorage4k
                        | MachineKind::CraftingStorage16k
                        | MachineKind::CraftingStorage64k
                        | MachineKind::CraftingStorage256k
                ) {
                    return Err("not a CPU block".into());
                }
                if c.config.contains_key(&"ae2_cpu_job".into()) || c.config.contains_key(&"ae2_recursive_job".into()) {
                    return Err("CPU is busy".into());
                }
                let Some(BlockEntity::Ae2(p)) = self.world.block_entities.get(&provider) else {
                    return Err("missing provider".into());
                };
                if p.kind != MachineKind::PatternProvider {
                    return Err("not a provider".into());
                }
                let pattern = p
                    .slots
                    .get(slot)
                    .and_then(Option::as_ref)
                    .ok_or("missing pattern")?;
                let (grid, encoded_output) =
                    ae2_processing::decode_pattern(pattern).ok_or("invalid laboratory pattern")?;
                let source_recipe = super::ae2_source_patterns::crafting(pattern).and_then(Result::ok).map(|source| source.recipe);
                let actual = match source_recipe.as_deref() {
                    Some(recipe) => self.res.recipes().assemble_named(recipe, &grid, 3),
                    None => self.res.recipes().find(&grid, 3).map(|(_, result)| result),
                }.ok_or("recipe missing")?;
                if source_recipe.is_none() && encoded_output != actual { return Err("pattern result changed".into()); }
                let expected=actual;
                let pattern_data = ae2_processing::assembler_pattern_data(pattern).ok_or("invalid pattern")?;
                let Some(BlockEntity::Ae2(m)) = self.world.block_entities.get(&assembler) else {
                    return Err("missing assembler".into());
                };
                if m.kind != MachineKind::MolecularAssembler
                    || m.slots.len() != 11
                    || m.slots.iter().any(Option::is_some)
                    || m.config.contains_key(&"ae2_assembler_pattern".into())
                {
                    return Err("assembler busy".into());
                }
                let mut have = Inventory::new();
                for (k, n) in self.ae2_grid_inventory(cpu)? {
                    let p = ae2_lab::prototype(&k)?;
                    have.insert(canon(&p), n);
                }
                let mut inputs = Inventory::new();
                for p in grid.iter().flatten() {
                    *inputs.entry(canon(p)).or_default() += 1;
                }
                let mut reg = Registry::default();
                reg.register(Pattern {
                    id: "lab:pattern".into(),
                    output: canon(&expected),
                    output_count: u64::from(expected.count),
                    inputs,
                })
                .map_err(|e| format!("{e:?}"))?;
                let plan = reg
                    .plan(&have, &canon(&expected), u64::from(expected.count))
                    .map_err(|e| format!("{e:?}"))?;
                if plan.steps.len() != 1 {
                    return Err(
                        "this CPU supports one recipe; output already exists or multi-step plan"
                            .into(),
                    );
                }
                let mut job = Job::submit(plan, &mut have).map_err(|e| format!("{e:?}"))?;
                job.dispatch().map_err(|e| format!("{e:?}"))?;
                let keys = grid
                    .iter()
                    .map(|item| item.as_ref().map(ae2_lab::key).transpose())
                    .collect::<Result<Vec<_>, _>>()?;
                let backup = snapshot(self);
                let mut formation = super::ae2_buses::FormationPlan::default();
                let mut moved = Vec::with_capacity(9);
                for key in &keys {
                    if let Some(key) = key {
                        match self.ae2_grid_extract_staged(cpu, key, 1, &mut formation) {
                            Ok(Some(stack)) if stack.count == 1 => moved.push(Some(stack)),
                            _ => {
                                restore(self, backup);
                                return Err(
                                    "unable to extract all inputs or power insufficient".into()
                                );
                            }
                        }
                    } else {
                        moved.push(None);
                    }
                }
                if let Some(BlockEntity::Ae2(m)) = self.world.block_entities.get_mut(&assembler) {
                    m.slots[..9].clone_from_slice(&moved);
                    m.progress = 0.0;
                    m.config
                        .insert("ae2_assembler_pattern".into(), pattern_data);
                } else {
                    restore(self, backup);
                    return Err("assembler disappeared".into());
                }
                if let Some(BlockEntity::Ae2(m)) = self.world.block_entities.get_mut(&cpu) {
                    if let Err(e) = put_state(m, &job, assembler) {
                        restore(self, backup);
                        return Err(e);
                    }
                } else {
                    restore(self, backup);
                    return Err("CPU disappeared".into());
                }
                if let Err(e) = self.ae2_commit_formation(formation) {
                    restore(self, backup);
                    return Err(e);
                }
                self.world.modified.insert((cpu.0 >> 4, cpu.2 >> 4));
                self.world
                    .modified
                    .insert((assembler.0 >> 4, assembler.2 >> 4));
                Ok(
                    json!({"started":true,"output":expected.item.to_string(),"count":expected.count}),
                )
            }
            "status" => {
                let cpu = coords(w, 1)?;
                let Some(BlockEntity::Ae2(m)) = self.world.block_entities.get(&cpu) else {
                    return Err("no CPU".into());
                };
                let (j, a) = state(m)?;
                Ok(json!({"waiting":j.waiting().is_some(),"assembler":[a.0,a.1,a.2]}))
            }
            "cancel" => {
                let cpu = coords(w, 1)?;
                self.ae2_cancel_job(cpu)?;
                Ok(json!({"cancelled":true}))
            }
            _ => Err("usage: /ae2 crafting encode|request|status|cancel".into()),
        }
    }
    /// Called after machine processing; returns actual completed outputs to storage.
    pub(super) fn tick_ae2_autocrafting(&mut self) {
        let cpus: Vec<_> = self
            .world
            .block_entities
            .iter()
            .filter_map(|(p, e)| match e {
                BlockEntity::Ae2(m) if m.config.contains_key(&"ae2_cpu_job".into()) => Some(*p),
                _ => None,
            })
            .collect();
        for cpu in cpus {
            let Some(BlockEntity::Ae2(c)) = self.world.block_entities.get(&cpu) else {
                continue;
            };
            let Ok((mut job, assembler)) = state(c) else {
                continue;
            };
            if !self.ae2_online(cpu) || !self.ae2_online(assembler) {
                continue;
            }
            let Some(BlockEntity::Ae2(m)) = self.world.block_entities.get(&assembler) else {
                continue;
            };
            if m.kind != MachineKind::MolecularAssembler || m.slots.len() != 11 {
                continue;
            }
            let Some(out) = m.slots[9].clone() else {
                continue;
            };
            let Some(step) = job.waiting() else { continue };
            if canon(&out) != step.output || u64::from(out.count) != step.count {
                continue;
            }
            let remainders: Vec<_> = m.slots[..9].iter().flatten().cloned().collect();
            let backup = snapshot(self);
            let mut formation = super::ae2_buses::FormationPlan::default();
            let mut all = vec![out.clone()];
            all.extend(remainders);
            if all.iter().any(|s| {
                self.ae2_grid_insert_staged(cpu, s, u64::from(s.count), &mut formation)
                    .ok()
                    != Some(u64::from(s.count))
            }) {
                restore(self, backup);
                continue;
            }
            if job
                .complete_step(&canon(&out), u64::from(out.count))
                .is_err()
            {
                restore(self, backup);
                continue;
            }
            let mut virtual_result = Inventory::new();
            if job.finish(&mut virtual_result).is_err() {
                restore(self, backup);
                continue;
            }
            if let Some(BlockEntity::Ae2(m)) = self.world.block_entities.get_mut(&assembler) {
                for slot in &mut m.slots[..10] {
                    *slot = None;
                }
            } else {
                restore(self, backup);
                continue;
            }
            if let Some(BlockEntity::Ae2(m)) = self.world.block_entities.get_mut(&cpu) {
                m.config.remove(&"ae2_cpu_job".into());
            } else {
                restore(self, backup);
                continue;
            }
            if self.ae2_commit_formation(formation).is_err() {
                restore(self, backup);
                continue;
            }
            self.world.modified.insert((cpu.0 >> 4, cpu.2 >> 4));
            self.world
                .modified
                .insert((assembler.0 >> 4, assembler.2 >> 4));
        }
    }
    fn ae2_cancel_job(&mut self, cpu: Pos) -> Result<(), String> {
        let Some(BlockEntity::Ae2(m)) = self.world.block_entities.get(&cpu) else {
            return Err("no CPU".into());
        };
        let (_, assembler) = state(m)?;
        if !self.ae2_online(cpu) || !self.ae2_online(assembler) {
            return Err("network offline; items remain in assembler".into());
        }
        let Some(BlockEntity::Ae2(m)) = self.world.block_entities.get(&assembler) else {
            return Err("assembler missing; reconcile job manually".into());
        };
        if m.kind != MachineKind::MolecularAssembler || m.slots.len() != 11 {
            return Err("assembler missing; reconcile job manually".into());
        }
        let stacks: Vec<_> = m.slots[..10].iter().flatten().cloned().collect();
        let backup = snapshot(self);
        let mut formation = super::ae2_buses::FormationPlan::default();
        for s in &stacks {
            if self
                .ae2_grid_insert_staged(cpu, s, u64::from(s.count), &mut formation)
                .ok()
                != Some(u64::from(s.count))
            {
                restore(self, backup);
                return Err("cannot return all items to network; job retained".into());
            }
        }
        if let Some(BlockEntity::Ae2(m)) = self.world.block_entities.get_mut(&assembler) {
            for slot in &mut m.slots[..10] {
                *slot = None;
            }
            m.config.remove(&"ae2_assembler_pattern".into());
            m.progress = 0.0;
        } else {
            restore(self, backup);
            return Err("assembler missing".into());
        }
        if let Some(BlockEntity::Ae2(m)) = self.world.block_entities.get_mut(&cpu) {
            m.config.remove(&"ae2_cpu_job".into());
        } else {
            restore(self, backup);
            return Err("CPU missing".into());
        }
        if let Err(e) = self.ae2_commit_formation(formation) {
            restore(self, backup);
            return Err(e);
        }
        self.world.modified.insert((cpu.0 >> 4, cpu.2 >> 4));
        self.world
            .modified
            .insert((assembler.0 >> 4, assembler.2 >> 4));
        Ok(())
    }
}
fn pattern_data(grid: &[Option<ItemStack>], out: &ItemStack) -> Tag {
    Tag::compound([
        ("grid", horizon_client::save::encode_items(grid)),
        ("output", horizon_client::save::item_to_nbt(out)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn card_generation_changes_without_a_link_and_fails_closed_on_overflow() {
        let mut config = Compound::new();
        assert_eq!(crafting_card_generation(&config), Some(0));
        advance_crafting_card_generation(&mut config);
        assert_eq!(crafting_card_generation(&config), Some(1));
        advance_crafting_card_generation(&mut config);
        assert_eq!(crafting_card_generation(&config), Some(2));
        config.insert(CARD_GENERATION.into(), Tag::Long(i64::MAX));
        advance_crafting_card_generation(&mut config);
        assert_eq!(crafting_card_generation(&config), None);
    }
    #[test]
    fn cpu_waiting_job_round_trips_in_world_config() {
        let mut reg = Registry::default();
        let output=canon(&ItemStack::new("minecraft:gold_ingot",1));
        reg.register(Pattern {
            id: "p".into(),
            output: output.clone(),
            output_count: 1,
            inputs: Inventory::from([("in".into(), 2)]),
        })
        .unwrap();
        let mut have = Inventory::from([("in".into(), 2)]);
        let plan = reg.plan(&have, &output, 1).unwrap();
        let mut job = Job::submit(plan, &mut have).unwrap();
        let mut m = Machine::new(MachineKind::CraftingStorage1k);
        put_state(&mut m, &job, (1, 64, -3)).unwrap();
        assert_eq!(legacy_waiting(&m).unwrap(),Some(Inventory::new()));
        job.dispatch().unwrap();
        put_state(&mut m, &job, (1, 64, -3)).unwrap();
        let original=m.clone();
        assert_eq!(legacy_waiting(&m).unwrap(),Some(Inventory::from([(output,1)])));
        assert_eq!(m,original); // No job or physical inventory is migrated.
        let (restored, pos) = state(&m).unwrap();
        assert_eq!(pos, (1, 64, -3));
        assert_eq!(restored, job);
        assert!(restored.waiting().is_some());
        m.config.insert("ae2_recursive_job".into(),Tag::String("other".into()));
        assert!(legacy_waiting(&m).is_err());
    }
}

// Recursive execution uses physical item stacks in `ae2_cpu_escrow`; the CPU
// planner's counters are only metadata and are never emitted as drops.
pub(super) use recursive::PendingMachineCraft;
mod recursive {
    use super::*;
    use ae2_crafting_core::extended::{
        Cpu, Ingredient, Pattern as ExtendedPattern, Registry as ExtendedRegistry, SourceInput,
    };
    use horizon_core::random::RandomSource;
    use serde::{Deserialize, Serialize};
    use std::collections::{BTreeMap, BTreeSet, VecDeque};
    const JOB: &str = "ae2_recursive_job";
    const ESCROW: &str = "ae2_cpu_escrow";
    use super::super::ae2_global::GridAddress;
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(untagged)]
    enum ProviderOwner {
        Block(GridAddress),
        Part { host: GridAddress, face: u8 },
    }
    impl ProviderOwner {
        fn anchor(self) -> GridAddress {
            match self {
                Self::Block(p) => p,
                Self::Part { host, .. } => host,
            }
        }
        fn id(self) -> String {
            let p = self.anchor().encoded();
            match self {
                Self::Block(_) => format!("{},{},{}", p.0, p.1, p.2),
                Self::Part { face, .. } => format!("{},{},{}/face{}", p.0, p.1, p.2, face),
            }
        }
    }
    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct Spec {
        provider: ProviderOwner,
        slot: usize,
        /// New jobs select a source definition at dispatch; old saved jobs
        /// retain their original physical provider and slot.
        #[serde(default)]
        definition: Option<String>,
        /// Optional source crafting recipe ID, absent in legacy saved jobs.
        #[serde(default)]
        crafting_recipe: Option<String>,
        grid: Vec<String>,
        #[serde(default)]
        choices: Vec<Vec<String>>,
        #[serde(default)]
        fluid_slots: BTreeMap<usize, (String, u64)>,
        #[serde(default)]
        processing: bool,
        #[serde(default)]
        special: Option<(String,String)>,
        #[serde(default)]
        special_grid_version:u8,
        #[serde(default)]
        fluid_inputs: Vec<(String, u64)>,
        #[serde(default)]
        fluid_outputs: Vec<(String, u64)>,
        #[serde(default)]
        extra_outputs: Vec<String>,
        output: String,
    }
    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct Session {
        cpu: Cpu,
        #[serde(default)]
        suspended: bool,
        specs: BTreeMap<String, Spec>,
        flights: BTreeMap<u64, GridAddress>,
        #[serde(default)]
        job_id: Option<[i64; 2]>,
        #[serde(default)]
        address_version: u8,
        #[serde(default)]
        requester: Option<InterfaceRequester>,
        #[serde(default)]
        export_requester: Option<ExportRequester>,
        #[serde(default)]
        player_owner: Option<[i64;2]>,
    }
    fn job_status(session:&Session,status:horizon_client::ae2_job_status::Status)->Option<(u128,horizon_client::ae2_job_status::CraftingJobStatus)> {
        let owner=super::super::ae2_job_events::owner_uuid(session.player_owner?);
        let (key,requested,remaining)=session.cpu.requested_output()?;
        Some((owner,horizon_client::ae2_job_status::CraftingJobStatus {
            job_id:session.job_id?,key:key.to_owned(),requested,remaining,status,
        }))
    }
    fn selection_policy(mode:Option<&str>,machine_request:bool)->Option<bool> {
        match (mode,machine_request) {
            (Some("ANY"),_)=>Some(false),
            (Some("PLAYER_ONLY"),false)|(Some("MACHINE_ONLY"),true)=>Some(true),
            _=>None,
        }
    }
    fn migrate_job_identity(session: &mut Session, fresh: [i64;2]) -> bool {
        if session.job_id.is_some() { return false; }
        session.job_id=Some(fresh);
        true
    }
    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct AssemblerFlightOwner {
        cpu: GridAddress,
        job_id: [i64; 2],
        flight: u64,
    }
    fn matching_assembler_flight(
        owner: &AssemblerFlightOwner,
        session: &Session,
        assembler: GridAddress,
    ) -> bool {
        session.job_id == Some(owner.job_id)
            && session.flights.get(&owner.flight) == Some(&assembler)
    }
    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct InterfaceRequester {
        owner: super::super::ae2_interface::InterfaceCraftOwner,
        slot: usize,
        target: String,
        amount: u64,
    }
    #[derive(Clone, Debug, Serialize, Deserialize)]
    struct ExportRequester {
        owner: super::super::ae2_buses::ExportBusCraftOwner,
        slot: usize,
        target: String,
        amount: u64,
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum RequestOwner {
        Interface(super::super::ae2_interface::InterfaceCraftOwner),
        Export(super::super::ae2_buses::ExportBusCraftOwner),
    }
    impl RequestOwner {
        fn anchor(self) -> GridAddress {
            match self { Self::Interface(owner) => owner.anchor(), Self::Export(owner) => owner.anchor() }
        }
        fn identity(self, network: &ae2_network_core::Network) -> Option<ae2_network_core::NodeIdentity> {
            let pos = self.anchor().network_pos();
            match self {
                Self::Interface(super::super::ae2_interface::InterfaceCraftOwner::Block(_)) => network.node_identities.get(&pos),
                Self::Interface(super::super::ae2_interface::InterfaceCraftOwner::Part { face, .. }) => network.part_identities.get(&(pos, face)),
                Self::Export(owner) => network.part_identities.get(&(pos, owner.face)),
            }.copied()
        }
    }
    /// One source MultiCraftingTracker future per physical requester slot.
    pub(in crate::game) struct PendingMachineCraft {
        owner: RequestOwner,
        slot: usize,
        key: ae2_cell_core::ItemKey,
        fluid: bool,
        identity: ae2_network_core::NodeIdentity,
        generation: i64,
        registry: ExtendedRegistry,
        calculation: Option<ae2_crafting_core::calculation::Calculation>,
        result: Option<Result<(ae2_crafting_core::extended::Plan, Inventory), ae2_crafting_core::Error>>,
    }
    struct Request {
        owner: RequestOwner,
        slot: usize,
        target: String,
        amount: u64,
    }
    fn session_request(s: &Session) -> Result<Option<Request>, String> {
        match (&s.requester, &s.export_requester) {
            (Some(_), Some(_)) => Err("multiple CPU requesters".into()),
            (Some(r), None) => Ok(Some(Request {
                owner: RequestOwner::Interface(r.owner),
                slot: r.slot,
                target: r.target.clone(),
                amount: r.amount,
            })),
            (None, Some(r)) => Ok(Some(Request {
                owner: RequestOwner::Export(r.owner),
                slot: r.slot,
                target: r.target.clone(),
                amount: r.amount,
            })),
            (None, None) => Ok(None),
        }
    }
    fn from_key(key: &str) -> Result<ItemStack, String> {
        item_data::parse_stack(key)?.ok_or_else(|| "empty item key".into())
    }
    fn unloaded_waiting_conflicts(waiting:&Inventory,key:Option<&str>) -> bool {
        key.is_none_or(|key|waiting.contains_key(key))
    }
    fn read_escrow(machine: &Machine) -> Result<Vec<ItemStack>, String> {
        match machine.config.get(&ESCROW.into()) {
            None => Ok(Vec::new()),
            Some(Tag::List(items)) => items
                .iter()
                .map(|t| {
                    if !matches!(t.get("count"),Some(Tag::Int(n)) if *n>0) {
                        return Err("invalid physical escrow count".into());
                    }
                    horizon_client::save::item_from_nbt(t)
                        .ok_or_else(|| "invalid physical escrow stack".into())
                })
                .collect(),
            _ => Err("invalid physical escrow".into()),
        }
    }
    fn escrow_counts(items: &[ItemStack]) -> Result<Inventory, String> {
        let mut out = Inventory::new();
        for item in items {
            let entry = out.entry(canon(item)).or_default();
            *entry = entry
                .checked_add(u64::from(item.count))
                .ok_or("escrow overflow")?;
        }
        Ok(out)
    }
    const FLUID_ESCROW: &str = "ae2_cpu_fluid_escrow";
    fn fluid_name(key: &ae2_cell_core::ItemKey) -> String {
        let bytes = key
            .components
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        format!("fluid|{}|{}", key.item_id, bytes)
    }
    fn processing_spec_outputs(
        pattern: &ItemStack,
        outputs: &[Option<ItemStack>],
        fluids: &[ae2_processing::PatternFluid],
    ) -> Result<(String, Vec<String>), String> {
        let mut items: Vec<_> = outputs.iter().flatten().map(|item| item_data::canon_stack(Some(item))).collect();
        if let Some((_, positioned)) = ae2_processing::ae2_decode_processing_positions(pattern)? {
            if let Some(fluid) = positioned.first().and_then(Option::as_ref) {
                return Ok((fluid_name(&fluid.key), items));
            }
        }
        let primary = if items.is_empty() {
            fluids.first().map(|fluid| fluid_name(&fluid.key)).ok_or("processing pattern has no primary output")?
        } else { items.remove(0) };
        Ok((primary, items))
    }
    fn parse_fluid(name: &str) -> Option<ae2_cell_core::ItemKey> {
        let (id, hex) = name.strip_prefix("fluid|")?.split_once('|')?;
        if id.is_empty()
            || !hex.bytes().all(|b| b.is_ascii_hexdigit())
            || hex.len() % 2 != 0
            || hex.len() > 2097152
        {
            return None;
        }
        let bytes = (0..hex.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
            .collect::<Option<Vec<_>>>()?;
        Some(ae2_cell_core::ItemKey {
            item_id: id.to_owned(),
            components: bytes,
        })
    }
    fn read_fluids(machine: &Machine) -> Result<BTreeMap<ae2_cell_core::ItemKey, u64>, String> {
        let mut out = BTreeMap::new();
        let Some(value) = machine.config.get(&FLUID_ESCROW.into()) else {
            return Ok(out);
        };
        let Tag::List(entries) = value else {
            return Err("invalid CPU fluid escrow".into());
        };
        for entry in entries {
            let Some(Tag::String(id)) = entry.get("fluid") else {
                return Err("invalid CPU fluid identity".into());
            };
            let id = id.to_string().map_err(|_| "invalid CPU fluid identity")?;
            let Some(Tag::ByteArray(components)) = entry.get("components") else {
                return Err("invalid CPU fluid components".into());
            };
            let Some(Tag::Long(amount)) = entry.get("amount") else {
                return Err("invalid CPU fluid amount".into());
            };
            if *amount <= 0 {
                return Err("invalid CPU fluid amount".into());
            }
            let key = ae2_cell_core::ItemKey {
                item_id: id,
                components: components.clone(),
            };
            if out.insert(key, *amount as u64).is_some() {
                return Err("duplicate CPU fluid key".into());
            }
        }
        Ok(out)
    }
    fn write_fluids(
        machine: &mut Machine,
        fluids: &BTreeMap<ae2_cell_core::ItemKey, u64>,
    ) -> Result<(), String> {
        let entries = fluids
            .iter()
            .map(|(k, v)| {
                let amount = i64::try_from(*v).map_err(|_| "fluid amount overflow")?;
                if amount <= 0 {
                    return Err("invalid fluid amount");
                };
                Ok(Tag::compound([
                    ("fluid", Tag::String(k.item_id.as_str().into())),
                    ("components", Tag::ByteArray(k.components.clone())),
                    ("amount", Tag::Long(amount)),
                ]))
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(str::to_owned)?;
        machine
            .config
            .insert(FLUID_ESCROW.into(), Tag::List(entries));
        Ok(())
    }
    fn owned_counts(
        items: &[ItemStack],
        fluids: &BTreeMap<ae2_cell_core::ItemKey, u64>,
    ) -> Result<Inventory, String> {
        let mut out = escrow_counts(items)?;
        for (k, n) in fluids {
            if out.insert(fluid_name(k), *n).is_some() {
                return Err("duplicate fluid identity".into());
            }
        }
        Ok(out)
    }
    fn read_session(machine: &Machine, cpu_dim: super::super::Dim) -> Result<Session, String> {
        let mut session: Session = match machine.config.get(&JOB.into()) {
            Some(Tag::String(raw))=>serde_json::from_str(&raw.to_string_lossy()),
            Some(Tag::ByteArray(raw))=>serde_json::from_slice(raw),
            None=>return Err("CPU has no recursive job".into()),
            _=>return Err("invalid CPU job".into()),
        }.map_err(|_| "invalid CPU job")?;
        // Older prototype jobs used unquoted namespaced SNBT keys. Normalize
        // every item identity together before checking physical ownership.
        let normalize=|value:&str| -> Result<String,String> {
            if value=="-" || value.starts_with("fluid|") {return Ok(value.to_owned());}
            Ok(item_data::canon_stack(item_data::parse_stack(value)?.as_ref()))
        };
        session.cpu.remap_keys(normalize)?;
        for spec in session.specs.values_mut() {
            for value in &mut spec.grid {*value=normalize(value)?;}
            for choices in &mut spec.choices {for value in choices {*value=normalize(value)?;}}
            for value in &mut spec.extra_outputs {*value=normalize(value)?;}
            spec.output=normalize(&spec.output)?;
        }
        if let Some(request)=session.requester.as_mut() {request.target=normalize(&request.target)?;}
        if let Some(request)=session.export_requester.as_mut() {request.target=normalize(&request.target)?;}
        // Stage 37 briefly stored special assembler grids in compact encoding
        // positions. Migrate pending (not yet in-flight) jobs without touching
        // their physical escrow; old in-flight grids finish in their old form.
        if session.flights.is_empty() {
            for spec in session.specs.values_mut().filter(|spec|spec.special.is_some() && spec.special_grid_version==0) {
                if spec.grid.len()!=9 || spec.choices.len()!=9 {return Err("invalid legacy special grid".into());}
                let kind=&spec.special.as_ref().unwrap().0;
                let mapping:&[(usize,usize)]=match kind.as_str(){"stonecutting"=>&[(0,4)],"smithing"=>&[(0,3),(1,4),(2,5)],_=>return Err("invalid special pattern kind".into())};
                let mut grid=vec!["-".to_string();9];let mut choices=vec![Vec::new();9];
                for &(from,to) in mapping {grid[to]=spec.grid[from].clone();choices[to]=spec.choices[from].clone();}
                spec.grid=grid;spec.choices=choices;spec.special_grid_version=1;
            }
        }
        if session.address_version == 0 {
            for spec in session.specs.values_mut() {
                let ProviderOwner::Block(old) = spec.provider else {
                    return Err("legacy job contains part provider".into());
                };
                spec.provider = ProviderOwner::Block(
                    GridAddress::from_local(cpu_dim, old.encoded())
                        .ok_or("invalid legacy provider position")?,
                );
            }
            for at in session.flights.values_mut() {
                *at = GridAddress::from_local(cpu_dim, at.encoded())
                    .ok_or("invalid legacy assembler position")?;
            }
            session.address_version = 1;
        } else if session.address_version != 1 {
            return Err("unknown CPU address version".into());
        }
        if owned_counts(&read_escrow(machine)?, &read_fluids(machine)?)? != *session.cpu.escrow() {
            return Err("CPU physical escrow does not match job; retained for recovery".into());
        }
        Ok(session)
    }
    // NBT strings use modified UTF-8 and are limited to 65535 bytes. Keep the
    // legacy representation when it fits, and use a byte array for larger
    // valid plans so the chunk can be saved and reloaded without truncation.
    fn job_payload(data:String)->Result<Tag,String> {
        let len=data.encode_utf16().try_fold(0usize,|n,c|n.checked_add(match c {1..=127=>1,0..=2047=>2,_=>3}))
            .ok_or("CPU job size overflow")?;
        if len<=u16::MAX as usize {Ok(Tag::String(data.as_str().into()))}
        else if data.len()<=horizon_world::nbt::Limits::default().max_bytes {Ok(Tag::ByteArray(data.into_bytes()))}
        else {Err("CPU job exceeds save size".into())}
    }
    fn write_session(
        machine: &mut Machine,
        session: &Session,
        items: &[ItemStack],
    ) -> Result<(), String> {
        if owned_counts(items, &read_fluids(machine)?)? != *session.cpu.escrow() {
            return Err("CPU escrow mismatch".into());
        }
        let data = serde_json::to_string(session).map_err(|e| e.to_string())?;
        machine
            .config
            .insert(JOB.into(), job_payload(data)?);
        machine.config.insert(
            ESCROW.into(),
            Tag::List(
                items
                    .iter()
                    .map(horizon_client::save::item_to_nbt)
                    .collect(),
            ),
        );
        Ok(())
    }
    fn kind(kind: MachineKind) -> bool {
        matches!(
            kind,
            MachineKind::CraftingUnit
                | MachineKind::CraftingAccelerator
                | MachineKind::CraftingStorage1k
                | MachineKind::CraftingStorage4k
                | MachineKind::CraftingStorage16k
                | MachineKind::CraftingStorage64k
                | MachineKind::CraftingStorage256k
                | MachineKind::CraftingMonitor
        )
    }
    fn capacity(kind: MachineKind) -> u64 {
        match kind {
            MachineKind::CraftingStorage1k => 1024,
            MachineKind::CraftingStorage4k => 4096,
            MachineKind::CraftingStorage16k => 16384,
            MachineKind::CraftingStorage64k => 65536,
            MachineKind::CraftingStorage256k => 262144,
            _ => 0,
        }
    }
    fn neighbors((x, y, z): Pos) -> impl Iterator<Item = Pos> {
        [
            (1, 0, 0),
            (-1, 0, 0),
            (0, 1, 0),
            (0, -1, 0),
            (0, 0, 1),
            (0, 0, -1),
        ]
        .into_iter()
        .filter_map(move |(a, b, c)| {
            Some((x.checked_add(a)?, y.checked_add(b)?, z.checked_add(c)?))
        })
    }
    fn cluster(g: &Game<'_>, root: Pos, allow_job: bool) -> Result<(Vec<Pos>, u64, u32), String> {
        cluster_checked(&g.world, root, allow_job, true)
    }
    fn cluster_checked(
        world: &horizon_client::world::World,
        root: Pos,
        allow_job: bool,
        check_busy: bool,
    ) -> Result<(Vec<Pos>, u64, u32), String> {
        let mut seen = BTreeSet::from([root]);
        let mut q = VecDeque::from([root]);
        while let Some(pos) = q.pop_front() {
            for n in neighbors(pos) {
                if !seen.contains(&n)
                    && matches!(world.block_entities.get(&n),Some(BlockEntity::Ae2(m)) if kind(m.kind))
                {
                    seen.insert(n);
                    q.push_back(n);
                }
            }
        }
        let min = (
            seen.iter().map(|p| p.0).min().unwrap(),
            seen.iter().map(|p| p.1).min().unwrap(),
            seen.iter().map(|p| p.2).min().unwrap(),
        );
        let max = (
            seen.iter().map(|p| p.0).max().unwrap(),
            seen.iter().map(|p| p.1).max().unwrap(),
            seen.iter().map(|p| p.2).max().unwrap(),
        );
        let dims = [
            i64::from(max.0) - i64::from(min.0),
            i64::from(max.1) - i64::from(min.1),
            i64::from(max.2) - i64::from(min.2),
        ];
        if dims.iter().any(|&n| n > 16)
            || (dims[0] + 1) * (dims[1] + 1) * (dims[2] + 1) != seen.len() as i64
        {
            return Err(
                "crafting CPU must be a filled rectangular cluster within 17 blocks per axis"
                    .into(),
            );
        }
        let mut bytes = 0u64;
        let mut coprocessors = 0u32;
        for p in &seen {
            if let Some(BlockEntity::Ae2(m)) = world.block_entities.get(p) {
                bytes = bytes
                    .checked_add(capacity(m.kind))
                    .ok_or("CPU capacity overflow")?;
                if m.kind == MachineKind::CraftingAccelerator {
                    coprocessors += 1;
                }
                if check_busy
                    && ((m.config.contains_key(&JOB.into()) && !(allow_job && *p == root))
                        || m.config.contains_key(&"ae2_cpu_job".into()))
                {
                    return Err("CPU cluster is busy".into());
                }
            }
        }
        if bytes == 0 {
            return Err("CPU cluster has no crafting storage".into());
        }
        Ok((seen.into_iter().collect(), bytes, coprocessors))
    }
    fn ordered_owner(medium: &super::super::ae2_provider_registry::ProviderMedium) -> Result<ProviderOwner, String> {
        let dim = match medium.dimension {
            0 => super::super::Dim::Overworld, 1 => super::super::Dim::Nether,
            2 => super::super::Dim::End, 3 => super::super::Dim::Spatial,
            _ => return Err("invalid provider dimension".into()),
        };
        let host = GridAddress::from_local(dim, medium.position).ok_or("invalid provider address")?;
        match (medium.face, medium.node.part) {
            (None, None) => Ok(ProviderOwner::Block(host)),
            (Some(face), Some(_)) if face < 6 => Ok(ProviderOwner::Part { host, face }),
            _ => Err("provider medium identity does not match its face".into()),
        }
    }
    fn register_source_pattern(registry:&mut ExtendedRegistry, pattern:ExtendedPattern,
        ordered:&Option<(String,String)>) -> Result<(), String> {
        match ordered {
            Some((_, primary)) => registry.register_primary(pattern, primary.clone()),
            None => registry.register(pattern),
        }.map_err(|error| format!("{error:?}"))
    }
    /// Mirror AECraftingPattern.getRecipeIngredient: shaped offsets use the
    /// occupied grid origin; shapeless uses the index of this filled slot.
    fn source_crafting_ingredient<'a>(recipes:&'a horizon_client::crafting::Recipes,name:&str,grid:&[Option<ItemStack>],slot:usize)->Option<&'a horizon_client::crafting::Ingredient> {
        use horizon_client::crafting::Recipe;
        let name=name.strip_prefix("minecraft:").unwrap_or(name);
        let (_,recipe)=recipes.crafting.iter().find(|(id,_)|id==name)?;
        match recipe {
            Recipe::Shaped{width,cells,..}=>{
                let top=grid.iter().enumerate().filter(|(_,s)|s.is_some()).map(|(i,_)|i/3).min()?;
                let left=grid.iter().enumerate().filter(|(_,s)|s.is_some()).map(|(i,_)|i%3).min()?;
                let x=(slot%3).checked_sub(left)?;let y=(slot/3).checked_sub(top)?;
                if x>=*width{return None;}cells.get(y.checked_mul(*width)?.checked_add(x)?)?.as_ref()
            }
            Recipe::Shapeless{ingredients,..}=>ingredients.get(grid.iter().take(slot).filter(|s|s.is_some()).count()),
            Recipe::Special(_)=>None,
        }
    }
    /// Produce the source possible-template sequence: first the selected
    /// item/fluid and fuzzy matching keys, then the recipe ingredient's items
    /// in holder order. The same key can appear at two distinct amounts.
    fn source_crafting_templates(recipes:&horizon_client::crafting::Recipes,name:&str,grid:&[Option<ItemStack>],slot:usize,choices:&[String],primary:&str,primary_amount:u64,substitutions:bool)->Vec<(String,u64)> {
        let mut templates=vec![(primary.to_owned(),primary_amount)];
        if parse_fluid(primary).is_none() {
            if let Ok(original)=from_key(primary) {
                for key in choices {
                    if key!=primary && from_key(key).is_ok_and(|item|item.item==original.item) {templates.push((key.clone(),primary_amount));}
                }
            }
        }
        if substitutions {
            if let Some(ingredient)=source_crafting_ingredient(recipes,name,grid,slot) {
                for item in &ingredient.1 {
                    for key in choices {
                        if from_key(key).is_ok_and(|stack|stack.item.as_ref()==item) {templates.push((key.clone(),1));}
                    }
                }
            }
        }
        templates
    }
    fn output_add(outputs: &mut Inventory, s: &ItemStack) -> Result<(), String> {
        let entry = outputs.entry(canon(s)).or_default();
        *entry = entry
            .checked_add(u64::from(s.count))
            .ok_or("output overflow")?;
        Ok(())
    }
    impl Game<'_> {
        /// Resolve a formed, powered local CPU to its canonical physical owner.
        pub(in crate::game) fn ae2_cpu_cluster_owner(&self, pos:Pos)->Option<GridAddress> {
            if !matches!(self.world.block_entities.get(&pos),Some(BlockEntity::Ae2(m)) if kind(m.kind)) || !self.ae2_online(pos) {return None;}
            let (members,_,_)=cluster_checked(&self.world,pos,true,false).ok()?;
            self.ae2_local_address(*members.first()?)
        }
        pub(in crate::game) fn ae2_cpu_selection_mode(&self, cpu:GridAddress)->Option<&'static str> {
            let BlockEntity::Ae2(machine)=self.ae2_block_entity(cpu)? else{return None;};
            match machine.config.get(&"ae2_cpu_selection_mode".into()) {
                None=>Some("ANY"),
                Some(Tag::String(value))=>match value.to_string().ok()?.as_str(){"ANY"=>Some("ANY"),"PLAYER_ONLY"=>Some("PLAYER_ONLY"),"MACHINE_ONLY"=>Some("MACHINE_ONLY"),_=>None},
                _=>None,
            }
        }
        pub(in crate::game) fn ae2_cpu_cycle_selection_mode(&mut self,cpu:GridAddress)->Result<&'static str,String> {
            let next=match self.ae2_cpu_selection_mode(cpu).ok_or("invalid CPU selection mode")? {"ANY"=>"PLAYER_ONLY","PLAYER_ONLY"=>"MACHINE_ONLY",_=>"ANY"};
            let Some(BlockEntity::Ae2(machine))=self.ae2_block_entity_mut(cpu) else{return Err("CPU disappeared".into());};
            machine.config.insert("ae2_cpu_selection_mode".into(),Tag::String(next.into()));self.ae2_mark_modified(cpu);Ok(next)
        }
        /// Public monitor projection of a formed CPU's live job, with no
        /// private inputs or pattern data exposed to a viewing client.
        pub(in crate::game) fn ae2_crafting_monitor_job(
            &self,
            pos: Pos,
        ) -> Option<(ae2_cell_core::ItemKey, bool, u64)> {
            let (members, _, _) = cluster_checked(&self.world, pos, true, false).ok()?;
            for member in members {
                let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get(&member) else {
                    continue;
                };
                if !machine.config.contains_key(&JOB.into()) {
                    continue;
                }
                let session = read_session(machine, self.dim).ok()?;
                let (name, remaining) = session.cpu.display_progress()?;
                if let Some(key) = parse_fluid(name) {
                    return Some((key, true, remaining));
                }
                return Some((ae2_lab::key(&from_key(name).ok()?).ok()?, false, remaining));
            }
            None
        }
        /// Associate only complete, storage-bearing rectangular CPU clusters.
        /// Unformed units retain their ordinary individual grid nodes.
        pub(in crate::game) fn ae2_crafting_graph(&self, network: &mut ae2_network_core::Network) {
            for (dim, world) in self.ae2_loaded_worlds() {
                let mut pending: BTreeSet<Pos> = world
                    .block_entities
                    .iter()
                    .filter_map(|(p, be)| {
                        matches!(be, BlockEntity::Ae2(m) if kind(m.kind)).then_some(*p)
                    })
                    .collect();
                while let Some(&root) = pending.first() {
                    let mut members = BTreeSet::from([root]);
                    let mut queue = VecDeque::from([root]);
                    while let Some(pos) = queue.pop_front() {
                        for next in neighbors(pos) {
                            if pending.contains(&next) && members.insert(next) {
                                queue.push_back(next);
                            }
                        }
                    }
                    for pos in &members {
                        pending.remove(pos);
                    }
                    let Ok((cluster, _, _)) = cluster_checked(world, root, true, false) else {
                        continue;
                    };
                    let Some(group) = self
                        .ae2_dimension_address(dim, cluster[0])
                        .map(|a| a.network_pos())
                    else {
                        continue;
                    };
                    let points: Option<Vec<_>> = cluster
                        .iter()
                        .map(|p| self.ae2_dimension_address(dim, *p).map(|a| a.network_pos()))
                        .collect();
                    let Some(points) = points else { continue };
                    if points.iter().any(|p| !network.nodes.contains_key(p)) {
                        continue;
                    }
                    for pos in points {
                        if let Some(device) = network.nodes.get_mut(&pos) {
                            device.multiblock = Some(group);
                        }
                    }
                }
            }
        }
        fn recursive_patterns(
            &self,
            pos: Pos,
        ) -> Result<(ExtendedRegistry, BTreeMap<String, Spec>), String> {
            let n = self.ae2_network();
            let analysis = n.analyze().map_err(str::to_owned)?;
            let target = self
                .ae2_local_address(pos)
                .ok_or("invalid local grid position")?
                .network_pos();
            let c = analysis
                .nodes
                .get(&target)
                .filter(|s| s.online)
                .ok_or("network offline")?
                .component;
            let mut registry = ExtendedRegistry::default();
            registry.set_resource_classifier(|key| {
                if parse_fluid(key).is_some() {ae2_crafting_core::extended::ResourceKind::Fluid}
                else {ae2_crafting_core::extended::ResourceKind::Item}
            });
            if super::super::ae2_event_live::cpu_formation_enabled() {
                registry.set_emittable(self.ae2_crafting_emittable_keys(pos)?)
                    .map_err(|e| format!("emittable registration: {e:?}"))?;
            }
            let mut specs = BTreeMap::new();
            let mut candidates: Vec<ItemStack> = self
                .ae2_grid_inventory(pos)?
                .into_iter()
                .filter_map(|(k, _)| ae2_lab::prototype(&k).ok())
                .collect();
            // A valid substitute may itself be craftable even when no instance
            // currently exists in storage. Include physical provider patterns'
            // decoded outputs; the planner must still find a feasible recipe.
            let ordered = super::super::ae2_event_live::ordered_provider_enabled();
            let mut providers: Vec<(ProviderOwner, Machine, Option<(String,String)>)> = Vec::new();
            if ordered {
                for definition in self.ae2_ordered_provider_definitions(pos)? {
                    let owner = ordered_owner(&definition.medium)?;
                    let pattern = item_data::parse_stack(&definition.key)?.ok_or("missing registered pattern")?;
                    let mut provider = Machine::new(MachineKind::PatternProvider);
                    provider.slots[0] = Some(pattern);
                    providers.push((owner, provider, Some((definition.key, definition.primary_output))));
                }
            } else {
            for p in &analysis.components[c].positions {
                let Some(address) = GridAddress::from_network(*p) else {
                    continue;
                };
                let Some(status) = analysis.nodes.get(p) else {
                    continue;
                };
                if status.online {
                    if let Some(BlockEntity::Ae2(m)) = self.ae2_block_entity(address) {
                        if m.kind == MachineKind::PatternProvider {
                            providers.push((ProviderOwner::Block(address), (**m).clone(), None));
                        }
                    }
                }
                for (&face, part_status) in &status.parts {
                    if !part_status.online {
                        continue;
                    }
                    let part = super::super::ae2_parts::PartAddress {
                        host: address,
                        face,
                    };
                    if let Ok(Some(m)) = self.ae2_part_machine(part) {
                        if m.kind == MachineKind::PatternProvider {
                            providers.push((
                                ProviderOwner::Part {
                                    host: address,
                                    face,
                                },
                                m,
                                None,
                            ));
                        }
                    }
                }
            }
            }
            for (_, provider, _) in &providers {
                for pattern in provider.slots.iter().flatten() {
                    let mut outputs = Vec::new();
                    if let Some((grid, encoded)) = ae2_processing::decode_pattern(pattern) {
                        if let Some(source)=super::super::ae2_source_patterns::crafting(pattern) {
                            if let Ok(source)=source {if let Some(output)=self.res.recipes().assemble_named(&source.recipe,&grid,3) {outputs.push(output);}}
                        } else {outputs.push(encoded);}
                    }
                    if let Some(special)=ae2_processing::ae2_decode_special_pattern(pattern){
                        if super::super::ae2_source_patterns::special(pattern).is_some() {
                            if let Some(output)=ae2_processing::special_recipe_result(self.res.stations(),&special.kind,&special.recipe,&special.grid){outputs.push(output);}
                        } else {outputs.push(special.output);}
                    }
                    if let Some((_, items)) = ae2_processing::ae2_decode_processing_pattern(pattern)
                    {
                        outputs.extend(items.into_iter().flatten());
                    }
                    for mut output in outputs {
                        output.count = 1;
                        if !candidates
                            .iter()
                            .any(|existing| canon(existing) == canon(&output))
                        {
                            candidates.push(output);
                        }
                    }
                }
            }
            for (owner, provider, source) in &providers {
                let owner = *owner;
                for (slot, pattern) in provider.slots.iter().enumerate() {
                    let id = source.as_ref().map_or_else(|| format!("{}/{}", owner.id(), slot),
                        |(definition,_)| format!("ordered:{definition}"));
                    let definition = source.as_ref().map(|(definition,_)| definition.clone());
                    if let Some(special)=pattern.as_ref().and_then(ae2_processing::ae2_decode_special_pattern) {
                        if special.output.item.as_ref()=="ae2:missing_content" || self.res.items().get(&special.output.item).is_none()
                            || special.grid.iter().flatten().any(|item|item.item.as_ref()=="ae2:missing_content"||self.res.items().get(&item.item).is_none()){continue;}
                        let Some(actual)=ae2_processing::special_recipe_result(self.res.stations(),&special.kind,&special.recipe,&special.grid) else{continue};
                        if self.res.items().get(&actual.item).is_none() {continue;}
                        if pattern.as_ref().and_then(super::super::ae2_source_patterns::special).is_none() && actual!=special.output {continue;}
                        let Some(physical_grid)=ae2_processing::special_assembler_grid(&special.kind,&special.grid) else{continue};
                        let mut choices=vec![Vec::new();9];
                        for (i,original) in physical_grid.iter().enumerate() {
                            let Some(original)=original else{continue;};choices[i].push(canon(original));
                            if special.substitutes {for candidate in &candidates {
                                let key=canon(candidate);if choices[i].contains(&key){continue;}
                                let mut trial=physical_grid.clone();let mut item=candidate.clone();item.count=1;trial[i]=Some(item);
                                if ae2_processing::special_logical_grid(&special.kind,&trial).and_then(|logical|ae2_processing::special_recipe_result(self.res.stations(),&special.kind,&special.recipe,&logical))==Some(actual.clone()){choices[i].push(key);}
                            }}
                        }
                        let inputs=choices.iter().filter(|c|!c.is_empty()).map(|c|Ingredient{choices:c.iter().cloned().map(|key|(key,1)).collect()}).collect();
                        let mut outputs=Inventory::new();output_add(&mut outputs,&actual)?;
                        register_source_pattern(&mut registry, ExtendedPattern{id:id.clone(),inputs,outputs}, source)?;
                        specs.insert(id,Spec{provider:owner,slot,definition:definition.clone(),crafting_recipe:None,grid:physical_grid.iter().map(|item|item_data::canon_stack(item.as_ref())).collect(),choices,fluid_slots:BTreeMap::new(),processing:false,special:Some((special.kind,special.recipe)),special_grid_version:1,fluid_inputs:Vec::new(),fluid_outputs:Vec::new(),extra_outputs:Vec::new(),output:item_data::canon_stack(Some(&actual))});
                        continue;
                    }
                    if let Some((inputs, outputs)) = pattern
                        .as_ref()
                        .and_then(ae2_processing::ae2_decode_processing_pattern)
                    {
                        if inputs.iter().chain(&outputs).flatten().any(|item|item.item.as_ref()=="ae2:missing_content"||self.res.items().get(&item.item).is_none()){continue;}
                        let (fluid_inputs, fluid_outputs) = pattern
                            .as_ref()
                            .and_then(ae2_processing::ae2_decode_processing_fluids)
                            .ok_or("invalid processing fluids")?;
                        let mut ingredients: Vec<_> = inputs
                            .iter()
                            .flatten()
                            .map(|item| Ingredient {
                                choices: Inventory::from([(canon(item), u64::from(item.count))]),
                            })
                            .collect();
                        for fluid in &fluid_inputs {
                            ingredients.push(Ingredient {
                                choices: Inventory::from([(fluid_name(&fluid.key), fluid.amount)]),
                            });
                        }
                        // Source processing patterns condense identical keys
                        // into one input node while keeping actual slots for dispatch.
                        let source_inputs:Vec<_>=ingredients.iter().map(|input| {
                            let key=input.choices.keys().next().expect("processing input key").clone();
                            SourceInput{encoded:key.clone(),primary:key,remaining:BTreeMap::new()}
                        }).collect();
                        let mut produced = Inventory::new();
                        for item in outputs.iter().flatten() {
                            output_add(&mut produced, item)?;
                        }
                        for fluid in &fluid_outputs {
                            let key = fluid_name(&fluid.key);
                            let entry = produced.entry(key).or_default();
                            *entry = entry
                                .checked_add(fluid.amount)
                                .ok_or("fluid output overflow")?;
                        }
                        register_source_pattern(&mut registry, ExtendedPattern {
                            id:id.clone(), inputs:ingredients, outputs:produced,
                        }, source)?;
                        registry.set_source_inputs(&id,source_inputs).map_err(|error|format!("{error:?}"))?;
                        let (output, extra_outputs) = processing_spec_outputs(
                            pattern.as_ref().ok_or("missing processing pattern")?, &outputs, &fluid_outputs)?;
                        specs.insert(
                            id,
                            Spec {
                                provider: owner,
                                slot,
                                definition: definition.clone(),
                                crafting_recipe: None,
                                grid: inputs
                                    .iter()
                                    .map(|s| item_data::canon_stack(s.as_ref()))
                                    .collect(),
                                choices: Vec::new(),
                                fluid_slots: BTreeMap::new(),
                                processing: true,
                                special:None,
                        special_grid_version:0,
                                fluid_inputs: fluid_inputs
                                    .iter()
                                    .map(|f| (fluid_name(&f.key), f.amount))
                                    .collect(),
                                fluid_outputs: fluid_outputs
                                    .iter()
                                    .map(|f| (fluid_name(&f.key), f.amount))
                                    .collect(),
                                extra_outputs,
                                output,
                            },
                        );
                        continue;
                    }
                    let Some((grid, encoded_output)) =
                        pattern.as_ref().and_then(ae2_processing::decode_pattern)
                    else {
                        continue;
                    };
                    if encoded_output.item.as_ref()=="ae2:missing_content" || self.res.items().get(&encoded_output.item).is_none()
                        || grid.iter().flatten().any(|item|item.item.as_ref()=="ae2:missing_content"||self.res.items().get(&item.item).is_none()){continue;}
                    let source_crafting = pattern.as_ref().and_then(|p| super::super::ae2_source_patterns::crafting(p))
                        .and_then(Result::ok);
                    let source_recipe = source_crafting.as_ref().map(|source|source.recipe.clone());
                    let (name, actual) = if let Some(recipe) = source_recipe.as_deref() {
                        let Some(actual) = self.res.recipes().assemble_named(recipe, &grid, 3) else { continue; };
                        (recipe.strip_prefix("minecraft:").unwrap_or(recipe), actual)
                    } else {
                        let Some(result) = self.res.recipes().find(&grid, 3) else { continue; };
                        result
                    };
                    if source_recipe.is_none() && actual != encoded_output { continue; }
                    if self.res.items().get(&actual.item).is_none() {continue;}
                    let expected=actual.clone();
                    let base_remainders=self.res.recipes().remainders(name,&grid,3);
                    let mut outputs = Inventory::new();
                    output_add(&mut outputs, &actual)?;
                    for s in base_remainders.iter().flatten() {output_add(&mut outputs,s)?;}
                    let substitutions = pattern.as_ref().and_then(ae2_processing::ae2_crafting_pattern_options)
                        .is_some_and(|options| options.0);
                    let mut choices = vec![Vec::new(); 9];
                    let mut source_inputs:Vec<Option<SourceInput>>=vec![None;9];
                    for (i, original) in grid.iter().enumerate() {
                        let Some(original) = original else { continue };
                        let primary=canon(original);
                        choices[i].push(primary.clone());
                        let mut remaining=BTreeMap::new();
                        if let Some(remainder)=base_remainders.get(i).and_then(Option::as_ref) {
                            remaining.insert(primary.clone(),canon(remainder));
                        }
                        if substitutions {
                            for candidate in &candidates {
                                let key = canon(candidate);
                                if choices[i].contains(&key) {
                                    continue;
                                }
                                let mut trial = grid.clone();
                                let mut candidate = candidate.clone();
                                candidate.count = 1;
                                trial[i] = Some(candidate);
                                let (candidate_name, candidate_result) = if source_recipe.is_some() {
                                    let Some(result) = self.res.recipes().assemble_named(name, &trial, 3) else { continue; };
                                    (name, result)
                                } else {
                                    let Some(result) = self.res.recipes().find(&trial, 3) else { continue; };
                                    result
                                };
                                if candidate_name != name || candidate_result != actual {
                                    continue;
                                }
                                let candidate_remainders=self.res.recipes().remainders(candidate_name,&trial,3);
                                // AECraftingPattern.isItemValid compares the
                                // matching recipe's assembled result, not its
                                // remaining containers. Track each candidate's
                                // actual return for the physical CPU flight.
                                if let Some(remainder)=candidate_remainders.get(i).and_then(Option::as_ref) {
                                    remaining.insert(key.clone(),canon(remainder));
                                }
                                choices[i].push(key);
                            }
                        }
                        source_inputs[i]=Some(SourceInput{encoded:primary.clone(),primary,remaining});
                    }
                    let allow_fluid = pattern.as_ref().and_then(ae2_processing::ae2_crafting_pattern_options)
                        .is_some_and(|options| options.1);
                    let mut fluid_slots = BTreeMap::new();
                    if allow_fluid {
                        // AECraftingPattern compresses identical encoded keys,
                        // then chooses its fluid template from the first slot.
                        let mut grouped = BTreeMap::<String, Option<String>>::new();
                        for (index, item) in grid.iter().enumerate() {
                            let Some(item) = item else { continue; };
                            let encoded = canon(item);
                            let fluid = grouped.entry(encoded).or_insert_with(|| {
                                let fluid = super::super::ae2_fluids::bucket_fluid(&item.item)?;
                                let remainder = base_remainders.get(index)?.as_ref()?;
                                (remainder.item.as_ref() == "minecraft:bucket" && remainder.count == 1)
                                    .then(|| fluid_name(&super::super::ae2_fluids::fluid_key(fluid)))
                            }).clone();
                            let Some(fluid) = fluid else { continue; };
                            let Some(input) = source_inputs[index].as_mut() else { return Err("missing source fluid slot".into()); };
                            input.primary = fluid.clone();
                            if !substitutions { choices[index].clear(); input.remaining.clear(); }
                            choices[index].insert(0, fluid.clone());
                            fluid_slots.insert(index, (fluid, 1000));
                            // Any physical remainder from this encoded slot is
                            // absent when its primary input is a fluid stack.
                            if let Some(remainder) = base_remainders.get(index).and_then(Option::as_ref) {
                                let key = canon(remainder);
                                let n = outputs.get_mut(&key).ok_or("missing fluid-slot remainder")?;
                                *n = n.checked_sub(u64::from(remainder.count)).ok_or("bad fluid-slot remainder")?;
                                if *n == 0 { outputs.remove(&key); }
                            }
                        }
                    }
                    let inputs: Vec<_> = choices.iter().enumerate().filter(|(_, choices)|!choices.is_empty())
                        .map(|(index, choices)| Ingredient {
                            choices: choices.iter().cloned().map(|key| {
                                let primary=source_inputs[index].as_ref().is_some_and(|input|input.primary==key);
                                let item_amount=if primary {source_crafting.as_ref().map_or(1,|source|source.input_amounts[index])} else {1};
                                let amount = fluid_slots.get(&index).filter(|(fluid,_)| fluid == &key).map_or(item_amount, |(_, amount)| *amount);
                                (key, amount)
                            }).collect(),
                        }).collect();
                    let multipliers=source_crafting.as_ref().map(|source|choices.iter().enumerate()
                        .filter(|(_,choices)|!choices.is_empty()).map(|(index,_)|source.input_amounts[index]).collect());
                    let templates=source_crafting.as_ref().map(|source|choices.iter().enumerate().filter(|(_,choices)|!choices.is_empty()).map(|(index,choices)| {
                        let primary=&source_inputs[index].as_ref().expect("source crafting input").primary;
                        let amount=fluid_slots.get(&index).filter(|(fluid,_)|fluid==primary).map_or(source.input_amounts[index],|(_,amount)|*amount);
                        source_crafting_templates(self.res.recipes(),name,&grid,index,choices,primary,amount,substitutions)
                    }).collect());
                    register_source_pattern(&mut registry, ExtendedPattern {
                        id:id.clone(), inputs, outputs,
                    }, source)?;
                    registry.set_source_inputs(&id,source_inputs.into_iter().flatten().collect()).map_err(|error|format!("{error:?}"))?;
                    if let Some(multipliers)=multipliers {registry.set_source_multipliers(&id,multipliers).map_err(|error|format!("{error:?}"))?;}
                    if let Some(templates)=templates {registry.set_source_templates(&id,templates).map_err(|error|format!("{error:?}"))?;}
                    registry.set_source_outputs(&id,[canon(&actual)]).map_err(|error|format!("{error:?}"))?;
                    specs.insert(id, Spec {
                        provider: owner,
                        slot,
                        definition: definition.clone(),
                        crafting_recipe: source_recipe.clone(),
                        grid: grid.iter().map(|s| item_data::canon_stack(s.as_ref())).collect(),
                        choices,
                        fluid_slots,
                        processing: false,
                        special: None,
                        special_grid_version: 0,
                        fluid_inputs: Vec::new(),
                        fluid_outputs: Vec::new(),
                        extra_outputs: Vec::new(),
                        output: item_data::canon_stack(Some(&expected)),
                    });
                }
            }
            Ok((registry, specs))
        }
        /// Virtual output keys for terminal listings; never create physical stacks.
        pub(in crate::game) fn ae2_craftable_outputs(
            &self,
            pos: Pos,
        ) -> Vec<ae2_cell_core::ItemKey> {
            self.recursive_patterns(pos)
                .ok()
                .into_iter()
                .flat_map(|(_, specs)| specs.into_values())
                .flat_map(|s| {
                    let mut outputs=vec![s.output];
                    if !super::super::ae2_event_live::ordered_provider_enabled() { outputs.extend(s.extra_outputs); }
                    outputs
                })
                .filter_map(|s| item_data::parse_stack(&s).ok().flatten())
                .filter_map(|s| ae2_lab::key(&s).ok())
                .collect()
        }
        pub(in crate::game) fn ae2_craftable_fluids(
            &self,
            pos: Pos,
        ) -> Vec<ae2_cell_core::ItemKey> {
            self.recursive_patterns(pos)
                .ok()
                .into_iter()
                .flat_map(|(_, specs)| specs.into_values())
                .flat_map(|s| {
                    if super::super::ae2_event_live::ordered_provider_enabled() {
                        parse_fluid(&s.output).into_iter().collect::<Vec<_>>()
                    } else {
                        s.fluid_outputs.into_iter().filter_map(|(name,_)|parse_fluid(&name)).collect::<Vec<_>>()
                    }
                })
                .collect()
        }
        /// Reserve real cell/container stacks into the CPU's physical NBT escrow.
        pub(in crate::game) fn ae2_begin_recursive_craft(
            &mut self,
            pos: Pos,
            key: &ae2_cell_core::ItemKey,
            amount: u64,
        ) -> Result<GridAddress, String> {
            let exemplar = ae2_lab::prototype(key)?;
            self.ae2_begin_recursive_target(pos, canon(&exemplar), amount)
        }
        pub(in crate::game) fn ae2_begin_recursive_craft_on(
            &mut self,
            pos: Pos,
            key: &ae2_cell_core::ItemKey,
            amount: u64,
            cpu: GridAddress,
        ) -> Result<GridAddress, String> {
            self.ae2_begin_recursive_target_on(
                pos,
                canon(&ae2_lab::prototype(key)?),
                amount,
                Some(cpu), false, None, None,
            )
        }
        /// Eligible physical CPU clusters on this network, one stable address
        /// per cluster. Submission rechecks availability and capacity.
        pub(in crate::game) fn ae2_craft_cpu_candidates(
            &self,
            pos: Pos,
            required: u64,
        ) -> Result<Vec<(GridAddress, u64, u32)>, String> {
            if super::super::ae2_event_live::cpu_formation_enabled() {
                return self.ae2_observed_cpu_candidates(pos,required);
            }
            let network = self.ae2_network();
            let analysis = network.analyze().map_err(str::to_owned)?;
            let node = self
                .ae2_local_address(pos)
                .ok_or("invalid grid position")?
                .network_pos();
            let component = analysis
                .nodes
                .get(&node)
                .filter(|s| s.online)
                .ok_or("network offline")?
                .component;
            let mut result = BTreeMap::new();
            for point in &analysis.components[component].positions {
                if !analysis.nodes.get(point).is_some_and(|s| s.online) {
                    continue;
                }
                let Some(address) = GridAddress::from_network(*point) else {
                    continue;
                };
                let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity(address) else {
                    continue;
                };
                if !kind(machine.kind) {
                    continue;
                }
                let Some((dim, local)) = self.ae2_address_parts(address) else {
                    continue;
                };
                let Some(world) = self.ae2_world_for(dim) else {
                    continue;
                };
                let Ok((members, capacity, coprocessors)) =
                    cluster_checked(world, local, false, true)
                else {
                    continue;
                };
                if capacity < required {
                    continue;
                }
                let Some(root) = self.ae2_dimension_address(dim, members[0]) else {
                    continue;
                };
                result.insert(root, (root, capacity, coprocessors));
            }
            Ok(result.into_values().collect())
        }
        /// Calculate a confirmation preview from the current physical network.
        /// This never reserves, extracts, or otherwise mutates its inputs.
        pub(in crate::game) fn ae2_preview_recursive_craft(
            &self,
            pos: Pos,
            key: &ae2_cell_core::ItemKey,
            amount: u64,
        ) -> Result<ae2_crafting_core::extended::Plan, String> {
            self.ae2_preview_recursive_target(pos, canon(&ae2_lab::prototype(key)?), amount)
        }
        pub(in crate::game) fn ae2_preview_recursive_fluid_craft(
            &self,
            pos: Pos,
            key: &ae2_cell_core::ItemKey,
            amount: u64,
        ) -> Result<ae2_crafting_core::extended::Plan, String> {
            self.ae2_preview_recursive_target(pos, fluid_name(key), amount)
        }
        fn ae2_preview_recursive_target(
            &self,
            pos: Pos,
            target: String,
            amount: u64,
        ) -> Result<ae2_crafting_core::extended::Plan, String> {
            if amount == 0 {
                return Err("craft amount must be positive".into());
            }
            let (registry, _) = self.recursive_patterns(pos)?;
            let mut available = Inventory::new();
            for (key, count) in self.ae2_grid_inventory(pos)? {
                let entry = available
                    .entry(canon(&ae2_lab::prototype(&key)?))
                    .or_default();
                *entry = entry.checked_add(count).ok_or("inventory overflow")?;
            }
            for (key, count) in self.ae2_grid_fluids(pos)? {
                available.insert(fluid_name(&key), count);
            }
            available.remove(&target);
            let plan = registry
                .plan(&available, &target, amount)
                .map_err(|e| format!("craft planning: {e:?}"))?;
            if plan.steps.is_empty() && plan.emitted.is_empty() {
                return Err("no craft steps".into());
            }
            Ok(plan)
        }
        pub(in crate::game) fn ae2_craft_material_summary(
            &self,
            plan: &ae2_crafting_core::extended::Plan,
            missing: &Inventory,
        ) -> Vec<String> {
            let mut lines = Vec::new();
            let keys:BTreeSet<_>=plan.reserved.keys().chain(missing.keys()).collect();
            for name in keys {
                let label = if let Some(fluid) = parse_fluid(name) {
                    format!("{} mB", fluid.item_id)
                } else {
                    from_key(name)
                        .map(|s| s.item.to_string())
                        .unwrap_or_else(|_| "Unknown item".into())
                };
                let shortage = missing.get(name).copied().unwrap_or(0);
                let present = plan.reserved.get(name).copied().unwrap_or(0);
                if present > 0 {
                    lines.push(format!("Stored {present} x {label}"));
                }
                if shortage > 0 {
                    lines.push(format!("Missing {shortage} x {label}"));
                }
            }
            let mut produced = BTreeMap::<String, u128>::new();
            for step in &plan.steps {
                for (name, count) in &step.outputs {
                    let entry = produced.entry(name.clone()).or_default();
                    *entry += u128::from(*count);
                }
            }
            for (name, count) in produced {
                let label = if let Some(fluid) = parse_fluid(&name) {
                    format!("{} mB", fluid.item_id)
                } else {
                    from_key(&name)
                        .map(|s| s.item.to_string())
                        .unwrap_or_else(|_| "Unknown item".into())
                };
                lines.push(format!("Planned {count} x {label}"));
            }
            lines
        }
        /// Capture only owned data for the worker; physical resources remain
        /// in the world and are checked again when the player submits.
        pub(in crate::game) fn ae2_craft_calculation_snapshot(
            &self,pos:Pos,key:&ae2_cell_core::ItemKey,fluid:bool,amount:u64,
        )->Result<(ExtendedRegistry,Inventory,String),String> {
            if amount==0 {return Err("craft amount must be positive".into());}
            let target=if fluid {fluid_name(key)} else {canon(&ae2_lab::prototype(key)?)};
            let (registry,_)=self.recursive_patterns(pos)?;
            let mut available=Inventory::new();
            for (key,count) in self.ae2_grid_inventory(pos)? {
                let entry=available.entry(canon(&ae2_lab::prototype(&key)?)).or_default();
                *entry=entry.checked_add(count).ok_or("inventory overflow")?;
            }
            for (key,count) in self.ae2_grid_fluids(pos)? {
                available.insert(fluid_name(&key),count);
            }
            available.remove(&target);
            Ok((registry,available,target))
        }
        /// Build a hypothetical report without touching the actual grid.
        pub(in crate::game) fn ae2_craft_report(
            &self,
            pos: Pos,
            key: &ae2_cell_core::ItemKey,
            fluid: bool,
            amount: u64,
        ) -> Result<(ae2_crafting_core::extended::Plan, Inventory), String> {
            let target = if fluid {
                fluid_name(key)
            } else {
                canon(&ae2_lab::prototype(key)?)
            };
            let (registry, _) = self.recursive_patterns(pos)?;
            let mut available = Inventory::new();
            for (key, count) in self.ae2_grid_inventory(pos)? {
                let entry = available
                    .entry(canon(&ae2_lab::prototype(&key)?))
                    .or_default();
                *entry = entry.checked_add(count).ok_or("inventory overflow")?;
            }
            for (key, count) in self.ae2_grid_fluids(pos)? {
                available.insert(fluid_name(&key), count);
            }
            available.remove(&target);
            registry
                .report(&available, &target, amount)
                .map_err(|e| format!("craft planning: {e:?}"))
        }
        pub(in crate::game) fn ae2_begin_recursive_fluid_craft(
            &mut self,
            pos: Pos,
            key: &ae2_cell_core::ItemKey,
            amount: u64,
        ) -> Result<GridAddress, String> {
            self.ae2_begin_recursive_target(pos, fluid_name(key), amount)
        }
        pub(in crate::game) fn ae2_begin_recursive_fluid_craft_on(
            &mut self,
            pos: Pos,
            key: &ae2_cell_core::ItemKey,
            amount: u64,
            cpu: GridAddress,
        ) -> Result<GridAddress, String> {
            self.ae2_begin_recursive_target_on(pos, fluid_name(key), amount, Some(cpu), false, None, None)
        }
        fn ae2_request_valid(
            &self,
            r: &Request,
            cpu: GridAddress,
            key: &ae2_cell_core::ItemKey,
            fluid: bool,
        ) -> bool {
            match r.owner {
                RequestOwner::Interface(owner) => {
                    self.ae2_interface_craft_link_valid(owner, r.slot, cpu, key, fluid)
                }
                RequestOwner::Export(owner) => {
                    self.ae2_export_craft_link_valid(owner, r.slot, cpu, key, fluid)
                }
            }
        }
        fn ae2_request_unlink(&mut self, r: &Request, cpu: GridAddress) -> Result<(), String> {
            match r.owner {
                RequestOwner::Interface(owner) => {
                    if self
                        .ae2_interface_craft_links(owner)
                        .ok()
                        .and_then(|links| links.get(r.slot).copied().flatten())
                        == Some(cpu)
                    {
                        self.ae2_interface_set_craft_link(owner, r.slot, Some(cpu), None)?;
                    }
                }
                RequestOwner::Export(owner) => {
                    if self
                        .ae2_export_craft_links(owner)
                        .ok()
                        .and_then(|links| links.get(r.slot).copied().flatten())
                        == Some(cpu)
                    {
                        self.ae2_export_set_craft_link(owner, r.slot, Some(cpu), None)?;
                    }
                }
            }
            Ok(())
        }
        fn ae2_request_deliver_item(
            &mut self,
            r: &Request,
            cpu: GridAddress,
            item: &ItemStack,
            plan: &mut super::super::ae2_buses::FormationPlan,
        ) -> Result<u32, String> {
            match r.owner {
                RequestOwner::Interface(owner) => {
                    self.ae2_interface_deliver_item(owner, r.slot, cpu, item, false)
                }
                RequestOwner::Export(owner) => {
                    self.ae2_export_deliver_item(owner, r.slot, cpu, item, false, plan)
                }
            }
        }
        fn ae2_request_deliver_fluid(
            &mut self,
            r: &Request,
            cpu: GridAddress,
            key: &ae2_cell_core::ItemKey,
            amount: u64,
            plan: &mut super::super::ae2_buses::FormationPlan,
        ) -> Result<u64, String> {
            match r.owner {
                RequestOwner::Interface(owner) => {
                    self.ae2_interface_deliver_fluid(owner, r.slot, cpu, key, amount, false)
                }
                RequestOwner::Export(owner) => {
                    self.ae2_export_deliver_fluid(owner, r.slot, cpu, key, amount, false, plan)
                }
            }
        }
        fn ae2_machine_request_generation(&self, owner: RequestOwner, slot: usize, key: &ae2_cell_core::ItemKey, fluid: bool) -> Option<i64> {
            match owner {
                RequestOwner::Interface(owner) => self.ae2_interface_pending_craft_generation(owner, slot, key, fluid),
                RequestOwner::Export(owner) => self.ae2_export_pending_craft_generation(owner, slot, key),
            }
        }
        /// Cancel stale futures on owner replacement, card removal or a changed
        /// ghost request before dividing this level's next calculation budget.
        pub(in crate::game) fn ae2_prune_machine_craft_calculations(&mut self) {
            if self.ae2_pending_machine_crafts.is_empty() { return; }
            let network = self.ae2_network();
            let pending = std::mem::take(&mut self.ae2_pending_machine_crafts);
            self.ae2_pending_machine_crafts = pending.into_iter().filter(|work| {
                work.owner.identity(&network) == Some(work.identity)
                    && self.ae2_machine_request_generation(work.owner, work.slot, &work.key, work.fluid) == Some(work.generation)
            }).collect();
        }
        pub(in crate::game) fn ae2_active_machine_craft_calculations(&self) -> usize {
            self.ae2_pending_machine_crafts.iter().filter(|work| work.calculation.is_some()
                && self.ae2_address_parts(work.owner.anchor()).is_some_and(|(dim, _)| dim == self.dim)).count()
        }
        pub(in crate::game) fn ae2_tick_machine_craft_calculations(&mut self) {
            if self.ae2_active_machine_craft_calculations() == 0 { return; }
            let allowance = self.ae2_craft_tick_allowance();
            let dim = self.dim;
            for work in &mut self.ae2_pending_machine_crafts {
                if !work.owner.anchor().local_parts().is_some_and(|(level, _)| level == dim) { continue; }
                let Some(calculation) = work.calculation.as_mut() else { continue; };
                if !calculation.run_slice(allowance) {
                    work.result = Some(calculation.take_result().unwrap_or(Err(ae2_crafting_core::Error::WorkerPanicked)));
                    work.calculation = None;
                }
            }
        }
        /// First call starts the owned CRAFT_LESS future; a later requester
        /// tick polls it and commits only after identity and stock rechecks.
        fn ae2_prepare_machine_craft(
            &mut self, owner: RequestOwner, slot: usize, key: &ae2_cell_core::ItemKey,
            fluid: bool, amount: u64, pos: Pos,
        ) -> Result<(ExtendedRegistry, ae2_crafting_core::extended::Plan), String> {
            if amount == 0 { return Err("craft amount must be positive".into()); }
            let identity = owner.identity(&self.ae2_network()).ok_or("crafting requester missing")?;
            let generation = self.ae2_machine_request_generation(owner, slot, key, fluid).ok_or("crafting requester changed")?;
            if let Some(index) = self.ae2_pending_machine_crafts.iter().position(|work| work.owner == owner && work.slot == slot) {
                let work = &self.ae2_pending_machine_crafts[index];
                if work.identity != identity || work.generation != generation || work.key != *key || work.fluid != fluid {
                    self.ae2_pending_machine_crafts.remove(index);
                } else {
                    if work.calculation.is_some() { return Err("craft calculation pending".into()); }
                    let work = self.ae2_pending_machine_crafts.remove(index);
                    let (plan, _) = work.result.ok_or("craft calculation missing result")?.map_err(|error| format!("craft planning: {error:?}"))?;
                    if plan.simulated || (plan.steps.is_empty() && plan.emitted.is_empty()) { return Err("no feasible craft".into()); }
                    return Ok((work.registry, plan));
                }
            }
            let (registry, inventory, target) = self.ae2_craft_calculation_snapshot(pos, key, fluid, amount)?;
            let calculation = ae2_crafting_core::calculation::Calculation::with_strategy(
                registry.clone(), inventory, target, amount, ae2_crafting_core::calculation::Strategy::CraftLess,
            );
            self.ae2_pending_machine_crafts.push(PendingMachineCraft {
                owner, slot, key: key.clone(), fluid, identity, generation, registry,
                calculation: Some(calculation), result: None,
            });
            Err("craft calculation pending".into())
        }
        pub(in crate::game) fn ae2_begin_interface_craft(
            &mut self,
            owner: super::super::ae2_interface::InterfaceCraftOwner,
            slot: usize,
            key: &ae2_cell_core::ItemKey,
            fluid: bool,
            amount: u64,
        ) -> Result<GridAddress, String> {
            if slot >= 9 || self.ae2_interface_craft_links(owner)?[slot].is_some() {
                return Err("interface crafting slot already linked or invalid".into());
            }
            let (dim, pos) = self
                .ae2_address_parts(owner.anchor())
                .ok_or("invalid interface address")?;
            if dim != self.dim {
                return Err("interface is not in the active level".into());
            }
            let target = if fluid {
                fluid_name(key)
            } else {
                canon(&ae2_lab::prototype(key)?)
            };
            let (registry, plan) = self.ae2_prepare_machine_craft(RequestOwner::Interface(owner), slot, key, fluid, amount, pos)?;
            let low = plan.amount;
            let before = snapshot(self);
            let result = (|| {
                let cpu = self.ae2_begin_recursive_target_on(pos, target.clone(), low, None, true, None, Some((&registry, &plan)))?;
                self.ae2_interface_set_craft_link(owner, slot, None, Some(cpu))?;
                if !self.ae2_interface_craft_link_valid(owner, slot, cpu, key, fluid) {
                    return Err("interface crafting configuration changed".into());
                }
                let cpu_dim = self.ae2_address_parts(cpu).ok_or("invalid CPU address")?.0;
                let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity(cpu) else {
                    return Err("CPU disappeared".into());
                };
                let mut session = read_session(machine, cpu_dim)?;
                let physical = read_escrow(machine)?;
                session.requester = Some(InterfaceRequester {
                    owner,
                    slot,
                    target,
                    amount: low,
                });
                let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity_mut(cpu) else {
                    return Err("CPU disappeared".into());
                };
                write_session(machine, &session, &physical)?;
                self.ae2_mark_modified(cpu);
                Ok(cpu)
            })();
            if result.is_err() {
                restore(self, before);
            }
            result
        }
        pub(in crate::game) fn ae2_begin_export_craft(
            &mut self,
            owner: super::super::ae2_buses::ExportBusCraftOwner,
            slot: usize,
            key: &ae2_cell_core::ItemKey,
            fluid: bool,
            amount: u64,
        ) -> Result<GridAddress, String> {
            if self
                .ae2_export_craft_links(owner)?
                .get(slot)
                .is_none_or(Option::is_some)
            {
                return Err("export crafting slot already linked or invalid".into());
            }
            let (dim, pos) = self
                .ae2_address_parts(owner.anchor())
                .ok_or("invalid export bus address")?;
            if dim != self.dim {
                return Err("export bus is not in the active level".into());
            }
            let target = if fluid {
                fluid_name(key)
            } else {
                canon(&ae2_lab::prototype(key)?)
            };
            let (registry, plan) = self.ae2_prepare_machine_craft(RequestOwner::Export(owner), slot, key, fluid, amount, pos)?;
            let amount = plan.amount;
            let before = snapshot(self);
            let result = (|| {
                let cpu = self.ae2_begin_recursive_target_on(pos, target.clone(), amount, None, true, None, Some((&registry, &plan)))?;
                self.ae2_export_set_craft_link(owner, slot, None, Some(cpu))?;
                if !self.ae2_export_craft_link_valid(owner, slot, cpu, key, fluid) {
                    return Err("export bus configuration changed".into());
                }
                let dim = self.ae2_address_parts(cpu).ok_or("invalid CPU address")?.0;
                let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity(cpu) else {
                    return Err("CPU disappeared".into());
                };
                let mut session = read_session(machine, dim)?;
                let physical = read_escrow(machine)?;
                session.export_requester = Some(ExportRequester {
                    owner,
                    slot,
                    target,
                    amount,
                });
                let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity_mut(cpu) else {
                    return Err("CPU disappeared".into());
                };
                write_session(machine, &session, &physical)?;
                self.ae2_mark_modified(cpu);
                Ok(cpu)
            })();
            if result.is_err() {
                restore(self, before);
            }
            result
        }
        fn ae2_begin_recursive_target(
            &mut self,
            pos: Pos,
            target: String,
            amount: u64,
        ) -> Result<GridAddress, String> {
            let owner=(!self.serving() && self.remote.is_none()).then(||self.ae2_local_player_uuid());
            self.ae2_begin_recursive_target_on(pos, target, amount, None, false, owner, None)
        }
        /// Submit for the actual player whose terminal requested the job.
        pub(in crate::game) fn ae2_begin_player_craft(&mut self,pos:Pos,key:&ae2_cell_core::ItemKey,fluid:bool,amount:u64,cpu:Option<GridAddress>,owner:u128,registry:&ExtendedRegistry,plan:&ae2_crafting_core::extended::Plan)->Result<GridAddress,String> {
            let target=if fluid {fluid_name(key)} else {canon(&ae2_lab::prototype(key)?)};
            self.ae2_begin_recursive_target_on(pos,target,amount,cpu,false,Some(owner),Some((registry,plan)))
        }
        fn ae2_begin_recursive_target_on(
            &mut self,
            pos: Pos,
            target: String,
            amount: u64,
            requested_cpu: Option<GridAddress>,
            machine_request: bool,
            player_owner: Option<u128>,
            prepared: Option<(&ExtendedRegistry,&ae2_crafting_core::extended::Plan)>,
        ) -> Result<GridAddress, String> {
            if amount == 0 {
                return Err("craft amount must be positive".into());
            }
            let (registry, specs) = self.recursive_patterns(pos)?;
            let mut available = Inventory::new();
            for (k, n) in self.ae2_grid_inventory(pos)? {
                let s = ae2_lab::prototype(&k)?;
                let e = available.entry(canon(&s)).or_default();
                *e = e.checked_add(n).ok_or("inventory overflow")?;
            }
            for (k, n) in self.ae2_grid_fluids(pos)? {
                available.insert(fluid_name(&k), n);
            }
            // AE2 ignores the target already in the network when crafting.
            available.remove(&target);
            let plan=if let Some((previous,plan))=prepared {
                if plan.simulated || plan.target!=target || plan.amount!=amount || !previous.same_definitions(&registry) {
                    return Err("crafting patterns changed; recalculate the request".into());
                }
                plan.clone()
            } else {
                registry.plan(&available,&target,amount).map_err(|e|format!("craft planning: {e:?}"))?
            };
            if plan.steps.is_empty() && plan.emitted.is_empty() {
                return Err("no craft steps".into());
            }
            let n = self.ae2_network();
            let analysis = n.analyze().map_err(str::to_owned)?;
            let node = self
                .ae2_local_address(pos)
                .ok_or("invalid local grid position")?
                .network_pos();
            let c = analysis
                .nodes
                .get(&node)
                .filter(|s| s.online)
                .ok_or("network offline")?
                .component;
            let mut roots = Vec::new();
            for p in &analysis.components[c].positions {
                let Some(at) = GridAddress::from_network(*p) else {
                    continue;
                };
                if !analysis.nodes.get(p).is_some_and(|s| s.online) {
                    continue;
                }
                if let Some(BlockEntity::Ae2(m)) = self.ae2_block_entity(at) {
                    if kind(m.kind) {
                        roots.push(at);
                    }
                }
            }
            roots.sort_by_key(|root| {
                let mode=self.ae2_cpu_selection_mode(*root);
                let preferred=selection_policy(mode,machine_request)==Some(true);
                (!preferred,*root)
            });
            let mut selected = None;
            if super::super::ae2_event_live::cpu_formation_enabled() {
                let mut candidates=self.ae2_observed_cpu_candidates(pos,plan.estimated_bytes)?;
                candidates.sort_by_key(|(root,_,_)| {
                    let preferred=selection_policy(self.ae2_cpu_selection_mode(*root),machine_request)==Some(true);
                    (!preferred,*root)
                });
                for (root,capacity,coprocessors) in candidates {
                    if requested_cpu.is_some_and(|wanted|wanted!=root) {continue;}
                    if requested_cpu.is_none() && selection_policy(self.ae2_cpu_selection_mode(root),machine_request).is_none() {continue;}
                    selected=Some((root,capacity,coprocessors));break;
                }
            } else { for root in roots {
                if requested_cpu.is_some_and(|wanted| wanted != root) {
                    continue;
                }
                let Some((dim, local)) = self.ae2_address_parts(root) else {
                    continue;
                };
                let Some(world) = self.ae2_world_for(dim) else {
                    continue;
                };
                if let Ok((members, capacity, coprocessors)) =
                    cluster_checked(world, local, false, true)
                {
                    if self.ae2_dimension_address(dim,members[0])!=Some(root) {continue;}
                    if requested_cpu.is_none() {
                        if selection_policy(self.ae2_cpu_selection_mode(root),machine_request).is_none() {continue;}
                    }
                    if plan.estimated_bytes <= capacity {
                        selected = Some((root, capacity, coprocessors));
                        break;
                    }
                }
            }}
            let (cpu_pos, capacity, coprocessors) =
                selected.ok_or("no idle formed CPU with sufficient storage")?;
            let mut cpu = Cpu::new(capacity, coprocessors);
            cpu.submit(plan.clone(), &mut available)
                .map_err(|e| format!("CPU submit: {e:?}"))?;
            let mut requested = Vec::new();
            let mut requested_fluids = Vec::new();
            for (name, amount) in &plan.reserved {
                if let Some(key) = parse_fluid(name) {
                    requested_fluids.push((key, *amount));
                } else {
                    let stack = from_key(name)?;
                    requested.push((ae2_lab::key(&stack)?, *amount));
                }
            }
            let backup = snapshot(self);
            let mut formation = super::super::ae2_buses::FormationPlan::default();
            let mut physical = Vec::new();
            let mut fluids = BTreeMap::new();
            for (key, amount) in requested_fluids {
                match self.ae2_grid_fluid_extract(pos, &key, amount, false) {
                    Ok(n) if n == amount => {
                        fluids.insert(key, n);
                    }
                    _ => {
                        restore(self, backup);
                        return Err("unable to reserve fluid; restored world".into());
                    }
                }
            }
            for (key, amount) in requested {
                let mut exemplar=match ae2_lab::prototype(&key) {Ok(item)=>item,Err(error)=>{restore(self,backup);return Err(error);}};
                let chunk_max=i32::MAX as u64;
                let chunks=amount/chunk_max+u64::from(amount%chunk_max!=0);
                let Ok(chunks)=usize::try_from(chunks) else {restore(self,backup);return Err("CPU escrow allocation overflow".into());};
                if physical.try_reserve(chunks).is_err(){restore(self,backup);return Err("CPU escrow allocation failed".into());}
                if self.ae2_grid_extract_bulk_staged(pos,&key,amount,&mut formation).ok()!=Some(amount) {
                    restore(self,backup);
                    return Err("not enough items or energy while reserving; restored world".into());
                }
                let mut left=amount;
                while left>0 {
                    let count=left.min(chunk_max);exemplar.count=count as u32;physical.push(exemplar.clone());left-=count;
                }
            }
            let session = Session {
                cpu,
                suspended: false,
                specs,
                flights: BTreeMap::new(),
                job_id: Some([self.world.random.next_long(), self.world.random.next_long()]),
                address_version: 1,
                requester: None,
                export_requester: None,
                player_owner: player_owner.map(super::super::ae2_job_events::owner_words),
            };
            let started=job_status(&session,horizon_client::ae2_job_status::Status::Started);
            if let Some(BlockEntity::Ae2(m)) = self.ae2_block_entity_mut(cpu_pos) {
                if let Err(e) =
                    write_fluids(m, &fluids).and_then(|_| write_session(m, &session, &physical))
                {
                    restore(self, backup);
                    return Err(e);
                }
            } else {
                restore(self, backup);
                return Err("CPU disappeared".into());
            }
            if let Err(e) = self.ae2_commit_formation(formation) {
                restore(self, backup);
                return Err(e);
            }
            self.ae2_mark_modified(cpu_pos);
            if let Some((owner,event))=started {self.ae2_emit_job_status(owner,event);}
            Ok(cpu_pos)
        }
    }
    fn take_physical(items: &mut Vec<ItemStack>, key: &str, count: u64) -> Option<ItemStack> {
        if count != 1 {
            return None;
        }
        let index = items.iter().position(|s| s.count > 0 && canon(s) == key)?;
        let result = ItemStack {
            count: 1,
            ..items[index].clone()
        };
        items[index].count -= 1;
        if items[index].count == 0 {
            items.remove(index);
        }
        Some(result)
    }
    // Processing patterns can request hundreds of thousands of the same
    // item. Drain physical escrow by stack, not by single item, and perform
    // the availability/allocation checks before changing the original list.
    fn take_physical_many(items:&mut Vec<ItemStack>,key:&str,count:u64)->Result<Option<Vec<ItemStack>>,String> {
        if count==0 {return Ok(Some(Vec::new()));}
        let mut available=0u64;
        let mut prototype=None;
        for item in items.iter() {
            if item.count>0 && canon(item)==key {
                if prototype.is_none(){prototype=Some(item.clone());}
                available=available.saturating_add(u64::from(item.count));
                if available>=count {break;}
            }
        }
        if available<count {return Ok(None);}
        let prototype=prototype.ok_or("missing physical escrow prototype")?;
        let max=u64::from(u32::MAX);
        let chunks=count/max+u64::from(count%max!=0);
        let chunks=usize::try_from(chunks).map_err(|_|"physical escrow allocation overflow")?;
        let mut taken=Vec::new();
        taken.try_reserve_exact(chunks).map_err(|_|"physical escrow allocation failed")?;
        let mut left=count;
        while left>0 {
            let amount=left.min(max);let mut stack=prototype.clone();stack.count=amount as u32;
            taken.push(stack);left-=amount;
        }
        let mut left=count;
        for item in items.iter_mut() {
            if left==0 {break;}
            if item.count>0 && canon(item)==key {
                let amount=left.min(u64::from(item.count));item.count-=amount as u32;left-=amount;
            }
        }
        debug_assert_eq!(left,0);items.retain(|item|item.count>0);Ok(Some(taken))
    }
    /// AEProcessingPattern expands condensed inputs in sparse-slot order
    /// before handing them to the target. The legacy format did not store
    /// fluid slot positions, so it retains the legacy item-then-fluid order.
    fn processing_input_order(pattern:&ItemStack)->Result<Vec<(String,u64)>,String> {
        let (items,_)=ae2_processing::ae2_decode_processing_pattern(pattern).ok_or("invalid processing pattern")?;
        let (fluids,_)=ae2_processing::ae2_decode_processing_fluids(pattern).ok_or("invalid processing fluids")?;
        let mut ordered=Vec::new();
        match ae2_processing::ae2_decode_processing_positions(pattern)? {
            Some((positioned,_))=>{
                if items.len()!=positioned.len(){return Err("processing position count mismatch".into());}
                for (item,fluid) in items.iter().zip(positioned) {
                    match (item,fluid) {
                        (Some(item),None)=>ordered.push((canon(item),u64::from(item.count))),
                        (None,Some(fluid))=>ordered.push((fluid_name(&fluid.key),fluid.amount)),
                        (None,None)=>{},
                        _=>return Err("overlapping processing position".into()),
                    }
                }
            }
            None=>{
                ordered.extend(items.iter().flatten().map(|item|(canon(item),u64::from(item.count))));
                ordered.extend(fluids.iter().map(|fluid|(fluid_name(&fluid.key),fluid.amount)));
            }
        }
        Ok(ordered)
    }
    fn checked_processing_input_order(pattern:&ItemStack,inputs:&Inventory)->Result<Vec<(String,u64)>,String> {
        let ordered=processing_input_order(pattern)?;
        let mut condensed=Inventory::new();
        for (key,amount) in &ordered {
            let value=condensed.entry(key.clone()).or_default();
            *value=value.checked_add(*amount).ok_or("processing input overflow")?;
        }
        if &condensed!=inputs {return Err("processing inputs changed before dispatch".into());}
        Ok(ordered)
    }
    fn pattern_grid(spec: &Spec) -> Result<Vec<Option<ItemStack>>, String> {
        if spec.grid.len() != 9 {
            return Err("invalid pattern grid".into());
        }
        spec.grid
            .iter()
            .map(|v| item_data::parse_stack(v))
            .collect()
    }
    fn choose_grid(
        spec: &Spec,
        step: &ae2_crafting_core::extended::Step,
        recipes: &horizon_client::crafting::Recipes,
        stations: &horizon_client::stations::StationData,
    ) -> Option<(Vec<Option<ItemStack>>, BTreeMap<usize, (String, u64)>)> {
        let template = pattern_grid(spec).ok()?;
        let choices: Vec<Vec<String>> = if spec.choices.len() == 9 {
            spec.choices.clone()
        } else {
            template
                .iter()
                .enumerate()
                .map(|(i,s)| spec.fluid_slots.get(&i).map(|(name,_)|vec![name.clone()])
                    .unwrap_or_else(||s.as_ref().map(|s| vec![canon(s)]).unwrap_or_default()))
                .collect()
        };
        fn assign(
            i: usize,
            choices: &[Vec<String>],
            template: &[Option<ItemStack>],
            remaining: &mut Inventory,
            grid: &mut [Option<ItemStack>],
            active_fluids: &mut BTreeMap<usize, (String, u64)>,
            spec: &Spec,
            step: &ae2_crafting_core::extended::Step,
            recipes: &horizon_client::crafting::Recipes,
            stations: &horizon_client::stations::StationData,
        ) -> bool {
            if i == 9 {
                if remaining.values().any(|n| *n > 0) {
                    return false;
                }
                if let Some((kind,id))=&spec.special {
                    let logical=if spec.special_grid_version==0 {grid.to_vec()}else{let Some(grid)=ae2_processing::special_logical_grid(kind,grid) else{return false};grid};
                    let Some(result)=ae2_processing::special_recipe_result(stations,kind,id,&logical) else{return false;};
                    let Ok(Some(expected))=item_data::parse_stack(&spec.output) else{return false;};
                    let mut output=Inventory::new();return result==expected && output_add(&mut output,&result).is_ok() && output==step.outputs;
                }
                let (name, result) = if let Some(name) = spec.crafting_recipe.as_deref() {
                    let Some(result) = recipes.assemble_named(name, grid, 3) else { return false; };
                    (name.strip_prefix("minecraft:").unwrap_or(name), result)
                } else {
                    let Some(result) = recipes.find(grid, 3) else { return false; };
                    result
                };
                let Ok(Some(expected)) = item_data::parse_stack(&spec.output) else {
                    return false;
                };
                if result != expected {
                    return false;
                }
                let mut output = Inventory::new();
                if output_add(&mut output, &result).is_err() {
                    return false;
                }
                for (index, remainder) in recipes.remainders(name, grid, 3).iter().enumerate() {
                    if !active_fluids.contains_key(&index)
                        && remainder
                            .as_ref()
                            .is_some_and(|s| output_add(&mut output, s).is_err())
                    {
                        return false;
                    }
                }
                return output == step.outputs;
            }
            if choices[i].is_empty() {
                return assign(
                    i + 1,
                    choices,
                    template,
                    remaining,
                    grid,
                    active_fluids,
                    spec,
                    step,
                    recipes,
                    stations,
                );
            }
            for key in &choices[i] {
                let Some(n) = remaining.get_mut(key) else {
                    continue;
                };
                let fluid = spec.fluid_slots.get(&i).filter(|(name,_)| name == key);
                let amount = fluid.map_or(1, |(_, amount)| *amount);
                if *n < amount {
                    continue;
                }
                *n -= amount;
                grid[i] = if let Some(fluid) = fluid {
                    active_fluids.insert(i, fluid.clone());
                    template[i].clone()
                } else {
                    from_key(key).ok()
                };
                if grid[i].is_some()
                    && assign(
                        i + 1,
                        choices,
                        template,
                        remaining,
                        grid,
                        active_fluids,
                        spec,
                        step,
                        recipes,
                        stations,
                    )
                {
                    return true;
                }
                *remaining.get_mut(key).unwrap() += amount;
                grid[i] = None;
                active_fluids.remove(&i);
            }
            false
        }
        let mut remaining = step.inputs.clone();
        let mut grid = vec![None; 9];
        let mut active_fluids = BTreeMap::new();
        if assign(
            0,
            &choices,
            &template,
            &mut remaining,
            &mut grid,
            &mut active_fluids,
            spec,
            step,
            recipes,
            stations,
        ) {Some((grid,active_fluids))} else {None}
    }
    impl Game<'_> {
        fn recursive_neighbors(&self, address: GridAddress) -> Vec<GridAddress> {
            let Some((dim, local)) = self.ae2_address_parts(address) else {
                return Vec::new();
            };
            neighbors(local)
                .filter_map(|p| self.ae2_dimension_address(dim, p))
                .collect()
        }
        fn provider_machine(&self, owner: ProviderOwner) -> Option<Machine> {
            match owner {
                ProviderOwner::Block(address) => match self.ae2_block_entity(address) {
                    Some(BlockEntity::Ae2(m)) if m.kind == MachineKind::PatternProvider => {
                        Some((**m).clone())
                    }
                    _ => None,
                },
                ProviderOwner::Part { host, face } => self
                    .ae2_part_machine(super::super::ae2_parts::PartAddress { host, face })
                    .ok()
                    .flatten()
                    .filter(|m| m.kind == MachineKind::PatternProvider),
            }
        }
        fn store_provider(
            &mut self,
            owner: ProviderOwner,
            machine: &Machine,
        ) -> Result<(), String> {
            match owner {
                ProviderOwner::Block(address) => {
                    let Some(BlockEntity::Ae2(m)) = self.ae2_block_entity_mut(address) else {
                        return Err("missing provider".into());
                    };
                    **m = machine.clone();
                    self.ae2_mark_modified(address);
                    Ok(())
                }
                ProviderOwner::Part { host, face } => self.ae2_store_part_machine(
                    super::super::ae2_parts::PartAddress { host, face },
                    machine,
                ),
            }
        }
        fn provider_online(&self, owner: ProviderOwner) -> bool {
            match owner {
                ProviderOwner::Block(address) => self.ae2_online_encoded(address),
                ProviderOwner::Part { host, face } => self
                    .ae2_network()
                    .analyze()
                    .ok()
                    .and_then(|a| {
                        a.nodes
                            .get(&host.network_pos())
                            .and_then(|s| s.parts.get(&face))
                            .map(|s| s.online)
                    })
                    .unwrap_or(false),
            }
        }
        fn provider_neighbors(&self, owner: ProviderOwner) -> Vec<GridAddress> {
            let all = self.recursive_neighbors(owner.anchor());
            if let ProviderOwner::Block(host)=owner {
                let Some(state)=self.ae2_grid_block(host).and_then(|id|self.pack.assets.registry.state(id)) else{return Vec::new()};
                let Ok(direction)=super::provider_push_direction(state) else{return Vec::new()};
                let Some(direction)=direction else{return all};
                let Some((_,p))=self.ae2_address_parts(host) else{return Vec::new()};let d=direction.normal();
                return all.into_iter().filter(|target|self.ae2_address_parts(*target).is_some_and(|(_,q)|q==(p.0+d[0],p.1+d[1],p.2+d[2]))).collect();
            }
            let ProviderOwner::Part { host, face } = owner else {return Vec::new()};
            let Some((_, p)) = self.ae2_address_parts(host) else {
                return Vec::new();
            };
            let d = match face {
                0 => (1, 0, 0),
                1 => (-1, 0, 0),
                2 => (0, 1, 0),
                3 => (0, -1, 0),
                4 => (0, 0, 1),
                5 => (0, 0, -1),
                _ => return Vec::new(),
            };
            all.into_iter()
                .filter(|a| {
                    self.ae2_address_parts(*a)
                        .is_some_and(|(_, q)| q == (p.0 + d.0, p.1 + d.1, p.2 + d.2))
                })
                .collect()
        }
        fn provider_target_face(
            &self,
            from: GridAddress,
            to: GridAddress,
        ) -> Option<horizon_core::geometry::Direction> {
            use horizon_core::geometry::Direction::*;
            let (a, p) = self.ae2_address_parts(from)?;
            let (b, q) = self.ae2_address_parts(to)?;
            if a != b {
                return None;
            }
            match (q.0 - p.0, q.1 - p.1, q.2 - p.2) {
                (1, 0, 0) => Some(West),
                (-1, 0, 0) => Some(East),
                (0, 1, 0) => Some(Down),
                (0, -1, 0) => Some(Up),
                (0, 0, 1) => Some(North),
                (0, 0, -1) => Some(South),
                _ => None,
            }
        }
        fn provider_arm_lock(
            &mut self,
            provider: ProviderOwner,
            output: &str,
        ) -> Result<(), String> {
            let expected = item_data::parse_stack(output)
                .ok()
                .flatten()
                .map(|s| canon(&s))
                .unwrap_or_else(|| output.to_owned());
            {
                let mut m = self
                    .provider_machine(provider)
                    .ok_or("provider disappeared")?;
                match m.config.get(&"ae2_lock_mode".into()) {
                    Some(Tag::Int(1)) => {
                        let high = m.config.get(&"ae2_redstone_high".into()) == Some(&Tag::Byte(1));
                        m.config.insert(
                            "ae2_unlock_event".into(),
                            Tag::Int(if high { 2 } else { 1 }),
                        );
                    }
                    Some(Tag::Int(4)) => {
                        m.config.insert("ae2_unlock_event".into(), Tag::Int(3));
                        m.config.insert(
                            "ae2_unlock_stack".into(),
                            Tag::String(expected.as_str().into()),
                        );
                    }
                    _ => {}
                }
                self.store_provider(provider, &m)?;
            }
            Ok(())
        }
        fn provider_receive(&mut self, owner: ProviderOwner, key: &str, accepted: u64) {
            if accepted == 0 {
                return;
            }
            if let Some(mut machine) = self.provider_machine(owner) {
                if machine.config.get(&"ae2_unlock_event".into()) == Some(&Tag::Int(3))
                    && matches!(machine.config.get(&"ae2_unlock_stack".into()),Some(Tag::String(s)) if s.to_string_lossy()==key)
                {
                    machine.config.remove(&"ae2_unlock_event".into());
                    machine.config.remove(&"ae2_unlock_stack".into());
                    let _ = self.store_provider(owner, &machine);
                }
            }
        }
        pub(in crate::game) fn ae2_provider_receive_item(
            &mut self,
            provider: GridAddress,
            item: &ItemStack,
            accepted: u32,
        ) {
            self.provider_receive(
                ProviderOwner::Block(provider),
                &canon(item),
                accepted as u64,
            );
        }
        pub(in crate::game) fn ae2_provider_receive_fluid(
            &mut self,
            provider: GridAddress,
            fluid: &ae2_cell_core::ItemKey,
            accepted: u64,
        ) {
            self.provider_receive(ProviderOwner::Block(provider), &fluid_name(fluid), accepted);
        }
        fn provider_send_pending(machine:&Machine)->bool {
            [horizon_client::ae2_provider_queue::QUEUE,"ae2_provider_send_items","ae2_provider_send_fluids"].iter().any(|key|match machine.config.get(&(*key).into()) {None=>false,Some(Tag::List(v))=>!v.is_empty(),Some(_)=>true})
        }
        fn provider_send_direction(face:horizon_core::geometry::Direction)->i8 {
            use horizon_core::geometry::Direction::*;match face {East=>0,West=>1,Up=>2,Down=>3,South=>4,North=>5}
        }
        fn tick_provider_send(&mut self,owner:ProviderOwner)->Result<(),String> {
            let before=snapshot(self);
            let mut plan=super::super::ae2_buses::FormationPlan::default();
            let result=self.tick_provider_send_staged(owner,&mut plan)
                .and_then(|()|self.ae2_commit_formation(plan));
            if result.is_err(){restore(self,before);}result
        }
        fn tick_provider_send_staged(&mut self,owner:ProviderOwner,plan:&mut super::super::ae2_buses::FormationPlan)->Result<(),String> {
            let Some(machine)=self.provider_machine(owner) else{return Ok(());};
            if !Self::provider_send_pending(&machine) {return Ok(());}
            let face=match machine.config.get(&"ae2_provider_send_face".into()) {
                Some(Tag::Byte(0))=>horizon_core::geometry::Direction::East,Some(Tag::Byte(1))=>horizon_core::geometry::Direction::West,
                Some(Tag::Byte(2))=>horizon_core::geometry::Direction::Up,Some(Tag::Byte(3))=>horizon_core::geometry::Direction::Down,
                Some(Tag::Byte(4))=>horizon_core::geometry::Direction::South,Some(Tag::Byte(5))=>horizon_core::geometry::Direction::North,
                _=>return Err("invalid provider send direction".into()),
            };
            let Some(target)=self.recursive_neighbors(owner.anchor()).into_iter().find(|at|self.provider_target_face(owner.anchor(),*at)==Some(face)) else{return Ok(());};
            let entries=horizon_client::ae2_provider_queue::read(&machine.config)?;
            let before=snapshot(self);
            let previous=plan.len();
            let result=(|| {
                use horizon_client::ae2_provider_queue::Entry;
                let mut remaining=Vec::new();
                remaining.try_reserve(entries.len()).map_err(|_|"provider send allocation failed")?;
                for entry in entries {
                    match entry {
                        Entry::Item{prototype,amount}=>{
                            let count=amount.min(u64::from(u32::MAX)) as u32;
                            let item=ItemStack{count,..prototype.clone()};
                            let moved=match self.ae2_item_p2p_insert_staged(target,face,&item,false,plan) {
                                Some(n)=>n,None=>self.ae2_native_item_insert_staged(target,face,&item,item.count,false,plan),
                            };
                            if moved>count{return Err("provider target overaccepted item".into());}
                            if u64::from(moved)<amount {remaining.push(Entry::Item{prototype,amount:amount-u64::from(moved)});}
                        }
                        Entry::Fluid{key,amount}=>{
                            let moved=self.provider_target_fluid_insert(target,face,&key,amount,false,plan);
                            if moved>amount{return Err("provider target overaccepted fluid".into());}
                            if moved<amount {remaining.push(Entry::Fluid{key,amount:amount-moved});}
                        }
                    }
                }
                let mut updated=self.provider_machine(owner).ok_or("provider disappeared")?;
                horizon_client::ae2_provider_queue::write(&mut updated.config,&remaining)?;
                if !Self::provider_send_pending(&updated){updated.config.remove(&"ae2_provider_send_face".into());}
                self.store_provider(owner,&updated)?;Ok(())
            })();
            if result.is_err(){restore(self,before);plan.truncate(previous);}result
        }
        fn provider_target_fluid_insert(&mut self,target:GridAddress,face:horizon_core::geometry::Direction,key:&ae2_cell_core::ItemKey,amount:u64,simulate:bool,plan:&mut super::super::ae2_buses::FormationPlan)->u64 {
            if matches!(self.ae2_block_entity(target),Some(BlockEntity::Ae2(machine)) if machine.kind==MachineKind::Condenser) {
                return self.ae2_condenser_me_fluid_insert_at(target,amount,simulate);
            }
            if let Some(n)=self.ae2_fluid_p2p_fill_staged(target,face,key,amount,simulate,plan) {return n;}
            if let Some(n)=self.ae2_interface_fluid_move_staged(target,face,key,amount,true,simulate,plan) {return n;}
            if let Some(n)=self.ae2_provider_return_fluid_at(target,face,key,amount,simulate) {return n;}
            self.ae2_tank_fill_encoded(target,key,amount,simulate).unwrap_or(0)
        }
        fn recursive_destination(&mut self, spec: &Spec, excluded:Option<&std::collections::BTreeSet<GridAddress>>,
            effective_patterns:Option<&[String]>,plan:&mut super::super::ae2_buses::FormationPlan) -> Option<GridAddress> {
            let provider = self.provider_machine(spec.provider)?;
            let stored = match spec.definition.as_ref() {
                Some(definition) => {
                    if !effective_patterns?.iter().any(|key|key == definition) {return None;}
                    item_data::parse_stack(definition).ok()??
                }
                None => provider.slots.get(spec.slot)?.clone()?,
            };
            if provider.kind != MachineKind::PatternProvider || !self.provider_online(spec.provider) || Self::provider_send_pending(&provider)
            {
                return None;
            }
            let mode = match provider.config.get(&"ae2_lock_mode".into()) {
                Some(Tag::Int(n)) => *n,
                _ => 0,
            };
            let high = provider.config.get(&"ae2_redstone_high".into()) == Some(&Tag::Byte(1));
            let awaiting = matches!(provider.config.get(&"ae2_unlock_event".into()),Some(Tag::Int(n)) if *n!=0);
            if (mode == 2 && high) || (mode == 3 && !high) || awaiting {
                return None;
            }
            if spec.processing {
                let pattern = &stored;
                let (inputs, outputs) = ae2_processing::ae2_decode_processing_pattern(pattern)?;
                if inputs
                    .iter()
                    .map(|s| item_data::canon_stack(s.as_ref()))
                    .collect::<Vec<_>>()
                    != spec.grid
                {
                    return None;
                }
                let (fluid_inputs, fluid_outputs) =
                    ae2_processing::ae2_decode_processing_fluids(pattern)?;
                if fluid_inputs
                    .iter()
                    .map(|f| (fluid_name(&f.key), f.amount))
                    .collect::<Vec<_>>()
                    != spec.fluid_inputs
                    || fluid_outputs
                        .iter()
                        .map(|f| (fluid_name(&f.key), f.amount))
                        .collect::<Vec<_>>()
                        != spec.fluid_outputs
                {
                    return None;
                }
                let (primary, extra_outputs) = processing_spec_outputs(pattern, &outputs, &fluid_outputs).ok()?;
                if primary != spec.output || extra_outputs != spec.extra_outputs {
                    return None;
                }
                let mut positions: Vec<_> = self.provider_neighbors(spec.provider);
                positions.sort();
                if !positions.is_empty() {
                    let index = match provider.config.get(&"ae2_provider_round_robin".into()) {
                        Some(Tag::Int(n)) => n.rem_euclid(positions.len() as i32) as usize,
                        _ => 0,
                    };
                    positions.rotate_left(index);
                }
                let blocking = provider.config.get(&"ae2_blocking".into()) == Some(&Tag::Byte(1));
                // AE2 blocking mode checks the inputs of every installed
                // pattern, not just the pattern currently being dispatched.
                let mut blocked_items = std::collections::BTreeSet::new();
                let mut blocked_fluids = std::collections::BTreeSet::new();
                if blocking {
                    let cached: Vec<ItemStack> = match effective_patterns {
                        Some(patterns) => patterns.iter().map(|key|item_data::parse_stack(key).ok().flatten()).collect::<Option<Vec<_>>>()?,
                        None => provider.slots.iter().flatten().cloned().collect(),
                    };
                    for stack in &cached {
                        if ae2_processing::ae2_decode_processing_pattern(stack).is_some() {
                            if let Some((fluids,_))=ae2_processing::ae2_decode_processing_fluids(stack) {
                                for fluid in fluids {blocked_fluids.insert(fluid_name(&fluid.key));}
                            }
                        }
                        let grid = ae2_processing::decode_pattern(stack)
                            .map(|(grid, _)| grid)
                            .or_else(|| {
                                ae2_processing::ae2_decode_processing_pattern(stack)
                                    .map(|(grid, _)| grid)
                            })
                            .or_else(||ae2_processing::ae2_decode_special_pattern(stack).map(|pattern|pattern.grid));
                        if let Some(grid) = grid {
                            for item in grid.iter().flatten() {
                                blocked_items.insert(canon(item));
                            }
                        }
                    }
                }
                return positions.iter().copied().find(|p| {
                    if excluded.is_some_and(|entries|entries.contains(p)) {return false;}
                    let Some(face)=self.provider_target_face(spec.provider.anchor(),*p) else {return false;};
                    let face_index=match face {horizon_core::geometry::Direction::East=>0,horizon_core::geometry::Direction::West=>1,horizon_core::geometry::Direction::Up=>2,horizon_core::geometry::Direction::Down=>3,horizon_core::geometry::Direction::South=>4,horizon_core::geometry::Direction::North=>5};
                    let part=super::super::ae2_parts::PartAddress{host:*p,face:face_index};
                    let is_interface_or_provider=matches!(self.ae2_block_entity(*p),Some(BlockEntity::Ae2(m)) if matches!(m.kind,MachineKind::PatternProvider|MachineKind::Interface))
                        || self.ae2_part_machine(part).ok().flatten().is_some_and(|m|matches!(m.kind,MachineKind::PatternProvider|MachineKind::Interface));
                    // Source excludes same-grid providers/interfaces, including face-owned parts.
                    if is_interface_or_provider && self.ae2_network().analyze().ok().is_some_and(|a|a.nodes.get(&p.network_pos()).zip(a.nodes.get(&spec.provider.anchor().network_pos())).is_some_and(|(left,right)|left.component==right.component)) {return false;}
                    let existing=self.ae2_item_p2p_candidates(*p,face).unwrap_or_else(||self.ae2_native_item_candidates(*p,face));
                    if blocking && existing.iter().any(|s|blocked_items.contains(&canon(s))) {return false;}
                    if blocking && !blocked_fluids.is_empty() {
                        let fluids=self.ae2_fluid_p2p_entries(*p,face).or_else(||self.ae2_interface_fluid_entries(*p,face)).unwrap_or_else(||self.ae2_tank_contents_encoded(*p).ok().flatten().into_iter().collect());
                        if fluids.iter().any(|(key,amount)|*amount>0 && blocked_fluids.contains(&fluid_name(key))) {return false;}
                    }
                    inputs.iter().flatten().all(|input|self.ae2_item_p2p_insert(*p,face,input,true).unwrap_or_else(||self.ae2_native_item_insert(*p,face,input,input.count,true))>0)
                        && fluid_inputs.iter().all(|fluid|self.provider_target_fluid_insert(*p,face,&fluid.key,fluid.amount,true,plan)>0)
                });
            }
            let stored=&stored;
            let (grid,output)=if let Some((kind,recipe))=&spec.special {
                let pattern=ae2_processing::ae2_decode_special_pattern(stored)?;
                if &pattern.kind!=kind || &pattern.recipe!=recipe {return None;}
                let output=ae2_processing::special_recipe_result(self.res.stations(),kind,recipe,&pattern.grid)?;
                let grid=if spec.special_grid_version==0 {pattern.grid}else{ae2_processing::special_assembler_grid(kind,&pattern.grid)?};
                (grid,output)
            } else {
                let (grid,encoded)=ae2_processing::decode_pattern(stored)?;
                let output=if let Some(recipe)=spec.crafting_recipe.as_deref(){self.res.recipes().assemble_named(recipe,&grid,3)?}else{encoded};
                (grid,output)
            };
            if grid
                .iter()
                .map(|s| item_data::canon_stack(s.as_ref()))
                .collect::<Vec<_>>()
                != spec.grid
                || item_data::canon_stack(Some(&output)) != spec.output
            {
                return None;
            }
            if !spec.fluid_slots.is_empty()
                && !ae2_processing::ae2_crafting_pattern_options(stored).is_some_and(|options| options.1)
            {
                return None;
            }
            let mut positions: Vec<_> = self.provider_neighbors(spec.provider);
            positions.sort();
            positions.into_iter().find(|p|self.ae2_online_encoded(*p) && matches!(self.ae2_block_entity(*p),Some(BlockEntity::Ae2(m)) if m.kind==MachineKind::MolecularAssembler && m.slots.len()==11 && m.slots.iter().all(Option::is_none) && !m.config.contains_key(&"ae2_assembler_pattern".into()) && !m.config.contains_key(&"ae2_assembler_fluid_escrow".into())))
        }
        /// Capture only actual matching item insertions for in-flight processing
        /// patterns. Call once from the authoritative grid insertion path; the
        /// returned count is removed from the caller's real offered stack.
        pub(in crate::game) fn ae2_crafting_receive_output(
            &mut self,
            pos: Pos,
            input: &ItemStack,
            limit: u64,
        ) -> Result<u64, String> {
            let address = self
                .ae2_local_address(pos)
                .ok_or("invalid local grid position")?;
            self.ae2_crafting_receive_output_encoded(address, input, limit)
        }
        pub(in crate::game) fn ae2_crafting_receive_output_encoded(
            &mut self,
            address: super::super::ae2_global::GridAddress,
            input: &ItemStack,
            limit: u64,
        ) -> Result<u64, String> {
            let mut left = limit.min(u64::from(input.count));
            if left == 0 {
                return Ok(0);
            }
            let network = self.ae2_network();
            let analysis = network.analyze().map_err(str::to_owned)?;
            let point = address.network_pos();
            let c = analysis
                .nodes
                .get(&point)
                .filter(|s| s.online)
                .ok_or("network offline")?
                .component;
            let key = canon(input);
            let cpus:Vec<_>=if super::super::ae2_event_live::cpu_formation_enabled() {
                self.ae2_crafting_waiting_owners(address,&key)?
            } else { analysis.components[c].positions.iter().filter_map(|p| {let at=super::super::ae2_global::GridAddress::from_network(*p)?;matches!(self.ae2_block_entity(at),Some(BlockEntity::Ae2(m)) if m.config.contains_key(&JOB.into())).then_some(at)}).collect() };
            let backup = snapshot(self);
            for cpu_pos in cpus {
                let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity(cpu_pos) else {
                    continue;
                };
                // Legacy jobs are serviced by their original assembler path;
                // they are busy owners, not recursive receipt destinations.
                if !machine.config.contains_key(&JOB.into()) {continue;}
                let mut session = match read_session(
                    machine,
                    self.ae2_address_parts(cpu_pos)
                        .ok_or("invalid CPU address")?
                        .0,
                ) {
                    Ok(s) => s,
                    Err(e) => {
                        restore(self, backup);
                        return Err(e);
                    }
                };
                let mut physical = match read_escrow(machine) {
                    Ok(v) => v,
                    Err(e) => {
                        restore(self, backup);
                        return Err(e);
                    }
                };
                let flights: Vec<_> = session
                    .cpu
                    .flights()
                    .iter()
                    .filter_map(|(id, f)| {
                        session
                            .specs
                            .get(&f.step.pattern)
                            .filter(|s| s.processing)
                            .and_then(|_| f.remaining.get(&key))
                            .map(|n| (*id, *n))
                    })
                    .collect();
                for (id, needed) in flights {
                    let n = left.min(needed);
                    if n == 0 {
                        continue;
                    }
                    if let Err(e) = session.cpu.receive(id, &key, n) {
                        restore(self, backup);
                        return Err(format!("processing output: {e:?}"));
                    }
                    let mut owned = input.clone();
                    owned.count = n as u32;
                    physical.push(owned);
                    left -= n;
                    if !session.cpu.flights().contains_key(&id) {
                        session.flights.remove(&id);
                    }
                    if left == 0 {
                        break;
                    }
                }
                let emitted = left.min(session.cpu.emitted_waiting().get(&key).copied().unwrap_or(0));
                if emitted > 0 {
                    if let Err(e) = session.cpu.receive_emitted(&key, emitted) {
                        restore(self, backup);
                        return Err(format!("emitted item receipt: {e:?}"));
                    }
                    let mut owned=input.clone();owned.count=emitted as u32;
                    physical.push(owned);left-=emitted;
                }
                if let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity_mut(cpu_pos) {
                    if let Err(e) = write_session(machine, &session, &physical) {
                        restore(self, backup);
                        return Err(e);
                    }
                }
                self.ae2_mark_modified(cpu_pos);
                if left == 0 {
                    break;
                }
            }
            Ok(limit.min(u64::from(input.count)) - left)
        }
        /// Capture actual fluid offered by a real network insertion; caller
        /// must debit the originating tank and charge transfer power once.
        pub(in crate::game) fn ae2_crafting_receive_fluid(
            &mut self,
            pos: Pos,
            key: &ae2_cell_core::ItemKey,
            amount: u64,
        ) -> Result<u64, String> {
            let address = self
                .ae2_local_address(pos)
                .ok_or("invalid local grid position")?;
            self.ae2_crafting_receive_fluid_encoded(address, key, amount)
        }
        pub(in crate::game) fn ae2_crafting_receive_fluid_encoded(
            &mut self,
            address: super::super::ae2_global::GridAddress,
            key: &ae2_cell_core::ItemKey,
            amount: u64,
        ) -> Result<u64, String> {
            if amount == 0 {
                return Ok(0);
            }
            let network = self.ae2_network();
            let analysis = network.analyze().map_err(str::to_owned)?;
            let point = address.network_pos();
            let c = analysis
                .nodes
                .get(&point)
                .filter(|s| s.online)
                .ok_or("network offline")?
                .component;
            let name = fluid_name(key);
            let cpus:Vec<_>=if super::super::ae2_event_live::cpu_formation_enabled() {
                self.ae2_crafting_waiting_owners(address,&name)?
            } else { analysis.components[c].positions.iter().filter_map(|p| {let at=super::super::ae2_global::GridAddress::from_network(*p)?;matches!(self.ae2_block_entity(at),Some(BlockEntity::Ae2(m)) if m.config.contains_key(&JOB.into())).then_some(at)}).collect() };
            let backup = snapshot(self);
            let mut left = amount;
            for cpu_pos in cpus {
                let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity(cpu_pos) else {
                    continue;
                };
                if !machine.config.contains_key(&JOB.into()) {continue;}
                let mut session = match read_session(
                    machine,
                    self.ae2_address_parts(cpu_pos)
                        .ok_or("invalid CPU address")?
                        .0,
                ) {
                    Ok(v) => v,
                    Err(e) => {
                        restore(self, backup);
                        return Err(e);
                    }
                };
                let physical = match read_escrow(machine) {
                    Ok(v) => v,
                    Err(e) => {
                        restore(self, backup);
                        return Err(e);
                    }
                };
                let mut fluids = match read_fluids(machine) {
                    Ok(v) => v,
                    Err(e) => {
                        restore(self, backup);
                        return Err(e);
                    }
                };
                let flights: Vec<_> = session
                    .cpu
                    .flights()
                    .iter()
                    .filter_map(|(id, f)| {
                        session
                            .specs
                            .get(&f.step.pattern)
                            .filter(|s| s.processing)
                            .and_then(|_| f.remaining.get(&name))
                            .map(|n| (*id, *n))
                    })
                    .collect();
                for (id, needed) in flights {
                    let n = left.min(needed);
                    if n == 0 {
                        continue;
                    }
                    let existing = fluids.get(key).copied().unwrap_or(0);
                    let Some(total) = existing.checked_add(n) else {
                        restore(self, backup);
                        return Err("fluid escrow overflow".into());
                    };
                    if let Err(e) = session.cpu.receive(id, &name, n) {
                        restore(self, backup);
                        return Err(format!("processing fluid output: {e:?}"));
                    }
                    fluids.insert(key.clone(), total);
                    left -= n;
                    if !session.cpu.flights().contains_key(&id) {
                        session.flights.remove(&id);
                    }
                    if left == 0 {
                        break;
                    }
                }
                let emitted = left.min(session.cpu.emitted_waiting().get(&name).copied().unwrap_or(0));
                if emitted > 0 {
                    let Some(total)=fluids.get(key).copied().unwrap_or(0).checked_add(emitted) else {
                        restore(self, backup);return Err("fluid escrow overflow".into());
                    };
                    if let Err(e)=session.cpu.receive_emitted(&name,emitted) {
                        restore(self, backup);return Err(format!("emitted fluid receipt: {e:?}"));
                    }
                    fluids.insert(key.clone(),total);left-=emitted;
                }
                if let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity_mut(cpu_pos) {
                    if let Err(e) = write_fluids(machine, &fluids)
                        .and_then(|_| write_session(machine, &session, &physical))
                    {
                        restore(self, backup);
                        return Err(e);
                    }
                }
                self.ae2_mark_modified(cpu_pos);
                if left == 0 {
                    break;
                }
            }
            Ok(amount - left)
        }
        pub(in crate::game) fn ae2_recursive_jobs(
            &self,
            pos: Pos,
        ) -> Vec<(GridAddress, usize, usize, bool)> {
            let network = self.ae2_network();
            let Ok(analysis) = network.analyze() else {
                return Vec::new();
            };
            let Some(point) = self.ae2_local_address(pos).map(|a| a.network_pos()) else {
                return Vec::new();
            };
            let Some(node) = analysis.nodes.get(&point) else {
                return Vec::new();
            };
            let mut jobs = Vec::new();
            for p in &analysis.components[node.component].positions {
                let Some(at) = GridAddress::from_network(*p) else {
                    continue;
                };
                let Some((dim, _)) = self.ae2_address_parts(at) else {
                    continue;
                };
                if let Some(BlockEntity::Ae2(m)) = self.ae2_block_entity(at) {
                    if let Ok(s) = read_session(m, dim) {
                        jobs.push((
                            at,
                            s.cpu.pending().len(),
                            s.cpu.flights().len(),
                            s.cpu.is_cancelled(),
                        ));
                    }
                }
            }
            jobs.sort_by_key(|j| j.0);
            jobs
        }
        /// Reduce an actual resident or unload-callback snapshot without
        /// moving its physical inventory. Orphaned escrow stays unavailable.
        pub(in crate::game) fn ae2_cpu_snapshot_waiting(machine:&Machine, dim:super::super::Dim) -> Result<Option<Inventory>,String> {
            if !kind(machine.kind) {return Err("CPU owner changed type".into());}
            if let Some(waiting)=legacy_waiting(machine)? {return Ok(Some(waiting));}
            if machine.config.contains_key(&JOB.into()) {
                let session=read_session(machine,dim)?;
                return Ok(Some(session.cpu.waiting_for().map_err(|e|format!("CPU waiting state: {e:?}"))?));
            }
            if !read_escrow(machine)?.is_empty() || !read_fluids(machine)?.is_empty() {
                return Err("CPU has orphaned physical escrow".into());
            }
            Ok(None)
        }
        /// Read only the real saved job at its existing physical owner.
        pub(in crate::game) fn ae2_cpu_job_waiting(&self, address: GridAddress) -> Result<Option<Inventory>, String> {
            let (dim, _) = self.ae2_address_parts(address).ok_or("invalid CPU address")?;
            let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity(address) else { return Err("CPU owner unavailable".into()); };
            Self::ae2_cpu_snapshot_waiting(machine,dim)
        }
        /// Bind a published cluster to its one physical saved-job owner.
        /// Older native jobs keep their address and requester links intact.
        pub(in crate::game) fn ae2_cpu_waiting_owner(
            &self, members: &[(ae2_network_core::NodeIdentity, ae2_crafting_core::cpu_formation::CpuPos, bool)],
        ) -> Result<Option<(GridAddress, Inventory)>, String> {
            self.ae2_cpu_waiting_owner_for_key(members,None)
        }
        /// An unloaded job cannot receive an item, but a verified snapshot
        /// can prove that a different key is not part of its waiting ledger.
        pub(in crate::game) fn ae2_cpu_waiting_owner_for_key(
            &self, members: &[(ae2_network_core::NodeIdentity, ae2_crafting_core::cpu_formation::CpuPos, bool)],
            key:Option<&str>,
        ) -> Result<Option<(GridAddress, Inventory)>, String> {
            let mut owner = None;
            let mut unloaded_owner = None;
            for (node, pos, resident) in members {
                if node.part.is_some() || node.outer { return Err("CPU cluster member unavailable".into()); }
                let dim=match pos.dimension {0=>super::super::Dim::Overworld,1=>super::super::Dim::Nether,
                    2=>super::super::Dim::End,3=>super::super::Dim::Spatial,_=>return Err("invalid CPU dimension".into())};
                let local=(pos.x,pos.y,pos.z);
                let world=self.ae2_world_for(dim).ok_or("CPU level unavailable")?;
                if !*resident {
                    // A freshly reloaded position must be rebound to its new
                    // incarnation before the old snapshot can be used.
                    if world.loaded(local.0,local.2) {return Err("CPU unload snapshot is stale after reload".into());}
                    if let Some(waiting)=self.ae2_unloaded_cpu_waiting(*node)? {
                        if owner.is_some() || unloaded_owner.is_some() {return Err("multiple jobs in one CPU cluster".into());}
                        unloaded_owner=Some(waiting);
                    }
                    continue;
                }
                if world.block_entities.incarnation(&local)!=Some(node.host) {return Err("CPU member incarnation changed".into());}
                let address=self.ae2_dimension_address(dim,local).ok_or("invalid CPU member address")?;
                if let Some(waiting)=self.ae2_cpu_job_waiting(address)? {
                    if owner.is_some() || unloaded_owner.is_some() {return Err("multiple jobs in one CPU cluster".into());}
                    owner=Some((address,waiting));
                }
            }
            if let Some(waiting)=unloaded_owner {
                if unloaded_waiting_conflicts(&waiting,key) {return Err("CPU waiting owner is unloaded".into());}
            }
            Ok(owner)
        }
        /// Active jobs first, then the other formed CPUs in the same grid.
        pub(in crate::game) fn ae2_terminal_status_cpus(&self,pos:Pos)->Vec<GridAddress> {
            let mut result:Vec<_>=self.ae2_recursive_jobs(pos).into_iter().map(|job|job.0).collect();
            if let Ok(idle)=self.ae2_craft_cpu_candidates(pos,0) {
                for (cpu,_,_) in idle {if !result.contains(&cpu) {result.push(cpu);}}
            }
            result
        }
        pub(in crate::game) fn ae2_assembler_flight_is_live(&self, pos: Pos) -> Option<bool> {
            let assembler = self.ae2_local_address(pos)?;
            let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity(assembler) else {
                return None;
            };
            let Tag::String(raw) = machine.config.get(&"ae2_flight_owner".into())? else {
                return None;
            };
            let owner: AssemblerFlightOwner = serde_json::from_str(&raw.to_string_lossy()).ok()?;
            let (dim, local) = self.ae2_address_parts(owner.cpu)?;
            let world = self.ae2_world_for(dim)?;
            if !world.loaded(local.0, local.2) {
                return None;
            }
            let Some(BlockEntity::Ae2(cpu)) = self.ae2_block_entity(owner.cpu) else {
                return Some(false);
            };
            if !cpu.config.contains_key(&JOB.into()) {
                return Some(false);
            }
            let session = read_session(cpu, dim).ok()?;
            Some(matching_assembler_flight(&owner, &session, assembler))
        }
        /// True only for a formed, storage-bearing physical cluster with a
        /// live job; unformed standalone crafting units are not busy clusters.
        pub(in crate::game) fn ae2_crafting_cluster_busy(&self, pos: Pos) -> bool {
            let Ok((members, _, _)) = cluster_checked(&self.world, pos, true, false) else {
                return false;
            };
            members.iter().any(|p| matches!(self.world.block_entities.get(p), Some(BlockEntity::Ae2(m)) if m.config.contains_key(&JOB.into()) || m.config.contains_key(&"ae2_cpu_job".into())))
        }
        /// CraftingBlockEntity.breakCluster: cancel each physical CPU job,
        /// return its actual local escrow to ME storage, and drop only item
        /// remainder. External/assembler flights retain their own ownership.
        pub(in crate::game) fn ae2_break_crafting_cluster(
            &mut self,
            pos: Pos,
        ) -> Result<(), String> {
            let Ok((members, _, _)) = cluster_checked(&self.world, pos, true, false) else {
                return Ok(());
            };
            self.ae2_break_crafting_members(pos, members)
        }
        /// Run before a real block removal while the old network and its
        /// physical CPU owner still exist. The saved legacy job is never
        /// silently moved or discarded if its custody cannot be reconciled.
        pub(in crate::game) fn ae2_prepare_cpu_removal(&mut self, pos: Pos) -> Result<(),String> {
            if !super::super::ae2_event_live::cpu_formation_enabled() {return Ok(());}
            let Some(BlockEntity::Ae2(machine))=self.world.block_entities.get(&pos) else{return Ok(());};
            if !kind(machine.kind) {return Ok(());}
            self.ae2_reconcile_pending_cpu_breaks()?;
            let Some(cluster)=self.ae2_cpu_before_removal(pos)? else {
                let has_job=matches!(self.world.block_entities.get(&pos),Some(BlockEntity::Ae2(machine))
                    if machine.config.contains_key(&JOB.into()) || machine.config.contains_key(&"ae2_cpu_job".into()));
                return if has_job {Err("CPU job has no observed formed cluster".into())} else {Ok(())};
            };
            let mut members=Vec::new();
            for (node,member,resident) in cluster {
                if !resident || member.dimension!=super::super::ae2_event_live::ordinal(self.dim) {
                    return Err("CPU removal includes an unavailable member".into());
                }
                let local=(member.x,member.y,member.z);
                if self.world.block_entities.incarnation(&local)!=Some(node.host) {
                    return Err("CPU removal member incarnation changed".into());
                }
                members.push(local);
            }
            self.ae2_break_crafting_members(pos,members)
        }
        /// Reconcile only members captured by the actual CPU callback. A
        /// removed owner or unavailable member is not guessed or recreated.
        fn ae2_reconcile_pending_cpu_breaks(&mut self) -> Result<(), String> {
            while let Some(notice)=self.ae2_next_cpu_break()? {
                if notice.removed_owned_job {return Err("removed CPU job requires synchronous custody reconciliation".into());}
                let mut members=Vec::new();
                for (node,pos,resident) in &notice.members {
                    if pos.dimension != super::super::ae2_event_live::ordinal(self.dim) {return Err("CPU break dimension mismatch".into());}
                    let local=(pos.x,pos.y,pos.z);
                    if !resident {
                        if !notice.unloaded_idle.contains(node) {return Err("CPU break includes an unavailable owner".into());}
                        if self.world.loaded(local.0,local.2) {
                            let Some(BlockEntity::Ae2(machine))=self.world.block_entities.get(&local) else {return Err("reloaded CPU member unavailable".into());};
                            if Self::ae2_cpu_snapshot_waiting(machine,self.dim)?.is_some() {return Err("reloaded CPU member owns a job".into());}
                        }
                        continue;
                    }
                    if self.world.block_entities.incarnation(&local)!=Some(node.host) {
                        if notice.removed==Some(*node) {continue;}
                        return Err("CPU break member incarnation unavailable".into());
                    }
                    members.push(local);
                }
                if let Some(&anchor)=members.first() {self.ae2_break_crafting_members(anchor,members)?;}
                self.ae2_ack_cpu_break(notice.sequence)?;
            }
            Ok(())
        }
        fn ae2_break_crafting_members(&mut self, pos: Pos, members: Vec<Pos>) -> Result<(), String> {
            let mut jobs = Vec::new();
            for p in members {
                let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get(&p) else {
                    continue;
                };
                if machine.config.contains_key(&"ae2_cpu_job".into()) {
                    return Err(
                        "legacy crafting job must be reconciled before cluster transformation"
                            .into(),
                    );
                }
                if !machine.config.contains_key(&JOB.into()) {
                    continue;
                }
                let address = self.ae2_local_address(p).ok_or("invalid CPU address")?;
                let session = read_session(machine, self.dim)?;
                // Assembler inputs are physically owned by the destination. Do not
                // erase their flight if its block entity cannot be marked for recovery.
                for (id, at) in &session.flights {
                    let flight = session.cpu.flights().get(id).ok_or("missing CPU flight")?;
                    let spec = session
                        .specs
                        .get(&flight.step.pattern)
                        .ok_or("missing flight pattern")?;
                    if !spec.processing
                        && session.job_id.is_none()
                        && !matches!(self.ae2_block_entity(*at),
                        Some(BlockEntity::Ae2(m)) if m.kind == MachineKind::MolecularAssembler)
                    {
                        return Err(
                            "flight assembler is unavailable; load it before breaking the cluster"
                                .into(),
                        );
                    }
                }
                let items = read_escrow(machine)?;
                let fluids = read_fluids(machine)?;
                jobs.push((p, address, session, items, fluids));
            }
            let before = snapshot(self);
            let result =
                (|| -> Result<Vec<ItemStack>, String> {
                    // Stop receiving processing outputs before reinserting escrow.
                    for (p, address, session, _, _) in &jobs {
                        if let Some(request) = session_request(session)? {
                            match request.owner {
                                RequestOwner::Interface(owner) => {
                                    if self.ae2_interface_craft_links(owner).ok().and_then(
                                        |links| links.get(request.slot).copied().flatten(),
                                    ) == Some(*address)
                                    {
                                        self.ae2_interface_set_craft_link(
                                            owner,
                                            request.slot,
                                            Some(*address),
                                            None,
                                        )?;
                                    }
                                }
                                RequestOwner::Export(owner) => {
                                    if self.ae2_export_craft_links(owner).ok().and_then(|links| {
                                        links.get(request.slot).copied().flatten()
                                    }) == Some(*address)
                                    {
                                        self.ae2_export_set_craft_link(
                                            owner,
                                            request.slot,
                                            Some(*address),
                                            None,
                                        )?;
                                    }
                                }
                            }
                        }
                        // The assembler owns its physical ingredients independently of the CPU.
                        // Mark it for recovery before discarding the associated flight record.
                        for (id, at) in &session.flights {
                            let flight =
                                session.cpu.flights().get(id).ok_or("missing CPU flight")?;
                            let spec = session
                                .specs
                                .get(&flight.step.pattern)
                                .ok_or("missing flight pattern")?;
                            if spec.processing {
                                continue;
                            }
                            let at = *at;
                            if let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity_mut(at) {
                                if machine.kind == MachineKind::MolecularAssembler {
                                    machine
                                        .config
                                        .insert("ae2_orphaned_flight".into(), Tag::Byte(1));
                                    self.ae2_mark_modified(at);
                                }
                            }
                        }
                        let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get_mut(p)
                        else {
                            return Err("CPU disappeared".into());
                        };
                        machine.config.remove(&JOB.into());
                        machine.config.remove(&ESCROW.into());
                        machine.config.remove(&FLUID_ESCROW.into());
                        self.world.modified.insert((p.0 >> 4, p.2 >> 4));
                    }
                    let mut plan = super::super::ae2_buses::FormationPlan::default();
                    let mut remaining = BTreeMap::<String,(ItemStack,u64)>::new();
                    for (_, address, _, items, fluids) in &jobs {
                        for item in items {
                            let inserted = self
                                .ae2_grid_insert_uncharged_staged_encoded(
                                    *address,
                                    item,
                                    u64::from(item.count),
                                    &mut plan,
                                )
                                .unwrap_or(0)
                                .min(u64::from(item.count));
                            if inserted < u64::from(item.count) {
                                let mut prototype=item.clone();prototype.count=1;
                                let entry=remaining.entry(canon(&prototype)).or_insert((prototype,0));
                                entry.1=entry.1.checked_add(u64::from(item.count)-inserted).ok_or("CPU break drop amount overflow")?;
                            }
                        }
                        for (key, amount) in fluids {
                            // AEFluidKey.addDrops emits no item; unaccepted fluid is voided upstream.
                            let _ = self.ae2_grid_fluid_capability_staged_encoded(
                                *address, key, *amount, true, false, &mut plan,
                            );
                        }
                    }
                    let mut drops=Vec::new();
                    for (_, (prototype,amount)) in remaining {
                        // CraftingBlockEntity.breakCluster calls AEItemKey.addDrops
                        // with a fresh list for every exact key after cancel().
                        let mut key_drops=Vec::new();
                        super::super::spill::append_ae2_key_drops(&mut key_drops,&prototype,amount,&|stack| {
                            let default=self.res.items().get(&stack.item).map_or(64,|info|info.max_stack).max(1);
                            super::super::ae2_formation::max_stack(stack,default)
                        });
                        drops.try_reserve(key_drops.len()).map_err(|_|"CPU break drop allocation failed")?;drops.extend(key_drops);
                    }
                    self.ae2_commit_formation(plan)?;
                    Ok(drops)
                })();
            match result {
                Ok(drops) => {
                    for item in drops {
                        self.pop_resource([pos.0, pos.1, pos.2], item);
                    }
                    for (_,_,session,_,_) in &jobs {
                        if !session.cpu.is_cancelled() {
                            if let Some((owner,event))=job_status(session,horizon_client::ae2_job_status::Status::Cancelled) {self.ae2_emit_job_status(owner,event);}
                        }
                    }
                    Ok(())
                }
                Err(error) => {
                    restore(self, before);
                    Err(error)
                }
            }
        }
        pub(in crate::game) fn ae2_recursive_cancel(
            &mut self,
            pos: GridAddress,
        ) -> Result<(), String> {
            let dim = self.ae2_address_parts(pos).ok_or("invalid CPU address")?.0;
            let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity(pos) else {
                return Err("no CPU".into());
            };
            let mut session = read_session(machine, dim)?;
            let physical = read_escrow(machine)?;
            if session.cpu.is_cancelled() {return Ok(());}
            session.cpu.cancel().map_err(|e| format!("{e:?}"))?;
            let cancelled=job_status(&session,horizon_client::ae2_job_status::Status::Cancelled);
            if let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity_mut(pos) {
                write_session(machine, &session, &physical)?;
            }
            self.ae2_mark_modified(pos);
            if let Some((owner,event))=cancelled {self.ae2_emit_job_status(owner,event);}
            Ok(())
        }
        pub(in crate::game) fn ae2_recursive_toggle_suspended(&mut self, pos: GridAddress) -> Result<bool, String> {
            let dim = self.ae2_address_parts(pos).ok_or("invalid CPU address")?.0;
            let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity(pos) else { return Err("no CPU".into()); };
            let mut session = read_session(machine, dim)?;
            let physical = read_escrow(machine)?;
            if session.cpu.is_cancelled() { return Err("job is cancelling".into()); }
            session.suspended = !session.suspended;
            let suspended = session.suspended;
            let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity_mut(pos) else { return Err("CPU disappeared".into()); };
            write_session(machine, &session, &physical)?;
            self.ae2_mark_modified(pos);
            Ok(suspended)
        }
        pub(in crate::game) fn ae2_recursive_suspended(&self, pos: GridAddress) -> Option<bool> {
            let dim = self.ae2_address_parts(pos)?.0;
            let BlockEntity::Ae2(machine) = self.ae2_block_entity(pos)? else { return None; };
            Some(read_session(machine, dim).ok()?.suspended)
        }
        pub(in crate::game) fn ae2_recursive_job_identity(&self, pos: GridAddress) -> Option<[i64;2]> {
            let dim = self.ae2_address_parts(pos)?.0;
            let BlockEntity::Ae2(machine) = self.ae2_block_entity(pos)? else { return None; };
            read_session(machine, dim).ok()?.job_id
        }
        pub(in crate::game) fn ae2_recursive_status_icon(&self, key:&str) -> Option<ItemStack> {
            if let Some(fluid)=parse_fluid(key) {return super::super::ae2_fluids::fluid_bucket(&fluid.item_id).map(|id|ItemStack::new(id,1));}
            from_key(key).ok()
        }
        /// A read-only snapshot of real CPU escrow, pending outputs, and in-flight outputs.
        pub(in crate::game) fn ae2_recursive_status_rows(&self, pos: GridAddress) -> Option<Vec<(String,u64,u64,u64)>> {
            let dim = self.ae2_address_parts(pos)?.0;
            let BlockEntity::Ae2(machine) = self.ae2_block_entity(pos)? else { return None; };
            let session = read_session(machine, dim).ok()?;
            let mut rows = BTreeMap::<String,(u64,u64,u64)>::new();
            for (key, count) in session.cpu.escrow() { rows.entry(key.clone()).or_default().0 = *count; }
            for step in session.cpu.pending() {
                for (key,count) in &step.outputs {
                    let row=rows.entry(key.clone()).or_default();
                    row.1=row.1.saturating_add(*count);
                }
            }
            for flight in session.cpu.flights().values() {
                for (key,count) in &flight.remaining {
                    let row=rows.entry(key.clone()).or_default();
                    row.2=row.2.saturating_add(*count);
                }
            }
            for (key,count) in session.cpu.emitted_waiting() {
                let row=rows.entry(key.clone()).or_default();
                row.2=row.2.saturating_add(*count);
            }
            let mut rows:Vec<_>=rows.into_iter().map(|(key,(stored,pending,active))| (key,stored,pending,active)).collect();
            rows.sort_by(|a,b| b.2.saturating_add(b.3).cmp(&a.2.saturating_add(a.3)).then_with(||b.1.cmp(&a.1)).then_with(||a.0.cmp(&b.0)));
            Some(rows)
        }
        /// Return a physical buffered stack through raw MEStorage insertion.
        /// Source PatternProviderReturnInventory fires its result callback only
        /// after the network has actually accepted the stack.
        fn tick_provider_fluid_return(&mut self, owner: ProviderOwner) -> Result<(), String> {
            let address = owner.anchor();
            let Some(provider) = self.provider_machine(owner) else {
                return Ok(());
            };
            let Some(Tag::List(entries)) =
                provider.config.get(&"ae2_provider_return_fluids".into())
            else {
                return Ok(());
            };
            let mut occupied = [false; 9];
            if let Some(Tag::List(items)) = provider.config.get(&"ae2_provider_return_items".into())
            {
                for entry in items {
                    let Some(Tag::Byte(slot)) = entry.get("Slot") else {
                        return Err("invalid provider return item slot".into());
                    };
                    let i = *slot as usize;
                    if i >= 9 || occupied[i] {
                        return Err("duplicate provider return slot".into());
                    }
                    occupied[i] = true;
                }
            } else if provider
                .config
                .contains_key(&"ae2_provider_return_items".into())
            {
                return Err("invalid provider return items".into());
            }
            for entry in entries {
                let Some(Tag::Byte(slot)) = entry.get("Slot") else {
                    return Err("invalid provider return fluid slot".into());
                };
                let i = *slot as usize;
                if i >= 9
                    || occupied[i]
                    || !matches!(entry.get("amount"),Some(Tag::Long(n)) if *n>0 && *n<=4000)
                    || !matches!(entry.get("fluid"), Some(Tag::String(_)))
                    || !matches!(entry.get("components"), Some(Tag::ByteArray(_)))
                {
                    return Err("invalid provider fluid entry".into());
                }
                occupied[i] = true;
            }
            let mut entries = entries.clone();
            let Some(entry) = entries.first() else {
                return Ok(());
            };
            let (
                Some(Tag::String(fluid)),
                Some(Tag::ByteArray(components)),
                Some(Tag::Long(amount)),
            ) = (
                entry.get("fluid"),
                entry.get("components"),
                entry.get("amount"),
            )
            else {
                return Err("invalid provider fluid return".into());
            };
            if *amount <= 0 {
                return Err("invalid provider fluid amount".into());
            }
            let key = ae2_cell_core::ItemKey {
                item_id: fluid.to_string().map_err(|_| "invalid fluid identifier")?,
                components: components.clone(),
            };
            let amount = *amount as u64;
            let before = snapshot(self);
            let result = (|| -> Result<(), String> {
                let mut m = self.provider_machine(owner).ok_or("provider disappeared")?;
                m.config
                    .insert("ae2_provider_injecting".into(), Tag::Byte(1));
                self.store_provider(owner, &m)?;
                let mut plan = super::super::ae2_buses::FormationPlan::default();
                let accepted = self.ae2_grid_fluid_insert_uncharged_staged_encoded(address, &key, amount, &mut plan)?;
                if accepted > amount {
                    return Err("provider fluid return exceeds physical amount".into());
                }
                if accepted > 0 {
                    if accepted == amount {
                        entries.remove(0);
                    } else if let Tag::Compound(map) = &mut entries[0] {
                        map.insert("amount".into(), Tag::Long((amount - accepted) as i64));
                    } else {
                        return Err("invalid provider fluid entry".into());
                    }
                    let mut m = self.provider_machine(owner).ok_or("provider disappeared")?;
                    m.config
                        .insert("ae2_provider_return_fluids".into(), Tag::List(entries));
                    m.config.remove(&"ae2_provider_injecting".into());
                    self.store_provider(owner, &m)?;
                    self.provider_receive(owner, &fluid_name(&key), accepted);
                    self.ae2_commit_formation(plan)?;
                } else {
                    let mut m = self.provider_machine(owner).ok_or("provider disappeared")?;
                    m.config.remove(&"ae2_provider_injecting".into());
                    self.store_provider(owner, &m)?;
                }
                Ok(())
            })();
            if result.is_err() {
                restore(self, before);
            }
            result
        }
        fn tick_provider_return(&mut self, owner: ProviderOwner) -> Result<(), String> {
            let address = owner.anchor();
            if !self.provider_online(owner) {
                return Ok(());
            }
            let Some(provider) = self.provider_machine(owner) else {
                return Ok(());
            };
            let Some(tag) = provider.config.get(&"ae2_provider_return_items".into()) else {
                return self.tick_provider_fluid_return(owner);
            };
            let Tag::List(entries) = tag else {
                return Err("invalid provider return list".into());
            };
            let mut seen = [false; 9];
            for e in entries {
                let Some(Tag::Byte(slot)) = e.get("Slot") else {
                    return Err("invalid provider return slot".into());
                };
                let i = *slot as usize;
                if i >= 9
                    || seen[i]
                    || !matches!(e.get("count"),Some(Tag::Int(n)) if *n>0)
                    || horizon_client::save::item_from_nbt(e).is_none()
                {
                    return Err("invalid provider return entry".into());
                }
                seen[i] = true;
            }
            let mut slots = horizon_client::save::decode_items(Some(tag), 9);
            let Some(index) = slots.iter().position(Option::is_some) else {
                return self.tick_provider_fluid_return(owner);
            };
            let stack = slots[index].clone().ok_or("missing provider return item")?;
            let before = snapshot(self);
            let result = (|| -> Result<(), String> {
                let mut m = self.provider_machine(owner).ok_or("provider disappeared")?;
                m.config
                    .insert("ae2_provider_injecting".into(), Tag::Byte(1));
                self.store_provider(owner, &m)?;
                let mut plan = super::super::ae2_buses::FormationPlan::default();
                let accepted = self.ae2_grid_insert_uncharged_staged_encoded(
                    address,
                    &stack,
                    stack.count as u64,
                    &mut plan,
                )?;
                if accepted > stack.count as u64 {
                    return Err("provider return exceeds physical stack".into());
                }
                if accepted > 0 {
                    let left = stack.count - accepted as u32;
                    slots[index] = (left > 0).then_some(ItemStack {
                        count: left,
                        ..stack.clone()
                    });
                    let mut m = self.provider_machine(owner).ok_or("provider disappeared")?;
                    m.config.insert(
                        "ae2_provider_return_items".into(),
                        horizon_client::save::encode_items(&slots),
                    );
                    m.config.remove(&"ae2_provider_injecting".into());
                    self.store_provider(owner, &m)?;
                    self.provider_receive(owner, &canon(&stack), accepted);
                    self.ae2_commit_formation(plan)?;
                } else {
                    let mut m = self.provider_machine(owner).ok_or("provider disappeared")?;
                    m.config.remove(&"ae2_provider_injecting".into());
                    self.store_provider(owner, &m)?;
                }
                Ok(())
            })();
            if result.is_err() {
                restore(self, before);
            }
            result
        }
        /// Continue reserved physical jobs after normal assembler ticks.
        pub(in crate::game) fn tick_ae2_providers(&mut self) {
            // Sample each provider in its real level, including parked levels
            // during their server tick. Pulse locks require a fresh rising edge.
            let hosts:Vec<_>=self.world.block_entities.iter().filter_map(|(&p,e)|matches!(e,BlockEntity::Ae2(m) if matches!(m.kind,MachineKind::PatternProvider|MachineKind::CableBus)).then_some(p)).collect();
            let mut providers = Vec::new();
            for pos in hosts {
                let Some(host) = self.ae2_local_address(pos) else {
                    continue;
                };
                let block = ProviderOwner::Block(host);
                if self.provider_machine(block).is_some() {
                    providers.push((pos, block));
                }
                for face in 0..6 {
                    let part = ProviderOwner::Part { host, face };
                    if self.provider_machine(part).is_some() {
                        providers.push((pos, part));
                    }
                }
            }
            let network=self.ae2_network();let services=self.ae2_event_service_ids(&network);let now=self.ae2_server_tick();
            for (p, owner) in providers {
                let identity=match owner {ProviderOwner::Block(a)=>network.node_identities.get(&a.network_pos()).copied(),ProviderOwner::Part{host,face}=>network.part_identities.get(&(host.network_pos(),face)).copied()};
                let current=self.provider_machine(owner);
                let signature=|m:&Machine| {
                    serde_json::to_vec(&["ae2_provider_return_items","ae2_provider_return_fluids",horizon_client::ae2_provider_queue::QUEUE,"ae2_provider_send_items","ae2_provider_send_fluids","ae2_provider_send_face"].iter().map(|key|m.config.get(&(*key).into()).map(horizon_client::item_data::canon_tag).unwrap_or_default()).collect::<Vec<_>>()).unwrap_or_default()
                };
                let work=|m:&Machine| ["ae2_provider_return_items","ae2_provider_return_fluids",horizon_client::ae2_provider_queue::QUEUE,"ae2_provider_send_items","ae2_provider_send_fluids"].iter().any(|key|matches!(m.config.get(&(*key).into()),Some(Tag::List(v)) if !v.is_empty()));
                if let (Some(identity),Some(before))=(identity,current.as_ref()) {
                    let token=signature(before);let active=self.provider_online(owner);let pending=work(before);
                    if token.len()<=1_048_576 {
                        let service=services.as_ref().and_then(|s|s.get(&identity)).copied();
                        // PatternProviderLogic.onMainNodeStateChanged alerts
                        // its tracker when an inactive node becomes active.
                        let observed=serde_json::to_vec(&(active,&token)).unwrap_or_default();
                        let (min, max) = horizon_client::ae2_tick_config::rates(horizon_client::ae2_tick_config::Kind::Interface);
                        let due=self.ae2_device_scheduler.borrow_mut().due_with_token(identity,service,now,min,max,!active || !pending,&observed);
                        if due {
                            if let Err(error)=self.tick_provider_return(owner) {log::warn!("AE2 provider return {p:?}: {error}");}
                            if let Err(error)=self.tick_provider_send(owner) {log::warn!("AE2 provider send {p:?}: {error}");}
                            let after=self.provider_machine(owner);let modulation=match after.as_ref() {
                                Some(m) if !work(m)=>super::super::ae2_tick_tracker::Modulation::Sleep,
                                Some(m) if signature(m)!=token=>super::super::ae2_tick_tracker::Modulation::Urgent,
                                _=>super::super::ae2_tick_tracker::Modulation::Slower,
                            };
                            self.ae2_device_scheduler.borrow_mut().completed(identity,now,modulation);
                        }
                    }
                }
                let high = self.with_level(|b, l, t| {
                    b.has_neighbor_signal(
                        l,
                        t,
                        horizon_core::position::BlockPos::new(p.0, p.1, p.2),
                    )
                });
                if let Some(mut m) = self.provider_machine(owner) {
                    let prev = m.config.get(&"ae2_redstone_high".into()) == Some(&Tag::Byte(1));
                    let event = match m.config.get(&"ae2_unlock_event".into()) {
                        Some(Tag::Int(n)) => *n,
                        _ => 0,
                    };
                    let next = if event == 2 && !high {
                        1
                    } else if event == 1 && high {
                        0
                    } else {
                        event
                    };
                    if prev != high || event != next {
                        m.config
                            .insert("ae2_redstone_high".into(), Tag::Byte(i8::from(high)));
                        if next == 0 {
                            m.config.remove(&"ae2_unlock_event".into());
                        } else {
                            m.config.insert("ae2_unlock_event".into(), Tag::Int(next));
                        }
                        if let Err(error) = self.store_provider(owner, &m) {
                            log::warn!("AE2 provider redstone {p:?}: {error}");
                        }
                    }
                }
            }
        }
        pub(in crate::game) fn tick_ae2_recursive_crafting(&mut self) {
            if super::super::ae2_event_live::cpu_formation_enabled() {
                if let Err(error)=self.ae2_reconcile_pending_cpu_breaks() {
                    log::warn!("AE2 CPU custody unavailable: {error}");
                    self.ae2_fault_cpu_break(error);
                    return;
                }
            }
            let cpus: Vec<_> = self
                .world
                .block_entities
                .iter()
                .filter_map(|(pos, e)| match e {
                    BlockEntity::Ae2(m) if m.config.contains_key(&JOB.into()) => Some(*pos),
                    _ => None,
                })
                .collect();
            for pos in cpus {
                if let Err(error) = self.tick_recursive_one(pos) {
                    log::warn!("AE2 crafting CPU {pos:?}: {error}");
                }
            }
        }
        fn tick_recursive_one(&mut self, pos: Pos) -> Result<(), String> {
            let before = snapshot(self);
            let prior=self.world.block_entities.get(&pos).and_then(|entity|match entity {
                BlockEntity::Ae2(machine)=>read_session(machine,self.dim).ok(),_=>None,
            });
            let mut formation = super::super::ae2_buses::FormationPlan::default();
            match self.tick_recursive_inner(pos, &mut formation).and_then(|()|self.ae2_commit_formation(formation)) {
                Ok(()) => {
                    let remains=matches!(self.world.block_entities.get(&pos),Some(BlockEntity::Ae2(machine)) if machine.config.contains_key(&JOB.into()));
                    if !remains {
                        if let Some(session)=prior.filter(|session|!session.cpu.is_cancelled()) {
                            if let Some((owner,mut event))=job_status(&session,horizon_client::ae2_job_status::Status::Finished) {
                                event.remaining=0;self.ae2_emit_job_status(owner,event);
                            }
                        }
                    }
                    Ok(())
                },
                Err(error) => {
                    restore(self, before);
                    Err(error)
                }
            }
        }
        fn tick_recursive_inner(&mut self, pos: Pos, formation: &mut super::super::ae2_buses::FormationPlan) -> Result<(), String> {
            let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get(&pos) else {
                return Ok(());
            };
            let mut session = read_session(machine, self.dim)?;
            let mut physical = read_escrow(machine)?;
            let mut fluids = read_fluids(machine)?;
            if session.job_id.is_none() {
                let fresh=[self.world.random.next_long(),self.world.random.next_long()];
                if migrate_job_identity(&mut session,fresh) {
                    let Some(BlockEntity::Ae2(cpu))=self.world.block_entities.get_mut(&pos) else {return Err("CPU disappeared".into());};
                    write_session(cpu,&session,&physical)?;
                    self.world.modified.insert((pos.0 >> 4,pos.2 >> 4));
                }
            }
            let cpu_address = self.ae2_local_address(pos).ok_or("invalid CPU address")?;
            let requester = session_request(&session)?;
            if let Some(requester) = &requester {
                let fluid = parse_fluid(&requester.target);
                let key = match &fluid {
                    Some(key) => key.clone(),
                    None => ae2_lab::key(&from_key(&requester.target)?)?,
                };
                if !session.cpu.is_cancelled()
                    && !self.ae2_request_valid(requester, cpu_address, &key, fluid.is_some())
                {
                    session.cpu.cancel().map_err(|e| format!("{e:?}"))?;
                    // Record the cancellation even while this CPU is offline;
                    // a subsequently replaced card must not revive the job.
                    let Some(BlockEntity::Ae2(cpu)) = self.world.block_entities.get_mut(&pos)
                    else {
                        return Err("CPU disappeared".into());
                    };
                    write_session(cpu, &session, &physical)?;
                    self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
                }
            }
            if !self.ae2_online(pos) {
                return Ok(());
            }
            // A broken or reduced CPU keeps its physical escrow until restored.
            let (capacity,coprocessors)=if super::super::ae2_event_live::cpu_formation_enabled() {
                let Ok(capacity)=self.ae2_observed_cpu_capacity(cpu_address) else {return Ok(());};capacity
            } else {
                let Ok((_,capacity,coprocessors))=cluster(self,pos,true) else{return Ok(());};(capacity,coprocessors)
            };
            if capacity < session.cpu.capacity_bytes || coprocessors < session.cpu.coprocessors {
                return Ok(());
            }
            let mut machine_updates: Vec<(GridAddress, BlockEntity)> = Vec::new();
            // Collect only complete expected physical outputs and recipe remainders.
            let flights: Vec<_> = session.flights.iter().map(|(id, p)| (*id, *p)).collect();
            for (id, at) in flights {
                let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity(at) else {
                    continue;
                };
                if machine.kind != MachineKind::MolecularAssembler
                    || machine.slots.len() != 11
                    || machine.slots[9].is_none()
                {
                    continue;
                }
                let outputs: Vec<_> = machine.slots[..10].iter().flatten().cloned().collect();
                let Some(flight) = session.cpu.flights().get(&id) else {
                    return Err("orphan assembler flight".into());
                };
                if escrow_counts(&outputs)? != flight.remaining {
                    continue;
                }
                let mut next = session.cpu.clone();
                for item in &outputs {
                    next.receive(id, &canon(item), u64::from(item.count))
                        .map_err(|e| format!("{e:?}"))?;
                }
                let mut updated = (**machine).clone();
                for slot in &mut updated.slots[..10] {
                    *slot = None;
                }
                updated.config.remove(&"ae2_assembler_pattern".into());
                updated.config.remove(&"ae2_flight_owner".into());
                session.cpu = next;
                session.flights.remove(&id);
                physical.extend(outputs);
                machine_updates.push((at, BlockEntity::Ae2(Box::new(updated))));
            }
            if !session.suspended { session.cpu.tick(); }
            // Upstream suspension stops scheduling, while reconciliation and cancellation continue.
            let mut rejected_targets=BTreeMap::<String,std::collections::BTreeSet<GridAddress>>::new();
            let mut passes = BTreeMap::<String, Option<super::super::ae2_event_live::OrderedProviderPass>>::new();
            let mut exhausted = BTreeSet::<String>::new();
            'dispatch: loop {
                if (session.suspended && !session.cpu.is_cancelled()) || session.cpu.remaining_operations()==0 { break; }
                let mut chosen:Option<(usize,GridAddress,Spec,f64)> = None;
                for (index, task) in session.cpu.pending().iter().cloned().enumerate() {
                    let spec = session.specs.get(&task.pattern).ok_or("missing plan pattern")?.clone();
                    if let Some(definition) = spec.definition.as_ref() {
                        if exhausted.contains(&task.pattern) {continue;}
                        if !passes.contains_key(&task.pattern) {
                            passes.insert(task.pattern.clone(), self.ae2_ordered_provider_pass(pos, definition)?);
                        }
                        let Some(pass) = passes.get_mut(&task.pattern).and_then(Option::as_mut) else {
                            exhausted.insert(task.pattern.clone()); continue;
                        };
                        loop {
                            let Some(medium) = self.ae2_ordered_provider_next(pos, pass)? else {
                                exhausted.insert(task.pattern.clone()); break;
                            };
                            // Java's enhanced-for obtains this medium before
                            // noticing that actual input extraction failed.
                            if !session.cpu.task_has_inputs(index) {exhausted.insert(task.pattern.clone()); break;}
                            if !self.ae2_ordered_provider_contains(pos, &medium, definition, pass)? {return Err("provider membership changed during dispatch".into());}
                            let mut bound = spec.clone(); bound.provider = ordered_owner(&medium)?;
                            let provider = self.provider_machine(bound.provider).ok_or("registered provider disappeared")?;
                            if provider.kind != MachineKind::PatternProvider {return Err("registered provider changed kind".into());}
                            if Self::provider_send_pending(&provider) {continue;}
                            // CraftingCpuHelper uses one AE per item and one AE
                            // per 125 mB of fluid. Simulate only after isBusy.
                            let power = task.inputs.iter().map(|(key, amount)| {
                                let one = amount / task.runs;
                                (one as f64) / if parse_fluid(key).is_some() {125.0} else {1.0}
                            }).sum::<f64>();
                            if power > 0.0 && self.ae2_take_power(pos, power, true) < power - 0.01 {
                                exhausted.insert(task.pattern.clone()); break;
                            }
                            if let Some(at) = self.recursive_destination(&bound, None, Some(&medium.effective_patterns),formation) {
                                if !machine_updates.iter().any(|(p, _)| *p == at) && !session.flights.values().any(|p| *p == at) {
                                    if !self.ae2_ordered_provider_contains(pos, &medium, definition, pass)? {return Err("provider membership changed before push".into());}
                                    chosen=Some((index,at,bound,power)); break;
                                }
                            }
                        }
                    } else if session.cpu.task_has_inputs(index) {
                        if let Some(at) = self.recursive_destination(&spec,rejected_targets.get(&task.pattern),None,formation) {
                            if !machine_updates.iter().any(|(p, _)| *p == at) && !session.flights.values().any(|p| *p == at) {
                                chosen=Some((index,at,spec,0.0));
                            }
                        }
                    }
                    if chosen.is_some() {break;}
                }
                let Some((index, at, spec, power)) = chosen else {break;};
                let mut next = session.cpu.clone();
                let (id, step) = next.dispatch_task(index).map_err(|e|format!("CPU task dispatch: {e:?}"))?;
                if spec.processing {
                    use horizon_client::ae2_provider_queue::Entry;
                    let provider=self.provider_machine(spec.provider).ok_or("provider disappeared")?;
                    let pattern=match &spec.definition {
                        Some(definition)=>item_data::parse_stack(definition)?.ok_or("missing processing definition")?,
                        None=>provider.slots.get(spec.slot).and_then(Clone::clone).ok_or("missing processing pattern")?,
                    };
                    let ordered=checked_processing_input_order(&pattern,&step.inputs)?;
                    let mut next_physical = physical.clone();
                    let mut next_fluids = fluids.clone();
                    let mut offered:Vec<Entry> = Vec::new();
                    offered.try_reserve(ordered.len()).map_err(|_|"processing input allocation failed")?;
                    let mut fits = true;
                    let face=self.provider_target_face(spec.provider.anchor(),at).ok_or("invalid provider target face")?;
                    for (key,amount) in ordered {
                        if let Some(fluid)=parse_fluid(&key) {
                            let old=next_fluids.get(&fluid).copied().unwrap_or(0);
                            if old<amount || self.provider_target_fluid_insert(at,face,&fluid,amount,true,formation)==0 {fits=false;break;}
                            if old==amount {next_fluids.remove(&fluid);}else{next_fluids.insert(fluid.clone(),old-amount);}
                            offered.push(Entry::Fluid{key:fluid,amount});
                        } else {
                            let Some(stacks)=take_physical_many(&mut next_physical,&key,amount)? else{fits=false;break;};
                            for mut stack in stacks {
                                let amount=u64::from(stack.count);stack.count=1;
                                offered.push(Entry::Item{prototype:stack,amount});
                            }
                        }
                    }
                    if !fits {
                        rejected_targets.entry(step.pattern.clone()).or_default().insert(at);continue 'dispatch;
                    }
                    if owned_counts(&next_physical, &next_fluids)? != *next.escrow() {
                        return Err("processing dispatch ownership mismatch".into());
                    }
                    // Source pushes sparse inputs in order and keeps each
                    // unaccepted amount as one owned generic-stack entry.
                    let mut send=Vec::new();send.try_reserve(offered.len()).map_err(|_|"provider send allocation failed")?;
                    for entry in offered {
                        match entry {
                            Entry::Item{prototype,amount}=>{
                                let count=u32::try_from(amount).map_err(|_|"processing item amount overflow")?;
                                let stack=ItemStack{count,..prototype.clone()};
                                let accepted=match self.ae2_item_p2p_insert_staged(at,face,&stack,false,formation) {
                                    Some(n)=>n,None=>self.ae2_native_item_insert_staged(at,face,&stack,count,false,formation),
                                };
                                if accepted>count{return Err("provider target overaccepted item".into());}
                                if accepted<count {send.push(Entry::Item{prototype,amount:u64::from(count-accepted)});}
                            }
                            Entry::Fluid{key,amount}=>{
                                let accepted=self.provider_target_fluid_insert(at,face,&key,amount,false,formation);
                                if accepted>amount{return Err("provider target overaccepted fluid".into());}
                                if accepted<amount {send.push(Entry::Fluid{key,amount:amount-accepted});}
                            }
                        }
                    }
                    if !send.is_empty() {
                        let mut provider=self.provider_machine(spec.provider).ok_or("provider disappeared")?;
                        provider.config.insert("ae2_provider_send_face".into(),Tag::Byte(Self::provider_send_direction(face)));
                        horizon_client::ae2_provider_queue::write(&mut provider.config,&send)?;
                        self.store_provider(spec.provider,&provider)?;
                    }
                    let mut positions = self.provider_neighbors(spec.provider);
                    positions.sort();
                    if let Some(index) = positions.iter().position(|p| *p == at) {
                        if let Some(mut provider) = self.provider_machine(spec.provider) {
                            provider.config.insert(
                                "ae2_provider_round_robin".into(),
                                Tag::Int(((index + 1) % positions.len()) as i32),
                            );
                            self.store_provider(spec.provider, &provider)?;
                        }
                    }
                    if power > 0.0 && self.ae2_take_power(pos, power, false) < power - 0.01 {
                        return Err("crafting power changed during push".into());
                    }
                    self.provider_arm_lock(spec.provider, &spec.output)?;
                    session.cpu = next;
                    session.flights.insert(id, at);
                    physical = next_physical;
                    fluids = next_fluids;
                    self.tick_provider_send_staged(spec.provider,formation)?;
                    continue;
                }
                if session.job_id.is_none() && session.flights.is_empty() {
                    session.job_id =
                        Some([self.world.random.next_long(), self.world.random.next_long()]);
                }
                let Some(job_id) = session.job_id else {
                    break;
                };
                let Some((template, active_fluids)) = choose_grid(&spec, &step, self.res.recipes(), self.res.stations()) else {
                    break;
                };
                let mut staged = vec![None; 9];
                let mut next_physical = physical.clone();
                let mut next_fluids = fluids.clone();
                let mut assembler_fluids = Vec::new();
                for (i, expected) in template.iter().enumerate() {
                    if let Some((name, amount)) = active_fluids.get(&i) {
                        let key = parse_fluid(name).ok_or("invalid assembler fluid key")?;
                        let n = next_fluids.get_mut(&key).ok_or("missing assembler fluid")?;
                        if *n < *amount {
                            return Err("insufficient assembler fluid".into());
                        }
                        *n -= *amount;
                        if *n == 0 {
                            next_fluids.remove(&key);
                        }
                        assembler_fluids.push(Tag::compound([
                            ("slot", Tag::Byte(i as i8)),
                            ("fluid", Tag::String(key.item_id.as_str().into())),
                            ("components", Tag::ByteArray(key.components)),
                            ("amount", Tag::Long(*amount as i64)),
                        ]));
                        continue;
                    }
                    if let Some(expected) = expected {
                        staged[i] = Some(
                            take_physical(&mut next_physical, &canon(expected), 1)
                                .ok_or("physical CPU input missing")?,
                        );
                    }
                }
                if owned_counts(&next_physical, &next_fluids)? != *next.escrow() {
                    return Err("physical CPU dispatch mismatch".into());
                }
                let Some(BlockEntity::Ae2(machine)) = self.ae2_block_entity(at) else {
                    return Err("assembler disappeared".into());
                };
                let mut updated = (**machine).clone();
                updated.slots[..9].clone_from_slice(&staged);
                updated.progress = 0.0;
                let output =
                    item_data::parse_stack(&spec.output)?.ok_or("missing pattern output")?;
                let mut data=pattern_data(&template,&output);
                if let Some((kind,recipe))=&spec.special {if let Tag::Compound(values)=&mut data {values.insert("special_kind".into(),Tag::String(kind.as_str().into()));values.insert("recipe".into(),Tag::String(recipe.as_str().into()));}}
                if let (Some(recipe), Tag::Compound(values))=(&spec.crafting_recipe, &mut data) { values.insert("crafting_recipe".into(),Tag::String(recipe.as_str().into())); }
                let definition=match &spec.definition {
                    Some(encoded)=>item_data::parse_stack(encoded)?,
                    None=>self.provider_machine(spec.provider).and_then(|provider|provider.slots.get(spec.slot).cloned().flatten()),
                };
                if let (Some(definition),Tag::Compound(values))=(definition,&mut data) {
                    values.insert("definition".into(),horizon_client::save::item_to_nbt(&definition));
                }
                updated.config.insert("ae2_assembler_pattern".into(),data);
                let flight_owner = AssemblerFlightOwner {
                    cpu: cpu_address,
                    job_id,
                    flight: id,
                };
                let owner_json = serde_json::to_string(&flight_owner).map_err(|e| e.to_string())?;
                updated.config.insert(
                    "ae2_flight_owner".into(),
                    Tag::String(owner_json.as_str().into()),
                );
                if !assembler_fluids.is_empty() {
                    updated.config.insert(
                        "ae2_assembler_fluid_escrow".into(),
                        Tag::List(assembler_fluids),
                    );
                }
                if power > 0.0 && self.ae2_take_power(pos, power, false) < power - 0.01 {
                    return Err("crafting power changed during push".into());
                }
                self.provider_arm_lock(spec.provider, &spec.output)?;
                session.cpu = next;
                session.flights.insert(id, at);
                physical = next_physical;
                fluids = next_fluids;
                machine_updates.push((at, BlockEntity::Ae2(Box::new(updated))));
            }
            if owned_counts(&physical, &fluids)? != *session.cpu.escrow() {
                return Err("CPU physical escrow mismatch".into());
            }
            // Return items only when all flights have been reconciled. Failure
            // restores every affected physical container and retains escrow.
            if session.cpu.pending().is_empty() && session.cpu.flights().is_empty()
                && session.cpu.emitted_waiting().is_empty() {
                let backup = snapshot(self);
                let mut remaining_items = physical.clone();
                let mut remaining_fluids = fluids.clone();
                // Clear completed assemblers before delivery: an output target
                // can itself be one of these physical inventories.
                for (p, m) in machine_updates.drain(..) {
                    let Some(dest) = self.ae2_block_entity_mut(p) else {
                        return Err("assembler disappeared".into());
                    };
                    *dest = m;
                    self.ae2_mark_modified(p);
                }
                if let Some(requester) = &requester {
                    let fluid_key = parse_fluid(&requester.target);
                    let key = match &fluid_key {
                        Some(key) => key.clone(),
                        None => ae2_lab::key(&from_key(&requester.target)?)?,
                    };
                    let linked =
                        self.ae2_request_valid(requester, cpu_address, &key, fluid_key.is_some());
                    if linked && !session.cpu.is_cancelled() {
                        let mut remaining = requester.amount;
                        if let Some(key) = fluid_key {
                            let available = remaining_fluids.get(&key).copied().unwrap_or(0);
                            let give = available.min(remaining);
                            let accepted =
                                self.ae2_request_deliver_fluid(requester, cpu_address, &key, give, formation)?;
                            if accepted > give {
                                return Err("requester accepted more fluid than offered".into());
                            }
                            if accepted > 0 {
                                session
                                    .cpu
                                    .deliver_target(&requester.target, accepted)
                                    .map_err(|e| format!("{e:?}"))?;
                            }
                            if let Some(amount) = remaining_fluids.get_mut(&key) {
                                *amount -= accepted;
                            }
                            remaining -= accepted;
                        } else {
                            for stack in &mut remaining_items {
                                if remaining == 0 {
                                    break;
                                }
                                if canon(stack) != requester.target {
                                    continue;
                                }
                                let give = u64::from(stack.count).min(remaining) as u32;
                                let part = ItemStack {
                                    count: give,
                                    ..stack.clone()
                                };
                                let accepted =
                                    self.ae2_request_deliver_item(requester, cpu_address, &part, formation)?;
                                if accepted > give {
                                    return Err("requester accepted more items than offered".into());
                                }
                                if accepted > 0 {
                                    session
                                        .cpu
                                        .deliver_target(&requester.target, u64::from(accepted))
                                        .map_err(|e| format!("{e:?}"))?;
                                }
                                stack.count -= accepted;
                                remaining -= u64::from(accepted);
                                if accepted < give {
                                    break;
                                }
                            }
                        }
                        if remaining != 0 {
                            match requester.owner {
                                RequestOwner::Interface(_) => {
                                    session
                                        .requester
                                        .as_mut()
                                        .ok_or("requester disappeared")?
                                        .amount = remaining
                                }
                                RequestOwner::Export(_) => {
                                    session
                                        .export_requester
                                        .as_mut()
                                        .ok_or("requester disappeared")?
                                        .amount = remaining
                                }
                            }
                            remaining_items.retain(|s| s.count > 0);
                            remaining_fluids.retain(|_, n| *n > 0);
                            if owned_counts(&remaining_items, &remaining_fluids)?
                                != *session.cpu.escrow()
                            {
                                return Err("partial requester escrow mismatch".into());
                            }
                            let Some(BlockEntity::Ae2(machine)) =
                                self.world.block_entities.get_mut(&pos)
                            else {
                                return Err("CPU disappeared".into());
                            };
                            write_fluids(machine, &remaining_fluids)?;
                            write_session(machine, &session, &remaining_items)?;
                            self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
                            return Ok(());
                        }
                    }
                    self.ae2_request_unlink(requester, cpu_address)?;
                }
                let mut virtual_result = Inventory::new();
                let mut complete = session.cpu.clone();
                complete
                    .finish(&mut virtual_result)
                    .map_err(|e| format!("{e:?}"))?;
                remaining_items.retain(|s| s.count > 0);
                remaining_fluids.retain(|_, n| *n > 0);
                if virtual_result != owned_counts(&remaining_items, &remaining_fluids)? {
                    return Err("finished CPU ownership mismatch".into());
                }
                for stack in remaining_items.iter().filter(|s| s.count > 0) {
                    if self
                        .ae2_grid_insert_staged(pos, stack, u64::from(stack.count), formation)
                        .ok()
                        != Some(u64::from(stack.count))
                    {
                        restore(self, backup);
                        return Err("CPU output storage temporarily unavailable".into());
                    }
                }
                for (key, amount) in remaining_fluids.iter().filter(|(_, amount)| **amount > 0) {
                    if self.ae2_grid_fluid_insert_staged(pos, key, *amount, false, formation).ok() != Some(*amount) {
                        restore(self, backup);
                        return Err("CPU fluid output storage temporarily unavailable".into());
                    }
                }
                if let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get_mut(&pos) {
                    machine.config.remove(&JOB.into());
                    machine.config.remove(&ESCROW.into());
                    machine.config.remove(&FLUID_ESCROW.into());
                }
                self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
                return Ok(());
            }
            for (p, m) in machine_updates {
                if let Some(dest) = self.ae2_block_entity_mut(p) {
                    *dest = m;
                    self.ae2_mark_modified(p);
                }
            }
            if let Some(BlockEntity::Ae2(machine)) = self.world.block_entities.get_mut(&pos) {
                write_fluids(machine, &fluids)?;
                write_session(machine, &session, &physical)?;
                self.world.modified.insert((pos.0 >> 4, pos.2 >> 4));
            }
            Ok(())
        }
    }
    #[cfg(test)]
    mod address_migration_tests {
        use super::*;
        #[test]
        fn bulk_processing_escrow_takes_exact_physical_counts_and_preserves_shortage() {
            let mut variant=ItemStack::new("minecraft:stone",64);
            variant.data_mut().set_other("minecraft:custom_data",Some(Tag::compound([("variant",Tag::String("kept".into()))])));
            let key=canon(&variant);
            let unrelated=ItemStack::new("minecraft:diamond",9);
            let mut physical=vec![variant.clone();8193];physical.insert(2,unrelated.clone());
            let requested=524_289u64;
            let taken=take_physical_many(&mut physical,&key,requested).unwrap().unwrap();
            assert_eq!(taken.len(),1);assert_eq!(u64::from(taken[0].count),requested);
            assert!(taken[0].same_item_same_components(&variant));
            assert_eq!(physical,vec![unrelated,ItemStack{count:63,..variant}]);
            let before=physical.clone();
            assert!(take_physical_many(&mut physical,&key,64).unwrap().is_none());
            assert_eq!(physical,before);
        }
        #[test]
        fn large_physical_cpu_reservation_survives_restart_and_cancellation() {
            let mut item=ItemStack::new("minecraft:stone",1);
            item.data_mut().set_other("minecraft:custom_data",Some(Tag::compound([("variant",Tag::String("exact".into()))])));
            let key=canon(&item);let target=canon(&ItemStack::new("minecraft:diamond",1));
            let mut registry=ExtendedRegistry::default();
            registry.register(ExtendedPattern{id:"large".into(),inputs:vec![Ingredient{choices:Inventory::from([(key.clone(),999_999)])}],outputs:Inventory::from([(target.clone(),1)])}).unwrap();
            let mut available=Inventory::from([(key.clone(),999_999)]);let plan=registry.plan(&available,&target,1).unwrap();
            let mut cpu=Cpu::new(2_000_000,0);cpu.submit(plan,&mut available).unwrap();assert!(available.is_empty());
            let mut session=Session{cpu,suspended:false,specs:BTreeMap::new(),flights:BTreeMap::new(),job_id:Some([3,4]),address_version:1,requester:None,export_requester:None,player_owner:None};
            item.count=999_999;let mut machine=Machine::new(MachineKind::CraftingStorage256k);
            write_session(&mut machine,&session,&[item.clone()]).unwrap();
            let bytes=horizon_world::nbt::write_network(&Tag::Compound(machine.config.clone()),Default::default()).unwrap();
            let (Tag::Compound(config),_)=horizon_world::nbt::read_network(&bytes,Default::default()).unwrap() else{panic!("compound config")};machine.config=config;
            session=read_session(&machine,super::super::super::Dim::Overworld).unwrap();assert_eq!(session.cpu.escrow(),&Inventory::from([(key,999_999)]));
            assert_eq!(read_escrow(&machine).unwrap(),vec![item.clone()]);session.cpu.cancel().unwrap();
            write_session(&mut machine,&session,&[item.clone()]).unwrap();assert_eq!(read_escrow(&machine).unwrap(),vec![item]);
        }
        #[test]
        fn processing_inputs_recover_source_sparse_mixed_order() {
            let water=ae2_cell_core::ItemKey{item_id:"minecraft:water".into(),components:vec![]};
            let mut input=vec![None;81];input[1]=Some(ItemStack::new("minecraft:iron_ingot",3));input[3]=Some(ItemStack::new("minecraft:iron_ingot",2));
            let mut fluid=vec![None;81];fluid[0]=Some(ae2_processing::PatternFluid{key:water.clone(),amount:1000});fluid[2]=Some(ae2_processing::PatternFluid{key:water.clone(),amount:500});
            let mut output=vec![None;27];output[0]=Some(ItemStack::new("minecraft:diamond",1));
            let pattern=ae2_processing::ae2_encode_processing_positioned(&input,&output,&fluid,&vec![None;27]).unwrap();
            let iron=canon(input[1].as_ref().unwrap());let water=fluid_name(&water);
            let expected=vec![(water.clone(),1000),(iron.clone(),3),(water.clone(),500),(iron.clone(),2)];
            assert_eq!(checked_processing_input_order(&pattern,&Inventory::from([(water.clone(),1500),(iron.clone(),5)])).unwrap(),expected);
            assert!(checked_processing_input_order(&pattern,&Inventory::from([(water,1500),(iron,4)])).is_err());
        }
        #[test]
        fn cpu_unloaded_waiting_owner_blocks_matching_key_but_not_unrelated_insertion() {
            let waiting=Inventory::from([("item:expected".to_owned(),2)]);
            assert!(unloaded_waiting_conflicts(&waiting,None));
            assert!(unloaded_waiting_conflicts(&waiting,Some("item:expected")));
            assert!(!unloaded_waiting_conflicts(&waiting,Some("item:unrelated")));
            assert!(!unloaded_waiting_conflicts(&Inventory::new(),Some("item:unrelated")));
        }
        #[test]
        fn positioned_fluid_primary_controls_provider_spec() {
            let fluid = ae2_processing::PatternFluid {
                key: ae2_cell_core::ItemKey { item_id: "minecraft:water".into(), components: vec![1, 2] }, amount: 1000,
            };
            let mut inputs = vec![None; 81]; inputs[1] = Some(ItemStack::new("minecraft:iron_ingot", 1));
            let mut outputs = vec![None; 27]; outputs[4] = Some(ItemStack::new("minecraft:diamond", 3));
            let fluid_inputs = vec![None; 81]; let mut fluid_outputs = vec![None; 27]; fluid_outputs[0] = Some(fluid);
            let pattern = ae2_processing::ae2_encode_processing_positioned(&inputs, &outputs, &fluid_inputs, &fluid_outputs).unwrap();
            let (_, decoded_outputs) = ae2_processing::ae2_decode_processing_pattern(&pattern).unwrap();
            let (_, decoded_fluids) = ae2_processing::ae2_decode_processing_fluids(&pattern).unwrap();
            let (primary, extras) = processing_spec_outputs(&pattern, &decoded_outputs, &decoded_fluids).unwrap();
            assert_eq!(primary, "fluid|minecraft:water|0102");
            assert_eq!(extras, vec![item_data::canon_stack(outputs[4].as_ref())]);
        }
        #[test]
        fn legacy_named_job_normalizes_keys_against_physical_escrow() {
            let mut item=ItemStack::new("minecraft:diamond_sword",1);item.damage=5;item.data_mut().custom_name=Some("Relic".into());
            let modern=canon(&item);let old=modern.replace("\"minecraft:damage\"","minecraft:damage").replace("\"minecraft:custom_name\"","minecraft:custom_name");assert_ne!(modern,old);
            let result=canon(&ItemStack::new("minecraft:netherite_sword",1));
            let mut registry=ExtendedRegistry::default();registry.register(ExtendedPattern{id:"legacy".into(),inputs:vec![Ingredient{choices:Inventory::from([(old.clone(),1)])}],outputs:Inventory::from([(result.clone(),1)])}).unwrap();
            let mut inventory=Inventory::from([(old.clone(),1)]);let plan=registry.plan(&inventory,&result,1).unwrap();let mut cpu=Cpu::new(1024,0);cpu.submit(plan,&mut inventory).unwrap();
            let session=Session{cpu,suspended:false,specs:BTreeMap::new(),flights:BTreeMap::new(),job_id:None,address_version:1,requester:None,export_requester:None,player_owner:None};
            let mut machine=Machine::new(MachineKind::CraftingStorage1k);let json=serde_json::to_string(&session).unwrap();machine.config.insert(JOB.into(),Tag::String(json.as_str().into()));machine.config.insert(ESCROW.into(),Tag::List(vec![horizon_client::save::item_to_nbt(&item)]));
            let loaded=read_session(&machine,super::super::super::Dim::Overworld).unwrap();assert_eq!(loaded.cpu.escrow(),&Inventory::from([(modern,1)]));
        }
        #[test]
        fn emitted_waiting_job_round_trips_only_real_escrow_and_clears_on_cancel() {
            let input=ItemStack::new("minecraft:oak_log",1);
            let input_key=canon(&input);let output_key=canon(&ItemStack::new("minecraft:oak_planks",1));
            let mut registry=ExtendedRegistry::default();
            registry.register(ExtendedPattern{id:"p".into(),inputs:vec![Ingredient{choices:Inventory::from([(input_key.clone(),1)])}],
                outputs:Inventory::from([(output_key.clone(),4)])}).unwrap();
            registry.set_emittable(vec![input_key.clone()]).unwrap();
            let mut available=Inventory::new();let plan=registry.plan(&available,&output_key,4).unwrap();
            let mut cpu=Cpu::new(1024,0);cpu.submit(plan,&mut available).unwrap();
            let mut session=Session{cpu,suspended:false,specs:BTreeMap::new(),flights:BTreeMap::new(),job_id:None,address_version:1,requester:None,export_requester:None,player_owner:None};
            let mut machine=Machine::new(MachineKind::CraftingStorage1k);
            write_fluids(&mut machine,&BTreeMap::new()).unwrap();write_session(&mut machine,&session,&[]).unwrap();
            session=read_session(&machine,super::super::super::Dim::Overworld).unwrap();
            assert_eq!(session.cpu.waiting_for().unwrap(),Inventory::from([(input_key.clone(),1)]));
            assert!(read_escrow(&machine).unwrap().is_empty());
            session.cpu.receive_emitted(&input_key,1).unwrap();write_session(&mut machine,&session,&[input.clone()]).unwrap();
            session=read_session(&machine,super::super::super::Dim::Overworld).unwrap();
            assert!(session.cpu.waiting_for().unwrap().is_empty());assert_eq!(read_escrow(&machine).unwrap(),vec![input.clone()]);
            session.cpu.cancel().unwrap();write_session(&mut machine,&session,&[input.clone()]).unwrap();
            assert!(read_session(&machine,super::super::super::Dim::Overworld).unwrap().cpu.waiting_for().unwrap().is_empty());
            assert_eq!(read_escrow(&machine).unwrap(),vec![input]);
        }
        #[test]
        fn assembler_flight_identity_rejects_reused_cpu_or_wrong_flight() {
            let cpu =
                GridAddress::from_local(super::super::super::Dim::Overworld, (1, 60, 2)).unwrap();
            let assembler =
                GridAddress::from_local(super::super::super::Dim::Overworld, (2, 60, 2)).unwrap();
            let owner = AssemblerFlightOwner {
                cpu,
                job_id: [42, 73],
                flight: 7,
            };
            let mut session = Session {
                cpu: Cpu::new(1024, 0),
                suspended: false,
                specs: BTreeMap::new(),
                flights: BTreeMap::from([(7, assembler)]),
                job_id: Some([42, 73]),
                address_version: 1,
                requester: None,
                export_requester: None,
                player_owner: None,
            };
            assert!(matching_assembler_flight(&owner, &session, assembler));
            session.job_id = Some([42, 74]);
            assert!(!matching_assembler_flight(&owner, &session, assembler));
            session.job_id = Some([42, 73]);
            session.flights.clear();
            assert!(!matching_assembler_flight(&owner, &session, assembler));
            session.flights.insert(7, cpu);
            assert!(!matching_assembler_flight(&owner, &session, assembler));
        }
        #[test]
        fn suspended_job_roundtrips_and_legacy_jobs_resume_by_default() {
            let session=Session { cpu:Cpu::new(1024,0), suspended:true, specs:BTreeMap::new(), flights:BTreeMap::new(), job_id:Some([1,2]), address_version:1, requester:None, export_requester:None, player_owner:None };
            let mut saved=serde_json::to_value(&session).unwrap();
            assert!(serde_json::from_value::<Session>(saved.clone()).unwrap().suspended);
            saved.as_object_mut().unwrap().remove("suspended");
            assert!(!serde_json::from_value::<Session>(saved).unwrap().suspended);
        }
        #[test]
        fn player_job_events_preserve_owner_output_and_legacy_saves() {
            use horizon_client::ae2_job_status::Status;
            let owner=0x0123_4567_89ab_cdef_fedc_ba98_7654_3210u128;
            let mut registry=ExtendedRegistry::default();
            registry.register(ExtendedPattern {
                id:"boards".into(),
                inputs:vec![Ingredient{choices:Inventory::from([("wood".into(),1)])}],
                outputs:Inventory::from([("board".into(),4)]),
            }).unwrap();
            let mut available=Inventory::from([("wood".into(),2)]);
            let plan=registry.plan(&available,"board",8).unwrap();
            let mut cpu=Cpu::new(1024,0);cpu.submit(plan,&mut available).unwrap();
            let mut session=Session {
                cpu,suspended:false,specs:BTreeMap::new(),flights:BTreeMap::new(),
                job_id:Some([i64::MIN,i64::MAX]),address_version:1,requester:None,
                export_requester:None,player_owner:Some(super::super::super::ae2_job_events::owner_words(owner)),
            };
            let (recipient,started)=job_status(&session,Status::Started).unwrap();
            assert_eq!(recipient,owner);
            assert_eq!(started.job_id,[i64::MIN,i64::MAX]);
            assert_eq!((&*started.key,started.requested,started.remaining,started.status),("board",8,8,Status::Started));
            let mut saved=serde_json::to_value(&session).unwrap();
            let loaded:Session=serde_json::from_value(saved.clone()).unwrap();
            assert_eq!(job_status(&loaded,Status::Started).unwrap().0,owner);
            saved.as_object_mut().unwrap().remove("player_owner");
            assert!(job_status(&serde_json::from_value::<Session>(saved).unwrap(),Status::Started).is_none());
            let (flight,_)=session.cpu.dispatch().unwrap();
            session.cpu.receive(flight,"board",4).unwrap();
            session.cpu.cancel().unwrap();
            let (_,cancelled)=job_status(&session,Status::Cancelled).unwrap();
            assert_eq!((cancelled.requested,cancelled.remaining,cancelled.status),(8,4,Status::Cancelled));
        }
        #[test]
        fn cpu_selection_policy_filters_and_prefers_the_request_source() {
            assert_eq!(selection_policy(Some("ANY"),false),Some(false));
            assert_eq!(selection_policy(Some("ANY"),true),Some(false));
            assert_eq!(selection_policy(Some("PLAYER_ONLY"),false),Some(true));
            assert_eq!(selection_policy(Some("PLAYER_ONLY"),true),None);
            assert_eq!(selection_policy(Some("MACHINE_ONLY"),true),Some(true));
            assert_eq!(selection_policy(Some("MACHINE_ONLY"),false),None);
            assert_eq!(selection_policy(None,false),None);
        }
        #[test]
        fn fluid_slots_follow_each_selected_flight_and_legacy_spec() {
            let resources=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../.local/resources/26.3");
            if !resources.join("items.json").exists(){return;}
            let items=horizon_client::items::Items::catalog(&resources).unwrap();
            let root=std::env::temp_dir().join(format!("ae2-fluid-grid-{}",std::process::id()));
            let dir=root.join("data/minecraft/recipe");std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("fluid_grid.json"),r#"{"type":"minecraft:crafting_shapeless","ingredients":["minecraft:water_bucket","minecraft:water_bucket"],"result":{"id":"minecraft:diamond","count":1}}"#).unwrap();
            let recipes=horizon_client::crafting::Recipes::load(&root,&items).unwrap();
            let stations=horizon_client::stations::StationData::load(&resources,&items);
            let water=fluid_name(&super::super::super::ae2_fluids::fluid_key("minecraft:water"));
            let filled=canon(&ItemStack::new("minecraft:water_bucket",1));let empty=canon(&ItemStack::new("minecraft:bucket",1));
            let result=canon(&ItemStack::new("minecraft:diamond",1));let mut grid=vec!["-".to_string();9];grid[0]=filled.clone();grid[1]=filled.clone();
            let mut choices=vec![Vec::new();9];choices[0]=vec![water.clone(),filled.clone()];choices[1]=choices[0].clone();
            let spec=Spec{provider:ProviderOwner::Block(GridAddress::from_local(super::super::super::Dim::Overworld,(0,64,0)).unwrap()),slot:0,definition:None,
                crafting_recipe:Some("minecraft:fluid_grid".into()),grid,choices,fluid_slots:BTreeMap::from([(0,(water.clone(),1000)),(1,(water.clone(),1000))]),
                processing:false,special:None,special_grid_version:0,fluid_inputs:Vec::new(),fluid_outputs:Vec::new(),extra_outputs:Vec::new(),output:result.clone()};
            let step=ae2_crafting_core::extended::Step{pattern:"p".into(),inputs:Inventory::from([(water.clone(),1000),(filled.clone(),1)]),outputs:Inventory::from([(result.clone(),1),(empty,1)]),runs:1};
            let (grid,fluids)=choose_grid(&spec,&step,&recipes,&stations).unwrap();
            assert_eq!(grid[0].as_ref().unwrap().item.as_ref(),"minecraft:water_bucket");
            assert_eq!(grid[1].as_ref().unwrap().item.as_ref(),"minecraft:water_bucket");
            assert_eq!(fluids,BTreeMap::from([(0,(water.clone(),1000))]));
            let mut legacy:Spec=serde_json::from_str(&serde_json::to_string(&spec).unwrap()).unwrap();
            legacy.choices[0]=vec![water.clone()];legacy.choices[1]=vec![filled];legacy.fluid_slots.remove(&1);
            let (_,fluids)=choose_grid(&legacy,&step,&recipes,&stations).unwrap();assert_eq!(fluids,BTreeMap::from([(0,(water.clone(),1000))]));
            let mut value=serde_json::to_value(&legacy).unwrap();value.as_object_mut().unwrap().remove("choices");
            let oldest:Spec=serde_json::from_value(value).unwrap();
            let (_,fluids)=choose_grid(&oldest,&step,&recipes,&stations).unwrap();assert_eq!(fluids,BTreeMap::from([(0,(water,1000))]));
            std::fs::remove_dir_all(root).unwrap();
        }
        #[test]
        fn nonunit_source_crafting_never_partially_moves_escrow_into_assembler() {
            let resources=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../.local/resources/26.3");
            if !resources.join("items.json").exists(){return;}
            let items=horizon_client::items::Items::catalog(&resources).unwrap();
            let root=std::env::temp_dir().join(format!("ae2-nonunit-grid-{}",std::process::id()));
            let dir=root.join("data/minecraft/recipe");std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("nonunit.json"),r#"{"type":"minecraft:crafting_shapeless","ingredients":["minecraft:oak_log"],"result":{"id":"minecraft:oak_planks","count":4}}"#).unwrap();
            let recipes=horizon_client::crafting::Recipes::load(&root,&items).unwrap();let stations=horizon_client::stations::StationData::load(&resources,&items);
            let input=canon(&ItemStack::new("minecraft:oak_log",1));let output=canon(&ItemStack::new("minecraft:oak_planks",4));
            let mut grid=vec!["-".to_string();9];grid[0]=input.clone();let mut choices=vec![Vec::new();9];choices[0]=vec![input.clone()];
            let mut recipe_grid=vec![None;9];recipe_grid[0]=Some(ItemStack::new("minecraft:oak_log",1));
            assert_eq!(source_crafting_templates(&recipes,"minecraft:nonunit",&recipe_grid,0,&choices[0],&input,2,true),vec![(input.clone(),2),(input.clone(),1)]);
            let spec=Spec{provider:ProviderOwner::Block(GridAddress::from_local(super::super::super::Dim::Overworld,(0,64,0)).unwrap()),slot:0,definition:None,
                crafting_recipe:Some("minecraft:nonunit".into()),grid,choices,fluid_slots:BTreeMap::new(),processing:false,special:None,special_grid_version:0,
                fluid_inputs:Vec::new(),fluid_outputs:Vec::new(),extra_outputs:Vec::new(),output:item_data::canon_stack(Some(&ItemStack::new("minecraft:oak_planks",4)))};
            let mut step=ae2_crafting_core::extended::Step{pattern:"p".into(),inputs:Inventory::from([(input.clone(),4)]),outputs:Inventory::from([(output,4)]),runs:1};
            let before=step.clone();assert!(choose_grid(&spec,&step,&recipes,&stations).is_none());assert_eq!(step,before);
            step.inputs.insert(input,1);assert!(choose_grid(&spec,&step,&recipes,&stations).is_some());
            std::fs::remove_dir_all(root).unwrap();
        }
        #[test]
        fn legacy_job_identity_migration_is_persistable_and_never_rotates() {
            let mut session=Session {cpu:Cpu::new(1024,0),suspended:false,specs:BTreeMap::new(),flights:BTreeMap::new(),job_id:None,address_version:1,requester:None,export_requester:None,player_owner:None};
            assert!(migrate_job_identity(&mut session,[13,17]));
            let mut restored:Session=serde_json::from_str(&serde_json::to_string(&session).unwrap()).unwrap();
            assert!(!migrate_job_identity(&mut restored,[19,23]));
            assert_eq!(restored.job_id,Some([13,17]));
        }
        #[test]
        fn large_cpu_jobs_round_trip_nbt_without_the_string_limit() {
            assert!(matches!(job_payload("x".repeat(65_535)).unwrap(),Tag::String(_)));
            assert!(matches!(job_payload("x".repeat(65_536)).unwrap(),Tag::ByteArray(_)));
            // One Unicode scalar becomes two modified-UTF-8 encoded UTF-16
            // units; ordinary UTF-8 length alone undercounts this boundary.
            assert!(matches!(job_payload("😀".repeat(10_923)).unwrap(),Tag::ByteArray(_)));
            let provider=ProviderOwner::Block(GridAddress::from_local(super::super::super::Dim::Overworld,(0,64,0)).unwrap());
            let mut inputs=Vec::new();let mut stock=Inventory::new();
            for i in 0..4097 {
                let mut input=ItemStack::new("minecraft:stone",1);input.data_mut().custom_name=Some(format!("variant-{i:04}"));
                stock.insert(canon(&input),1);inputs.push(input);
            }
            let primary=stock.keys().next().unwrap().clone();let result=canon(&ItemStack::new("minecraft:diamond",1));
            let mut grid=vec!["-".to_string();9];grid[0]=primary.clone();let mut choices=vec![Vec::new();9];choices[0]=stock.keys().cloned().collect();
            let spec=Spec{provider,slot:0,definition:None,crafting_recipe:None,grid,choices,fluid_slots:BTreeMap::new(),
                processing:false,special:None,special_grid_version:0,fluid_inputs:Vec::new(),fluid_outputs:Vec::new(),extra_outputs:Vec::new(),
                output:result.clone()};
            let specs=BTreeMap::from([("pattern".into(),spec)]);
            let mut registry=ExtendedRegistry::default();
            registry.register(ExtendedPattern{id:"pattern".into(),inputs:vec![Ingredient{choices:stock.clone()}],outputs:Inventory::from([(result.clone(),1)])}).unwrap();
            registry.set_source_inputs("pattern",vec![SourceInput{encoded:primary.clone(),primary,remaining:BTreeMap::new()}]).unwrap();
            let mut network=stock.clone();let plan=registry.plan(&network,&result,4097).unwrap();assert_eq!(plan.steps.len(),4097);
            let mut cpu=Cpu::new(16384,0);cpu.submit(plan,&mut network).unwrap();assert!(network.is_empty());
            let session=Session{cpu,suspended:false,specs,flights:BTreeMap::new(),job_id:Some([7,11]),address_version:1,requester:None,export_requester:None,player_owner:None};
            let mut machine=Machine::new(MachineKind::CraftingStorage16k);write_session(&mut machine,&session,&inputs).unwrap();
            assert!(matches!(machine.config.get(&JOB.into()),Some(Tag::ByteArray(v)) if v.len()>65_535));
            let root=Tag::Compound(machine.config.clone());
            let bytes=horizon_world::nbt::write_network(&root,horizon_world::nbt::Limits::default()).unwrap();
            let (Tag::Compound(config),_)=horizon_world::nbt::read_network(&bytes,horizon_world::nbt::Limits::default()).unwrap() else{panic!("compound config")};
            machine.config=config;
            let loaded=read_session(&machine,super::super::super::Dim::Overworld).unwrap();
            assert_eq!(loaded.specs.len(),1);assert_eq!(loaded.job_id,Some([7,11]));assert_eq!(loaded.cpu.pending().len(),4097);assert_eq!(loaded.cpu.escrow(),&stock);
            assert_eq!(read_escrow(&machine).unwrap().len(),4097);
        }
        #[test]
        fn provider_owner_keeps_legacy_block_and_separate_part_faces() {
            let addr =
                GridAddress::from_local(super::super::super::Dim::Nether, (3, 64, -9)).unwrap();
            let legacy = serde_json::to_value(addr).unwrap();
            assert_eq!(
                serde_json::from_value::<ProviderOwner>(legacy).unwrap(),
                ProviderOwner::Block(addr)
            );
            for face in [0, 5] {
                let owner = ProviderOwner::Part { host: addr, face };
                let data = serde_json::to_string(&owner).unwrap();
                assert_eq!(serde_json::from_str::<ProviderOwner>(&data).unwrap(), owner);
            }
            assert_ne!(
                ProviderOwner::Part {
                    host: addr,
                    face: 0
                }
                .id(),
                ProviderOwner::Part {
                    host: addr,
                    face: 5
                }
                .id()
            );
            assert_ne!(
                ProviderOwner::Block(addr).id(),
                ProviderOwner::Part {
                    host: addr,
                    face: 0
                }
                .id()
            );
        }
        #[test]
        fn legacy_job_positions_migrate_in_cpu_dimension_without_changing_items() {
            let local = (3, 64, -9);
            let legacy =
                GridAddress::from_local(super::super::super::Dim::Overworld, local).unwrap();
            let spec = Spec {
                provider: ProviderOwner::Block(legacy),
                slot: 0,
                definition: None,
                crafting_recipe: None,
                grid: vec![],
                choices: vec![],
                fluid_slots: BTreeMap::new(),
                processing: false,
                        special:None,
                        special_grid_version:0,
                fluid_inputs: vec![],
                fluid_outputs: vec![],
                extra_outputs: vec![],
                output: canon(&ItemStack::new("minecraft:stone",1)),
            };
            let session = Session {
                cpu: Cpu::new(1024, 0),
                suspended: false,
                specs: BTreeMap::from([("pattern".into(), spec)]),
                flights: BTreeMap::from([(4, legacy)]),
                job_id: None,
                address_version: 0,
                requester: None,
                export_requester: None,
                player_owner: None,
            };
            let mut machine = Machine::new(MachineKind::CraftingStorage1k);
            let mut value = serde_json::to_value(&session).unwrap();
            value.as_object_mut().unwrap().remove("address_version");
            value["specs"]["pattern"].as_object_mut().unwrap().remove("crafting_recipe");
            let json = serde_json::to_string(&value).unwrap();
            machine
                .config
                .insert(JOB.into(), Tag::String(json.as_str().into()));
            let loaded = read_session(&machine, super::super::super::Dim::Nether).unwrap();
            let expected =
                GridAddress::from_local(super::super::super::Dim::Nether, local).unwrap();
            assert_eq!(
                loaded.specs["pattern"].provider,
                ProviderOwner::Block(expected)
            );
            assert_eq!(loaded.flights[&4], expected);
            assert_eq!(loaded.address_version, 1);
            assert!(read_escrow(&machine).unwrap().is_empty());
        }
    }
}

#[cfg(test)]
mod provider_push_direction_tests {
    use super::provider_push_direction;
    use horizon_core::geometry::Direction;
    use horizon_world::chunk::BlockState;
    #[test]
    fn source_push_direction_limits_target_face_and_rejects_invalid_values() {
        let mut state=BlockState{name:"ae2:pattern_provider".into(),properties:Default::default()};
        assert_eq!(provider_push_direction(&state),Ok(None));
        for (value,expected) in [("all",None),("down",Some(Direction::Down)),("up",Some(Direction::Up)),("north",Some(Direction::North)),("south",Some(Direction::South)),("west",Some(Direction::West)),("east",Some(Direction::East))] {
            state.properties.insert("push_direction".into(),value.into());assert_eq!(provider_push_direction(&state),Ok(expected));
        }
        state.properties.insert("push_direction".into(),"bad".into());assert_eq!(provider_push_direction(&state),Err(()));
    }
}
