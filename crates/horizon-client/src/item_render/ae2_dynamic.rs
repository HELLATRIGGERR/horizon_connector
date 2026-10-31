//! AE2 MemoryCardBakedModel and GUI GenericStack key render adapters.
use super::*;
use crate::items::ItemStack;
use horizon_assets::{bake::{bake_flat_face, default_uv}, model::Material};

const CARD_HASH: &str = "ae2:item/memory_card_hash";
const CARD_BASE: &str = "ae2:item/memory_card_base";
const DEFAULT_LED: u32 = 0x915dcd;
const COLOR_NAMES: [(&str,u32);17] = [
    ("white",0xe0e0e0),("light_gray",0xa09fa0),("gray",0x6c6b6c),("black",0x272727),
    ("lime",0x70e259),("yellow",0xffe359),("orange",0xeca23c),("brown",0x7e5c16),
    ("red",0xd73e42),("pink",0xff99bb),("magenta",0xd5719c),("purple",0x915dcd),
    ("blue",0x3894ff),("light_blue",0x70d2ff),("cyan",0x2fccb7),("green",0x17b86d),
    ("fluix",DEFAULT_LED),
];
fn opaque(rgb: u32) -> i32 { (0xff00_0000 | (rgb & 0x00ff_ffff)) as i32 }

/// Source colors are authoritative when present; older native cards can use
/// their P2P frequency from the legacy custom-data envelope.
fn card_led_colors(stack: &ItemStack) -> [u32;8] {
    let default = [DEFAULT_LED;8];
    let Some(data) = stack.data() else { return default; };
    if let Some(colors)=data.other("ae2:memory_card_colors") {
        let Tag::List(values)=colors else{return default;};
        let mut result=default;
        for (out,value) in result.iter_mut().zip(values) {
            let Tag::String(name)=value else{return default;};
            let Ok(name)=name.to_string() else{return default;};
            let Some((_,rgb))=COLOR_NAMES.iter().find(|(key,_)|*key==name) else{return default;};
            *out=*rgb;
        }
        return result;
    }
    let custom = data.other("minecraft:custom_data");
    if let Some(card) = custom.and_then(|v|v.get("ae2_p2p_card")) {
        let Some(Tag::Short(value)) = card.get("frequency") else { return default; };
        if *value == 0 { return default; }
        let value = *value as u16;
        let digits: [u32;4] = std::array::from_fn(|i|COLOR_NAMES[((value >> (4 * (3-i))) & 15) as usize].1);
        return [digits[0],digits[0],digits[1],digits[1],digits[2],digits[2],digits[3],digits[3]];
    }
    default
}

/// Geometry and standard UVs follow MemoryCardBakedModel.buildGeneralQuads
/// and CubeBuilder's coordinate-based UV mapping. The tint index identifies
/// one of the eight independent source LEDs.
fn card_light_model() -> BakedItemModel {
    let material=Material{sprite:CARD_HASH.to_owned(),force_translucent:false};
    let mut quads=Vec::with_capacity(48);
    for x in 0..4 {
        for y in 0..2 {
            let from=[8.0+x as f32,9.0-y as f32,7.5];
            let to=[9.0+x as f32,10.0-y as f32,8.5];
            for face in [Direction::Down,Direction::Up,Direction::North,Direction::South,Direction::West,Direction::East] {
                let uv=if face==Direction::Down {[from[0],from[2],to[0],to[2]]} else {default_uv(from,to,face)};
                quads.push(bake_flat_face(from,to,uv,face,material.clone(),(x+y*4) as i32).expect("valid memory card face"));
            }
        }
    }
    BakedItemModel {quads,uses_block_light:false,particle:material,transforms:Default::default()}
}

#[derive(Debug,PartialEq)]
pub enum WrappedKey { Item(ItemStack), Fluid(String) }

fn valid_id(id:&str) -> bool {
    let Some((ns,path))=id.split_once(':') else {return false;};
    !ns.is_empty() && !path.is_empty()
        && ns.bytes().all(|c|c.is_ascii_lowercase()||c.is_ascii_digit()||matches!(c,b'_'|b'-'|b'.'))
        && path.bytes().all(|c|c.is_ascii_lowercase()||c.is_ascii_digit()||matches!(c,b'_'|b'-'|b'.'|b'/'))
        && path.split('/').all(|p|!p.is_empty() && p!="." && p!="..")
}

/// GenericStack.CODEC fields are `#t`, `id`, optional `components`, and `#`.
/// The custom-data form is the native wire adapter for a server whose item
/// protocol currently carries vanilla components only.
pub fn wrapped_key(stack:&ItemStack) -> Option<WrappedKey> {
    if stack.item.as_ref()!="ae2:wrapped_generic_stack" {return None;}
    let data=stack.data()?;
    let value=data.other("ae2:wrapped_stack")
        .or_else(||data.other("minecraft:custom_data").and_then(|v|v.get("ae2:wrapped_stack")))?;
    if !matches!(value.get("#"),Some(Tag::Long(_) | Tag::Int(_) | Tag::Short(_) | Tag::Byte(_))) {return None;}
    let (Some(Tag::String(kind)),Some(Tag::String(id)))=(value.get("#t"),value.get("id")) else{return None;};
    let kind=kind.to_string().ok()?;let id=id.to_string().ok()?;
    if !valid_id(&id) {return None;}
    match kind.as_str() {
        "ae2:i" if id!="ae2:wrapped_generic_stack" && id!="minecraft:air" => {
            let mut fields=vec![("id",Tag::String(id.as_str().into())),("count",Tag::Int(1))];
            if let Some(components)=value.get("components") {
                if !matches!(components,Tag::Compound(_)) {return None;}
                fields.push(("components",components.clone()));
            }
            crate::save::item_from_nbt(&Tag::compound(fields)).map(WrappedKey::Item)
        }
        "ae2:f" => Some(WrappedKey::Fluid(id)),
        _ => None,
    }
}

impl ItemRenderer {
    pub fn ae2_memory_card(&self, stack:&ItemStack, fallback:ItemRenderState) -> ItemRenderState {
        let display=fallback.display;
        let base=self.bake(CARD_BASE);
        if !base.quads.iter().any(|q|q.material.sprite=="ae2:item/memory_card_base") {return fallback;}
        let matrix=glam_matrix(&base.layer_matrix(display,&horizon_assets::math::Mat4::IDENTITY));
        let foil=fallback.layers.first().map_or(FoilType::None,|l|l.foil);
        let tint=stack.data().and_then(|d|d.dyed_color).unwrap_or(0x9cd3ff) as u32;
        let extents=self.quad_extents(CARD_BASE,&base);
        let mut layers=vec![LayerRender{geometry:LayerGeometry::Quads(base.clone()),tints:vec![-1,opaque(tint)],foil,matrix,particle:sprite_name(&base.particle.sprite).to_owned(),uses_block_light:false,extents}];
        if self.sprites.mask(CARD_HASH).is_some() {
            let key="ae2:memory_card/lights";
            let cached=self.baked.lock().ok().and_then(|cache|cache.get(key).cloned());
            let lights=cached.unwrap_or_else(||{
                let model=Arc::new(card_light_model());
                if let Ok(mut cache)=self.baked.lock(){cache.insert(key.to_owned(),model.clone());}
                model
            });
            let extents=self.quad_extents(key,&lights);
            layers.push(LayerRender{geometry:LayerGeometry::Quads(lights),tints:card_led_colors(stack).into_iter().map(opaque).collect(),foil,matrix,particle:sprite_name(CARD_HASH).to_owned(),uses_block_light:false,extents});
        }
        ItemRenderState{display,layers,oversized_in_gui:false,animated:false}
    }

    /// Source FluidKeyRenderHandler draws the still sprite as a GUI quad.
    /// Known vanilla fluids receive their vanilla tint; arbitrary extension
    /// render properties are not exposed by the native content system yet.
    pub fn ae2_wrapped_fluid(&self,display:DisplayContext,id:&str) -> Option<ItemRenderState> {
        if display!=DisplayContext::Gui || !valid_id(id) {return None;}
        let (namespace,path)=id.split_once(':')?;
        let path=match (namespace,path) {("minecraft","flowing_water")=>"water",("minecraft","flowing_lava")=>"lava",_=>path};
        let sprite=format!("{namespace}:block/{path}_still");
        if self.sprites.mask(&sprite).is_none() {return None;}
        let key=format!("ae2:wrapped_fluid/{sprite}");
        let cached=self.baked.lock().ok().and_then(|cache|cache.get(&key).cloned());
        let geometry=cached.unwrap_or_else(||{
            let material=Material{sprite:sprite.clone(),force_translucent:false};
            let quad=bake_flat_face([0.0,0.0,8.0],[16.0,16.0,8.0],[0.0,0.0,16.0,16.0],Direction::South,material.clone(),0).expect("valid fluid face");
            let model=Arc::new(BakedItemModel{quads:vec![quad],uses_block_light:false,particle:material,transforms:Default::default()});
            if let Ok(mut cache)=self.baked.lock(){cache.insert(key.clone(),model.clone());}
            model
        });
        let matrix=glam_matrix(&geometry.layer_matrix(display,&horizon_assets::math::Mat4::IDENTITY));
        let extents=self.quad_extents(&key,&geometry);
        let tint=if namespace=="minecraft" && path=="water" {0x3f76e4} else {0xffffff};
        Some(ItemRenderState{display,layers:vec![LayerRender{geometry:LayerGeometry::Quads(geometry),tints:vec![opaque(tint)],foil:FoilType::None,matrix,particle:sprite_name(&sprite).to_owned(),uses_block_light:false,extents}],oversized_in_gui:false,animated:false})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn card_led_geometry_and_frequency_follow_source_coordinates() {
        let geometry=card_light_model();assert_eq!(geometry.quads.len(),48);
        for index in 0..8 {
            let quads:Vec<_>=geometry.quads.iter().filter(|q|q.tint_index==index).collect();
            assert_eq!(quads.len(),6);
            let p=quads.iter().flat_map(|q|q.positions).collect::<Vec<_>>();
            let x=(8+(index%4)) as f32/16.0;let y=(9-(index/4)) as f32/16.0;
            assert!(p.iter().any(|v|v==&[x,y,7.5/16.0]));
            assert!(p.iter().any(|v|v==&[x+1.0/16.0,y+1.0/16.0,8.5/16.0]));
            assert!(quads.iter().all(|q|q.material.sprite==CARD_HASH));
        }
        let mut card=ItemStack::new("ae2:memory_card",1);
        assert_eq!(card_led_colors(&card),[DEFAULT_LED;8]);
        card.data_mut().set_other("ae2:memory_card_colors",Some(Tag::List(vec![Tag::String("red".into()),Tag::String("blue".into())])));
        assert_eq!(&card_led_colors(&card)[..3],&[0xd73e42,0x3894ff,DEFAULT_LED]);
        card.data_mut().set_other("minecraft:custom_data",Some(Tag::compound([("ae2_p2p_card",Tag::compound([("frequency",Tag::Short(0x1234))]))])));
        assert_eq!(&card_led_colors(&card)[..3],&[0xd73e42,0x3894ff,DEFAULT_LED]);
        card.data_mut().set_other("ae2:memory_card_colors",None);
        assert_eq!(card_led_colors(&card),[0xa09fa0,0xa09fa0,0x6c6b6c,0x6c6b6c,0x272727,0x272727,0x70e259,0x70e259]);
    }
    #[test]
    fn wrapped_keys_preserve_item_components_and_accept_zero_amount() {
        let mut wrapped=ItemStack::new("ae2:wrapped_generic_stack",1);
        wrapped.data_mut().set_other("ae2:wrapped_stack",Some(Tag::compound([
            ("#t",Tag::String("ae2:i".into())),("id",Tag::String("minecraft:diamond".into())),("#",Tag::Long(0)),
            ("components",Tag::compound([("minecraft:custom_data",Tag::compound([("marker",Tag::Int(7))]))])),
        ])));
        let Some(WrappedKey::Item(item))=wrapped_key(&wrapped) else {panic!("item key")};
        assert_eq!(item.item.as_ref(),"minecraft:diamond");
        assert_eq!(item.data().unwrap().other("minecraft:custom_data").unwrap().get("marker"),Some(&Tag::Int(7)));
        wrapped.data_mut().set_other("minecraft:custom_data",Some(Tag::compound([("ae2:wrapped_stack",Tag::compound([("#t",Tag::String("ae2:f".into())),("id",Tag::String("minecraft:water".into())),("#",Tag::Long(1000))]))])));
        assert!(matches!(wrapped_key(&wrapped),Some(WrappedKey::Item(item)) if item.item.as_ref()=="minecraft:diamond"));
        wrapped.data_mut().set_other("ae2:wrapped_stack",None);
        assert_eq!(wrapped_key(&wrapped),Some(WrappedKey::Fluid("minecraft:water".into())));
        assert!(!valid_id("minecraft:../secret"));
    }
}
