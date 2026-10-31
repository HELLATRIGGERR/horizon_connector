/** A large mixed-substitute plan is calculated without consuming physical stock. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('crafting-large-substitutions', mode);
const count = (status: any, item: string) => status.items?.find((entry: any) => entry.item === item)?.count ?? 0;
const cell = 'ae2:item_storage_cell_64k[ae2:storage_cell_inv=[{"#t":"ae2:i",id:"minecraft:oak_log","#":5001L},{"#t":"ae2:i",id:"minecraft:stripped_oak_log","#":4999L}]]';
const pattern = 'ae2:crafting_pattern[ae2:encoded_crafting_pattern={inputs:[{id:"minecraft:oak_log",count:1},{},{},{},{},{},{},{},{}],result:{id:"minecraft:oak_planks",count:4},recipeId:"minecraft:oak_planks",canSubstitute:1b,canSubstituteFluids:0b}]';
try {
  const state = await fixture.start(); const [px, py, pz] = state.player.position;
  const x = Math.floor(px) + 32, y = Math.ceil(py) + 5, z = Math.floor(pz) + 32;
  const at = (dx: number, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const drive = at(1), terminal = at(2), cpu = at(2, 1), provider = at(3), assembler = at(3, 0, -1);
  await fixture.command('clear @s'); await fixture.command(`fill ${at(-2, 0, -3)} ${at(6, 5, 3)} minecraft:air`);
  await fixture.command(`fill ${at(-2, -1, -3)} ${at(6, -1, 3)} minecraft:stone`);
  for (const [pos, id] of [[at(0), 'creative_energy_cell'], [drive, 'drive'], [terminal, 'cable_bus'],
    [cpu, '64k_crafting_storage'], [provider, 'pattern_provider'], [assembler, 'molecular_assembler']] as const) await fixture.command(`setblock ${pos} ae2:${id}`);
  await fixture.command(`item replace block ${drive} container.0 with ${cell}`);
  await fixture.replace('hotbar.0', 'ae2:terminal'); await fixture.select(0, 'ae2:terminal');
  assert.equal((await fixture.ae(`grid part ${terminal} north`)).attached, true);
  const ready = () => until(() => fixture.ae(`grid status ${drive}`), view => view.online && view.powered, 'large-plan network powered', 30000);
  await ready();
  for (const pos of [provider, assembler]) await until(() => fixture.ae(`grid status ${pos}`), view => view.online && view.powered, `online ${pos}`, 30000);
  const stock = await until(ready, view => count(view, 'minecraft:oak_log') === 5001 && count(view, 'minecraft:stripped_oak_log') === 4999, 'two physical substitute stocks');
  assert.equal(count(stock, 'minecraft:oak_planks'), 0);
  await fixture.replace('hotbar.0', pattern); await fixture.replace('hotbar.8', 'minecraft:blaze_rod'); await fixture.select(8, 'minecraft:blaze_rod');
  await fixture.command(`tp @s ${x + 5.5} ${y} ${z + 0.5}`); await fixture.raw(`interact ${provider} east`);
  const providerMenu = await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot < 36 && slot.item === 'ae2:crafting_pattern'), 'real pattern in player inventory');
  await fixture.raw(`click-slot ${providerMenu.slots.find((slot: any) => slot.slot < 36 && slot.item === 'ae2:crafting_pattern').slot} shift`);
  await until(() => fixture.raw('inventory'), view => !view.counts?.['ae2:crafting_pattern'], 'real pattern installed'); await fixture.raw('close');
  await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`); await fixture.raw(`interact ${terminal} north`);
  const screen = await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:oak_planks'), 'substitutable plank pattern is craftable');
  await fixture.raw(`click-slot ${screen.slots.find((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:oak_planks').slot} shift`);
  await until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 Craft Amount'), 'large craft amount');
  await fixture.raw('ui-key right'); await fixture.raw('ui-key backspace'); await fixture.raw('type 40000'); await fixture.raw('click-slot 302');
  const confirm = await until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 Craft Confirm') && /bytes|Missing materials|craft planning:/.test(JSON.stringify(view)), '10,000-push plan', 90000);
  assert.doesNotMatch(JSON.stringify(confirm), /Missing materials|SearchLimit|craft planning:/);
  assert.match(JSON.stringify(confirm), /60016 bytes/);
  assert.match(JSON.stringify(confirm), /Planned 40000 x minecraft:oak_planks/);
  const after = await ready();
  assert.equal(count(after, 'minecraft:oak_log'), 5001); assert.equal(count(after, 'minecraft:stripped_oak_log'), 4999); assert.equal(count(after, 'minecraft:oak_planks'), 0);
  fixture.mark('10,000 mixed-substitute pushes calculate at the source-oracle 60,016 bytes without consuming stock');
  await fixture.raw('close'); await fixture.restart();
  const restored = await ready();
  assert.equal(count(restored, 'minecraft:oak_log'), 5001); assert.equal(count(restored, 'minecraft:stripped_oak_log'), 4999); assert.equal(count(restored, 'minecraft:oak_planks'), 0);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('the uncommitted physical stock survives restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
