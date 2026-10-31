/** In-frame Crafting Terminal matrix, result and physical clear operations. */
import assert from 'node:assert/strict';
import { WorldFixture, until } from './harness';

const fixture = new WorldFixture('inframe-crafting-matrix', 'in-frame');
const slot = (view: any, index: number) => view.slots?.find((entry: any) => entry.slot === index);
const stored = (view: any, id: string) => view.items?.find((entry: any) => entry.item === id)?.count ?? 0;

try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 32, y = Math.ceil(py) + 5, z = Math.floor(pz) + 32;
  const at = (dx: number) => `${x + dx} ${y} ${z}`;
  const drive = at(1), terminal = at(2);
  await fixture.command(`fill ${x - 2} ${y} ${z - 3} ${x + 5} ${y + 4} ${z + 3} minecraft:air`);
  await fixture.command(`fill ${x - 2} ${y - 1} ${z - 3} ${x + 5} ${y - 1} ${z + 3} minecraft:stone`);
  for (const [pos, id] of [[at(0), 'creative_energy_cell'], [drive, 'drive'], [terminal, 'cable_bus']] as const) {
    await fixture.command(`setblock ${pos} ae2:${id}`);
  }
  await fixture.command(`item replace block ${drive} container.0 with ae2:item_storage_cell_1k`);
  await fixture.command('clear @s');
  await fixture.replace('hotbar.0', 'ae2:crafting_terminal'); await fixture.select(0, 'ae2:crafting_terminal');
  assert.equal((await fixture.ae(`grid part ${terminal} north`)).attached, true);
  const grid = () => until(() => fixture.ae(`grid status ${drive}`), state => state.online && state.powered, 'powered crafting grid', 30000);
  await grid(); await fixture.command('gamemode survival @s');
  await fixture.replace('hotbar.0', 'minecraft:oak_log');
  await fixture.replace('hotbar.8', 'minecraft:stick'); await fixture.select(8, 'minecraft:stick');
  await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`);
  const open = async () => {
    await fixture.raw(`interact ${terminal} north`);
    return until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 Crafting Terminal'), 'crafting terminal');
  };
  await open();
  await fixture.raw('click-slot 0'); await fixture.raw('click-slot 327');
  await until(() => fixture.raw('screen'), view => slot(view, 327)?.item === 'minecraft:oak_log' && slot(view, 336)?.item === 'minecraft:oak_planks' && slot(view, 336)?.count === 4, 'real input and matching recipe result');
  await fixture.raw('click-slot 336');
  await until(() => fixture.raw('inventory'), view => view.carried?.item === 'minecraft:oak_planks' && view.carried.count === 4, 'crafted planks on cursor');
  const consumed = await fixture.raw('screen');
  assert.equal(slot(consumed, 327)?.item, null);
  assert.equal(slot(consumed, 336)?.item, null);
  await fixture.raw('click-slot 1');
  await until(() => fixture.raw('inventory'), view => !view.carried && (view.counts?.['minecraft:oak_planks'] ?? 0) === 4, 'crafted result in player inventory');
  fixture.mark('one physical log produces four planks and leaves no matrix input');

  await fixture.replace('hotbar.0', 'minecraft:oak_log', 2);
  await fixture.raw('click-slot 0'); await fixture.raw('click-slot 327');
  await until(() => fixture.raw('screen'), view => slot(view, 327)?.item === 'minecraft:oak_log' && slot(view, 327)?.count === 2, 'two logs in physical matrix');
  await fixture.raw('click-slot 338');
  await until(() => fixture.raw('screen'), view => !slot(view, 327)?.item, 'clear to inventory emptied matrix');
  const afterInventory = await fixture.raw('inventory');
  assert.equal(afterInventory.counts?.['minecraft:oak_log'], 2);
  assert.equal(afterInventory.counts?.['minecraft:oak_planks'], 4);
  assert.equal(stored(await grid(), 'minecraft:oak_log'), 0);
  const logs = (await fixture.raw('screen')).slots.find((entry: any) => entry.slot < 36 && entry.item === 'minecraft:oak_log');
  assert(logs, 'the returned physical logs are available in the player inventory');
  await fixture.raw(`click-slot ${logs.slot}`); await fixture.raw('click-slot 327');
  await until(() => fixture.raw('screen'), view => slot(view, 327)?.item === 'minecraft:oak_log' && slot(view, 327)?.count === 2, 'returned logs back in matrix');
  await fixture.raw('click-slot 337');
  await until(grid, state => stored(state, 'minecraft:oak_log') === 2, 'clear moved both physical logs to the network');
  const afterNetwork = await fixture.raw('screen');
  assert.equal(slot(afterNetwork, 327)?.item, null);
  const inventory = await fixture.raw('inventory');
  assert.equal(inventory.carried, null);
  assert.equal(inventory.counts?.['minecraft:oak_log'] ?? 0, 0);
  assert.equal(inventory.counts?.['minecraft:oak_planks'], 4);
  fixture.mark('inventory and network clear each move the original stacks exactly once');

  const networkLogs = afterNetwork.slots.find((entry: any) => entry.slot >= 300 && entry.slot < 327 && entry.item === 'minecraft:oak_log');
  assert(networkLogs, 'the network contains the two returned logs');
  await fixture.raw(`click-slot ${networkLogs.slot}`); await fixture.raw('click-slot 327 right');
  await fixture.raw(`click-slot ${networkLogs.slot}`);
  await until(grid, state => stored(state, 'minecraft:oak_log') === 1, 'one log remains to refill the matrix');
  await fixture.raw('click-slot 336 shift');
  await until(() => fixture.raw('inventory'), view => (view.counts?.['minecraft:oak_planks'] ?? 0) === 12, 'shift result crafts twice after network refill');
  assert.equal(stored(await grid(), 'minecraft:oak_log'), 0);
  assert.equal(slot(await fixture.raw('screen'), 327)?.item, null);
  fixture.mark('shift result refills the original ingredient from storage and crafts without duplication');

  await fixture.replace('hotbar.0', 'minecraft:cobblestone', 4);
  await fixture.raw('click-slot 0');
  const widgets = await fixture.raw('ui');
  const centers = [327, 328].map(index => {
    const node = widgets.widgets.find((entry: any) => entry.id === `slot-${index}`);
    assert(node, `matrix slot ${index} has a visible hit target`);
    return [node.bounds[0] + 8, node.bounds[1] + 8];
  });
  await fixture.raw(`drag-at ${centers[0][0]} ${centers[0][1]} ${centers[1][0]} ${centers[1][1]}`);
  await until(() => fixture.raw('screen'), view => slot(view, 327)?.count === 2 && slot(view, 328)?.count === 2 && !view.carried, 'quick-craft drag splits four physical items');
  await fixture.raw('click-slot 338');
  assert.equal((await fixture.raw('inventory')).counts?.['minecraft:cobblestone'], 4);
  fixture.mark('a physical drag distributes and returns all four matrix items');

  await fixture.raw('close'); await fixture.restart();
  assert.equal(stored(await grid(), 'minecraft:oak_log'), 0);
  assert.equal((await fixture.raw('inventory')).counts?.['minecraft:oak_planks'], 12);
  assert.equal((await fixture.raw('inventory')).counts?.['minecraft:cobblestone'], 4);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('network input and crafted player output survive restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
