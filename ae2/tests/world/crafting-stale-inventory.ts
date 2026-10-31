/** A completed planning snapshot must recheck real stock at CPU submission. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('crafting-stale-inventory', mode);
const count = (state: any, item: string) => state.items?.find((entry: any) => entry.item === item)?.count ?? 0;
try {
  const state = await fixture.start();
  const [px, py, pz] = state.player.position;
  const x = Math.floor(px) + 32, y = Math.ceil(py) + 5, z = Math.floor(pz) + 32;
  const at = (dx: number, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const drive = at(1), terminal = at(2), cpu = at(2, 1), provider = at(3), assembler = at(3, 0, -1);
  await fixture.command('clear @s');
  await fixture.command(`fill ${at(-2, 0, -3)} ${at(6, 5, 3)} minecraft:air`);
  await fixture.command(`fill ${at(-2, -1, -3)} ${at(6, -1, 3)} minecraft:stone`);
  for (const [pos, id] of [[at(0), 'creative_energy_cell'], [drive, 'drive'], [terminal, 'cable_bus'], [cpu, '1k_crafting_storage'], [provider, 'pattern_provider'], [assembler, 'molecular_assembler']] as const) await fixture.command(`setblock ${pos} ae2:${id}`);
  await fixture.command(`item replace block ${drive} container.0 with ae2:item_storage_cell_1k`);
  await fixture.replace('hotbar.0', 'ae2:terminal');
  await fixture.select(0, 'ae2:terminal');
  assert.equal((await fixture.ae(`grid part ${terminal} north`)).attached, true);
  const ready = () => until(() => fixture.ae(`grid status ${drive}`), value => value.online && value.powered, 'powered network', 30000);
  await ready();
  await fixture.replace('hotbar.0', 'ae2:blank_pattern');
  await fixture.select(0, 'ae2:blank_pattern');
  assert.equal((await fixture.ae('crafting encode minecraft:oak_log - - - - - - - -')).encoded, true);
  await until(() => fixture.raw('inventory'), view => view.hotbar?.[0]?.item === 'ae2:crafting_pattern', 'physical plank pattern');
  await fixture.command(`tp @s ${x + 3.5} ${y} ${z + 2.5}`);
  await fixture.raw(`interact ${provider} south`);
  const menu = await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot < 36 && slot.item === 'ae2:crafting_pattern'), 'provider menu');
  await fixture.raw(`click-slot ${menu.slots.find((slot: any) => slot.slot < 36 && slot.item === 'ae2:crafting_pattern').slot} shift`);
  await until(() => fixture.raw('inventory'), view => !view.counts?.['ae2:crafting_pattern'], 'pattern installed');
  await fixture.raw('close');
  await fixture.replace('hotbar.0', 'minecraft:oak_log', 2);
  await fixture.select(0, 'minecraft:oak_log');
  assert.equal((await fixture.ae(`grid deposit ${drive}`)).inserted, 2);
  await fixture.replace('hotbar.8', 'minecraft:blaze_rod');
  await fixture.select(8, 'minecraft:blaze_rod');
  await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`);
  await fixture.raw(`interact ${terminal} north`);
  const view = await until(() => fixture.raw('screen'), screen => screen.slots?.some((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:oak_planks'), 'craftable planks');
  await fixture.raw(`click-slot ${view.slots.find((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:oak_planks').slot}`);
  await until(() => fixture.raw('screen'), screen => JSON.stringify(screen.title ?? '').includes('AE2 Craft Amount'), 'craft amount');
  await fixture.raw('ui-key right'); await fixture.raw('ui-key backspace'); await fixture.raw('type 8');
  await fixture.raw('wait 5'); await fixture.raw('click-slot 302');
  await until(() => fixture.raw('screen'), screen => JSON.stringify(screen.title ?? '').includes('AE2 Craft Confirm') && /bytes/.test(JSON.stringify(screen)), 'calculated confirmation');

  const before = await ready();
  const logIndex = before.items.findIndex((entry: any) => entry.item === 'minecraft:oak_log');
  assert(logIndex >= 0);
  assert.equal((await fixture.ae(`grid withdraw ${drive} ${logIndex} 1`)).extracted, 1);
  await until(() => fixture.raw('inventory'), inventory => inventory.counts?.['minecraft:oak_log'] === 1, 'one real log withdrawn after planning');
  await fixture.raw('click-slot 302');
  await fixture.step(5);
  const rejected = await ready();
  assert.equal(count(rejected, 'minecraft:oak_log'), 1);
  assert.equal(count(rejected, 'minecraft:oak_planks'), 0);
  assert.equal((await fixture.raw('inventory')).counts?.['minecraft:oak_log'], 1);
  assert.doesNotMatch(JSON.stringify(await fixture.command(`data get block ${cpu}`)), /ae2_recursive_job/);
  fixture.mark('a stale completed plan cannot take the remaining log or start a CPU job');

  await fixture.raw('close');
  const inventory = await fixture.raw('inventory');
  const slot = inventory.hotbar.findIndex((entry: any) => entry?.item === 'minecraft:oak_log');
  assert(slot >= 0, 'withdrawn real log is in the hotbar');
  await fixture.select(slot, 'minecraft:oak_log');
  assert.equal((await fixture.ae(`grid deposit ${drive}`)).inserted, 1);
  await fixture.restart();
  assert.equal(count(await ready(), 'minecraft:oak_log'), 2);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('both physical logs remain conserved after returning the withdrawn log and restarting');
  fixture.report('passed');
} catch (error) {
  fixture.report('failed', error);
  throw error;
} finally {
  await fixture.close();
}
