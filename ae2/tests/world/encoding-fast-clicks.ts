/** Independent queued ghost edits must not be rejected as a stale recipe. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('encoding-fast-clicks', mode);
const slot = (view: any, index: number) => view.slots?.find((entry: any) => entry.slot === index);
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 32, y = Math.ceil(py) + 5, z = Math.floor(pz) + 32;
  const at = (dx: number) => `${x + dx} ${y} ${z}`, terminal = at(2);
  // In-frame setblock/fill require loaded chunks; the +32 fixture can cross
  // the initial simulation boundary even after the world startup barrier.
  await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`);
  for (const [dx,dz] of [[-2,-3],[-2,3],[5,-3],[5,3]] as const) {
    await until(() => fixture.raw(`block ${x + dx} ${y} ${z + dz}`), block => block.loaded, 'fixture corner chunk loaded', 30000);
  }
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
  await fixture.replace('hotbar.0', 'minecraft:oak_planks', 2);
  await fixture.replace('hotbar.8', 'minecraft:stick'); await fixture.select(8, 'minecraft:stick');
  await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`); await fixture.raw(`interact ${terminal} north`);
  await until(() => fixture.raw('screen'), view => String(slot(view, 348)?.components ?? '').includes('Encode crafting pattern'), 'encoding terminal');
  await fixture.raw('click-slot 348');
  await until(() => fixture.raw('screen'), view => String(slot(view, 348)?.components ?? '').includes('Back to storage'), 'encoding editor');
  // PatternEncodingLogic clamps crafting inputs to one; processing mode retains quantities.
  await fixture.raw('click-slot 346'); await until(() => fixture.raw('screen'), view => String(slot(view, 346)?.components ?? '').includes('processing'), 'processing mode');
  await fixture.raw('click-slot 0');
  await until(() => fixture.raw('inventory'), view => view.carried?.item === 'minecraft:oak_planks' && view.carried.count === 2, 'physical template on cursor');
  await fixture.raw('click-slot 300');
  await fixture.raw('click-slot 303');
  const view = await until(() => fixture.raw('screen'), state => slot(state, 300)?.item === 'minecraft:oak_planks' && slot(state, 303)?.item === 'minecraft:oak_planks', 'both rapidly queued ghosts accepted');
  assert.equal(view.carried?.count, 2, 'ghost edits do not consume the real item');
  // Both actions are sent without an acknowledgment between them. Source fake
  // actions operate on the current authoritative ghost, including the same slot.
  await fixture.raw('click-slot 300'); await fixture.raw('click-slot 300');
  await fixture.command(`data get block ${terminal} ae2_lab_config.parts`);
  const same = await until(() => fixture.raw('screen'), state => slot(state, 300)?.item === 'minecraft:oak_planks' && slot(state, 300)?.count === 6 && state.carried?.count === 2, 'two ordered same-slot ghost increases');
  assert.equal(same.carried?.item, 'minecraft:oak_planks');
  await fixture.raw('close');
  // The close packet returns the server cursor asynchronously. Await its
  // inventory update before asserting client custody, then check the server.
  await until(() => fixture.raw('inventory'), state => !state.carried && state.counts?.['minecraft:oak_planks'] === 2, 'server returns the two cursor items after close');
  const authoritative = String((await fixture.command('data get entity @s Inventory')).chat?.[0] ?? '');
  assert.match(authoritative, /minecraft:oak_planks/);
  assert.match(authoritative, /count: 2/);
  await fixture.restart();
  const saved = JSON.stringify((await fixture.command(`data get block ${terminal} ae2_lab_config.parts`)).chat);
  assert.match(saved, /minecraft:oak_planks/);
  assert.equal((await fixture.raw('inventory')).counts?.['minecraft:oak_planks'], 2);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('independent and same-slot rapid ghost clicks and the real template survive restart without loss');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
