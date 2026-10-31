/** PortableCellMenuHost keeps its menu open while its idle buffer is unpowered. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('portable-menu-status', mode);
const cell = (id: string, power: number) => `ae2:portable_item_cell_1k[ae2:stored_energy=${power}.0d,ae2:storage_cell_inv=[{"#t":"ae2:i",id:"minecraft:${id}","#":3L}]]`;
const entry = (view: any, id: string) => view.slots?.find((item: any) => item.slot >= 300 && item.slot < 345 && item.item === id);
try {
  await fixture.start(); await fixture.command('clear @s'); await fixture.command('gamemode survival @s');
  await fixture.replace('hotbar.0', cell('diamond', 0)); await fixture.replace('hotbar.1', 'minecraft:gold_ingot', 2);
  await fixture.replace('hotbar.2', 'minecraft:coal', 2); await fixture.select(0, 'ae2:portable_item_cell_1k');
  await fixture.raw('look 0 -80'); await fixture.raw('use-item');
  const expectStatus = async (code: number, description: string | null) => {
    const view = await until(() => fixture.raw('screen'), state => state.screen === 'container' && state.portable_status?.code === code, `portable link status ${code}`, 30000);
    assert.equal(view.portable_status.description, description); return view;
  };
  let view = await expectStatus(4, 'Out of Power'); assert.equal(entry(view, 'minecraft:diamond'), undefined);
  assert.match(JSON.stringify(await fixture.command('data get entity @s SelectedItem')), /minecraft:diamond/, 'unpowered contents remain in the physical cell');
  await fixture.raw('click-slot 1'); await until(() => fixture.raw('inventory'), state => state.carried?.item === 'minecraft:gold_ingot', 'ordinary player slot remains usable');
  await fixture.raw('click-slot 310'); await fixture.command('data get entity @s SelectedItem');
  let inventory = await fixture.raw('inventory'); assert.equal(inventory.carried?.count, 2); assert.equal(inventory.carried?.item, 'minecraft:gold_ingot');
  await fixture.raw('click-slot 1'); await fixture.raw('click-slot 2 shift'); await fixture.command('data get entity @s SelectedItem');
  inventory = await fixture.raw('inventory'); assert.equal(inventory.hotbar?.[2]?.count, 2, 'unpowered shift-click does not deposit items');

  await fixture.replace('hotbar.0', cell('emerald', 10000)); view = await expectStatus(0, null);
  await until(() => fixture.raw('screen'), state => entry(state, 'minecraft:emerald'), 'charged replacement reconnects and shows its actual contents');
  await fixture.raw('click-slot 1'); await fixture.raw('click-slot 310');
  view = await until(() => fixture.raw('screen'), state => entry(state, 'minecraft:gold_ingot') && !state.carried, 'connected cell accepts the physical item');
  await fixture.raw(`click-slot ${entry(view, 'minecraft:gold_ingot').slot}`);
  await until(() => fixture.raw('inventory'), state => state.carried?.item === 'minecraft:gold_ingot' && state.carried.count === 2, 'connected cell returns the same item');
  await fixture.raw('click-slot 1');
  await fixture.hold(); await fixture.replace('hotbar.0', cell('emerald', 0)); await fixture.stepServer(11);
  view = await expectStatus(4, 'Out of Power'); assert.equal(entry(view, 'minecraft:emerald'), undefined, 'the view disconnects after the buffered ticks expire');
  await fixture.hold(false); await fixture.raw('close'); await fixture.restart();
  await fixture.select(0, 'ae2:portable_item_cell_1k'); await fixture.raw('look 0 -80'); await fixture.raw('use-item');
  view = await expectStatus(4, 'Out of Power'); assert.equal(entry(view, 'minecraft:emerald'), undefined);
  assert.match(JSON.stringify(await fixture.command('data get entity @s SelectedItem')), /minecraft:emerald/, 'unpowered contents persist across restart');
  fixture.assertHidden(await fixture.raw('status')); fixture.mark('portable menus retain the item and cursor while power loss hides and disables storage, then reconnect'); fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
