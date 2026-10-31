/** A physical Annihilation Plane preserves container contents and item components. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('annihilation-container-custody', mode);
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
  await fixture.command(`setblock ${chest} minecraft:chest[facing=north]`);
  await fixture.command(`data merge block ${chest} {Items:[{Slot:0b,id:"minecraft:emerald",count:7,components:{"minecraft:custom_data":{custody_probe:17}}}]}`);
  await fixture.stepServer(100);
  const before = JSON.stringify(await fixture.command(`data get block ${chest}`));
  assert.match(before, /minecraft:emerald/);
  assert.match(before, /custody_probe/);
  fixture.mark('without storage, the real chest and its component-bearing contents remain in place');

  await fixture.command(`item replace block ${drive} container.0 with ae2:item_storage_cell_1k`);
  await fixture.stepServer(300);
  const absent = await fixture.command(`execute if block ${chest} minecraft:air`);
  assert((absent.chat ?? []).some((line: string) => line === 'Test passed'), 'the plane broke the physical chest');
  const grid = await fixture.ae(`grid status ${drive}`);
  const chestEntry = grid.items.find((entry: any) => entry.item === 'minecraft:chest');
  const emeraldIndex = grid.items.findIndex((entry: any) => entry.item === 'minecraft:emerald');
  assert.equal(chestEntry?.count, 1, 'the chest block drop entered storage');
  assert(emeraldIndex >= 0 && grid.items[emeraldIndex].count === 7, 'all seven physical contents entered storage');
  assert.equal((await fixture.ae(`grid withdraw ${drive} ${emeraldIndex} 7`)).extracted, 7);
  const player = await until(() => fixture.command('data get entity @s Inventory'), reply =>
    JSON.stringify(reply).includes('custody_probe'), 'withdrawn physical contents preserve the original item component', 30000);
  assert.match(JSON.stringify(player), /custody_probe.{0,12}17/);
  fixture.mark('the chest drop and all seven component-bearing contents are conserved through physical pickup');

  await fixture.command(`setblock ${chest} minecraft:furnace[facing=north]`);
  await fixture.command(`data merge block ${chest} {Items:[{Slot:0b,id:"minecraft:emerald",count:5,components:{"minecraft:custom_data":{custody_probe:18}}}]}`);
  assert.match(JSON.stringify(await fixture.command(`data get block ${chest}`)), /custody_probe/);
  await fixture.stepServer(300);
  assert((await fixture.command(`execute if block ${chest} minecraft:air`)).chat?.includes('Test passed'), 'the plane broke the physical furnace');
  const afterFurnace = await fixture.ae(`grid status ${drive}`);
  assert.equal(afterFurnace.items.find((entry: any) => entry.item === 'minecraft:furnace')?.count, 1);
  const furnaceContents = afterFurnace.items.findIndex((entry: any) => entry.item === 'minecraft:emerald');
  assert(furnaceContents >= 0 && afterFurnace.items[furnaceContents].count === 5, 'all five furnace contents entered storage');
  assert.equal((await fixture.ae(`grid withdraw ${drive} ${furnaceContents} 5`)).extracted, 5);
  assert.match(JSON.stringify(await fixture.command('data get entity @s Inventory')), /custody_probe.{0,12}18/);
  fixture.mark('the furnace drop and five component-bearing contents are conserved through physical pickup');

  await fixture.hold(false); await fixture.restart();
  const after = await until(() => fixture.ae(`grid status ${drive}`), s => s.online && s.items?.some((entry: any) => entry.item === 'minecraft:chest') && s.items?.some((entry: any) => entry.item === 'minecraft:furnace'), 'plane network and container drops restored', 30000);
  assert.equal(after.items.find((entry: any) => entry.item === 'minecraft:chest')?.count, 1);
  assert.equal(after.items.find((entry: any) => entry.item === 'minecraft:furnace')?.count, 1);
  const restoredPlayer = JSON.stringify(await fixture.command('data get entity @s Inventory'));
  assert.match(restoredPlayer, /custody_probe.{0,12}17/);
  assert.match(restoredPlayer, /custody_probe.{0,12}18/);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('stored blocks and withdrawn contents survive restart without duplication');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
