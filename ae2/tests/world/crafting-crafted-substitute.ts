/** A missing encoded ingredient is supplied by a physically crafted substitute. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('crafting-crafted-substitute', mode);
const count = (status: any, item: string) => status.items?.find((entry: any) => entry.item === item)?.count ?? 0;
try {
  const state = await fixture.start();
  const [px, py, pz] = state.player.position;
  const x = Math.floor(px) + 32, y = Math.ceil(py) + 5, z = Math.floor(pz) + 32;
  const at = (dx: number, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const drive = at(1), terminal = at(2), cpu = at(2, 1);
  const sticksProvider = at(3), planksProvider = at(3, 0, 1);
  await fixture.command('clear @s');
  await fixture.command(`fill ${at(-2, 0, -3)} ${at(6, 5, 4)} minecraft:air`);
  await fixture.command(`fill ${at(-2, -1, -3)} ${at(6, -1, 4)} minecraft:stone`);
  for (const [pos, id] of [
    [at(0), 'creative_energy_cell'], [drive, 'drive'], [terminal, 'cable_bus'],
    [cpu, '64k_crafting_storage'], [sticksProvider, 'pattern_provider'],
    [planksProvider, 'pattern_provider'], [at(3, 0, -1), 'molecular_assembler'],
    [at(4, 0, 1), 'molecular_assembler'],
  ] as const) await fixture.command(`setblock ${pos} ae2:${id}`);
  await fixture.command(`item replace block ${drive} container.0 with ae2:item_storage_cell_1k`);
  await fixture.replace('hotbar.0', 'ae2:pattern_encoding_terminal'); await fixture.select(0, 'ae2:pattern_encoding_terminal');
  assert.equal((await fixture.ae(`grid part ${terminal} north`)).attached, true);
  const ready = () => until(() => fixture.ae(`grid status ${drive}`), view => view.online && view.powered, 'powered drive', 30000);
  await ready();
  for (const pos of [sticksProvider, planksProvider, at(3, 0, -1), at(4, 0, 1)]) {
    await until(() => fixture.ae(`grid status ${pos}`), view => view.online && view.powered, `online device ${pos}`, 30000);
  }

  // Encode the vanilla stick recipe with item substitution enabled. The
  // player retains the two oak planks used as ghost templates.
  await fixture.replace('hotbar.0', 'minecraft:oak_planks', 2);
  await fixture.replace('hotbar.1', 'ae2:blank_pattern');
  await fixture.replace('hotbar.8', 'minecraft:blaze_rod'); await fixture.select(8, 'minecraft:blaze_rod');
  await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`);
  await fixture.raw(`interact ${terminal} north`);
  await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot === 348 && String(slot.components ?? '').includes('Encode crafting pattern')), 'encoding terminal');
  await fixture.raw('click-slot 348');
  await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot === 348 && String(slot.components ?? '').includes('Back to storage')), 'encoding grid');
  await fixture.raw('click-slot 0');
  await until(() => fixture.raw('inventory'), view => view.carried?.item === 'minecraft:oak_planks', 'oak template on cursor');
  await fixture.raw('click-slot 300');
  await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot === 300 && slot.item === 'minecraft:oak_planks'), 'first oak ghost input accepted');
  await fixture.raw('click-slot 303');
  await until(() => fixture.raw('screen'), view => [300, 303].every(index => view.slots?.some((slot: any) => slot.slot === index && slot.item === 'minecraft:oak_planks')), 'two oak ghost inputs');
  await fixture.raw('click-slot 0');
  await until(() => fixture.raw('inventory'), view => !view.carried && view.hotbar?.[0]?.count === 2, 'oak templates returned to player');
  await fixture.raw('click-slot 1 shift');
  await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot === 349 && slot.item === 'ae2:blank_pattern'), 'physical blank in encoding terminal');
  await fixture.raw('click-slot 347');
  await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot === 347 && String(slot.components ?? '').includes('Substitutions: on')), 'item substitutions enabled');
  await fixture.raw('click-slot 313');
  await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot === 350 && slot.item === 'ae2:crafting_pattern'), 'encoded substitutable stick pattern');
  await fixture.raw('click-slot 350 shift');
  await until(() => fixture.raw('inventory'), view => view.counts?.['ae2:crafting_pattern'] === 1, 'encoded physical pattern returned');
  await fixture.raw('close');

  async function install(provider: string, playerX: number, playerZ: number, face: string) {
    await fixture.command(`tp @s ${playerX} ${y} ${playerZ}`);
    await fixture.raw(`interact ${provider} ${face}`);
    const menu = await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot < 36 && slot.item === 'ae2:crafting_pattern'), 'physical provider menu');
    await fixture.raw(`click-slot ${menu.slots.find((slot: any) => slot.slot < 36 && slot.item === 'ae2:crafting_pattern').slot} shift`);
    await until(() => fixture.raw('inventory'), view => !view.counts?.['ae2:crafting_pattern'], 'pattern installed');
    await fixture.raw('close');
  }
  await install(sticksProvider, x + 5.5, z + 0.5, 'east');
  await fixture.replace('hotbar.0', 'ae2:blank_pattern'); await fixture.select(0, 'ae2:blank_pattern');
  const encoded = await fixture.ae('crafting encode minecraft:birch_log - - - - - - - -');
  assert.deepEqual([encoded.encoded, encoded.output, encoded.count], [true, 'minecraft:birch_planks', 4]);
  await until(() => fixture.raw('inventory'), view => view.hotbar?.[0]?.item === 'ae2:crafting_pattern', 'birch plank pattern encoded');
  await install(planksProvider, x + 3.5, z + 2.5, 'south');
  await fixture.replace('hotbar.0', 'minecraft:birch_log'); await fixture.select(0, 'minecraft:birch_log');
  assert.equal((await fixture.ae(`grid deposit ${drive}`)).inserted, 1);
  const initial = await ready();
  assert.equal(count(initial, 'minecraft:birch_log'), 1);
  assert.equal(count(initial, 'minecraft:birch_planks'), 0);
  assert.equal(count(initial, 'minecraft:oak_planks'), 0);
  fixture.mark('the encoded oak ingredient is absent and birch planks are craftable from one physical log');

  await fixture.select(8, 'minecraft:blaze_rod'); await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`);
  await fixture.raw(`interact ${terminal} north`);
  const terminalView = await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:stick'), 'craftable sticks');
  await fixture.raw(`click-slot ${terminalView.slots.find((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:stick').slot} shift`);
  await until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 Craft Amount'), 'craft amount');
  await fixture.raw('ui-key right'); await fixture.raw('ui-key backspace'); await fixture.raw('type 4'); await fixture.raw('click-slot 302');
  const confirm = await until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 Craft Confirm') && /bytes|Missing materials|craft planning:/.test(JSON.stringify(view)), 'substitute craft confirmation');
  assert.doesNotMatch(JSON.stringify(confirm), /Missing materials/);
  assert.match(JSON.stringify(confirm), /33 bytes/);
  await fixture.raw('click-slot 302'); await fixture.raw('close');
  const finished = await until(() => fixture.ae(`grid status ${drive}`), view => count(view, 'minecraft:stick') === 4 && count(view, 'minecraft:birch_planks') === 2 && count(view, 'minecraft:birch_log') === 0, 'two physical recipes complete', 90000);
  assert.equal(count(finished, 'minecraft:oak_planks'), 0);
  fixture.mark('one birch log produces the substitute planks, four sticks and two surplus planks');
  await fixture.restart();
  const saved = await ready();
  assert.equal(count(saved, 'minecraft:stick'), 4); assert.equal(count(saved, 'minecraft:birch_planks'), 2);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('the crafted substitute and final output survive restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
