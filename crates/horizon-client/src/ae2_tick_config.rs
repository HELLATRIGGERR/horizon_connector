//! AE2 19.2.18 `[tickRates]` request bounds. Unsupported nonpositive or
//! reversed ranges fall back per device; AE2 itself does not validate them.
use std::{cell::RefCell,path::PathBuf,time::{Duration,Instant}};
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
#[repr(usize)]
pub enum Kind { Interface,ImportBus,ExportBus,AnnihilationPlane,MeTunnel,Inscriber,Charger,IoPort,VibrationChamber,StorageBus,ItemTunnel,LightTunnel }
const SPECS:[(&str,(u64,u64));12]=[
 ("Interface",(5,120)),("ImportBus",(5,40)),("ExportBus",(5,60)),("AnnihilationPlane",(2,120)),("METunnel",(5,20)),("Inscriber",(1,20)),("Charger",(10,10)),("IOPort",(1,5)),("VibrationChamber",(10,40)),("StorageBus",(5,60)),("ItemTunnel",(5,60)),("LightTunnel",(5,60))];
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct Settings { values:[(u64,u64);12] }
impl Default for Settings {fn default()->Self{Self{values:SPECS.map(|(_,bounds)|bounds)}}}
fn parse(text:&str)->Result<Settings,String>{
 let config:toml::Value=toml::from_str(text).map_err(|e|e.to_string())?;let mut out=Settings::default();
 let Some(section)=config.get("tickRates") else{return Ok(out)};
 if !section.is_table(){return Err("tickRates must be a table".into())}
 for (i,(name,defaults)) in SPECS.iter().enumerate(){
  let read=|suffix:&str,default:u64|->Result<u64,String>{let key=format!("{name}{suffix}");match section.get(&key){None=>Ok(default),Some(v)=>v.as_integer().filter(|n|*n>0 && *n<=i32::MAX as i64).map(|n|n as u64).ok_or_else(||format!("tickRates.{key} is outside supported positive i32 bounds"))}};
  match (read("Min",defaults.0),read("Max",defaults.1)) { (Ok(min),Ok(max)) if min<=max=>out.values[i]=(min,max), (a,b)=>log::warn!("unsupported AE2 tick-rate bounds for {name}: min={a:?}, max={b:?}; using source defaults") }
 }
 Ok(out)
}
struct Cache {path:PathBuf,checked:Instant,value:Settings}
thread_local!{static CACHE:RefCell<Option<Cache>>=const{RefCell::new(None)};}
/// Reads the same local common config as other AE2 settings. Existing Java
/// TickingRequests retain their captured bounds until their node is recreated.
pub fn rates(kind:Kind)->(u64,u64){CACHE.with(|cache|{let path=crate::logging::game_home().join("config/ae2-common.toml");let mut cache=cache.borrow_mut();if let Some(c)=cache.as_ref(){if c.path==path&&c.checked.elapsed()<Duration::from_secs(1){return c.value.values[kind as usize]}}
 let value=match std::fs::read_to_string(&path){Ok(s)=>parse(&s).unwrap_or_else(|error|{log::warn!("invalid AE2 tick config: {error}");Settings::default()}),Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Settings::default(),Err(e)=>{log::warn!("cannot read AE2 tick config: {e}");Settings::default()}};let result=value.values[kind as usize];*cache=Some(Cache{path,checked:Instant::now(),value});result})}
/// Java adds signed i32s before division and clamps the result. This matters
/// even for valid positive ordered bounds when their sum overflows.
pub fn initial_rate(min:u64,max:u64)->u64 {let raw=(min as i32).wrapping_add(max as i32)/2;(raw as i64).clamp(min as i64,max as i64) as u64}
#[cfg(test)]mod tests{use super::*;
 #[test]fn configured_rates_are_isolated_per_source_key(){let s=parse("[tickRates]\nImportBusMin=2\nImportBusMax=3\nChargerMin=4\nChargerMax=9\nExportBusMin=0\nExportBusMax=3\nIOPortMin=8\nIOPortMax=4\n").unwrap();assert_eq!(s.values[Kind::ImportBus as usize],(2,3));assert_eq!(s.values[Kind::Charger as usize],(4,9));assert_eq!(s.values[Kind::ExportBus as usize],(5,60));assert_eq!(s.values[Kind::IoPort as usize],(1,5));assert_eq!(s.values[Kind::Interface as usize],(5,120));}
 #[test]fn java_signed_midpoint_overflow_is_clamped(){assert_eq!(initial_rate(5,40),22);assert_eq!(initial_rate(i32::MAX as u64,i32::MAX as u64),i32::MAX as u64);assert_eq!(initial_rate(1,i32::MAX as u64),1);}
}
