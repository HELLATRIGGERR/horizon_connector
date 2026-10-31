//! Ordered StatisticsService chunk multiset and Spatial Anchor owner events.
//! Source: AE2 19.2.18 Grid.add/remove, StatisticsService.addNode/removeNode,
//! GridHelper.addNodeOwnerEventHandler and SpatialAnchorBlockEntity.
use std::collections::{BTreeMap, BTreeSet};

pub type Chunk = (i32, i32);
pub type Dimension = i32;
pub type Service = u64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind { Added, Removed }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event<N> {
    pub anchor: N,
    pub dimension: Dimension,
    pub chunk: Chunk,
    pub kind: Kind,
}

#[derive(Clone, Copy, Debug)]
struct Node {
    dimension: Dimension,
    chunk: Chunk,
    in_world: bool,
    anchor: bool,
    service: Option<Service>,
}

#[derive(Clone, Debug, Default)]
struct Grid<N: Ord> {
    chunks: BTreeMap<(Dimension, Chunk), usize>,
    anchors: BTreeSet<N>,
    members: usize,
}

/// Receives exact GridNode.setGrid transitions, not graph snapshots.
#[derive(Clone, Debug)]
pub struct ChunkObserver<N: Ord> {
    nodes: BTreeMap<N, Node>,
    grids: BTreeMap<Service, Grid<N>>,
}
impl<N: Ord> Default for ChunkObserver<N> {
    fn default() -> Self { Self { nodes: BTreeMap::new(), grids: BTreeMap::new() } }
}
impl<N: Copy + Ord> ChunkObserver<N> {
    /// Registration describes the real managed node. Ordinary AEBasePart
    /// mains are not InWorldGridNode; cable centers, exposed outer nodes and
    /// both Phantom nodes are. Only actual Spatial Anchor block mains set
    /// `anchor`; parts and phantom proxies cannot do so.
    pub fn register(&mut self, id:N, dimension:Dimension, chunk:Chunk, in_world:bool, anchor:bool) -> Result<(), &'static str> {
        if self.nodes.contains_key(&id) { return Err("duplicate chunk observer node"); }
        if anchor && !in_world { return Err("anchor node must be in-world"); }
        self.nodes.insert(id,Node{dimension,chunk,in_world,anchor,service:None});
        Ok(())
    }
    pub fn forget(&mut self, id:N) -> Result<(), &'static str> {
        match self.nodes.get(&id) {
            None => Err("unknown chunk observer node"),
            Some(node) if node.service.is_some() => Err("cannot forget a node still on a grid"),
            Some(_) => {self.nodes.remove(&id);Ok(())}
        }
    }
    /// Transfer emits old-grid removal before new-grid addition. Grid.remove
    /// runs service callbacks before removing the machine owner; Grid.add
    /// inserts the owner before calling its service callbacks. Individual
    /// callbacks to different anchor owners may commute; source hash-set
    /// iteration order between those owners is intentionally not claimed.
    pub fn membership(&mut self, id:N, from:Option<Service>, to:Option<Service>) -> Result<Vec<Event<N>>, &'static str> {
        let node=*self.nodes.get(&id).ok_or("unknown chunk observer node")?;
        if node.service != from {return Err("chunk observer service mismatch");}
        if from == to {return Ok(Vec::new());}
        let key=(node.dimension,node.chunk);
        if let Some(old)=from {
            let grid=self.grids.get(&old).ok_or("missing old chunk grid")?;
            if grid.members==0 || (node.anchor && !grid.anchors.contains(&id))
                || (node.in_world && !grid.chunks.contains_key(&key))
            {return Err("inconsistent old chunk grid");}
        }
        if let Some(new)=to {
            if self.grids.get(&new).and_then(|grid|grid.chunks.get(&key))==Some(&usize::MAX) && node.in_world {
                return Err("chunk reference overflow");
            }
            if self.grids.get(&new).is_some_and(|grid|grid.members==usize::MAX) {
                return Err("grid member overflow");
            }
        }
        let mut events=Vec::new();
        if let Some(old)=from {
            let grid=self.grids.get_mut(&old).expect("validated old grid");
            if node.in_world {
                let count=grid.chunks.get_mut(&key).expect("validated chunk");
                *count-=1;
                if *count==0 {
                    grid.chunks.remove(&key);
                    for anchor in &grid.anchors {
                        if self.nodes[anchor].dimension==node.dimension {
                            events.push(Event{anchor:*anchor,dimension:node.dimension,chunk:node.chunk,kind:Kind::Removed});
                        }
                    }
                }
            }
            if node.anchor {grid.anchors.remove(&id);}
            grid.members-=1;
            if grid.members==0 {self.grids.remove(&old);}
        }
        if let Some(new)=to {
            let grid=self.grids.entry(new).or_insert_with(|| Grid{chunks:BTreeMap::new(),anchors:BTreeSet::new(),members:0});
            if node.anchor {grid.anchors.insert(id);}
            if node.in_world {
                if !grid.chunks.contains_key(&key) {
                    for anchor in &grid.anchors {
                        if self.nodes[anchor].dimension==node.dimension {
                            events.push(Event{anchor:*anchor,dimension:node.dimension,chunk:node.chunk,kind:Kind::Added});
                        }
                    }
                }
                *grid.chunks.entry(key).or_insert(0)+=1;
            }
            grid.members+=1;
        }
        self.nodes.get_mut(&id).expect("validated node").service=to;
        Ok(events)
    }
    pub fn chunk_count(&self, service:Service, dimension:Dimension, chunk:Chunk)->usize {
        self.grids.get(&service).and_then(|grid|grid.chunks.get(&(dimension,chunk))).copied().unwrap_or(0)
    }
    pub fn contains(&self,id:N)->bool {self.nodes.contains_key(&id)}
    pub fn service_of(&self,id:N)->Option<Service> {self.nodes.get(&id).and_then(|node|node.service)}
    pub fn chunks(&self,service:Service,dimension:Dimension)->BTreeSet<Chunk> {
        self.grids.get(&service).map_or_else(BTreeSet::new,|grid|
            grid.chunks.keys().filter_map(|(dim,chunk)|(*dim==dimension).then_some(*chunk)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn event(anchor:u8,dimension:i32,chunk:Chunk,kind:Kind)->Event<u8>{Event{anchor,dimension,chunk,kind}}
    #[test]
    fn anchor_receives_its_own_first_and_last_chunk_events() {
        let mut o=ChunkObserver::default();
        o.register(1,0,(2,3),true,true).unwrap();
        assert_eq!(o.membership(1,None,Some(10)).unwrap(),vec![event(1,0,(2,3),Kind::Added)]);
        assert_eq!(o.membership(1,Some(10),None).unwrap(),vec![event(1,0,(2,3),Kind::Removed)]);
        o.forget(1).unwrap();
    }
    #[test]
    fn source_multiset_counts_real_in_world_nodes_not_plain_part_mains() {
        let mut o=ChunkObserver::default();
        o.register(1,0,(0,0),true,true).unwrap();
        o.register(2,0,(1,0),false,false).unwrap(); // ordinary part main
        o.register(3,0,(1,0),true,false).unwrap();  // in-world outer node
        o.register(4,0,(1,0),true,false).unwrap();  // another exposed node
        o.membership(1,None,Some(1)).unwrap();
        assert!(o.membership(2,None,Some(1)).unwrap().is_empty());
        assert_eq!(o.chunk_count(1,0,(1,0)),0);
        assert_eq!(o.membership(3,None,Some(1)).unwrap(),vec![event(1,0,(1,0),Kind::Added)]);
        assert!(o.membership(4,None,Some(1)).unwrap().is_empty());
        assert_eq!(o.chunk_count(1,0,(1,0)),2);
        assert!(o.membership(3,Some(1),None).unwrap().is_empty());
        assert_eq!(o.membership(4,Some(1),None).unwrap(),vec![event(1,0,(1,0),Kind::Removed)]);
    }
    #[test]
    fn only_anchor_owners_in_the_same_dimension_receive_chunk_events() {
        let mut o=ChunkObserver::default();
        for (id,dim,chunk,anchor) in [(1,0,(0,0),true),(2,1,(0,0),true),(3,1,(5,5),false)] {
            o.register(id,dim,chunk,true,anchor).unwrap();
        }
        o.membership(1,None,Some(7)).unwrap();
        assert_eq!(o.membership(2,None,Some(7)).unwrap(),vec![event(2,1,(0,0),Kind::Added)]);
        assert_eq!(o.membership(3,None,Some(7)).unwrap(),vec![event(2,1,(5,5),Kind::Added)]);
    }
    #[test]
    fn moving_a_node_emits_old_removal_before_new_addition_without_boot_gates() {
        let mut o=ChunkObserver::default();
        for (id,chunk,anchor) in [(1,(0,0),true),(2,(9,0),true),(3,(4,0),false)] {
            o.register(id,0,chunk,true,anchor).unwrap();
        }
        o.membership(1,None,Some(1)).unwrap();o.membership(2,None,Some(2)).unwrap();
        o.membership(3,None,Some(1)).unwrap();
        assert_eq!(o.membership(3,Some(1),Some(2)).unwrap(),vec![
            event(1,0,(4,0),Kind::Removed),event(2,0,(4,0),Kind::Added),
        ]);
    }
    #[test]
    fn wrong_identity_or_service_cannot_mutate_the_observed_multiset() {
        let mut o=ChunkObserver::default();o.register(1,0,(0,0),true,true).unwrap();
        o.membership(1,None,Some(1)).unwrap();
        assert!(o.membership(1,Some(2),None).is_err());
        assert!(o.forget(1).is_err());
        assert_eq!(o.chunk_count(1,0,(0,0)),1);
    }
}
