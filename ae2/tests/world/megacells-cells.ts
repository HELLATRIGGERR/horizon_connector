/** MEGA Cells slice 1: Sky Steel from its lava transformation, an M-tier cell from MEGA's own
 * recipe, and 1M-256M cells in an ME Drive and an ME Chest with MEGA's capacities and type limits,
 * listed in terminals and kept through a restart. Needs content/megacells
 * (ae2/scripts/prepare-addon.ts megacells) and a client built with ae2-megacells. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('megacells-cells', mode, { addons: ['megacells'] });
const count = (view: any, item: string) => view.items?.find((entry: any) => entry.item === item)?.count ?? 0;
const fluid = (view: any, id: string) => view.entries?.find((entry: any) => entry.fluid === id)?.amount_mb ?? 0;
const entities = async (id: string) => ((await fixture.raw('entities 48 item')).entities ?? []).filter((e: any) => e.item?.item === id);
// A 256M cell: 2^28 bytes, 2^21 of them for each type; 8 items or 8,000 mB a byte.
const ITEMS_256M = (2 ** 28 - 2 ** 21) * 8, FLUID_256M = (2 ** 28 - 2 ** 21) * 8000;
const colours = ['white', 'orange', 'magenta', 'light_blue', 'yellow', 'lime', 'pink', 'gray', 'light_gray', 'cyan', 'purple', 'blue', 'brown', 'green', 'red', 'black'];
const kinds = colours.flatMap(c => [`minecraft:${c}_wool`, `minecraft:${c}_concrete`, `minecraft:${c}_terracotta`, `minecraft:${c}_stained_glass`]);
const stored = (entries: [string, string, number][]) => '[' + entries.map(([t, id, n]) => `{"#t":"${t}",id:"${id}","#":${n}L}`).join(',') + ']';
try {
  const start = await fixture.start(), [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 24, y = Math.ceil(py) + 5, z = Math.floor(pz) + 24;
  const at = (dx: number, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  for (const [dx, dz] of [[-3, -4], [-3, 6], [10, -4], [10, 6]] as const) {
    await until(async () => (await fixture.command(`execute if loaded ${at(dx, 0, dz)}`)).chat ?? [], lines => lines.includes('Test passed'), 'loaded fixture corner', 30000);
  }
  await fixture.command(`fill ${at(-3, 0, -4)} ${at(10, 4, 6)} minecraft:air`);
  await fixture.command(`fill ${at(-3, -3, -4)} ${at(10, -1, 6)} minecraft:stone`);
  await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`);

  // Sky Steel: MEGA's ae2:transform recipe in lava. Its ingredients would burn in two ticks; MEGA keeps them alive.
  await fixture.command(`setblock ${at(8, -2, 2)} minecraft:lava`); await fixture.command(`setblock ${at(8, -1, 2)} minecraft:lava`);
  for (const id of ['ae2:charged_certus_quartz_crystal', 'minecraft:iron_ingot', 'ae2:sky_stone_block', 'minecraft:gold_ingot']) {
    await fixture.command(`summon minecraft:item ${x + 8.5} ${y - 1.4} ${z + 2.5} {Item:{id:"${id}",count:1},PickupDelay:32767s}`);
  }
  const steel = await until(() => entities('megacells:sky_steel_ingot'), list => list.some((e: any) => e.item.count === 2), 'lava transformation makes two Sky Steel Ingots', 30000);
  for (const id of ['ae2:charged_certus_quartz_crystal', 'minecraft:iron_ingot', 'ae2:sky_stone_block', 'minecraft:gold_ingot']) assert.equal((await entities(id)).length, 0, `${id} is gone`);
  assert(steel[0].item.count === 2);
  fixture.mark('three ingredients survive lava long enough for MEGA\'s Sky Steel recipe; a gold ingot burns');

  // An M-tier cell from MEGA's shapeless recipe, in the survival inventory's crafting grid.
  await fixture.command('clear @s'); await fixture.command('gamemode survival @s');
  await until(() => fixture.raw('status'), s => s.creative === false, 'survival inventory');
  await fixture.replace('hotbar.0', 'megacells:mega_item_cell_housing'); await fixture.replace('hotbar.1', 'megacells:cell_component_1m');
  await fixture.raw('open-inventory');
  await until(() => fixture.raw('screen'), s => s.slots?.some((slot: any) => slot.slot === 100), 'inventory crafting grid');
  for (const [from, to] of [[0, 100], [1, 101]]) {
    await fixture.raw(`click-slot ${from}`); await until(() => fixture.raw('inventory'), s => !!s.carried, 'ingredient on the cursor');
    await fixture.raw(`click-slot ${to}`); await until(() => fixture.raw('inventory'), s => !s.carried, 'ingredient in the grid');
  }
  await until(() => fixture.raw('screen'), s => s.slots?.some((slot: any) => slot.slot === 200 && slot.item === 'megacells:item_storage_cell_1m'), 'the recipe offers a 1M MEGA Item Storage Cell');
  await fixture.raw('click-slot 200 shift'); await fixture.raw('close');
  const crafted = await until(() => fixture.raw('inventory'), s => s.counts?.['megacells:item_storage_cell_1m'] === 1, 'the crafted cell is in the inventory');
  assert(!crafted.counts?.['megacells:mega_item_cell_housing'] && !crafted.counts?.['megacells:cell_component_1m'], 'both ingredients were used');
  const cellSlot = crafted.hotbar.findIndex((s: any) => s?.item === 'megacells:item_storage_cell_1m');
  assert(cellSlot >= 0, 'the crafted cell is on the hotbar');
  // Hotbar slots for what follows, around the crafted cell.
  const [cobbleSlot, newSlot, oldSlot, bucketSlot] = [2, 3, 4, 5, 6, 7].filter(i => i !== cellSlot);
  fixture.mark('MEGA\'s own recipe crafts a 1M item cell from its housing and component');

  // ME Drive: a full 256M item cell, a 1M cell holding 63 types and the crafted empty 1M cell.
  const drive = at(1), terminal = at(2);
  await fixture.command(`setblock ${at(0)} ae2:creative_energy_cell`); await fixture.command(`setblock ${drive} ae2:drive`);
  await fixture.command(`setblock ${terminal} ae2:cable_bus`);
  const status = () => fixture.ae(`grid status ${drive}`);
  await until(status, s => s.online && s.powered, 'powered drive', 30000);
  await fixture.command(`item replace block ${drive} container.0 with megacells:item_storage_cell_256m[ae2:storage_cell_inv=${stored([['ae2:i', 'minecraft:cobblestone', ITEMS_256M - 32]])}]`);
  await fixture.command(`item replace block ${drive} container.1 with megacells:item_storage_cell_1m[ae2:storage_cell_inv=${stored(kinds.slice(0, 63).map(id => ['ae2:i', id, 1]))}]`);
  await until(status, s => count(s, 'minecraft:cobblestone') === ITEMS_256M - 32 && s.items.length === 64, '256M and 63-type cells are online', 30000);
  await fixture.replace(`hotbar.${cobbleSlot}`, 'minecraft:cobblestone', 64); await fixture.select(cobbleSlot, 'minecraft:cobblestone');
  assert.equal((await fixture.ae(`grid deposit ${drive}`)).inserted, 32, 'a 256M cell holds 2,130,706,432 of one item');
  const sixtyFourth = kinds[63];
  await fixture.replace(`hotbar.${newSlot}`, sixtyFourth, 4); await fixture.select(newSlot, sixtyFourth);
  assert.equal((await fixture.ae(`grid deposit ${drive}`)).inserted, 0, 'a 1M item cell holds 63 types');
  await fixture.replace(`hotbar.${oldSlot}`, kinds[0], 5); await fixture.select(oldSlot, kinds[0]);
  assert.equal((await fixture.ae(`grid deposit ${drive}`)).inserted, 5, 'a stored type still fits');
  await fixture.select(cellSlot, 'megacells:item_storage_cell_1m');
  assert.equal((await fixture.ae(`grid install ${drive} 2`)).installed, true);
  await fixture.select(newSlot, sixtyFourth);
  assert.equal((await fixture.ae(`grid deposit ${drive}`)).inserted, 4, 'the crafted cell takes the 64th type');
  let drives = await status();
  assert.equal(count(drives, 'minecraft:cobblestone'), ITEMS_256M); assert.equal(count(drives, kinds[0]), 6); assert.equal(count(drives, sixtyFourth), 4);
  fixture.mark('in an ME Drive a 256M cell stops at 2,130,706,432 items and a 1M cell at 63 types');

  await fixture.replace('hotbar.0', 'ae2:terminal'); await fixture.select(0, 'ae2:terminal');
  assert.equal((await fixture.ae(`grid part ${terminal} north`)).attached, true);
  await fixture.replace('hotbar.8', 'minecraft:stick'); await fixture.select(8, 'minecraft:stick');
  await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`); await fixture.raw(`interact ${terminal} north`);
  const listed = (view: any, id: string) => view.slots?.some((slot: any) => slot.slot >= 300 && slot.slot < 354 && slot.item === id);
  await until(() => fixture.raw('screen'), view => listed(view, 'minecraft:cobblestone') && listed(view, sixtyFourth), 'the ME Terminal lists the MEGA cells\' items', 30000);
  await fixture.raw('close');
  fixture.mark('an ME Terminal lists what the MEGA cells hold');

  // ME Chest: a 256M fluid cell one bucket short of full, then a 1M fluid cell with 18 types.
  const chest = at(4, 0, 3);
  await fixture.command(`setblock ${at(3, 0, 3)} ae2:creative_energy_cell`); await fixture.command(`setblock ${chest} ae2:chest`);
  await fixture.command(`item replace block ${chest} container.0 with megacells:fluid_storage_cell_256m[ae2:storage_cell_inv=${stored([['ae2:f', 'minecraft:water', FLUID_256M - 1000]])}]`);
  await until(() => fixture.ae(`grid status ${chest}`), s => s.online && s.powered, 'powered ME Chest', 30000);
  const fluids = () => fixture.ae(`fluid grid status ${chest}`);
  await until(fluids, s => fluid(s, 'minecraft:water') === FLUID_256M - 1000, 'ME Chest with a 256M fluid cell', 30000);
  const bucket = async () => { await fixture.replace(`hotbar.${bucketSlot}`, 'minecraft:water_bucket'); await fixture.select(bucketSlot, 'minecraft:water_bucket'); };
  await bucket(); assert.equal((await fixture.ae(`fluid grid insert ${chest}`)).inserted_mb, 1000);
  assert.equal(fluid(await fluids(), 'minecraft:water'), FLUID_256M, 'a 256M fluid cell holds 2,130,706,432 buckets');
  await bucket(); await assert.rejects(fixture.ae(`fluid grid insert ${chest}`), 'a full 256M fluid cell refuses a bucket');
  await fixture.select(8, 'minecraft:stick');
  await fixture.command(`tp @s ${x + 4.5} ${y} ${z + 5.5}`); await fixture.raw(`interact ${chest} south`);
  await until(() => fixture.raw('screen'), view => listed(view, 'minecraft:water_bucket'), 'the ME Chest\'s terminal lists the stored water', 30000);
  await fixture.raw('close');
  fixture.mark('in an ME Chest a 256M fluid cell stops at 2,130,706,432 buckets, and its terminal lists the fluid');

  const typed = (n: number) => `megacells:fluid_storage_cell_1m[ae2:storage_cell_inv=${stored(Array.from({ length: n }, (_, i) => ['ae2:f', `megacells_test:fluid_${i}`, 1000]))}]`;
  await fixture.command(`item replace block ${chest} container.0 with ${typed(18)}`);
  await until(fluids, s => s.entries?.length === 18, '1M fluid cell with 18 types', 30000);
  await bucket(); await assert.rejects(fixture.ae(`fluid grid insert ${chest}`), 'a 1M fluid cell holds 18 types');
  await fixture.command(`item replace block ${chest} container.0 with ${typed(17)}`);
  await until(fluids, s => s.entries?.length === 17, '1M fluid cell with 17 types', 30000);
  assert.equal((await fixture.ae(`fluid grid insert ${chest}`)).inserted_mb, 1000, 'the 18th type fits');
  fixture.mark('a 1M fluid cell holds 18 types');

  await fixture.restart();
  drives = await until(status, s => s.online && s.powered && count(s, 'minecraft:cobblestone') === ITEMS_256M, 'MEGA cells survive restart', 60000);
  await until(() => fixture.ae(`grid status ${chest}`), s => s.online && s.powered, 'powered ME Chest after restart', 30000);
  assert.equal(count(drives, kinds[0]), 6); assert.equal(count(drives, sixtyFourth), 4); assert.equal(drives.items.length, 65);
  const after = await until(fluids, s => s.entries?.length === 18, 'the chest\'s fluid cell survives restart', 30000);
  assert.equal(fluid(after, 'minecraft:water'), 1000);
  const data = (await fixture.command(`data get block ${drive}`)).chat?.join('\n') ?? '';
  assert.match(data, /megacells:item_storage_cell_256m/); assert.match(data, /ae2:storage_cell_inv/);
  fixture.mark('the drive\'s and chest\'s MEGA cells keep their contents in AE2\'s components through save and load');
  fixture.assertHidden(await fixture.raw('status')); fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; } finally { await fixture.close(); }
