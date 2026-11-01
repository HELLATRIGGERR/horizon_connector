//! Derived from MEGA Cells 4.11.0 `LavaTransformLogic` and `ItemEntityMixin`
//! (LGPL-3.0), https://github.com/62832/MEGACells tag 4.11.0.
//!
//! MEGA's Sky Steel and Sky Bronze come from `ae2:transform` recipes in lava.
//! An item in lava burns in two ticks, so MEGA keeps the ingredients of such
//! a recipe fire immune: up to 200 ticks in lava, long enough for AE2's
//! 60-tick transformation, and again from the start while all the
//! ingredients of a lava recipe lie together.
use crate::game::Game;
use horizon_client::{blocks::Fluid, entities::ItemEntity};
use horizon_core::geometry::Vec3;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

/// `mega$lavaTicks` and `mega$lavaImmune`, per item entity of a level. Not
/// saved, as the mixin's fields are not.
type Key = (PathBuf, &'static str, u32);
static STATE: OnceLock<Mutex<HashMap<Key, (u32, bool)>>> = OnceLock::new();

/// How long an ingredient stays immune in lava, in ticks.
const IMMUNE_TICKS: u32 = 200;

/// The ingredients of the `ae2:transform` recipes whose circumstance is lava
/// (`getLavaTransformableItems`), read once per tick of the items.
pub(in crate::game) struct LavaRecipes(Vec<Vec<HashSet<String>>>);
impl LavaRecipes {
    pub(in crate::game) fn new(
        recipes: &horizon_client::crafting::Recipes,
        items: &horizon_client::items::Items,
    ) -> Self {
        Self(
            horizon_client::ae2_transform::rules(recipes, items)
                .into_iter()
                .filter(|rule| !rule.explosion() && rule.fluid("minecraft:lava"))
                .map(|rule| rule.ingredients().to_vec())
                .collect(),
        )
    }
    /// `canTransformInLava`.
    fn transformable(&self, id: &str) -> bool {
        self.0.iter().flatten().any(|ingredient| ingredient.contains(id))
    }
    /// `allIngredientsPresent`: the items of the entities within a block of
    /// `items[at]` cover every ingredient of a lava recipe. MEGA answers for
    /// the first lava recipe the recipe manager lists, in no fixed order;
    /// this answers for any of them.
    fn all_present(&self, items: &[ItemEntity], at: usize) -> bool {
        let p = items[at].position;
        let reach = Vec3::new(1.0, 1.0, 1.0);
        let (min, max) = (p.subtract(reach), p.add_vector(reach));
        let near: HashSet<&str> = items
            .iter()
            .filter(|other| other.stack.count > 0 && other.bounds().intersects_corners(min, max))
            .map(|other| other.stack.item.as_ref())
            .collect();
        self.0.iter().any(|recipe| {
            recipe
                .iter()
                .all(|ingredient| ingredient.iter().any(|id| near.contains(id.as_str())))
        })
    }
}

/// One tick of `ItemEntityMixin.lavaTimeout` for an item that may be in lava.
/// `in_lava`: the fluid at the middle of its box is lava. Returns whether the
/// item is fire immune; the flag keeps its value once the item leaves lava.
fn step(state: &mut (u32, bool), in_lava: bool, all_present: impl FnOnce() -> bool) -> bool {
    if in_lava {
        state.1 = state.0 <= IMMUNE_TICKS;
        let before = state.0;
        state.0 += 1;
        if before > IMMUNE_TICKS && all_present() {
            state.0 = 0;
        }
    }
    state.1
}

impl Game<'_> {
    /// Whether `items[at]` is fire immune this tick by MEGA's lava rule. Call
    /// once per item and tick, after it moved.
    pub(in crate::game) fn megacells_lava_immune(
        &self,
        lava: &LavaRecipes,
        items: &[ItemEntity],
        at: usize,
    ) -> bool {
        let Ok(mut map) = STATE.get_or_init(|| Mutex::new(HashMap::new())).lock() else {
            return false;
        };
        let scope = (self.world_root.clone(), self.dim.id());
        if at == 0 {
            let live: HashSet<u32> = items.iter().map(|item| item.id).collect();
            map.retain(|(root, dim, id), _| root != &scope.0 || *dim != scope.1 || live.contains(id));
        }
        let item = &items[at];
        if item.stack.count == 0 || !lava.transformable(&item.stack.item) {
            return map.get(&(scope.0, scope.1, item.id)).is_some_and(|state| state.1);
        }
        let p = item.position;
        let block = self.world.block(
            p.x.floor() as i32,
            (p.y + ItemEntity::WIDTH / 2.0).floor() as i32,
            p.z.floor() as i32,
        );
        let in_lava = self.res.facts().get(block).fluid == Fluid::Lava;
        let state = map.entry((scope.0, scope.1, item.id)).or_insert((0, false));
        step(state, in_lava, || lava.all_present(items, at))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn immunity_lasts_two_hundred_ticks_in_lava_and_restarts_with_all_ingredients() {
        let mut state = (0, false);
        assert!(!step(&mut state, false, || unreachable!()));
        for tick in 0..=IMMUNE_TICKS {
            assert!(step(&mut state, true, || unreachable!()), "tick {tick}");
        }
        // Tick 201: the counter is past 200, so immunity ends; the
        // ingredients are not all there, so it does not start again.
        assert!(!step(&mut state, true, || false));
        assert!(!step(&mut state, true, || false));
        assert_eq!(state.0, IMMUNE_TICKS + 3);
        // With every ingredient present the counter starts over: immune from
        // the next tick.
        assert!(!step(&mut state, true, || true));
        assert_eq!(state.0, 0);
        assert!(step(&mut state, true, || unreachable!()));
        // Out of lava the flag keeps its last value, as the mixin's does.
        assert!(step(&mut state, false, || unreachable!()));
    }

    #[test]
    fn lava_recipes_know_their_ingredients_and_need_all_of_them_near() {
        let lava = LavaRecipes(vec![vec![
            HashSet::from(["ae2:charged_certus_quartz_crystal".to_owned()]),
            HashSet::from(["minecraft:iron_ingot".to_owned()]),
            HashSet::from(["ae2:sky_stone_block".to_owned()]),
        ]]);
        assert!(lava.transformable("minecraft:iron_ingot"));
        assert!(!lava.transformable("minecraft:gold_ingot"));
        let entity = |id: u32, item: &str, x: f64| ItemEntity {
            target: None,
            health: 5,
            fire_ticks: 0,
            id,
            stack: horizon_client::items::ItemStack::new(item, 1),
            thrower: None,
            position: Vec3::new(x, 64.0, 0.5),
            previous: Vec3::new(x, 64.0, 0.5),
            velocity: Vec3::ZERO,
            on_ground: false,
            age: 0,
            pickup_delay: 0,
            bob_offset: 0.0,
            ticks: 0,
        };
        let mut items = vec![
            entity(1, "ae2:charged_certus_quartz_crystal", 0.5),
            entity(2, "minecraft:iron_ingot", 0.9),
            entity(3, "ae2:sky_stone_block", 1.4),
        ];
        assert!(lava.all_present(&items, 0));
        items[2].position.x = 1.7;
        assert!(!lava.all_present(&items, 0), "the sky stone is more than a block away");
        assert!(lava.all_present(&items, 1), "but within a block of the iron");
    }
}
