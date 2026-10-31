//! Source AEConfig.CommonConfig spatial and world generation settings. The same native common file
//! is shared with the channel setting; unrelated sections remain untouched.
use std::{
    cell::RefCell,
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spatial {
    pub power_multiplier: f64,
    pub power_exponent: f64,
}
impl Default for Spatial {
    fn default() -> Self {
        Self {
            power_multiplier: 1250.0,
            power_exponent: 1.35,
        }
    }
}
fn parse_spatial(text: &str) -> Result<Spatial, String> {
    let config: toml::Value = toml::from_str(text).map_err(|e| e.to_string())?;
    let mut result = Spatial::default();
    let Some(section) = config.get("spatialio") else {
        return Ok(result);
    };
    if !section.is_table() {
        return Err("spatialio must be a TOML table".into());
    }
    for (name, target) in [
        ("spatialPowerMultiplier", &mut result.power_multiplier),
        ("spatialPowerExponent", &mut result.power_exponent),
    ] {
        if let Some(value) = section.get(name) {
            let n = value
                .as_float()
                .or_else(|| value.as_integer().map(|n| n as f64))
                .ok_or_else(|| format!("spatialio.{name} must be numeric"))?;
            // AEConfig.define(double) accepts Double.MIN_VALUE through MAX_VALUE.
            // In particular, positive exponents below 1 are valid source settings.
            if !n.is_finite() || n <= 0.0 {
                return Err(format!("spatialio.{name} must be positive and finite"));
            }
            *target = n;
        }
    }
    Ok(result)
}
fn parse_world_generation(text: &str) -> Result<crate::ae2_meteorite::Options, String> {
    let config: toml::Value = toml::from_str(text).map_err(|e| e.to_string())?;
    let mut options = crate::ae2_meteorite::Options::default();
    let Some(section) = config.get("worldGen") else {
        return Ok(options);
    };
    if !section.is_table() {
        return Err("worldGen must be a TOML table".into());
    }
    for (name, target) in [
        ("spawnPressesInMeteorites", &mut options.spawn_presses),
        ("spawnFlawlessOnly", &mut options.flawless_only),
    ] {
        if let Some(value) = section.get(name) {
            *target = value
                .as_bool()
                .ok_or_else(|| format!("worldGen.{name} must be boolean"))?;
        }
    }
    Ok(options)
}
struct Cache<T> {
    path: PathBuf,
    checked: Instant,
    value: T,
}
thread_local! {
    static SPATIAL: RefCell<Option<Cache<Spatial>>> = const { RefCell::new(None) };
    static WORLD_GEN: RefCell<Option<Cache<crate::ae2_meteorite::Options>>> = const { RefCell::new(None) };
}
fn cached<T: Copy + Default>(
    cache: &RefCell<Option<Cache<T>>>,
    parser: fn(&str) -> Result<T, String>,
) -> T {
    let path = crate::logging::game_home().join("config/ae2-common.toml");
    let mut cache = cache.borrow_mut();
    if let Some(existing) = cache.as_ref() {
        if existing.path == path && existing.checked.elapsed() < Duration::from_secs(1) {
            return existing.value;
        }
    }
    let value = match std::fs::read_to_string(&path) {
        Ok(text) => parser(&text).unwrap_or_else(|error| {
            log::warn!("invalid AE2 common config: {error}");
            T::default()
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => T::default(),
        Err(error) => {
            log::warn!("cannot read AE2 common config: {error}");
            T::default()
        }
    };
    *cache = Some(Cache {
        path,
        checked: Instant::now(),
        value,
    });
    value
}
pub fn spatial() -> Spatial {
    SPATIAL.with(|c| cached(c, parse_spatial))
}
pub fn world_generation() -> crate::ae2_meteorite::Options {
    WORLD_GEN.with(|c| cached(c, parse_world_generation))
}

/// TickHandler's per-server-tick crafting calculation budget, in milliseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CraftingCalculationBudget(i32);
impl Default for CraftingCalculationBudget {
    fn default() -> Self { Self(5) }
}
fn parse_crafting_calculation_budget(text: &str) -> Result<CraftingCalculationBudget, String> {
    let config: toml::Value = toml::from_str(text).map_err(|e| e.to_string())?;
    let Some(section) = config.get("craftingCPU") else { return Ok(CraftingCalculationBudget::default()); };
    if !section.is_table() { return Err("craftingCPU must be a TOML table".into()); }
    let Some(value) = section.get("craftingCalculationTimePerTick") else { return Ok(CraftingCalculationBudget::default()); };
    let value = value.as_integer().and_then(|n| i32::try_from(n).ok())
        .ok_or_else(|| "craftingCPU.craftingCalculationTimePerTick must be a signed 32-bit integer".to_string())?;
    Ok(CraftingCalculationBudget(value))
}
thread_local! { static CRAFTING_CALCULATION_BUDGET: RefCell<Option<Cache<CraftingCalculationBudget>>> = const { RefCell::new(None) }; }
pub fn crafting_calculation_time_per_tick() -> i32 {
    CRAFTING_CALCULATION_BUDGET.with(|c| cached(c, parse_crafting_calculation_budget)).0
}

/// Source CommonConfig settings shared by grid energy, automation and chunk
/// loading. Values are read independently of unrelated TOML sections.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gameplay {
    pub spatial_anchor_random_ticks: bool,
    pub annihilation_sky_dust: bool,
    pub forge_energy: f64,
    pub usage_multiplier: f64,
    pub grid_energy_per_node: f64,
    pub p2p_energy_tax: f64,
    pub p2p_transport_tax: f64,
}
impl Default for Gameplay {
    fn default() -> Self {
        Self {
            spatial_anchor_random_ticks: true,
            annihilation_sky_dust: true,
            forge_energy: 0.5,
            usage_multiplier: 1.0,
            grid_energy_per_node: 25.0,
            p2p_energy_tax: 0.025,
            p2p_transport_tax: 0.025,
        }
    }
}
fn parse_gameplay(text: &str) -> Result<Gameplay, String> {
    let config: toml::Value = toml::from_str(text).map_err(|e| e.to_string())?;
    let mut result = Gameplay::default();
    for (section, name, target) in [
        ("general", "spatialAnchorEnableRandomTicks", &mut result.spatial_anchor_random_ticks),
        ("crafting", "annihilationPlaneSkyDustGeneration", &mut result.annihilation_sky_dust),
    ] {
        if let Some(table) = config.get(section) {
            if !table.is_table() { return Err(format!("{section} must be a TOML table")); }
            if let Some(value) = table.get(name) {
                *target = value.as_bool().ok_or_else(|| format!("{section}.{name} must be boolean"))?;
            }
        }
    }
    if let Some(table) = config.get("powerRatios") {
        if !table.is_table() { return Err("powerRatios must be a TOML table".into()); }
        for (name, target, min, max) in [
            ("forgeEnergy", &mut result.forge_energy, f64::from_bits(1), f64::MAX),
            ("usageMultiplier", &mut result.usage_multiplier, 0.01, f64::MAX),
            ("gridEnergyStoragePerNode", &mut result.grid_energy_per_node, 1.0, 1_000_000.0),
            ("p2pTunnelEnergyTax", &mut result.p2p_energy_tax, 0.0, 1.0),
            ("p2pTunnelTransportTax", &mut result.p2p_transport_tax, 0.0, 1.0),
        ] {
            if let Some(value) = table.get(name) {
                let n = value.as_float().or_else(|| value.as_integer().map(|n| n as f64))
                    .ok_or_else(|| format!("powerRatios.{name} must be numeric"))?;
                if !n.is_finite() || !(min..=max).contains(&n) {
                    return Err(format!("powerRatios.{name} is outside source bounds"));
                }
                *target = n;
            }
        }
    }
    Ok(result)
}
thread_local! { static GAMEPLAY: RefCell<Option<Cache<Gameplay>>> = const { RefCell::new(None) }; }
pub fn gameplay() -> Gameplay { GAMEPLAY.with(|c| cached(c, parse_gameplay)) }

/// AE2's automation limit is a signed source integer, including zero and
/// negative values (which prevent Formation Plane entity placement).
#[derive(Clone, Copy)]
struct FormationEntityLimit(i32);
impl Default for FormationEntityLimit {
    fn default() -> Self { Self(128) }
}
fn parse_formation_entity_limit(text: &str) -> Result<FormationEntityLimit, String> {
    let config: toml::Value = toml::from_str(text).map_err(|e| e.to_string())?;
    let Some(section) = config.get("automation") else { return Ok(FormationEntityLimit::default()); };
    if !section.is_table() { return Err("automation must be a TOML table".into()); }
    let Some(value) = section.get("formationPlaneEntityLimit") else { return Ok(FormationEntityLimit::default()); };
    let value = value.as_integer().and_then(|n| i32::try_from(n).ok())
        .ok_or("automation.formationPlaneEntityLimit must be a source 32-bit integer")?;
    Ok(FormationEntityLimit(value))
}
thread_local! { static FORMATION_LIMIT: RefCell<Option<Cache<FormationEntityLimit>>> = const { RefCell::new(None) }; }
pub fn formation_entity_limit() -> i32 {
    FORMATION_LIMIT.with(|c| cached(c, parse_formation_entity_limit)).0
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Processing {
    pub charger_rate: f64,
    pub growth_interval: u64,
    pub fuel_energy: f64,
    pub vibration_min: f64,
    pub vibration_max: f64,
}
impl Default for Processing {
    fn default() -> Self { Self { charger_rate: 1.0, growth_interval: 10, fuel_energy: 5.0, vibration_min: 4.0, vibration_max: 40.0 } }
}
fn parse_processing(text: &str) -> Result<Processing, String> {
    let config: toml::Value = toml::from_str(text).map_err(|e|e.to_string())?;
    let mut result=Processing::default();
    for (section,name,target,min,max,integer) in [
        ("battery","chargerChargeRate",&mut result.charger_rate,0.1,10.0,false),
        ("vibrationChamber","baseEnergyPerFuelTick",&mut result.fuel_energy,0.1,1000.0,false),
        ("vibrationChamber","minEnergyPerGameTick",&mut result.vibration_min,0.0,1000.0,true),
        ("vibrationChamber","baseMaxEnergyPerGameTick",&mut result.vibration_max,1.0,1000.0,true),
    ] {
        if let Some(table)=config.get(section) {
            if !table.is_table() { return Err(format!("{section} must be a TOML table")); }
            if let Some(value)=table.get(name) {
                let n=if integer { value.as_integer().map(|v|v as f64) } else {value.as_float().or_else(||value.as_integer().map(|v|v as f64))}.ok_or_else(||format!("{section}.{name} has invalid numeric type"))?;
                if !n.is_finite() || !(min..=max).contains(&n) { return Err(format!("{section}.{name} is outside source bounds")); }
                *target=n;
            }
        }
    }
    if let Some(section)=config.get("crafting") {
        if !section.is_table() { return Err("crafting must be a TOML table".into()); }
        if let Some(value)=section.get("growthAccelerator") {
            let n=value.as_integer().filter(|v|(1..=100).contains(v)).ok_or("crafting.growthAccelerator is outside source integer bounds")?;
            result.growth_interval=n as u64;
        }
    }
    Ok(result)
}
thread_local! { static PROCESSING: RefCell<Option<Cache<Processing>>> = const { RefCell::new(None) }; }
pub fn processing() -> Processing { PROCESSING.with(|c|cached(c,parse_processing)) }

/// Source AEConfig.powerRatios passive generation. Invalid files follow the
/// existing common-config fallback rather than inventing a generator rate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PassivePower {pub resonance_rate:f64}
impl Default for PassivePower {fn default()->Self{Self{resonance_rate:20.0}}}
fn parse_passive_power(text:&str)->Result<PassivePower,String>{
    let config:toml::Value=toml::from_str(text).map_err(|e|e.to_string())?;
    let mut result=PassivePower::default();
    if let Some(table)=config.get("powerRatios") {
        if !table.is_table(){return Err("powerRatios must be a TOML table".into());}
        if let Some(value)=table.get("crystalResonanceGeneratorRate") {
            let rate=value.as_float().or_else(||value.as_integer().map(|v|v as f64)).ok_or("invalid resonance generation rate")?;
            if !rate.is_finite()||!(0.0..=1_000_000.0).contains(&rate){return Err("resonance generation rate outside source bounds".into());}
            result.resonance_rate=rate;
        }
    }
    Ok(result)
}
thread_local! {static PASSIVE_POWER:RefCell<Option<Cache<PassivePower>>>=const{RefCell::new(None)};}
pub fn passive_power()->PassivePower{PASSIVE_POWER.with(|c|cached(c,parse_passive_power))}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Batteries {
    pub charged_staff: f64,
    pub entropy_manipulator: f64,
    pub matter_cannon: f64,
    pub color_applicator: f64,
    pub portable_cell: f64,
}
impl Default for Batteries {
    fn default()->Self {Self{charged_staff:8000.0,entropy_manipulator:200000.0,matter_cannon:200000.0,color_applicator:20000.0,portable_cell:20000.0}}
}
fn parse_batteries(text:&str)->Result<Batteries,String> {
    let config:toml::Value=toml::from_str(text).map_err(|e|e.to_string())?;
    let mut result=Batteries::default();
    if let Some(section)=config.get("battery") {
        if !section.is_table() {return Err("battery must be a TOML table".into());}
        for (name,target) in [("chargedStaff",&mut result.charged_staff),("entropyManipulator",&mut result.entropy_manipulator),("matterCannon",&mut result.matter_cannon),("colorApplicator",&mut result.color_applicator),("portableCell",&mut result.portable_cell)] {
            if let Some(value)=section.get(name) {
                let n=value.as_integer().and_then(|n|i32::try_from(n).ok()).ok_or_else(||format!("battery.{name} must be a source 32-bit integer"))?;
                *target=f64::from(n);
            }
        }
    }
    Ok(result)
}
thread_local! { static BATTERIES: RefCell<Option<Cache<Batteries>>> = const { RefCell::new(None) }; }
pub fn batteries()->Batteries {BATTERIES.with(|c|cached(c,parse_batteries))}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DamageSettings {pub matter_cannon:bool,pub tiny_tnt:bool}
impl Default for DamageSettings {fn default()->Self{Self{matter_cannon:true,tiny_tnt:true}}}
fn parse_damage(text:&str)->Result<DamageSettings,String>{
    let config:toml::Value=toml::from_str(text).map_err(|e|e.to_string())?;let mut result=DamageSettings::default();
    if let Some(section)=config.get("general") {
        if !section.is_table(){return Err("general must be a TOML table".into());}
        for (key,target) in [("matterCannonBlockDamage",&mut result.matter_cannon),("tinyTntBlockDamage",&mut result.tiny_tnt)] {if let Some(value)=section.get(key){*target=value.as_bool().ok_or_else(||format!("general.{key} must be boolean"))?;}}
    }
    Ok(result)
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Condenser {pub matter_balls:f64,pub singularity:f64}
impl Default for Condenser {fn default()->Self{Self{matter_balls:256.0,singularity:256000.0}}}
fn parse_condenser(text:&str)->Result<Condenser,String>{
    let config:toml::Value=toml::from_str(text).map_err(|e|e.to_string())?;let mut result=Condenser::default();
    if let Some(section)=config.get("condenser") {
        if !section.is_table(){return Err("condenser must be a TOML table".into());}
        for (key,target) in [("matterBalls",&mut result.matter_balls),("singularity",&mut result.singularity)] {if let Some(value)=section.get(key){let n=value.as_integer().and_then(|v|i32::try_from(v).ok()).ok_or_else(||format!("condenser.{key} must be a source 32-bit integer"))?;*target=f64::from(n);}}
    }
    Ok(result)
}
thread_local! {
    static DAMAGE: RefCell<Option<Cache<DamageSettings>>> = const {RefCell::new(None)};
    static CONDENSER: RefCell<Option<Cache<Condenser>>> = const {RefCell::new(None)};
}
pub fn damage()->DamageSettings{DAMAGE.with(|c|cached(c,parse_damage))}
pub fn condenser()->Condenser{CONDENSER.with(|c|cached(c,parse_condenser))}

#[derive(Clone, Copy, Debug)]
pub struct Wireless {
    pub link: ae2_links_core::WirelessConfig,
    pub terminal_capacity: f64,
}
impl Default for Wireless {
    fn default() -> Self {
        Self {
            link: Default::default(),
            terminal_capacity: 1_600_000.0,
        }
    }
}
fn parse_wireless(text: &str) -> Result<Wireless, String> {
    let config: toml::Value = toml::from_str(text).map_err(|e| e.to_string())?;
    let mut result = Wireless::default();
    if let Some(section) = config.get("wireless") {
        if !section.is_table() {
            return Err("wireless must be a TOML table".into());
        }
        for (name, target) in [
            ("wirelessBaseRange", &mut result.link.base_range),
            (
                "wirelessBoosterRangeMultiplier",
                &mut result.link.range_multiplier,
            ),
            ("wirelessBoosterExp", &mut result.link.range_exponent),
            ("wirelessBaseCost", &mut result.link.base_power),
            ("wirelessCostMultiplier", &mut result.link.power_multiplier),
            ("wirelessHighWirelessCount", &mut result.link.high_count),
            (
                "wirelessTerminalDrainMultiplier",
                &mut result.link.terminal_drain,
            ),
        ] {
            if let Some(value) = section.get(name) {
                let n = value
                    .as_float()
                    .or_else(|| value.as_integer().map(|n| n as f64))
                    .ok_or_else(|| format!("wireless.{name} must be numeric"))?;
                if !n.is_finite() || n <= 0.0 {
                    return Err(format!("wireless.{name} must be positive and finite"));
                }
                *target = n;
            }
        }
    }
    if let Some(section) = config.get("battery") {
        if !section.is_table() {
            return Err("battery must be a TOML table".into());
        }
        if let Some(value) = section.get("wirelessTerminal") {
            let n = value
                .as_integer()
                .and_then(|n| i32::try_from(n).ok())
                .ok_or("battery.wirelessTerminal must be a source 32-bit integer")?;
            result.terminal_capacity = f64::from(n);
        }
    }
    Ok(result)
}
thread_local! { static WIRELESS: RefCell<Option<Cache<Wireless>>> = const { RefCell::new(None) }; }
pub fn wireless() -> Wireless {
    WIRELESS.with(|c| cached(c, parse_wireless))
}
#[derive(Clone, Copy, Default)]
struct DeveloperTools(bool);
fn parse_developer_tools(text: &str) -> Result<DeveloperTools, String> {
    let value: toml::Value = toml::from_str(text).map_err(|e| e.to_string())?;
    let Some(general) = value.get("general") else {
        return Ok(DeveloperTools::default());
    };
    if !general.is_table() {
        return Err("general must be a table".into());
    }
    match general.get("unsupportedDeveloperTools") {
        None => Ok(DeveloperTools::default()),
        Some(value) => value
            .as_bool()
            .map(DeveloperTools)
            .ok_or("general.unsupportedDeveloperTools must be boolean".into()),
    }
}
thread_local! { static DEVELOPER_TOOLS: RefCell<Option<Cache<DeveloperTools>>> = const { RefCell::new(None) }; }
/// Source production default. As in AE2, this gates creative visibility,
/// not block registration or command access to developer tools.
pub fn unsupported_developer_tools() -> bool {
    DEVELOPER_TOOLS.with(|c| cached(c, parse_developer_tools)).0
}
#[cfg(test)]
mod tests {
    #[test]
    fn crafting_calculation_budget_matches_source_integer_bounds() {
        use super::parse_crafting_calculation_budget as parse;
        assert_eq!(parse("").unwrap().0, 5);
        assert_eq!(parse("[craftingCPU]\ncraftingCalculationTimePerTick=-2147483648").unwrap().0, i32::MIN);
        assert_eq!(parse("[craftingCPU]\ncraftingCalculationTimePerTick=2147483647").unwrap().0, i32::MAX);
        assert!(parse("[craftingCPU]\ncraftingCalculationTimePerTick=2147483648").is_err());
        assert!(parse("[craftingCPU]\ncraftingCalculationTimePerTick=1.5").is_err());
    }
    #[test]
    fn gameplay_config_uses_source_defaults_bounds_and_independent_sections() {
        use super::*;
        assert_eq!(parse_gameplay("").unwrap(), Gameplay::default());
        let parsed = parse_gameplay("[general]\nspatialAnchorEnableRandomTicks=false\n[crafting]\nannihilationPlaneSkyDustGeneration=false\n[powerRatios]\nforgeEnergy=1.5\nusageMultiplier=0.01\ngridEnergyStoragePerNode=1000000\np2pTunnelEnergyTax=0\np2pTunnelTransportTax=1\n[wireless]\nwirelessBaseRange=42").unwrap();
        assert_eq!(parsed, Gameplay { spatial_anchor_random_ticks: false, annihilation_sky_dust: false,
            forge_energy: 1.5, usage_multiplier: 0.01, grid_energy_per_node: 1_000_000.0,
            p2p_energy_tax: 0.0, p2p_transport_tax: 1.0 });
        assert!(parse_gameplay("[powerRatios]\nforgeEnergy=4.9406564584124654e-324").is_ok());
        for bad in ["[general]\nspatialAnchorEnableRandomTicks=0", "[crafting]\nannihilationPlaneSkyDustGeneration='false'",
            "[powerRatios]\nforgeEnergy=0", "[powerRatios]\nusageMultiplier=0.009",
            "[powerRatios]\ngridEnergyStoragePerNode=0.99", "[powerRatios]\ngridEnergyStoragePerNode=1000001",
            "[powerRatios]\np2pTunnelEnergyTax=-0.01", "[powerRatios]\np2pTunnelTransportTax=1.01"] {
            assert!(parse_gameplay(bad).is_err(), "{bad}");
        }
    }
    #[test]
    fn formation_limit_uses_source_integer_default_and_bounds() {
        assert_eq!(super::parse_formation_entity_limit("").unwrap().0, 128);
        assert_eq!(super::parse_formation_entity_limit("[automation]\nformationPlaneEntityLimit=-1").unwrap().0, -1);
        assert!(super::parse_formation_entity_limit("[automation]\nformationPlaneEntityLimit=2147483648").is_err());
    }
    #[test]
    fn resonance_power_config_uses_source_defaults_and_bounds(){
        assert_eq!(super::parse_passive_power("").unwrap().resonance_rate,20.0);
        assert_eq!(super::parse_passive_power("[powerRatios]\ncrystalResonanceGeneratorRate=0").unwrap().resonance_rate,0.0);
        assert_eq!(super::parse_passive_power("[powerRatios]\ncrystalResonanceGeneratorRate=123.5").unwrap().resonance_rate,123.5);
        assert!(super::parse_passive_power("[powerRatios]\ncrystalResonanceGeneratorRate=-1").is_err());
        assert!(super::parse_passive_power("[powerRatios]\ncrystalResonanceGeneratorRate=1000001").is_err());
    }

    use super::*;
    #[test]
    fn wireless_source_settings_drive_range_power_and_terminal_capacity() {
        let c = parse_wireless("[wireless]\nwirelessBaseRange=32\nwirelessBoosterRangeMultiplier=2\nwirelessBoosterExp=2\nwirelessBaseCost=3\nwirelessCostMultiplier=4\nwirelessHighWirelessCount=8\nwirelessTerminalDrainMultiplier=0.25\n[battery]\nwirelessTerminal=12345").unwrap();
        assert_eq!(c.link.range(4), 64.0);
        assert_eq!(c.link.idle(8), 259.0);
        assert_eq!(c.link.drain(12.0), 3.0);
        assert_eq!(c.terminal_capacity, 12345.0);
        for bad in ["0", "-1", "nan", "inf", "'3'"] {
            assert!(parse_wireless(&format!("[wireless]\nwirelessBaseRange={bad}")).is_err());
        }
        assert!(parse_wireless("[battery]\nwirelessTerminal=2147483648").is_err());
        assert!(parse_wireless("[battery]\nwirelessTerminal=123.5").is_err());
    }
    #[test]
    fn world_generation_uses_source_case_and_boolean_settings() {
        let defaults = parse_world_generation("[general]\nchannels='DEFAULT'").unwrap();
        assert!(defaults.spawn_presses && !defaults.flawless_only);
        let options = parse_world_generation(
            "[worldGen]\nspawnPressesInMeteorites=false\nspawnFlawlessOnly=true",
        )
        .unwrap();
        assert!(!options.spawn_presses && options.flawless_only);
        assert!(parse_world_generation("[worldGen]\nspawnFlawlessOnly='true'").is_err());
    }
    #[test]
    fn spatial_source_config_accepts_fractional_exponents_and_numeric_forms() {
        assert_eq!(
            parse_spatial("[general]\nchannels='X4'").unwrap(),
            Spatial::default()
        );
        assert_eq!(
            parse_spatial("[spatialio]\nspatialPowerMultiplier=7\nspatialPowerExponent=0.5")
                .unwrap(),
            Spatial {
                power_multiplier: 7.0,
                power_exponent: 0.5
            }
        );
        assert!(parse_spatial("[spatialio]\nspatialPowerExponent=4.9406564584124654e-324").is_ok());
        for bad in ["0", "-1", "nan", "inf", "'1.35'"] {
            assert!(parse_spatial(&format!("[spatialio]\nspatialPowerExponent={bad}")).is_err());
        }
    }
}
#[cfg(test)] mod processing_tests {
    use super::*;
    #[test]
    fn source_processing_settings_accept_boundaries_and_reject_wrong_types() {
        assert_eq!(parse_processing("").unwrap(),Processing::default());
        let settings=parse_processing("[battery]\nchargerChargeRate=0.1\n[crafting]\ngrowthAccelerator=100\n[vibrationChamber]\nbaseEnergyPerFuelTick=1000\nminEnergyPerGameTick=0\nbaseMaxEnergyPerGameTick=1000").unwrap();
        assert_eq!(settings.charger_rate,0.1);assert_eq!(settings.growth_interval,100);assert_eq!(settings.vibration_min,0.0);assert_eq!(settings.fuel_energy,1000.0);
        for bad in ["[battery]\nchargerChargeRate=0","[crafting]\ngrowthAccelerator=0","[crafting]\ngrowthAccelerator=10.0","[vibrationChamber]\nminEnergyPerGameTick=-1","[vibrationChamber]\nbaseMaxEnergyPerGameTick=0","[vibrationChamber]\nbaseEnergyPerFuelTick=0.09"] {assert!(parse_processing(bad).is_err(),"{bad}");}
    }
}
#[cfg(test)] mod battery_tests {
    use super::*;
    #[test]
    fn source_batteries_are_independent_signed32_suppliers() {
        assert_eq!(parse_batteries("").unwrap(),Batteries::default());
        let v=parse_batteries("[battery]\nchargedStaff=1234\nentropyManipulator=5678\nmatterCannon=10000\ncolorApplicator=2000\nportableCell=3000").unwrap();assert_eq!(v,Batteries{charged_staff:1234.0,entropy_manipulator:5678.0,matter_cannon:10000.0,color_applicator:2000.0,portable_cell:3000.0});
        assert_eq!(parse_batteries("[battery]\nchargedStaff=-2147483648").unwrap().charged_staff,-2147483648.0);
        assert_eq!(parse_batteries("[battery]\nportableCell=0").unwrap().portable_cell,0.0);
        for bad in ["[battery]\nchargedStaff=1.0","[battery]\nportableCell=2147483648","[battery]\nmatterCannon=true"] {assert!(parse_batteries(bad).is_err());}
    }
}
#[cfg(test)] mod damage_and_condenser_tests {
    use super::*;
    #[test]
    fn damage_flags_and_runtime_condenser_defaults_match_source_common_config() {
        assert_eq!(parse_damage("").unwrap(),DamageSettings{matter_cannon:true,tiny_tnt:true});
        assert_eq!(parse_damage("[general]\nmatterCannonBlockDamage=false\ntinyTntBlockDamage=false").unwrap(),DamageSettings{matter_cannon:false,tiny_tnt:false});
        assert!(parse_damage("[general]\ntinyTntBlockDamage=0").is_err());
        assert_eq!(parse_condenser("").unwrap(),Condenser{matter_balls:256.0,singularity:256000.0});
        assert_eq!(parse_condenser("[condenser]\nmatterBalls=4\nsingularity=8").unwrap(),Condenser{matter_balls:4.0,singularity:8.0});
        assert_eq!(parse_condenser("[condenser]\nmatterBalls=0\nsingularity=-1").unwrap(),Condenser{matter_balls:0.0,singularity:-1.0});
        assert!(parse_condenser("[condenser]\nsingularity=2147483648").is_err());
    }
}
