//! Local Guide item and shared physical/virtual reader input integration.
use super::{Game, Screen};
use horizon_client::{items::ItemStack, text::Text};
use serde_json::{Value,json};
use winit::event::MouseButton;
impl Game<'_> {
    pub(super) fn ae2_guide_active(&self) -> bool {self.screen==Screen::Mod && self.ae2_guide.is_some()}
    pub(super) fn ae2_guide_open(&mut self) -> bool {
        match super::ae2_guide::GuideState::load(&self.options.resources) {
            Ok(state)=>{self.ae2_guide=Some(state);self.ae2_guide_left_down=false;self.ae2_guide_right_down=false;self.screen=Screen::Mod;self.capture(false);true},
            Err(error)=>{self.system_message(Text::literal(format!("AE2 guide: {error}")));false},
        }
    }
    pub(super) fn ae2_try_guide_use(&mut self,stack:&ItemStack)->bool {
        if stack.item.as_ref()!="ae2:guide" || self.serving.is_some() || self.lan_acting.is_some(){return false;}
        self.ae2_guide_open();true
    }
    pub(super) fn ae2_guide_control(&mut self,words:&[&str])->horizon_client::Result<Option<Value>> {
        if words.first()!=Some(&"guide"){return Ok(None);}
        let action=words.get(1).copied().unwrap_or("status");
        if action=="status"{return Ok(Some(self.ae2_guide_snapshot()));}
        if !self.ae2_guide_active(){return Err("the AE2 guide is not open".into());}
        let number=|i:usize|->horizon_client::Result<f32>{let v:f32=words.get(i).ok_or("guide input requires x y")?.parse()?;if !v.is_finite(){return Err("guide coordinates must be finite".into());}Ok(v)};
        match action {
            "pointer"|"click"=>{
                self.cursor=(number(2)?,number(3)?);let phase=words.get(4).copied().unwrap_or("move");
                if action=="click" {
                    self.ae2_guide_left_down=false;self.ae2_guide_right_down=false;self.ae2_guide_pointer(self.cursor,false,false);
                    self.ae2_guide_click(if phase=="right"{MouseButton::Right}else{MouseButton::Left});
                    if self.ae2_guide_active(){self.ae2_guide_pointer(self.cursor,false,false);}
                }else{
                    let right=match words.get(5).copied().unwrap_or("left"){"right"=>true,"left"=>false,_=>return Err("guide pointer button must be left or right".into())};
                    let pressed=if right{&mut self.ae2_guide_right_down}else{&mut self.ae2_guide_left_down};
                    match phase{"down"=>*pressed=true,"up"=>*pressed=false,"move"=>{},_=>return Err("guide pointer phase must be down, move or up".into())}
                    if phase=="down"{self.ae2_guide_click(if right{MouseButton::Right}else{MouseButton::Left});}
                    if self.ae2_guide_active(){self.ae2_guide_pointer(self.cursor,self.ae2_guide_left_down,self.ae2_guide_right_down);}
                }
            },
            "scroll"=>self.ae2_guide_scroll(number(2)?),
            _=>return Err("guide status|click x y [right]|pointer x y down|move|up [left|right]|scroll delta".into()),
        }
        Ok(Some(json!({"ok":true})))
    }
}
