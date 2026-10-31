//! NeoForge conditions used by the pinned AE2 19.2.18 recipe data.
//! Evaluate tag conditions at data-load time, against the resolved item tags.
use serde_json::Value;
use std::collections::{HashMap, HashSet};
fn condition(v: &Value, tags: &HashMap<String, HashSet<String>>, depth: u8) -> Option<bool> {
    if depth == 64 { return None; }
    match v.get("type")?.as_str()? {
        "neoforge:tag_empty" => Some(tags.get(v.get("tag")?.as_str()?).is_none_or(HashSet::is_empty)),
        "neoforge:not" => Some(!condition(v.get("value")?, tags, depth + 1)?),
        _ => None,
    }
}
pub(crate) fn enabled(recipe: &Value, tags: &HashMap<String, HashSet<String>>) -> bool {
    let Some(conditions) = recipe.get("neoforge:conditions") else { return true; };
    let Some(conditions) = conditions.as_array() else { return false; };
    conditions.iter().all(|v| match condition(v, tags, 0) {
        Some(enabled) => enabled,
        None => { log::warn!("Horizon: unsupported or invalid recipe condition {v}"); false }
    })
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn ae2_optional_ammunition_tracks_resolved_tags_and_rejects_unknown_conditions() {
        let tags = HashMap::from([("c:nuggets/iron".to_owned(), HashSet::from(["minecraft:iron_nugget".to_owned()])),("c:nuggets/empty".to_owned(),HashSet::new())]);
        let recipe = |tag: &str| serde_json::json!({"neoforge:conditions":[{"type":"neoforge:not","value":{"type":"neoforge:tag_empty","tag":tag}}]});
        assert!(enabled(&recipe("c:nuggets/iron"), &tags));
        assert!(!enabled(&recipe("c:nuggets/zinc"), &tags));
        assert!(!enabled(&recipe("c:nuggets/empty"), &tags));
        assert!(enabled(&serde_json::json!({}), &tags));
        assert!(!enabled(&serde_json::json!({"neoforge:conditions":[{"type":"unknown:condition"}]}), &tags));
    }
}
