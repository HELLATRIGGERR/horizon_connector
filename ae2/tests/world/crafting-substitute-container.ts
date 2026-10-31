/** Valid substitutes may return a different container while preserving the named recipe output. */
import assert from 'node:assert/strict';
import { cpSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { root } from '../../scripts/common';
import { WorldFixture, until, type Mode } from './harness';

const isolated = mkdtempSync(join(tmpdir(), 'ae2-substitute-container-'));
const content = join(isolated, 'content');
cpSync(process.env.AE2_CONTENT_DIR ?? join(root, 'content/ae2'), content, { recursive: true });
const recipes = join(content, 'data/ae2/recipe/test'); mkdirSync(recipes, { recursive: true });
writeFileSync(join(recipes, 'container_substitute.json'), JSON.stringify({
  type: 'minecraft:crafting_shapeless',
  ingredients: [['minecraft:stone', 'minecraft:milk_bucket']],
  result: { id: 'minecraft:diamond', count: 1 },
}));
const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('crafting-substitute-container', mode, { content });
const count = (status: any, item: string) => status.items?.find((entry: any) => entry.item === item)?.count ?? 0;
// The encoded stone has no remainder; its valid milk substitute returns an
// empty bucket. Source simulation does not charge the substitute-only return,
// but its CPU still expects and physically recovers that returned bucket.
const pattern = 'ae2:crafting_pattern[ae2:encoded_crafting_pattern={inputs:[{id:"minecraft:stone",count:1},{},{},{},{},{},{},{},{}],result:{id:"minecraft:diamond",count:1},recipeId:"ae2:test/container_substitute",canSubstitute:1b,canSubstituteFluids:0b}]';
try {
  const state = await fixture.start(); const [px, py, pz] = state.player.position;
  const x = Math.floor(px) + 32, y = Math.ceil(py) + 5, z = Math.floor(pz) + 32;
  const at = (dx: number, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const drive = at(1), terminal = at(2), cpu = at(2, 1), provider = at(3), assembler = at(3, 0, -1);
  await fixture.command('clear @s'); await fixture.command(`fill ${at(-2, 0, -3)} ${at(6, 5, 3)} minecraft:air`);
  await fixture.command(`fill ${at(-2, -1, -3)} ${at(6, -1, 3)} minecraft:stone`);
  for (const [pos, id] of [[at(0), 'creative_energy_cell'], [drive, 'drive'], [terminal, 'cable_bus'],
    [cpu, '64k_crafting_storage'], [provider, 'pattern_provider'], [assembler, 'molecular_assembler']] as const) {
    await fixture.command(`setblock ${pos} ae2:${id}`);
  }
  await fixture.command(`item replace block ${drive} container.0 with ae2:item_storage_cell_1k`);
  await fixture.replace('hotbar.0', 'ae2:terminal'); await fixture.select(0, 'ae2:terminal');
  assert.equal((await fixture.ae(`grid part ${terminal} north`)).attached, true);
  const ready = () => until(() => fixture.ae(`grid status ${drive}`), view => view.online && view.powered, 'powered container-substitution network', 30000);
  await ready();
  for (const pos of [provider, assembler]) await until(() => fixture.ae(`grid status ${pos}`), view => view.online && view.powered, `online ${pos}`, 30000);
  await fixture.replace('hotbar.0', pattern); await fixture.replace('hotbar.8', 'minecraft:blaze_rod'); await fixture.select(8, 'minecraft:blaze_rod');
  await fixture.command(`tp @s ${x + 5.5} ${y} ${z + 0.5}`); await fixture.raw(`interact ${provider} east`);
  const menu = await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot < 36 && slot.item === 'ae2:crafting_pattern'), 'source pattern in inventory');
  await fixture.raw(`click-slot ${menu.slots.find((slot: any) => slot.slot < 36 && slot.item === 'ae2:crafting_pattern').slot} shift`);
  await until(() => fixture.raw('inventory'), view => !view.counts?.['ae2:crafting_pattern'], 'source pattern installed'); await fixture.raw('close');
  await fixture.replace('hotbar.0', 'minecraft:milk_bucket'); await fixture.select(0, 'minecraft:milk_bucket');
  assert.equal((await fixture.ae(`grid deposit ${drive}`)).inserted, 1);
  assert.equal(count(await ready(), 'minecraft:milk_bucket'), 1);
  await fixture.select(8, 'minecraft:blaze_rod'); await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`); await fixture.raw(`interact ${terminal} north`);
  const screen = await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:diamond'), 'named recipe is craftable');
  await fixture.raw(`click-slot ${screen.slots.find((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:diamond').slot} shift`);
  await until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 Craft Amount'), 'substitute amount');
  await fixture.raw('click-slot 302');
  const confirm = await until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 Craft Confirm') && /bytes|Missing materials|craft planning:/.test(JSON.stringify(view)), 'substitute confirmation');
  assert.doesNotMatch(JSON.stringify(confirm), /Missing materials|craft planning:/);
  assert.match(JSON.stringify(confirm), /19 bytes/);
  await fixture.raw('click-slot 302'); await fixture.raw('close');
  await until(ready, view => count(view, 'minecraft:diamond') === 1 && count(view, 'minecraft:bucket') === 1 && count(view, 'minecraft:milk_bucket') === 0, 'substitute craft returns its actual container', 90000);
  await fixture.restart(); const restored = await ready();
  assert.equal(count(restored, 'minecraft:diamond'), 1); assert.equal(count(restored, 'minecraft:bucket'), 1);
  assert.equal(count(restored, 'minecraft:milk_bucket'), 0); assert.equal(count(restored, 'minecraft:stone'), 0);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('source-valid substitute uses 19 calculation bytes and returns the actual bucket across restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
