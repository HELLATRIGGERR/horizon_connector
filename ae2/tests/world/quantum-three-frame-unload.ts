/**
 * QuantumCluster registers the first two live callbacks, not the first two
 * positions. onChunkUnloaded disconnects the existing pair without promoting
 * the third frame; its ready callback on reload registers the original pair.
 */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('quantum-three-frame-unload', mode);
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  // Keep the first frame away from any always-loaded world-spawn area.
  const x = Math.floor(px / 16) * 16 + 328;
  const y = Math.ceil(py) + 6;
  const z = Math.floor(pz / 16) * 16 + 8;
  const centers = [x, x + 160, x + 320];
  const pos = (cx: number, dx = 0, dy = 0, dz = 0) => `${cx + dx} ${y + dy} ${z + dz}`;
  const condition = async (cx: number, loaded: boolean) => {
    try {
      const reply = await fixture.command(`execute ${loaded ? 'if' : 'unless'} loaded ${pos(cx)}`);
      return (reply.chat ?? []).some((line: string) => line === 'Test passed');
    }
    catch { return false; }
  };
  const grid = async (where: string) => {
    try { return await fixture.ae(`grid status ${where}`); }
    catch { return null; }
  };
  for (const cx of centers) {
    await fixture.command(`forceload add ${cx} ${z}`);
    await until(() => condition(cx, true), Boolean, `forced frame chunk ${cx} loaded`, 90000);
  }
  for (const cx of centers) {
    // Visit each frame to establish actual block-ticking/onReady before
    // observing ordering; a merely resident remote chunk is not readiness.
    await fixture.command(`tp @s ${cx + 0.5} ${y + 3} ${z + 0.5}`);
    await fixture.command(`fill ${pos(cx, -3, -2, -3)} ${pos(cx, 3, 2, 3)} minecraft:air`);
    await fixture.command(`fill ${pos(cx, -3, -3, -3)} ${pos(cx, 3, -3, 3)} minecraft:stone`);
    for (let dx = -1; dx <= 1; dx++) for (let dz = -1; dz <= 1; dz++) {
      const where = pos(cx, dx, 0, dz);
      await fixture.command(`setblock ${where} ae2:${dx === 0 && dz === 0 ? 'quantum_link' : 'quantum_ring'}`);
      // The diagnostic also ensures a native block entity exists and drains
      // its registration event. A just-placed node may not be ready yet.
      const seeded = await fixture.command(`ae2 grid status ${where}`);
      assert((seeded.chat ?? []).some((line: string) => line.startsWith('AE2 {') || line === 'AE2 missing graph node'),
        `quantum node initialization failed: ${JSON.stringify(seeded)}`);
    }
    await until(() => grid(pos(cx)), value => value !== null, `frame ${cx} has a ready center node`, 30000);
  }
  await fixture.command(`setblock ${pos(centers[2], -2)} ae2:creative_energy_cell`);
  await fixture.command(`setblock ${pos(centers[0], 2)} ae2:drive`);
  await fixture.command(`setblock ${pos(centers[1], 2)} ae2:drive`);
  await until(() => fixture.ae(`grid status ${pos(centers[2], -2)}`), s => s.online && s.powered, 'source grid is ready', 30000);
  await fixture.hold();
  // Register high, low, then middle position: a coordinate-sorted fallback
  // would connect low/middle instead of the first high/low pair.
  for (const cx of [centers[2], centers[0], centers[1]]) {
    await fixture.command(`item replace block ${pos(cx)} container.0 with ae2:quantum_entangled_singularity[ae2:entangled_singularity_id=42]`);
    await fixture.stepServer(100);
  }
  assert((await fixture.ae(`grid status ${pos(centers[0], 2)}`)).online, 'first remote frame is linked to the powered frame');
  assert(!(await fixture.ae(`grid status ${pos(centers[1], 2)}`)).online, 'third frame is not promoted by position');
  fixture.mark('the first two status callbacks, not sorted positions, determine the physical quantum pair');

  await fixture.command(`forceload remove ${centers[0]} ${z}`);
  await fixture.hold(false);
  await until(() => condition(centers[0], false), Boolean, 'the first remote frame actually unloads', 90000);
  assert(await condition(centers[1], true), 'third frame stays loaded');
  assert(await condition(centers[2], true), 'powered frame stays loaded');
  await until(() => fixture.ae(`grid status ${pos(centers[2], -2)}`), s => s.online && s.powered, 'powered surviving grid after unload', 30000);
  assert(!(await fixture.ae(`grid status ${pos(centers[1], 2)}`)).online, 'unloading a pair member does not promote the third');
  fixture.mark('the observed chunk unload disconnects its peer without promoting the third frame');

  await fixture.command(`forceload add ${centers[0]} ${z}`);
  await until(() => condition(centers[0], true), Boolean, 'the first remote frame reloads', 90000);
  await fixture.command(`tp @s ${centers[0] + 0.5} ${y + 3} ${z + 0.5}`);
  const nbt = JSON.stringify(await fixture.command(`data get block ${pos(centers[0])}`));
  assert.match(nbt, /ae2:entangled_singularity_id.*42/, 'the reloaded chamber retains its source frequency');
  assert((await fixture.ae(`grid status ${pos(centers[2], -2)}`)).powered, 'the original peer remains powered after the player moves');
  assert((await fixture.ae(`grid status ${pos(centers[2])}`)).powered, 'the original peer center is still on its powered grid');
  assert(!(await fixture.ae(`grid status ${pos(centers[1], 2)}`)).online, 'third frame is still separate on reload');
  await until(() => fixture.ae(`grid status ${pos(centers[0], 2)}`), s => s.online && s.powered, 'reloaded center restores its prior peer', 60000);
  assert(!(await fixture.ae(`grid status ${pos(centers[1], 2)}`)).online, 'third frame remains separate after reload');
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('the saved physical center reloads and its ready callback reconnects the first pair');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
