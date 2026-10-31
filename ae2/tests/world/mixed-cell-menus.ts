/** Mixed source cell contents remain visible and recoverable in both menus. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('mixed-cell-menus', mode);
const itemCell = 'ae2:item_storage_cell_1k[ae2:storage_cell_inv=[{"#t":"ae2:i",id:"minecraft:iron_ingot","#":3L},{"#t":"ae2:f",id:"minecraft:water","#":2000L}]]';
const portable = 'ae2:portable_fluid_cell_1k[ae2:stored_energy=10000.0d,ae2:storage_cell_inv=[{"#t":"ae2:f",id:"minecraft:water","#":1000L},{"#t":"ae2:i",id:"minecraft:diamond","#":3L}]]';
const itemPortable = 'ae2:portable_item_cell_1k[ae2:stored_energy=10000.0d,ae2:storage_cell_inv=[{"#t":"ae2:i",id:"minecraft:coal","#":1L},{"#t":"ae2:f",id:"minecraft:water","#":1000L}]]';
const entry = (view: any, id: string) => view.slots?.find((item: any) => item.slot >= 300 && item.slot < 345 && item.item === id);
try {
  const start = await fixture.start(), [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 24, y = Math.ceil(py) + 5, z = Math.floor(pz) + 24, chest = `${x + 1} ${y} ${z}`;
  await fixture.command(`tp @s ${x + 1.5} ${y} ${z + 2.5}`);
  for (const [dx, dz] of [[-2, -3], [-2, 3], [6, -3], [6, 3]] as const) {
    await until(() => fixture.raw(`block ${x + dx} ${y} ${z + dz}`), state => state.loaded, 'fixture corner chunk loaded', 30000);
  }
  await fixture.command(`fill ${x - 2} ${y} ${z - 3} ${x + 6} ${y + 4} ${z + 3} minecraft:air`);
  await fixture.command(`fill ${x - 2} ${y - 1} ${z - 3} ${x + 6} ${y - 1} ${z + 3} minecraft:stone`);
  await fixture.command(`setblock ${x} ${y} ${z} ae2:creative_energy_cell`); await fixture.command(`setblock ${chest} ae2:chest`);
  await fixture.command(`item replace block ${chest} container.0 with ${itemCell}`);
  await until(() => fixture.ae(`grid status ${chest}`), state => state.online && state.powered, 'powered ME Chest', 30000);
  await fixture.command('clear @s'); await fixture.command('gamemode survival @s');
  await fixture.replace('hotbar.0', 'minecraft:bucket'); await fixture.replace('hotbar.2', portable);
  await fixture.replace('hotbar.3', 'minecraft:bucket'); await fixture.replace('hotbar.4', 'minecraft:bucket');
  await fixture.replace('hotbar.6', 'minecraft:gold_ingot', 2); await fixture.replace('hotbar.7', itemPortable);
  await fixture.replace('hotbar.8', 'minecraft:stick'); await fixture.select(8, 'minecraft:stick');
  await fixture.command(`tp @s ${x + 1.5} ${y} ${z + 2.5}`); await fixture.raw(`interact ${chest} south`);
  let view = await until(() => fixture.raw('screen'), state => entry(state, 'minecraft:iron_ingot') && entry(state, 'minecraft:water_bucket'), 'ME Chest displays native items and foreign stored fluid');
  let water = entry(view, 'minecraft:water_bucket'); await fixture.raw('click-slot 0'); await fixture.raw(`click-slot ${water.slot}`);
  await until(() => fixture.raw('inventory'), state => state.carried?.item === 'minecraft:water_bucket', 'foreign water fills a real bucket');
  await fixture.raw('click-slot 0'); await until(() => fixture.raw('inventory'), state => !state.carried && state.hotbar[0]?.item === 'minecraft:water_bucket', 'filled bucket returned to player');
  view = await fixture.raw('screen'); water = entry(view, 'minecraft:water_bucket'); assert(water, 'one stored bucket of foreign water remains');
  await fixture.raw('click-slot 0'); await fixture.raw(`click-slot ${water.slot} right`); await fixture.command(`data get block ${chest}`);
  await until(() => fixture.raw('inventory'), state => state.carried?.item === 'minecraft:water_bucket', 'item cell refuses foreign fluid insertion');
  await fixture.raw('click-slot 0'); await fixture.raw('click-slot 6');
  await until(() => fixture.raw('inventory'), state => state.carried?.item === 'minecraft:gold_ingot' && state.carried.count === 2, 'native item insertion sentinel held');
  view = await fixture.raw('screen'); water = entry(view, 'minecraft:water_bucket'); assert(water);
  await fixture.raw(`click-slot ${water.slot}`);
  assert.doesNotMatch(JSON.stringify(await fixture.command(`data get block ${chest}`)), /minecraft:gold_ingot/);
  await until(() => fixture.raw('inventory'), state => state.carried?.item === 'minecraft:gold_ingot' && state.carried.count === 2, 'fluid entry does not consume a normal carried item');
  await fixture.raw('click-slot 310');
  view = await until(() => fixture.raw('screen'), state => entry(state, 'minecraft:gold_ingot') && !state.carried, 'native item inserts through an empty virtual entry');
  await fixture.raw(`click-slot ${entry(view, 'minecraft:gold_ingot').slot}`);
  await until(() => fixture.raw('inventory'), state => state.carried?.item === 'minecraft:gold_ingot' && state.carried.count === 2, 'inserted gold is recovered once');
  await fixture.raw('click-slot 6'); view = await fixture.raw('screen'); const iron = entry(view, 'minecraft:iron_ingot'); assert(iron);
  await fixture.raw(`click-slot ${iron.slot}`); await until(() => fixture.raw('inventory'), state => state.carried?.item === 'minecraft:iron_ingot' && state.carried.count === 3, 'native item extraction retains its count');
  await fixture.raw('click-slot 1'); await fixture.raw('close');
  const chestData = JSON.stringify(await fixture.command(`data get block ${chest}`)); assert.match(chestData, /minecraft:water/); assert.match(chestData, /1000L/); assert.doesNotMatch(chestData, /minecraft:iron_ingot/);

  await fixture.select(2, 'ae2:portable_fluid_cell_1k'); await fixture.raw('look 0 -80'); await fixture.raw('use-item');
  view = await until(() => fixture.raw('screen'), state => entry(state, 'minecraft:water_bucket'), 'portable fluid cell displays its own fluid');
  assert.equal(entry(view, 'minecraft:diamond'), undefined, 'portable fluid host filters the stored item key');
  assert.match(JSON.stringify(await fixture.command('data get entity @s SelectedItem')), /minecraft:diamond/, 'hidden foreign item stays in the physical cell');
  await fixture.raw('click-slot 3'); view = await fixture.raw('screen'); water = entry(view, 'minecraft:water_bucket'); assert(water);
  await fixture.raw(`click-slot ${water.slot}`); await until(() => fixture.raw('inventory'), state => state.carried?.item === 'minecraft:water_bucket', 'native portable fluid fills a real bucket');
  await fixture.raw('close'); await until(() => fixture.raw('inventory'), state => !state.carried && state.counts?.['minecraft:water_bucket'] === 2, 'portable cursor returned');
  assert.match(JSON.stringify(await fixture.command('data get entity @s SelectedItem')), /minecraft:diamond/, 'foreign item survives native fluid extraction');

  await fixture.select(7, 'ae2:portable_item_cell_1k'); await fixture.raw('look 0 -80'); await fixture.raw('use-item');
  view = await until(() => fixture.raw('screen'), state => entry(state, 'minecraft:coal'), 'portable item cell displays its own item');
  assert.equal(entry(view, 'minecraft:water_bucket'), undefined, 'portable item host filters stored fluid');
  assert.match(JSON.stringify(await fixture.command('data get entity @s SelectedItem')), /minecraft:water/, 'hidden foreign fluid stays in the physical cell');
  await fixture.raw('click-slot 6'); await until(() => fixture.raw('inventory'), state => state.carried?.item === 'minecraft:gold_ingot', 'portable insertion sentinel held');
  await fixture.raw('click-slot 310');
  view = await until(() => fixture.raw('screen'), state => entry(state, 'minecraft:gold_ingot') && !state.carried, 'portable item inserts through an empty virtual entry');
  await fixture.raw(`click-slot ${entry(view, 'minecraft:gold_ingot').slot}`);
  await until(() => fixture.raw('inventory'), state => state.carried?.item === 'minecraft:gold_ingot' && state.carried.count === 2, 'portable inserted gold recovered');
  await fixture.raw('click-slot 6'); await fixture.raw('close');
  assert.match(JSON.stringify(await fixture.command('data get entity @s SelectedItem')), /minecraft:water/, 'foreign fluid survives native item transfers');
  await fixture.restart();
  const saved = await fixture.raw('inventory'); assert.equal(saved.counts?.['minecraft:iron_ingot'], 3); assert.equal(saved.counts?.['minecraft:gold_ingot'], 2); assert.equal(saved.counts?.['minecraft:water_bucket'], 2); assert.equal(saved.counts?.['minecraft:bucket'], 1);
  assert.match(JSON.stringify(await fixture.command(`data get block ${chest}`)), /1000L/);
  await fixture.select(2, 'ae2:portable_fluid_cell_1k'); assert.match(JSON.stringify(await fixture.command('data get entity @s SelectedItem')), /minecraft:diamond/);
  await fixture.select(7, 'ae2:portable_item_cell_1k'); assert.match(JSON.stringify(await fixture.command('data get entity @s SelectedItem')), /minecraft:water/);
  fixture.assertHidden(await fixture.raw('status')); fixture.mark('ME Chest extracts mixed contents; portable views filter and preserve foreign keys across native transfers and restart'); fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
