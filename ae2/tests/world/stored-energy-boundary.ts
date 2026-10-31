/** AE2 StoredEnergyAmount threshold and maximum, through real powered blocks. */
import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('stored-energy-boundary', mode);
const dir = join(fixture.game, 'config'); mkdirSync(dir, { recursive: true });
const path = join(dir, 'ae2-common.toml');
const config = (ratio: string) => `[powerRatios]\nforgeEnergy=${ratio}\nusageMultiplier=2000000000000.0\n`;
writeFileSync(path, config('0.00000001'));
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 6, y = Math.ceil(py) + 3, z = Math.floor(pz) + 6;
  const at = (dx: number, dz = 0) => `${x + dx} ${y} ${z + dz}`;
  const charger = at(0), generator = at(-1), probe = at(3), chest = at(6);
  const power = (pos: string) => fixture.ae(`power ${pos} east`);
  await fixture.hold();
  await fixture.command(`setblock ${charger} ae2:charger[facing=north]`);
  await fixture.command(`setblock ${probe} ae2:charger[facing=north]`);
  await fixture.command(`data merge block ${probe} {ae2_lab_energy:1.0d}`);
  await fixture.command(`setblock ${chest} ae2:chest`);
  await fixture.command(`data merge block ${chest} {ae2_lab_energy:1000000000000000.0d}`);
  const sourceMaximum = Number(9223372036854775807n / 10000n);
  assert.equal((await power(chest)).energy, sourceMaximum, 'saved chest power clamps to source MAX_MAXIMUM');
  await fixture.command(`setblock ${generator} ae2:debug_energy_gen`);
  await fixture.stepServer(1);
  assert.equal((await power(charger)).energy, 0, '8 FE at 1e-8 AE/FE is below source MIN_AMOUNT');
  fixture.mark('source maximum and sub-minimum insertion apply to actual powered block entities');

  writeFileSync(path, config('0.000000125'));
  await until(() => power(probe), s => s.fe?.stored === 8_000_000, 'threshold ratio hot-reloaded', 10000);
  await fixture.stepServer(1);
  assert.equal((await power(charger)).energy, 0.000001, 'a 1e-6 AE offer is accepted at the exact boundary');
  await fixture.command(`setblock ${generator} minecraft:air`);
  await fixture.hold(false); await fixture.restart();
  assert.equal((await power(charger)).energy, 0.000001);
  assert.equal((await power(chest)).energy, sourceMaximum);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('the exact minimum is stored and both bounded values survive restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
