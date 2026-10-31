//! Spatial Anchor overlay topology from AE2 19.2.18 OverlayRenderer
//! (LGPL-3.0-or-later). Geometry comes solely from the server's owned tickets.
use std::collections::HashSet;
use horizon_client::gizmos::Line;
use super::Game;

const COLOR: u32 = 0x80915dcd; // AEColor.TRANSPARENT.mediumVariant, alpha 0x80.
const OCCLUDED: u32 = 0x30ffffff;

fn geometry(chunks: &[[i32;2]], min_y: i32, max_y: i32, camera:[f64;3], faces:&mut Vec<[f32;7]>, lines:&mut Vec<Line>, occluded:&mut Vec<Line>) {
 let set:HashSet<_>=chunks.iter().map(|c|(c[0],c[1])).collect();
 let col=[145./255.,93./255.,205./255.,128./255.];
 for &(cx,cz) in &set {
  let x=(cx as f64)*16.;let z=(cz as f64)*16.;let a=min_y as f64;let b=max_y as f64;let (x2,z2)=(x+16.,z+16.);
  let n=!set.contains(&(cx,cz-1));let s=!set.contains(&(cx,cz+1));let w=!set.contains(&(cx-1,cz));let e=!set.contains(&(cx+1,cz));
  let mut face=|p:[[f64;3];4]|{for i in [0,1,2,0,2,3] {faces.push([(p[i][0]-camera[0]) as f32,(p[i][1]-camera[1]) as f32,(p[i][2]-camera[2]) as f32,col[0],col[1],col[2],col[3]]);}};
  let mut edge=|p:[f64;3],q:[f64;3]|{lines.push(Line{start:p,end:q,color:COLOR,width:3.});occluded.push(Line{start:p,end:q,color:OCCLUDED,width:3.});};
  if n {face([[x,a,z],[x2,a,z],[x2,b,z],[x,b,z]]);edge([x,a,z],[x2,a,z]);edge([x2,b,z],[x,b,z]);}
  if s {face([[x2,a,z2],[x,a,z2],[x,b,z2],[x2,b,z2]]);edge([x2,a,z2],[x,a,z2]);edge([x,b,z2],[x2,b,z2]);}
  if w {face([[x,a,z],[x,a,z2],[x,b,z2],[x,b,z]]);edge([x,a,z],[x,a,z2]);edge([x,b,z2],[x,b,z]);}
  if e {face([[x2,a,z2],[x2,a,z],[x2,b,z],[x2,b,z2]]);edge([x2,a,z2],[x2,a,z]);edge([x2,b,z],[x2,b,z2]);}
  if n||w {edge([x,a,z],[x,b,z]);}if n||e {edge([x2,b,z],[x2,a,z]);}
  if s||e {edge([x2,a,z2],[x2,b,z2]);}if s||w {edge([x,b,z2],[x,a,z2]);}
  face([[x,a,z],[x2,a,z],[x2,a,z2],[x,a,z2]]);
 }
}
impl Game<'_> {
 pub(super) fn ae2_anchor_overlay_geometry(&self,camera:[f64;3])->(Vec<[f32;7]>,Vec<Line>,Vec<Line>) {
  let mut faces=Vec::new();let mut lines=Vec::new();let mut occluded=Vec::new();
  let min=self.dim.min_y();let max=self.dim.max_y()+1;
  if self.remote.is_some() {
   for v in self.ae2_visual_cache.values() {if !v.anchor_chunks.is_empty(){geometry(&v.anchor_chunks,min,max,camera,&mut faces,&mut lines,&mut occluded);}}
  } else {
   for (&p,e) in self.world.block_entities.iter() {
    let Some(mut v)=super::ae2_visual_sync::visual(p,e,false) else{continue};self.ae2_enrich_anchor_visual(&mut v);
    if !v.anchor_chunks.is_empty(){geometry(&v.anchor_chunks,min,max,camera,&mut faces,&mut lines,&mut occluded);}
   }
  }
  (faces,lines,occluded)
 }
}
#[cfg(test)] mod tests {
 use super::*;
 #[test] fn source_overlay_culls_shared_faces_and_keeps_one_bottom_per_chunk(){
  let mut f=vec![];let mut l=vec![];let mut o=vec![];
  geometry(&[[0,0],[1,0]],-64,320,[0.;3],&mut f,&mut l,&mut o);
  // Two cubes: six outer side faces plus two bottoms; no interior wall.
  assert_eq!(f.len(),8*6);assert_eq!(l.len(),o.len());
  assert!(f.iter().all(|v|v[3..]==[145./255.,93./255.,205./255.,128./255.]));
  assert!(f.iter().any(|v|v[1]==-64.)&&f.iter().any(|v|v[1]==320.));
 }
}
