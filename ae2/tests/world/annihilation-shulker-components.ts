/** Shulker loot components follow the actual block incarnation across saves. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('annihilation-shulker-components', mode);
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 6, y = Math.ceil(py) + 6, z = Math.floor(pz) + 6;
  const at = (dx = 0, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const host = at(), power = at(-1), drive = at(1), target = at(0, 1);
  const blockData = () => fixture.command(`data get block ${target}`);
  const inventory = async () => JSON.stringify(await fixture.command('data get entity @s Inventory'));
  const installCell = () => fixture.command(`item replace block ${drive} container.0 with ae2:item_storage_cell_1k`);
  const retrieve = async () => {
    await fixture.stepServer(300);
    assert((await fixture.command(`execute if block ${target} minecraft:air`)).chat?.includes('Test passed'));
    const grid = await fixture.ae(`grid status ${drive}`);
    const index = grid.items.findIndex((entry: any) => entry.item === 'minecraft:shulker_box');
    assert(index >= 0 && grid.items[index].count === 1, 'one physical shulker block drop');
    assert.equal((await fixture.ae(`grid withdraw ${drive} ${index} 1`)).extracted, 1);
  };

  await fixture.command(`fill ${at(-3, -2, -3)} ${at(4, -2, 3)} minecraft:stone`);
  await fixture.command(`fill ${at(-3, -1, -3)} ${at(4, 3, 3)} minecraft:air`);
  await fixture.command(`tp @s ${x + 2.5} ${y + 1} ${z - 2.5}`);
  await fixture.command('clear @s');
  await fixture.replace('hotbar.0', 'ae2:annihilation_plane');
  await fixture.select(0, 'ae2:annihilation_plane');
  await fixture.hold();
  await fixture.command(`setblock ${power} ae2:creative_energy_cell`);
  await fixture.command(`setblock ${drive} ae2:drive`);
  await fixture.command(`setblock ${host} ae2:cable_bus`);
  assert.equal((await fixture.ae(`grid part ${host} up`)).attached, true);
  await fixture.command(`setblock ${target} minecraft:shulker_box`);
  await fixture.command(`data merge block ${target} {CustomName:{text:"Owned",color:"red"},lock:{components:{"minecraft:custom_name":"Key"}},Items:[{Slot:0b,id:"minecraft:emerald",count:3,components:{"minecraft:custom_data":{shulker_marker:41}}}]}`);
  assert.match(JSON.stringify(await blockData()), /shulker_marker/);
  await fixture.hold(false); await fixture.restart(); await fixture.hold();
  const loaded = JSON.stringify(await blockData());
  assert.match(loaded, /shulker_marker/);
  assert.match(loaded, /color.{0,12}red/);
  assert.match(loaded, /lock/);
  // This edit changes only the rich component and lock, after the chunk has
  // already been saved. An unchanged block state must still be marked dirty.
  await fixture.command(`data merge block ${target} {CustomName:{text:"Owned",color:"blue"},lock:{components:{"minecraft:custom_name":"New Key"}}}`);
  await fixture.hold(false); await fixture.restart(); await fixture.hold();
  const edited = JSON.stringify(await blockData());
  assert.match(edited, /color.{0,12}blue/);
  assert.match(edited, /New Key/);
  assert.match(edited, /shulker_marker/);
  await installCell(); await retrieve();
  const first = await inventory();
  assert.match(first, /shulker_marker/);
  assert.match(first, /color.{0,12}blue/);
  assert.match(first, /minecraft:lock/);
  fixture.mark('a metadata-only edit and the shulker drop retain styled name, lock, and inventory through restart');

  await fixture.command('clear @s minecraft:shulker_box');
  await fixture.command(`setblock ${target} minecraft:shulker_box`);
  await fixture.command(`data merge block ${target} {Items:[{Slot:0b,id:"minecraft:diamond",count:1,components:{"minecraft:custom_data":{replacement_marker:42}}}]}`);
  await retrieve();
  const replacement = await inventory();
  assert.match(replacement, /replacement_marker/);
  assert.doesNotMatch(replacement, /minecraft:lock|shulker_marker|color.{0,12}blue/);
  fixture.mark('a replacement at the same position does not inherit the previous block entity metadata');

  await fixture.command('clear @s minecraft:shulker_box');
  const cloneSource = at(3, 1);
  await fixture.command(`item replace block ${drive} container.0 with minecraft:air`);
  await fixture.command(`setblock ${target} minecraft:shulker_box`);
  await fixture.command(`setblock ${cloneSource} minecraft:shulker_box`);
  await fixture.command(`data merge block ${cloneSource} {CustomName:{text:"Cloned",color:"blue"},lock:{components:{"minecraft:custom_name":"Other Key"}},Items:[{Slot:0b,id:"minecraft:diamond",count:1,components:{"minecraft:custom_data":{clone_marker:43}}}]}`);
  // Both ends are saved before cloning onto the same block state, so only
  // the copied entity changes and must dirty the destination by itself.
  await fixture.hold(false); await fixture.restart(); await fixture.hold();
  await fixture.command(`clone ${cloneSource} ${cloneSource} ${target}`);
  await fixture.hold(false); await fixture.restart(); await fixture.hold();
  assert.match(JSON.stringify(await blockData()), /clone_marker/);
  await installCell();
  await retrieve();
  const cloned = await inventory();
  assert.match(cloned, /clone_marker/);
  assert.match(cloned, /minecraft:lock/);
  assert.match(cloned, /color.{0,12}blue/);
  assert.match(JSON.stringify(await fixture.command(`data get block ${cloneSource}`)), /clone_marker/, 'clone leaves its physical source intact');
  fixture.mark('cloning onto an unchanged block state carries its own metadata through restart and leaves the source intact');

  await fixture.command('clear @s minecraft:shulker_box');
  await fixture.command(`item replace block ${drive} container.0 with minecraft:air`);
  assert.doesNotMatch(JSON.stringify(await fixture.command(`data get block ${drive}`)), /ae2:item_storage_cell_1k/, 'storage is removed before the reload setup');
  await fixture.command(`setblock ${target} minecraft:shulker_box`);
  await fixture.command(`data merge block ${target} {LootTable:"minecraft:chests/simple_dungeon",LootTableSeed:12L}`);
  await fixture.hold(false); await fixture.restart(); await fixture.hold();
  assert.match(JSON.stringify(await blockData()), /minecraft:chests\/simple_dungeon/);
  await installCell(); await retrieve();
  const unopened = await inventory();
  assert.match(unopened, /minecraft:container_loot/);
  assert.match(unopened, /minecraft:chests\/simple_dungeon/);
  assert(!(await fixture.ae(`grid status ${drive}`)).items.some((entry: any) => entry.item !== 'minecraft:shulker_box'), 'unopened rolled contents are not duplicated as separate drops');
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('an unopened shulker loot table remains on the item rather than duplicating rolled contents');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
