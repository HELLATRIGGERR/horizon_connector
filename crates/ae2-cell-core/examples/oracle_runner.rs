use ae2_cell_core::{Action, Cell, CellConfig, ItemKey, Tier};
use std::io::{self, BufRead};
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let tier = match args[1].as_str() {
        "1" => Tier::K1,
        "4" => Tier::K4,
        "16" => Tier::K16,
        "64" => Tier::K64,
        "256" => Tier::K256,
        _ => panic!(),
    };
    let mut config = if args[2] == "fluid" {
        CellConfig::fluid(tier)
    } else {
        CellConfig::item(tier)
    };
    config.equal_distribution = args[3] == "equal";
    let mut cell = Cell::new(config);
    for line in io::stdin().lock().lines() {
        let line = line.unwrap();
        let p: Vec<_> = line.split_whitespace().collect();
        let key = ItemKey {
            item_id: p[1].into(),
            components: p[2].as_bytes().to_vec(),
        };
        let n: u64 = p[3].parse().unwrap();
        let action = if p[4] == "M" {
            Action::Modulate
        } else {
            Action::Simulate
        };
        let result = match p[0] {
            "I" => cell.insert(&key, n, action),
            "E" => cell.extract(&key, n, action),
            _ => panic!(),
        };
        println!(
            "{} {} {} {}",
            result,
            cell.used_bytes(),
            cell.total_items(),
            cell.entries().count()
        );
    }
}
