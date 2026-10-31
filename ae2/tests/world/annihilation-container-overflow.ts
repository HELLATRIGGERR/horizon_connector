/** Source block-loot preflight can succeed while container spill overflows. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('annihilation-container-overflow', mode);
const itemSelector = '@e[type=minecraft:item,nbt={Item:{id:"minecraft:emerald"}},limit=1,sort=nearest]';
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 6, y = Math.ceil(py) + 6, z = Math.floor(pz) + 6;
  const at = (dx = 0, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const host = at(), power = at(-1), drive = at(1), chest = at(0, 1);
  await fixture.command(`fill ${at(-3, -2, -3)} ${at(4, -2, 3)} minecraft:stone`);
  await fixture.command(`fill ${at(-3, -1, -3)} ${at(4, 3, 3)} minecraft:air`);
  await fixture.command(`tp @s ${x + 2.5} ${y + 1} ${z - 2.5}`);
  await fixture.replace('hotbar.0', 'ae2:annihilation_plane');
  await fixture.select(0, 'ae2:annihilation_plane');
  await fixture.hold();
  await fixture.command(`setblock ${power} ae2:creative_energy_cell`);
  await fixture.command(`setblock ${drive} ae2:drive`);
  await fixture.command(`setblock ${host} ae2:cable_bus`);
  assert.equal((await fixture.ae(`grid part ${host} up`)).attached, true);
  // One stored type uses 8 bytes and 7,936 items use 992 bytes. A new chest
  // can fit in the remaining 24 bytes; a full chest's contents cannot.
  await fixture.command(`item replace block ${drive} container.0 with ae2:item_storage_cell_1k[ae2:storage_cell_inv=[{"#t":"ae2:i",id:"minecraft:stone","#":7936L}]]`);
  await fixture.stepServer(100);
  await until(() => fixture.ae(`grid status ${drive}`), s => s.online && s.items?.some((e: any) => e.item === 'minecraft:stone' && e.count === 7936), 'nearly full physical cell is online');
  await fixture.command(`setblock ${chest} minecraft:chest[facing=north]`);
  const slots = Array.from({ length: 27 }, (_, slot) =>
    `{Slot:${slot}b,id:"minecraft:emerald",count:64,components:{"minecraft:custom_data":{overflow_probe:99}}}`);
  await fixture.command(`data merge block ${chest} {Items:[${slots.join(',')}]}`);
  assert.match(JSON.stringify(await fixture.command(`data get block ${chest}`)), /overflow_probe/);
  // Stay out of pickup range while the server advances.
  await fixture.command(`tp @s ${x + 12.5} ${y + 1} ${z + 10.5}`);
  await fixture.stepServer(300);
  assert((await fixture.command(`execute if block ${chest} minecraft:air`)).chat?.includes('Test passed'), 'the plane broke a chest whose own block drop fit the simulated storage');
  const grid = await fixture.ae(`grid status ${drive}`);
  assert.equal(grid.items.find((e: any) => e.item === 'minecraft:stone')?.count, 7936);
  assert.equal(grid.items.find((e: any) => e.item === 'minecraft:emerald')?.count, 128, 'only available storage capacity is committed');
  const outside = await fixture.command(`data get entity ${itemSelector} Item`);
  assert.match(JSON.stringify(outside), /overflow_probe.{0,12}99/, 'unaccepted contents remain a component-bearing physical item entity');
  fixture.mark('capacity is committed once and the unaccepted chest contents remain physical item entities');

  await fixture.hold(false);
  await fixture.restart();
  await until(() => fixture.ae(`grid status ${drive}`), s => s.online && s.items?.some((e: any) => e.item === 'minecraft:emerald' && e.count === 128), 'stored contents restored', 30000);
  const restored = await until(() => fixture.command(`data get entity ${itemSelector} Item`), r => JSON.stringify(r).includes('overflow_probe'), 'overflow entity restored', 30000);
  assert.match(JSON.stringify(restored), /overflow_probe.{0,12}99/);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('both the stored portion and a component-bearing overflow entity survive restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
