/** A valid component-bearing ingredient makes a real active CPU job exceed NBT String limits. */
import assert from 'node:assert/strict';
import { WorldFixture, until } from './harness';
const fixture = new WorldFixture('crafting-large-job-persistence');
// This byte-array component is valid item NBT; each command remains below the
// vanilla command-length limit. Repeated exact keys expand the CPU's JSON job.
const padding = Array(9000).fill('1b').join(',');
const components = `{"minecraft:custom_data":{marker:"large-job",padding:[B;${padding}]}}`;
const cell = `ae2:item_storage_cell_1k[ae2:storage_cell_inv=[{"#t":"ae2:i",id:"minecraft:stone",components:${components},"#":1L}]]`;
const pattern = `ae2:processing_pattern[ae2:encoded_processing_pattern={sparseInputs:[{"#t":"ae2:i",id:"minecraft:stone",components:${components},"#":1L}],sparseOutputs:[{"#t":"ae2:i",id:"minecraft:diamond","#":1L}]}]`;
assert(cell.length < 32000 && pattern.length < 32000);
try {
  const state = await fixture.start(), [px, py, pz] = state.player.position;
  const x = Math.floor(px) + 32, y = Math.ceil(py) + 5, z = Math.floor(pz) + 32;
  const at = (dx: number, dy = 0) => `${x + dx} ${y + dy} ${z}`;
  await fixture.command(`tp @s ${x + 2.5} ${y + 1} ${z + 0.5}`);
  for (const dx of [-2, 5]) for (const dz of [-2, 2]) await until(() => fixture.command(`execute if loaded ${x + dx} ${y} ${z + dz}`), reply => reply.chat?.some((line: string) => line.includes('Test passed')), 'loaded build corner', 30000);
  await fixture.command(`fill ${x - 2} ${y} ${z - 2} ${x + 5} ${y + 4} ${z + 2} minecraft:air`);
  await fixture.command(`fill ${x - 2} ${y - 1} ${z - 2} ${x + 5} ${y - 1} ${z + 2} minecraft:stone`);
  const drive = at(1), terminal = at(2), cpu = at(2, 1), provider = at(3);
  for (const [pos, id] of [[at(0), 'creative_energy_cell'], [drive, 'drive'], [terminal, 'cable_bus'], [cpu, '1k_crafting_storage'], [provider, 'pattern_provider']]) await fixture.command(`setblock ${pos} ae2:${id}`);
  await fixture.command('clear @s'); await fixture.command(`item replace block ${drive} container.0 with ${cell}`);
  await fixture.replace('hotbar.0', 'ae2:terminal'); await fixture.select(0, 'ae2:terminal');
  assert.equal((await fixture.ae(`grid part ${terminal} north`)).attached, true);
  const stock = () => fixture.ae(`grid status ${drive}`);
  const count = (view: any) => (view.items ?? []).filter((entry: any) => entry.item === 'minecraft:stone').reduce((n: number, entry: any) => n + entry.count, 0);
  await until(stock, view => view.online && view.powered && count(view) === 1, 'component-bearing source cell mounted', 30000);
  await fixture.replace('hotbar.0', pattern); await fixture.replace('hotbar.8', 'minecraft:blaze_rod'); await fixture.select(8, 'minecraft:blaze_rod');
  await fixture.command(`tp @s ${x + 5.5} ${y} ${z + 0.5}`); await fixture.raw(`interact ${provider} east`);
  const menu = await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot < 36 && slot.item === 'ae2:processing_pattern'), 'physical pattern');
  await fixture.raw(`click-slot ${menu.slots.find((slot: any) => slot.slot < 36 && slot.item === 'ae2:processing_pattern').slot} shift`); await fixture.raw('close');
  const open = async () => { await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`); await fixture.raw(`interact ${terminal} north`); };
  await open(); const screen = await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:diamond'), 'processing recipe visible');
  await fixture.raw(`click-slot ${screen.slots.find((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:diamond').slot} shift`);
  await until(() => fixture.raw('screen'), view => JSON.stringify(view.title).includes('AE2 Craft Amount'), 'amount'); await fixture.raw('click-slot 302');
  const confirm = await until(() => fixture.raw('screen'), view => JSON.stringify(view.title).includes('AE2 Craft Confirm') && /bytes|Missing materials|craft planning:/.test(JSON.stringify(view)), 'confirmation', 90000);
  assert.doesNotMatch(JSON.stringify(confirm), /Missing materials|craft planning:/); await fixture.raw('click-slot 302'); await fixture.raw('close');
  await until(stock, view => count(view) === 0, 'exact ingredient reserved', 90000);
  const payload = async () => (await fixture.command(`data get block ${cpu} ae2_lab_config.ae2_recursive_job`)).chat?.join('\n') ?? '';
  const checkPayload = async () => {
    assert.match(await payload(), /\[B;/, 'CPU job uses an NBT byte array');
    const check = await fixture.command(`execute if data block ${cpu} ae2_lab_config.ae2_recursive_job[65535]`);
    assert(check.chat?.some((line: string) => line.includes('Test passed')), 'actual saved payload contains byte 65,536');
  };
  await checkPayload(); await fixture.restart(); await checkPayload();
  assert.equal(count(await stock()), 0, 'the ingredient remains in physical escrow after reload');
  await open(); await fixture.raw('click-slot 349'); await until(() => fixture.raw('screen'), view => JSON.stringify(view.title).includes('AE2 Crafting Status'), 'CPU status');
  await fixture.raw('click-slot 303'); await fixture.raw('close');
  await until(stock, view => count(view) === 1, 'cancellation returns exact ingredient', 90000);
  await fixture.restart(); await until(stock, view => view.online && view.powered && count(view) === 1, 'returned exact ingredient survives restart', 30000);
  const stored = (await fixture.command(`data get block ${drive}`)).chat?.join('\n') ?? ''; assert.match(stored, /large-job/); assert.match(stored, /padding/);
  fixture.assertHidden(await fixture.raw('status')); fixture.mark('a real active CPU job larger than 65 KB saves, reloads and cancels with exact component custody'); fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; } finally { await fixture.close(); }
