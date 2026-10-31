//! AE2 dimensional-link and spatial-region rules, derived from AE2 19.2.18,
//! commit db17504a86128fdf3dae31f5fb7a112a646e0b93. Copyright AE2 contributors.
//! LGPL-3.0-or-later. Physical world transactions are implemented by the host.
pub mod anchor;
pub mod anchor_grid;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone,Copy,Debug,Default,PartialEq,Eq,PartialOrd,Ord,Serialize,Deserialize)]
pub struct Pos(pub i32,pub i32,pub i32);
impl Pos{
 pub fn offset(self,d:[i32;3])->Option<Self>{Some(Self(self.0.checked_add(d[0])?,self.1.checked_add(d[1])?,self.2.checked_add(d[2])?))}
 pub fn neighbours(self)->impl Iterator<Item=Self>{[[1,0,0],[-1,0,0],[0,1,0],[0,-1,0],[0,0,1],[0,0,-1]].into_iter().filter_map(move|d|self.offset(d))}
 fn axis(self,a:usize)->i32{[self.0,self.1,self.2][a]}
}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum QuantumBlock{Ring,Link}
#[derive(Clone,Debug,PartialEq,Eq)]
pub struct QuantumFrame{pub center:Pos,pub normal:usize,pub members:BTreeSet<Pos>}
impl QuantumFrame{
 /// QuantumCalculator.updateBlockEntities uses BlockPos.betweenClosed, whose
 /// iteration advances X first, then Y, then Z.
 pub fn formation_order(&self)->Vec<Pos>{
  let mut members:Vec<_>=self.members.iter().copied().collect();
  members.sort_by_key(|p|(p.2,p.1,p.0));
  members
 }
 /// QuantumCluster.destroy clears the center first, followed by its stored
 /// ring array, which was populated in formation order.
 pub fn destruction_order(&self)->Vec<Pos>{
  let mut members=vec![self.center];
  members.extend(self.formation_order().into_iter().filter(|p|*p!=self.center));
  members
 }
 /// QuantumCalculator accepts exactly a connected 3x3 plane: eight ring blocks
 /// around one link. A touching extra block invalidates the multiblock.
 pub fn scan(blocks:&BTreeMap<Pos,QuantumBlock>)->Vec<Self>{
  let mut left=blocks.keys().copied().collect::<BTreeSet<_>>();let mut frames=Vec::new();
  while let Some(first)=left.pop_first(){let mut members=BTreeSet::new();let mut queue=VecDeque::from([first]);while let Some(p)=queue.pop_front(){members.insert(p);for n in p.neighbours(){if left.remove(&n){queue.push_back(n);}}}
   if members.len()!=9{continue;}
   let mins=[0,1,2].map(|a|members.iter().map(|p|p.axis(a)).min().unwrap());
   let maxs=[0,1,2].map(|a|members.iter().map(|p|p.axis(a)).max().unwrap());
   let ext=[0,1,2].map(|a|i64::from(maxs[a])-i64::from(mins[a]));
   let Some(normal)=ext.iter().position(|v|*v==0)else{continue;};if ext.iter().filter(|v|**v==2).count()!=2{continue;}
   let center=Pos(mins[0]+(ext[0]/2)as i32,mins[1]+(ext[1]/2)as i32,mins[2]+(ext[2]/2)as i32);
   if members.iter().all(|p|blocks[p]==if *p==center{QuantumBlock::Link}else{QuantumBlock::Ring}){frames.push(Self{center,normal,members});}
  }frames
 }
 /// In-world connections for center/corners are restricted to ring members;
 /// edge middles also expose connections to adjacent external cables.
 pub fn faces(&self,p:Pos)->u8{
  if !self.members.contains(&p){return 0;}
  let differing=[p.0!=self.center.0,p.1!=self.center.1,p.2!=self.center.2].into_iter().filter(|v|*v).count();
  if differing==1{return 63;}
  [[1,0,0],[-1,0,0],[0,1,0],[0,-1,0],[0,0,1],[0,0,-1]].into_iter().enumerate().fold(0,|mask,(i,d)|mask|if p.offset(d).is_some_and(|p|self.members.contains(&p)){1<<i}else{0})
 }
}
#[derive(Clone,Copy,Debug,Serialize,Deserialize)]
pub struct WirelessConfig{pub base_range:f64,pub range_multiplier:f64,pub range_exponent:f64,pub base_power:f64,pub power_multiplier:f64,pub high_count:f64,pub terminal_drain:f64}
impl Default for WirelessConfig{fn default()->Self{Self{base_range:16.0,range_multiplier:1.0,range_exponent:1.5,base_power:8.0,power_multiplier:1.0,high_count:64.0,terminal_drain:1.0}}}
impl WirelessConfig{
 pub fn range(&self,boosters:u32)->f64{self.base_range+self.range_multiplier*f64::from(boosters).powf(self.range_exponent)}
 pub fn idle(&self,boosters:u32)->f64{self.base_power+self.power_multiplier*f64::from(boosters).powf(1.0+f64::from(boosters)/self.high_count)}
 pub fn in_range(&self,boosters:u32,a:[f64;3],b:[f64;3])->bool{a.into_iter().chain(b).all(f64::is_finite)&&(0..3).map(|i|(a[i]-b[i]).powi(2)).sum::<f64>()<self.range(boosters).powi(2)}
 pub fn drain(&self,distance:f64)->f64{self.terminal_drain*distance}
}
#[derive(Clone,Debug,PartialEq,Eq)]
pub struct PylonLine{pub min:Pos,pub max:Pos,pub axis:usize,pub count:usize}
impl PylonLine{
 pub fn scan(positions:&BTreeSet<Pos>)->Vec<Self>{
  let mut left=positions.clone();let mut out=Vec::new();while let Some(start)=left.pop_first(){let mut queue=VecDeque::from([start]);let mut group=vec![];while let Some(p)=queue.pop_front(){group.push(p);for n in p.neighbours(){if left.remove(&n){queue.push_back(n);}}}
   let min=Pos(group.iter().map(|p|p.0).min().unwrap(),group.iter().map(|p|p.1).min().unwrap(),group.iter().map(|p|p.2).min().unwrap());let max=Pos(group.iter().map(|p|p.0).max().unwrap(),group.iter().map(|p|p.1).max().unwrap(),group.iter().map(|p|p.2).max().unwrap());
   let axes=(0..3).filter(|a|min.axis(*a)!=max.axis(*a)).collect::<Vec<_>>();if axes.len()==1{let axis=axes[0];if i64::from(max.axis(axis))-i64::from(min.axis(axis))+1==group.len()as i64{out.push(Self{min,max,axis,count:group.len()});}}
  }out
 }
}
#[derive(Clone,Debug,PartialEq)]
pub struct SpatialRegion{pub min:Pos,pub max:Pos,pub size:[i32;3],pub efficiency:f64,pub power:i64}
impl SpatialRegion{
 /// SpatialPylonService's region, boundary validity, efficiency and power law.
 pub fn from_lines(lines:&[PylonLine],multiplier:f64,exponent:f64)->Result<Self,&'static str>{
  if lines.is_empty()||!multiplier.is_finite()||multiplier<=0.0||!exponent.is_finite()||exponent<=0.0{return Err("invalid spatial parameters");}
  let min=Pos(lines.iter().map(|l|l.min.0).min().unwrap(),lines.iter().map(|l|l.min.1).min().unwrap(),lines.iter().map(|l|l.min.2).min().unwrap());let max=Pos(lines.iter().map(|l|l.max.0).max().unwrap(),lines.iter().map(|l|l.max.1).max().unwrap(),lines.iter().map(|l|l.max.2).max().unwrap());
  let mut lengths=[0i64;3];for(a,n)in lengths.iter_mut().enumerate(){*n=i64::from(max.axis(a))-i64::from(min.axis(a));if *n<=1||*n>i64::from(i32::MAX){return Err("spatial region has no interior");}}
  for l in lines{let other=(0..3).filter(|a|*a!=l.axis).collect::<Vec<_>>();let edge=|p:Pos|other.iter().any(|a|p.axis(*a)==min.axis(*a)||p.axis(*a)==max.axis(*a));if !edge(l.min)||!edge(l.max){return Err("pylon crosses region interior");}}
  let area=lengths[0].saturating_mul(lengths[1]).saturating_add(lengths[0].saturating_mul(lengths[2])).saturating_add(lengths[1].saturating_mul(lengths[2]));let required=(area.saturating_mul(3)/8).max(6);let efficiency=(lines.iter().map(|l|l.count).sum::<usize>()as f64/required as f64).clamp(0.0,1.0);
  let minimum=lengths.into_iter().map(|n|n as f64).product::<f64>()*multiplier;let power=minimum.powf(1.0+(exponent-1.0)*(1.0-efficiency))as i64;
  Ok(Self{min,max,size:lengths.map(|n|(n-1)as i32),efficiency,power})
 }
 pub fn fits(&self,max_dimension:i32)->bool{self.size.iter().all(|n|*n<=max_dimension)}
}
/// SpatialStoragePlot uses one 512-block region per cell, spiralling by bits.
pub fn plot_origin(id:i32)->Result<Pos,&'static str>{if id<0{return Err("negative plot id");}let signs=id&3;let mut bits=id>>2;let(mut scale,mut x,mut z)=(1i64,256i64,256i64);while bits!=0{x+=512*scale*i64::from(bits&1);z+=512*scale*i64::from((bits>>1)&1);bits>>=2;scale*=2;}if signs&1!=0{z=-z;}if signs&2!=0{x=-x;}Ok(Pos(i32::try_from(x-64).map_err(|_|"plot overflow")?,64,i32::try_from(z-64).map_err(|_|"plot overflow")?))}

#[cfg(test)]mod tests{use super::*;
 #[test]fn quantum_source_update_and_destroy_visit_members_in_distinct_orders(){
  let mut blocks=BTreeMap::new();for x in -1..=1{for y in -1..=1{blocks.insert(Pos(x,y,0),if x==0&&y==0{QuantumBlock::Link}else{QuantumBlock::Ring});}}
  let frame=QuantumFrame::scan(&blocks).pop().unwrap();
  assert_eq!(frame.formation_order(),vec![Pos(-1,-1,0),Pos(0,-1,0),Pos(1,-1,0),Pos(-1,0,0),Pos(0,0,0),Pos(1,0,0),Pos(-1,1,0),Pos(0,1,0),Pos(1,1,0)]);
  assert_eq!(frame.destruction_order(),vec![Pos(0,0,0),Pos(-1,-1,0),Pos(0,-1,0),Pos(1,-1,0),Pos(-1,0,0),Pos(1,0,0),Pos(-1,1,0),Pos(0,1,0),Pos(1,1,0)]);
 }
 #[test]fn quantum_frames_validate_plane_and_reject_extra_blocks(){let mut b=BTreeMap::new();for x in -1..=1{for y in -1..=1{b.insert(Pos(x,y,0),if x==0&&y==0{QuantumBlock::Link}else{QuantumBlock::Ring});}}let f=QuantumFrame::scan(&b);assert_eq!(f.len(),1);assert_eq!(f[0].faces(Pos(0,0,0)),15);assert_eq!(f[0].faces(Pos(1,0,0)),63);b.insert(Pos(2,0,0),QuantumBlock::Ring);assert!(QuantumFrame::scan(&b).is_empty());}
 #[test]fn wireless_source_defaults_and_boundaries(){let c=WirelessConfig::default();assert_eq!(c.range(0),16.0);assert_eq!(c.range(64),528.0);assert_eq!(c.idle(64),4104.0);assert!(c.in_range(0,[0.0;3],[15.999,0.0,0.0]));assert!(!c.in_range(0,[0.0;3],[16.0,0.0,0.0]));assert!(!c.in_range(0,[0.0;3],[16.0001,0.0,0.0]));}
 #[test]fn plot_bit_layout_matches_source_quadrants(){assert_eq!(plot_origin(0).unwrap(),Pos(192,64,192));assert_eq!(plot_origin(1).unwrap(),Pos(192,64,-320));assert_eq!(plot_origin(2).unwrap(),Pos(-320,64,192));assert_eq!(plot_origin(4).unwrap(),Pos(704,64,192));assert!(plot_origin(-1).is_err());}
 #[test]fn spatial_source_positive_exponent_below_one_is_supported(){
  let lines=[PylonLine{min:Pos(0,0,0),max:Pos(0,9,0),axis:1,count:10},PylonLine{min:Pos(9,0,9),max:Pos(9,9,9),axis:1,count:10}];
  let r=SpatialRegion::from_lines(&lines,1.0,0.5).unwrap();
  assert_eq!(r.size,[8,8,8]);assert_eq!(r.efficiency,20.0/91.0);assert_eq!(r.power,55);
  assert!(SpatialRegion::from_lines(&lines,1.0,0.0).is_err());
 }
 #[test]fn spatial_power_and_capacity_follow_full_pylon_edges(){let mut p=BTreeSet::new();for y in 0..=3{for(x,z)in [(0,0),(0,3),(3,0),(3,3)]{p.insert(Pos(x,y,z));}}let lines=PylonLine::scan(&p);let r=SpatialRegion::from_lines(&lines,1250.0,1.35).unwrap();assert_eq!(r.size,[2,2,2]);assert_eq!(r.efficiency,1.0);assert_eq!(r.power,33750);assert!(r.fits(2));assert!(!r.fits(1));}
}
