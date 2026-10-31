/** A Provider-forced Assembler retains its source menu definition on reload. */
import assert from 'node:assert/strict';
import { cpSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { root } from '../../scripts/common';
import { WorldFixture, until } from './harness';

const isolated = mkdtempSync(join(tmpdir(), 'ae2-forced-assembler-'));
const content = join(isolated, 'content'); cpSync(process.env.AE2_CONTENT_DIR ?? join(root, 'content/ae2'), content, { recursive: true });
const recipes = join(content, 'data/ae2/recipe/test'); mkdirSync(recipes, { recursive: true });
writeFileSync(join(recipes, 'named_diamond.json'), JSON.stringify({ type: 'minecraft:crafting_shapeless', ingredients: ['minecraft:oak_log'], result: { id: 'minecraft:diamond', count: 1 } }));
const fixture = new WorldFixture('assembler-forced-pattern-menu', 'server-thread', { content });
const pattern = 'ae2:crafting_pattern[ae2:encoded_crafting_pattern={inputs:[{id:"minecraft:oak_log",count:1},{},{},{},{},{},{},{},{}],result:{id:"minecraft:oak_planks",count:4},recipeId:"ae2:test/named_diamond",canSubstitute:1b,canSubstituteFluids:0b}]';
const substitute = 'minecraft:oak_log[minecraft:custom_name={text:"Forced substitute",color:"gold"}]';
const slot = (view: any, index: number) => view.slots?.find((entry: any) => entry.slot === index);
const count = (status: any, item: string) => status.items?.find((entry: any) => entry.item === item)?.count ?? 0;
try {
  const start = await fixture.start(), [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 24, y = Math.ceil(py) + 5, z = Math.floor(pz) + 24;
  const at = (dx: number, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const energy = at(0), drive = at(1), terminal = at(2), cpu = at(2, 1), provider = at(3), assembler = at(3, 0, -1);
  await fixture.command(`tp @s ${x + 2.5} ${y} ${z + 2.5}`);
  for (const [dx, dz] of [[-2, -3], [-2, 3], [6, -3], [6, 3]] as const) await until(() => fixture.raw(`block ${x + dx} ${y} ${z + dz}`), state => state.loaded, 'fixture corner chunk loaded', 30000);
  await fixture.command(`fill ${at(-2, 0, -3)} ${at(6, 5, 3)} minecraft:air`); await fixture.command(`fill ${at(-2, -1, -3)} ${at(6, -1, 3)} minecraft:stone`);
  for (const [pos, id] of [[energy, 'creative_energy_cell'], [drive, 'drive'], [terminal, 'cable_bus'], [cpu, '64k_crafting_storage'], [provider, 'pattern_provider'], [assembler, 'molecular_assembler']] as const) await fixture.command(`setblock ${pos} ae2:${id}`);
  await fixture.command(`item replace block ${drive} container.0 with ae2:item_storage_cell_1k`); await fixture.command('clear @s');
  await fixture.replace('hotbar.0', 'ae2:terminal'); await fixture.select(0, 'ae2:terminal'); assert.equal((await fixture.ae(`grid part ${terminal} north`)).attached, true);
  for (const pos of [drive, cpu, provider, assembler]) await until(() => fixture.ae(`grid status ${pos}`), state => state.online && state.powered, 'forced-plan network ready', 30000);
  await fixture.replace('hotbar.0', pattern); await fixture.replace('hotbar.8', 'minecraft:stick'); await fixture.select(8, 'minecraft:stick');
  await fixture.command(`tp @s ${x + 5.5} ${y} ${z + .5}`); await fixture.raw(`interact ${provider} east`);
  let view = await until(() => fixture.raw('screen'), state => state.slots?.some((item: any) => item.slot < 36 && item.item === 'ae2:crafting_pattern'), 'physical source pattern');
  const source = view.slots.find((item: any) => item.slot < 36 && item.item === 'ae2:crafting_pattern'); await fixture.raw(`click-slot ${source.slot} shift`);
  await until(() => fixture.raw('inventory'), state => !state.counts?.['ae2:crafting_pattern'], 'real pattern installed in Provider'); await fixture.raw('close');
  await fixture.replace('hotbar.0', 'minecraft:oak_log'); await fixture.select(0, 'minecraft:oak_log'); assert.equal((await fixture.ae(`grid deposit ${drive}`)).inserted, 1);
  await fixture.replace('hotbar.1', substitute, 3); await fixture.replace('hotbar.0', 'minecraft:air'); await fixture.select(8, 'minecraft:stick');
  await fixture.hold();
  assert.equal((await fixture.ae(`crafting request ${cpu} ${provider} ${assembler} 0`)).started, true);
  const forced = JSON.stringify(await fixture.command(`data get block ${assembler} ae2_lab_config`)); assert.match(forced, /ae2_assembler_pattern/);
  // Keep the forced grid alive across restart without advancing its recipe.
  await fixture.command(`setblock ${energy} minecraft:air`); await fixture.hold(false); await fixture.restart();
  await fixture.command('gamemode survival @s'); await fixture.select(8, 'minecraft:stick');
  await fixture.command(`tp @s ${x + 3.5} ${y} ${z - 3.5}`); await fixture.raw(`interact ${assembler} north`);
  view = await until(() => fixture.raw('screen'), state => slot(state, 300)?.item === 'minecraft:oak_log' && !slot(state, 310)?.item, 'reloaded forced plan has a physical input and no installed pattern');
  await fixture.raw('click-slot 300'); await fixture.raw('click-slot 0');
  await until(() => fixture.raw('inventory'), state => !state.carried && state.hotbar[0]?.item === 'minecraft:oak_log', 'original forced input returned to player');
  await fixture.raw('click-slot 1'); await fixture.raw('click-slot 300');
  await until(() => fixture.raw('screen'), state => slot(state, 300)?.item === 'minecraft:oak_log' && String(slot(state, 300)?.components ?? '').includes('Forced substitute') && state.carried?.count === 2, 'reloaded forced plan accepts a source-valid substitute');
  await fixture.raw('click-slot 1'); await fixture.raw('close'); await fixture.restart();
  const saved = JSON.stringify(await fixture.command(`data get block ${assembler}`)); assert.match(saved, /Forced substitute/); assert.match(saved, /ae2_assembler_pattern/);
  const inventory = await fixture.raw('inventory'); assert.equal(inventory.counts?.['minecraft:oak_log'], 3);
  await fixture.command(`setblock ${energy} ae2:creative_energy_cell`);
  const finished = await until(() => fixture.ae(`grid status ${drive}`), state =>
    state.online && state.powered && count(state, 'minecraft:diamond') === 1, 'restored source plan crafts the accepted substitute', 90000);
  assert.equal(count(finished, 'minecraft:oak_planks'), 0);
  assert.equal((await fixture.raw('inventory')).counts?.['minecraft:oak_log'], 3, 'the original input and two unused substitutes remain with the player');
  await fixture.restart();
  const restored = await until(() => fixture.ae(`grid status ${drive}`), state => state.online && state.powered, 'reloaded crafted network', 30000);
  assert.equal(count(restored, 'minecraft:diamond'), 1); assert.equal(count(restored, 'minecraft:oak_planks'), 0);
  assert.equal((await fixture.raw('inventory')).counts?.['minecraft:oak_log'], 3);
  fixture.assertHidden(await fixture.raw('status')); fixture.mark('a forced plan retains and executes the named source recipe with a real substitute, exact custody and restart'); fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
