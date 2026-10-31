//! AE2 GuideME scenes from the original guide structures. The scene is a
//! private, read-only world; it never enters the player's server/world.
use super::{Game, ae2_visual_sync::{CellVisual, Part, Visual}};
use glam::{DVec3, Mat4, Vec2, Vec3};
use horizon_client::{chunk::Chunk, hud::{HudBatch, rgba}, mesher::Vertex, world::World};
use horizon_core::position::BlockPos;
use horizon_world::{chunk::BlockState, nbt::{Tag, parse_snbt}, registry::BlockStateId};
use serde_json::Value;
use std::{cell::RefCell, collections::{BTreeMap, HashSet, VecDeque}, path::{Component, Path, PathBuf}, sync::{Arc, Weak}, time::SystemTime};

/// Reader-owned offsets, keyed by the compiler's stable scene_id.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct GuideSceneOrbit { pub yaw: f32, pub pitch: f32, pub pan: [f32;2] }
impl GuideSceneOrbit {
    pub(super) fn drag(&mut self, dx: f32, dy: f32) {
        if dx.is_finite() && dy.is_finite() {
            // GuideME's viewport applies one degree per logical pointer pixel;
            // both rotations may pass through a complete revolution.
            self.yaw = (self.yaw + dx).rem_euclid(360.0);
            self.pitch = (self.pitch + dy).rem_euclid(360.0);
        }
    }
    pub(super) fn translate(&mut self,dx:f32,dy:f32) {
        let next=[self.pan[0]+dx,self.pan[1]+dy];
        if next.iter().all(|n|n.is_finite()){self.pan=next;}
    }
}
pub(super) struct GuideSceneResult {
    pub scene_id: String,
    pub interactive: bool,
    /// Original document nodes: nested scenes remain nested and renderable.
    pub tooltip: Option<Vec<Value>>,
    pub diagnostics: Vec<String>,
    pub blocks: usize,
    pub multipart_parts: usize,
}
#[derive(Clone)]
struct StructureBlock { pos: [i32;3], state: String, nbt: Option<Tag> }
#[derive(Clone)]
struct Structure { blocks: Vec<StructureBlock>, entities: usize }
struct Cached { path: PathBuf, modified: Option<SystemTime>, len: u64, structure: Arc<Structure> }
thread_local! { static CACHE: RefCell<VecDeque<Cached>> = const { RefCell::new(VecDeque::new()) }; }

fn tag_int(tag: &Tag) -> Option<i32> { match tag { Tag::Int(n)=>Some(*n),Tag::Short(n)=>Some(i32::from(*n)),Tag::Byte(n)=>Some(i32::from(*n)),_=>None } }
fn tag_string(tag: Option<&Tag>) -> Option<String> { if let Some(Tag::String(s))=tag {s.to_string().ok()} else {None} }
fn tag_list(tag: Option<&Tag>) -> Option<&[Tag]> { if let Some(Tag::List(v))=tag {Some(v)} else {None} }
fn position(tag: &Tag) -> Option<[i32;3]> {
    let v=tag_list(Some(tag))?; if v.len()!=3 {return None} Some([tag_int(&v[0])?,tag_int(&v[1])?,tag_int(&v[2])?])
}
/// Some shipped GuideME structures use a final comma. Do not change the
/// original asset or the general command/NBT parser; remove only commas
/// followed by a closing delimiter, outside quoted and escaped strings.
fn guide_trailing_commas(raw:&str)->String {
    let bytes=raw.as_bytes();let mut output=Vec::with_capacity(bytes.len());let mut quoted=None;let mut escaped=false;
    for (i,&byte) in bytes.iter().enumerate() {
        if let Some(quote)=quoted {
            output.push(byte);
            if escaped {escaped=false;} else if byte==b'\\' {escaped=true;} else if byte==quote {quoted=None;}
        } else if matches!(byte,b'"'|b'\'') {
            quoted=Some(byte);output.push(byte);
        } else if byte==b',' && bytes[i+1..].iter().copied().find(|c|!c.is_ascii_whitespace()).is_some_and(|c|matches!(c,b']'|b'}')) {
            continue
        } else {output.push(byte)}
    }
    String::from_utf8(output).expect("removing ASCII commas preserves UTF-8")
}
fn parse_structure(raw:&str) -> Result<Structure,String> {
    let fallback;
    let (tag,end,input)=match parse_snbt(raw) {
        Ok((tag,end))=>(tag,end,raw),
        Err(_)=>{fallback=guide_trailing_commas(raw);let (tag,end)=parse_snbt(&fallback).map_err(|e|format!("invalid guide structure: {e}"))?;(tag,end,fallback.as_str())}
    };
    if !input[end..].trim().is_empty() {return Err("guide structure has trailing content".into())}
    let entries=tag_list(tag.get("data")).ok_or("guide structure has no data list")?;
    if entries.len()>65_536 {return Err("guide structure exceeds block limit".into())}
    let mut seen=HashSet::new();let mut blocks=Vec::with_capacity(entries.len());
    for entry in entries {
        let pos=position(entry.get("pos").ok_or("guide block has no position")?).ok_or("invalid guide block position")?;
        if !(-64..=319).contains(&pos[1]) || pos[0].unsigned_abs()>30_000_000 || pos[2].unsigned_abs()>30_000_000 {return Err("guide block outside world bounds".into())}
        if !seen.insert(pos) {return Err("duplicate guide block position".into())}
        let state=tag_string(entry.get("state")).ok_or("guide block has no state")?;
        blocks.push(StructureBlock{pos,state,nbt:entry.get("nbt").cloned()});
    }
    Ok(Structure{blocks,entities:tag_list(tag.get("entities")).map_or(0,|v|v.len())})
}
fn structure(root:&Path, asset:&str) -> Result<Arc<Structure>,String> {
    let path=Path::new(asset);
    if path.as_os_str().is_empty() || path.components().any(|c|!matches!(c,Component::Normal(_))) {return Err("invalid guide asset path".into())}
    let root=root.canonicalize().map_err(|e|format!("guide asset root: {e}"))?;
    let path=root.join(path).canonicalize().map_err(|e|format!("guide structure {asset}: {e}"))?;
    if !path.starts_with(&root) {return Err("guide asset escapes its root".into())}
    let meta=std::fs::metadata(&path).map_err(|e|format!("guide structure {}: {e}",asset))?;
    if !meta.is_file() || meta.len()>4*1024*1024 {return Err("guide structure is not a bounded file".into())}
    let modified=meta.modified().ok();
    if let Some(found)=CACHE.with(|cache| {
        let mut cache=cache.borrow_mut();
        let at=cache.iter().position(|e|e.path==path && e.modified==modified && e.len==meta.len())?;
        let entry=cache.remove(at)?;let value=entry.structure.clone();cache.push_front(entry);Some(value)
    }) {return Ok(found)}
    let text=std::fs::read_to_string(&path).map_err(|e|format!("guide structure {}: {e}",asset))?;
    let result=Arc::new(parse_structure(&text)?);
    CACHE.with(|cache| {let mut cache=cache.borrow_mut();cache.push_front(Cached{path,modified,len:meta.len(),structure:result.clone()});while cache.len()>32 {cache.pop_back();}});
    Ok(result)
}
fn descriptor(input:&str) -> Result<BlockState,String> {
    let (name,props)=if let Some((name,rest))=input.split_once('{') {
        (name,rest.strip_suffix('}').ok_or("invalid guide block state")?)
    } else {(input,"")};
    if name.is_empty() {return Err("empty guide block state".into())}
    let mut properties=BTreeMap::new();
    if !props.is_empty() {for prop in props.split(',') {
        let (key,value)=prop.split_once(':').ok_or("invalid guide block property")?;
        let key=if key=="light_level" {"light"} else {key};
        properties.insert(key.to_owned(),value.to_owned());
    }}
    Ok(BlockState{name:name.to_owned(),properties})
}
fn attr<'a>(node:&'a Value, key:&str)->Option<&'a Value> {node.get("attrs")?.get(key)}
fn block_image_state(node:&Value)->Result<BlockState,String> {
    let name=node.get("resource_id").and_then(Value::as_str).ok_or("BlockImage has no block ID")?;
    let mut properties=BTreeMap::new();
    if let Some(attrs)=node.get("attrs").and_then(Value::as_object) {for (key,value) in attrs {
        if let Some(key)=key.strip_prefix("p:") {
            let value=match value {Value::String(v)=>v.clone(),Value::Bool(v)=>v.to_string(),Value::Number(v)=>v.to_string(),_=>return Err(format!("invalid block property {key}"))};
            properties.insert(key.to_owned(),value);
        }
    }}
    Ok(BlockState{name:name.to_owned(),properties})
}
fn number(node:&Value,key:&str,default:f32)->f32 {
    let n=attr(node,key).and_then(|x|x.as_f64().map(|v|v as f32).or_else(||x.as_str().and_then(|s|s.parse().ok()))).unwrap_or(default);
    if n.is_finite() {n} else {default}
}
fn triple(node:&Value,key:&str)->Option<Vec3> {
    let value=attr(node,key)?.as_str()?;
    let values=value.split_whitespace().map(str::parse::<f32>).collect::<Result<Vec<_>,_>>().ok()?;
    if values.len()!=3 || values.iter().any(|x|!x.is_finite()) {return None}
    Some(Vec3::new(values[0],values[1],values[2]))
}
fn rgba_color(node:&Value, default:u32)->u32 {
    let Some(s)=attr(node,"color").and_then(Value::as_str).and_then(|s|s.strip_prefix('#')) else{return default};
    if s.len()!=6 {return default} let Ok(rgb)=u32::from_str_radix(s,16) else{return default};
    rgba((rgb>>16) as u8,(rgb>>8) as u8,rgb as u8,255)
}
fn valid_rect(r:[f32;4])->bool {r.iter().all(|v|v.is_finite()) && r[2]>r[0] && r[3]>r[1]}
fn intersect(a:[f32;4],b:[f32;4])->Option<[f32;4]> {
    let r=[a[0].max(b[0]),a[1].max(b[1]),a[2].min(b[2]),a[3].min(b[3])]; valid_rect(r).then_some(r)
}
fn contains(r:[f32;4],p:Vec2)->bool {p.x>=r[0] && p.x<r[2] && p.y>=r[1] && p.y<r[3]}
fn byte(tag:Option<&Tag>)->i32 {tag.and_then(tag_int).unwrap_or(0)}
fn visual(block:&StructureBlock, cpu_positions:&HashSet<[i32;3]>) -> Option<Visual> {
    let state=descriptor(&block.state).ok()?;let tag=block.nbt.as_ref()?;
    let mut v=Visual{pos:block.pos,kind:state.name.clone(),slots:Vec::new(),parts:Vec::new(),facades:Vec::new(),cable:None,powered:false,
        connections:Vec::new(),channels:0,paint_dots:Vec::new(),charger_working:false,crank_angle:0,crafting_job:None,painted_color:None,anchor_chunks:Vec::new(),anchor_pending:None};
    let raw_visual=tag.get("visual");v.powered=byte(raw_visual.and_then(|t|t.get("online")))!=0;
    if state.name=="ae2:cable_bus" {
        if let Some(cable)=tag.get("cable") {
            v.cable=tag_string(cable.get("id"));
            v.connections=tag_list(cable.get("visual").and_then(|t|t.get("connections"))).unwrap_or(&[]).iter().filter_map(|t|tag_string(Some(t))).collect();
        }
        for face in ["down","up","north","south","west","east"] {
            let Some(part)=tag.get(face) else {continue};let Some(id)=tag_string(part.get("id")) else{continue};
            let visual=part.get("visual");let powered=byte(visual.and_then(|t|t.get("powered")))!=0;
            v.parts.push(Part{id,face:face.to_owned(),online:powered && byte(visual.and_then(|t|t.get("missingChannel")))==0,
                powered,plane_edges:[false;4],monitor:None,monitor_locked:false});
        }
        return Some(v)
    }
    if state.name=="ae2:drive" {
        for i in 0..10 {let cell=raw_visual.and_then(|t|t.get(&format!("cell{i}")));
            v.slots.push(cell.and_then(|c|tag_string(c.get("id")).map(|id|CellVisual{id,state:match tag_string(c.get("state")).as_deref(){Some("empty")=>1,Some("not_empty")=>2,Some("types_full")=>3,Some("full")=>4,_=>0}})));
        }
        return Some(v)
    }
    if cpu_positions.contains(&block.pos) {
        for (name,delta) in [("down",[0,-1,0]),("up",[0,1,0]),("north",[0,0,-1]),("south",[0,0,1]),("west",[-1,0,0]),("east",[1,0,0])] {
            if cpu_positions.contains(&[block.pos[0]+delta[0],block.pos[1]+delta[1],block.pos[2]+delta[2]]) {v.connections.push(name.into())}
        }
        return Some(v)
    }
    if matches!(state.name.as_str(),"ae2:charger"|"ae2:crank"|"ae2:paint") {return Some(v)}
    None
}
#[derive(Clone)]
struct SceneQuad { pos:[Vec3;4], uv:[[f32;2];4], color:u32, normal:Vec3, translucent:bool }
fn shaded(color:u32,normal:Vec3)->u32 {
    let n=normal.normalize_or_zero();let amount=(0.65 + 0.20*n.y.max(0.0) + 0.10*n.x.abs() + 0.05*n.z.abs()).clamp(0.0,1.0);
    let [r,g,b,a]=color.to_le_bytes();rgba((r as f32*amount) as u8,(g as f32*amount) as u8,(b as f32*amount) as u8,a)
}
fn source_quads(game:&Game<'_>,blocks:&[(StructureBlock,BlockStateId)], world:&World, diagnostics:&mut Vec<String>)->Vec<SceneQuad> {
    let mut quads=Vec::new();let mut models=Vec::new();
    for (block,state) in blocks {
        if game.pack.render.model_catalog.models_at(*state,BlockPos::new(block.pos[0],block.pos[1],block.pos[2]),&mut models).is_err() {continue}
        for model in &models {for face in &model.faces {
            let q=&face.quad;let key=q.material.sprite.strip_prefix("minecraft:").unwrap_or(&q.material.sprite);
            let Some(rect)=game.gui.atlas.sprites.get(key) else {continue};
            let origin=Vec3::new(block.pos[0] as f32,block.pos[1] as f32,block.pos[2] as f32);
            let points=q.positions.map(|p|origin+Vec3::from_array(p));
            let normal=(points[1]-points[0]).cross(points[2]-points[0]);
            quads.push(SceneQuad{pos:points,uv:q.uv.map(|uv|[rect.u0+uv[0]*(rect.u1-rect.u0),rect.v0+uv[1]*(rect.v1-rect.v0)]),color:0xffffffff,normal,translucent:face.translucent});
        }}
    }
    let cpu_positions:HashSet<_>=blocks.iter().filter(|(b,_)|descriptor(&b.state).ok().is_some_and(|s| matches!(s.name.as_str(),"ae2:crafting_unit"|"ae2:crafting_accelerator"|"ae2:crafting_monitor"|"ae2:1k_crafting_storage"|"ae2:4k_crafting_storage"|"ae2:16k_crafting_storage"|"ae2:64k_crafting_storage"|"ae2:256k_crafting_storage") && s.properties.get("formed").is_some_and(|v|v=="true"))).map(|(b,_)|b.pos).collect();
    let visuals:Vec<_>=blocks.iter().filter_map(|(b,_)|visual(b,&cpu_positions)).collect();
    let refs:Vec<_>=visuals.iter().collect();let mut dynamic=Vec::<Vertex>::new();
    game.ae2_visual_geometry_from(DVec3::ZERO,&refs,world,&mut dynamic);
    // Reuse the native block-entity models for chests, banners and other
    // structures, with a fresh immutable scene world and closed animations.
    let atlas=&game.gui.atlas;
    let scene=horizon_client::block_entity_render::Scene{glyphs:0,camera:DVec3::ZERO,orientation:glam::Quat::IDENTITY,
        anim:&|_|Default::default(),texts:None,partial:0.0,game_time:0,max_y:319,world,sprites:&atlas.sprites,nether:false,
        lid:&|_|0.0,sign:&|_|None,banner:&|_|Vec::new(),skull:&|_|None,glyph:&|c|atlas.glyph_cell(c)};
    let entity_blocks:Vec<_>=blocks.iter().map(|(b,s)|(b.pos,s.0)).collect();
    game.res.block_entity_render().render(&scene,entity_blocks.iter(),&mut dynamic);
    if dynamic.len()%4!=0 {diagnostics.push("block-entity mesh has an incomplete quad".into())}
    for q in dynamic.chunks_exact(4) {
        let normal=Vec3::new(q[0].normal[0] as f32,q[0].normal[1] as f32,q[0].normal[2] as f32);
        quads.push(SceneQuad{pos:std::array::from_fn(|i|Vec3::from_array(q[i].pos)),uv:std::array::from_fn(|i|q[i].uv),color:q[0].color,normal,translucent:false});
    }
    quads
}
struct Prepared {
    sources:Vec<Arc<Structure>>,
    render:Weak<horizon_client::assets::RenderAssets>,
    atlas:usize,
    positions:Vec<[i32;3]>,
    multipart_parts:usize,
    diagnostics:Vec<String>,
    quads:Vec<SceneQuad>,
}
thread_local! { static PREPARED: RefCell<VecDeque<Arc<Prepared>>> = const { RefCell::new(VecDeque::new()) }; }
thread_local! { static SINGLE_BLOCKS: RefCell<VecDeque<(String,Arc<Structure>)>> = const { RefCell::new(VecDeque::new()) }; }
fn single_block(state:String) -> Arc<Structure> {
    SINGLE_BLOCKS.with(|entries|{
        let mut entries=entries.borrow_mut();
        if let Some(i)=entries.iter().position(|(key,_)|key==&state) {
            let entry=entries.remove(i).expect("cached entry");let value=entry.1.clone();entries.push_front(entry);return value;
        }
        let nbt=state.starts_with("ae2:").then(||Tag::compound([("visual",Tag::compound([] as [(&str,Tag);0]))]));
        let value=Arc::new(Structure{blocks:vec![StructureBlock{pos:[0,0,0],state:state.clone(),nbt}],entities:0});
        entries.push_front((state,value.clone()));while entries.len()>128 {entries.pop_back();}value
    })
}
fn prepare(game:&Game<'_>, sources:&[Arc<Structure>]) -> Result<Arc<Prepared>,String> {
    let render=Arc::downgrade(&game.pack.render);
    let atlas=std::ptr::from_ref(&game.gui.atlas) as usize;
    if let Some(found)=PREPARED.with(|entries|{
        let mut entries=entries.borrow_mut();
        let at=entries.iter().position(|entry|entry.render.ptr_eq(&render) && entry.render.strong_count()>0 && entry.atlas==atlas
            && entry.sources.len()==sources.len()
            && entry.sources.iter().zip(sources).all(|(a,b)|Arc::ptr_eq(a,b)))?;
        let entry=entries.remove(at)?;let result=entry.clone();entries.push_front(entry);Some(result)
    }) {return Ok(found)}
    let air=game.pack.assets.registry.default_state("minecraft:air").ok_or("air block unavailable")?;
    let mut world=World::new(air,0);let mut blocks=Vec::new();let mut diagnostics=Vec::new();let mut multipart_parts=0;
    for source in sources {
        if source.entities>0 {diagnostics.push(format!("{} source entities not yet rendered",source.entities));}
        for block in &source.blocks {
            let desc=descriptor(&block.state)?;
            let Some(default)=game.pack.assets.registry.default_state(&desc.name) else {diagnostics.push(format!("unknown guide block {}",desc.name));continue};
            let resolved=game.pack.assets.registry.resolve(&desc).unwrap_or_else(|_| {diagnostics.push(format!("unresolved state {}",block.state));default});
            let key=(block.pos[0]>>4,block.pos[2]>>4);
            if !world.chunks.contains_key(&key) {world.chunks.insert(key,Arc::new(Chunk::empty(key.0,key.1)));}
            world.set(block.pos[0],block.pos[1],block.pos[2],resolved).map_err(|e|e.to_string())?;
            multipart_parts+=if desc.name=="ae2:cable_bus" {block.nbt.as_ref().map_or(0,|t|["down","up","north","south","west","east"].iter().filter(|f|t.get(f).is_some()).count())} else {0};
            blocks.push((block.clone(),resolved));
        }
    }
    if blocks.is_empty(){return Err("no available guide block models".into())}
    let quads=source_quads(game,&blocks,&world,&mut diagnostics);
    let result=Arc::new(Prepared{sources:sources.to_vec(),render,atlas,positions:blocks.iter().map(|(b,_)|b.pos).collect(),multipart_parts,diagnostics,quads});
    // Keep repeated frames cheap without unbounded retention when browsing the
    // entire corpus; a single unusually large scene is used without caching.
    if result.quads.len()<=150_000 {PREPARED.with(|entries|{
        let mut entries=entries.borrow_mut();entries.push_front(result.clone());
        while entries.len()>16 || entries.iter().map(|v|v.quads.len()).sum::<usize>()>150_000 {entries.pop_back();}
    });}
    Ok(result)
}
struct Projection { rotation:Mat4, inverse:Mat4, center:Vec2, middle:Vec2, scale:f32 }
impl Projection {
    fn project(&self,p:Vec3)->Vec3 {let p=self.rotation.transform_point3(p);Vec3::new(self.center.x+(p.x-self.middle.x)*self.scale,self.center.y-(p.y-self.middle.y)*self.scale,p.z)}
    fn ray(&self,p:Vec2)->(Vec3,Vec3) {
        let p=Vec3::new((p.x-self.center.x)/self.scale+self.middle.x,-(p.y-self.center.y)/self.scale+self.middle.y,10_000.0);
        (self.inverse.transform_point3(p),self.inverse.transform_vector3(Vec3::NEG_Z))
    }
}
fn ray_box(origin:Vec3,dir:Vec3,min:Vec3,max:Vec3)->Option<f32> {
    let mut lo=0.0f32;let mut hi=f32::INFINITY;
    for axis in 0..3 {if dir[axis].abs()<1e-7 {if origin[axis]<min[axis]||origin[axis]>max[axis] {return None}}
        else {let a=(min[axis]-origin[axis])/dir[axis];let b=(max[axis]-origin[axis])/dir[axis];lo=lo.max(a.min(b));hi=hi.min(a.max(b));if lo>hi{return None}}
    } Some(lo)
}
fn line_quad(proj:&Projection,a:Vec3,b:Vec3,width:f32,uv:[f32;2],color:u32)->Option<SceneQuad> {
    let (a,b)=(proj.project(a),proj.project(b));let delta=b.truncate()-a.truncate();
    if delta.length_squared()<1e-6{return None}let n=Vec2::new(-delta.y,delta.x).normalize()*width.max(0.5)/2.0;
    Some(SceneQuad{pos:[Vec3::new(a.x+n.x,a.y+n.y,a.z),Vec3::new(b.x+n.x,b.y+n.y,b.z),Vec3::new(b.x-n.x,b.y-n.y,b.z),Vec3::new(a.x-n.x,a.y-n.y,a.z)],uv:[uv;4],color,normal:Vec3::ZERO,translucent:false})
}
fn emit(batch:&mut HudBatch,quad:&SceneQuad,project:&impl Fn(Vec3)->Vec3) {
    batch.quad_3d(quad.pos.map(|p|project(p).to_array()),quad.uv,quad.color);
}
fn emit_prepared(batch:&mut HudBatch,prepared:&Prepared,projection:&Projection) {
    // Opaque faces are drawn first. Transparent faces retain a back-to-front
    // order; all faces are still resolved by the GPU's actual depth buffer.
    let mut order:Vec<_>=(0..prepared.quads.len()).collect();
    order.sort_by(|a,b|{let a=&prepared.quads[*a];let b=&prepared.quads[*b];a.translucent.cmp(&b.translucent).then_with(|| {
        let da=a.pos.iter().map(|p|projection.rotation.transform_point3(*p).z).sum::<f32>();
        let db=b.pos.iter().map(|p|projection.rotation.transform_point3(*p).z).sum::<f32>();da.total_cmp(&db)
    })});
    for index in order {let q=&prepared.quads[index];
        let color=if q.normal.length_squared()>0.0 {shaded(q.color,projection.rotation.transform_vector3(q.normal))} else {q.color};
        batch.quad_3d(q.pos.map(|p|projection.project(p).to_array()),q.uv,color);
    }
}
impl Game<'_> {
    /// GuideME BlockImage is a single actual block state, including p:*.
    /// Its camera and depth are independent from the reader and live world.
    pub(super) fn ae2_guide_block_image(&self,batch:&mut HudBatch,node:&Value,viewport:[f32;4],clip:[f32;4]) -> Result<(),String> {
        if !valid_rect(viewport) || !valid_rect(clip) {return Ok(())}
        let Some(visible)=intersect(viewport,clip) else {return Ok(())};
        let desc=block_image_state(node)?;
        self.pack.assets.registry.resolve(&desc).map_err(|e|format!("BlockImage {}: {e}",desc.name))?;
        let mut state=desc.name.clone();
        if !desc.properties.is_empty() {
            state.push('{');state.push_str(&desc.properties.iter().map(|(k,v)|format!("{k}:{v}")).collect::<Vec<_>>().join(","));state.push('}');
        }
        let prepared=prepare(self,&[single_block(state)])?;
        let (yaw,pitch,roll)=match attr(node,"perspective").and_then(Value::as_str) {
            Some("ISOMETRIC_NORTH_WEST")=>(135.0f32,30.0f32,0.0f32),
            Some("UP")=>(120.0,0.0,45.0),
            _=>(225.0,30.0,0.0),
        };
        let rotation=Mat4::from_rotation_z(roll.to_radians())*Mat4::from_rotation_x(pitch.to_radians())*Mat4::from_rotation_y(yaw.to_radians());
        let mut lo=Vec2::splat(f32::INFINITY);let mut hi=Vec2::splat(f32::NEG_INFINITY);
        for x in [0.0,1.0] {for y in [0.0,1.0] {for z in [0.0,1.0] {
            let p=rotation.transform_point3(Vec3::new(x,y,z)).truncate();lo=lo.min(p);hi=hi.max(p);
        }}}
        let projection=Projection{rotation,inverse:rotation.inverse(),center:Vec2::new((viewport[0]+viewport[2])/2.0,(viewport[1]+viewport[3])/2.0),
            middle:(lo+hi)/2.0,scale:(10.0*number(node,"scale",1.0)).clamp(1.0,640.0)};
        let start=batch.vertices.len();emit_prepared(batch,&prepared,&projection);batch.depth_since(start,visible);
        Ok(())
    }
    pub(super) fn ae2_guide_scene(&self,batch:&mut HudBatch,node:&Value,viewport:[f32;4],clip:[f32;4],orbit:GuideSceneOrbit,pointer:Option<[f32;2]>) -> Result<GuideSceneResult,String> {
        let id=node.get("scene_id").and_then(Value::as_str).ok_or("guide scene has no id")?.to_owned();
        let interactive=attr(node,"interactive").and_then(Value::as_bool).unwrap_or(false);
        let children=node.get("children").and_then(Value::as_array).ok_or("guide scene has no children")?;
        let mut result=GuideSceneResult{scene_id:id,interactive,tooltip:None,diagnostics:Vec::new(),blocks:0,multipart_parts:0};
        if !valid_rect(viewport) || !valid_rect(clip) {return Ok(result)}
        let Some(visible)=intersect(viewport,clip) else {return Ok(result)};
        let root=self.options.resources.join("assets/ae2/ae2guide");let mut sources=Vec::new();
        let mut yaw=225.0;let mut pitch=30.0;let mut roll=0.0;
        for child in children {match child.get("name").and_then(Value::as_str) {
            Some("ImportStructure")=>{let asset=child.get("asset").and_then(Value::as_str).ok_or("guide scene has no structure path")?;
                sources.push(structure(&root,asset)?);}
            Some("IsometricCamera")=>{yaw=number(child,"yaw",0.0);pitch=number(child,"pitch",0.0);roll=number(child,"roll",0.0);}
            _=>{}
        }}
        if sources.is_empty() {return Err("guide scene has no structures".into())}
        let rotation=Mat4::from_rotation_z(roll.to_radians())*Mat4::from_rotation_x((pitch+orbit.pitch).to_radians())*Mat4::from_rotation_y((yaw+orbit.yaw).to_radians());
        let prepared=prepare(self,&sources)?;
        result.blocks=prepared.positions.len();result.multipart_parts=prepared.multipart_parts;result.diagnostics=prepared.diagnostics.clone();
        let mut lo=Vec2::splat(f32::INFINITY);let mut hi=Vec2::splat(f32::NEG_INFINITY);
        for block in &prepared.positions {
            for x in [0.0,1.0] {for y in [0.0,1.0] {for z in [0.0,1.0] {let p=rotation.transform_point3(Vec3::new(block[0] as f32+x,block[1] as f32+y,block[2] as f32+z));lo=lo.min(p.truncate());hi=hi.max(p.truncate());}}}
        }
        let projection=Projection{rotation,inverse:rotation.inverse(),center:Vec2::new((viewport[0]+viewport[2])/2.0+orbit.pan[0],(viewport[1]+viewport[3])/2.0+orbit.pan[1]),middle:(lo+hi)/2.0,scale:(10.0*number(node,"zoom",1.0)).clamp(1.0,640.0)};
        if attr(node,"background").and_then(Value::as_str)!=Some("transparent") {batch.fill(&self.gui.atlas,visible[0],visible[1],visible[2]-visible[0],visible[3]-visible[1],rgba(224,224,218,255));}
        let start=batch.vertices.len();emit_prepared(batch,&prepared,&projection);
        let white=self.gui.atlas.sprites.get("horizon/white").ok_or("guide white atlas sprite unavailable")?;
        let uv=[(white.u0+white.u1)/2.0,(white.v0+white.v1)/2.0];
        let pointer=pointer.map(Vec2::from_array).filter(|p|contains(visible,*p));let ray=pointer.map(|p|projection.ray(p));
        let mut nearest=f32::INFINITY;let mut overlay:Vec<(Vec3,u32,Vec<Value>)>=Vec::new();let mut top_lines=Vec::new();
        for child in children {let Some(kind)=child.get("name").and_then(Value::as_str) else{continue};
            let contents=child.get("children").and_then(Value::as_array).cloned().unwrap_or_default();let color=rgba_color(child,rgba(255,255,255,255));
            match kind {
                "BoxAnnotation"=>{let (Some(min),Some(max))=(triple(child,"min"),triple(child,"max")) else {result.diagnostics.push("invalid box annotation".into());continue};
                    let (min,max)=(min.min(max),min.max(max));if !contents.is_empty() {if let Some((o,d))=ray {if let Some(distance)=ray_box(o,d,min,max) {if distance<nearest {nearest=distance;result.tooltip=Some(contents.clone());}}}}
                    for a in 0..3 {for b in 0..2 {for c in 0..2 {let mut x=min;let mut y=min;let j=(a+1)%3;let k=(a+2)%3;x[j]=if b==0 {min[j]}else{max[j]};x[k]=if c==0 {min[k]}else{max[k]};y=x;y[a]=max[a];
                        if let Some(q)=line_quad(&projection,x,y,(number(child,"thickness",0.03)*projection.scale).max(1.0),uv,color){
                            if attr(child,"alwaysOnTop").and_then(Value::as_bool)==Some(true) {top_lines.push(q)} else {emit(batch,&q,&|p|p)}
                        }
                    }}}
                }
                "LineAnnotation"=>{let (Some(a),Some(b))=(triple(child,"from"),triple(child,"to")) else {result.diagnostics.push("invalid line annotation".into());continue};
                    if !contents.is_empty() {if let Some(pointer)=pointer {let pa=projection.project(a);let pb=projection.project(b);let delta=pb.truncate()-pa.truncate();
                        let fraction=if delta.length_squared()>1e-6 {(pointer-pa.truncate()).dot(delta)/delta.length_squared()} else {0.0}.clamp(0.0,1.0);
                        let closest=pa.truncate()+delta*fraction;let width=(number(child,"thickness",0.03)*projection.scale).max(1.0);
                        let distance=10_000.0-(pa.z+(pb.z-pa.z)*fraction);
                        if pointer.distance(closest)<=width/2.0+3.0 && distance<nearest {nearest=distance;result.tooltip=Some(contents.clone());}
                    }}
                    if let Some(q)=line_quad(&projection,a,b,(number(child,"thickness",0.03)*projection.scale).max(1.0),uv,color) {
                        if attr(child,"alwaysOnTop").and_then(Value::as_bool)==Some(true) {top_lines.push(q)} else {emit(batch,&q,&|p|p)}
                    }
                }
                "DiamondAnnotation"=>{if let Some(p)=triple(child,"pos") {overlay.push((projection.project(p),color,contents));}else {result.diagnostics.push("invalid diamond annotation".into());}}
                _=>{}
            }
        }
        batch.depth_since(start,visible);
        // A second scissored pass clears depth so authored overlays sit above
        // the scene without leaking outside a partially scrolled viewport.
        let overlay_start=batch.vertices.len();
        for q in top_lines {batch.quad_3d(q.pos.map(|p|[p.x,p.y,0.0]),q.uv,q.color);}
        for (p,color,contents) in overlay {
            if !contains(visible,p.truncate()) {continue}let size=5.0;
            batch.quad_3d([[p.x,p.y-size,0.0],[p.x+size,p.y,0.0],[p.x,p.y+size,0.0],[p.x-size,p.y,0.0]],[uv;4],color);
            if !contents.is_empty() && pointer.is_some_and(|m|(m-p.truncate()).abs().element_sum()<=size+2.0) {result.tooltip=Some(contents)}
        }
        if batch.vertices.len()>overlay_start {batch.depth_since(overlay_start,visible);}
        Ok(result)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn guide_scene_structure_camera_and_depth_bounds() {
        let source=r#"{size:[2,1,1],data:[{pos:[0,0,0],state:"minecraft:stone"},{pos:[1,0,0],state:"ae2:cable_bus{light_level:9,waterlogged:false}",nbt:{cable:{id:"ae2:fluix_smart_cable",visual:{connections:["west"]}},north:{id:"ae2:terminal",visual:{powered:1b,missingChannel:0b}}}}],entities:[]}"#;
        let scene=parse_structure(source).unwrap();assert_eq!(scene.blocks.len(),2);
        let trailing=r#"{data:[{pos:[0,0,0],state:"minecraft:stone",nbt:{custom:",]",}},],entities:[],}"#;
        let normalized=guide_trailing_commas(trailing);
        assert!(normalized.contains(r#"custom:",]""#));
        assert_eq!(parse_structure(trailing).unwrap().blocks.len(),1);
        if let Ok(path)=std::env::var("AE2_GUIDE_TEST_ASSET") {
            let raw=std::fs::read_to_string(path).unwrap();
            let original=parse_structure(&raw).unwrap();
            assert!(original.blocks.len()>=2);
            assert!(original.blocks.iter().any(|b|b.state.starts_with("ae2:cable_bus") && visual(b,&HashSet::new()).is_some()));
        }
        let state=descriptor(&scene.blocks[1].state).unwrap();assert_eq!(state.properties.get("light").map(String::as_str),Some("9"));
        let v=visual(&scene.blocks[1],&HashSet::new()).unwrap();assert_eq!(v.cable.as_deref(),Some("ae2:fluix_smart_cable"));assert_eq!(v.connections,["west"]);assert_eq!(v.parts[0].face,"north");assert!(v.parts[0].online);
        let image=serde_json::json!({"resource_id":"ae2:energy_cell","attrs":{"p:fullness":"full","p:powered":true,"scale":"8"}});
        let state=block_image_state(&image).unwrap();assert_eq!(state.name,"ae2:energy_cell");assert_eq!(state.properties.get("fullness").map(String::as_str),Some("full"));assert_eq!(state.properties.get("powered").map(String::as_str),Some("true"));assert!(!state.properties.contains_key("scale"));
        let rotation=Mat4::from_rotation_z(0.0)*Mat4::from_rotation_x(30f32.to_radians())*Mat4::from_rotation_y(195f32.to_radians());
        let mut p=Projection{rotation,inverse:rotation.inverse(),center:Vec2::new(100.0,100.0),middle:Vec2::ZERO,scale:40.0};
        let at=p.project(Vec3::splat(0.5)).truncate();let (origin,dir)=p.ray(at);assert!(ray_box(origin,dir,Vec3::ZERO,Vec3::ONE).is_some());
        p.center+=Vec2::new(27.0,-13.0);
        let moved=p.project(Vec3::splat(0.5)).truncate();assert!((moved-at-Vec2::new(27.0,-13.0)).length()<0.001);
        let (origin,dir)=p.ray(moved);assert!(ray_box(origin,dir,Vec3::ZERO,Vec3::ONE).is_some(),"annotation picking follows the panned model");
        assert_eq!(intersect([0.0,0.0,100.0,100.0],[25.0,30.0,80.0,120.0]),Some([25.0,30.0,80.0,100.0]));
        let mut orbit=GuideSceneOrbit::default();orbit.drag(20.0,400.0);assert_eq!(orbit.yaw,20.0);assert_eq!(orbit.pitch,40.0);
    }
    #[test]
    fn guide_scene_all_referenced_structures_parse() {
        let Ok(root)=std::env::var("AE2_GUIDE_TEST_ROOT") else {return};
        let root=std::path::Path::new(&root);
        let document:serde_json::Value=serde_json::from_slice(&std::fs::read(root.join("compiled-guide.json")).expect("compiled Guide")).expect("valid compiled Guide");
        let mut assets=std::collections::BTreeSet::<String>::new();
        fn collect(value:&Value,assets:&mut std::collections::BTreeSet<String>) {
            match value {
                Value::Array(items)=>for item in items {collect(item,assets)},
                Value::Object(object)=>{
                    if object.get("name").and_then(Value::as_str)==Some("ImportStructure") {
                        assets.insert(object.get("asset").and_then(Value::as_str).expect("normalized structure asset").to_owned());
                    }
                    for child in object.values(){collect(child,assets)}
                }
                _=>{}
            }
        }
        collect(&document["pages"],&mut assets);
        assert_eq!(assets.len(),115,"pinned unique scene imports");
        let mut errors=Vec::new();let (mut blocks,mut entities)=(0usize,0usize);
        for asset in &assets {
            match structure(root,asset) {
                Ok(scene)=>{blocks+=scene.blocks.len();entities+=scene.entities;
                    for block in &scene.blocks {if let Err(error)=descriptor(&block.state) {errors.push(format!("{asset}: {}: {error}",block.state));}}
                }
                Err(error)=>errors.push(format!("{asset}: {error}")),
            }
        }
        assert!(errors.is_empty(),"scene corpus parse failures:\n{}",errors.join("\n"));
        eprintln!("parsed {} unique guide structures, {blocks} blocks, {entities} entities",assets.len());
    }
}
