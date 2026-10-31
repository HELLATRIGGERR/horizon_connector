//! Synthetic graphs for the pinned Java PathingCalculation differential oracle.
use ae2_network_core::{Device, DeviceKind, Network, Pos};
use std::io::{self, Read};
fn pos(index: usize) -> Pos { Pos { x: (index * 16) as i32, y: 0, z: 0 } }
fn main() {
    let mut input = String::new(); io::stdin().read_to_string(&mut input).unwrap();
    let mut words = input.split_whitespace();
    let cases: usize = words.next().unwrap_or("0").parse().unwrap();
    let mut answers = Vec::new();
    for _ in 0..cases {
        let n: usize = words.next().unwrap().parse().unwrap();
        let m: usize = words.next().unwrap().parse().unwrap();
        let mut network = Network::default();
        for index in 0..n {
            let kind: u8 = words.next().unwrap().parse().unwrap();
            let group: i32 = words.next().unwrap().parse().unwrap();
            let mut node = Device::new(match kind { 0 => DeviceKind::Controller, 1 => DeviceKind::DenseCable, 2 => DeviceKind::SmartCable, 3 => DeviceKind::Drive, 4 => DeviceKind::CraftingCpu, _ => panic!("unknown fixture kind") });
            node.idle_override = Some(0.0);
            if kind == 0 { node.energy = 8000.0; }
            if group >= 0 { node.multiblock = Some(pos(group as usize)); }
            network.nodes.insert(pos(index), node);
        }
        for _ in 0..m {
            let a: usize = words.next().unwrap().parse().unwrap(); let b: usize = words.next().unwrap().parse().unwrap();
            network.connect(pos(a), pos(b)).unwrap();
        }
        let analysis = network.analyze().unwrap();
        assert_eq!(analysis.components.len(), 1);
        answers.push(serde_json::json!({"channels":analysis.components[0].channels_in_use,"blocks":(analysis.components[0].idle_per_tick*128.0) as u64,"used":(0..n).map(|i|analysis.nodes[&pos(i)].channels_used).collect::<Vec<_>>(),"assigned":(0..n).map(|i|analysis.nodes[&pos(i)].channel).collect::<Vec<_>>() }));
    }
    println!("{}", serde_json::to_string(&answers).unwrap());
}
