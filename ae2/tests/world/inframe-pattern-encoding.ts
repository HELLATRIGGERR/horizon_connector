/** A face-owned in-frame encoding terminal turns physical blanks into patterns. */
import assert from 'node:assert/strict';
import { WorldFixture, until } from './harness';

const fixture = new WorldFixture('inframe-pattern-encoding', 'in-frame');
const slot = (view: any, index: number) => view.slots?.find((entry: any) => entry.slot === index);
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 32, y = Math.ceil(py) + 5, z = Math.floor(pz) + 32;
  const at = (dx: number) => `${x + dx} ${y} ${z}`;
  const terminal = at(2);
  await fixture.command(`fill ${x - 2} ${y} ${z - 3} ${x + 5} ${y + 4} ${z + 3} minecraft:air`);
  await fixture.command(`fill ${x - 2} ${y - 1} ${z - 3} ${x + 5} ${y - 1} ${z + 3} minecraft:stone`);
  await fixture.command(`setblock ${at(0)} ae2:creative_energy_cell`);
  await fixture.command(`setblock ${at(1)} ae2:drive`);
  await fixture.command(`setblock ${terminal} ae2:cable_bus`);
  await fixture.command('clear @s');
  await fixture.replace('hotbar.0', 'ae2:pattern_encoding_terminal'); await fixture.select(0, 'ae2:pattern_encoding_terminal');
  assert.equal((await fixture.ae(`grid part ${terminal} north`)).attached, true);
  await until(() => fixture.ae(`grid status ${terminal}`), state => state.online && state.powered, 'powered encoding face', 30000);
  await fixture.command('gamemode survival @s');
  await fixture.replace('hotbar.0', 'minecraft:oak_log');
  await fixture.replace('hotbar.1', 'ae2:blank_pattern');
  await fixture.replace('hotbar.8', 'minecraft:stick'); await fixture.select(8, 'minecraft:stick');
  await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`);
  await fixture.raw(`interact ${terminal} north`);
  await until(() => fixture.raw('screen'), view => String(slot(view, 348)?.components ?? '').includes('Encode crafting pattern'), 'encoding terminal storage view');
  await fixture.raw('click-slot 348');
  await until(() => fixture.raw('screen'), view => String(slot(view, 348)?.components ?? '').includes('Back to storage'), 'encoding editor');
  await fixture.raw('click-slot 0'); await fixture.raw('click-slot 300');
  await until(() => fixture.raw('screen'), view => slot(view, 300)?.item === 'minecraft:oak_log' && slot(view, 313)?.item === 'minecraft:oak_planks', 'ghost recipe and crafting preview');
  assert.equal((await fixture.raw('inventory')).carried?.item, 'minecraft:oak_log', 'ghost input does not consume the source');
  await fixture.raw('click-slot 0');
  await fixture.raw('click-slot 1'); await fixture.raw('click-slot 349');
  await until(() => fixture.raw('screen'), view => slot(view, 349)?.item === 'ae2:blank_pattern', 'physical blank inserted');
  await fixture.raw('click-slot 313');
  const encoded=await until(() => fixture.raw('screen'), view => slot(view, 350)?.item === 'ae2:crafting_pattern' && !slot(view, 349)?.item, 'blank becomes a real crafting pattern');
  if (process.env.AE2_EXPECT_SOURCE_PATTERNS === '1') {
    assert.match(String(slot(encoded, 350)?.components ?? ''), /ae2:encoded_crafting_pattern/, 'the real pattern contains the source recipe component');
  }
  await fixture.raw('click-slot 350'); await fixture.raw('click-slot 1');
  const inventory = await fixture.raw('inventory');
  assert.equal(inventory.counts?.['minecraft:oak_log'], 1);
  assert.equal(inventory.counts?.['ae2:crafting_pattern'], 1);
  assert.equal(inventory.counts?.['ae2:blank_pattern'] ?? 0, 0);
  fixture.mark('a ghost recipe and one physical blank create exactly one crafting pattern');
  await fixture.raw('close'); await fixture.restart();
  const after = await fixture.raw('inventory');
  assert.equal(after.counts?.['minecraft:oak_log'], 1);
  assert.equal(after.counts?.['ae2:crafting_pattern'], 1);
  assert.match(JSON.stringify((await fixture.command(`data get block ${terminal} ae2_lab_config.parts`)).chat), /minecraft:oak_log/);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('the physical pattern and face-owned ghost recipe survive restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
