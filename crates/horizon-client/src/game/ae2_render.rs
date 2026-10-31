//! Client-side AE2 drive cell overlays from server-approved visual snapshots.
//! The chassis uses source geometry. Cell item geometry and LED quads are a
//! partial approximation of AE2's dynamic DriveBakedModel and multipart geometry.
use super::{
    Game,
    ae2_visual_sync::{Visual, visual},
};
use horizon_assets::item_model::DisplayContext;
use horizon_client::{block_entity_render::vertex_light, items::ItemStack, mesher::Vertex};
use std::f32::consts::PI;

/// AE2's four-character slot amount for integral item quantities.
fn monitor_amount(amount:u64,craftable:bool,fluid:bool)->String {
    if amount==0 && craftable {return "Craft".into();}
    if fluid && amount%1000!=0 && amount<10_000_000 {
        let units=amount as f64/1000.0;
        let digits:usize=if units>=100.0 {0} else if units>=10.0 {1} else if units>=1.0 {2} else {3};
        let scale=10_f64.powi(digits as i32);
        let s=format!("{:.digits$}",(units*scale).floor()/scale,digits=digits);
        return if s.contains('.') {s.trim_end_matches('0').trim_end_matches('.').to_owned()} else {s};
    }
    let mut base=if fluid {amount/1000} else {amount};
    if base.to_string().len()<=4 {return base.to_string();}
    let suffix=['K','M','G','T','P','E'];let mut last=base;let mut index=0;
    while base.to_string().len()+1>4 && index<suffix.len() {last=base;base/=1000;index+=1;}
    let decimal=last/100%10;
    if decimal!=0 && format!("{base}.{decimal}{}",suffix[index-1]).len()<=4 {format!("{base}.{decimal}{}",suffix[index-1])} else {format!("{base}{}",suffix[index-1])}
}
fn crafting_palette(color:&str)->([u32;3],u32) {
    let (dark,medium,bright)=match color {
        "white"=>(0xb4b4b4,0xe0e0e0,0xf9f9f9),"light_gray"=>(0x7e7e7e,0xa09fa0,0xc4c4c4),"gray"=>(0x4f4f4f,0x6c6b6c,0x949294),"black"=>(0x131313,0x272727,0x3b3b3b),
        "lime"=>(0x4ec04e,0x70e259,0xb3f86d),"yellow"=>(0xffcf40,0xffe359,0xf4ff80),"orange"=>(0xd9782f,0xeca23c,0xf2ba49),"brown"=>(0x6e4a12,0x7e5c16,0x8e6e1a),
        "red"=>(0xaa212b,0xd73e42,0xf07665),"pink"=>(0xd86eaa,0xff99bb,0xfbcad5),"magenta"=>(0xc15189,0xd5719c,0xe69ebf),"purple"=>(0x6e5cb8,0x915dcd,0xb06fdd),
        "blue"=>(0x337ff0,0x3894ff,0x40c1ff),"light_blue"=>(0x69b9ff,0x70d2ff,0x80f7ff),"cyan"=>(0x22b0ae,0x2fccb7,0x65e8c9),"green"=>(0x079b6b,0x17b86d,0x32d850),_ =>(0x5a479e,0x915dcd,0xe2a3e3),
    };
    let abgr=|rgb:u32|0xff000000|((rgb&255)<<16)|(rgb&0xff00)|((rgb>>16)&255);
    ([abgr(dark),abgr(medium),abgr(bright)],if color=="black" {0xffffffff} else {0xff000000})
}
#[cfg(test)]
mod monitor_tests {
    use super::{monitor_amount,crafting_palette};
    #[test]
    fn source_slot_amount_labels() {
        assert_eq!(monitor_amount(0,true,false),"Craft");
        assert_eq!(monitor_amount(1234,false,false),"1234");
        assert_eq!(monitor_amount(12_345,false,false),"12K");
        assert_eq!(monitor_amount(1_234_567,false,false),"1.2M");
        assert_eq!(monitor_amount(1_500,false,true),"1.5");
    }
    #[test]
    fn source_monitor_palette_has_black_text_except_black_monitor() {
        assert_eq!(crafting_palette("fluix"),([0xff9e475a,0xffcd5d91,0xffe3a3e2],0xff000000));
        assert_eq!(crafting_palette("black").1,0xffffffff);
        assert_ne!(crafting_palette("red").0,crafting_palette("fluix").0);
    }
}

fn orientation(facing: &str, spin: u8) -> glam::Mat4 {
    // Rotations match AE2's drive blockstate variants (quarter turns).
    let (x, y) = match facing {
        "up" => (270, 0),
        "down" => (90, 0),
        "east" => (0, 90),
        "south" => (0, 180),
        "west" => (0, 270),
        _ => (0, 0),
    };
    let z = match facing {
        "up" => [180, 90, 0, 270][spin as usize % 4],
        "south" => [0, 90, 180, 270][spin as usize % 4],
        _ => [0, 270, 180, 90][spin as usize % 4],
    };
    glam::Mat4::from_rotation_y(-(y as f32) * PI / 180.0)
        * glam::Mat4::from_rotation_x(-(x as f32) * PI / 180.0)
        * glam::Mat4::from_rotation_z(-(z as f32) * PI / 180.0)
}
// CraftingCubeBakedModel deliberately overlaps its 3/16 frame by 0.01
// source-model units to prevent a seam at the edge.
fn cpu_inner_bounds(negative_connected: bool, positive_connected: bool) -> (f32, f32) {
    (if negative_connected {0.} else {2.99/16.}, if positive_connected {1.} else {13.01/16.})
}

impl Game<'_> {
    pub(super) fn ae2_visual_geometry(&self, camera: glam::DVec3, out: &mut Vec<Vertex>) {
        let owned: Vec<Visual>;
        let visuals: Vec<&Visual> = if self.remote.is_some() {
            self.ae2_visual_cache.values().collect()
        } else {
            let network = self.ae2_network();
            let analysis = network.analyze().ok();
            let mut monitor_cache=super::ae2_visual_sync::MonitorCache::new();
            let mut cpu_jobs=super::ae2_visual_sync::CpuJobCache::new();
            owned = self
                .world
                .block_entities
                .iter()
                .filter_map(|(&p, e)| {
                    let powered = self
                        .ae2_local_address(p)
                        .and_then(|a| analysis.as_ref()?.nodes.get(&a.network_pos()))
                        .is_some_and(|s| s.online);
                    let mut v = visual(p, e, powered)?;
                    self.ae2_enrich_anchor_visual(&mut v);
                    if v.kind=="ae2:crank" {v.crank_angle=self.ae2_cranks.get(&(self.dim,p)).map_or(0,|s|s.angle);}
                    if let Some(a) = &analysis { self.ae2_enrich_visual(&mut v, &network, a); self.ae2_enrich_monitor_visual(&mut v,&network,a,&mut monitor_cache,&mut cpu_jobs); }
                    Some(v)
                })
                .collect();
            owned.iter().collect()
        };
        self.ae2_visual_geometry_from(camera, &visuals, &self.world, out);
    }
    /// Render supplied visuals against an isolated world. The live caller uses
    /// its original world; the Guide supplies a read-only structure snapshot.
    pub(super) fn ae2_visual_geometry_from(
        &self,
        camera: glam::DVec3,
        visuals: &[&Visual],
        world: &horizon_client::world::World,
        out: &mut Vec<Vertex>,
    ) {
        let white = self.gui.atlas.sprites.get("horizon/white").copied();
        for v in visuals {
            let cpu = matches!(v.kind.as_str(), "ae2:crafting_monitor" | "ae2:crafting_unit" | "ae2:crafting_accelerator" | "ae2:1k_crafting_storage" | "ae2:4k_crafting_storage" | "ae2:16k_crafting_storage" | "ae2:64k_crafting_storage" | "ae2:256k_crafting_storage");
            if !cpu && v.kind != "ae2:drive" && v.kind != "ae2:cable_bus" && v.kind != "ae2:paint" && v.kind != "ae2:charger" && v.kind != "ae2:crafting_monitor" && v.kind != "ae2:crank" {
                continue;
            }
            let p = v.pos;
            let center = glam::DVec3::new(
                f64::from(p[0]) + 0.5,
                f64::from(p[1]) + 0.5,
                f64::from(p[2]) + 0.5,
            );
            if center.distance_squared(camera) >= 64.0 * 64.0 {
                continue;
            }
            let block = world.block(p[0], p[1], p[2]);
            let Some(state) = self.pack.assets.registry.state(block) else {
                continue;
            };
            if v.kind=="ae2:crank" {
                if state.name!="ae2:crank" {continue}
                let facing=state.properties.get("facing").map(String::as_str).unwrap_or("north");
                let base=glam::Mat4::from_translation((center-camera).as_vec3())*orientation(facing,0);
                let light=vertex_light(world.light(p[0],p[1],p[2]));
                let library=self.pack.render.model_library.as_ref().ok();
                for (name,pose) in [("ae2:block/crank_base",base),("ae2:block/crank_handle",base*glam::Mat4::from_rotation_z(-(v.crank_angle as f32).to_radians()))] {
                    let Some(quads)=library.and_then(|l|l.resolve(name).ok()).and_then(|m|horizon_assets::bake::bake_model(&m).ok()) else{continue};
                    for q in quads {
                        let key=q.material.sprite.strip_prefix("minecraft:").unwrap_or(&q.material.sprite);
                        let Some(rect)=self.gui.atlas.sprites.get(key) else{continue};
                        let pts=q.positions.map(|p|pose.transform_point3(glam::Vec3::from_array(p)-glam::Vec3::splat(0.5)));
                        let normal=(pts[1]-pts[0]).cross(pts[2]-pts[0]).normalize_or_zero();
                        for i in 0..4 {out.push(Vertex{pos:pts[i].to_array(),uv:[rect.u0+q.uv[i][0]*(rect.u1-rect.u0),rect.v0+q.uv[i][1]*(rect.v1-rect.v0)],layer:0,color:0xffffffff,light,normal:[(normal.x*127.0) as i8,(normal.y*127.0) as i8,(normal.z*127.0) as i8,-127]});}
                    }
                }
                continue;
            }
            if cpu {
                if state.name!=v.kind || state.properties.get("formed").map(String::as_str)!=Some("true") {continue;}
                let base=(center-camera).as_vec3();
                let facing=state.properties.get("facing").map(String::as_str).unwrap_or("north");
                let powered=state.properties.get("powered").map(String::as_str)==Some("true");
                let (palette,text_color)=crafting_palette(v.painted_color.as_deref().unwrap_or("fluix"));
                let contains=|s:&str|v.connections.iter().any(|x|x==s);
                // The source CPU model draws only the visible exterior faces,
                // with 3/16 corner caps and stripes where that edge has no
                // adjacent member of the same formed cluster.
                for (face,normal,u,w,left,right,bottom,top) in [
                    ("north",[0.,0.,-1.],[-1.,0.,0.],[0.,1.,0.],"east","west","down","up"),
                    ("south",[0.,0.,1.],[1.,0.,0.],[0.,1.,0.],"west","east","down","up"),
                    ("west",[-1.,0.,0.],[0.,0.,1.],[0.,1.,0.],"north","south","down","up"),
                    ("east",[1.,0.,0.],[0.,0.,-1.],[0.,1.,0.],"south","north","down","up"),
                    ("up",[0.,1.,0.],[1.,0.,0.],[0.,0.,-1.],"west","east","south","north"),
                    ("down",[0.,-1.,0.],[1.,0.,0.],[0.,0.,1.],"west","east","north","south")
                ] {
                    if contains(face) {continue;}
                    let n=glam::Vec3::from_array(normal);let u=glam::Vec3::from_array(u);let w=glam::Vec3::from_array(w);
                    // An opaque CPU's own light is zero. Vanilla samples the
                    // neighboring exterior face, including its sky light.
                    let light=vertex_light(world.light(p[0]+normal[0] as i32,p[1]+normal[1] as i32,p[2]+normal[2] as i32));
                    let mut quad=|sprite:&str,x0:f32,y0:f32,x1:f32,y1:f32,color:u32,emissive:bool| {
                        let Some(rect)=self.gui.atlas.sprites.get(sprite) else{return};
                        let positions=[(x0,y0),(x1,y0),(x1,y1),(x0,y1)];
                        // CubeBuilder selects the corresponding sprite region.
                        let us=rect.u1-rect.u0; let vs=rect.v1-rect.v0;
                        for (x,y) in positions {
                            let ty=if face=="down" {y} else {1.-y};
                            out.push(Vertex{pos:(base+n*0.5+u*(x-0.5)+w*(y-0.5)).to_array(),uv:[rect.u0+x*us,rect.v0+ty*vs],color,light:if emissive {0xF0F0}else{light},layer:0,normal:[(normal[0]*127.) as i8,(normal[1]*127.) as i8,(normal[2]*127.) as i8,-127]});
                        }
                    };
                    let edge=3./16.;
                    let (x0,x1)=cpu_inner_bounds(contains(left),contains(right));
                    let (y0,y1)=cpu_inner_bounds(contains(bottom),contains(top));
                    if v.kind=="ae2:crafting_monitor" && face==facing {
                        quad("ae2:block/crafting/monitor_base",x0,y0,x1,y1,0xffffffff,false);
                        for (name,color) in [("dark",palette[0]),("medium",palette[1]),("bright",palette[2])] {
                            quad(&format!("ae2:block/crafting/monitor_light_{name}"),x0,y0,x1,y1,color,powered);
                        }
                    } else if v.kind=="ae2:crafting_monitor" || v.kind=="ae2:crafting_unit" {
                        quad("ae2:block/crafting/unit_base",x0,y0,x1,y1,0xffffffff,false);
                    } else {
                        quad("ae2:block/crafting/light_base",x0,y0,x1,y1,0xffffffff,false);
                        let overlay=match v.kind.as_str() {
                            "ae2:crafting_accelerator"=>"accelerator_light",
                            "ae2:1k_crafting_storage"=>"1k_storage_light",
                            "ae2:4k_crafting_storage"=>"4k_storage_light",
                            "ae2:16k_crafting_storage"=>"16k_storage_light",
                            "ae2:64k_crafting_storage"=>"64k_storage_light",
                            "ae2:256k_crafting_storage"=>"256k_storage_light",
                            _=>unreachable!(),
                        };
                        quad(&format!("ae2:block/crafting/{overlay}"),x0,y0,x1,y1,0xffffffff,powered);
                    }
                    let (sx0,sx1)=(if contains(left){0.}else{edge},if contains(right){1.}else{1.-edge});
                    let (sy0,sy1)=(if contains(bottom){0.}else{edge},if contains(top){1.}else{1.-edge});
                    for (dir,x,y) in [(left,0.,false),(right,1.-edge,false),(bottom,0.,true),(top,1.-edge,true)] {
                        if contains(dir) {continue;}
                        if y {quad("ae2:block/crafting/ring_side_hor",sx0,x,sx1,x+edge,0xffffffff,false);}
                        else {quad("ae2:block/crafting/ring_side_ver",x,sy0,x+edge,sy1,0xffffffff,false);}
                    }
                    for (horizontal,x) in [(left,0.),(right,1.-edge)] {for (vertical,y) in [(bottom,0.),(top,1.-edge)] {
                        if !contains(horizontal) && !contains(vertical) {quad("ae2:block/crafting/ring_corner",x,y,x+edge,y+edge,0xffffffff,false);}
                    }}
                }
                if let Some(job)=&v.crafting_job {
                    let spin=state.properties.get("spin").and_then(|s|s.parse().ok()).unwrap_or(0);
                    let pose=glam::Mat4::from_translation(base)*orientation(facing,spin);
                    if !job.fluid {
                        let key=ae2_cell_core::ItemKey{item_id:job.item.clone(),components:job.components.clone()};
                        if let Ok(stack)=horizon_client::ae2_cell_codec::prototype(&key) {
                            let item=self.resolve_item(&stack,DisplayContext::Gui,None,0);
                            let transform=pose*glam::Mat4::from_translation(glam::Vec3::new(0.,0.02,-0.515))*glam::Mat4::from_scale(glam::Vec3::splat(0.3));
                            self.submit_item_world(&item,transform,0xF0F0,out);
                        }
                    } else {
                        let (ns,name)=job.item.split_once(':').unwrap_or(("minecraft",job.item.as_str()));
                        let sprite=if ns=="minecraft" {format!("block/{name}_still")} else {format!("{ns}:block/{name}_still")};
                        if let Some(rect)=self.gui.atlas.sprites.get(&sprite) {
                            let positions=[[-0.15,-0.13,-0.515],[0.15,-0.13,-0.515],[0.15,0.17,-0.515],[-0.15,0.17,-0.515]];
                            let uv=[[rect.u0,rect.v1],[rect.u1,rect.v1],[rect.u1,rect.v0],[rect.u0,rect.v0]];
                            let color=if job.item=="minecraft:water" {0xffff763f} else {0xffffffff};
                            for i in 0..4 {out.push(Vertex{pos:pose.transform_point3(glam::Vec3::from_array(positions[i])).to_array(),uv:uv[i],color,light:0xF0F0,layer:0,normal:[0;4]});}
                        }
                    }
                    let text=monitor_amount(job.amount,false,job.fluid);let atlas=&self.gui.atlas;
                    let mut batch=horizon_client::hud::HudBatch::new([1,1]);
                    batch.text(atlas,&text,-atlas.font.width(&text)/2.,0.,text_color,false);
                    let matrix=pose*glam::Mat4::from_translation(glam::Vec3::new(0.,-0.16,-0.525))*glam::Mat4::from_scale(glam::Vec3::new(1./124.,-1./124.,1./124.));
                    for quad in batch.vertices.as_chunks::<6>().0 {for i in [0,1,2,5] {let v=quad[i];let p=matrix.transform_point3(glam::Vec3::new(v.pos[0],v.pos[1],v.depth));out.push(Vertex{pos:p.to_array(),uv:v.uv,color:v.color,light:0xF0F0,layer:0,normal:[0;4]});}}
                }
                continue;
            }
            if v.kind == "ae2:charger" {
                if state.name!="ae2:charger" {continue}
                let Some(Some(slot))=v.slots.first() else {continue};
                let facing=state.properties.get("facing").map(String::as_str).unwrap_or("north");
                let spin=state.properties.get("spin").and_then(|s|s.parse().ok()).unwrap_or(0);
                let y_offset=(self.started.elapsed().as_secs_f32().sin())*0.02;
                let pose=glam::Mat4::from_translation((center-camera).as_vec3())
                    *orientation(facing,spin)*glam::Mat4::from_translation(glam::Vec3::new(0.,-0.15+y_offset,0.));
                let item=ItemStack::new(&slot.id,1);
                let model=self.resolve_item(&item,DisplayContext::Ground,None,0);
                let light=vertex_light(world.light(p[0],p[1],p[2]));
                self.submit_item_world(&model,pose,light,out);
                if v.charger_working && world.game_time.rem_euclid(2)==0 {
                    if let Some(rect)=white {
                        let uv=[(rect.u0+rect.u1)*0.5,(rect.v0+rect.v1)*0.5];
                        let rotation=orientation(facing,spin);
                        let base=glam::Mat4::from_translation((center-camera).as_vec3())*rotation;
                        let mut seed=(p[0] as u64).wrapping_mul(0x9e3779b9)^(p[2] as u64).rotate_left(19)^(world.game_time as u64);
                        let mut random=|| {seed^=seed<<13;seed^=seed>>7;seed^=seed<<17;(seed&65535) as f32/65535.};
                        for _ in 0..3 {
                            let x=(random()-0.5)*0.3;let z=(random()-0.5)*0.3;
                            let reverse=random()>0.5;
                            let mut prev=glam::Vec3::new(x,if reverse {0.3}else{-0.3},z);
                            for step in 1..=5 {
                                let y=if reverse {0.3-0.6*step as f32/5.} else {-0.3+0.6*step as f32/5.};
                                let jitter=if step==5 {0.} else {0.06};
                                let next=glam::Vec3::new(x+(random()-0.5)*jitter,y,z+(random()-0.5)*jitter);
                                let a=base.transform_point3(prev);let b=base.transform_point3(next);
                                let mut n=(b-a).cross(glam::Vec3::Y).normalize_or_zero();if n.length_squared()<0.01 {n=(b-a).cross(glam::Vec3::X).normalize_or_zero();}
                                for point in [a-n*0.02,a+n*0.02,b+n*0.02,b-n*0.02] {out.push(Vertex{pos:point.to_array(),uv,layer:0,color:0xffffb776,light:0xF0F0,normal:[0,0,0,0]});}
                                prev=next;
                            }
                        }
                    }
                }
                continue;
            }
            if v.kind == "ae2:paint" {
                if state.name!="ae2:paint" {continue}
                let data=&v.paint_dots;
                let Some(&count)=data.first() else {continue};
                if count>21 || data.len()<1+usize::from(count)*2 {continue}
                let colors:[(u32,u32);16]=[
                    (0xe0e0e0,0xf9f9f9),(0xa09fa0,0xc4c4c4),(0x6c6b6c,0x949294),(0x272727,0x3b3b3b),
                    (0x70e259,0xb3f86d),(0xffe359,0xf4ff80),(0xeca23c,0xf2ba49),(0x7e5c16,0x8e6e1a),
                    (0xd73e42,0xf07665),(0xff99bb,0xfbcad5),(0xd5719c,0xe69ebf),(0x915dcd,0xb06fdd),
                    (0x3894ff,0x40c1ff),(0x70d2ff,0x80f7ff),(0x2fccb7,0x65e8c9),(0x17b86d,0x32d850)];
                let base=(glam::DVec3::new(p[0] as f64,p[1] as f64,p[2] as f64)-camera).as_vec3();
                let packed_light=vertex_light(world.light(p[0],p[1],p[2]));
                for (i,d) in data[1..1+usize::from(count)*2].chunks_exact(2).enumerate() {
                    let side=d[1]&7;if side>5 {continue} let color_index=(d[1]>>3)&15;let lumen=d[1]&128!=0;
                    let x=(f32::from(d[0]&15)/15.).clamp(0.1,0.9);let y=(f32::from(d[0]>>4)/15.).clamp(0.1,0.9);
                    let offset=0.001*(i as f32+1.);let [a,b,c,e]=[x-0.1,x+0.1,y-0.1,y+0.1];
                    let points=match side {
                        0=>[[a,offset,c],[b,offset,c],[b,offset,e],[a,offset,e]],
                        1=>[[a,1.-offset,c],[a,1.-offset,e],[b,1.-offset,e],[b,1.-offset,c]],
                        2=>[[a,c,offset],[a,e,offset],[b,e,offset],[b,c,offset]],
                        3=>[[a,c,1.-offset],[b,c,1.-offset],[b,e,1.-offset],[a,e,1.-offset]],
                        4=>[[offset,a,c],[offset,b,c],[offset,b,e],[offset,a,e]],
                        _=>[[1.-offset,a,c],[1.-offset,a,e],[1.-offset,b,e],[1.-offset,b,c]],
                    };
                    let val=(side as i32)|((color_index as i32)<<3)|if lumen {128}else{0};
                    let seed=((d[0] as i8 as i32)+val).unsigned_abs() as usize;
                    let Some(rect)=self.gui.atlas.sprites.get(&format!("ae2:block/paint{}",seed%3+1)) else {continue};
                    let rgb=if lumen {colors[color_index as usize].1}else{colors[color_index as usize].0};
                    let color=0xff000000|((rgb&255)<<16)|(rgb&0xff00)|((rgb>>16)&255);
                    let uv=[[rect.u0,rect.v1],[rect.u1,rect.v1],[rect.u1,rect.v0],[rect.u0,rect.v0]];
                    for j in 0..4 {out.push(Vertex{pos:(base+glam::Vec3::from_array(points[j])).to_array(),uv:uv[j],layer:0,color,light:if lumen {0xF0F0}else{packed_light},normal:[0,0,0,0]});}
                }
                continue;
            }
            if v.kind == "ae2:cable_bus" {
                if state.name != "ae2:cable_bus" {
                    continue;
                }
                let base = glam::Mat4::from_translation((center - camera).as_vec3());
                let light = vertex_light(world.light(p[0], p[1], p[2]));
                let library = self.pack.render.model_library.as_ref().ok();
                let cable_color=v.cable.as_deref().and_then(|id|id.strip_prefix("ae2:")).and_then(|id|["_covered_dense_cable","_smart_dense_cable","_covered_cable","_smart_cable","_glass_cable"].iter().find_map(|suffix|id.strip_suffix(suffix))).unwrap_or("fluix");
                let tint = |index:i32| -> u32 {
                    let (dark,medium,bright)=match cable_color {
                        "white"=>(0xb4b4b4,0xe0e0e0,0xf9f9f9),"light_gray"=>(0x7e7e7e,0xa09fa0,0xc4c4c4),"gray"=>(0x4f4f4f,0x6c6b6c,0x949294),"black"=>(0x131313,0x272727,0x3b3b3b),
                        "lime"=>(0x4ec04e,0x70e259,0xb3f86d),"yellow"=>(0xffcf40,0xffe359,0xf4ff80),"orange"=>(0xd9782f,0xeca23c,0xf2ba49),"brown"=>(0x6e4a12,0x7e5c16,0x8e6e1a),
                        "red"=>(0xaa212b,0xd73e42,0xf07665),"pink"=>(0xd86eaa,0xff99bb,0xfbcad5),"magenta"=>(0xc15189,0xd5719c,0xe69ebf),"purple"=>(0x6e5cb8,0x915dcd,0xb06fdd),
                        "blue"=>(0x337ff0,0x3894ff,0x40c1ff),"light_blue"=>(0x69b9ff,0x70d2ff,0x80f7ff),"cyan"=>(0x22b0ae,0x2fccb7,0x65e8c9),"green"=>(0x079b6b,0x17b86d,0x32d850),_ =>(0x5a479e,0x915dcd,0xe2a3e3),
                    };
                    let rgb=match index {1=>dark,2=>medium,3=>bright,_=>0xffffff};
                    0xff000000 | ((rgb & 0xff)<<16) | (rgb & 0xff00) | ((rgb>>16)&0xff)
                };
                let mut model = |name: &str, pose: glam::Mat4, color: Option<&str>| {
                    let Some(quads) = library.and_then(|l| l.resolve(name).ok())
                        .and_then(|m| horizon_assets::bake::bake_model(&m).ok()) else { return };
                    for q in quads {
                        let mut sprite = q.material.sprite.clone();
                        if let Some(color) = color {
                            sprite = sprite.replace("_color_", color);
                            if sprite.contains("part/cable/glass/transparent") && color != "transparent" {
                                sprite = sprite.replace("part/cable/glass/transparent", &format!("part/cable/glass/{color}"));
                            }
                        }
                        let key = sprite.strip_prefix("minecraft:").unwrap_or(&sprite);
                        let Some(rect) = self.gui.atlas.sprites.get(key) else { continue };
                        let pts = q.positions.map(|p| pose.transform_point3(glam::Vec3::from_array(p)-glam::Vec3::splat(0.5)));
                        let normal = (pts[1]-pts[0]).cross(pts[2]-pts[0]).normalize_or_zero();
                        for n in 0..4 { out.push(Vertex {pos:pts[n].to_array(),
                            uv:[rect.u0+q.uv[n][0]*(rect.u1-rect.u0),rect.v0+q.uv[n][1]*(rect.v1-rect.v0)],
                            layer:0,color:tint(q.tint_index),light:if name.ends_with("_on") || name.ends_with("_has_channel") {0xF0F0} else {light},
                            normal:[(normal.x*127.0) as i8,(normal.y*127.0) as i8,(normal.z*127.0) as i8,-127]}); }
                    }
                };
                let rotate = |face: &str| match face {
                    "south"=>glam::Quat::from_rotation_y(PI),
                    "east"=>glam::Quat::from_rotation_y(-PI/2.0),
                    "west"=>glam::Quat::from_rotation_y(PI/2.0),
                    "up"=>glam::Quat::from_rotation_x(PI/2.0),
                    "down"=>glam::Quat::from_rotation_x(-PI/2.0),
                    _=>glam::Quat::IDENTITY,
                };
                if let Some(id) = &v.cable {
                    let id = id.strip_prefix("ae2:").unwrap_or(id);
                    let (color, kind) = [
                        ("_covered_dense_cable","dense_covered"), ("_smart_dense_cable","dense_smart"),
                        ("_covered_cable","covered"), ("_smart_cable","smart"), ("_glass_cable","glass"),
                    ].iter().find_map(|(suffix,kind)| id.strip_suffix(suffix).map(|color|(color,*kind))).unwrap_or(("fluix","glass"));
                    let color = if color=="fluix" { "transparent" } else { color };
                    model(&format!("ae2:part/cable/{kind}/center"), base, Some(color));
                    for face in &v.connections {
                        model(&format!("ae2:part/cable/{kind}/connection"),base*glam::Mat4::from_quat(rotate(face)),Some(color));
                    }
                }
                for part in &v.parts {
                    let pose = base * glam::Mat4::from_quat(rotate(&part.face));
                    let name = part.id.strip_prefix("ae2:").unwrap_or(&part.id);
                    let terminal = match name {"terminal"=>Some("terminal"),"crafting_terminal"=>Some("crafting_terminal"),"pattern_encoding_terminal"=>Some("pattern_encoding_terminal"),"pattern_access_terminal"=>Some("pattern_access_terminal"),_=>None};
                    if let Some(terminal) = terminal {
                        model("ae2:part/display_base",pose,None);
                        model(&format!("ae2:part/{terminal}_{}",if part.online {"on"} else {"off"}),pose,None);
                        model(&format!("ae2:part/display_status_{}",if part.online {"has_channel"} else {"off"}),pose,None);
                    } else if matches!(name,"storage_monitor"|"conversion_monitor") {
                        model("ae2:part/display_base",pose,None);
                        let lock=if part.monitor_locked {"_locked"} else {""};
                        model(&format!("ae2:part/{name}{lock}_{}",if part.powered {"on"} else {"off"}),pose,None);
                        model(&format!("ae2:part/display_status_{}",if part.online {"has_channel"} else if part.powered {"on"} else {"off"}),pose,None);
                    } else if matches!(name,"monitor"|"semi_dark_monitor"|"dark_monitor") {
                        let tone=match name {"monitor"=>"bright","semi_dark_monitor"=>"medium",_=>"dark"};
                        model("ae2:part/monitor_base",pose,None);
                        model(&format!("ae2:part/monitor_{tone}_{}",if part.online {"on"} else {"off"}),pose,None);
                    } else if name=="cable_energy_acceptor" {
                        model("ae2:part/energy_acceptor",pose,None);
                    } else if let Some(kind)=name.strip_suffix("_p2p_tunnel") {
                        let front=match kind {"item"=>"items","fluid"=>"fluids","me"=>"me","fe"=>"fe","redstone"=>"redstone","light"=>"light",_=>continue};
                        model(&format!("ae2:part/p2p/p2p_tunnel_{front}"),pose,None);
                        model(&format!("ae2:part/p2p/p2p_tunnel_status_{}",if part.online {"has_channel"} else {"off"}),pose,None);
                    } else {
                        let name = match name {"import_bus"=>"import_bus_base","export_bus"=>"export_bus_base","storage_bus"=>"storage_bus_base","cable_interface"=>"interface_base","cable_pattern_provider"=>"pattern_provider_base","quartz_fiber"=>"quartz_fiber","cable_anchor"=>if v.facades.iter().any(|f|f.face==part.face){"cable_anchor_short"}else{"cable_anchor"},"toggle_bus"=>"toggle_bus_base","inverted_toggle_bus"=>"inverted_toggle_bus_base",_=>continue};
                        model(&format!("ae2:part/{name}"),pose,None);
                    }
                }
                drop(model);
                // Resolve the actual facade block state so directional and
                // other blockstate variants use the same model as the world.
                for facade in &v.facades {
                    // The block-state name and item registry ID need not be
                    // equal. Check the public material against the state
                    // before rendering a server-provided projection.
                    if !facade.material.is_empty() {
                        let Some(material)=self.res.items().get(&facade.material) else {continue};
                        let Some(block)=material.block
                            .filter(|id| !self.res.facts().is_air(*id))
                            .or_else(|| self.pack.assets.registry.default_state("minecraft:glass"))
                        else {continue};
                        let Some(expected)=self.pack.assets.registry.state(block) else {continue};
                        if expected.name!=facade.block {continue;}
                    }
                    let state = horizon_world::chunk::BlockState {
                        name: facade.block.clone(), properties: facade.properties.clone(),
                    };
                    let Ok(state) = self.pack.assets.registry.resolve(&state) else {continue};
                    let pose=base*glam::Mat4::from_quat(rotate(&facade.face))
                        *glam::Mat4::from_translation(glam::Vec3::new(0.,0.,-15.0/32.0))
                        *glam::Mat4::from_scale(glam::Vec3::new(1.,1.,1./16.))
                        *glam::Mat4::from_translation(glam::Vec3::splat(-0.5));
                    let seed = horizon_core::random::position_seed(horizon_core::position::BlockPos::new(p[0],p[1],p[2]));
                    self.block_model_vertices(state, seed, pose, light, true, out);
                }
                for part in &v.parts {
                    let Some(display)=part.monitor.as_ref().filter(|_|part.online) else {continue};
                    let pose=base*glam::Mat4::from_quat(rotate(&part.face));
                    if !display.fluid {
                        let key=ae2_cell_core::ItemKey{item_id:display.item.clone(),components:display.components.clone()};
                        if let Ok(stack)=horizon_client::ae2_cell_codec::prototype(&key) {
                            let item=self.resolve_item(&stack,DisplayContext::Gui,None,0);
                            let transform=pose*glam::Mat4::from_translation(glam::Vec3::new(0.,0.05,-0.51))
                                *glam::Mat4::from_scale(glam::Vec3::splat(0.4));
                            self.submit_item_world(&item,transform,0xF0F0,out);
                        }
                    } else {
                        let (ns,name)=display.item.split_once(':').unwrap_or(("minecraft",display.item.as_str()));
                        let sprite=if ns=="minecraft" {format!("block/{name}_still")} else {format!("{ns}:block/{name}_still")};
                        if let Some(rect)=self.gui.atlas.sprites.get(&sprite) {
                            let points=[[-0.2,-0.15,-0.511],[0.2,-0.15,-0.511],[0.2,0.25,-0.511],[-0.2,0.25,-0.511]];
                            let uv=[[rect.u0,rect.v1],[rect.u1,rect.v1],[rect.u1,rect.v0],[rect.u0,rect.v0]];
                            let color=if display.item=="minecraft:water" {0xffff763f} else {0xffffffff};
                            for i in 0..4 {out.push(Vertex{pos:pose.transform_point3(glam::Vec3::from_array(points[i])).to_array(),uv:uv[i],color,light:0xF0F0,layer:0,normal:[0;4]});}
                        }
                    }
                    let text=monitor_amount(display.amount,display.craftable,display.fluid);
                    let atlas=&self.gui.atlas;
                    let mut batch=horizon_client::hud::HudBatch::new([1,1]);
                    let width=atlas.font.width(&text);
                    batch.text(atlas,&text,-width/2.,0.,0xffffffff,false);
                    let matrix=pose*glam::Mat4::from_translation(glam::Vec3::new(0.,-0.18,-0.52))
                        *glam::Mat4::from_scale(glam::Vec3::new(1./124.,-1./124.,1./124.));
                    for quad in batch.vertices.as_chunks::<6>().0 {
                        for i in [0,1,2,5] {
                            let v=quad[i];let point=matrix.transform_point3(glam::Vec3::new(v.pos[0],v.pos[1],v.depth));
                            out.push(Vertex{pos:point.to_array(),uv:v.uv,color:v.color,light:0xF0F0,layer:0,normal:[0;4]});
                        }
                    }
                }
                continue;
            }
            if state.name != "ae2:drive" {
                continue;
            }
            let facing = state
                .properties
                .get("facing")
                .map(String::as_str)
                .unwrap_or("north");
            let spin = state
                .properties
                .get("spin")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let base = glam::Mat4::from_translation((center - camera).as_vec3())
                * orientation(facing, spin);
            let light = vertex_light(world.light(p[0], p[1], p[2]));
            // DriveBakedModel translates the source block-cell model into each
            // slot. Keep the original geometry and atlas UVs, rather than using
            // the inventory item's billboard model.
            let library = self.pack.render.model_library.as_ref().ok();
            for (i, slot) in v.slots.iter().enumerate() {
                let Some(slot) = slot else { continue };
                let row = i / 2;
                let col = i % 2;
                let origin = glam::Vec3::new(
                    (9.0 - col as f32 * 8.0) / 16.0,
                    (13.0 - row as f32 * 3.0) / 16.0,
                    1.0 / 16.0,
                );
                let name = slot.id.strip_prefix("ae2:").unwrap_or("");
                let model_name = name.split_once("_storage_cell_")
                    .map(|(kind, size)| format!("ae2:block/drive/cells/{size}_{kind}_cell"))
                    .unwrap_or_else(|| "ae2:block/drive/drive_cell".to_owned());
                let quads = library.and_then(|l| l.resolve(&model_name).ok())
                    .and_then(|m| horizon_assets::bake::bake_model(&m).ok());
                if let Some(quads) = quads {
                    for q in quads {
                        let key = q.material.sprite.strip_prefix("minecraft:").unwrap_or(&q.material.sprite);
                        let Some(rect) = self.gui.atlas.sprites.get(key) else { continue };
                        let points = q.positions.map(|p| base.transform_point3(
                            glam::Vec3::from_array(p) + origin - glam::Vec3::splat(0.5)));
                        let normal = (points[1]-points[0]).cross(points[2]-points[0]).normalize_or_zero();
                        for n in 0..4 {
                            out.push(Vertex { pos: points[n].to_array(),
                                uv: [rect.u0+q.uv[n][0]*(rect.u1-rect.u0), rect.v0+q.uv[n][1]*(rect.v1-rect.v0)],
                                layer: 0, color: 0xFFFF_FFFF, light,
                                normal: [(normal.x*127.0) as i8,(normal.y*127.0) as i8,(normal.z*127.0) as i8,-127], });
                        }
                    }
                }
                // Source CellLedRenderer's front face: a 1x1 pixel cuboid at
                // x=4..5, y=0..1 relative to the slot. Unpowered is black.
                if let Some(rect) = white {
                    let color = if !v.powered { 0xFF000000 } else { match slot.state {
                        1 => 0xFF00FF00, 2 => 0xFFFFAA00,
                        3 => 0xFF00AAFF, 4 => 0xFF0000FF, _ => 0xFF000000,
                    }};
                    let x = origin.x + 4.0/16.0 - 0.5;
                    let y = origin.y - 0.5;
                    let z = origin.z - 0.50001;
                    let points = [[x+1.0/16.0,y+1.0/16.0,z],[x,y+1.0/16.0,z],
                        [x,y,z],[x+1.0/16.0,y,z]];
                    let normal = base.transform_vector3(glam::Vec3::NEG_Z);
                    for p in points {
                        out.push(Vertex { pos: base.transform_point3(glam::Vec3::from_array(p)).to_array(),
                            uv: [(rect.u0+rect.u1)*0.5,(rect.v0+rect.v1)*0.5],
                            layer: 0,color,light:0x00F000F0,
                            normal:[(normal.x*127.0) as i8,(normal.y*127.0) as i8,(normal.z*127.0) as i8,-127], });
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod cpu_model_tests {
    use super::cpu_inner_bounds;

    #[test]
    fn formed_cpu_inner_faces_overlap_unconnected_frame_and_extend_to_neighbors() {
        let edge = 2.99_f32 / 16.;
        let opposite = 13.01_f32 / 16.;
        assert_eq!(cpu_inner_bounds(false, false), (edge, opposite));
        assert_eq!(cpu_inner_bounds(true, false), (0., opposite));
        assert_eq!(cpu_inner_bounds(false, true), (edge, 1.));
        assert_eq!(cpu_inner_bounds(true, true), (0., 1.));
        assert!(edge < 3. / 16. && opposite > 13. / 16.);
    }
}
