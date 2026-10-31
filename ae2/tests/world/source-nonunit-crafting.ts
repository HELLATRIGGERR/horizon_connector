/** Source non-unit crafting inputs calculate like AE2; failed assembler dispatch preserves real escrow. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('source-nonunit-crafting', mode);
const count = (status: any, item: string) => status.items?.find((entry: any) => entry.item === item)?.count ?? 0;
const log = 'minecraft:oak_log[minecraft:custom_data={variant:"wrapped-two"}]';
const pattern = 'ae2:crafting_pattern[ae2:encoded_crafting_pattern={inputs:[{id:"ae2:wrapped_generic_stack",count:1,components:{"ae2:wrapped_stack":{"#t":"ae2:i",id:"minecraft:oak_log",components:{"minecraft:custom_data":{variant:"wrapped-two"}},"#":2L}}},{},{},{},{},{},{},{},{}],result:{id:"minecraft:oak_planks",count:4},recipeId:"minecraft:oak_planks",canSubstitute:1b,canSubstituteFluids:0b}]';
try {
  const state = await fixture.start(); const [px, py, pz] = state.player.position;
  const x = Math.floor(px) + 32, y = Math.ceil(py) + 5, z = Math.floor(pz) + 32;
  const at = (dx: number, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const drive = at(1), terminal = at(2), cpu = at(2, 1), provider = at(3), assembler = at(3, 0, -1);
  await fixture.command(`tp @s ${x + 0.5} ${y} ${z + 0.5}`);
  for (const dx of [-2, 6]) for (const dz of [-3, 3]) {
    await until(async () => (await fixture.command(`execute if loaded ${at(dx, 0, dz)}`)).chat ?? [],
      lines => lines.includes('Test passed'), 'construction corner chunk loaded', 30000);
  }
  await fixture.command('clear @s'); await fixture.command(`fill ${at(-2, 0, -3)} ${at(6, 5, 3)} minecraft:air`);
  await fixture.command(`fill ${at(-2, -1, -3)} ${at(6, -1, 3)} minecraft:stone`);
  for (const [pos, id] of [[at(0), 'creative_energy_cell'], [drive, 'drive'], [terminal, 'cable_bus'],
    [cpu, '64k_crafting_storage'], [provider, 'pattern_provider'], [assembler, 'molecular_assembler']] as const) {
    await fixture.command(`setblock ${pos} ae2:${id}`);
  }
  await fixture.command(`item replace block ${drive} container.0 with ae2:item_storage_cell_1k`);
  await fixture.replace('hotbar.0', 'ae2:terminal'); await fixture.select(0, 'ae2:terminal');
  assert.equal((await fixture.ae(`grid part ${terminal} north`)).attached, true);
  const ready = () => until(() => fixture.ae(`grid status ${drive}`), view => view.online && view.powered, 'powered non-unit network', 30000);
  await ready(); for (const pos of [provider, assembler]) await until(() => fixture.ae(`grid status ${pos}`), view => view.online && view.powered, `online ${pos}`, 30000);
  await fixture.replace('hotbar.0', pattern); await fixture.replace('hotbar.8', 'minecraft:blaze_rod'); await fixture.select(8, 'minecraft:blaze_rod');
  await fixture.command(`tp @s ${x + 5.5} ${y} ${z + 0.5}`); await fixture.raw(`interact ${provider} east`);
  const menu = await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot < 36 && slot.item === 'ae2:crafting_pattern'), 'non-unit source pattern in player inventory');
  await fixture.raw(`click-slot ${menu.slots.find((slot: any) => slot.slot < 36 && slot.item === 'ae2:crafting_pattern').slot} shift`);
  await until(() => fixture.raw('inventory'), view => !view.counts?.['ae2:crafting_pattern'], 'non-unit source pattern installed'); await fixture.raw('close');
  await fixture.replace('hotbar.0', log, 3); await fixture.select(0, 'minecraft:oak_log');
  assert.equal((await fixture.ae(`grid deposit ${drive}`)).inserted, 3); assert.equal(count(await ready(), 'minecraft:oak_log'), 3);
  await fixture.select(8, 'minecraft:blaze_rod'); await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`);
  const openTerminal = async () => {
    await fixture.raw(`interact ${terminal} north`);
    return until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:oak_planks'), 'non-unit recipe craftable');
  };
  const screen = await openTerminal();
  await fixture.raw(`click-slot ${screen.slots.find((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:oak_planks').slot} shift`);
  await until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 Craft Amount'), 'non-unit amount');
  await fixture.raw('ui-key right'); await fixture.raw('ui-key backspace'); await fixture.raw('type 4'); await fixture.raw('click-slot 302');
  const confirm = await until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 Craft Confirm') && /bytes|Missing materials|craft planning:/.test(JSON.stringify(view)), 'non-unit confirmation');
  assert.doesNotMatch(JSON.stringify(confirm), /Missing materials|craft planning:/); assert.match(JSON.stringify(confirm), /25 bytes/); assert.match(JSON.stringify(confirm), /Stored 3 x minecraft:oak_log/);
  await fixture.raw('click-slot 302'); await fixture.raw('close');
  await until(ready, view => count(view, 'minecraft:oak_log') === 0, 'three exact logs reserved in CPU escrow', 30000);
  await fixture.step(40);
  const data = async (pos: string) => (await fixture.command(`data get block ${pos}`)).chat?.join('\n') ?? '';
  const escrow = await data(cpu); assert.match(escrow, /ae2_cpu_escrow/); assert.match(escrow, /wrapped-two/);
  const initialAssembler = await data(assembler); assert.doesNotMatch(initialAssembler, /ae2_assembler_pattern|ae2_flight_owner/);
  assert.equal(count(await ready(), 'minecraft:oak_planks'), 0, 'source-invalid assembler input must not invent output');
  fixture.mark('the source 25-byte plan reserves three component-bearing items while a partial assembler mutation is refused');
  await fixture.restart(); const restored = await data(cpu); assert.match(restored, /ae2_cpu_escrow/); assert.match(restored, /wrapped-two/);
  assert.equal(count(await ready(), 'minecraft:oak_log'), 0); assert.doesNotMatch(await data(assembler), /ae2_assembler_pattern|ae2_flight_owner/);
  await openTerminal(); await fixture.raw('click-slot 349');
  await until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 Crafting Status'), 'non-unit CPU status');
  await fixture.raw('click-slot 303'); await fixture.raw('close');
  await until(ready, view => count(view, 'minecraft:oak_log') === 3 && count(view, 'minecraft:oak_planks') === 0, 'cancellation restores every exact item', 30000);
  assert.match(await data(drive), /wrapped-two/); fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('the blocked job survives restart and cancellation restores all three exact physical ingredients');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
