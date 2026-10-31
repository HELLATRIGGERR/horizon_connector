/** AEBaseScreen handles hotkeys over FakeSlot as filter actions, not item moves. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('encoding-fake-hotkeys', mode);
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
  await fixture.command(`setblock ${at(0)} ae2:creative_energy_cell`); await fixture.command(`setblock ${at(1)} ae2:drive`);
  await fixture.command(`setblock ${terminal} ae2:cable_bus`); await fixture.command('clear @s');
  await fixture.replace('hotbar.0', 'ae2:pattern_encoding_terminal'); await fixture.select(0, 'ae2:pattern_encoding_terminal');
  assert.equal((await fixture.ae(`grid part ${terminal} north`)).attached, true);
  await until(() => fixture.ae(`grid status ${terminal}`), state => state.online && state.powered, 'powered encoding face', 30000);
  await fixture.command('gamemode survival @s'); await fixture.replace('hotbar.0', 'minecraft:oak_planks', 3);
  await fixture.replace('hotbar.1', 'minecraft:emerald'); await fixture.replace('weapon.offhand', 'minecraft:gold_ingot');
  await fixture.replace('hotbar.8', 'minecraft:stick'); await fixture.select(8, 'minecraft:stick');
  await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`); await fixture.raw(`interact ${terminal} north`);
  await until(() => fixture.raw('screen'), view => String(slot(view, 348)?.components ?? '').includes('Encode crafting pattern'), 'encoding terminal');
  await fixture.raw('click-slot 348'); await until(() => fixture.raw('screen'), view => String(slot(view, 348)?.components ?? '').includes('Back to storage'), 'encoding editor');
  // PatternEncodingLogic clamps crafting inputs to one; processing mode retains quantities.
  await fixture.raw('click-slot 346'); await until(() => fixture.raw('screen'), view => String(slot(view, 346)?.components ?? '').includes('processing'), 'processing mode');
  await fixture.raw('click-slot 0'); await until(() => fixture.raw('inventory'), view => view.carried?.item === 'minecraft:oak_planks' && view.carried.count === 3, 'real template held');
  const widgets = await fixture.raw('ui'); const node = widgets.widgets.find((entry: any) => entry.id === 'slot-300'); assert(node, 'ghost hit target');
  await fixture.raw(`click-at ${node.bounds[0] + 8} ${node.bounds[1] + 8}`);
  const state = async (n: number, carried: boolean, label: string) => {
    await fixture.command(`data get block ${terminal} ae2_lab_config.parts`);
    return until(() => fixture.raw('screen'), view => (slot(view, 300)?.count ?? 0) === n && (n === 0 || slot(view, 300)?.item === 'minecraft:oak_planks') && (carried ? view.carried?.item === 'minecraft:oak_planks' && view.carried.count === 3 : !view.carried), label);
  };
  await state(3, true, 'initial fake filter');
  await fixture.raw('key 2 tap'); await fixture.raw('key f tap');
  await state(3, true, 'hotbar and offhand keys leave a held cursor untouched');
  await fixture.raw('click-slot 0'); await state(3, false, 'real template returned');
  await fixture.raw('key 2 tap'); await state(3, false, 'overstacking fake slot cannot be swapped into the hotbar');
  await fixture.raw('key f tap'); await state(0, false, 'offhand key invokes the source fake-slot action');
  let inventory = await fixture.raw('inventory'); assert.equal(inventory.hotbar?.[1]?.item, 'minecraft:emerald'); assert.equal(inventory.offhand?.item, 'minecraft:gold_ingot');
  await fixture.raw('click-slot 0'); await fixture.raw('click-slot 300'); await fixture.raw('click-slot 0');
  await state(3, false, 'fake filter restored with physical cursor returned');
  await fixture.raw('key ctrl down'); await fixture.raw('key q tap'); await fixture.raw('key ctrl up');
  await state(2, false, 'whole-stack drop key uses the source right fake-slot action');
  await fixture.raw('key q tap'); await state(0, false, 'drop key uses the source left fake-slot action');
  await fixture.raw('close'); await fixture.restart();
  inventory = await fixture.raw('inventory'); assert.equal(inventory.counts?.['minecraft:oak_planks'], 3); assert.equal(inventory.counts?.['minecraft:emerald'], 1); assert.equal(inventory.offhand?.item, 'minecraft:gold_ingot');
  const saved = JSON.stringify((await fixture.command(`data get block ${terminal} ae2_lab_config.parts`)).chat); assert(!saved.includes('minecraft:oak_planks'));
  fixture.assertHidden(await fixture.raw('status')); fixture.mark('fake-slot hotkeys honor the source cursor and slot-limit rules while preserving physical items'); fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
