/** ForgeEnergyAdapter stores the fractional final FE before truncating its report. */
import assert from 'node:assert/strict';
import { WorldFixture, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('fe-partial-acceptance', mode);
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 6, y = Math.ceil(py) + 3, z = Math.floor(pz) + 6;
  const charger = `${x} ${y} ${z}`, generator = `${x - 1} ${y} ${z}`;
  const read = () => fixture.ae(`power ${charger} east`);
  await fixture.hold();
  await fixture.command(`setblock ${charger} ae2:charger[facing=north]`);
  await fixture.command(`data merge block ${charger} {ae2_lab_energy:1599.6d}`);
  await fixture.command(`setblock ${generator} ae2:debug_energy_gen`);
  const before = await read();
  assert(Math.abs(before.energy - 1599.6) < 1e-9);
  await fixture.stepServer(1);
  const after = await read();
  assert.equal(after.energy, 1600, 'a 0.4 AE remainder is physically stored at 0.5 AE/FE');
  assert.equal(after.fe.stored, 3200);
  await fixture.command(`setblock ${generator} minecraft:air`);
  await fixture.hold(false); await fixture.restart();
  assert.equal((await read()).energy, 1600);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('a partial final FE physically fills the charger and survives restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
