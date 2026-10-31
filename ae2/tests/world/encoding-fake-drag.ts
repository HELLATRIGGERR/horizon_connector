/** AEBaseScreen fake-slot drag actions preserve the physical cursor stack. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('encoding-fake-drag', mode);
const slot = (view: any, index: number) => view.slots?.find((entry: any) => entry.slot === index);
try {
  const start = await fixture.start(); const [px, py, pz] = start.player.position;
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
  await fixture.command(`setblock ${terminal} ae2:cable_bus`); await fixture.command('clear @s');
  await fixture.replace('hotbar.0', 'ae2:pattern_encoding_terminal'); await fixture.select(0, 'ae2:pattern_encoding_terminal');
  assert.equal((await fixture.ae(`grid part ${terminal} north`)).attached, true);
  await until(() => fixture.ae(`grid status ${terminal}`), state => state.online && state.powered, 'powered encoding face', 30000);
  await fixture.command('gamemode survival @s'); await fixture.replace('hotbar.0', 'minecraft:oak_planks', 3);
  await fixture.replace('hotbar.1', 'minecraft:water_bucket'); await fixture.replace('hotbar.8', 'minecraft:stick'); await fixture.select(8, 'minecraft:stick');
  await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`); await fixture.raw(`interact ${terminal} north`);
  await until(() => fixture.raw('screen'), view => String(slot(view, 348)?.components ?? '').includes('Encode crafting pattern'), 'encoding terminal');
  await fixture.raw('click-slot 348'); await until(() => fixture.raw('screen'), view => String(slot(view, 348)?.components ?? '').includes('Back to storage'), 'encoding editor');
  await fixture.raw('click-slot 346'); await until(() => fixture.raw('screen'), view => String(slot(view, 346)?.components ?? '').includes('processing'), 'processing mode');
  await fixture.raw('click-slot 0'); await until(() => fixture.raw('inventory'), view => view.carried?.item === 'minecraft:oak_planks' && view.carried.count === 3, 'physical template on cursor');
  const widgets = await fixture.raw('ui');
  const center = (index: number): [number, number] => {
    const node = widgets.widgets.find((entry: any) => entry.id === `slot-${index}`);
    assert(node, `fake slot ${index} has a visible hit target`);
    return [node.bounds[0] + 8, node.bounds[1] + 8];
  };
  const drag = async (a: number, b: number, right = false) => {
    const [ax, ay] = center(a), [bx, by] = center(b);
    // Source drag_click records slots as the pointer moves, not on press.
    await fixture.raw(`drag-at ${right ? 'right ' : ''}${ax} ${ay} ${bx} ${by} ${ax} ${ay}`);
    await fixture.command(`data get block ${terminal} ae2_lab_config.parts`);
  };
  for (const expected of [3, 6]) {
    await drag(300, 301);
    const view = await until(() => fixture.raw('screen'), v => slot(v, 300)?.count === expected && slot(v, 301)?.count === expected, `left drag adds the full template to each ghost (${expected})`);
    assert.equal(view.carried?.item, 'minecraft:oak_planks'); assert.equal(view.carried?.count, 3);
  }
  await drag(300, 301, true);
  let view = await until(() => fixture.raw('screen'), v => slot(v, 300)?.count === 7 && slot(v, 301)?.count === 7, 'right drag adds one to each ghost');
  assert.equal(view.carried?.count, 3);
  await fixture.raw('click-slot 300 shift'); await fixture.command(`data get block ${terminal} ae2_lab_config.parts`);
  view = await until(() => fixture.raw('screen'), v => slot(v, 300)?.count === 10 && slot(v, 301)?.count === 7 && v.carried?.count === 3, 'shift-click still uses the fake-slot action');
  await fixture.raw('click-slot 0'); await until(() => fixture.raw('inventory'), v => !v.carried && v.counts?.['minecraft:oak_planks'] === 3, 'item template returned');
  await fixture.raw('click-slot 1'); await until(() => fixture.raw('inventory'), v => v.carried?.item === 'minecraft:water_bucket', 'filled bucket held');
  await drag(302, 303, true);
  view = await until(() => fixture.raw('screen'), v => slot(v, 302)?.item === 'minecraft:water_bucket' && slot(v, 303)?.item === 'minecraft:water_bucket', 'right drag treats a bucket as a one-item template');
  assert.equal(view.carried?.item, 'minecraft:water_bucket');
  await fixture.raw('click-slot 304 right'); await fixture.command(`data get block ${terminal} ae2_lab_config.parts`);
  view = await until(() => fixture.raw('screen'), v => slot(v, 304)?.item === 'ae2:wrapped_generic_stack' && String(slot(v, 304)?.components ?? '').includes('minecraft:water'), 'right click separately uses EMPTY_ITEM');
  assert.equal(view.carried?.item, 'minecraft:water_bucket');
  await fixture.raw('click-slot 1'); await until(() => fixture.raw('inventory'), v => !v.carried && v.counts?.['minecraft:water_bucket'] === 1, 'bucket returned');
  await fixture.raw('close'); await fixture.restart();
  const saved = JSON.stringify((await fixture.command(`data get block ${terminal} ae2_lab_config.parts`)).chat);
  assert.match(saved, /minecraft:oak_planks/); assert.match(saved, /minecraft:water_bucket/); assert.match(saved, /minecraft:water/);
  const inventory = await fixture.raw('inventory'); assert.equal(inventory.counts?.['minecraft:oak_planks'], 3); assert.equal(inventory.counts?.['minecraft:water_bucket'], 1);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('left and right fake-slot drags preserve exact source quantities, bucket action, cursor custody and restart'); fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
