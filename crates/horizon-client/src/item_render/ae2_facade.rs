//! AE2 FacadeBuilder/FacadeBakedItemModel; clipping and UV interpolation follow
//! AE2 db17504a and its LGPL-2.1-or-later CodeChickenLib transformers.
use super::*;
use horizon_assets::bake::ModelQuad;

fn clip(quad:&ModelQuad)->Option<ModelQuad> {
    let (x,y)=match quad.facing {Direction::Down|Direction::Up=>(0,2),Direction::North|Direction::South=>(0,1),_=>(2,1)};
    let mut out=quad.clone();
    for pos in &mut out.positions {pos[0]=pos[0].clamp(0.0,1.0);pos[1]=pos[1].clamp(0.0,1.0);pos[2]=pos[2].clamp(0.0,(1.0f64/16.0-0.002) as f32);}
    if (1..4).all(|i|out.positions[i][x]==out.positions[0][x]) || (1..4).all(|i|out.positions[i][y]==out.positions[0][y]) {return None;}
    let origin=quad.positions[0];
    let mut p10=0;let mut p01=0;let mut p11=0;
    for i in 1..4 {if quad.positions[i][y]==origin[y] {p10=i;} else if quad.positions[i][x]==origin[x] {p01=i;} else {p11=i;}}
    let dx=quad.positions[p10][x]-origin[x];let dy=quad.positions[p01][y]-origin[y];
    if dx==0.0 || dy==0.0 {return None;}
    for i in 0..4 {
        let rx=(out.positions[i][x]-origin[x])/dx;let ry=(out.positions[i][y]-origin[y])/dy;
        for axis in 0..2 {let f0=quad.uv[0][axis]*(1.0-rx)+quad.uv[p10][axis]*rx;let f1=quad.uv[p01][axis]*(1.0-rx)+quad.uv[p11][axis]*rx;out.uv[i][axis]=f0*(1.0-ry)+f1*ry;}
    }
    out.cull=None;
    Some(out)
}
impl ItemRenderer {
    pub fn ae2_facade(&self,texture_id:&str,texture:ItemRenderState)->ItemRenderState {
        let display=texture.display;
        let base=self.bake("ae2:item/facade_base");
        let local=horizon_assets::math::Mat4::IDENTITY;
        let matrix=glam_matrix(&base.layer_matrix(display,&local));
        let mut layers=Vec::new();
        for (index,layer) in texture.layers.into_iter().enumerate() {
            let LayerGeometry::Quads(source)=layer.geometry else {continue;};
            let key=format!("ae2:facade/{texture_id}/{index}");
            let cached=self.baked.lock().ok().and_then(|cache|cache.get(&key).cloned());
            let geometry=cached.unwrap_or_else(||{
                let mut baked=(*source).clone();baked.quads=source.quads.iter().filter_map(clip).collect();baked.uses_block_light=false;
                let baked=Arc::new(baked);if let Ok(mut cache)=self.baked.lock(){cache.insert(key.clone(),baked.clone());}baked
            });
            let extents=self.quad_extents(&key,&geometry);
            layers.push(LayerRender{geometry:LayerGeometry::Quads(geometry),matrix,extents,uses_block_light:false,..layer});
        }
        let extents=self.quad_extents("ae2:item/facade_base",&base);
        layers.push(LayerRender{geometry:LayerGeometry::Quads(base.clone()),tints:Vec::new(),foil:FoilType::None,matrix,particle:sprite_name(&base.particle.sprite).to_owned(),uses_block_light:false,extents});
        ItemRenderState{display,layers,oversized_in_gui:false,animated:texture.animated}
    }
}
#[cfg(test)] mod tests {
    use super::*;
    #[test]
    fn north_facade_clamps_thin_geometry_and_reinterpolates_side_uvs() {
        let side=ModelQuad{positions:[[1.,1.,1.],[1.,0.,1.],[1.,0.,0.],[1.,1.,0.]],uv:[[1.,0.],[1.,1.],[0.,1.],[0.,0.]],facing:Direction::East,cull:Some(Direction::East),material:horizon_assets::model::Material{sprite:"minecraft:block/oak_log".into(),force_translucent:false},tint_index:2,shade_direction_override:None,light_emission:0};
        let result=clip(&side).unwrap();let thin=(1.0f64/16.0-0.002) as f32;
        assert_eq!(result.positions,[[1.,1.,thin],[1.,0.,thin],[1.,0.,0.],[1.,1.,0.]]);
        let expected=[[thin,0.],[thin,1.],[0.,1.],[0.,0.]];for i in 0..4 {for a in 0..2 {assert!((result.uv[i][a]-expected[i][a]).abs()<1e-7);}}
        assert_eq!(result.material,side.material);assert_eq!(result.tint_index,2);assert_eq!(result.cull,None);
        let mut invisible=side.clone();invisible.positions=[[1.,1.,1.],[1.,0.,1.],[1.,0.,0.5],[1.,1.,0.5]];
        assert!(clip(&invisible).is_none());
        let mut front=side;front.facing=Direction::North;front.positions=[[1.,1.,0.],[1.,0.,0.],[0.,0.,0.],[0.,1.,0.]];
        assert_eq!(clip(&front).unwrap().uv,front.uv);
    }
}
