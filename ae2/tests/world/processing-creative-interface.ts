/** A large request crosses an unconfigured Interface into a creative-cell grid. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('processing-creative-interface', mode);
const exact = 'minecraft:stone[minecraft:custom_data={variant:"creative"}]';
const cell = 'ae2:creative_storage_cell[ae2:storage_cell_config_inv=[{"#t":"ae2:i",id:"minecraft:stone",components:{"minecraft:custom_data":{variant:"creative"}},"#":0L}]]';
const pattern = 'ae2:processing_pattern[ae2:encoded_processing_pattern={sparseInputs:[{"#t":"ae2:i",id:"minecraft:stone",components:{"minecraft:custom_data":{variant:"creative"}},"#":999999L}],sparseOutputs:[{"#t":"ae2:i",id:"minecraft:diamond","#":1L}]}]';
const total = (view: any) => (view.items ?? []).filter((entry: any) => entry.item === 'minecraft:stone').reduce((sum: number, entry: any) => sum + entry.count, 0);
try {
  const state = await fixture.start(); const [px, py, pz] = state.player.position;
  const x = Math.floor(px) + 32, y = Math.ceil(py) + 5, z = Math.floor(pz) + 32;
  const at = (dx: number, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const drive = at(1), terminal = at(2), provider = at(3), sink = at(3, 0, -1);
  const bus = at(1, 0, 1), iface = at(1, 0, 2), remote = at(1, 0, 3), remotePower = at(0, 0, 3);
  await fixture.command(`tp @s ${x + 0.5} ${y} ${z + 0.5}`);
  for (const dx of [-2, 6]) for (const dz of [-3, 5]) {
    await until(async () => (await fixture.command(`execute if loaded ${at(dx, 0, dz)}`)).chat ?? [],
      lines => lines.includes('Test passed'), 'construction corner chunk loaded', 30000);
  }
  await fixture.command('clear @s'); await fixture.command(`fill ${at(-2, 0, -3)} ${at(6, 7, 5)} minecraft:air`);
  await fixture.command(`fill ${at(-2, -1, -3)} ${at(6, -1, 5)} minecraft:stone`);
  for (const [pos, id] of [[at(0), 'ae2:creative_energy_cell'], [drive, 'ae2:drive'], [terminal, 'ae2:cable_bus'],
    [provider, 'ae2:pattern_provider'], [sink, 'minecraft:chest'], [bus, 'ae2:cable_bus'],
    [iface, 'ae2:interface'], [remote, 'ae2:drive'], [remotePower, 'ae2:creative_energy_cell']] as const) {
    await fixture.command(`setblock ${pos} ${id}`);
  }
  for (let height = 1; height <= 4; height++) await fixture.command(`setblock ${at(2, height)} ae2:256k_crafting_storage`);
  await fixture.command(`item replace block ${drive} container.0 with ae2:item_storage_cell_1k`);
  await fixture.command(`item replace block ${remote} container.0 with ${cell}`);
  for (let i = 0; i < 26; i++) await fixture.command(`item replace block ${sink} container.${i} with minecraft:dirt 64`);
  await fixture.command(`item replace block ${sink} container.26 with ${exact} 63`);
  await fixture.replace('hotbar.0', 'ae2:terminal'); await fixture.select(0, 'ae2:terminal');
  assert.equal((await fixture.ae(`grid part ${terminal} north`)).attached, true);
  await fixture.replace('hotbar.0', 'ae2:storage_bus'); await fixture.select(0, 'ae2:storage_bus');
  assert.equal((await fixture.ae(`grid part ${bus} south`)).attached, true);
  const main = () => fixture.ae(`grid status ${drive}`); const other = () => fixture.ae(`grid status ${remote}`);
  await until(other, view => view.online && view.powered && total(view) === 2_147_483_647, 'remote creative cell online', 30000);
  await fixture.ae(`bus access ${bus} south write_only`);
  await until(main, view => view.online && view.powered && total(view) === 0, 'write-only bus hides remote creative stock', 30000);
  assert.equal(total(await other()), 2_147_483_647, 'remote grid remains independently powered');
  await fixture.ae(`bus access ${bus} south read_write`);
  await until(main, view => total(view) === 2_147_483_647, 'unconfigured Interface forwards creative stock', 30000);
  await until(() => fixture.ae(`grid status ${provider}`), view => view.online && view.powered, 'processing provider online', 30000);
  await fixture.replace('hotbar.0', pattern); await fixture.replace('hotbar.8', 'minecraft:blaze_rod'); await fixture.select(8, 'minecraft:blaze_rod');
  await fixture.command(`tp @s ${x + 5.5} ${y} ${z + 0.5}`); await fixture.raw(`interact ${provider} east`);
  const menu = await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot < 36 && slot.item === 'ae2:processing_pattern'), 'large processing pattern in inventory');
  await fixture.raw(`click-slot ${menu.slots.find((slot: any) => slot.slot < 36 && slot.item === 'ae2:processing_pattern').slot} shift`);
  await until(() => fixture.raw('inventory'), view => !view.counts?.['ae2:processing_pattern'], 'processing pattern installed'); await fixture.raw('close');
  await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`); await fixture.raw(`interact ${terminal} north`);
  const screen = await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:diamond'), 'creative-sourced output craftable');
  await fixture.raw(`click-slot ${screen.slots.find((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:diamond').slot} shift`);
  await until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 Craft Amount'), 'processing amount'); await fixture.raw('click-slot 302');
  const confirm = await until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 Craft Confirm') && /bytes|Missing materials|craft planning:/.test(JSON.stringify(view)), 'creative processing confirmation', 90000);
  assert.doesNotMatch(JSON.stringify(confirm), /Missing materials|craft planning:/); await fixture.raw('click-slot 302'); await fixture.raw('close');
  const providerData = async () => (await fixture.command(`data get block ${provider}`)).chat?.join('\n') ?? '';
  await until(providerData, text => text.includes('ae2_provider_send_queue') && /999998/.test(text), 'creative source supplies the full amount to provider custody', 120000);
  assert.match(await providerData(), /variant.*creative/); assert.equal(total(await other()), 2_147_483_647);
  fixture.mark('a physically separate creative-cell grid supplies a million-item request through the Interface');
  await fixture.restart(); await until(providerData, text => text.includes('ae2_provider_send_queue') && /999998/.test(text), 'creative-sourced queue survives restart', 30000);
  await until(other, view => view.online && view.powered && total(view) === 2_147_483_647, 'remote creative source survives restart', 30000);
  await fixture.command(`item replace block ${sink} container.25 with minecraft:air`);
  await until(providerData, text => /999934/.test(text), 'creative-sourced provider retries exactly 64 items', 90000);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('the component-bearing pending amount persists and resumes after restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
