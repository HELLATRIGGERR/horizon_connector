/** ItemMenuHost locator and disconnected wireless behavior on both routes. */
import assert from 'node:assert/strict';
import { appendFileSync } from 'node:fs';
import { join } from 'node:path';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('wireless-menu-lifecycle', mode);
appendFileSync(join(fixture.game, 'options.txt'), 'version:5023\nkey_key.ae2.wireless_terminal:key.keyboard.o\n');
const slot = (view: any, index: number) => view.slots?.find((entry: any) => entry.slot === index);
const shown = (view: any, item: string) => view.slots?.some((entry: any) => entry.slot >= 300 && entry.slot < 327 && entry.item === item);

try {
  const start = await fixture.start(), [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 24, y = Math.ceil(py) + 5, z = Math.floor(pz) + 24;
  const at = (dx: number) => `${x + dx} ${y} ${z}`, access = at(1), drive = at(-1);
  await fixture.command(`tp @s ${x + 1.5} ${y} ${z + 2.5}`);
  for (const [dx, dz] of [[-3, -3], [-3, 3], [4, -3], [4, 3]] as const) {
    await until(() => fixture.raw(`block ${x + dx} ${y} ${z + dz}`), state => state.loaded, 'fixture corner chunk loaded', 30000);
  }
  await fixture.command(`fill ${x - 3} ${y} ${z - 3} ${x + 4} ${y + 4} ${z + 3} minecraft:air`);
  await fixture.command(`fill ${x - 3} ${y - 1} ${z - 3} ${x + 4} ${y - 1} ${z + 3} minecraft:stone`);
  await fixture.command(`setblock ${at(0)} ae2:creative_energy_cell`);
  await fixture.command(`setblock ${access} ae2:wireless_access_point[facing=east]`);
  await fixture.command(`setblock ${drive} ae2:drive`);
  await fixture.command(`item replace block ${drive} container.0 with ae2:item_storage_cell_1k[ae2:storage_cell_inv=[{"#t":"ae2:i",id:"minecraft:iron_ingot","#":3L}]]`);
  await until(() => fixture.ae(`grid status ${access}`), state => state.online && state.powered, 'powered wireless access point', 30000);
  await fixture.command('clear @s'); await fixture.command('gamemode survival @s');
  const owner = (ingredient: string, linked = true, energy = 10000) => `ae2:wireless_crafting_terminal[ae2:stored_energy=${energy}.0d,${linked ? `ae2:wireless_link_target={dimension:"minecraft:overworld",pos:[I;${x + 1},${y},${z}]},` : ''}ae2:crafting_inv=[{slot:0,item:{id:"minecraft:${ingredient}",count:1}}]]`;
  await fixture.replace('hotbar.0', 'minecraft:stick');
  await fixture.replace('hotbar.1', owner('oak_log', false));
  await fixture.replace('hotbar.2', 'minecraft:diamond');
  await fixture.replace('hotbar.3', 'minecraft:emerald');
  await fixture.select(0, 'minecraft:stick');
  const open = async () => {
    await fixture.raw('key o tap');
    await until(() => fixture.raw('screen'), view => view.screen === 'container' && slot(view, 352), 'unselected wireless terminal opens by hotkey');
    assert.equal((await fixture.raw('inventory')).held?.item, 'minecraft:stick');
  };

  await open();
  await until(() => fixture.raw('screen'), view => slot(view, 327)?.item === 'minecraft:oak_log' && !shown(view, 'minecraft:iron_ingot'), 'unlinked menu opens with its physical matrix and no network items');
  await fixture.raw('click-slot 336');
  await until(() => fixture.raw('inventory'), state => state.carried?.item === 'minecraft:oak_planks' && state.carried.count === 4, 'unlinked menu crafts its real matrix without network refill');
  await until(() => fixture.raw('screen'), view => !slot(view, 327)?.item, 'unlinked crafting consumes the owner input');
  await fixture.raw('close');
  await until(() => fixture.raw('inventory'), state => !state.carried && state.counts?.['minecraft:oak_planks'] === 4, 'unlinked crafted output is returned once');

  await fixture.replace('hotbar.1', owner('oak_log', true, 0));
  await open();
  await until(() => fixture.raw('screen'), view => view.screen === 'container' && slot(view, 327)?.item === 'minecraft:oak_log' && !shown(view, 'minecraft:iron_ingot'), 'empty-battery item opens disconnected');
  await fixture.raw('close');

  await fixture.replace('hotbar.1', owner('oak_log'));
  await open();
  await until(() => fixture.raw('screen'), view => shown(view, 'minecraft:iron_ingot'), 'linked network view');
  await fixture.raw('click-slot 1');
  await until(() => fixture.raw('inventory'), state => !state.carried && state.hotbar[1]?.item === 'ae2:wireless_crafting_terminal', 'source item stays locked');
  await fixture.raw('click-slot 2');
  await until(() => fixture.raw('inventory'), state => state.carried?.item === 'minecraft:diamond', 'range sentinel is on the cursor');
  await fixture.command(`tp @s ${x + 128.5} ${y} ${z + 2.5}`);
  await until(() => fixture.raw('screen'), view => view.screen === 'container' && slot(view, 327)?.item === 'minecraft:oak_log' && !shown(view, 'minecraft:iron_ingot'), 'out-of-range menu remains open and disconnected');
  assert.equal((await fixture.raw('inventory')).carried?.item, 'minecraft:diamond');
  await fixture.command(`tp @s ${x + 1.5} ${y} ${z + 2.5}`);
  await until(() => fixture.raw('screen'), view => view.screen === 'container' && shown(view, 'minecraft:iron_ingot'), 'same session reconnects when the player returns', 30000);
  await fixture.raw('click-slot 2');
  await until(() => fixture.raw('inventory'), state => !state.carried && state.counts?.['minecraft:diamond'] === 1, 'range sentinel returned once');

  await fixture.raw('click-slot 3');
  await until(() => fixture.raw('inventory'), state => state.carried?.item === 'minecraft:emerald', 'replacement sentinel is on the cursor');
  await fixture.replace('hotbar.1', owner('birch_log'));
  await until(() => fixture.raw('screen'), view => view.screen === 'container' && slot(view, 327)?.item === 'minecraft:birch_log' && shown(view, 'minecraft:iron_ingot'), 'same-item replacement updates the locator and its real crafting matrix');
  assert.equal((await fixture.raw('inventory')).carried?.item, 'minecraft:emerald');
  await fixture.replace('hotbar.1', 'minecraft:stone');
  await until(() => fixture.raw('screen'), view => view.screen === 'playing', 'different item invalidates the source locator');
  let inventory = await fixture.raw('inventory');
  assert.equal(inventory.carried, null); assert.equal(inventory.counts?.['minecraft:emerald'], 1);
  assert.equal(inventory.counts?.['minecraft:diamond'], 1); assert.equal(inventory.counts?.['minecraft:stone'], 1);
  assert.equal(inventory.counts?.['minecraft:oak_planks'], 4);

  await fixture.replace('weapon.offhand', owner('birch_log'));
  await fixture.select(0, 'minecraft:stick'); await fixture.raw('look 0 -80'); await fixture.raw('use-item');
  await until(() => fixture.raw('screen'), view => view.screen === 'container' && slot(view, 327)?.item === 'minecraft:birch_log', 'direct use opens the actual offhand crafting terminal');
  await fixture.raw('click-slot 0'); await fixture.raw('click-slot 0'); await fixture.raw('key f tap');
  await fixture.command('data get entity @s Inventory');
  inventory = await until(() => fixture.raw('inventory'), state => state.offhand?.item === 'ae2:wireless_crafting_terminal' && state.hotbar?.[0]?.item === 'minecraft:stick' && !state.carried, 'offhand owner cannot be swapped out through a player slot');
  await fixture.raw('click-slot 327');
  await until(() => fixture.raw('inventory'), state => state.carried?.item === 'minecraft:birch_log', 'offhand item retains the real crafting matrix');
  await fixture.raw('close');
  await until(() => fixture.raw('inventory'), state => !state.carried && state.counts?.['minecraft:birch_log'] === 1, 'offhand matrix item returned once');
  await fixture.restart();
  inventory = await fixture.raw('inventory');
  assert.equal(inventory.counts?.['minecraft:diamond'], 1); assert.equal(inventory.counts?.['minecraft:emerald'], 1);
  assert.equal(inventory.counts?.['minecraft:oak_planks'], 4); assert.equal(inventory.counts?.['minecraft:birch_log'], 1);
  assert.equal(inventory.hotbar?.[1]?.item, 'minecraft:stone');
  assert.equal(inventory.offhand?.item, 'ae2:wireless_crafting_terminal');
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('unlinked and unpowered menus open, offline matrix crafts, range reconnects, same-item replacement is followed, and offhand ownership survives swap and restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
