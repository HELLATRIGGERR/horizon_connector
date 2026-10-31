/** Portable ItemMenuHost type locator and CONFIG-scaled idle power. */
import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('portable-menu-lifecycle', mode);
const dir = join(fixture.game, 'config'); mkdirSync(dir, { recursive: true });
writeFileSync(join(dir, 'ae2-common.toml'), '[powerRatios]\nusageMultiplier=3\n');
const cell = (id: string) => `ae2:portable_item_cell_1k[ae2:stored_energy=1000.0d,ae2:storage_cell_inv=[{"#t":"ae2:i",id:"minecraft:${id}","#":3L}]]`;
const shown = (view: any, id: string) => view.slots?.some((entry: any) => entry.slot >= 300 && entry.slot < 345 && entry.item === id);
const energy = (reply: any) => {
  const match = (reply.chat ?? []).join('\n').match(/"ae2:stored_energy":\s*(-?[0-9]+(?:\.[0-9]+)?)(?:d)?/);
  assert(match, `missing physical cell energy: ${JSON.stringify(reply)}`); return Number(match[1]);
};
try {
  await fixture.start(); await fixture.command('clear @s'); await fixture.command('gamemode survival @s');
  await fixture.replace('hotbar.0', cell('emerald')); await fixture.replace('hotbar.1', 'minecraft:diamond');
  await fixture.select(0, 'ae2:portable_item_cell_1k'); await fixture.raw('look 0 -80'); await fixture.raw('use-item');
  await until(() => fixture.raw('screen'), view => view.screen === 'container' && shown(view, 'minecraft:emerald'), 'portable cell physical contents');
  await fixture.hold();
  const before = energy(await fixture.command('data get entity @s SelectedItem'));
  await fixture.stepServer(11);
  const after = energy(await fixture.command('data get entity @s SelectedItem'));
  assert.equal(before - after, 15, 'one ten-tick nominal idle buffer consumes 15 AE with usageMultiplier=3');
  await fixture.hold(false);
  await fixture.raw('click-slot 1'); await until(() => fixture.raw('inventory'), state => state.carried?.item === 'minecraft:diamond', 'portable owner-change sentinel held');
  await fixture.replace('hotbar.0', cell('gold_ingot'));
  await until(() => fixture.raw('screen'), view => view.screen === 'container' && shown(view, 'minecraft:gold_ingot') && !shown(view, 'minecraft:emerald'), 'same item type follows the replacement contents');
  assert.equal((await fixture.raw('inventory')).carried?.item, 'minecraft:diamond');
  await fixture.replace('hotbar.0', 'ae2:portable_fluid_cell_1k[ae2:stored_energy=1000.0d]');
  await until(() => fixture.raw('screen'), view => view.screen === 'playing', 'different portable item type closes the original menu');
  let inventory = await fixture.raw('inventory');
  assert.equal(inventory.carried, null); assert.equal(inventory.counts?.['minecraft:diamond'], 1);
  assert.equal(inventory.hotbar?.[0]?.item, 'ae2:portable_fluid_cell_1k');
  await fixture.restart(); inventory = await fixture.raw('inventory');
  assert.equal(inventory.counts?.['minecraft:diamond'], 1); assert.equal(inventory.hotbar?.[0]?.item, 'ae2:portable_fluid_cell_1k');
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('portable idle power uses CONFIG, same-type replacement stays open and a different type returns the cursor once');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
