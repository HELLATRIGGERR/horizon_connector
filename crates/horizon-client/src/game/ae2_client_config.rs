//! Native client settings that currently have a behavioral consumer.
//! Defaults and TOML names follow AE2 19.2.18 AEConfig.ClientConfig.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ClientSettings {
    pub remember_last_search: bool,
    pub auto_focus_search: bool,
    pub search_mod_name_in_tooltips: bool,
    pub clear_grid_on_close: bool,
    pub pin_auto_crafted_items: bool,
    pub notify_finished_jobs: bool,
    pub show_cell_upgrades: bool,
    pub show_cell_content: bool,
    pub max_cell_content_shown: usize,
}

impl Default for ClientSettings {
    fn default() -> Self {
        Self { remember_last_search: true, auto_focus_search: false, search_mod_name_in_tooltips: false, clear_grid_on_close: false,
            pin_auto_crafted_items: true, notify_finished_jobs: true,
            show_cell_upgrades: true, show_cell_content: true, max_cell_content_shown: 5 }
    }
}

fn parse(text: &str) -> Result<ClientSettings, String> {
    let value: toml::Value = toml::from_str(text).map_err(|error| error.to_string())?;
    let mut settings = ClientSettings::default();
    for (section, name, target) in [
        ("search", "rememberLastSearch", &mut settings.remember_last_search),
        ("search", "autoFocusSearch", &mut settings.auto_focus_search),
        ("search", "searchModNameInTooltips", &mut settings.search_mod_name_in_tooltips),
        ("terminals", "clearGridOnClose", &mut settings.clear_grid_on_close),
        ("terminals", "pinAutoCraftedItems", &mut settings.pin_auto_crafted_items),
        ("client", "notifyForFinishedCraftingJobs", &mut settings.notify_finished_jobs),
        ("tooltips", "showCellUpgrades", &mut settings.show_cell_upgrades),
        ("tooltips", "showCellContent", &mut settings.show_cell_content),
    ] {
        if let Some(table) = value.get(section) {
            if !table.is_table() { return Err(format!("{section} must be a table")); }
            if let Some(value) = table.get(name) {
                *target = value.as_bool().ok_or_else(|| format!("{section}.{name} must be boolean"))?;
            }
        }
    }
    if let Some(table)=value.get("tooltips") {
        if let Some(value)=table.get("maxCellContentShown") {
            let count=value.as_integer().filter(|n|(1..=32).contains(n)).ok_or("tooltips.maxCellContentShown must be 1..32")?;
            settings.max_cell_content_shown=count as usize;
        }
    }
    Ok(settings)
}

/// The source tooltip image contains sorted generic stacks and installed
/// cards. The native text presentation uses these same physical records.
#[derive(Debug)]
pub(super) struct CellPreview {
    pub upgrades: Vec<horizon_client::items::ItemStack>,
    pub entries: Vec<(ae2_cell_core::ItemKey,u64,ae2_cell_core::Channel)>,
    pub has_more: bool,
}
pub(super) fn cell_preview(stack:&horizon_client::items::ItemStack, settings:ClientSettings) -> Option<CellPreview> {
    use ae2_cell_core::{Channel,FilterMode};
    use horizon_client::{ae2_cell_codec as codec,ae2_upgrade_inventory};
    use horizon_world::nbt::Tag;
    let id=stack.item.as_ref();
    if !codec::is_cell(id) && !matches!(id,"ae2:matter_cannon"|"ae2:color_applicator") { return None; }
    let cell=if codec::is_fluid_cell(id) {codec::read_fluid_cell(stack).ok()?} else {codec::read_cell(stack).ok()?};
    let raw=codec::cell_settings(stack).ok()?;
    let upgrades=if settings.show_cell_upgrades {
        let Tag::List(cards)=raw.get(&"upgrades".into())? else{return None;};
        let max=ae2_upgrade_inventory::capacity(id)?;
        ae2_upgrade_inventory::decode(cards,max).ok()?.into_iter().flatten().collect()
    } else {Vec::new()};
    let mut entries=Vec::new();
    if settings.show_cell_content {
        for channel in [Channel::Item,Channel::Fluid] {
            entries.extend(cell.entries_for(channel).map(|(key,n)|(key.clone(),*n,channel)));
        }
        if let Some(filter)=cell.config().filter.as_ref().filter(|f|f.mode==FilterMode::Whitelist) {
            let channel=cell.config().channel;
            for key in &filter.keys {
                if !entries.iter().any(|(existing,_,kind)|existing==key && *kind==channel) {entries.push((key.clone(),0,channel));}
            }
        }
        entries.sort_by(|a,b|b.1.cmp(&a.1));
    }
    let has_more=entries.len()>settings.max_cell_content_shown;
    entries.truncate(settings.max_cell_content_shown);
    Some(CellPreview {upgrades,entries,has_more})
}

pub(super) fn settings() -> ClientSettings {
    let path = horizon_client::logging::game_home().join("config/ae2-client.toml");
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text).unwrap_or_else(|error| {
            log::warn!("invalid AE2 client config: {error}"); ClientSettings::default()
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => ClientSettings::default(),
        Err(error) => { log::warn!("cannot read AE2 client config: {error}"); ClientSettings::default() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_defaults_and_independent_search_and_grid_settings() {
        assert_eq!(parse("").unwrap(), ClientSettings::default());
        assert!(parse("[search]\nsearchModNameInTooltips = true\n").unwrap().search_mod_name_in_tooltips);
        let parsed = parse("[search]\nrememberLastSearch = false\nautoFocusSearch = true\n[terminals]\nclearGridOnClose = true\n").unwrap();
        assert_eq!(parsed, ClientSettings {remember_last_search:false,auto_focus_search:true,clear_grid_on_close:true,..ClientSettings::default()});
        assert!(parse("[search]\nautoFocusSearch = 'yes'").is_err());
        assert!(parse("[tooltips]\nmaxCellContentShown = 33").is_err());
    }
    #[test]
    fn preview_keeps_each_mixed_content_channel() {
        use ae2_cell_core::Channel;
        use horizon_client::items::ItemStack;
        use horizon_world::nbt::Tag;
        let mut stack=ItemStack::new("ae2:item_storage_cell_1k",1);
        let item=Tag::compound([("#t",Tag::String("ae2:i".into())),("id",Tag::String("minecraft:diamond".into())),("#",Tag::Long(3))]);
        let fluid=Tag::compound([("#t",Tag::String("ae2:f".into())),("id",Tag::String("minecraft:water".into())),("#",Tag::Long(1000))]);
        stack.data_mut().set_other("ae2:storage_cell_inv",Some(Tag::List(vec![item,fluid])));
        let preview=cell_preview(&stack,ClientSettings::default()).unwrap();
        assert!(preview.entries.iter().any(|(key,n,channel)|key.item_id=="minecraft:diamond" && *n==3 && *channel==Channel::Item));
        assert!(preview.entries.iter().any(|(key,n,channel)|key.item_id=="minecraft:water" && *n==1000 && *channel==Channel::Fluid));
    }
    #[test]
    fn preview_limits_and_switches_preserve_actual_cell_contents() {
        use ae2_cell_core::Action;
        use horizon_client::{ae2_cell_codec,items::ItemStack};
        let mut stack=ItemStack::new("ae2:item_storage_cell_1k",1);
        let mut cell=ae2_cell_codec::read_cell(&stack).unwrap();
        for (name,count) in [("minecraft:diamond",2),("minecraft:iron_ingot",7),("minecraft:gold_ingot",4)] {
            let key=ae2_cell_codec::key(&ItemStack::new(name,1)).unwrap();
            assert_eq!(cell.insert(&key,count,Action::Modulate),count);
        }
        ae2_cell_codec::write_cell(&mut stack,&cell).unwrap();
        let preview=cell_preview(&stack,ClientSettings {max_cell_content_shown:2,..ClientSettings::default()}).unwrap();
        assert_eq!(preview.entries.iter().map(|(key,n,_)|(key.item_id.as_str(),*n)).collect::<Vec<_>>(),vec![("minecraft:iron_ingot",7),("minecraft:gold_ingot",4)]);
        assert!(preview.has_more);
        let hidden=cell_preview(&stack,ClientSettings {show_cell_content:false,..ClientSettings::default()}).unwrap();
        assert!(hidden.entries.is_empty() && !hidden.has_more);
        assert_eq!(ae2_cell_codec::read_cell(&stack).unwrap().total_items(),13);
    }
}
