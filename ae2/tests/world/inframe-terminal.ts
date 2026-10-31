/** In-frame ME terminal views, exact item custody and replacement identity. */
import assert from 'node:assert/strict';
import { WorldFixture, until } from './harness';

const fixture = new WorldFixture('inframe-terminal', 'in-frame');
const entry = (view: any, slot: number) => view.slots?.find((item: any) => item.slot === slot);
const stored = (view: any, id: string) => view.items?.find((item: any) => item.item === id)?.count ?? 0;

try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 32, y = Math.ceil(py) + 5, z = Math.floor(pz) + 32;
  const at = (dx: number) => `${x + dx} ${y} ${z}`;
  const drive = at(1), terminal = at(2), donor = at(3);
  await fixture.command(`fill ${x - 2} ${y} ${z - 3} ${x + 5} ${y + 4} ${z + 3} minecraft:air`);
  await fixture.command(`fill ${x - 2} ${y - 1} ${z - 3} ${x + 5} ${y - 1} ${z + 3} minecraft:stone`);
  for (const [pos, id] of [[at(0), 'creative_energy_cell'], [drive, 'drive'], [terminal, 'cable_bus'], [donor, 'cable_bus']] as const) {
    await fixture.command(`setblock ${pos} ae2:${id}`);
  }
  await fixture.command(`item replace block ${drive} container.0 with ae2:item_storage_cell_1k`);
  await fixture.command('clear @s');
  await fixture.replace('hotbar.0', 'ae2:terminal');
  await fixture.select(0, 'ae2:terminal');
  assert.equal((await fixture.ae(`grid part ${terminal} north`)).attached, true);
  assert.equal((await fixture.ae(`grid part ${donor} north`)).attached, true);
  const grid = () => until(() => fixture.ae(`grid status ${drive}`), state => state.online && state.powered, 'powered terminal grid', 30000);
  await grid();
  await fixture.command('gamemode survival @s');
  await fixture.replace('hotbar.0', 'minecraft:iron_ingot', 16);
  await fixture.select(0, 'minecraft:iron_ingot');
  assert.equal((await fixture.ae(`grid deposit ${drive}`)).inserted, 16);
  await fixture.replace('hotbar.8', 'minecraft:stick');
  await fixture.select(8, 'minecraft:stick');
  await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`);
  await fixture.raw(`interact ${terminal} north`);
  const view = await until(() => fixture.raw('screen'), state => JSON.stringify(state.title ?? '').includes('AE2 ME Terminal') && state.slots?.some((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:iron_ingot'), 'in-frame real storage view');
  const iron = view.slots.find((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:iron_ingot');
  assert.equal(iron.count, 16);
  await fixture.raw(`click-slot ${iron.slot}`);
  await until(() => fixture.raw('inventory'), state => state.carried?.item === 'minecraft:iron_ingot' && state.carried.count === 16, 'real stored stack taken to cursor');
  assert.equal(stored(await grid(), 'minecraft:iron_ingot'), 0);
  await fixture.raw('click-slot 0');
  await until(() => fixture.raw('inventory'), state => !state.carried && state.hotbar[0]?.item === 'minecraft:iron_ingot' && state.hotbar[0].count === 16, 'cursor stack placed in player inventory');
  await fixture.raw('click-slot 0 shift');
  await until(grid, state => stored(state, 'minecraft:iron_ingot') === 16, 'same stack returned to cell by shift click');
  assert.equal((await fixture.raw('inventory')).counts?.['minecraft:iron_ingot'] ?? 0, 0);
  fixture.mark('virtual entries and ordinary and shift clicks preserve exactly sixteen physical ingots');

  await fixture.replace('hotbar.0', 'minecraft:diamond');
  await fixture.raw('click-slot 0');
  await until(() => fixture.raw('inventory'), state => state.carried?.item === 'minecraft:diamond', 'cursor holds replacement sentinel');
  // The donor is a separate part identity. This command reloads the host too,
  // so it does not model a concurrent same-host survival removal.
  await fixture.command(`data modify block ${terminal} ae2_lab_config.parts[{face:"north"}] set from block ${donor} ae2_lab_config.parts[{face:"north"}]`);
  await until(() => fixture.raw('screen'), state => state.screen === 'playing', 'replacement closes original terminal');
  const inventory = await fixture.raw('inventory');
  assert.equal(inventory.carried, null);
  assert.equal(inventory.counts?.['minecraft:diamond'], 1);
  assert.equal(stored(await grid(), 'minecraft:iron_ingot'), 16);
  fixture.mark('replacement invalidates the original face and returns the cursor item exactly once');

  await fixture.restart();
  assert.equal(stored(await grid(), 'minecraft:iron_ingot'), 16);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('cell contents survive restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
