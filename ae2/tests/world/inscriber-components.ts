/** Loaded recipe components must survive actual machine production and restart. */
import assert from 'node:assert/strict';
import { cpSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { root } from '../../scripts/common';
import { WorldFixture, until, type Mode } from './harness';
const tests = join(root, '.local/world-tests'); mkdirSync(tests, { recursive: true });
const content = join(mkdtempSync(join(tests, 'inscriber-content-')), 'content');
cpSync(resolve(process.env.AE2_CONTENT_DIR ?? join(root, 'content/ae2')), content, { recursive: true });
const recipes = join(content, 'data/ae2/recipe/test'); mkdirSync(recipes, { recursive: true });
writeFileSync(join(recipes, 'component_inscriber.json'), JSON.stringify({ type: 'ae2:inscriber', ingredients: { middle: { item: 'minecraft:cobblestone' } }, mode: 'inscribe', result: { id: 'minecraft:diamond', count: 2, components: { 'minecraft:custom_data': { ae2_component_test: 'native-component-output' } } } }));
const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('inscriber-components', mode, { content });
try {
  const state = await fixture.start(); const [px, py, pz] = state.player.position;
  const x = Math.floor(px), y = Math.ceil(py) + 3, z = Math.floor(pz);
  const machine = `${x} ${y} ${z}`;
  await fixture.command(`fill ${x - 2} ${y - 1} ${z - 2} ${x + 3} ${y - 1} ${z + 3} minecraft:bedrock`);
  await fixture.command(`fill ${x - 2} ${y} ${z - 2} ${x + 3} ${y + 4} ${z + 3} minecraft:air`);
  await fixture.command(`setblock ${x + 1} ${y} ${z} ae2:creative_energy_cell`);
  await fixture.command(`setblock ${machine} ae2:inscriber`);
  await fixture.ae(`grid status ${machine}`);
  await fixture.command(`item replace block ${machine} container.2 with minecraft:cobblestone`);
  await until(() => fixture.raw(`container ${machine}`), view => view.slots?.some((s: any) => s?.item === 'minecraft:diamond' && s.count === 2), 'component-bearing inscriber output', 30000);
  // The compact container control reports only ID/count. Inspect physical
  // block-entity NBT for component identity, not that display projection.
  assert.match(JSON.stringify(await fixture.command(`data get block ${machine}`)), /native-component-output/);
  fixture.mark('a loaded recipe consumes one real input and produces two component-bearing output items');
  await fixture.restart();
  const saved = await fixture.raw(`container ${machine}`);
  assert.match(JSON.stringify(await fixture.command(`data get block ${machine}`)), /native-component-output/);
  assert.equal(saved.slots.filter((s: any) => s?.item === 'minecraft:diamond').reduce((n: number, s: any) => n + s.count, 0), 2);
  await fixture.command(`item replace block ${machine} container.2 with minecraft:cobblestone`);
  await until(() => fixture.raw(`container ${machine}`), view => view.slots?.some((s: any) => s?.item === 'minecraft:diamond' && s.count === 4), 'identical component output stacks', 30000);
  assert.match(JSON.stringify(await fixture.command(`data get block ${machine}`)), /native-component-output/);
  fixture.mark('components survive process restart and identical outputs stack without losing identity');
  await fixture.command(`item replace block ${machine} container.3 with minecraft:diamond 4`);
  await fixture.command(`item replace block ${machine} container.2 with minecraft:cobblestone`);
  await fixture.command('tick freeze'); await fixture.step(300);
  const blocked = await fixture.raw(`container ${machine}`);
  assert(blocked.slots.some((s: any) => s?.item === 'minecraft:cobblestone' && s.count === 1), 'different output components prevent input consumption');
  assert(blocked.slots.some((s: any) => s?.item === 'minecraft:diamond' && s.count === 4));
  assert.doesNotMatch(JSON.stringify(await fixture.command(`data get block ${machine}`)), /native-component-output/);
  fixture.mark('a component-mismatched output stack blocks production without consuming the input');
  fixture.assertHidden(await fixture.raw('status')); fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
