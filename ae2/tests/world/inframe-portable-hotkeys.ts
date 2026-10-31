/** Source inventory hotkeys open the ranked, unselected portable cell. */
import assert from 'node:assert/strict';
import { appendFileSync } from 'node:fs';
import { join } from 'node:path';
import { WorldFixture, until } from './harness';

const fixture = new WorldFixture('inframe-portable-hotkeys', 'in-frame');
appendFileSync(join(fixture.game, 'options.txt'), 'version:5023\nkey_key.ae2.portable_item_cell:key.keyboard.o\nkey_key.ae2.portable_fluid_cell:key.keyboard.i\n');
const slot = (view: any, index: number) => view.slots?.find((entry: any) => entry.slot === index);
const itemCell = (size: string, id: string, amount: number) => `ae2:portable_item_cell_${size}[ae2:stored_energy=10000.0d,ae2:storage_cell_inv=[{"#t":"ae2:i",id:"${id}","#":${amount}L}]]`;
const fluidCell = 'ae2:portable_fluid_cell_1k[ae2:stored_energy=10000.0d,ae2:storage_cell_inv=[{"#t":"ae2:f",id:"minecraft:water","#":1000L}]]';
try {
  await fixture.start(); await fixture.command('clear @s'); await fixture.command('gamemode survival @s');
  await fixture.replace('hotbar.0', 'minecraft:stick');
  await fixture.replace('hotbar.1', itemCell('1k', 'minecraft:iron_ingot', 3));
  await fixture.replace('hotbar.2', itemCell('4k', 'minecraft:gold_ingot', 5));
  await fixture.replace('hotbar.3', fluidCell); await fixture.replace('hotbar.4', 'minecraft:bucket');
  await fixture.select(0, 'minecraft:stick');
  await fixture.raw('key o tap');
  await until(() => fixture.raw('screen'), view => slot(view, 300)?.item === 'minecraft:gold_ingot' && slot(view, 300)?.count === 5, 'item hotkey selects higher-ranked 4k cell without selecting it in the hotbar');
  await fixture.raw('click-slot 300');
  await until(() => fixture.raw('inventory'), view => view.carried?.item === 'minecraft:gold_ingot' && view.carried.count === 5, 'real stored gold is extracted to the cursor');
  await fixture.raw('close');
  await until(() => fixture.raw('inventory'), view => !view.carried && view.counts?.['minecraft:gold_ingot'] === 5, 'closing returns the gold once');
  assert.equal((await fixture.raw('inventory')).held?.item, 'minecraft:stick');
  await fixture.raw('key i tap');
  await until(() => fixture.raw('screen'), view => slot(view, 300)?.item === 'minecraft:water_bucket', 'fluid hotkey selects the separate unheld fluid cell');
  await fixture.raw('click-slot 4'); await fixture.raw('click-slot 300');
  await until(() => fixture.raw('inventory'), view => view.carried?.item === 'minecraft:water_bucket' && view.carried.count === 1, 'fluid cell fills the real bucket once');
  await fixture.raw('close'); await until(() => fixture.raw('inventory'), view => !view.carried && view.counts?.['minecraft:water_bucket'] === 1, 'filled bucket returns on close');
  await fixture.restart();
  const saved = await fixture.raw('inventory');
  assert.equal(saved.held?.item, 'minecraft:stick'); assert.equal(saved.counts?.['minecraft:gold_ingot'], 5); assert.equal(saved.counts?.['minecraft:water_bucket'], 1);
  assert.equal(saved.hotbar?.[1]?.item, 'ae2:portable_item_cell_1k'); assert.equal(saved.hotbar?.[2]?.item, 'ae2:portable_item_cell_4k'); assert.equal(saved.hotbar?.[3]?.item, 'ae2:portable_fluid_cell_1k');
  await fixture.raw('key o tap'); await until(() => fixture.raw('screen'), view => slot(view, 300) && !slot(view, 300)?.item, 'ranked cell reopens empty after restart');
  fixture.assertHidden(await fixture.raw('status')); fixture.mark('ranked item and fluid hotkeys preserve selected slot, physical contents, cursor and restart'); fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
