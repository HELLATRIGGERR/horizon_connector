//! O(1) block-state property access over the pinned registry.
//!
//! Vanilla's `StateDefinition` enumerates a block's states as the cartesian
//! product of its properties sorted by name, the last property varying fastest,
//! and the global state registry lists every block's states contiguously. A
//! content mod's block lists its template's states times the combinations of
//! the properties it declares, so its properties are a cartesian product in
//! another order. The table below derives each property's stride from the
//! layout and verifies it against every registry descriptor at load, so
//! `setValue` is arithmetic.
use crate::{
    compression::invalid,
    registry::{BlockRegistry, BlockStateId},
};
use horizon_core::geometry::{Axis, Direction};
use std::{collections::HashMap, io};

/// Registry-order block index; equals vanilla's `Block` registry ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BlockId(pub u16);

/// Interned property name (`facing`, `waterlogged`, ...), shared by all blocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PropKey(pub u16);

#[derive(Clone, Debug)]
pub struct PropValue {
    pub text: Box<str>,
    pub int: Option<i32>,
    pub boolean: Option<bool>,
    pub direction: Option<Direction>,
}
impl PropValue {
    fn new(text: &str) -> Self {
        Self {
            int: text.parse().ok(),
            boolean: match text {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            },
            direction: direction_from_name(text),
            text: text.into(),
        }
    }
}

#[derive(Clone, Debug)]
struct Slot {
    key: PropKey,
    stride: u32,
    values: Vec<PropValue>,
}

#[derive(Clone, Debug)]
pub struct BlockInfo {
    pub name: Box<str>,
    pub base: BlockStateId,
    pub count: u32,
    pub default: BlockStateId,
    slots: Vec<Slot>,
}

pub struct StateTable {
    blocks: Vec<BlockInfo>,
    state_block: Vec<u16>,
    keys: HashMap<String, PropKey>,
    key_names: Vec<String>,
    by_name: HashMap<Box<str>, BlockId>,
}

pub fn direction_from_name(name: &str) -> Option<Direction> {
    Some(match name {
        "down" => Direction::Down,
        "up" => Direction::Up,
        "north" => Direction::North,
        "south" => Direction::South,
        "west" => Direction::West,
        "east" => Direction::East,
        _ => return None,
    })
}
pub const fn direction_name(direction: Direction) -> &'static str {
    match direction {
        Direction::Down => "down",
        Direction::Up => "up",
        Direction::North => "north",
        Direction::South => "south",
        Direction::West => "west",
        Direction::East => "east",
    }
}
pub const fn axis_name(axis: Axis) -> &'static str {
    match axis {
        Axis::X => "x",
        Axis::Y => "y",
        Axis::Z => "z",
    }
}

impl StateTable {
    pub fn new(registry: &BlockRegistry) -> io::Result<Self> {
        let mut table = Self {
            blocks: Vec::new(),
            state_block: Vec::with_capacity(registry.len()),
            keys: HashMap::new(),
            key_names: Vec::new(),
            by_name: HashMap::new(),
        };
        let mut index = 0;
        while index < registry.len() {
            let first = registry.state(BlockStateId(index as u32)).unwrap();
            let mut end = index + 1;
            while end < registry.len()
                && registry.state(BlockStateId(end as u32)).unwrap().name == first.name
            {
                end += 1;
            }
            let count = (end - index) as u32;
            // Each property's values in the order they first appear, and the
            // first offset where it leaves its first state's value: its
            // stride. Vanilla's blocks run their properties in name order;
            // a content mod's block runs its template's states times its own
            // properties' combinations, so its properties need not be in name
            // order. Any order of a cartesian product is taken, and checked
            // state by state below.
            let mut found = Vec::with_capacity(first.properties.len());
            for name in first.properties.keys() {
                let mut seen = Vec::<&str>::new();
                let mut changes = None;
                for offset in 0..count {
                    let value = registry
                        .state(BlockStateId(index as u32 + offset))
                        .unwrap()
                        .properties[name]
                        .as_str();
                    if !seen.contains(&value) {
                        if seen.len() == 1 {
                            changes = Some(offset);
                        }
                        seen.push(value);
                    }
                }
                found.push((name, seen, changes.unwrap_or(count)));
            }
            // Slowest first; ties (single-valued properties) keep name order.
            found.sort_by(|a, b| b.2.cmp(&a.2));
            let mut stride = count;
            let mut slots = Vec::with_capacity(found.len());
            for (name, seen, changes) in found {
                stride /= seen.len() as u32;
                if seen.len() > 1 && changes != stride {
                    return Err(invalid(
                        "registry state order differs from vanilla's state definition",
                    ));
                }
                let values = seen.into_iter().map(PropValue::new).collect();
                let key = table.key_or_insert(name);
                slots.push(Slot {
                    key,
                    stride,
                    values,
                });
            }
            if stride != 1 {
                return Err(invalid("block state count is not a property product"));
            }
            // Order of values: derive from the states at multiples of stride.
            for slot in &mut slots {
                let len = slot.values.len() as u32;
                let name = &table.key_names[slot.key.0 as usize];
                slot.values = (0..len)
                    .map(|i| {
                        PropValue::new(
                            &registry
                                .state(BlockStateId(index as u32 + i * slot.stride))
                                .unwrap()
                                .properties[name],
                        )
                    })
                    .collect();
            }
            let block =
                BlockId(u16::try_from(table.blocks.len()).map_err(|_| invalid("too many blocks"))?);
            let default = registry
                .default_state(&first.name)
                .ok_or_else(|| invalid("block without default state"))?;
            let info = BlockInfo {
                name: first.name.as_str().into(),
                base: BlockStateId(index as u32),
                count,
                default,
                slots,
            };
            for offset in 0..count {
                let state = registry.state(BlockStateId(index as u32 + offset)).unwrap();
                for slot in &info.slots {
                    let value =
                        &slot.values[((offset / slot.stride) % slot.values.len() as u32) as usize];
                    if *value.text != *state.properties[&table.key_names[slot.key.0 as usize]] {
                        return Err(invalid(
                            "registry state order differs from vanilla's state definition",
                        ));
                    }
                }
                table.state_block.push(block.0);
            }
            if table.by_name.insert(info.name.clone(), block).is_some() {
                return Err(invalid("block states are not contiguous"));
            }
            table.blocks.push(info);
            index = end;
        }
        Ok(table)
    }
    fn key_or_insert(&mut self, name: &str) -> PropKey {
        if let Some(key) = self.keys.get(name) {
            return *key;
        }
        let key = PropKey(self.key_names.len() as u16);
        self.key_names.push(name.to_owned());
        self.keys.insert(name.to_owned(), key);
        key
    }
    /// Property keys absent from every block resolve to an unused key, so
    /// lookups through them simply report "no such property".
    pub fn key(&mut self, name: &str) -> PropKey {
        self.key_or_insert(name)
    }
    /// An existing property key, without interning a new one.
    pub fn key_of(&self, name: &str) -> Option<PropKey> {
        self.keys.get(name).copied()
    }
    pub fn state_count(&self) -> usize {
        self.state_block.len()
    }
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }
    pub fn block(&self, state: BlockStateId) -> BlockId {
        BlockId(self.state_block[state.0 as usize])
    }
    pub fn info(&self, block: BlockId) -> &BlockInfo {
        &self.blocks[block.0 as usize]
    }
    pub fn name(&self, state: BlockStateId) -> &str {
        &self.info(self.block(state)).name
    }
    pub fn block_by_name(&self, name: &str) -> Option<BlockId> {
        self.by_name.get(name).copied()
    }
    pub fn default_state(&self, block: BlockId) -> BlockStateId {
        self.info(block).default
    }
    pub fn is(&self, state: BlockStateId, block: BlockId) -> bool {
        self.state_block[state.0 as usize] == block.0
    }
    fn slot(&self, state: BlockStateId, key: PropKey) -> Option<&Slot> {
        self.info(self.block(state))
            .slots
            .iter()
            .find(|slot| slot.key == key)
    }
    pub fn has(&self, state: BlockStateId, key: PropKey) -> bool {
        self.slot(state, key).is_some()
    }
    fn index_in(&self, state: BlockStateId, slot: &Slot) -> u32 {
        let base = self.info(self.block(state)).base.0;
        ((state.0 - base) / slot.stride) % slot.values.len() as u32
    }
    pub fn value(&self, state: BlockStateId, key: PropKey) -> Option<&PropValue> {
        let slot = self.slot(state, key)?;
        Some(&slot.values[self.index_in(state, slot) as usize])
    }
    pub fn text(&self, state: BlockStateId, key: PropKey) -> &str {
        self.value(state, key).map_or("", |v| &v.text)
    }
    /// Missing boolean properties read as false.
    pub fn bool(&self, state: BlockStateId, key: PropKey) -> bool {
        self.value(state, key)
            .and_then(|v| v.boolean)
            .unwrap_or(false)
    }
    /// Missing integer properties read as 0.
    pub fn int(&self, state: BlockStateId, key: PropKey) -> i32 {
        self.value(state, key).and_then(|v| v.int).unwrap_or(0)
    }
    pub fn dir(&self, state: BlockStateId, key: PropKey) -> Option<Direction> {
        self.value(state, key).and_then(|v| v.direction)
    }
    /// Number of values and minimum/maximum for integer properties.
    pub fn int_range(&self, state: BlockStateId, key: PropKey) -> Option<(i32, i32)> {
        let slot = self.slot(state, key)?;
        let ints = slot.values.iter().filter_map(|v| v.int);
        Some((ints.clone().min()?, ints.max()?))
    }
    pub fn values(&self, block: BlockId, key: PropKey) -> impl Iterator<Item = &PropValue> {
        self.info(block)
            .slots
            .iter()
            .filter(move |slot| slot.key == key)
            .flat_map(|slot| slot.values.iter())
    }
    fn with_index(&self, state: BlockStateId, slot: &Slot, index: u32) -> BlockStateId {
        let current = self.index_in(state, slot);
        BlockStateId(state.0 - current * slot.stride + index * slot.stride)
    }
    /// `setValue` by text; None if the block lacks the property or value.
    pub fn with_text(&self, state: BlockStateId, key: PropKey, text: &str) -> Option<BlockStateId> {
        let slot = self.slot(state, key)?;
        let index = slot.values.iter().position(|v| &*v.text == text)?;
        Some(self.with_index(state, slot, index as u32))
    }
    pub fn with_int(&self, state: BlockStateId, key: PropKey, value: i32) -> Option<BlockStateId> {
        let slot = self.slot(state, key)?;
        let index = slot.values.iter().position(|v| v.int == Some(value))?;
        Some(self.with_index(state, slot, index as u32))
    }
    pub fn with_bool(
        &self,
        state: BlockStateId,
        key: PropKey,
        value: bool,
    ) -> Option<BlockStateId> {
        let slot = self.slot(state, key)?;
        let index = slot.values.iter().position(|v| v.boolean == Some(value))?;
        Some(self.with_index(state, slot, index as u32))
    }
    pub fn with_dir(
        &self,
        state: BlockStateId,
        key: PropKey,
        value: Direction,
    ) -> Option<BlockStateId> {
        self.with_text(state, key, direction_name(value))
    }
    /// Vanilla `setValue` callers always pass legal values; when Horizon's port
    /// would compute an illegal one the state is returned unchanged.
    pub fn set_text(&self, state: BlockStateId, key: PropKey, text: &str) -> BlockStateId {
        self.with_text(state, key, text).unwrap_or(state)
    }
    pub fn set_int(&self, state: BlockStateId, key: PropKey, value: i32) -> BlockStateId {
        self.with_int(state, key, value).unwrap_or(state)
    }
    pub fn set_bool(&self, state: BlockStateId, key: PropKey, value: bool) -> BlockStateId {
        self.with_bool(state, key, value).unwrap_or(state)
    }
    pub fn set_dir(&self, state: BlockStateId, key: PropKey, value: Direction) -> BlockStateId {
        self.with_dir(state, key, value).unwrap_or(state)
    }
    /// `cycle`: the next value in definition order, wrapping around.
    pub fn cycle(&self, state: BlockStateId, key: PropKey) -> BlockStateId {
        let Some(slot) = self.slot(state, key) else {
            return state;
        };
        let next = (self.index_in(state, slot) + 1) % slot.values.len() as u32;
        self.with_index(state, slot, next)
    }
    /// `Block.withPropertiesOf`: copy every property the target block shares.
    pub fn with_properties_of(&self, target: BlockId, source: BlockStateId) -> BlockStateId {
        let mut result = self.default_state(target);
        for slot in &self.info(self.block(source)).slots {
            let value = &slot.values[self.index_in(source, slot) as usize];
            if let Some(next) = self.with_text(result, slot.key, &value.text) {
                result = next;
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A facts.json of air and `blocks`, each a name and its states'
    /// properties in registry order (the default first).
    fn registry(blocks: &[(&str, Vec<Vec<(&str, &str)>>)]) -> BlockRegistry {
        let mut rows = vec![serde_json::json!(["minecraft:air", true, false, "", "minecraft:air"])];
        let mut defaults = serde_json::Map::new();
        defaults.insert("minecraft:air".into(), 0.into());
        for (name, states) in blocks {
            defaults.insert((*name).into(), rows.len().into());
            for props in states {
                let props: serde_json::Map<_, _> =
                    props.iter().map(|(k, v)| ((*k).to_owned(), serde_json::json!(v))).collect();
                rows.push(serde_json::json!([name, false, false, "", {"id": name, "properties": props}]));
            }
        }
        let json = serde_json::json!({"defaults": defaults, "states": rows});
        BlockRegistry::from_facts_json(json.to_string().as_bytes()).unwrap()
    }

    /// A furnace-like template (`facing`, `lit`) times a mod's own `down` and
    /// `west`, template properties outermost: not name order.
    fn extended() -> Vec<Vec<(&'static str, &'static str)>> {
        let mut states = Vec::new();
        for facing in ["north", "south"] {
            for lit in ["false", "true"] {
                for down in ["input", "none", "output"] {
                    for west in ["output", "none"] {
                        states.push(vec![("facing", facing), ("lit", lit), ("down", down), ("west", west)]);
                    }
                }
            }
        }
        states
    }

    #[test]
    fn a_mod_block_may_extend_a_template_with_properties() {
        let registry = registry(&[("test:machine", extended())]);
        let mut table = StateTable::new(&registry).unwrap();
        let block = table.block_by_name("test:machine").unwrap();
        let base = table.info(block).base;
        let (facing, lit, down, west) = (table.key("facing"), table.key("lit"), table.key("down"), table.key("west"));
        assert_eq!(table.info(block).count, 24);
        // State 15: facing=south (12 on), lit=false, down=none (2 on), west=none.
        let s = BlockStateId(base.0 + 15);
        assert_eq!(
            [table.text(s, facing), table.text(s, lit), table.text(s, down), table.text(s, west)],
            ["south", "false", "none", "none"]
        );
        // setValue lands on the registry's own state for every change.
        for (key, value) in [(down, "output"), (west, "output"), (lit, "true"), (facing, "north")] {
            let next = table.with_text(s, key, value).unwrap();
            let mut want = registry.state(s).unwrap().clone();
            want.properties.insert(table.key_names[key.0 as usize].clone(), value.into());
            assert_eq!(registry.id(&want), Some(next), "{value}");
        }
    }

    #[test]
    fn a_layout_that_is_no_product_is_refused() {
        let mut states = extended();
        states.swap(0, 1);
        assert!(StateTable::new(&registry(&[("test:machine", states)])).is_err());
        let mut short = extended();
        short.pop();
        assert!(StateTable::new(&registry(&[("test:machine", short)])).is_err());
    }
}
