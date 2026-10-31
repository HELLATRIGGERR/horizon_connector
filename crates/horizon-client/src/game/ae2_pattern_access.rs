//! Face-owned Pattern Access Terminal. Provider slots always remain physical.
use super::{Game,lan::LanHost,ae2_global::GridAddress,ae2_parts::PartAddress};
use horizon_client::{ae2_world::{Machine,MachineKind},containers::BlockEntity,items::ItemStack,menu::{Input,Kind,Menu}};
use horizon_protocol::packets::play::{self,Clientbound as C};
use horizon_world::nbt::Tag;
use horizon_core::geometry::Direction;
use std::collections::BTreeSet;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum Owner {Block(GridAddress),Part(PartAddress)}
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct OwnerIdentity { owner:Owner, host:u64, part:Option<[i32;4]> }
#[derive(Clone,Debug,PartialEq)]
struct Row {owner:Owner,name:String,group:Group,sort:i64,slots:Vec<Option<ItemStack>>}
/// Group identity includes the icon components and tooltip, not only the
/// display name: two different machines can legitimately have the same name.
#[derive(Clone,Debug,PartialEq,Eq,PartialOrd,Ord)]
struct Group {icon:ae2_cell_core::ItemKey,name:String,tooltip:Vec<String>}
fn same_sent_owner(sent:Option<OwnerIdentity>,current:Option<OwnerIdentity>)->bool {
    sent.is_some() && sent==current
}
#[derive(Clone,Debug,PartialEq)]
enum DisplayLine { Header(Group,usize), Provider(Row) }
fn display_lines(rows:&[Row])->Vec<DisplayLine> {
    let mut lines=Vec::new();let mut i=0;
    while i<rows.len() {
        let mut end=i+1;while end<rows.len() && rows[end].group==rows[i].group {end+=1;}
        lines.push(DisplayLine::Header(rows[i].group.clone(),end-i));
        lines.extend(rows[i..end].iter().cloned().map(DisplayLine::Provider));i=end;
    }
    lines
}
struct AccessView {
    rows:Vec<Row>, pinned:Vec<Owner>, page:usize, items:Vec<Option<ItemStack>>,
    layout:Vec<serde_json::Value>, owners:Vec<Option<OwnerIdentity>>, max_scroll:usize,
}
pub(super) struct LocalPatternAccess {
    pos:(i32,i32,i32),face:String,dim:String,terminal:OwnerIdentity,filter:u8,
    view:AccessView,field:horizon_client::ui::EditBox,menu:Menu,refresh:u8,
}
pub(super) struct GuestPatternAccess {
    window:i32,state:i32,pos:(i32,i32,i32),face:String,dim:String,page:usize,terminal:OwnerIdentity,
    search:String,filter:u8,rows:Vec<Row>,pinned:Vec<Owner>,menu:Menu,refresh:u8,
    sent_views:std::collections::VecDeque<(i32,Vec<Option<OwnerIdentity>>)>,
}
fn pattern(s:&ItemStack)->bool {
    super::ae2_processing::decode_pattern(s).is_some() || super::ae2_processing::ae2_decode_processing_pattern(s).is_some() || super::ae2_processing::ae2_decode_special_pattern(s).is_some()
}
/// PatternDetailsHelper.isEncodedPattern checks a registered encoded item
/// type without decoding its payload. Invalid patterns remain recoverable
/// physical items even though they contribute no crafting definition.
pub(super) fn encoded_pattern_item(s:&ItemStack)->bool {
    s.count>0 && matches!(s.item.as_ref(),"ae2:crafting_pattern"|"ae2:processing_pattern"|"ae2:smithing_table_pattern"|"ae2:stonecutting_pattern")
}
impl Game<'_> {
    fn access_filter(&self,pos:(i32,i32,i32),face:&str)->Option<u8> {
        let BlockEntity::Ae2(host)=self.world.block_entities.get(&pos)? else{return None};
        let Tag::List(parts)=host.config.get(&"parts".into())? else{return None};
        let mut found=parts.iter().filter(|p|p.get("face")==Some(&Tag::String(face.into())));
        let part=found.next()?;if found.next().is_some() || part.get("id")!=Some(&Tag::String("ae2:pattern_access_terminal".into())){return None;}
        match part.get("ae2_access_filter") {None=>Some(0),Some(Tag::Byte(n)) if (0..=2).contains(n)=>Some(*n as u8),_=>None}
    }
    fn access_set_filter(&mut self,pos:(i32,i32,i32),face:&str,filter:u8)->bool {
        if filter>2 || self.access_filter(pos,face).is_none(){return false;}
        let Some(BlockEntity::Ae2(host))=self.world.block_entities.get_mut(&pos) else{return false};
        let Some(Tag::List(parts))=host.config.get_mut(&"parts".into()) else{return false};
        let Some(Tag::Compound(part))=parts.iter_mut().find(|p|p.get("face")==Some(&Tag::String(face.into()))) else{return false};
        part.insert("ae2_access_filter".into(),Tag::Byte(filter as i8));
        self.world.modified.insert((pos.0>>4,pos.2>>4));true
    }
    fn access_pattern_matches(&self,pattern:&ItemStack,query:&str)->bool {
        let mut outputs=Vec::new();
        if let Some((_,output))=super::ae2_processing::decode_pattern(pattern){outputs.push(output);}
        if let Some(special)=super::ae2_processing::ae2_decode_special_pattern(pattern){outputs.push(special.output);}
        if let Some((_,items))=super::ae2_processing::ae2_decode_processing_pattern(pattern){outputs.extend(items.into_iter().flatten());}
        if outputs.iter().any(|item|{
            let name=horizon_client::stations::hover_name(&|id|self.pack.names.display_item(id),item);
            name.to_lowercase().contains(query) || item.item.to_lowercase().contains(query)
        }) {return true;}
        super::ae2_processing::ae2_decode_processing_fluids(pattern).is_some_and(|(_,fluids)|
            fluids.iter().any(|fluid|fluid.key.item_id.to_lowercase().contains(query)))
    }
    fn access_owner_identity(&self,owner:Owner)->Option<OwnerIdentity> {
        let address=match owner {Owner::Block(p)=>p,Owner::Part(p)=>p.host};
        let (dim,pos)=self.ae2_address_parts(address)?;
        let world=self.ae2_world_for(dim)?;
        let host=world.block_entities.incarnation(&pos)?;
        let part=match owner {
            Owner::Block(_)=>None,
            Owner::Part(p)=>{
                let BlockEntity::Ae2(machine)=world.block_entities.get(&pos)? else{return None};
                let Tag::List(parts)=machine.config.get(&"parts".into())? else{return None};
                let face=["east","west","up","down","south","north"].get(usize::from(p.face))?;
                let mut found=parts.iter().filter(|part|part.get("face")==Some(&Tag::String((*face).into())));
                let record=found.next()?;if found.next().is_some(){return None;}
                Some(super::ae2_channels::part_identity(record)?)
            }
        };
        Some(OwnerIdentity{owner,host,part})
    }
    /// A menu belongs to the original host and face-part incarnation, even
    /// if another Pattern Access terminal is installed at the same address.
    fn access_terminal_identity(&self,pos:(i32,i32,i32),face:&str)->Option<OwnerIdentity> {
        self.access_filter(pos,face)?;
        let face=["east","west","up","down","south","north"].iter().position(|name|*name==face)? as u8;
        let host=self.ae2_local_address(pos)?;
        self.access_owner_identity(Owner::Part(PartAddress{host,face}))
    }
    fn access_machine(&self,owner:Owner)->Option<Machine> {
        match owner {
            Owner::Block(p)=>match self.ae2_block_entity(p)? {BlockEntity::Ae2(m)=>Some((**m).clone()),_=>None},
            Owner::Part(p)=>self.ae2_part_machine(p).ok().flatten(),
        }.filter(|m|m.kind==MachineKind::PatternProvider && m.slots.len()==9)
    }
    fn access_name(&self,owner:Owner)->Option<String> {
        match owner {
            Owner::Block(address)=>{
                let (dim,pos)=self.ae2_address_parts(address)?;
                self.ae2_world_for(dim)?.block_names.get(&pos).cloned()
            }
            Owner::Part(part)=>{
                let BlockEntity::Ae2(host)=self.ae2_block_entity(part.host)? else{return None};
                let Tag::List(parts)=host.config.get(&"parts".into())? else{return None};
                let face=["east","west","up","down","south","north"].get(usize::from(part.face))?;
                let part=parts.iter().find(|p|p.get("face")==Some(&Tag::String((*face).into())))?;
                let stack=horizon_client::save::item_from_nbt(part.get("stack")?)?;
                stack.custom_name()
            }
        }
    }
    /// AE2 groups an unnamed provider by its one distinct adjacent machine,
    /// using sided capability presence even for empty handlers.
    fn access_group(&self,owner:Owner,component:usize,network:&ae2_network_core::Network,analysis:&ae2_network_core::Analysis)->Option<Group> {
        let host_icon=match owner {
            Owner::Block(_) => ItemStack::new("ae2:pattern_provider",1),
            Owner::Part(p) => {
                let BlockEntity::Ae2(host)=self.ae2_block_entity(p.host)? else{return None};
                let Tag::List(parts)=host.config.get(&"parts".into())? else{return None};
                let face=["east","west","up","down","south","north"].get(usize::from(p.face))?;
                let part=parts.iter().find(|part|part.get("face")==Some(&Tag::String((*face).into())))?;
                horizon_client::save::item_from_nbt(part.get("stack")?)?
            }
        };
        let host_key=horizon_client::ae2_cell_codec::key(&host_icon).ok()?;
        if let Some(name)=self.access_name(owner){return Some(Group{icon:host_key,name,tooltip:Vec::new()});}
        let address=match owner {Owner::Block(p)=>p,Owner::Part(p)=>p.host};
        let Some((dim,pos))=self.ae2_address_parts(address) else{return None};
        let directions:Vec<Direction>=match owner {
            Owner::Block(address)=>{
                let Some(state)=self.ae2_grid_block(address).and_then(|id|self.pack.assets.registry.state(id)) else{return None};
                match super::ae2_autocrafting::provider_push_direction(state) {Ok(Some(direction))=>vec![direction],Ok(None)=>Direction::ALL.to_vec(),Err(())=>return None}
            },
            Owner::Part(p)=>vec![[Direction::East,Direction::West,Direction::Up,Direction::Down,Direction::South,Direction::North][usize::from(p.face)]],
        };
        let mut groups=BTreeSet::new();
        for direction in directions {
            let delta=direction.normal();
            let Some(x)=pos.0.checked_add(delta[0]) else{continue};
            let Some(y)=pos.1.checked_add(delta[1]) else{continue};
            let Some(z)=pos.2.checked_add(delta[2]) else{continue};
            let Some(target)=self.ae2_dimension_address(dim,(x,y,z)) else{continue};
            let side=direction.opposite();
            let same_grid=network.physical_connection(address.network_pos(),target.network_pos())
                && analysis.nodes.get(&target.network_pos()).is_some_and(|s|s.component==component);
            if same_grid && (self.ae2_interface_supports(target,side) || self.ae2_provider_part_at(target,side).is_some()
                || matches!(self.ae2_block_entity(target),Some(BlockEntity::Ae2(m)) if m.kind==MachineKind::PatternProvider)) {continue;}
            let assembler=matches!(self.ae2_block_entity(target),Some(BlockEntity::Ae2(m)) if m.kind==MachineKind::MolecularAssembler);
            if !assembler && !self.ae2_native_item_supports(target,side) && !self.ae2_native_fluid_supports(target,side){continue;}
            let Some(block)=self.ae2_grid_block(target) else{continue};
            let id=self.res.facts().get(block).name.to_string();
            let mut label=self.ae2_world_for(dim).and_then(|world|world.block_names.get(&(x,y,z)).cloned())
                .unwrap_or_else(||self.pack.names.display_item(&id));
            let mut icon=ItemStack::new(&id,1);
            if let Some(BlockEntity::Ae2(host))=self.ae2_block_entity(target) {
                if host.kind==MachineKind::CableBus {
                    let face=match side {Direction::East=>"east",Direction::West=>"west",Direction::Up=>"up",Direction::Down=>"down",Direction::South=>"south",Direction::North=>"north"};
                    let Some(Tag::List(parts))=host.config.get(&"parts".into()) else{continue};
                    let Some(part)=parts.iter().find(|p|p.get("face")==Some(&Tag::String(face.into()))) else{continue};
                    let Some(stack)=part.get("stack").and_then(horizon_client::save::item_from_nbt) else{continue};
                    icon=stack.clone();label=horizon_client::stations::hover_name(&|id|self.pack.names.display_item(id),&stack);
                }
            }
            if let Ok(icon)=horizon_client::ae2_cell_codec::key(&icon) {groups.insert(Group{icon,name:label,tooltip:Vec::new()});}
        }
        if groups.len()==1 {groups.into_iter().next()} else {
            let tooltip=if groups.len()>1 {groups.into_iter().map(|group|group.name).collect()} else {Vec::new()};
            Some(Group{icon:host_key,name:"Pattern Provider".into(),tooltip})
        }
    }
    fn access_store(&mut self,owner:Owner,machine:&Machine)->Result<(),String> {
        let old_slots=self.access_machine(owner).map(|old|old.slots);
        let result=match owner {
            Owner::Part(p)=>self.ae2_store_part_machine(p,machine),
            Owner::Block(p)=>{
                let Some(BlockEntity::Ae2(target))=self.ae2_block_entity_mut(p) else{return Err("provider disappeared".into());};
                if target.kind!=MachineKind::PatternProvider {return Err("provider changed".into());}
                **target=machine.clone();self.ae2_mark_modified(p);Ok(())
            }
        };
        if result.is_ok() && old_slots.as_ref().is_some_and(|old|old!=&machine.slots) {
            match owner {
                Owner::Block(address)=>self.ae2_provider_patterns_changed(address,None),
                Owner::Part(part)=>self.ae2_provider_patterns_changed(part.host,Some(part.face)),
            }
        }
        result
    }
    fn access_rows(&self,pos:(i32,i32,i32),search:&str,filter:u8,pinned:&[Owner])->(Vec<Row>,Vec<Owner>) {
        let n=self.ae2_network();let Ok(a)=n.analyze() else{return (Vec::new(),Vec::new())};
        let Some(node)=self.ae2_local_address(pos).and_then(|p|a.nodes.get(&p.network_pos())) else{return (Vec::new(),Vec::new())};
        if !node.online{return (Vec::new(),Vec::new());}
        let mut rows=Vec::new();let mut pins=Vec::new();
        for p in &a.components[node.component].positions {
            let Some(address)=GridAddress::from_network(*p) else{continue;};
            let Some(status)=a.nodes.get(p) else{continue;};
            let mut owners=Vec::new();
            if status.online {owners.push(Owner::Block(address));}
            for (&face,part) in &status.parts {if part.online {owners.push(Owner::Part(PartAddress{host:address,face}));}}
            for owner in owners {
                let Some(machine)=self.access_machine(owner) else{continue;};
                let visible=!matches!(machine.config.get(&"terminal_visible".into()),Some(Tag::Byte(0)));
                if filter!=2 && !visible {continue;}
                if filter==1 && machine.slots.iter().all(Option::is_some) && !pinned.contains(&owner){continue;}
                if filter==1 {pins.push(owner);}
                let location=self.ae2_address_parts(address).map(|(dim,p)|if dim==self.dim {format!("{},{},{}",p.0,p.1,p.2)} else {format!("{} {},{},{}",dim.id(),p.0,p.1,p.2)}).unwrap_or_else(||"?".into());
                let Some(group)=self.access_group(owner,node.component,&n,&a) else{continue};
                let name=match owner {Owner::Block(_)=>format!("{} {location}",group.name),Owner::Part(p)=>format!("{} {} {location}",group.name,["E","W","U","D","S","N"][usize::from(p.face)])};
                let query=search.to_lowercase();
                if !query.is_empty() && !group.name.to_lowercase().contains(&query)
                    && !machine.slots.iter().flatten().any(|pattern|self.access_pattern_matches(pattern,&query)) {continue;}
                let sort=self.ae2_address_parts(address).map_or(0,|(_,p)|((i64::from(p.2))<<24)^((i64::from(p.0))<<8)^i64::from(p.1));
                rows.push(Row{owner,name,group,sort,slots:machine.slots});
            }
        }
        rows.sort_by(|a,b|a.group.name.to_lowercase().cmp(&b.group.name.to_lowercase()).then(a.group.cmp(&b.group)).then(a.sort.cmp(&b.sort)));
        (rows,pins)
    }
    /// In-frame access sessions retain the clicked terminal and the exact
    /// provider identities shown to the player.
    pub(super) fn ae2_open_local_pattern_access(&mut self,pos:(i32,i32,i32),face:[i32;3])->bool {
        if self.remote.is_some(){return false;}
        let face=match face {[1,0,0]=>"east",[-1,0,0]=>"west",[0,1,0]=>"up",[0,-1,0]=>"down",[0,0,1]=>"south",[0,0,-1]=>"north",_=>return false};
        let Some(filter)=self.access_filter(pos,face) else{return false};
        let Some(terminal)=self.access_terminal_identity(pos,face) else{return false};
        let view=self.access_view(pos,face,0,"",filter,&[]);
        let mut field=horizon_client::ui::EditBox::new(0.0,0.0,65.0,12.0);field.max_length=128;field.focused=true;
        self.container=Some(pos);self.ae2_local_pattern_access=Some(LocalPatternAccess{pos,face:face.into(),dim:self.dim.id().into(),terminal,filter,view,field,menu:Menu::new(Kind::Chest(6)),refresh:0});
        self.screen=super::Screen::Inventory;self.capture(false);self.right=false;true
    }
    pub(super) fn ae2_local_pattern_access_open(&self)->bool {
        self.remote.is_none() && self.ae2_local_pattern_access.as_ref().is_some_and(|s|self.container==Some(s.pos))
    }
    pub(super) fn ae2_local_pattern_access_valid(&self)->bool {
        self.ae2_local_pattern_access_open() && self.ae2_local_pattern_access.as_ref().is_some_and(|s|
            s.dim==self.dim.id() && self.access_terminal_identity(s.pos,&s.face)==Some(s.terminal)
                && self.eye_position().is_some_and(|eye|horizon_client::lan::guest::within_block_range(eye,self.reach()+4.0,[s.pos.0,s.pos.1,s.pos.2])))
    }
    pub(super) fn ae2_local_pattern_access_refresh(&mut self) {
        if !self.ae2_local_pattern_access_valid(){return;}
        let Some(s)=self.ae2_local_pattern_access.as_ref() else{return};
        let (pos,face,page,search,old_filter,pinned)=(s.pos,s.face.clone(),s.view.page,s.field.value.clone(),s.filter,s.view.pinned.clone());
        let Some(filter)=self.access_filter(pos,&face) else{return};
        let view=self.access_view(pos,&face,if filter==old_filter {page}else{0},&search,filter,if filter==old_filter {&pinned}else{&[]});
        if let Some(s)=self.ae2_local_pattern_access.as_mut(){s.filter=filter;s.view=view;}
    }
    pub(super) fn ae2_local_pattern_access_tick(&mut self) {
        let Some(s)=self.ae2_local_pattern_access.as_mut() else{return};
        s.refresh=s.refresh.saturating_add(1);
        if s.refresh>=10 {s.refresh=0;self.ae2_local_pattern_access_refresh();}
    }
    pub(super) fn ae2_pattern_access_layout(&self)->Option<serde_json::Value> {
        if self.ae2_local_pattern_access_open() {
            let s=self.ae2_local_pattern_access.as_ref()?;
            Some(serde_json::json!({"rows":s.view.layout.clone(),"offset":s.view.page,"maximum":s.view.max_scroll,"filter":s.filter}))
        }else{self.remote.as_ref().and_then(|r|r.ae2_pattern_layout.clone())}
    }
    pub(super) fn ae2_local_pattern_access_slot_view(&self,slot:usize)->Option<ItemStack> {
        if !self.ae2_local_pattern_access_valid(){return None;}
        self.ae2_local_pattern_access.as_ref()?.view.items.get(slot.checked_sub(300)?).cloned().flatten()
    }
    pub(super) fn ae2_local_pattern_access_search_focused(&self)->bool {
        self.ae2_local_pattern_access_open() && self.ae2_local_pattern_access.as_ref().is_some_and(|s|s.field.focused)
    }
    pub(super) fn ae2_local_pattern_access_search_input(&mut self,text:Option<&str>,key:Option<winit::keyboard::KeyCode>) {
        if !self.ae2_local_pattern_access_valid(){return;}
        let ctrl=self.has_control_down();let shift=self.has_shift_down();
        if let Some(s)=self.ae2_local_pattern_access.as_mut(){let before=s.field.value.clone();if !key.is_some_and(|key|s.field.key(key,ctrl,shift)){if let Some(text)=text{s.field.insert_text(text);}}if s.field.value!=before{s.view.page=0;}}
        self.ae2_local_pattern_access_refresh();
    }
    pub(super) fn ae2_local_pattern_access_search_shortcut(&mut self,shortcut:horizon_client::clipboard::EditShortcut) {
        if let Some(s)=self.ae2_local_pattern_access.as_mut(){let before=s.field.value.clone();s.field.shortcut(shortcut,&mut self.clipboard);if s.field.value!=before{s.view.page=0;}}
        self.ae2_local_pattern_access_refresh();
    }
    pub(super) fn ae2_local_pattern_access_search_click(&mut self,ox:f32,oy:f32,right:bool)->bool {
        let (mx,my)=self.cursor;let hit=mx>=ox+104.0 && mx<ox+169.0 && my>=oy+4.0 && my<oy+16.0;let shift=self.has_shift_down();
        let Some(s)=self.ae2_local_pattern_access.as_mut() else{return false};s.field.focused=hit;
        if hit {s.field.x=ox+104.0;s.field.y=oy+4.0;s.field.width=65.0;s.field.height=12.0;s.field.click(&self.gui.atlas,mx,shift,right);}hit
    }
    pub(super) fn ae2_local_pattern_access_search_draw(&self,batch:&mut horizon_client::hud::HudBatch,ox:f32,oy:f32) {
        if let Some(mut field)=self.ae2_local_pattern_access.as_ref().map(|s|s.field.clone()){field.x=ox+104.0;field.y=oy+4.0;field.width=65.0;field.height=12.0;field.draw(batch,&self.gui.atlas,None);}
    }
    pub(super) fn ae2_local_pattern_access_control(&mut self,action:i16)->bool {
        if !self.ae2_local_pattern_access_valid() || !matches!(action,-10|-11|-12){return false;}
        let Some(s)=self.ae2_local_pattern_access.as_ref() else{return false};
        let (pos,face,next)=(s.pos,s.face.clone(),(s.filter+1)%3);
        if action==-12 && !self.access_set_filter(pos,&face,next){return true;}
        if let Some(s)=self.ae2_local_pattern_access.as_mut(){match action {
            -10=>s.view.page=s.view.page.saturating_sub(1),-11=>s.view.page=(s.view.page+1).min(s.view.max_scroll),
            -12=>{s.filter=next;s.view.page=0;s.view.pinned.clear();},_=>{}
        }}
        self.ae2_local_pattern_access_refresh();true
    }
    /// An actual player menu remains the transaction engine for non-provider
    /// inventory slots and quick-craft state.
    fn access_local_player_menu(&mut self,session:&mut LocalPatternAccess,slot:Option<usize>,button:i32,input:Input) {
        let before_random=self.world.random.clone();
        let result={let env=self.menu_env();let menu=&mut session.menu;
            for (i,item) in self.inventory.iter().enumerate(){menu.set_player_item(i,item.clone());}
            for (i,slot) in [39,38,37,36].into_iter().enumerate(){menu.set_player_item(slot,self.armor[i].clone());}
            menu.set_player_item(40,self.offhand.clone());menu.selected=self.selected;menu.creative=self.creative;menu.carried=self.carried.clone();
            menu.recipe_durability_rng=Some(before_random);
            let result=menu.click(&env,slot.map_or(-999,|slot|slot as i32),button,input);
            let random=menu.recipe_durability_rng.take();let dropped=std::mem::take(&mut menu.dropped);let equipped=std::mem::take(&mut menu.equipped);
            menu.drops.clear();menu.crafted.clear();
            result.map(|()|((0..36).map(|i|menu.player_item(i).cloned()).collect::<Vec<_>>(),
                std::array::from_fn(|i|menu.player_item([39,38,37,36][i]).cloned()),menu.player_item(40).cloned(),menu.carried.take(),dropped,equipped,random))
        };
        if let Ok((inventory,armor,offhand,carried,dropped,equipped,random))=result {
            self.inventory=inventory;self.armor=armor;self.offhand=offhand;self.carried=carried;
            if let Some(random)=random{self.world.random=random;}
            for item in dropped{self.player_drop(item);}for (slot,item) in equipped{self.player_equipped(slot,item.as_ref());}
        }else{session.menu=Menu::new(Kind::Chest(6));}
    }
    pub(super) fn ae2_local_pattern_access_click(&mut self,slot:Option<usize>,button:i32,input:Input) {
        if !self.ae2_local_pattern_access_valid(){self.close_container();self.screen=super::Screen::Playing;self.capture(true);return;}
        if input==Input::Clone && slot.is_some_and(|slot|self.slot_view(slot).is_some_and(|item|item.item.as_ref()=="ae2:wrapped_generic_stack")){return;}
        let Some(mut session)=self.ae2_local_pattern_access.take() else{return};
        let fresh=self.access_view(session.pos,&session.face,session.view.page,&session.field.value,session.filter,&session.view.pinned);
        if fresh.rows!=session.view.rows || fresh.owners!=session.view.owners {session.view=fresh;self.ae2_local_pattern_access=Some(session);return;}
        match slot {
            Some(slot @ 300..=353)=>{
                let line=(slot-300)/9;let index=(slot-300)%9;
                let owner=display_lines(&session.view.rows).get(session.view.page+line).and_then(|line|match line {DisplayLine::Provider(row)=>Some(row.owner),_=>None});
                if let Some(owner)=owner.filter(|owner|same_sent_owner(session.view.owners.get(line).copied().flatten(),self.access_owner_identity(*owner))) {
                    self.access_provider_action(owner,index,button,input);
                }
            },
            Some(index @ 0..=35) if input==Input::QuickMove=>self.access_quick_move_player(index,&session.view.rows),
            Some(index @ 0..=35)=>{let wire=if index<9 {81+index}else{54+index-9};self.access_local_player_menu(&mut session,Some(wire),button,input);},
            None if input==Input::QuickCraft || input==Input::Pickup=>self.access_local_player_menu(&mut session,None,button,input),
            _=>{}
        }
        session.view=self.access_view(session.pos,&session.face,session.view.page,&session.field.value,session.filter,&session.view.pinned);
        self.ae2_local_pattern_access=Some(session);
    }
    pub(super) fn lan_open_pattern_access(&mut self,host:&mut LanHost,conn:u32,pos:(i32,i32,i32),face:&str)->bool {
        let Some(filter)=self.access_filter(pos,face) else{return false};
        let Some(terminal)=self.access_terminal_identity(pos,face) else{return false};
        self.lan_pattern_access_closed(host,conn,None);
        self.lan_ae2_terminal_closed(host,conn,None);self.lan_ae2_machine_closed(host,conn,None);
        self.lan_ae2_portable_closed(host,conn,None);self.lan_ae2_network_tool_closed(host,conn,None);self.lan_ae2_anchor_closed(host,conn,None);
        self.lan_ae2_bus_closed(host,conn,None);self.lan_craft_confirm_closed(host,conn,None);
        self.lan_station_closed(host,conn,None);self.lan_crafting_closed(host,conn,None);self.guest_container_closed(host,conn);
        let Some(window)=host.hub.open_window(conn,"minecraft:generic_9x6",horizon_protocol::text::literal("AE2 Pattern Access")) else{return true;};
        host.hub.send_to(conn,&C::CustomPayload(horizon_protocol::packets::configuration::CustomPayload {
            channel:"ae2:bus_ack_enable".into(),data:window.to_be_bytes().to_vec(),
        }));
        host.play.entry(conn).or_default().ae2_pattern_access=Some(GuestPatternAccess{window,state:0,pos,face:face.into(),dim:self.dim.id().into(),page:0,terminal,search:String::new(),filter,rows:Vec::new(),pinned:Vec::new(),menu:Menu::new(Kind::Chest(6)),refresh:0,sent_views:std::collections::VecDeque::new()});
        self.lan_send_pattern_access(host,conn);true
    }
    /// Build the same provider rows and original-owner identities for both
    /// the remote and in-frame Pattern Access screens.
    fn access_view(&self,pos:(i32,i32,i32),face:&str,page:usize,search:&str,filter:u8,pinned:&[Owner])->AccessView {
        let (rows,pins)=if self.ae2_part_online(pos,&face){self.access_rows(pos,search,filter,pinned)} else{(Vec::new(),Vec::new())};
        let lines=display_lines(&rows);let max_scroll=lines.len().saturating_sub(6);let page=page.min(max_scroll);
        let mut items=vec![None;54];let mut layout=Vec::new();let mut owners=vec![None;6];
        for (i,line) in lines.iter().skip(page).take(6).enumerate(){
            match line {
                DisplayLine::Header(group,count)=>layout.push(serde_json::json!({"kind":"header","name":group.name,"count":count,"tooltip":group.tooltip,"icon":{"id":group.icon.item_id,"components":group.icon.components}})),
                DisplayLine::Provider(row)=>{owners[i]=self.access_owner_identity(row.owner);items[i*9..i*9+9].clone_from_slice(&row.slots);layout.push(serde_json::json!({"kind":"provider","name":row.name}));}
            }
        }
        AccessView{rows,pinned:pins,page,items,layout,owners,max_scroll}
    }
    fn lan_send_pattern_access(&mut self,host:&mut LanHost,conn:u32) {
        let Some((window,state,pos,face,page,search,filter,pinned,terminal))=host.play.get(&conn).and_then(|p|p.ae2_pattern_access.as_ref()).map(|s|(s.window,s.state,s.pos,s.face.clone(),s.page,s.search.clone(),s.filter,s.pinned.clone(),s.terminal)) else{return};
        if self.access_terminal_identity(pos,&face)!=Some(terminal) {
            self.lan_pattern_access_closed(host,conn,None);host.hub.send_to(conn,&C::ContainerClose(play::ContainerClose{container:window}));return;
        }
        let AccessView{rows,pinned:pins,page,mut items,layout,owners,max_scroll}=self.access_view(pos,&face,page,&search,filter,&pinned);
        let data=serde_json::to_vec(&serde_json::json!({"window":window,"rows":layout,"offset":page,"maximum":max_scroll,"filter":filter})).unwrap_or_default();
        host.hub.send_to(conn,&C::CustomPayload(horizon_protocol::packets::configuration::CustomPayload{channel:"ae2:pattern_layout".into(),data}));
        let Some(g)=host.hub.guests.iter().find(|g|g.conn==conn) else{return};
        items.extend((9..36).chain(0..9).map(|i|g.inventory[i].clone()));let carried=g.carried.clone();
        let wire=items.iter().map(|s|host.hub.stack(s)).collect();let carried=host.hub.stack(&carried);
        let Some(s)=host.play.get_mut(&conn).and_then(|p|p.ae2_pattern_access.as_mut()) else{return};
        s.rows=rows;s.pinned=pins;s.page=page;s.state=(state+1)&0x7fff;let state=s.state;
        s.sent_views.push_back((state,owners));while s.sent_views.len()>64 {s.sent_views.pop_front();}
        host.hub.send_to(conn,&C::ContainerSetContent(play::ContainerSetContent{container:window,state,items:wire,carried}));
    }
    pub(super) fn lan_pattern_access_search(&mut self,host:&mut LanHost,conn:u32,value:&str)->bool {
        let Some((pos,face,dim,terminal,window))=host.play.get(&conn).and_then(|p|p.ae2_pattern_access.as_ref()).map(|s|(s.pos,s.face.clone(),s.dim.clone(),s.terminal,s.window)) else{return false};
        let valid=dim==self.dim.id() && self.access_terminal_identity(pos,&face)==Some(terminal)
            && host.hub.guests.iter().any(|g|g.conn==conn && host.hub.here(g) && g.within_block_range([pos.0,pos.1,pos.2],4.0));
        if !valid {self.lan_pattern_access_closed(host,conn,None);host.hub.send_to(conn,&C::ContainerClose(play::ContainerClose{container:window}));return true;}
        let Some(s)=host.play.get_mut(&conn).and_then(|p|p.ae2_pattern_access.as_mut()) else{return true};
        s.search=value.chars().take(128).collect();s.page=0;self.lan_send_pattern_access(host,conn);true
    }
    /// Transfer a real encoded item through the same provider transaction
    /// for both the LAN and in-frame Pattern Access screen.
    fn access_provider_action(&mut self,owner:Owner,slot:usize,button:i32,input:Input) {
        let Some(mut machine)=self.access_machine(owner) else{return};
        if slot>=machine.slots.len(){return;}
        let snapshot=self.ae2_snapshot_blocks();let old_inventory=self.inventory.clone();let old_cursor=self.carried.clone();
        let current=machine.slots[slot].clone();
        if input==Input::QuickMove {
            if let Some(item)=current {machine.slots[slot]=self.give(item);}
        } else if input==Input::Clone && self.creative && self.carried.is_none() {
            self.carried=current;
        } else if input==Input::Pickup && matches!(button,0|1) {
            let held=self.carried.clone();
            if held.is_none() {self.carried=current;machine.slots[slot]=None;}
            else if (button==0 || current.is_none()) && held.as_ref().is_some_and(|x|x.count==1 && encoded_pattern_item(x)) {
                self.carried=current;machine.slots[slot]=held;
            }
        }
        if self.access_store(owner,&machine).is_err() {
            self.ae2_restore_blocks(snapshot);self.inventory=old_inventory;self.carried=old_cursor;
        }
    }
    fn access_quick_move_player(&mut self,index:usize,rows:&[Row]) {
        if let Some(item)=self.inventory.get(index).and_then(Clone::clone).filter(|v|v.count==1 && pattern(v)) {
            for row in rows {
                let Some(mut machine)=self.access_machine(row.owner) else{continue};
                let Some(slot)=machine.slots.iter().position(Option::is_none) else{continue};
                machine.slots[slot]=Some(item.clone());
                if self.access_store(row.owner,&machine).is_ok(){self.inventory[index]=None;break;}
            }
        }
    }
    pub(super) fn lan_pattern_access_click(&mut self,host:&mut LanHost,conn:u32,p:&play::ContainerClick)->bool {
        let Some(mut s)=host.play.get_mut(&conn).and_then(|p|p.ae2_pattern_access.take()) else{return false};
        if p.container!=s.window {host.play.entry(conn).or_default().ae2_pattern_access=Some(s);return false;}
        let Some(gi)=host.hub.guests.iter().position(|g|g.conn==conn && host.hub.here(g)) else{return true};
        if s.dim!=self.dim.id() || !host.hub.guests[gi].within_block_range([s.pos.0,s.pos.1,s.pos.2],4.0)
            || self.access_terminal_identity(s.pos,&s.face)!=Some(s.terminal) {
            let window=s.window;host.play.entry(conn).or_default().ae2_pattern_access=Some(s);
            self.lan_pattern_access_closed(host,conn,None);host.hub.send_to(conn,&C::ContainerClose(play::ContainerClose{container:window}));return true;
        }
        let (rows,_)=if self.ae2_part_online(s.pos,&s.face){self.access_rows(s.pos,&s.search,s.filter,&s.pinned)}else{(Vec::new(),Vec::new())};
        let stale_target=(0..54).contains(&p.slot) && {
            let index=p.slot as usize/9;
            let old=s.sent_views.iter().rev().find(|(state,_)|*state==p.state).and_then(|(_,view)|view.get(index)).copied().flatten();
            let current=display_lines(&rows).get(s.page+index).and_then(|line|match line {DisplayLine::Provider(row)=>self.access_owner_identity(row.owner),_=>None});
            !same_sent_owner(old,current)
        };
        if rows!=s.rows || stale_target {
            host.play.entry(conn).or_default().ae2_pattern_access=Some(s);self.lan_send_pattern_access(host,conn);return true;
        }
        let input=super::lan_crafting::input(p.input);
        if (0..54).contains(&p.slot) {
            let lines=display_lines(&rows);let index=s.page+p.slot as usize/9;let slot=p.slot as usize%9;
            if let Some(DisplayLine::Provider(row))=lines.get(index) {
                self.as_guest(host,conn,|game|game.access_provider_action(row.owner,slot,i32::from(p.button),input));
            }
        } else if p.slot == -10 {s.page=s.page.saturating_sub(1);}
        else if p.slot == -11 {s.page=(s.page+1).min(display_lines(&rows).len().saturating_sub(6));}
        else if p.slot == -12 {let next=(s.filter+1)%3;if self.access_set_filter(s.pos,&s.face,next){s.filter=next;s.page=0;s.pinned.clear();}}
        else if (54..90).contains(&p.slot) && input==Input::QuickMove {
            let index=if p.slot<81 {9+(p.slot as usize-54)} else {p.slot as usize-81};
            self.as_guest(host,conn,|game|game.access_quick_move_player(index,&rows));
        }
        else if (54..90).contains(&p.slot) {
            self.guest_menu_op(host,gi,&mut s.menu,|m,env|{let _=m.click(env,i32::from(p.slot),i32::from(p.button),input);});
        }
        host.play.entry(conn).or_default().ae2_pattern_access=Some(s);self.lan_send_pattern_access(host,conn);true
    }
    pub(super) fn lan_pattern_access_closed(&mut self,host:&mut LanHost,conn:u32,window:Option<i32>)->bool {
        if host.play.get_mut(&conn).and_then(|p|p.ae2_pattern_access.take_if(|s|window.is_none_or(|w|w==s.window))).is_none(){return false;}
        if let Some(gi)=host.hub.guests.iter().position(|g|g.conn==conn){self.guest_carried_back(host,gi);}true
    }
    pub(super) fn lan_refresh_pattern_access(&mut self,host:&mut LanHost) {
        let viewers:Vec<_>=host.play.iter().filter_map(|(&id,p)|p.ae2_pattern_access.as_ref().map(|s|(id,s.pos,s.face.clone(),s.dim.clone(),s.window,s.terminal))).collect();
        for (conn,pos,face,dim,window,terminal) in viewers {
            let Some(g)=host.hub.guests.iter().find(|g|g.conn==conn && host.hub.here(g)) else{continue;};
            let filter=self.access_filter(pos,&face);
            if dim!=self.dim.id() || !g.within_block_range([pos.0,pos.1,pos.2],4.0) || filter.is_none()
                || self.access_terminal_identity(pos,&face)!=Some(terminal) {
                self.lan_pattern_access_closed(host,conn,None);host.hub.send_to(conn,&C::ContainerClose(play::ContainerClose{container:window}));
            } else if let Some(s)=host.play.get_mut(&conn).and_then(|p|p.ae2_pattern_access.as_mut()) {
                if filter!=Some(s.filter) {s.filter=filter.unwrap();s.page=0;s.pinned.clear();self.lan_send_pattern_access(host,conn);continue;}
                s.refresh=s.refresh.saturating_add(1);
                if s.refresh>=10 {
                    s.refresh=0;
                    let (search,filter,old,pinned)=(s.search.clone(),s.filter,s.rows.clone(),s.pinned.clone());
                    let rows=if self.ae2_part_online(pos,&face){self.access_rows(pos,&search,filter,&pinned).0}else{Vec::new()};
                    if rows!=old {self.lan_send_pattern_access(host,conn);}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Group;
    use ae2_cell_core::ItemKey;
    #[test]
    fn pattern_access_stale_row_rejects_replaced_host_or_part() {
        use super::{Owner,OwnerIdentity,same_sent_owner};
        use crate::game::ae2_global::GridAddress;
        let address=GridAddress::from_network(ae2_network_core::Pos{x:1,y:64,z:2}).unwrap();
        let old=OwnerIdentity{owner:Owner::Block(address),host:7,part:None};
        assert!(same_sent_owner(Some(old),Some(old)));
        assert!(!same_sent_owner(Some(old),Some(OwnerIdentity{host:8,..old})));
        let old_part=OwnerIdentity{owner:Owner::Part(super::PartAddress{host:address,face:0}),part:Some([1,2,3,4]),..old};
        assert!(!same_sent_owner(Some(old_part),Some(OwnerIdentity{part:Some([5,6,7,8]),..old_part})));
        assert!(!same_sent_owner(None,None));
    }
    #[test]
    fn pattern_access_group_identity_keeps_icon_components_and_tooltip() {
        let group=Group{icon:ItemKey{item_id:"minecraft:chest".into(),components:vec![1]},name:"Smelter".into(),tooltip:vec![]};
        let mut changed=group.clone();changed.icon.components.push(2);
        assert_ne!(group,changed);
        changed=group.clone();changed.tooltip.push("Other machine".into());
        assert_ne!(group,changed);
        assert_eq!(group,group.clone());
    }
}
