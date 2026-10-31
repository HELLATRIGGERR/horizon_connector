/** Pattern Encoding CONFIG_STACKS follows source FakeSlot increase/decrease. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('encoding-fake-gestures', mode);
const slot = (view: any, index: number) => view.slots?.find((entry: any) => entry.slot === index);
try {
  const start = await fixture.start(); const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 32, y = Math.ceil(py) + 5, z = Math.floor(pz) + 32;
  const at = (dx: number) => `${x + dx} ${y} ${z}`, terminal = at(2);
  await fixture.command(`fill ${x - 2} ${y} ${z - 3} ${x + 5} ${y + 4} ${z + 3} minecraft:air`);
  await fixture.command(`fill ${x - 2} ${y - 1} ${z - 3} ${x + 5} ${y - 1} ${z + 3} minecraft:stone`);
  await fixture.command(`setblock ${at(0)} ae2:creative_energy_cell`);
  await fixture.command(`setblock ${at(1)} ae2:drive`);
  await fixture.command(`setblock ${terminal} ae2:cable_bus`); await fixture.command('clear @s');
  await fixture.replace('hotbar.0', 'ae2:pattern_encoding_terminal'); await fixture.select(0, 'ae2:pattern_encoding_terminal');
  assert.equal((await fixture.ae(`grid part ${terminal} north`)).attached, true);
  await until(() => fixture.ae(`grid status ${terminal}`), state => state.online && state.powered, 'powered encoding face', 30000);
  await fixture.command('gamemode survival @s'); await fixture.replace('hotbar.0', 'minecraft:oak_planks', 3);
  await fixture.replace('hotbar.1', 'minecraft:stone'); await fixture.replace('hotbar.2', 'minecraft:water_bucket'); await fixture.replace('hotbar.8', 'minecraft:stick'); await fixture.select(8, 'minecraft:stick');
  await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`); await fixture.raw(`interact ${terminal} north`);
  await until(() => fixture.raw('screen'), view => String(slot(view, 348)?.components ?? '').includes('Encode crafting pattern'), 'encoding terminal');
  await fixture.raw('click-slot 348'); await until(() => fixture.raw('screen'), view => String(slot(view, 348)?.components ?? '').includes('Back to storage'), 'encoding editor');
  await fixture.raw('click-slot 346'); await until(() => fixture.raw('screen'), view => String(slot(view, 346)?.components ?? '').includes('processing'), 'processing mode');
  await fixture.raw('click-slot 0'); await until(() => fixture.raw('inventory'), view => view.carried?.item === 'minecraft:oak_planks' && view.carried.count === 3, 'physical template on cursor');
  const ghost = async (button: 'left' | 'right', expected: number, carried: string | null) => {
    await fixture.raw(`click-slot 300${button === 'right' ? ' right' : ''}`);
    // A command round trip orders the following observation behind the click.
    await fixture.command(`data get block ${terminal} ae2_lab_config.parts`);
    return until(() => fixture.raw('screen'), view => (slot(view, 300)?.count ?? 0) === expected && (expected === 0 || slot(view, 300)?.item === 'minecraft:oak_planks') && (view.carried?.item ?? null) === carried, `authoritative ghost amount ${expected} and cursor`);
  };
  await ghost('right', 1, 'minecraft:oak_planks'); await ghost('left', 4, 'minecraft:oak_planks'); await ghost('right', 1, 'minecraft:oak_planks');
  await fixture.raw('click-slot 0'); await until(() => fixture.raw('inventory'), view => !view.carried && view.counts?.['minecraft:oak_planks'] === 3, 'oak template returned');
  await fixture.raw('click-slot 1'); await until(() => fixture.raw('inventory'), view => view.carried?.item === 'minecraft:stone', 'different physical key held');
  const unchanged = await ghost('right', 1, 'minecraft:stone'); assert.equal(unchanged.carried?.item, 'minecraft:stone');
  await fixture.raw('click-slot 1'); await until(() => fixture.raw('inventory'), view => !view.carried && view.counts?.['minecraft:stone'] === 1, 'different key returned');
  await ghost('right', 0, null);
  await fixture.raw('click-slot 0'); await fixture.raw('click-slot 301');
  await fixture.command(`data get block ${terminal} ae2_lab_config.parts`);
  await until(() => fixture.raw('screen'), view => slot(view, 301)?.item === 'minecraft:oak_planks' && slot(view, 301)?.count === 3, 'independent three-item ghost');
  await fixture.raw('click-slot 0'); await until(() => fixture.raw('inventory'), view => !view.carried, 'oak template restored');
  await fixture.raw('click-slot 2'); await until(() => fixture.raw('inventory'), view => view.carried?.item === 'minecraft:water_bucket', 'filled physical bucket on cursor');
  for (const index of [302,318]) {
    await fixture.raw(`click-slot ${index} right`); await fixture.command(`data get block ${terminal} ae2_lab_config.parts`);
    const view=await until(() => fixture.raw('screen'), current => slot(current,index)?.item === 'ae2:wrapped_generic_stack' && String(slot(current,index)?.components ?? '').includes('minecraft:water'), 'bucket emptying action sets fluid ghost');
    assert.equal(view.carried?.item, 'minecraft:water_bucket', 'fluid filter does not consume the bucket');
  }
  await fixture.raw('click-slot 2'); await until(() => fixture.raw('inventory'), view => !view.carried && view.counts?.['minecraft:water_bucket'] === 1, 'physical bucket restored');
  await fixture.raw('close'); await until(() => fixture.raw('inventory'), view => !view.carried && view.counts?.['minecraft:oak_planks'] === 3, 'exact cursor custody on close');
  await fixture.restart();
  const saved = JSON.stringify((await fixture.command(`data get block ${terminal} ae2_lab_config.parts`)).chat);
  assert.match(saved, /minecraft:oak_planks/); assert.match(saved, /count: 3/); assert.match(saved, /minecraft:water/);
  const inventory = await fixture.raw('inventory'); assert.equal(inventory.counts?.['minecraft:oak_planks'], 3); assert.equal(inventory.counts?.['minecraft:stone'], 1); assert.equal(inventory.counts?.['minecraft:water_bucket'], 1);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('fake-slot clicks set, add and subtract source quantities; filled buckets set fluid ghosts without consumption');
  fixture.mark('ghost state and physical template custody survive restart'); fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
