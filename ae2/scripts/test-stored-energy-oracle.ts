/** Compare native numerics with the actual AE2 StoredEnergyAmount Java source.
 * Usage: AE2_REFERENCE_DIR=/path/to/Applied-Energistics-2 bun run ae2/scripts/test-stored-energy-oracle.ts
 * Requires a JDK and Cargo. Only minimal Minecraft/event type stubs are supplied.
 */
import assert from 'node:assert/strict';
import { existsSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../../', import.meta.url));
const ref = process.env.AE2_REFERENCE_DIR;
assert(ref, 'Set AE2_REFERENCE_DIR to the pinned AE2 source checkout');
const actual = resolve(ref, 'src/main/java/appeng/me/energy/StoredEnergyAmount.java');
assert(existsSync(actual), `Missing source class: ${actual}`);
const workRoot = join(root, '.local'); mkdirSync(workRoot, { recursive: true });
const work = mkdtempSync(join(workRoot, 'stored-energy-oracle-'));
const math = join(work, 'net/minecraft/util');
const events = join(work, 'appeng/api/networking/events');
mkdirSync(math, { recursive: true }); mkdirSync(events, { recursive: true });
writeFileSync(join(math, 'Mth.java'), 'package net.minecraft.util; public final class Mth { public static double clamp(double v, double min, double max) { return v < min ? min : Math.min(v, max); } }\n');
writeFileSync(join(events, 'GridPowerStorageStateChanged.java'), 'package appeng.api.networking.events; public class GridPowerStorageStateChanged { public enum PowerEventType { PROVIDE_POWER, RECEIVE_POWER } }\n');
const oracle = join(work, 'Oracle.java');
writeFileSync(oracle, `import java.util.Locale;
import appeng.me.energy.StoredEnergyAmount;
public class Oracle {
  static void emit(String label, double value, StoredEnergyAmount p) {
    System.out.printf(Locale.ROOT, "%s %016x %016x %016x%n", label,
      Double.doubleToRawLongBits(value), Double.doubleToRawLongBits(p.getAmount()), Double.doubleToRawLongBits(p.getMaximum()));
  }
  public static void main(String[] args) {
    double n=StoredEnergyAmount.MIN_AMOUNT;
    var p=new StoredEnergyAmount(0,1, ignored -> {});
    emit("below_insert",p.insert(n/2,true),p);
    emit("sim_insert",p.insert(n,false),p);
    emit("min_insert",p.insert(n,true),p);
    emit("below_extract",p.extract(n/2,true),p);
    p.setStored(n*1.5); emit("remainder",p.extract(n,true),p);
    p.setStored(0.6); emit("partial",p.insert(0.8,true),p);
    p.setMaximum(Double.MAX_VALUE); emit("maximum",0,p);
    p.setStored(0.5*n); emit("set_small",0,p);
    var unusual=new StoredEnergyAmount(2.0e15,3.0e15,ignored -> {});
    unusual.setMaximum(Double.MAX_VALUE); emit("cap_old",0,unusual);
  }
}\n`);
function run(cmd: string[]): string {
  const child = Bun.spawnSync({ cmd, cwd: root, stdout: 'pipe', stderr: 'pipe' });
  assert.equal(child.exitCode, 0, `${cmd[0]} failed:\n${child.stderr.toString()}`);
  return child.stdout.toString().trim();
}
run(['javac', '-d', work, join(math, 'Mth.java'), join(events, 'GridPowerStorageStateChanged.java'), actual, oracle]);
const java = run(['java', '-cp', work, 'Oracle']);
const native = run(['cargo', 'run', '--quiet', '--manifest-path', join(root, 'crates/ae2-machines-core/Cargo.toml'), '--example', 'stored_energy_oracle']);
assert.equal(native, java, 'native StoredEnergyAmount numerics differ from the pinned source class');
console.log(`PASS actual AE2 StoredEnergyAmount equals native across ${java.split('\n').length} numeric cases`);
