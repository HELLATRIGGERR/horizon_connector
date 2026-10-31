/** Source patterns use their named recipe and its current output even if a different recipe matches first. */
import assert from 'node:assert/strict';
import { cpSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { root } from '../../scripts/common';
import { WorldFixture, until, type Mode } from './harness';

const isolated = mkdtempSync(join(tmpdir(), 'ae2-named-recipe-'));
const content = join(isolated, 'content');
cpSync(process.env.AE2_CONTENT_DIR ?? join(root, 'content/ae2'), content, { recursive: true });
const recipes = join(content, 'data/ae2/recipe/test'); mkdirSync(recipes, { recursive: true });
writeFileSync(join(recipes, 'named_diamond.json'), JSON.stringify({
  type: 'minecraft:crafting_shapeless', ingredients: ['minecraft:oak_log'], result: { id: 'minecraft:diamond', count: 1 },
}));
const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('source-named-recipe', mode, { content });
const count = (status: any, item: string) => status.items?.find((entry: any) => entry.item === item)?.count ?? 0;
// The stored tooltip output is deliberately stale: the selected AE2 recipe now
// makes a diamond. The vanilla oak-planks recipe also matches this grid first.
const pattern = 'ae2:crafting_pattern[ae2:encoded_crafting_pattern={inputs:[{id:"minecraft:oak_log",count:1},{},{},{},{},{},{},{},{}],result:{id:"minecraft:oak_planks",count:4},recipeId:"ae2:test/named_diamond",canSubstitute:0b,canSubstituteFluids:0b}]';
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
  const ready = () => until(() => fixture.ae(`grid status ${drive}`), view => view.online && view.powered, 'powered source-recipe network', 30000);
  await ready();
  for (const pos of [provider, assembler]) await until(() => fixture.ae(`grid status ${pos}`), view => view.online && view.powered, `online ${pos}`, 30000);
  await fixture.replace('hotbar.0', pattern); await fixture.replace('hotbar.8', 'minecraft:blaze_rod'); await fixture.select(8, 'minecraft:blaze_rod');
  await fixture.command(`tp @s ${x + 5.5} ${y} ${z + 0.5}`); await fixture.raw(`interact ${provider} east`);
  const menu = await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot < 36 && slot.item === 'ae2:crafting_pattern'), 'source pattern in player inventory');
  await fixture.raw(`click-slot ${menu.slots.find((slot: any) => slot.slot < 36 && slot.item === 'ae2:crafting_pattern').slot} shift`);
  await until(() => fixture.raw('inventory'), view => !view.counts?.['ae2:crafting_pattern'], 'source pattern installed'); await fixture.raw('close');
  await fixture.replace('hotbar.0', 'minecraft:oak_log'); await fixture.select(0, 'minecraft:oak_log');
  assert.equal((await fixture.ae(`grid deposit ${drive}`)).inserted, 1);
  assert.equal(count(await ready(), 'minecraft:oak_log'), 1);
  fixture.mark('the source pattern selects a recipe that shares its physical input with vanilla oak planks');
  await fixture.select(8, 'minecraft:blaze_rod'); await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`); await fixture.raw(`interact ${terminal} north`);
  const screen = await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:diamond'), 'the selected current recipe output is craftable');
  await fixture.raw(`click-slot ${screen.slots.find((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:diamond').slot} shift`);
  await until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 Craft Amount'), 'named recipe amount');
  await fixture.raw('click-slot 302');
  const confirm = await until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 Craft Confirm') && /bytes|Missing materials|craft planning:/.test(JSON.stringify(view)), 'named recipe confirmation');
  assert.doesNotMatch(JSON.stringify(confirm), /Missing materials/); assert.match(JSON.stringify(confirm), /minecraft:diamond/);
  await fixture.raw('click-slot 302'); await fixture.raw('close');
  await until(ready, view => count(view, 'minecraft:diamond') === 1 && count(view, 'minecraft:oak_log') === 0, 'selected named recipe physically completes', 90000);
  assert.equal(count(await ready(), 'minecraft:oak_planks'), 0);
  await fixture.restart(); const restored = await ready();
  assert.equal(count(restored, 'minecraft:diamond'), 1); assert.equal(count(restored, 'minecraft:oak_planks'), 0);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('the live named recipe produces one diamond, does not produce the stale tooltip output, and persists through restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
