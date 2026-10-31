//! Native reader for the compiled, source-ordered AE2 Guide corpus.
use super::{Game, Screen, ae2_guide_scene::GuideSceneOrbit};
use horizon_client::{
    crafting::{Ingredient, Recipe},
    hud::{HudAtlas, HudBatch},
    items::ItemStack,
    recipe_book::Display,
    text::{Style, Text},
    ui::{self, EditBox},
};
use serde_json::Value;
use std::{collections::{BTreeMap, HashMap}, path::{Component, Path}, sync::Arc};
use winit::{event::MouseButton, keyboard::KeyCode};

const PAPER: u32 = 0xffd9e9f1;
const INK: u32 = 0xff252a30;
const LINK: u32 = 0xff2a498a;
const SUBTLE: u32 = 0xff5c6a78;
const LINE: u32 = 0xffa7bbc9;
const ROW: f32 = 12.0;

type Rect = [f32; 4];
fn inside(r: Rect, p: (f32,f32)) -> bool { p.0 >= r[0] && p.0 < r[2] && p.1 >= r[1] && p.1 < r[3] }
fn intersect(a: Rect, b: Rect) -> bool { a[0] < b[2] && a[2] > b[0] && a[1] < b[3] && a[3] > b[1] }
fn safe_asset(s: &str) -> bool {
    !s.is_empty() && !s.contains('\\') && Path::new(s).components().all(|c| matches!(c, Component::Normal(_)))
}
fn children(v: &Value) -> &[Value] { v.get("children").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]) }
fn string<'a>(v: &'a Value, key: &str) -> &'a str { v.get(key).and_then(Value::as_str).unwrap_or("") }
fn attr<'a>(v: &'a Value, key: &str) -> &'a str { v.get("attrs").map_or("", |a| string(a,key)) }
fn text_of(v: &Value, out: &mut String) {
    match v {
        Value::Array(a) => for c in a { text_of(c,out); },
        Value::Object(_) => {
            if let Some(s)=v.get("text").and_then(Value::as_str) { out.push_str(s); out.push(' '); }
            if let Some(id)=v.get("resource_id").and_then(Value::as_str) { out.push_str(id); out.push(' '); }
            if let Some(a)=v.get("children") { text_of(a,out); }
            if let Some(a)=v.get("items") { text_of(a,out); }
            if let Some(a)=v.get("rows") { text_of(a,out); }
            if let Some(a)=v.get("header") { text_of(a,out); }
        }
        _ => ()
    }
}
fn plain(v: &Value) -> String { let mut s=String::new(); text_of(v,&mut s); s.trim().to_owned() }
fn stack(id: &str) -> ItemStack { ItemStack { item: Arc::from(id), count: 1, damage: 0, extra: None } }
fn normalize_id(s: &str) -> String { if s.contains(':') { s.to_owned() } else { format!("minecraft:{s}") } }

#[derive(Clone)]
struct Page { title:String, parent:Option<String>, position:i64, nodes:Vec<Value>, search:String }
#[derive(Clone)]
struct History { page:String, scroll:f32 }
#[derive(Clone)]
enum Action { Page(String,Option<String>), Item(String), External(String), Back, Forward, Home, Scene(String), Zoom(String,f32) }
#[derive(Clone)]
struct Hit { rect:Rect, action:Action, tip:Option<String> }
#[derive(Clone,Default)]
struct Ink { bold:bool, italic:bool, strike:bool, code:bool, action:Option<Action> }
impl Ink {
    fn style(&self)->Style {Style{bold:Some(self.bold),italic:Some(self.italic),strikethrough:Some(self.strike),underlined:Some(self.action.is_some()),..Style::default()}}
    fn width(&self,atlas:&HudAtlas,text:&str)->f32 {let style=self.style();text.chars().map(|c|atlas.font.advance(c,&style)).sum()}
}
#[derive(Clone)]
enum Op {
    Text { x:f32,y:f32,text:String,ink:Ink,scale:f32 },
    Image { rect:Rect, sprite:String },
    Item { rect:Rect, stack:ItemStack },
    BlockImage { rect:Rect, node:Value },
    Scene { rect:Rect, node:Value },
    Fill { rect:Rect, color:u32 },
    Recipe { rect:Rect, id:String },
}

/// Independent screen state; the host only needs to load it and route input.
pub(super) struct GuideState {
    root:String,
    pages:BTreeMap<String,Page>,
    item_index:HashMap<String,Vec<String>>,
    categories:BTreeMap<String,Vec<String>>,
    current:String,
    history:Vec<History>, history_index:usize,
    scroll:f32, scroll_max:f32, side_scroll:f32, side_max:f32,
    search:EditBox,
    hits:Vec<Hit>, doc_clip:Rect, side_clip:Rect,
    pending_anchor:Option<String>,
    orbits:HashMap<String,GuideSceneOrbit>, zoom:HashMap<String,f32>, dragging:Option<(String,(f32,f32),MouseButton)>,
    status:Option<String>,
    visible:Vec<Value>,
    recipes:HashMap<String,Vec<(String,Display,ItemStack)>>,
}
impl GuideState {
    pub(super) fn load(root:&Path) -> Result<Self,String> {
        let path=root.join("assets/ae2/ae2guide/compiled-guide.json");
        let bytes=std::fs::read(&path).map_err(|e|format!("Could not read AE2 Guide: {e}"))?;
        let v:Value=serde_json::from_slice(&bytes).map_err(|e|format!("Invalid AE2 Guide: {e}"))?;
        if v["version"].as_u64()!=Some(1) || v["guide_id"].as_str()!=Some("ae2:guide") { return Err("Unsupported AE2 Guide format".into()); }
        let root=string(&v,"root").to_owned();
        let mut pages=BTreeMap::new();
        let entries=v["pages"].as_object().ok_or("Guide has no pages")?;
        for (id,p) in entries {
            if !safe_asset(id) || !id.ends_with(".md") || string(p,"id")!=id { return Err(format!("Invalid guide page: {id}")); }
            let nav=&p["navigation"];
            let title=nav["title"].as_str().filter(|s|!s.is_empty()).unwrap_or(id).to_owned();
            let nodes=p["nodes"].as_array().ok_or_else(||format!("Page {id} has no content"))?.clone();
            let mut search=format!("{title} {id} "); text_of(&p["nodes"],&mut search);
            pages.insert(id.clone(),Page { title, parent:nav["parent"].as_str().map(str::to_owned),position:nav["position"].as_i64().unwrap_or(i64::MAX),nodes,search:search.to_lowercase() });
        }
        if !pages.contains_key(&root) { return Err("Guide root page is missing".into()); }
        fn index(v:&Value)->HashMap<String,Vec<String>> {
            v.as_object().map(|m|m.iter().map(|(k,v)|(k.clone(),v.as_array().map(|a|a.iter().filter_map(Value::as_str).map(str::to_owned).collect()).unwrap_or_default())).collect()).unwrap_or_default()
        }
        let item_index=index(&v["item_index"]);
        let categories=index(&v["categories"]).into_iter().collect();
        let mut search=EditBox::new(0.0,0.0,100.0,18.0); search.max_length=80; search.focused=false;
        Ok(Self { current:root.clone(), history:vec![History{page:root.clone(),scroll:0.0}],history_index:0,root,pages,item_index,categories,scroll:0.0,scroll_max:0.0,side_scroll:0.0,side_max:0.0,search,hits:Vec::new(),doc_clip:[0.0;4],side_clip:[0.0;4],pending_anchor:None,orbits:HashMap::new(),zoom:HashMap::new(),dragging:None,status:None,visible:Vec::new(),recipes:HashMap::new() })
    }
    /// Shows the page the item index gives item `id` (`OpenGuideButton`);
    /// false when it has none.
    pub(super) fn show_item(&mut self, id: &str) -> bool {
        let Some(page) = self.item_index.get(id).and_then(|p| p.first()).cloned() else { return false };
        self.navigate(&page, None);
        true
    }
    fn navigate(&mut self, page:&str, anchor:Option<String>) {
        if !self.pages.contains_key(page) { self.status=Some(format!("Page unavailable: {page}")); return; }
        if let Some(h)=self.history.get_mut(self.history_index) { h.scroll=self.scroll; }
        self.history.truncate(self.history_index+1);
        self.history.push(History {page:page.to_owned(),scroll:0.0}); self.history_index+=1;
        self.current=page.to_owned(); self.scroll=0.0; self.pending_anchor=anchor; self.status=None;
    }
    fn go_history(&mut self, delta:isize) {
        let next=(self.history_index as isize+delta).clamp(0,self.history.len().saturating_sub(1) as isize) as usize;
        if next==self.history_index { return; }
        self.history[self.history_index].scroll=self.scroll; self.history_index=next;
        self.current=self.history[next].page.clone(); self.scroll=self.history[next].scroll; self.pending_anchor=None; self.status=None;
    }
    fn action(&mut self, action:Action) {
        match action {
            Action::Page(p,a)=>self.navigate(&p,a),
            Action::Item(id)=>if let Some(p)=self.item_index.get(&id).and_then(|p|p.first()).cloned() { self.navigate(&p,None); } else { self.status=Some(format!("No guide page for {id}")); },
            Action::Back=>self.go_history(-1), Action::Forward=>self.go_history(1),
            Action::Home=>{ let root=self.root.clone(); self.navigate(&root,None); },
            Action::External(url)=>self.status=Some(format!("External link: {url}")),
            Action::Scene(id)=>self.dragging=Some((id,(0.0,0.0),MouseButton::Left)),
            Action::Zoom(id,factor)=>{let zoom=self.zoom.entry(id).or_insert(1.0);*zoom=(*zoom*factor).clamp(0.2,4.0);},
        }
    }
    fn ordered_pages(&self)->Vec<(String,usize)> {
        fn append(id:&str,depth:usize,pages:&BTreeMap<String,Page>,out:&mut Vec<(String,usize)>) {
            if depth>12 || out.iter().any(|(s,_)|s==id) {return;}
            out.push((id.to_owned(),depth));
            let mut sub:Vec<_>=pages.iter().filter(|(_,p)|p.parent.as_deref()==Some(id)).collect();
            sub.sort_by_key(|(_,p)|(p.position,p.title.to_lowercase()));
            for (child,_) in sub { append(child,depth+1,pages,out); }
        }
        let mut out=Vec::new(); append(&self.root,0,&self.pages,&mut out);
        let mut orphan:Vec<_>=self.pages.iter().filter(|(id,_)| !out.iter().any(|(n,_)|n==*id)).collect();
        orphan.sort_by_key(|(_,p)|(p.position,p.title.to_lowercase()));
        for (id,_) in orphan { append(id,0,&self.pages,&mut out); }
        out
    }
}

struct Layout<'a,'g> {
    game:&'a Game<'g>, guide:&'a GuideState, atlas:&'a HudAtlas,
    x0:f32, width:f32, x:f32, y:f32, line_height:f32,
    ops:Vec<Op>, hits:Vec<Hit>, anchors:HashMap<String,f32>,
}
impl<'a,'g> Layout<'a,'g> {
    fn new(game:&'a Game<'g>,guide:&'a GuideState,x0:f32,width:f32)->Self {
        Self {game,guide,atlas:&game.gui.atlas,x0,width,x:x0,y:0.0,line_height:ROW,ops:Vec::new(),hits:Vec::new(),anchors:HashMap::new()}
    }
    fn newline(&mut self) { self.y+=self.line_height; self.x=self.x0; self.line_height=ROW; }
    fn block_start(&mut self) { if self.x>self.x0+0.1 {self.newline();} }
    fn text(&mut self,s:&str,ink:&Ink,scale:f32) {
        // Markdown source newlines are soft wraps; explicit break nodes are hard wraps.
        for word in s.split_inclusive(char::is_whitespace) {
            let word=word.replace('\n'," ").replace('\r',"");
            if word.is_empty() {continue;}
            let mut rest=word.as_str();
            while !rest.is_empty() {
                let available=(self.x0+self.width-self.x).max(1.0);
                let all=ink.width(self.atlas,rest)*scale;
                if all>available && self.x>self.x0+0.1 {self.newline(); continue;}
                let mut end=rest.len();
                if all>available {
                    end=rest.char_indices().map(|(i,c)|i+c.len_utf8()).take_while(|i|ink.width(self.atlas,&rest[..*i])*scale<=available).last().unwrap_or_else(||rest.chars().next().unwrap().len_utf8());
                }
                let part=&rest[..end]; let w=ink.width(self.atlas,part)*scale;
                self.ops.push(Op::Text{x:self.x,y:self.y,text:part.to_owned(),ink:ink.clone(),scale});
                if let Some(action)=&ink.action {self.hits.push(Hit{rect:[self.x,self.y,self.x+w,self.y+9.0*scale],action:action.clone(),tip:None});}
                self.x+=w; self.line_height=self.line_height.max(10.0*scale+2.0); rest=&rest[end..];
                if !rest.is_empty(){self.newline();}
            }
        }
    }
    fn item(&mut self,id:&str,size:f32) {
        let size=size.clamp(16.0,self.width.max(16.0));
        if self.x+size>self.x0+self.width && self.x>self.x0 {self.newline();}
        let rect=[self.x,self.y,self.x+size,self.y+size];
        self.ops.push(Op::Item{rect,stack:stack(id)});
        self.hits.push(Hit{rect,action:Action::Item(id.to_owned()),tip:Some(self.game.pack.names.display_item(id))});
        self.x+=size+3.0; self.line_height=self.line_height.max(size+3.0);
    }
    fn block_image(&mut self,node:&Value) {
        let size=(attr(node,"scale").parse::<f32>().unwrap_or(1.0)*16.0).clamp(16.0,self.width.max(16.0));
        if self.x+size>self.x0+self.width && self.x>self.x0 {self.newline();}
        let rect=[self.x,self.y,self.x+size,self.y+size];let id=string(node,"resource_id");
        self.ops.push(Op::BlockImage{rect,node:node.clone()});
        self.hits.push(Hit{rect,action:Action::Item(id.to_owned()),tip:Some(self.game.pack.names.display_item(id))});
        self.x+=size+3.0;self.line_height=self.line_height.max(size+3.0);
    }
    fn image(&mut self,node:&Value,block:bool) {
        let asset=string(node,"asset");
        if !safe_asset(asset) { self.text("[Image unavailable]",&Ink::default(),1.0); return; }
        let sprite=format!("ae2:guide/{}",asset.strip_suffix(".png").unwrap_or(asset));
        let Some(r)=self.atlas.sprites.get(&sprite) else {self.text(&format!("[{}]",string(node,"alt")),&Ink::default(),1.0);return;};
        let (w,h)=if block { let w=(r.w as f32).min(self.width); (w,w*r.h as f32/(r.w.max(1) as f32)) } else {let h=14.0; (h*r.w as f32/(r.h.max(1) as f32),h)};
        if block {self.block_start();} else if self.x+w>self.x0+self.width && self.x>self.x0 {self.newline();}
        self.ops.push(Op::Image{rect:[self.x,self.y,self.x+w,self.y+h],sprite}); self.x+=w+2.0; self.line_height=self.line_height.max(h+2.0);
        if block {self.newline(); self.y+=3.0;}
    }
    fn scene(&mut self,node:&Value) {
        self.block_start(); let h=(self.width*0.55).clamp(120.0,220.0);
        let rect=[self.x0,self.y,self.x0+self.width,self.y+h];
        self.ops.push(Op::Scene{rect,node:node.clone()});
        if node["attrs"]["interactive"].as_bool()==Some(true) {self.hits.push(Hit{rect,action:Action::Scene(string(node,"scene_id").to_owned()),tip:None});}
        self.y+=h+6.0; self.x=self.x0;
    }
    fn inline(&mut self,node:&Value,ink:&Ink) {
        match string(node,"type") {
            "text"|"code"=> {let mut s=ink.clone(); s.code|=string(node,"type")=="code"; self.text(string(node,"text"),&s,1.0);},
            "break"=>self.newline(),
            "emphasis"|"strong"|"strikethrough"=> {let mut s=ink.clone(); match string(node,"type") {"emphasis"=>s.italic=true,"strong"=>s.bold=true,_=>s.strike=true}; for c in children(node){self.inline(c,&s);} },
            "link"=> {let mut s=ink.clone();s.action=target(node);for c in children(node){self.inline(c,&s);} },
            "image"=>self.image(node,false),
            "element"=>match string(node,"name") {
                "ItemLink"=> {let id=string(node,"resource_id");let mut s=ink.clone();s.action=Some(Action::Item(id.to_owned()));let label=self.game.pack.names.display_item(id);self.text(&label,&s,1.0);},
                "ItemImage"|"ItemIcon"=> {let size=attr(node,"scale").parse::<f32>().unwrap_or(1.0)*16.0;self.item(string(node,"resource_id"),size);},
                "BlockImage"=>self.block_image(node),
                "ae2:ConfigValue"=> {let val=match attr(node,"name") {"crystalResonanceGeneratorRate"=>horizon_client::ae2_common_config::passive_power().resonance_rate.to_string(),_=>format!("[{}]",attr(node,"name"))};self.text(&val,ink,1.0);},
                "br"=>self.newline(),"a"=> {let anchor=string(node,"anchor");if !anchor.is_empty(){self.anchors.insert(anchor.to_owned(),self.y);}let mut s=ink.clone();s.action=target(node);for c in children(node){self.inline(c,&s);} },
                "GameScene"=>self.scene(node), "RecipeFor"|"Recipe"=>self.recipe(node),
                _=> for c in children(node){self.inline(c,ink);},
            },
            "paragraph"|"list_item"=>for c in children(node){self.inline(c,ink);},
            _=>for c in children(node){self.inline(c,ink);},
        }
    }
    fn recipe(&mut self,node:&Value) {
        self.block_start();
        let id=string(node,"resource_id");
        let key=format!("{}|{}",string(node,"name"),id);
        let recipes=self.guide.recipes.get(&key).cloned().unwrap_or_default();
        if recipes.is_empty() {self.text(&format!("Recipe unavailable: {id}"),&Ink::default(),1.0);self.newline();self.y+=3.0;return;}
        for (name,display,result) in recipes {
            let (ingredients,cols)=match display {Display::Shaped{width,cells,..}=>(cells,width.max(1)),Display::Shapeless(c)=>(c.into_iter().map(Some).collect(),3),Display::Furnace(i)=>(vec![Some(i)],1)};
            let rows=ingredients.len().div_ceil(cols).max(1);let card_h=(rows as f32*20.0+18.0).max(48.0);
            self.ops.push(Op::Recipe{rect:[self.x0,self.y,self.x0+self.width,self.y+card_h],id:name.clone()});
            let label=name.split_once(" | ").map(|(label,_)|label).unwrap_or("Crafting");
            let title=format!("{} · {label}",self.game.pack.names.display_item(&result.item));
            self.ops.push(Op::Text{x:self.x0+4.0,y:self.y+3.0,text:title,ink:Ink{bold:true,..Ink::default()},scale:1.0});
            for (i,ing) in ingredients.iter().enumerate() { if let Some(id)=ing.as_ref().and_then(|v|v.1.first()) {let x=self.x0+5.0+(i%cols) as f32*20.0;let y=self.y+16.0+(i/cols) as f32*20.0;let rect=[x,y,x+16.0,y+16.0];self.ops.push(Op::Item{rect,stack:stack(id)});self.hits.push(Hit{rect,action:Action::Item(id.clone()),tip:Some(self.game.pack.names.display_item(id))});}}
            let x=(self.x0+10.0+cols as f32*20.0).min(self.x0+self.width-40.0);let y=self.y+18.0;
            self.ops.push(Op::Text{x,y,text:"→".into(),ink:Ink::default(),scale:1.0});let rect=[x+14.0,y-2.0,x+30.0,y+14.0];
            self.hits.push(Hit{rect,action:Action::Item(result.item.to_string()),tip:Some(format!("{} ({name})",self.game.pack.names.display_item(&result.item)))});self.ops.push(Op::Item{rect,stack:result});
            self.y+=card_h+4.0;
        }
    }
    fn block(&mut self,node:&Value) {
        let kind=string(node,"type");
        match kind {
            "paragraph"=> {self.block_start(); let only=children(node);if only.len()==1&&string(&only[0],"type")=="image" {self.image(&only[0],true);} else {for c in only{self.inline(c,&Ink::default());} if self.x>self.x0 {self.newline();} self.y+=4.0;}},
            "heading"=> {self.block_start();let a=string(node,"anchor");if !a.is_empty(){self.anchors.insert(a.to_owned(),self.y);} let mut ink=Ink::default();ink.bold=true;let scale=if node["level"].as_u64().unwrap_or(2)==1 {1.4}else{1.15}; for c in children(node){if string(c,"type")=="text"{self.text(string(c,"text"),&ink,scale);}else{self.inline(c,&ink);}} if self.x>self.x0 {self.newline();}self.y+=5.0;},
            "list"=> {let ordered=node["ordered"].as_bool()==Some(true);let start=node["start"].as_i64().unwrap_or(1);if let Some(items)=node["items"].as_array(){for (i,item) in items.iter().enumerate(){self.block_start();self.text(&if ordered{format!("{}. ",start+i as i64)}else{"• ".to_owned()},&Ink::default(),1.0);for c in children(item){if string(c,"type")=="list" {self.newline();self.block(c);}else{self.inline(c,&Ink::default());}}if self.x>self.x0{self.newline();}}}self.y+=4.0;},
            "table"=>self.table(node),
            "quote"=>{self.block_start();let old=self.x0;self.x0+=8.0;self.width=(self.width-8.0).max(20.0);self.x=self.x0;for c in children(node){self.block(c);}self.x0=old;self.width+=8.0;self.x=self.x0;},
            "code_block"=>{self.block_start();let mut ink=Ink::default();ink.code=true;for l in string(node,"text").lines(){self.text(l,&ink,1.0);self.newline();}self.y+=4.0;},
            "rule"=>{self.block_start();self.ops.push(Op::Fill{rect:[self.x0,self.y+3.0,self.x0+self.width,self.y+4.0],color:LINE});self.y+=9.0;},
            "element"=>match string(node,"name") {
                "GameScene"=>self.scene(node),"RecipeFor"|"Recipe"=>self.recipe(node),
                "SubPages"=> {let mut pages:Vec<_>=self.guide.pages.iter().filter(|(_,p)|p.parent.as_deref()==Some(&self.guide.current)).collect();pages.sort_by_key(|(_,p)|(p.position,p.title.to_lowercase()));for (id,p) in pages {self.page_link(id,&p.title);}},
                "CategoryIndex"=> {if let Some(ids)=self.guide.categories.get(attr(node,"category")){for id in ids {if let Some(p)=self.guide.pages.get(id){self.page_link(id,&p.title);}}}},
                "Row"=>self.row(node),
                "Column"|"ItemGrid"|"div"=>for c in children(node){self.block_or_inline(c);},
                "a"=> {let a=string(node,"anchor");if !a.is_empty(){self.anchors.insert(a.to_owned(),self.y);}self.inline(node,&Ink::default());},
                _=>self.inline(node,&Ink::default()),
            },
            _=>self.inline(node,&Ink::default()),
        }
    }
    fn block_or_inline(&mut self,node:&Value) {match string(node,"type") {"paragraph"|"heading"|"list"|"table"|"quote"|"rule"|"code_block"=>self.block(node),"element" if matches!(string(node,"name"),"GameScene"|"Recipe"|"RecipeFor"|"Row"|"Column"|"SubPages"|"CategoryIndex"|"div")=>self.block(node),_=>self.inline(node,&Ink::default())}}
    fn page_link(&mut self,id:&str,title:&str){self.block_start();let ink=Ink{action:Some(Action::Page(id.to_owned(),None)),..Ink::default()};self.text(&format!("• {title}"),&ink,1.0);self.newline();}
    fn row(&mut self,node:&Value) {
        self.block_start();let items=children(node);if items.is_empty(){return;}
        let gap=attr(node,"gap").parse::<f32>().unwrap_or(4.0).clamp(-16.0,30.0);
        let old_x=self.x0;let old_width=self.width;let start=self.y;
        let w=((old_width-gap*(items.len().saturating_sub(1) as f32))/items.len() as f32).max(30.0);let mut bottom=start;
        for (i,c) in items.iter().enumerate() {self.x0=old_x+i as f32*(w+gap);self.width=w;self.x=self.x0;self.y=start;self.line_height=ROW;self.block_or_inline(c);if self.x>self.x0{self.newline();}bottom=bottom.max(self.y);}
        self.x0=old_x;self.width=old_width;self.x=old_x;self.y=bottom+3.0;self.line_height=ROW;
    }
    fn table(&mut self,node:&Value) {
        self.block_start();let Some(header)=node["header"].as_array() else{return;};let cols=header.len().max(1);let cw=self.width/cols as f32;
        let mut rows=vec![header.clone()];if let Some(a)=node["rows"].as_array(){for row in a {if let Some(cells)=row.as_array(){rows.push(cells.clone());}}}
        let original_x=self.x0;let original_w=self.width;
        for (ri,cells) in rows.iter().enumerate(){let top=self.y;let mut bottom=top+ROW;
            if ri==0 {self.ops.push(Op::Fill{rect:[original_x,top,original_x+original_w,top+ROW],color:0xffdfd2bd});}
            for (i,cell) in cells.iter().enumerate().take(cols){self.x0=original_x+i as f32*cw+2.0;self.width=(cw-4.0).max(8.0);self.x=self.x0;self.y=top;self.line_height=ROW;let ink=Ink{bold:ri==0,..Ink::default()};if let Some(a)=cell.as_array(){for c in a {self.inline(c,&ink);}}if self.x>self.x0{self.newline();}bottom=bottom.max(self.y);}
            self.x0=original_x;self.width=original_w;self.x=original_x;self.y=bottom+1.0;self.line_height=ROW;
        }self.y+=4.0;
    }
}
fn target(node:&Value)->Option<Action> {let t=&node["target"];match string(t,"kind") {"page"=>Some(Action::Page(string(t,"page").to_owned(),t["anchor"].as_str().map(str::to_owned))),"external"=>Some(Action::External(string(node,"href").to_owned())),_=>None}}
fn recipe_displays(game:&Game<'_>,node:&Value)->Vec<(String,Display,ItemStack)> {
    let id=string(node,"resource_id");let by_output=string(node,"name")=="RecipeFor";
        let mut recipes:Vec<(String,Display,ItemStack)>=Vec::new();
        for (name,r) in &game.res.recipes().crafting {
            let exact=normalize_id(name)==id;
            match r {
                Recipe::Shaped{width,height,cells,result}=>{let out=result.create(); if (by_output && out.item.as_ref()==id)||(!by_output&&exact){recipes.push((name.clone(),Display::Shaped{width:*width,height:*height,cells:cells.clone()},out));}},
                Recipe::Shapeless{ingredients,result,..}=>{let out=result.create(); if (by_output&&out.item.as_ref()==id)||(!by_output&&exact){recipes.push((name.clone(),Display::Shapeless(ingredients.clone()),out));}},
                Recipe::Special(s)=>for display in s.book_displays() {if (by_output&&display.result.item.as_ref()==id)||(!by_output&&exact){recipes.push((name.clone(),display.display,display.result));}},
            }
        }
        for r in &game.res.recipes().cooking {if (by_output&&r.result.item.as_ref()==id)||(!by_output&&normalize_id(&r.id)==id){recipes.push((r.id.clone(),Display::Furnace(r.ingredient.clone()),r.result.clone()));}}
        // Custom recipe data is shown from the loaded registry, retaining its actual inputs and output.
        for (kind,entries) in &game.res.recipes().ae2_custom {
            for (name,v) in entries {
                let out=custom_result(v);
                if (by_output&&out.as_deref()==Some(id))||(!by_output&&normalize_id(name)==id) {
                    let result=out.unwrap_or_else(||id.to_owned());
                    let result_stack=v.get("result").and_then(horizon_client::crafting::Template::parse).map(|t|t.create()).unwrap_or_else(||stack(&result));
                    let (label,display)=if kind=="ae2:inscriber" {
                        let slots=&v["ingredients"];
                        let cells=["top","middle","bottom"].into_iter().map(|key|slots.get(key).and_then(|v|custom_ingredient(game,v))).collect();
                        ("Inscriber".to_owned(),Display::Shaped{width:1,height:3,cells})
                    } else {
                        let label=match kind.as_str(){
                            "ae2:charger"=>"Charger".to_owned(),
                            "ae2:transform"=>match v["circumstance"]["type"].as_str().unwrap_or("fluid") {
                                "explosion"=>"Transform: Explosion".to_owned(),
                                "fluid"=>format!("Transform: {}",v["circumstance"]["tag"].as_str().unwrap_or("minecraft:water")),
                                other=>format!("Transform: {other}"),
                            },
                            other=>other.to_owned(),
                        };
                        (label,Display::Shapeless(custom_inputs(game,v)))
                    };
                    recipes.push((format!("{label} | {name}"),display,result_stack));
                }
            }
        }
    recipes.sort_by(|a,b|a.0.cmp(&b.0));
    recipes
}
fn recipe_nodes(node:&Value,out:&mut Vec<Value>) {
    if matches!(string(node,"name"),"Recipe"|"RecipeFor") {out.push(node.clone());}
    for child in children(node){recipe_nodes(child,out);}
    if let Some(items)=node.get("items").and_then(Value::as_array){for child in items {recipe_nodes(child,out);}}
}
fn custom_result(v:&Value)->Option<String>{
    fn find(v:&Value)->Option<String>{ match v {Value::String(s) if s.contains(':')=>Some(s.clone()),Value::Object(m)=>m.get("id").or_else(||m.get("item")).and_then(find).or_else(||m.get("block").and_then(find)),Value::Array(a)=>a.iter().find_map(find),_=>None} }
    v.get("result").or_else(||v.get("output")).or_else(||v.get("outputs")).and_then(find)
}
fn custom_ingredient(game:&Game<'_>,v:&Value)->Option<Ingredient>{
    let mut ids=Vec::new();
    fn add(game:&Game<'_>,v:&Value,ids:&mut Vec<String>){
        match v {
            Value::String(s)=>{if let Some(tag)=s.strip_prefix('#'){if let Some(items)=game.res.items().item_tag_order.get(tag){for id in items {if !ids.contains(id){ids.push(id.clone());}}}}else if s.contains(':')&&!ids.contains(s){ids.push(s.clone());}},
            Value::Array(a)=>for x in a{add(game,x,ids);},
            Value::Object(m)=>{if let Some(item)=m.get("item"){add(game,item,ids);}if let Some(tag)=m.get("tag").and_then(Value::as_str){add(game,&Value::String(format!("#{tag}")),ids);}},
            _=>()
        }
    }
    add(game,v,&mut ids);(!ids.is_empty()).then(||Ingredient::of(ids))
}
fn custom_inputs(game:&Game<'_>,v:&Value)->Vec<Ingredient>{
    let mut out=Vec::new();
    for key in ["input","inputs","ingredient","ingredients","top","middle","bottom"] {
        if let Some(value)=v.get(key) {
            if let Some(array)=value.as_array() {for entry in array {if let Some(i)=custom_ingredient(game,entry){out.push(i);}}}
            else if let Some(i)=custom_ingredient(game,value) {out.push(i);}
        }
    }
    out.truncate(9);out
}

fn open_external(url:&str)->Result<(),String> {
    if !(url.starts_with("https://")||url.starts_with("http://")) || url.chars().any(char::is_control) {return Err("Unsupported external link".into());}
    let program=if cfg!(target_os="windows") {"explorer"} else if cfg!(target_os="macos") {"open"} else {"xdg-open"};
    std::process::Command::new(program).arg(url).spawn().map(|_|()).map_err(|e|format!("Could not open link: {e}"))
}
fn nested_scenes<'a>(nodes:&'a [Value],out:&mut Vec<&'a Value>) {
    for n in nodes {
        if string(n,"name")=="GameScene" {out.push(n);} else {nested_scenes(children(n),out);}
    }
}
fn translated(mut r:Rect,dy:f32)->Rect {r[1]+=dy;r[3]+=dy;r}
fn fully_inside(r:Rect,c:Rect)->bool {r[0]>=c[0]&&r[1]>=c[1]&&r[2]<=c[2]&&r[3]<=c[3]}

impl Game<'_> {
    pub(super) fn ae2_guide_draw(&mut self,batch:&mut HudBatch) {
        let Some(mut state)=self.ae2_guide.take() else{return;};
        let (sw,sh)=self.gui_size();
        let w=(sw-18.0).clamp(300.0,920.0);let h=(sh-18.0).clamp(220.0,650.0);
        let x=(sw-w)*0.5;let y=(sh-h)*0.5;
        let sidebar=(w*0.27).clamp(92.0,190.0);
        let doc=[x+sidebar+14.0,y+37.0,x+w-12.0,y+h-13.0];
        let side=[x+8.0,y+48.0,x+sidebar-3.0,y+h-12.0];
        state.doc_clip=doc;state.side_clip=side;state.hits.clear();state.visible.clear();
        {
            let atlas=&self.gui.atlas;
            batch.fill(atlas,0.0,0.0,sw,sh,0xb0000000);
            batch.fill(atlas,x,y,w,h,0xffbfae96);
            batch.fill(atlas,x+2.0,y+2.0,w-4.0,h-4.0,PAPER);
            batch.fill(atlas,x+sidebar+5.0,y+7.0,1.0,h-14.0,LINE);
            batch.text(atlas,"AE2 GUIDE",x+10.0,y+9.0,INK,false);
            let nav=[("‹",Action::Back),("›",Action::Forward),("⌂",Action::Home)];
            for (i,(label,action)) in nav.into_iter().enumerate(){let px=x+sidebar+13.0+i as f32*19.0;let r=[px,y+8.0,px+16.0,y+25.0];batch.fill(atlas,r[0],r[1],16.0,17.0,0xffded1bc);batch.text(atlas,label,px+4.0,y+12.0,INK,false);state.hits.push(Hit{rect:r,action,tip:None});}
            if let Some(page)=state.pages.get(&state.current) {let start=batch.vertices.len();batch.text(atlas,&page.title,x+sidebar+75.0,y+12.0,INK,false);batch.clip_since(start,x+sidebar+75.0,y+7.0,x+w-10.0,y+26.0);}
            state.search.x=x+8.0;state.search.y=y+25.0;state.search.width=sidebar-12.0;state.search.height=18.0;state.search.draw(batch,atlas,None);
            if state.search.value.is_empty()&&!state.search.focused {batch.text(atlas,"Search",state.search.x+4.0,state.search.y+5.0,0xffa6a6a6,false);}
            let query=state.search.value.trim().to_lowercase();
            let ordered=state.ordered_pages();let visible:Vec<_>=ordered.into_iter().filter(|(id,_)|query.is_empty()||state.pages.get(id).is_some_and(|p|p.search.contains(&query))).collect();
            let content=visible.len() as f32*15.0;state.side_max=(content-(side[3]-side[1])).max(0.0);state.side_scroll=state.side_scroll.clamp(0.0,state.side_max);
            for (i,(id,depth)) in visible.iter().enumerate(){let yy=side[1]+i as f32*15.0-state.side_scroll;let rect=[side[0],yy,side[2],yy+14.0];if !intersect(rect,side){continue;}let start=batch.vertices.len();if id==&state.current {batch.fill(atlas,side[0],yy,side[2]-side[0],14.0,0xffe3d4bb);}let indent=if query.is_empty(){(*depth as f32*7.0).min(35.0)}else{0.0};let title=&state.pages[id].title;batch.text(atlas,title,side[0]+3.0+indent,yy+3.0,INK,false);batch.clip_since(start,side[0],side[1],side[2],side[3]);let clipped=[rect[0].max(side[0]),rect[1].max(side[1]),rect[2].min(side[2]),rect[3].min(side[3])];state.visible.push(serde_json::json!({"kind":"sidebar","target":id,"rect":clipped,"title":title}));state.hits.push(Hit{rect:clipped,action:Action::Page(id.clone(),None),tip:Some(title.clone())});}
        }
        let mut needed=Vec::new();
        if let Some(page)=state.pages.get(&state.current){for node in &page.nodes{recipe_nodes(node,&mut needed);}}
        for node in needed {let key=format!("{}|{}",string(&node,"name"),string(&node,"resource_id"));if !state.recipes.contains_key(&key){state.recipes.insert(key,recipe_displays(self,&node));}}
        let (ops,hits,anchors,content_height)= {
            let mut layout=Layout::new(self,&state,doc[0]+3.0,(doc[2]-doc[0]-9.0).max(20.0));
            if let Some(page)=state.pages.get(&state.current) {for node in &page.nodes {layout.block(node);}if layout.x>layout.x0{layout.newline();}}
            (layout.ops,layout.hits,layout.anchors,layout.y+4.0)
        };
        state.scroll_max=(content_height-(doc[3]-doc[1])).max(0.0);
        if let Some(anchor)=state.pending_anchor.take(){if let Some(y)=anchors.get(&anchor){state.scroll=*y;}else{state.status=Some(format!("Section unavailable: {anchor}"));}}
        state.scroll=state.scroll.clamp(0.0,state.scroll_max);
        let shift=doc[1]-state.scroll;
        let mut scene_tip:Option<Vec<Value>>=None;
        let mut zoom_hits=Vec::new();
        for op in ops {
            match op {
                Op::Text{x,y,text,ink,scale}=> {let r=[x,y+shift,x+ink.width(&self.gui.atlas,&text)*scale,y+shift+10.0*scale];if !intersect(r,doc){continue;}let start=batch.vertices.len();let style=ink.style();let text=Text::styled(text,style);let color=if ink.action.is_some(){LINK}else if ink.code {SUBTLE}else{INK};batch.text_styled(&self.gui.atlas,&text,x,y+shift,color,false);if scale!=1.0 {batch.scale_since(start,x,y+shift,scale,scale);}batch.clip_since(start,doc[0],doc[1],doc[2],doc[3]);},
                Op::Recipe{rect,id}=>{let r=translated(rect,shift);if intersect(r,doc){let start=batch.vertices.len();batch.fill(&self.gui.atlas,r[0],r[1],r[2]-r[0],r[3]-r[1],0xffc7dae5);batch.clip_since(start,doc[0],doc[1],doc[2],doc[3]);state.visible.push(serde_json::json!({"kind":"recipe","id":id,"rect":r}));}},
                Op::Fill{rect,color}=> {let r=translated(rect,shift);if intersect(r,doc){let start=batch.vertices.len();batch.fill(&self.gui.atlas,r[0],r[1],r[2]-r[0],r[3]-r[1],color);batch.clip_since(start,doc[0],doc[1],doc[2],doc[3]);}},
                Op::Image{rect,sprite}=>{let r=translated(rect,shift);if intersect(r,doc){let start=batch.vertices.len();batch.sprite_scaled(&self.gui.atlas,&sprite,r[0],r[1],r[2]-r[0],r[3]-r[1],u32::MAX);batch.clip_since(start,doc[0],doc[1],doc[2],doc[3]);let size=self.gui.atlas.sprites.get(&sprite).map(|image|[image.w,image.h]);state.visible.push(serde_json::json!({"kind":"image","sprite":sprite,"rect":r,"source_size":size}));}},
                Op::Item{rect,stack}=>{let r=translated(rect,shift);if fully_inside(r,doc){let start=batch.vertices.len();self.draw_item(batch,&stack,r[0],r[1]);let scale=(r[2]-r[0])/16.0;if scale!=1.0 {batch.scale_since(start,r[0],r[1],scale,scale);}}},
                Op::BlockImage{rect,node}=>{let r=translated(rect,shift);if intersect(r,doc){let result=self.ae2_guide_block_image(batch,&node,r,doc);let error=result.err();if error.is_some()&&fully_inside(r,doc){let start=batch.vertices.len();self.draw_item(batch,&stack(string(&node,"resource_id")),r[0],r[1]);let scale=(r[2]-r[0])/16.0;if scale!=1.0{batch.scale_since(start,r[0],r[1],scale,scale);}}state.visible.push(serde_json::json!({"kind":"block_image","id":string(&node,"resource_id"),"attrs":node["attrs"],"rect":r,"rendered":error.is_none(),"error":error}));}},
                Op::Scene{rect,mut node}=>{let r=translated(rect,shift);if intersect(r,doc){let id=string(&node,"scene_id").to_owned();let orbit=state.orbits.get(&id).copied().unwrap_or_default();let zoom=state.zoom.get(&id).copied().unwrap_or(1.0);let base=attr(&node,"zoom").parse::<f32>().unwrap_or(1.0);if let Some(attrs)=node.get_mut("attrs").and_then(Value::as_object_mut){attrs.insert("zoom".into(),Value::String((base*zoom).to_string()));}let cursor=self.cursor;let pointer=if inside(r,cursor)&&inside(doc,cursor){Some([cursor.0,cursor.1])}else{None};match self.ae2_guide_scene(batch,&node,r,doc,orbit,pointer){Ok(v)=>{state.visible.push(serde_json::json!({"kind":"scene","id":v.scene_id,"rect":r,"interactive":v.interactive,"blocks":v.blocks,"multipart_parts":v.multipart_parts,"diagnostics":v.diagnostics,"yaw":orbit.yaw,"pitch":orbit.pitch,"pan":orbit.pan,"zoom":zoom}));if v.interactive{for (i,(label,factor)) in [("+",1.25),("−",0.8)].into_iter().enumerate(){let x=r[2]-20.0;let y=r[1]+4.0+i as f32*18.0;let b=[x,y,x+16.0,y+16.0];if intersect(b,doc){let start=batch.vertices.len();batch.fill(&self.gui.atlas,x,y,16.0,16.0,0xffc7dae5);batch.text(&self.gui.atlas,label,x+4.0,y+4.0,INK,false);batch.clip_since(start,doc[0],doc[1],doc[2],doc[3]);let clipped=[b[0].max(doc[0]),b[1].max(doc[1]),b[2].min(doc[2]),b[3].min(doc[3])];zoom_hits.push(Hit{rect:clipped,action:Action::Zoom(id.clone(),factor),tip:Some(if i==0{"Zoom in"}else{"Zoom out"}.to_owned())});state.visible.push(serde_json::json!({"kind":"scene_zoom","id":id,"direction":if i==0{"in"}else{"out"},"rect":clipped}));}}}if let Some(t)=v.tooltip{scene_tip=Some(t);}},Err(e)=>{let start=batch.vertices.len();batch.text(&self.gui.atlas,&format!("Scene unavailable: {e}"),r[0]+4.0,r[1]+4.0,SUBTLE,false);batch.clip_since(start,doc[0],doc[1],doc[2],doc[3]);}}}},
            }
        }
        for mut hit in hits {hit.rect=translated(hit.rect,shift);if intersect(hit.rect,doc){hit.rect=[hit.rect[0].max(doc[0]),hit.rect[1].max(doc[1]),hit.rect[2].min(doc[2]),hit.rect[3].min(doc[3])];let (kind,target)=match &hit.action{Action::Page(id,_) => ("link",id.as_str()),Action::Item(id)=>("item",id.as_str()),Action::External(id)=>("external",id.as_str()),Action::Scene(id)=>("scene_hit",id.as_str()),_ => ("control","")};state.visible.push(serde_json::json!({"kind":kind,"target":target,"rect":hit.rect}));state.hits.push(hit);}}
        state.hits.extend(zoom_hits);
        if let Some(status)=&state.status {let start=batch.vertices.len();batch.text(&self.gui.atlas,status,x+sidebar+14.0,y+h-10.0,SUBTLE,false);batch.clip_since(start,doc[0],y+h-11.0,doc[2],y+h-1.0);}
        if let Some(nodes)=scene_tip {
            let text=plain(&Value::Array(nodes.clone()));let mut lines=Vec::new();let mut line=String::new();for word in text.split_whitespace(){if line.len()+word.len()>52&&!line.is_empty(){lines.push(Text::literal(std::mem::take(&mut line)));}if !line.is_empty(){line.push(' ');}line.push_str(word);}if !line.is_empty(){lines.push(Text::literal(line));}ui::tooltip(batch,&self.gui.atlas,&lines,self.cursor);
            let mut nested=Vec::new();nested_scenes(&nodes,&mut nested);
            let count=nested.len();let width=110.0;let height=80.0;
            let x=(self.cursor.0+14.0).min((sw-width*count as f32).max(0.0)).max(0.0);
            let y=(self.cursor.1+16.0+lines.len() as f32*10.0).min((sh-height).max(0.0)).max(0.0);
            for (i,scene) in nested.into_iter().enumerate(){let xx=x+i as f32*width;let rect=[xx,y,xx+width,y+height];let _=self.ae2_guide_scene(batch,scene,rect,[0.0,0.0,sw,sh],GuideSceneOrbit::default(),None);}
        } else if let Some(hit)=state.hits.iter().rev().find(|h|inside(h.rect,self.cursor)&&h.tip.is_some()) {ui::tooltip(batch,&self.gui.atlas,&[Text::literal(hit.tip.clone().unwrap_or_default())],self.cursor);}
        self.ae2_guide=Some(state);
    }
    pub(super) fn ae2_guide_key(&mut self,key:KeyCode)->bool {
        let ctrl=self.has_control_down() || (cfg!(target_os="macos") && self.modifiers.super_key());let shift=self.has_shift_down();
        let Some(state)=self.ae2_guide.as_mut() else{return false;};
        if key==KeyCode::Escape {if state.search.focused {state.search.focused=false;return true;}return false;}
        if ctrl&&key==KeyCode::KeyF {state.search.focused=true;return true;}
        if state.search.focused {if key==KeyCode::Enter {state.search.focused=false;return true;}let handled=state.search.key(key,ctrl,shift);if handled{state.side_scroll=0.0;}return handled;}
        match key {
            KeyCode::Backspace|KeyCode::ArrowLeft=>{state.go_history(-1);true},
            KeyCode::ArrowRight=>{state.go_history(1);true},
            KeyCode::ArrowUp=>{state.scroll=(state.scroll-24.0).max(0.0);true},
            KeyCode::ArrowDown=>{state.scroll=(state.scroll+24.0).min(state.scroll_max);true},
            KeyCode::PageUp=>{state.scroll=(state.scroll-(state.doc_clip[3]-state.doc_clip[1])*0.85).max(0.0);true},
            KeyCode::PageDown=>{state.scroll=(state.scroll+(state.doc_clip[3]-state.doc_clip[1])*0.85).min(state.scroll_max);true},
            KeyCode::Home=>{state.scroll=0.0;true},KeyCode::End=>{state.scroll=state.scroll_max;true},_=>false
        }
    }
    pub(super) fn ae2_guide_text(&mut self,text:&str) {if let Some(s)=self.ae2_guide.as_mut(){if s.search.focused{s.search.insert_text(text);s.side_scroll=0.0;}}}
    pub(super) fn ae2_guide_click(&mut self,button:MouseButton) {
        if !matches!(button,MouseButton::Left|MouseButton::Right) {return;}let cursor=self.cursor;let shift=self.has_shift_down();
        let Some(mut state)=self.ae2_guide.take() else{return;};
        let search=[state.search.x,state.search.y,state.search.x+state.search.width,state.search.y+state.search.height];
        if button==MouseButton::Right {
            if let Some(hit)=state.hits.iter().rev().find(|h|inside(h.rect,cursor)&&matches!(h.action,Action::Scene(_))) {
                if let Action::Scene(id)=&hit.action{state.dragging=Some((id.clone(),cursor,button));}
            }
        }else if inside(search,cursor){state.search.focused=true;state.search.click(&self.gui.atlas,cursor.0,shift,false);}else{
            state.search.focused=false;
            if let Some(hit)=state.hits.iter().rev().find(|h|inside(h.rect,cursor)).cloned(){
                match hit.action {Action::External(url)=>{state.status=open_external(&url).err();},Action::Scene(id)=>state.dragging=Some((id,cursor,button)),action=>state.action(action)}
            }
        }
        self.ae2_guide=Some(state);
    }
    pub(super) fn ae2_guide_scroll(&mut self,delta:f32) {let cursor=self.cursor;if let Some(s)=self.ae2_guide.as_mut(){if inside(s.side_clip,cursor){s.side_scroll=(s.side_scroll-delta*22.0).clamp(0.0,s.side_max);}else if inside(s.doc_clip,cursor){s.scroll=(s.scroll-delta*22.0).clamp(0.0,s.scroll_max);}}}
    pub(super) fn ae2_guide_pointer(&mut self,pointer:(f32,f32),left_down:bool,right_down:bool) {
        if let Some(s)=self.ae2_guide.as_mut(){
            if s.dragging.as_ref().is_some_and(|(_,_,button)|match button{MouseButton::Left=>!left_down,MouseButton::Right=>!right_down,_=>true}) {s.dragging=None;}
            if let Some((id,last,button))=&mut s.dragging {
                let orbit=s.orbits.entry(id.clone()).or_default();let delta=(pointer.0-last.0,pointer.1-last.1);
                if *button==MouseButton::Right{orbit.translate(delta.0,delta.1);}else{orbit.drag(delta.0,delta.1);}
                *last=pointer;
            }
        }
    }
    pub(super) fn ae2_guide_snapshot(&self)->Value {
        let Some(s)=self.ae2_guide.as_ref() else{return Value::Null;};
        serde_json::json!({"page":s.current,"title":s.pages.get(&s.current).map(|p|p.title.as_str()),"search":s.search.value,"search_focused":s.search.focused,"search_rect":[s.search.x,s.search.y,s.search.x+s.search.width,s.search.y+s.search.height],"scroll":s.scroll,"scroll_max":s.scroll_max,"sidebar_scroll":s.side_scroll,"sidebar_scroll_max":s.side_max,"doc_clip":s.doc_clip,"side_clip":s.side_clip,"visible":s.visible,"history_index":s.history_index,"history_len":s.history.len(),"status":s.status})
    }
    pub(super) fn ae2_guide_close(&mut self) {self.ae2_guide=None;self.ae2_guide_left_down=false;self.ae2_guide_right_down=false;if self.screen==Screen::Mod {self.screen=Screen::Playing;if self.player.is_some(){self.capture(true);}}}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn guide_reader_loads_navigates_searches_and_restores_history() {
        let dir=std::env::temp_dir().join(format!("horizon-guide-reader-{}",std::process::id()));
        let pack=dir.join("assets/ae2/ae2guide");std::fs::create_dir_all(&pack).unwrap();
        let data=serde_json::json!({"version":1,"guide_id":"ae2:guide","root":"index.md","pages":{
            "index.md":{"id":"index.md","navigation":{"title":"Guide"},"nodes":[{"type":"paragraph","children":[{"type":"text","text":"welcome"}]}]},
            "items/tool.md":{"id":"items/tool.md","navigation":{"title":"Network Tool","parent":"index.md"},"nodes":[{"type":"heading","anchor":"controls","children":[{"type":"text","text":"Useful controls"}]}]}
        },"item_index":{"ae2:network_tool":["items/tool.md"]},"categories":{}});
        std::fs::write(pack.join("compiled-guide.json"),serde_json::to_vec(&data).unwrap()).unwrap();
        let mut state=GuideState::load(&dir).unwrap();assert!(state.pages["items/tool.md"].search.contains("useful controls"));
        state.scroll=23.0;state.action(Action::Item("ae2:network_tool".into()));assert_eq!(state.current,"items/tool.md");
        state.navigate("items/tool.md",Some("controls".into()));assert_eq!(state.pending_anchor.as_deref(),Some("controls"));
        state.go_history(-1);state.go_history(-1);assert_eq!(state.current,"index.md");assert_eq!(state.scroll,23.0);
        state.go_history(1);assert_eq!(state.current,"items/tool.md");
        assert!(!safe_asset("../outside.png"));assert!(!safe_asset("/absolute.png"));assert!(safe_asset("assets/logo.png"));
        let _=std::fs::remove_dir_all(dir);
    }
}
