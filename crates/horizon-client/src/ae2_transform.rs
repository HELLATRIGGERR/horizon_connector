//! AE2 19.2.18 TransformRecipe / TransformLogic, LGPL-3.0-or-later.
//! Active recipe/tag data and source item-first greedy ingredient claiming.
use crate::{
    crafting::{Recipes, Template},
    entities::ItemEntity,
    items::{ItemStack, Items},
};
use horizon_world::nbt::Tag;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
#[derive(Clone, Debug)]
enum Circumstance {
    Explosion,
    Fluid(HashSet<String>),
}
#[derive(Clone, Debug)]
pub struct Rule {
    ingredients: Vec<HashSet<String>>,
    pub output: ItemStack,
    circumstance: Circumstance,
}
fn ingredient(value: &Value, items: &Items) -> Option<HashSet<String>> {
    match value {
        Value::String(id) => {
            if let Some(tag) = id.strip_prefix('#') {
                items.item_tags.get(tag).cloned()
            } else {
                Some(HashSet::from([id.clone()]))
            }
        }
        Value::Array(values) => {
            let mut out = HashSet::new();
            for value in values {
                out.extend(ingredient(value, items)?);
            }
            (!out.is_empty()).then_some(out)
        }
        Value::Object(_) => {
            if let Some(id) = value["item"].as_str() {
                Some(HashSet::from([id.into()]))
            } else {
                items.item_tags.get(value["tag"].as_str()?).cloned()
            }
        }
        _ => None,
    }
}
fn parse(
    value: &Value,
    items: &Items,
    fluid_tags: &HashMap<String, HashSet<String>>,
) -> Option<Rule> {
    let ingredients = value["ingredients"]
        .as_array()?
        .iter()
        .map(|v| ingredient(v, items))
        .collect::<Option<Vec<_>>>()?;
    if ingredients.is_empty() || ingredients.iter().any(HashSet::is_empty) {
        return None;
    }
    let output = Template::parse(&value["result"])?.create();
    if output.count == 0 {
        return None;
    }
    let circumstance = match value["circumstance"]["type"].as_str() {
        Some("explosion") => Circumstance::Explosion,
        None if value["circumstance"].is_null() => Circumstance::Fluid(
            fluid_tags
                .get("minecraft:water")
                .cloned()
                .unwrap_or_default(),
        ),
        Some("fluid") => Circumstance::Fluid(
            fluid_tags
                .get(value["circumstance"]["tag"].as_str()?)
                .cloned()
                .unwrap_or_default(),
        ),
        _ => return None,
    };
    Some(Rule {
        ingredients,
        output,
        circumstance,
    })
}
pub fn rules(recipes: &Recipes, items: &Items) -> Vec<Rule> {
    recipes
        .ae2_custom
        .get("ae2:transform")
        .into_iter()
        .flatten()
        .filter_map(|(_, recipe)| parse(recipe, items, &recipes.fluid_tags))
        .collect()
}
fn within(anchor: [f64; 3], other: [f64; 3]) -> bool {
    other[0] + 0.125 > anchor[0] - 1.0
        && other[0] - 0.125 < anchor[0] + 1.0
        && other[1] + 0.25 > anchor[1] - 1.0
        && other[1] < anchor[1] + 1.0
        && other[2] + 0.125 > anchor[2] - 1.0
        && other[2] - 0.125 < anchor[2] + 1.0
}
impl Rule {
    pub fn explosion(&self) -> bool {
        matches!(self.circumstance, Circumstance::Explosion)
    }
    pub fn fluid(&self, id: &str) -> bool {
        matches!(&self.circumstance,Circumstance::Fluid(ids) if ids.contains(id))
    }
    pub fn any_fluid_anchor(&self, stack: &ItemStack) -> bool {
        !self.explosion() && self.ingredients[0].contains(stack.item.as_ref())
    }
    pub fn select(&self, anchor: usize, items: &[ItemEntity]) -> Option<Vec<(usize, u32)>> {
        let first = items.get(anchor)?;
        if first.stack.count == 0 {
            return None;
        }
        let eligible = if self.explosion() {
            self.ingredients
                .iter()
                .any(|i| i.contains(first.stack.item.as_ref()))
        } else {
            self.ingredients[0].contains(first.stack.item.as_ref())
        };
        if !eligible {
            return None;
        }
        let at = [first.position.x, first.position.y, first.position.z];
        let mut missing = vec![true; self.ingredients.len()];
        let mut claims = Vec::new();
        // TransformLogic iterates item entities first, then the remaining
        // ingredients; it does not perform bipartite reassignment.
        for (index, item) in items.iter().enumerate() {
            if item.stack.count == 0
                || !within(at, [item.position.x, item.position.y, item.position.z])
            {
                continue;
            }
            let mut used = 0;
            for (ingredient, missing) in self.ingredients.iter().zip(missing.iter_mut()) {
                if *missing
                    && used < item.stack.count
                    && ingredient.contains(item.stack.item.as_ref())
                {
                    used += 1;
                    *missing = false;
                }
            }
            if used > 0 {
                claims.push((index, used));
            }
        }
        missing.iter().all(|v| !*v).then_some(claims)
    }
    pub fn assemble(&self) -> Result<ItemStack, String> {
        let mut output = self.output.clone();
        if output.item.as_ref() == "ae2:quantum_entangled_singularity" && output.count > 1 {
            use std::sync::atomic::{AtomicI64, Ordering};
            static SEED: AtomicI64 = AtomicI64::new(0);
            let millis = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_millis() as i64;
            let frequency = millis
                .wrapping_mul(100)
                .wrapping_add(SEED.fetch_add(1, Ordering::Relaxed) % 100);
            // QuantumBridgeBlockEntity.assignFrequency writes the AE2
            // component itself, preserving unrelated custom data unchanged.
            output.data_mut().set_other("ae2:entangled_singularity_id", Some(Tag::Long(frequency)));
        }
        Ok(output)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use horizon_core::geometry::Vec3;
    fn entity(id: u32, item: &str, count: u32) -> ItemEntity {
        ItemEntity {
            target: None,
            health: 5,
            fire_ticks: 0,
            id,
            stack: ItemStack::new(item, count),
            thrower: None,
            position: Vec3::ZERO,
            previous: Vec3::ZERO,
            velocity: Vec3::ZERO,
            on_ground: false,
            age: 0,
            pickup_delay: 0,
            bob_offset: 0.0,
            ticks: 0,
            needs_sync: false,
        }
    }
    #[test]
    fn active_tags_and_circumstance_control_transform_anchors_and_claims() {
        let items = Items::for_tests(HashMap::from([(
            "test:stones".into(),
            HashSet::from(["minecraft:stone".into()]),
        )]));
        let fluids = HashMap::from([(
            "test:hot".into(),
            HashSet::from(["minecraft:lava".into(), "minecraft:flowing_lava".into()]),
        )]);
        let rule=parse(&serde_json::json!({"ingredients":[{"item":"minecraft:dirt"},{"tag":"test:stones"}],"result":{"id":"minecraft:emerald","count":3},"circumstance":{"type":"fluid","tag":"test:hot"}}),&items,&fluids).unwrap();
        assert!(rule.fluid("minecraft:flowing_lava"));
        assert!(!rule.fluid("minecraft:water"));
        let inputs = vec![
            entity(1, "minecraft:dirt", 2),
            entity(2, "minecraft:stone", 1),
        ];
        assert_eq!(rule.select(0, &inputs), Some(vec![(0, 1), (1, 1)]));
        assert!(rule.select(1, &inputs).is_none());
        assert_eq!(inputs[0].stack.count, 2);
        assert_eq!(rule.assemble().unwrap().count, 3);
    }
    #[test]
    fn source_item_first_greedy_does_not_reassign_overlapping_ingredients() {
        let items = Items::for_tests(HashMap::new());
        let rule=parse(&serde_json::json!({"ingredients":[[{"item":"minecraft:dirt"},{"item":"minecraft:stone"}],{"item":"minecraft:dirt"}],"result":{"id":"minecraft:diamond"},"circumstance":{"type":"explosion"}}),&items,&HashMap::new()).unwrap();
        let inputs = vec![
            entity(1, "minecraft:dirt", 1),
            entity(2, "minecraft:stone", 1),
        ];
        assert!(rule.select(0, &inputs).is_none());
        let inputs = vec![
            entity(2, "minecraft:stone", 1),
            entity(1, "minecraft:dirt", 1),
        ];
        assert_eq!(rule.select(1, &inputs), Some(vec![(0, 1), (1, 1)]));
    }
    #[test]
    fn pearl_tag_explosion_accepts_either_anchor_and_assigns_one_frequency_to_pair() {
        let items = Items::for_tests(HashMap::from([(
            "c:ender_pearls".into(),
            HashSet::from(["minecraft:ender_pearl".into()]),
        )]));
        let rule=parse(&serde_json::json!({"ingredients":[{"item":"ae2:singularity"},{"tag":"c:ender_pearls"}],"result":{"id":"ae2:quantum_entangled_singularity","count":2},"circumstance":{"type":"explosion"}}),&items,&HashMap::new()).unwrap();
        let inputs = vec![
            entity(1, "minecraft:ender_pearl", 1),
            entity(2, "ae2:singularity", 1),
        ];
        assert!(rule.select(0, &inputs).is_some());
        assert!(rule.select(1, &inputs).is_some());
        let pair = rule.assemble().unwrap();
        assert_eq!(pair.count, 2);
        assert!(matches!(
            pair.data().unwrap().other("ae2:entangled_singularity_id"),
            Some(Tag::Long(_))
        ));
        assert!(pair.data().unwrap().other("minecraft:custom_data").is_none());
        let mut tagged_rule = rule.clone();
        let marker = Tag::compound([("marker", Tag::Int(7))]);
        tagged_rule.output.data_mut().set_other("minecraft:custom_data", Some(marker.clone()));
        let tagged = tagged_rule.assemble().unwrap();
        assert_eq!(tagged.data().unwrap().other("minecraft:custom_data"), Some(&marker));
        assert!(matches!(tagged.data().unwrap().other("ae2:entangled_singularity_id"), Some(Tag::Long(_))));
        let saved = crate::save::item_to_nbt(&tagged);
        assert_eq!(crate::save::item_from_nbt(&saved).unwrap(), tagged);
    }
    #[test]
    fn all_nearby_ingredients_are_claimed_before_any_actual_stack_is_consumed() {
        let items = Items::for_tests(HashMap::new());
        let rule=parse(&serde_json::json!({"ingredients":[{"item":"ae2:charged_certus_quartz_crystal"},{"item":"minecraft:redstone"},{"item":"minecraft:quartz"}],"result":{"id":"ae2:fluix_crystal","count":2}}),&items,&HashMap::new()).unwrap();
        let mut inputs = vec![
            entity(1, "ae2:charged_certus_quartz_crystal", 3),
            entity(2, "minecraft:redstone", 2),
            entity(3, "minecraft:quartz", 1),
        ];
        inputs[1].position.x = 0.5;
        inputs[2].position.x = 3.0;
        assert!(rule.select(0, &inputs).is_none());
        assert_eq!(inputs[0].stack.count, 3);
        assert_eq!(inputs[1].stack.count, 2);
        inputs[2].position.x = 0.8;
        assert_eq!(rule.select(0, &inputs), Some(vec![(0, 1), (1, 1), (2, 1)]));
    }
    #[test]
    fn matching_region_intersects_actual_item_bounds() {
        assert!(within([0.0; 3], [1.1, 0.0, 0.0]));
        assert!(!within([0.0; 3], [1.2, 0.0, 0.0]));
        assert!(!within([0.0; 3], [0.0, -1.3, 0.0]));
    }
}
