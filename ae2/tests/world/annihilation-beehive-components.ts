/** Silk Touch plane copies the hive's bees and honey-state item components. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('annihilation-beehive-components', mode);
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 6, y = Math.ceil(py) + 6, z = Math.floor(pz) + 6;
  const at = (dx = 0, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const host = at(), power = at(-1), drive = at(1), hive = at(0, 1);
  await fixture.command(`fill ${at(-3, -2, -3)} ${at(4, -2, 3)} minecraft:stone`);
  await fixture.command(`fill ${at(-3, -1, -3)} ${at(4, 3, 3)} minecraft:air`);
  await fixture.command(`tp @s ${x + 2.5} ${y + 1} ${z - 2.5}`);
  await fixture.command('clear @s');
  await fixture.replace('hotbar.0', 'ae2:annihilation_plane[minecraft:enchantments={"minecraft:silk_touch":1}]');
  await fixture.select(0, 'ae2:annihilation_plane');
  await fixture.hold();
  await fixture.command(`setblock ${power} ae2:creative_energy_cell`);
  await fixture.command(`setblock ${drive} ae2:drive`);
  await fixture.command(`setblock ${host} ae2:cable_bus`);
  await fixture.command(`item replace block ${drive} container.0 with ae2:item_storage_cell_1k`);
  assert.equal((await fixture.ae(`grid part ${host} up`)).attached, true);
  await fixture.command(`setblock ${hive} minecraft:beehive[facing=north,honey_level=4]`);
  await fixture.command(`data merge block ${hive} {bees:[{entity_data:{id:"minecraft:bee",CustomName:"Queen"},ticks_in_hive:0,min_ticks_in_hive:6000}]}`);
  assert.match(JSON.stringify(await fixture.command(`data get block ${hive}`)), /Queen/);
  await fixture.stepServer(300);
  assert((await fixture.command(`execute if block ${hive} minecraft:air`)).chat?.includes('Test passed'));
  const grid = await fixture.ae(`grid status ${drive}`);
  const index = grid.items.findIndex((entry: any) => entry.item === 'minecraft:beehive');
  assert(index >= 0 && grid.items[index].count === 1);
  assert.equal((await fixture.ae(`grid withdraw ${drive} ${index} 1`)).extracted, 1);
  const item = JSON.stringify(await fixture.command('data get entity @s Inventory'));
  assert.match(item, /minecraft:bees/);
  assert.match(item, /Queen/);
  assert.match(item, /minecraft:block_state/);
  assert.match(item, /honey_level.{0,12}4/);
  fixture.mark('one hive item carries its actual resident bee and honey level');
  await fixture.hold(false); await fixture.restart();
  const restored = await until(() => fixture.command('data get entity @s Inventory'), reply =>
    JSON.stringify(reply).includes('Queen'), 'hive item restored', 30000);
  assert.match(JSON.stringify(restored), /minecraft:bees/);
  assert.match(JSON.stringify(restored), /honey_level.{0,12}4/);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('the component-bearing hive survives restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
