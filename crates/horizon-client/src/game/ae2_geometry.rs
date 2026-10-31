//! Source-derived multipart collision and selection boxes, in block coordinates.
use super::ae2_visual_sync::Visual;
type B = [f64; 6];
fn turn(b:B, face:&str)->B {
    let [x,y,z,X,Y,Z]=b;
    match face {
        "north"=>[1.-X,y,1.-Z,1.-x,Y,1.-z],
        "east"=>[z,y,1.-X,Z,Y,1.-x],
        "west"=>[1.-Z,y,x,1.-z,Y,X],
        "up"=>[x,z,1.-Y,X,Z,1.-y],
        "down"=>[x,1.-Z,y,X,1.-z,Y],
        _=>b,
    }
}
fn b(a:[f64;6])->B { a.map(|v|v/16.) }
pub(super) fn boxes(v:&Visual, collision:bool)->Vec<B> {
    let mut out=Vec::new();
    if let Some(cable)=&v.cable {
        let (min,max)=if cable.contains("dense") {if collision {(4.9,11.1)} else {(3.,13.)}} else if cable.contains("glass") {(6.,10.)} else {(5.,11.)};
        out.push(b([min,min,min,max,max,max]));
        for face in &v.connections {
            out.push(turn(b([min,min,max,max,max,16.]),face));
        }
    }
    for p in &v.parts {
        let id=p.id.strip_prefix("ae2:").unwrap_or(&p.id);
        let parts:Vec<B>=match id {
            "terminal"|"crafting_terminal"|"pattern_encoding_terminal"|"pattern_access_terminal"|"storage_monitor"|"conversion_monitor"|"monitor"|"semi_dark_monitor"|"dark_monitor" => vec![b([2.,2.,14.,14.,14.,16.]),b([4.,4.,13.,12.,12.,14.])],
            "export_bus" => vec![b([4.,4.,12.,12.,12.,14.]),b([5.,5.,14.,11.,11.,15.]),b([6.,6.,15.,10.,10.,16.]),b([6.,6.,11.,10.,10.,12.])],
            "import_bus" => vec![b([6.,6.,11.,10.,10.,13.]),b([5.,5.,13.,11.,11.,14.]),b([4.,4.,14.,12.,12.,16.])],
            "cable_pattern_provider"|"cable_interface" => vec![b([2.,2.,14.,14.,14.,16.]),b([5.,5.,12.,11.,11.,14.])],
            "storage_bus" => vec![b([3.,3.,15.,13.,13.,16.]),b([2.,2.,14.,14.,14.,15.]),b([5.,5.,12.,11.,11.,14.])],
            id if id.ends_with("_p2p_tunnel") => vec![b([5.,5.,12.,11.,11.,13.]),b([3.,3.,13.,13.,13.,14.]),b([2.,2.,14.,14.,14.,16.])],
            "level_emitter" => vec![b([7.,7.,11.,9.,9.,16.])],
            "cable_energy_acceptor" => vec![b([2.,2.,14.,14.,14.,16.]),b([4.,4.,12.,12.,12.,14.])],
            "toggle_bus"|"inverted_toggle_bus" => vec![b([6.,6.,11.,10.,10.,16.])],
            "quartz_fiber" => vec![b([6.,6.,10.,10.,10.,16.])],
            "cable_anchor" => vec![b([7.,7.,10.,9.,9.,if v.facades.iter().any(|f|f.face==p.face){14.}else{16.}])],
            "annihilation_plane" if collision => vec![b([0.,0.,14.,16.,16.,15.5])],
            "annihilation_plane"|"formation_plane" => {
                let [left,right,down,up]=p.plane_edges;
                vec![b([5.,5.,14.,11.,11.,15.]), b([if left {0.} else {1.},if down {0.} else {1.},15.,if right {16.} else {15.},if up {16.} else {15.},16.])]
            },
            // Keep unknown part types solid until their source shape is ported.
            _ => vec![b([0.,0.,0.,16.,16.,16.])],
        };
        out.extend(parts.into_iter().map(|b|turn(b,&p.face)));
    }
    for facade in &v.facades {
        out.push(turn(b([0.,0.,15.,16.,16.,16.]),&facade.face));
    }
    out
}
#[cfg(test)] mod tests {
 use super::*; use crate::game::ae2_visual_sync::{Part,Visual};
 #[test] fn source_dense_and_terminal_shapes_rotate() {
  let v=Visual{pos:[0,0,0],kind:"ae2:cable_bus".into(),slots:vec![],facades:vec![],parts:vec![Part{id:"ae2:terminal".into(),face:"north".into(),online:true,powered:true,plane_edges:[false;4],monitor:None,monitor_locked:false}],cable:Some("ae2:fluix_smart_dense_cable".into()),powered:true,connections:vec!["east".into()],channels:0,paint_dots:vec![],charger_working:false,crank_angle:0,crafting_job:None,painted_color:None,anchor_chunks:vec![],anchor_pending:None};
  let c=boxes(&v,true); let o=boxes(&v,false);
  assert_eq!(c[0],[4.9/16.,4.9/16.,4.9/16.,11.1/16.,11.1/16.,11.1/16.]);
  assert_eq!(o[0],[3./16.,3./16.,3./16.,13./16.,13./16.,13./16.]);
  assert_eq!(c[2],[2./16.,2./16.,0.,14./16.,14./16.,2./16.]);
 }
}
