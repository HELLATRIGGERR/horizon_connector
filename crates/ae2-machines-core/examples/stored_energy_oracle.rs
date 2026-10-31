//! Bit-exact numeric transcript compared with AE2's actual Java class by
//! scripts/test-stored-energy-oracle.ts.
use ae2_machines_core::stored_energy::{StoredEnergy, MIN_AMOUNT};

fn emit(label: &str, result: f64, storage: &StoredEnergy) {
    println!(
        "{label} {:016x} {:016x} {:016x}",
        result.to_bits(),
        storage.stored().to_bits(),
        storage.maximum().to_bits()
    );
}
fn main() {
    let mut p = StoredEnergy::new(0.0, 1.0);
    let result = p.insert(MIN_AMOUNT / 2.0, true);
    emit("below_insert", result, &p);
    let result = p.insert(MIN_AMOUNT, false);
    emit("sim_insert", result, &p);
    let result = p.insert(MIN_AMOUNT, true);
    emit("min_insert", result, &p);
    let result = p.extract(MIN_AMOUNT / 2.0, true);
    emit("below_extract", result, &p);
    p.set_stored(MIN_AMOUNT * 1.5);
    let result = p.extract(MIN_AMOUNT, true);
    emit("remainder", result, &p);
    p.set_stored(0.6);
    let result = p.insert(0.8, true);
    emit("partial", result, &p);
    p.set_maximum(f64::MAX);
    emit("maximum", 0.0, &p);
    p.set_stored(0.5 * MIN_AMOUNT);
    emit("set_small", 0.0, &p);
    let mut unusual = StoredEnergy::new(2.0e15, 3.0e15);
    unusual.set_maximum(f64::MAX);
    emit("cap_old", 0.0, &unusual);
}
