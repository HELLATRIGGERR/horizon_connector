/** AE2's enabled fluid substitution is a primary input template, not an extra pattern. */
import assert from 'node:assert/strict';
import { cpSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { root } from '../../scripts/common';
import { WorldFixture, until, type Mode } from './harness';

const isolated = mkdtempSync(join(tmpdir(), 'ae2-fluid-template-'));
const content = join(isolated, 'content');
cpSync(process.env.AE2_CONTENT_DIR ?? join(root, 'content/ae2'), content, { recursive: true });
const recipes = join(content, 'data/ae2/recipe/test'); mkdirSync(recipes, { recursive: true });
for (const [id, result] of [['fluid_only', 'minecraft:diamond'], ['fluid_first', 'minecraft:emerald']] as const) {
  writeFileSync(join(recipes, `${id}.json`), JSON.stringify({ type: 'minecraft:crafting_shapeless',
    ingredients: ['minecraft:water_bucket'], result: { id: result, count: 1 } }));
}
const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('crafting-fluid-template', mode, { content });
const count = (status: any, item: string) => status.items?.find((entry: any) => entry.item === item)?.count ?? 0;
const pattern = (recipe: string, result: string, substitutes: boolean) => `ae2:crafting_pattern[ae2:encoded_crafting_pattern={inputs:[{id:"minecraft:water_bucket",count:1},{},{},{},{},{},{},{},{}],result:{id:"${result}",count:1},recipeId:"ae2:test/${recipe}",canSubstitute:${substitutes ? 1 : 0}b,canSubstituteFluids:1b}]`;
const waterCell = 'ae2:fluid_storage_cell_1k[ae2:storage_cell_inv=[{"#t":"ae2:f",id:"minecraft:water","#":1000L}]]';
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
  const ready = () => until(() => fixture.ae(`grid status ${drive}`), view => view.online && view.powered, 'powered fluid-template network', 30000);
  const fluids = () => fixture.ae(`fluid grid status ${drive}`);
  await ready();
  for (const pos of [provider, assembler]) await until(() => fixture.ae(`grid status ${pos}`), view => view.online && view.powered, `online ${pos}`, 30000);
  await fixture.replace('hotbar.0', pattern('fluid_only', 'minecraft:diamond', false));
  await fixture.replace('hotbar.1', pattern('fluid_first', 'minecraft:emerald', true));
  await fixture.replace('hotbar.8', 'minecraft:blaze_rod'); await fixture.select(8, 'minecraft:blaze_rod');
  await fixture.command(`tp @s ${x + 5.5} ${y} ${z + 0.5}`); await fixture.raw(`interact ${provider} east`);
  for (const remaining of [1, 0]) {
    const menu = await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot < 36 && slot.item === 'ae2:crafting_pattern'), 'source pattern in player inventory');
    await fixture.raw(`click-slot ${menu.slots.find((slot: any) => slot.slot < 36 && slot.item === 'ae2:crafting_pattern').slot} shift`);
    await until(() => fixture.raw('inventory'), view => (view.counts?.['ae2:crafting_pattern'] ?? 0) === remaining, `source patterns remaining: ${remaining}`);
  }
  await fixture.raw('close'); await fixture.replace('hotbar.0', 'minecraft:water_bucket'); await fixture.select(0, 'minecraft:water_bucket');
  assert.equal((await fixture.ae(`grid deposit ${drive}`)).inserted, 1);
  assert.equal(count(await ready(), 'minecraft:water_bucket'), 1);
  const confirm = async (item: string) => {
    await fixture.select(8, 'minecraft:blaze_rod'); await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`); await fixture.raw(`interact ${terminal} north`);
    const screen = await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === item), `${item} is craftable`);
    await fixture.raw(`click-slot ${screen.slots.find((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === item).slot} shift`);
    await until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 Craft Amount'), `${item} amount`);
    await fixture.raw('click-slot 302');
    return until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 Craft Confirm') && /bytes|Missing materials|craft planning:/.test(JSON.stringify(view)), `${item} confirmation`);
  };
  const missing = await confirm('minecraft:diamond');
  assert.match(JSON.stringify(missing), /Missing materials/, 'without item substitutes source requires fluid, even when its bucket is stored');
  await fixture.raw('close');
  assert.equal(count(await ready(), 'minecraft:water_bucket'), 1);
  await fixture.command(`item replace block ${drive} container.1 with ${waterCell}`);
  await until(fluids, view => view.entries?.some((entry: any) => entry.fluid === 'minecraft:water' && entry.amount_mb === 1000), 'the first physical water cell is mounted');
  const first = await confirm('minecraft:diamond'); assert.doesNotMatch(JSON.stringify(first), /Missing materials|craft planning:/); assert.match(JSON.stringify(first), /19 bytes/);
  await fixture.raw('click-slot 302'); await fixture.raw('close');
  await until(ready, view => count(view, 'minecraft:diamond') === 1 && count(view, 'minecraft:water_bucket') === 1, 'fluid-only recipe completes without consuming the bucket', 90000);
  await until(fluids, view => !view.entries?.some((entry: any) => entry.fluid === 'minecraft:water' && entry.amount_mb), 'first fluid input is consumed');
  assert.equal(count(await ready(), 'minecraft:bucket'), 0);
  await fixture.command(`item replace block ${drive} container.1 with ${waterCell}`);
  await until(fluids, view => view.entries?.some((entry: any) => entry.fluid === 'minecraft:water' && entry.amount_mb === 1000), 'the second physical water cell is mounted');
  const second = await confirm('minecraft:emerald'); assert.doesNotMatch(JSON.stringify(second), /Missing materials|craft planning:/); assert.match(JSON.stringify(second), /19 bytes/);
  await fixture.raw('click-slot 302'); await fixture.raw('close');
  await until(ready, view => count(view, 'minecraft:emerald') === 1 && count(view, 'minecraft:water_bucket') === 1, 'fluid wins before the allowed bucket substitute', 90000);
  await until(fluids, view => !view.entries?.some((entry: any) => entry.fluid === 'minecraft:water' && entry.amount_mb), 'second fluid input is consumed');
  const fallback = await confirm('minecraft:emerald'); assert.doesNotMatch(JSON.stringify(fallback), /Missing materials|craft planning:/); assert.match(JSON.stringify(fallback), /19 bytes/);
  await fixture.raw('click-slot 302'); await fixture.raw('close');
  await until(ready, view => count(view, 'minecraft:emerald') === 2 && count(view, 'minecraft:water_bucket') === 0 && count(view, 'minecraft:bucket') === 1, 'bucket is used only after fluid is unavailable and its container returns', 90000);
  await fixture.restart(); const restored = await ready();
  assert.equal(count(restored, 'minecraft:diamond'), 1); assert.equal(count(restored, 'minecraft:emerald'), 2);
  assert.equal(count(restored, 'minecraft:water_bucket'), 0); assert.equal(count(restored, 'minecraft:bucket'), 1);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('source templates prioritize fluid, then use the real bucket and return its container across restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
