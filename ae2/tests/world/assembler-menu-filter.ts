/** Molecular Assembler slots enforce the physical pattern and output rules. */
import assert from 'node:assert/strict';
import { cpSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { root } from '../../scripts/common';
import { WorldFixture, until, type Mode } from './harness';

const isolated = mkdtempSync(join(tmpdir(), 'ae2-assembler-menu-'));
const content = join(isolated, 'content');
cpSync(process.env.AE2_CONTENT_DIR ?? join(root, 'content/ae2'), content, { recursive: true });
const recipes = join(content, 'data/ae2/recipe/test'); mkdirSync(recipes, { recursive: true });
writeFileSync(join(recipes, 'named_diamond.json'), JSON.stringify({ type: 'minecraft:crafting_shapeless', ingredients: ['minecraft:oak_log'], result: { id: 'minecraft:diamond', count: 1 } }));
const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('assembler-menu-filter', mode, { content });
// The source-stored display result is deliberately stale; the named recipe
// currently makes a diamond, and allows an oak-log key with custom components.
const pattern = 'ae2:crafting_pattern[ae2:encoded_crafting_pattern={inputs:[{id:"minecraft:oak_log",count:1},{},{},{},{},{},{},{},{}],result:{id:"minecraft:oak_planks",count:4},recipeId:"ae2:test/named_diamond",canSubstitute:1b,canSubstituteFluids:0b}]';
const substitute = 'minecraft:oak_log[minecraft:custom_name={text:"Named substitute",color:"gold"}]';
const slot = (view: any, index: number) => view.slots?.find((entry: any) => entry.slot === index);
try {
  const start = await fixture.start(), [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 24, y = Math.ceil(py) + 5, z = Math.floor(pz) + 24, machine = `${x} ${y} ${z}`;
  await fixture.command(`tp @s ${x + .5} ${y} ${z + 2.5}`);
  for (const [dx, dz] of [[-2, -3], [-2, 3], [2, -3], [2, 3]] as const) {
    await until(() => fixture.raw(`block ${x + dx} ${y} ${z + dz}`), state => state.loaded, 'fixture corner chunk loaded', 30000);
  }
  await fixture.command(`fill ${x - 2} ${y} ${z - 3} ${x + 2} ${y + 4} ${z + 3} minecraft:air`);
  await fixture.command(`fill ${x - 2} ${y - 1} ${z - 3} ${x + 2} ${y - 1} ${z + 3} minecraft:stone`);
  await fixture.command(`setblock ${machine} ae2:molecular_assembler`); await fixture.command('clear @s');
  await fixture.replace('hotbar.2', pattern);
  await fixture.command('gamemode survival @s'); await fixture.replace('hotbar.0', 'minecraft:dirt');
  await fixture.replace('hotbar.1', substitute, 3); await fixture.replace('hotbar.8', 'minecraft:stick');
  const open = async () => {
    await fixture.select(8, 'minecraft:stick'); await fixture.command(`tp @s ${x + .5} ${y} ${z + 2.5}`); await fixture.raw(`interact ${machine} south`);
    return until(() => fixture.raw('screen'), view => view.screen === 'container' && slot(view, 300) && slot(view, 310), 'physical Molecular Assembler menu');
  };
  let view = await open();
  await fixture.raw('click-slot 0'); await fixture.raw('click-slot 300'); await fixture.command(`data get block ${machine}`);
  await until(() => fixture.raw('screen'), state => !slot(state, 300)?.item && state.carried?.item === 'minecraft:dirt', 'empty pattern disables grid insertion');
  await fixture.raw('click-slot 309'); await fixture.raw('click-slot 310'); await fixture.command(`data get block ${machine}`);
  await until(() => fixture.raw('screen'), state => !slot(state, 309)?.item && !slot(state, 310)?.item && state.carried?.item === 'minecraft:dirt', 'output and encoded-pattern slot reject dirt');
  await fixture.raw('click-slot 0'); view = await fixture.raw('screen');
  const patternSlot = view.slots.find((item: any) => item.slot < 36 && item.item === 'ae2:crafting_pattern'); assert(patternSlot);
  await fixture.raw(`click-slot ${patternSlot.slot}`); await fixture.raw('click-slot 310');
  await until(() => fixture.raw('screen'), state => slot(state, 310)?.item === 'ae2:crafting_pattern' && !state.carried, 'valid source crafting pattern installed');
  await fixture.raw('click-slot 1'); await fixture.raw('click-slot 300');
  await until(() => fixture.raw('screen'), state => slot(state, 300)?.item === 'minecraft:oak_log' && slot(state, 300)?.count === 1 && String(slot(state, 300)?.components ?? '').includes('Named substitute') && state.carried?.count === 2, 'named live recipe accepts one component-bearing substitute');
  await fixture.raw('click-slot 301'); await fixture.command(`data get block ${machine}`);
  await until(() => fixture.raw('screen'), state => !slot(state, 301)?.item && state.carried?.item === 'minecraft:oak_log' && state.carried.count === 2, 'unused pattern slot rejects the remaining logs');
  await fixture.raw('click-slot 1'); await fixture.raw('close'); await fixture.restart();
  view = await open(); assert.equal(slot(view, 300)?.item, 'minecraft:oak_log'); assert.equal(slot(view, 300)?.count, 1);
  assert.equal(slot(view, 310)?.item, 'ae2:crafting_pattern'); assert.equal((await fixture.raw('inventory')).counts?.['minecraft:oak_log'], 2);
  fixture.assertHidden(await fixture.raw('status')); fixture.mark('named live recipe with a stale display output, pattern-owned input, extract-only output and exact custody survive restart'); fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
