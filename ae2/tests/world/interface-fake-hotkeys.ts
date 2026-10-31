/** Source 64-limit Interface fake slots receive hotbar actions, not real swaps. */
import assert from 'node:assert/strict';
import { WorldFixture, until } from './harness';

const fixture = new WorldFixture('interface-fake-hotkeys', 'server-thread');
const slot = (view: any, index: number) => view.slots?.find((entry: any) => entry.slot === index);
try {
  const start = await fixture.start(); const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 24, y = Math.ceil(py) + 5, z = Math.floor(pz) + 24, iface = `${x} ${y} ${z}`;
  await fixture.command(`fill ${x - 2} ${y} ${z - 3} ${x + 2} ${y + 4} ${z + 3} minecraft:air`);
  await fixture.command(`fill ${x - 2} ${y - 1} ${z - 3} ${x + 2} ${y - 1} ${z + 3} minecraft:stone`);
  await fixture.command(`setblock ${iface} ae2:interface`); await fixture.command('clear @s'); await fixture.command('gamemode survival @s');
  await fixture.replace('hotbar.0', 'minecraft:diamond', 3); await fixture.replace('hotbar.1', 'minecraft:emerald');
  await fixture.replace('weapon.offhand', 'minecraft:gold_ingot'); await fixture.replace('hotbar.8', 'minecraft:stick'); await fixture.select(8, 'minecraft:stick');
  await fixture.command(`tp @s ${x + .5} ${y} ${z + 2.5}`); await fixture.raw(`interact ${iface} south`);
  await until(() => fixture.raw('screen'), view => /Interface|AE2 Machine/i.test(JSON.stringify(view.title ?? '')) && slot(view, 309), 'Interface menu');
  await fixture.raw('click-slot 0'); await until(() => fixture.raw('inventory'), view => view.carried?.item === 'minecraft:diamond' && view.carried.count === 3, 'physical diamond template');
  const widgets = await fixture.raw('ui'); const node = widgets.widgets.find((entry: any) => entry.id === 'slot-309'); assert(node, 'stock ghost hit target');
  await fixture.raw(`click-at ${node.bounds[0] + 8} ${node.bounds[1] + 8}`); await fixture.command(`data get block ${iface} ae2_lab_config`);
  await until(() => fixture.raw('screen'), view => slot(view, 309)?.item === 'minecraft:diamond' && slot(view, 309)?.count === 3 && view.carried?.count === 3, 'physical template configures stock');
  await fixture.raw('click-slot 0'); await until(() => fixture.raw('inventory'), view => !view.carried, 'cursor returned while pointer stays on the ghost');
  await fixture.raw('key 2 tap'); await fixture.command(`data get block ${iface} ae2_lab_config`);
  await until(() => fixture.raw('screen'), view => slot(view, 309)?.item === 'minecraft:diamond' && slot(view, 309)?.count === 2 && !view.carried, 'hotbar 2 decrements the 64-limit fake slot');
  let inventory = await fixture.raw('inventory'); assert.equal(inventory.hotbar?.[1]?.item, 'minecraft:emerald'); assert.equal(inventory.offhand?.item, 'minecraft:gold_ingot');
  await fixture.raw('key f tap'); await fixture.command(`data get block ${iface} ae2_lab_config`);
  await until(() => fixture.raw('screen'), view => !slot(view, 309)?.item && !view.carried, 'offhand key clears the ghost without a swap');
  await fixture.raw('close'); await fixture.restart(); inventory = await fixture.raw('inventory');
  assert.equal(inventory.counts?.['minecraft:diamond'], 3); assert.equal(inventory.counts?.['minecraft:emerald'], 1); assert.equal(inventory.offhand?.item, 'minecraft:gold_ingot');
  const saved = JSON.stringify((await fixture.command(`data get block ${iface} ae2_lab_config`)).chat); assert(!saved.includes('minecraft:diamond'));
  fixture.assertHidden(await fixture.raw('status')); fixture.mark('Interface fake hotkeys alter stock without moving the real hotbar, offhand or template'); fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
