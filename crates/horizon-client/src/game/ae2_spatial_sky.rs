//! AE2 19.2.18 SpatialSkyRender (LGPL-3.0-or-later): black sky with
//! fifty randomized sparkle attempts and a two-second quadratic fade.
use super::Game;
use horizon_client::mesher::Vertex;
use horizon_core::{random::{LegacyRandom,RandomSource},geometry::trig::atan2};
use std::sync::{Mutex,OnceLock};

// The pinned 1.21.1 Mth float table/indexing, rather than the host's newer
// double-angle indexing. All random and geometric intermediates remain f32.
fn sine()->&'static [f32] {
    static TABLE:OnceLock<Vec<f32>>=OnceLock::new();
    TABLE.get_or_init(||(0..65536).map(|i|horizon_core::jmath::sin(i as f64 * std::f64::consts::PI * 2.0 / 65536.0) as f32).collect())
}
fn sin(v:f32)->f32 {sine()[((v*10430.378_f32) as i32 & 65535) as usize]}
fn cos(v:f32)->f32 {sine()[((v*10430.378_f32+16384.0) as i32 & 65535) as usize]}
fn rebuild(random:&mut impl RandomSource)->Vec<[f32;3]> {
    let mut vertices=Vec::new();
    for _ in 0..50 {
        let mut ix=random.next_float()*2.0-1.0;
        let mut iy=random.next_float()*2.0-1.0;
        let mut iz=random.next_float()*2.0-1.0;
        let size=0.05+random.next_float()*0.1;
        let dist=ix*ix+iy*iy+iz*iz;
        if dist<1.0 && dist>0.01 {
            let dist=1.0/dist.sqrt();ix*=dist;iy*=dist;iz*=dist;
            let (x,y,z)=(ix*100.0,iy*100.0,iz*100.0);
            let yaw=atan2(ix as f64,iz as f64) as f32;
            let (sy,cy)=(sin(yaw),cos(yaw));
            let pitch=atan2((ix*ix+iz*iz).sqrt() as f64,iy as f64) as f32;
            let (sp,cp)=(sin(pitch),cos(pitch));
            let rotation=random.next_float()*std::f32::consts::PI*2.0;
            let (sr,cr)=(sin(rotation),cos(rotation));
            for j in 0..4i32 {
                let a=((j&2)-1) as f32*size;
                let b=(((j+1)&2)-1) as f32*size;
                let u=a*cr-b*sr;let v=b*cr+a*sr;
                let dy=u*sp+0.0*cp;let side=0.0*sp-u*cp;
                let dx=side*sy-v*cy;let dz=v*sy+side*cy;
                vertices.push([x+dx,y+dy,z+dz]);
            }
        }
    }
    vertices
}
fn fade(elapsed:u128)->f32 {
    let t=elapsed as f32/1000.0;
    0.25*(1.0-((t-1.0)*(t-1.0)).abs())
}
struct Sparkles {cycle:u128,random:LegacyRandom,vertices:Vec<[f32;3]>}
impl Game<'_> {
    pub(super) fn ae2_spatial_sky(&self)->Vec<Vertex> {
        static STATE:OnceLock<Mutex<Sparkles>>=OnceLock::new();
        let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis();
        let state=STATE.get_or_init(||Mutex::new(Sparkles{cycle:0,random:LegacyRandom::new(now as i64),vertices:Vec::new()}));
        let Ok(mut state)=state.lock() else{return Vec::new()};
        if now.saturating_sub(state.cycle)>2000 {
            state.cycle=now;
            state.vertices=rebuild(&mut state.random);
        }
        let brightness=fade(now.saturating_sub(state.cycle));
        if brightness<=0.0 {return Vec::new();}
        let Some(white)=self.gui.atlas.sprites.get("horizon/white") else{return Vec::new()};
        let uv=[(white.u0+white.u1)*0.5,(white.v0+white.v1)*0.5];
        let level=(brightness*255.0).clamp(0.0,255.0) as u32;
        let color=u32::from_le_bytes([level as u8,level as u8,level as u8,255]);
        state.vertices.iter().map(|pos|Vertex{glint_uv:[0.0,0.0],normal:[0;4],pos:*pos,uv,layer:2,color,light:0}).collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spatial_sparkles_match_unmodified_java_source_and_remaining_random_stream() {
        let cases:serde_json::Value=serde_json::from_str(include_str!("ae2_spatial_sky_oracle.json")).unwrap();
        for case in cases.as_array().unwrap() {
            let seed=case["seed"].as_str().unwrap().parse::<i64>().unwrap();
            let mut random=LegacyRandom::new(seed);
            let actual=rebuild(&mut random);
            let expected=case["vertices_bits"].as_array().unwrap();
            assert_eq!(actual.len(),expected.len(),"seed {seed} vertex count");
            for (i,(actual,expected)) in actual.iter().zip(expected).enumerate() {
                let expected:[i32;3]=std::array::from_fn(|axis|expected[axis].as_i64().unwrap() as i32);
                assert_eq!(actual.map(|f|f.to_bits() as i32),expected,"seed {seed} vertex {i}");
            }
            assert_eq!(random.next_int(),case["next_int"].as_i64().unwrap() as i32,"seed {seed} remaining stream");
        }
    }
    #[test]
    fn spatial_sparkles_follow_source_cycle_and_have_bounded_sky_geometry() {
        assert_eq!(fade(0),0.0);assert_eq!(fade(1000),0.25);assert_eq!(fade(2000),0.0);
        assert_eq!(fade(500),fade(1500));
        let mut random=LegacyRandom::new(617);
        let first=rebuild(&mut random);let second=rebuild(&mut random);
        assert!(!first.is_empty() && first.len()<=200 && first.len()%4==0);
        assert_ne!(first,second);
        for p in first {let d=p.iter().map(|v|v*v).sum::<f32>().sqrt();assert!((d-100.0).abs()<0.01);}
    }
}
