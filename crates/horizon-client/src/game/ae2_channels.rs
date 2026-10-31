//! AE2 common-config channel capacity mode. Node-level forced modes use `cm`.
use ae2_network_core::ChannelMode;
use std::{
    cell::RefCell,
    path::PathBuf,
    time::{Duration, Instant},
};

fn parse_common_config(text: &str) -> Result<ChannelMode, String> {
    let config: toml::Value = toml::from_str(text).map_err(|error| error.to_string())?;
    let Some(value) = config
        .get("general")
        .and_then(|section| section.get("channels"))
    else {
        return Ok(ChannelMode::Default);
    };
    let name = value
        .as_str()
        .ok_or("general.channels must be an AE2 enum string")?;
    ChannelMode::from_source_name(name).ok_or_else(|| format!("unknown AE2 channel mode {name}"))
}
/// ManagedGridNode serializes service data under `gn`; ME P2P's separate
/// outer node uses `outer`. A `cm` belongs to that node alone.
pub(super) fn node_mode(data: &horizon_world::nbt::Compound, node: &str) -> Option<ChannelMode> {
    match data.get(&node.into()).and_then(|tag| tag.get("cm")) {
        Some(horizon_world::nbt::Tag::String(name)) => {
            ChannelMode::from_source_name(&name.to_string_lossy())
        }
        _ => None,
    }
}
pub(super) fn set_node_mode(
    data: &mut horizon_world::nbt::Compound,
    node: &str,
    mode: Option<ChannelMode>,
) -> Result<(), &'static str> {
    use horizon_world::nbt::{Compound, Tag};
    if data.get(&node.into()).is_some_and(|tag| !matches!(tag, Tag::Compound(_))) {
        return Err("invalid managed grid node data");
    }
    if let Some(mode) = mode {
        let entry = data
            .entry(node.into())
            .or_insert_with(|| Tag::Compound(Compound::new()));
        if let Tag::Compound(values) = entry {
            values.insert("cm".into(), Tag::String(mode.source_name().into()));
        }
    } else if let Some(Tag::Compound(values)) = data.get_mut(&node.into()) {
        values.remove(&"cm".into());
    }
    Ok(())
}
/// Identity belongs to the physical face part, never to its replaceable item
/// stack or synthetic graph coordinates. Malformed legacy data fails closed.
pub(super) fn part_identity(part: &horizon_world::nbt::Tag) -> Option<[i32; 4]> {
    match part.get("ae2_part_identity") {
        Some(horizon_world::nbt::Tag::IntArray(values)) => {
            let id: [i32; 4] = values.as_slice().try_into().ok()?;
            (id != [0; 4]).then_some(id)
        }
        _ => None,
    }
}
/// Migrate one legacy physical part exactly once. Invalid pre-existing IDs
/// remain untouched so consumers can fail closed rather than rebind them.
pub(super) fn ensure_part_identity(part: &mut horizon_world::nbt::Tag) -> bool {
    use horizon_world::nbt::Tag;
    let Tag::Compound(data) = part else { return false };
    if data.contains_key(&"ae2_part_identity".into())
        || !matches!(data.get(&"id".into()), Some(Tag::String(_)))
        || !matches!(data.get(&"face".into()), Some(Tag::String(_)))
    { return false; }
    data.insert("ae2_part_identity".into(), Tag::IntArray(horizon_client::save::random_uuid().to_vec()));
    true
}
/// Merging saved source nodes carries the most permissive locked mode.
pub(super) fn merge_mode(a: Option<ChannelMode>, b: Option<ChannelMode>) -> Option<ChannelMode> {
    a.into_iter().chain(b).max_by_key(|mode| mode.ad_hoc_limit())
}
struct Cache {
    path: PathBuf,
    checked: Instant,
    mode: ChannelMode,
}
thread_local! { static COMMON: RefCell<Option<Cache>> = const { RefCell::new(None) }; }
/// Match AE2's global `[general] channels`; periodically re-read to honor edits
/// without performing disk I/O on every network operation.
pub(super) fn common_channel_mode() -> ChannelMode {
    let path = horizon_client::logging::game_home().join("config/ae2-common.toml");
    COMMON.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(existing) = cache.as_ref() {
            if existing.path == path && existing.checked.elapsed() < Duration::from_secs(1) {
                return existing.mode;
            }
        }
        let mode = match std::fs::read_to_string(&path) {
            Ok(text) => parse_common_config(&text).unwrap_or_else(|error| {
                log::warn!("invalid AE2 common config: {error}");
                ChannelMode::Default
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => ChannelMode::Default,
            Err(error) => {
                log::warn!("cannot read AE2 common config: {error}");
                ChannelMode::Default
            }
        };
        *cache = Some(Cache {
            path,
            checked: Instant::now(),
            mode,
        });
        mode
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_part_identity_migration_is_stable_and_preserves_malformed_data() {
        use horizon_world::nbt::Tag;
        let mut part = Tag::compound([("id", Tag::String("ae2:export_bus".into())), ("face", Tag::String("north".into()))]);
        assert!(ensure_part_identity(&mut part));
        let id = part_identity(&part).unwrap();
        assert!(!ensure_part_identity(&mut part));
        assert_eq!(part_identity(&part), Some(id));
        let mut malformed = Tag::compound([("id", Tag::String("ae2:export_bus".into())), ("face", Tag::String("north".into())), ("ae2_part_identity", Tag::Int(9))]);
        assert!(!ensure_part_identity(&mut malformed));
        assert_eq!(part_identity(&malformed), None);
        assert!(matches!(malformed.get("ae2_part_identity"), Some(Tag::Int(9))));
    }
    #[test]
    fn source_common_config_uses_general_channels_and_rejects_invalid_values() {
        assert_eq!(
            parse_common_config("[general]\nchannels = \"X2\"\n").unwrap(),
            ChannelMode::X2
        );
        assert_eq!(
            parse_common_config("[general]\nchannels = \"INFINITE\"\n").unwrap(),
            ChannelMode::Infinite
        );
        assert_eq!(
            parse_common_config("[automation]\nchannels = \"X4\"\n").unwrap(),
            ChannelMode::Default
        );
        assert!(parse_common_config("[general]\nchannels = \"INVALID\"\n").is_err());
        let mut nbt = horizon_world::nbt::Compound::new();
        set_node_mode(&mut nbt, "gn", Some(ChannelMode::X2)).unwrap();
        set_node_mode(&mut nbt, "outer", Some(ChannelMode::X4)).unwrap();
        assert_eq!(node_mode(&nbt, "gn"), Some(ChannelMode::X2));
        assert_eq!(node_mode(&nbt, "outer"), Some(ChannelMode::X4));
        set_node_mode(&mut nbt, "gn", None).unwrap();
        assert_eq!(node_mode(&nbt, "gn"), None);
        assert_eq!(node_mode(&nbt, "outer"), Some(ChannelMode::X4));
        nbt.insert("gn".into(), horizon_world::nbt::Tag::Int(17));
        assert!(set_node_mode(&mut nbt, "gn", Some(ChannelMode::X3)).is_err());
        assert!(matches!(nbt.get(&"gn".into()), Some(horizon_world::nbt::Tag::Int(17))));
    }
}
