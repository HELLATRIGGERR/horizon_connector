//! AE2 19.2.18 AEBasePoweredItem, EntropyManipulatorItem and ChargedStaffItem.
//! LGPL-3.0-or-later. Source components take precedence; older native
//! custom-data saves remain readable.
use crate::items::ItemStack;
use crate::{crafting::Recipes, items::Items};
use horizon_world::nbt::{Compound, Tag};
const STORED: &str = "ae2:stored_energy";
const CAPACITY: &str = "ae2:energy_capacity";
/// Tooltips.energyStorageComponent, with AE2's capacity-dependent SI precision.
pub fn energy_tooltip(label: &str, current: f64, maximum: f64) -> crate::text::Text {
    use crate::text::Text;
    fn number(n: f64, scale: f64) -> String {
        let value = n / scale;
        let precision = if value < 10.0 {
            3
        } else if value < 100.0 {
            2
        } else {
            1
        };
        format!("{value:.precision$}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_owned()
    }
    let mut scale = 1.0;
    let mut unit = "";
    if maximum >= 10_000.0 {
        for next in ["k", "M", "G", "T", "P", "E"] {
            scale *= 1000.0;
            unit = next;
            if maximum / scale < 1000.0 {
                break;
            }
        }
    }
    let stored = number(current, scale);
    let stored_unit = if stored == "0" { "" } else { unit };
    let ratio = if maximum > 0.0 {
        (current / maximum).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let percent = format!("{:.2}", ratio * 100.0)
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned();
    let red = (255.0 * (2.0 - 2.0 * ratio).clamp(0.0, 1.0)) as u32;
    let green = (255.0 * (2.0 * ratio).clamp(0.0, 1.0)) as u32;
    Text::colored(format!("{label}: "), 0xaaaaaa)
        .append(Text::colored(format!("{stored}{stored_unit}"), 0x886eff))
        .append(Text::colored("/", 0xaaaaaa))
        .append(Text::colored(
            format!("{}{unit}", number(maximum, scale)),
            0x886eff,
        ))
        .append(Text::colored(" AE (", 0xaaaaaa))
        .append(Text::colored(
            format!("{percent}%"),
            (red << 16) | (green << 8),
        ))
        .append(Text::colored(")", 0xaaaaaa))
}
pub fn selected_color(stack: &ItemStack) -> Option<u8> {
    let data=stack.data()?;
    let value=data.other("ae2:selected_color").or_else(||data.other("minecraft:custom_data").and_then(|custom|custom.get("ae2:selected_color")))?;
    let Tag::String(value) = value else {
        return None;
    };
    let value = value.to_string().ok()?;
    COLORS
        .iter()
        .position(|name| *name == value)
        .map(|i| i as u8)
}
pub fn applicator_entries(
    items: &Items,
    stack: &ItemStack,
) -> Result<Vec<(ae2_cell_core::ItemKey, u64, u8)>, String> {
    let cell = crate::ae2_cell_codec::read_cell(stack)?;
    Ok(cell
        .entries()
        .filter_map(|(key, count)| {
            let item = crate::ae2_cell_codec::prototype(key).ok()?;
            Some((key.clone(), *count, item_color(items, &item)?))
        })
        .collect())
}
/// Source getColor uses the selected component or the first available key.
pub fn applicator_color(items: &Items, stack: &ItemStack) -> Result<Option<u8>, String> {
    if let Some(color) = selected_color(stack) {
        return Ok(Some(color));
    }
    Ok(applicator_entries(items, stack)?
        .first()
        .map(|(_, _, color)| *color))
}
pub fn color_translation(color: u8) -> Option<&'static str> {
    [
        "gui.ae2.White",
        "gui.ae2.LightGray",
        "gui.ae2.Gray",
        "gui.ae2.Black",
        "gui.ae2.Lime",
        "gui.ae2.Yellow",
        "gui.ae2.Orange",
        "gui.ae2.Brown",
        "gui.ae2.Red",
        "gui.ae2.Pink",
        "gui.ae2.Magenta",
        "gui.ae2.Purple",
        "gui.ae2.Blue",
        "gui.ae2.LightBlue",
        "gui.ae2.Cyan",
        "gui.ae2.Green",
        "gui.ae2.Fluix",
    ]
    .get(color as usize)
    .copied()
}
/// AEColor's black/medium/white variants; tint zero remains opaque white.
pub fn color_tints(color: u8) -> Option<Vec<i32>> {
    let [dark, medium, bright] = *[
        [0xb4b4b4, 0xe0e0e0, 0xf9f9f9],
        [0x7e7e7e, 0xa09fa0, 0xc4c4c4],
        [0x4f4f4f, 0x6c6b6c, 0x949294],
        [0x131313, 0x272727, 0x3b3b3b],
        [0x4ec04e, 0x70e259, 0xb3f86d],
        [0xffcf40, 0xffe359, 0xf4ff80],
        [0xd9782f, 0xeca23c, 0xf2ba49],
        [0x6e4a12, 0x7e5c16, 0x8e6e1a],
        [0xaa212b, 0xd73e42, 0xf07665],
        [0xd86eaa, 0xff99bb, 0xfbcad5],
        [0xc15189, 0xd5719c, 0xe69ebf],
        [0x6e5cb8, 0x915dcd, 0xb06fdd],
        [0x337ff0, 0x3894ff, 0x40c1ff],
        [0x69b9ff, 0x70d2ff, 0x80f7ff],
        [0x22b0ae, 0x2fccb7, 0x65e8c9],
        [0x079b6b, 0x17b86d, 0x32d850],
        [0x5a479e, 0x915dcd, 0xe2a3e3],
    ]
    .get(color as usize)?;
    Some(vec![
        -1,
        (0xff000000u32 | dark) as i32,
        (0xff000000u32 | medium) as i32,
        (0xff000000u32 | bright) as i32,
    ])
}
/// Source AEColor order, which differs from the vanilla dye numeric order.
pub const COLORS: [&str; 17] = [
    "white",
    "light_gray",
    "gray",
    "black",
    "lime",
    "yellow",
    "orange",
    "brown",
    "red",
    "pink",
    "magenta",
    "purple",
    "blue",
    "light_blue",
    "cyan",
    "green",
    "fluix",
];
pub fn item_color(items: &Items, stack: &ItemStack) -> Option<u8> {
    if stack.item.as_ref() == "minecraft:snowball" {
        return Some(16);
    }
    COLORS
        .iter()
        .take(16)
        .position(|color| {
            stack.item.as_ref() == format!("minecraft:{color}_dye")
                || stack.item.as_ref() == format!("ae2:{color}_paint_ball")
                || stack.item.as_ref() == format!("ae2:{color}_lumen_paint_ball")
                || items
                    .item_tags
                    .get(&format!("c:dyes/{color}"))
                    .is_some_and(|ids| ids.contains(stack.item.as_ref()))
        })
        .map(|i| i as u8)
}
pub fn paint_ball(stack: &ItemStack) -> Option<(u8, bool)> {
    COLORS.iter().take(16).enumerate().find_map(|(i, color)| {
        if stack.item.as_ref() == format!("ae2:{color}_paint_ball") {
            Some((i as u8, false))
        } else if stack.item.as_ref() == format!("ae2:{color}_lumen_paint_ball") {
            Some((i as u8, true))
        } else {
            None
        }
    })
}
pub fn ingredient_matches(value: &serde_json::Value, items: &Items, stack: &ItemStack) -> bool {
    match value {
        serde_json::Value::String(id) => id.strip_prefix('#').map_or_else(
            || id == stack.item.as_ref(),
            |tag| {
                items
                    .item_tags
                    .get(tag)
                    .is_some_and(|ids| ids.contains(stack.item.as_ref()))
            },
        ),
        serde_json::Value::Array(values) => {
            values.iter().any(|v| ingredient_matches(v, items, stack))
        }
        serde_json::Value::Object(value) => {
            value
                .get("item")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|id| id == stack.item.as_ref())
                || value
                    .get("tag")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|tag| {
                        items
                            .item_tags
                            .get(tag)
                            .is_some_and(|ids| ids.contains(stack.item.as_ref()))
                    })
        }
        _ => false,
    }
}
/// First matching actual source ammo recipe, including datapack replacements.
pub fn ammo_weight(recipes: &Recipes, items: &Items, stack: &ItemStack) -> f32 {
    recipes
        .ae2_custom
        .get("ae2:matter_cannon")
        .and_then(|recipes| {
            recipes
                .iter()
                .find(|(_, r)| ingredient_matches(&r["ammo"], items, stack))
        })
        .and_then(|(_, r)| r["weight"].as_f64())
        .filter(|v| v.is_finite())
        .map_or(0.0, |v| v as f32)
}
pub fn profile(stack: &ItemStack) -> Option<(f64, f64)> {
    if stack.count != 1 {
        return None;
    }
    let battery=crate::ae2_common_config::batteries();
    match stack.item.as_ref() {
        "ae2:entropy_manipulator" => Some((battery.entropy_manipulator, 800.0)),
        "ae2:charged_staff" => Some((battery.charged_staff, 32.0)),
        "ae2:matter_cannon" => Some((battery.matter_cannon, 800.0)),
        "ae2:color_applicator" => Some((battery.color_applicator, 80.0)),
        _ => None,
    }
}
fn custom(stack: &ItemStack) -> Result<Compound, String> {
    match stack.data().and_then(|d| d.other("minecraft:custom_data")) {
        None => Ok(Default::default()),
        Some(Tag::Compound(c)) => Ok(c.clone()),
        _ => Err("invalid powered item data".into()),
    }
}
fn number(data: &Compound, name: &str, default: f64) -> Result<f64, String> {
    let value = match data.get(&name.into()) {
        None => default,
        Some(Tag::Double(bits)) => f64::from_bits(*bits),
        _ => return Err(format!("invalid {name}")),
    };
    if !value.is_finite() || (value < 0.0 && name != CAPACITY) {
        return Err(format!("invalid {name}"));
    }
    Ok(value)
}
/// Read AEBasePoweredItem's persistent double component, with the native
/// legacy field only when that component is absent.
pub fn component_power_value(stack: &ItemStack, component: &str, legacy: &str, default: f64) -> Result<f64, String> {
    if let Some(value) = stack.data().and_then(|data| data.other(component)) {
        let Tag::Double(bits) = value else { return Err(format!("invalid {component}")); };
        let value = f64::from_bits(*bits);
        if !value.is_finite() || (value < 0.0 && component != CAPACITY) { return Err(format!("invalid {component}")); }
        return Ok(value);
    }
    number(&custom(stack)?, legacy, default)
}
/// Keep legacy readers synchronized while writing the actual source component.
/// Removing the source value also removes its old alias, so zero stays zero.
pub fn set_component_power_value(stack: &mut ItemStack, component: &str, legacy: &str, value: Option<f64>) -> Result<(), String> {
    if value.is_some_and(|v| !v.is_finite() || (v < 0.0 && component != CAPACITY)) { return Err(format!("invalid {component}")); }
    let mut data = custom(stack)?;
    if let Some(value) = value { data.insert(legacy.into(), Tag::Double(value.to_bits())); }
    else { data.remove(&legacy.into()); }
    stack.data_mut().set_other("minecraft:custom_data", Some(Tag::Compound(data)));
    stack.data_mut().set_other(component, value.map(|value| Tag::Double(value.to_bits())));
    Ok(())
}
pub fn energy(stack: &ItemStack) -> Result<(f64, f64), String> {
    let (base, _) = profile(stack).ok_or("not an AE2 powered tool")?;
    let capacity = component_power_value(stack, CAPACITY, CAPACITY, base)?;
    Ok((component_power_value(stack, STORED, STORED, 0.0)?, capacity))
}
pub fn charge_rate(stack: &ItemStack) -> Result<f64, String> {
    let (_, rate) = profile(stack).ok_or("not an AE2 powered tool")?;
    let (cards, _) = tool_upgrades(stack)?;
    Ok(rate * (1.0 + f64::from(cards)))
}
/// Source physical upgrade limits. Returns (energy multiplier, speed cards).
pub fn tool_upgrades(stack: &ItemStack) -> Result<(u8, u8), String> {
    if !matches!(stack.item.as_ref(),"ae2:matter_cannon" | "ae2:color_applicator") {return Ok((0,0));}
    let inventory=crate::ae2_upgrade_inventory::Inventory::read(stack)?;
    let count=|id:&str|inventory.slots.iter().flatten().filter(|card|card.item.as_ref()==id).count() as u8;
    Ok((count("ae2:energy_card"),count("ae2:speed_card")))
}
pub fn transfer(
    stack: &mut ItemStack,
    amount: f64,
    insert: bool,
    simulate: bool,
) -> Result<f64, String> {
    if !amount.is_finite() || amount < 0.0 {
        return Err("invalid tool power amount".into());
    }
    let (current, capacity) = energy(stack)?;
    let moved = amount.min(if insert {
        (capacity - current).max(0.0)
    } else {
        current
    });
    if !simulate {
        let next = if insert {
            current + amount.min(capacity-current)
        } else {
            current - moved
        };
        set_component_power_value(stack, STORED, STORED, (next >= 0.0001).then_some(next))?;
    }
    Ok(moved)
}
/// Both source tools require strictly more energy than their use cost.
pub fn can_use(stack: &ItemStack, cost: f64) -> bool {
    energy(stack).is_ok_and(|(n, _)| n > cost)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn physical_tool_cards_apply_source_capacity_and_rate() {
        let card = |id: &str| horizon_items::item_nbt::item_to_nbt(&ItemStack::new(id, 1));
        let mut cannon = ItemStack::new("ae2:matter_cannon", 1);
        cannon.data_mut().set_other(
            "minecraft:custom_data",
            Some(Tag::compound([(
                "ae2_cell_settings",
                Tag::compound([(
                    "upgrades",
                    Tag::List(vec![card("ae2:energy_card"), card("ae2:speed_card")]),
                )]),
            )])),
        );
        assert_eq!(energy(&cannon).unwrap(), (0.0, 200_000.0),"raw card components do not invoke the source upgrade callback");
        crate::ae2_upgrade_inventory::Inventory::read(&cannon).unwrap().store(&mut cannon).unwrap();
        assert_eq!(energy(&cannon).unwrap(), (0.0, 1_800_000.0));
        assert_eq!(charge_rate(&cannon).unwrap(), 1600.0);
        assert_eq!(tool_upgrades(&cannon).unwrap(), (1, 1));
        let mut applicator = ItemStack::new("ae2:color_applicator", 1);
        applicator.data_mut().set_other(
            "minecraft:custom_data",
            Some(Tag::compound([(
                "ae2_cell_settings",
                Tag::compound([(
                    "upgrades",
                    Tag::List(vec![card("ae2:energy_card"), card("ae2:energy_card")]),
                )]),
            )])),
        );
        assert_eq!(energy(&applicator).unwrap(), (0.0, 20_000.0));
        crate::ae2_upgrade_inventory::Inventory::read(&applicator).unwrap().store(&mut applicator).unwrap();
        assert_eq!(energy(&applicator).unwrap(), (0.0, 340_000.0));
        assert_eq!(charge_rate(&applicator).unwrap(), 240.0);
    }
    #[test]
    fn source_power_limits_simulation_and_strict_threshold() {
        let mut staff = ItemStack::new("ae2:charged_staff", 1);
        assert_eq!(transfer(&mut staff, 300.0, true, true).unwrap(), 300.0);
        assert_eq!(energy(&staff).unwrap().0, 0.0);
        transfer(&mut staff, 300.0, true, false).unwrap();
        assert!(!can_use(&staff, 300.0));
        transfer(&mut staff, 1.0, true, false).unwrap();
        assert!(can_use(&staff, 300.0));
        transfer(&mut staff, 300.0, false, false).unwrap();
        assert_eq!(energy(&staff).unwrap().0, 1.0);
        assert_eq!(transfer(&mut staff, 99999.0, true, false).unwrap(), 7999.0);
        assert_eq!(energy(&staff).unwrap(), (8000.0, 8000.0));
        let mut bad = ItemStack::new("ae2:entropy_manipulator", 1);
        bad.data_mut().set_other(
            "minecraft:custom_data",
            Some(Tag::compound([(STORED, Tag::Double(f64::NAN.to_bits()))])),
        );
        assert!(energy(&bad).is_err());
    }
}
#[cfg(test)] mod source_energy_tests {
    use super::*;
    #[test]
    fn source_components_override_legacy_and_mutations_update_both() {
        let mut stack=ItemStack::new("ae2:charged_staff",1);
        stack.data_mut().set_other("minecraft:custom_data",Some(Tag::compound([(STORED,Tag::Double(90.0f64.to_bits())),(CAPACITY,Tag::Double(8000.0f64.to_bits()))])));
        stack.data_mut().set_other(STORED,Some(Tag::Double(20.0f64.to_bits())));
        stack.data_mut().set_other(CAPACITY,Some(Tag::Double(50.0f64.to_bits())));
        assert_eq!(energy(&stack).unwrap(),(20.0,50.0));
        assert_eq!(transfer(&mut stack,10.0,true,false).unwrap(),10.0);
        assert_eq!(energy(&stack).unwrap(),(30.0,50.0));
        assert_eq!(stack.data().unwrap().other(STORED),Some(&Tag::Double(30.0f64.to_bits())));
        assert_eq!(custom(&stack).unwrap().get(&STORED.into()),Some(&Tag::Double(30.0f64.to_bits())));
        transfer(&mut stack,30.0,false,false).unwrap();
        assert!(stack.data().unwrap().other(STORED).is_none());
        assert!(!custom(&stack).unwrap().contains_key(&STORED.into()));
        stack.data_mut().set_other(STORED,Some(Tag::String("invalid".into())));
        assert!(energy(&stack).is_err());
    }
    fn overfull()->ItemStack {let mut stack=ItemStack::new("ae2:charged_staff",1);stack.data_mut().set_other("minecraft:custom_data",Some(Tag::compound([(STORED,Tag::Double(9000.0f64.to_bits())),(CAPACITY,Tag::Double(2000.0f64.to_bits()))])));stack}
    #[test]
    fn source_reads_and_extracts_over_capacity_power_but_insertion_clamps_it() {
        let mut stack=overfull();assert_eq!(energy(&stack).unwrap(),(9000.0,2000.0));let unchanged=stack.clone();
        assert_eq!(transfer(&mut stack,100.0,true,true).unwrap(),0.0);assert_eq!(stack,unchanged);
        assert_eq!(transfer(&mut stack,100.0,false,false).unwrap(),100.0);assert_eq!(energy(&stack).unwrap(),(8900.0,2000.0));
        assert_eq!(transfer(&mut stack,0.0,true,false).unwrap(),0.0);assert_eq!(energy(&stack).unwrap(),(2000.0,2000.0));
        assert_eq!(transfer(&mut stack,5000.0,false,false).unwrap(),2000.0);assert_eq!(energy(&stack).unwrap(),(0.0,2000.0));assert!(!custom(&stack).unwrap().contains_key(&STORED.into()));
    }
}
