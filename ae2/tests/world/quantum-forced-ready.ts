/** Distinguish remote forced-chunk readiness from ordinary furnace ticking. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('quantum-forced-ready', mode);
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px / 16) * 16 + 328, z = Math.floor(pz / 16) * 16 + 8, y = Math.ceil(py) + 5;
  const pos = (dx = 0) => `${x + dx} ${y} ${z}`;
  const loaded = async (dx: number, dz: number) => {
    const reply = await fixture.command(`execute if loaded ${x + 16 * dx} ${y} ${z + 16 * dz}`);
    return (reply.chat ?? []).some((line: string) => line === 'Test passed');
  };
  await fixture.command(`forceload add ${x} ${z}`);
  for (let dz = -1; dz <= 1; dz++) for (let dx = -1; dx <= 1; dx++) {
    await until(() => loaded(dx, dz), Boolean, `forced chunk neighbor ${dx},${dz} is resident`, 90000);
  }
  await fixture.command(`setblock ${pos()} ae2:quantum_ring`);
  const seed = await fixture.command(`ae2 grid status ${pos()}`);
  assert((seed.chat ?? []).some((line: string) => line.startsWith('AE2 {') || line === 'AE2 missing graph node'));
  await fixture.command(`setblock ${pos(4)} minecraft:furnace[facing=north]`);
  await fixture.command(`data merge block ${pos(4)} {Items:[{Slot:0b,id:"minecraft:sand",count:1},{Slot:1b,id:"minecraft:coal",count:1}]}`);
  await fixture.hold(); await fixture.stepServer(220);
  const furnace = JSON.stringify(await fixture.command(`data get block ${pos(4)}`));
  assert.match(furnace, /minecraft:glass/, 'the same forced chunk actually ticks a physical furnace');
  const node = await fixture.command(`ae2 grid status ${pos()}`);
  assert((node.chat ?? []).some((line: string) => line.startsWith('AE2 {')),
    `AE2 onReady has not produced a real node in the ticking forced chunk: ${JSON.stringify(node)}`);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('a forced remote chunk ticks a furnace and delivers the QNB ready callback without a nearby player');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
