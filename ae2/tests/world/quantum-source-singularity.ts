/** Explosion-produced source components, two real QNB menus, and restart. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('quantum-source-singularity', mode);
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 10, y = Math.ceil(py) + 4, z = Math.floor(pz);
  const at = (dx: number, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  await fixture.command('clear @s');
  await fixture.command(`fill ${at(-4, -1, -4)} ${at(22, -1, 4)} minecraft:obsidian`);
  await fixture.command(`fill ${at(-3, 0, -3)} ${at(21, 4, 3)} minecraft:air`);
  await fixture.command(`tp @s ${x + 4.5} ${y} ${z + 3.5}`);
  for (const id of ['ae2:singularity', 'minecraft:ender_pearl']) {
    await fixture.command(`summon minecraft:item ${x + .5} ${y + .3} ${z + .5} {Item:{id:"${id}",count:1},PickupDelay:32767s}`);
  }
  await until(() => fixture.raw('entities 32 item'), v => v.entities?.filter((e: any) => ['ae2:singularity', 'minecraft:ender_pearl'].includes(e.item?.item)).length === 2, 'two physical explosion inputs');
  await fixture.command(`summon minecraft:tnt ${x + .5} ${y + .3} ${z + .5} {fuse:30s}`);
  const output = await until(() => fixture.raw('entities 32 item'), v => v.entities?.some((e: any) => e.item?.item === 'ae2:quantum_entangled_singularity' && e.item.count === 2), 'TNT produces the actual paired singularities', 30000);
  const pair = output.entities.find((e: any) => e.item?.item === 'ae2:quantum_entangled_singularity');
  const components = String(pair.item.components);
  const frequency = components.match(/ae2:entangled_singularity_id["']?\s*:\s*(-?\d+)/)?.[1];
  assert(frequency && frequency !== '0', `source component frequency: ${components}`);
  assert(!components.includes('minecraft:custom_data'), `frequency is the real AE2 component: ${components}`);
  assert(!output.entities.some((e: any) => ['ae2:singularity', 'minecraft:ender_pearl'].includes(e.item?.item)));
  fixture.mark('a real explosion consumes its two ingredients and creates a source-component pair');

  await fixture.command(`tp @s ${pair.pos[0]} ${pair.pos[1]} ${pair.pos[2]}`);
  const inventory = await until(() => fixture.raw('inventory'), v => v.hotbar?.some((s: any) => s?.item === 'ae2:quantum_entangled_singularity' && s.count === 2), 'physical pair picked up', 30000);
  let pairSlot = inventory.hotbar.findIndex((s: any) => s?.item === 'ae2:quantum_entangled_singularity' && s.count === 2);
  for (const dx of [8, 18]) {
    for (let a = -1; a <= 1; a++) for (let b = -1; b <= 1; b++) {
      const pos = at(dx + a, b);
      await fixture.command(`setblock ${pos} ae2:${a === 0 && b === 0 ? 'quantum_link' : 'quantum_ring'}`);
      await fixture.ae(`grid status ${pos}`);
    }
  }
  await fixture.command(`setblock ${at(8, 1, 1)} ae2:creative_energy_cell`);
  await fixture.command(`setblock ${at(18, 1, 1)} ae2:drive`);
  for (const dx of [8, 18]) {
    await fixture.select(pairSlot, 'ae2:quantum_entangled_singularity');
    await fixture.command(`tp @s ${x + dx + .5} ${y} ${z - 2.5}`);
    await fixture.raw(`interact ${at(dx)} north`);
    await until(() => fixture.raw('screen'), s => s.slots?.some((slot: any) => slot.slot === pairSlot && slot.item === 'ae2:quantum_entangled_singularity'), 'quantum chamber menu with the physical pair', 30000);
    await fixture.raw(`click-slot ${pairSlot}`);
    await until(() => fixture.raw('inventory'), v => v.carried?.item === 'ae2:quantum_entangled_singularity', 'pair on cursor');
    await fixture.raw('click-slot 300');
    await until(() => fixture.raw('screen'), s => s.slots?.some((slot: any) => slot.slot === 300 && slot.item === 'ae2:quantum_entangled_singularity' && slot.count === 1), 'one singularity in the real chamber slot', 30000);
    await fixture.raw('close');
    if (dx === 8) {
      const remaining = await until(() => fixture.raw('inventory'), v => v.hotbar?.some((s: any) => s?.item === 'ae2:quantum_entangled_singularity' && s.count === 1), 'other half returned to the player');
      pairSlot = remaining.hotbar.findIndex((s: any) => s?.item === 'ae2:quantum_entangled_singularity' && s.count === 1);
    }
  }
  const remote = at(18, 1, 1);
  await until(() => fixture.ae(`grid status ${remote}`), s => s.online && s.powered, 'two physical QNB frames carry grid power', 30000);
  const chamber = async (dx: number) => JSON.stringify(await fixture.command(`data get block ${at(dx)}`));
  for (const dx of [8, 18]) {
    const saved = await chamber(dx);
    assert.match(saved, new RegExp(`ae2:entangled_singularity_id.*${frequency}`));
    assert(!saved.includes('minecraft:custom_data'), 'chamber stores the source component directly');
  }
  fixture.mark('both actual chamber menus accept and split the produced pair, joining the powered grids');
  await fixture.restart();
  await until(() => fixture.ae(`grid status ${remote}`), s => s.online && s.powered, 'paired bridge online after restart', 30000);
  for (const dx of [8, 18]) assert.match(await chamber(dx), new RegExp(`ae2:entangled_singularity_id.*${frequency}`));
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('both source-component singularities and the linked grid survive restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
