/** Large processing custody: exact components, storage priority and retry after restart. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('processing-large-provider-queue', mode);
const exact = 'minecraft:stone[minecraft:custom_data={variant:"exact"}]';
const cell = 'ae2:item_storage_cell_256k[ae2:storage_cell_inv=[{"#t":"ae2:i",id:"minecraft:stone",components:{"minecraft:custom_data":{variant:"exact"}},"#":999999L},{"#t":"ae2:i",id:"minecraft:stone",components:{"minecraft:custom_data":{variant:"other"}},"#":1L}]]';
const pattern = 'ae2:processing_pattern[ae2:encoded_processing_pattern={sparseInputs:[{"#t":"ae2:i",id:"minecraft:stone",components:{"minecraft:custom_data":{variant:"exact"}},"#":999999L}],sparseOutputs:[{"#t":"ae2:i",id:"minecraft:diamond","#":1L}]}]';
const total = (view: any, id: string) => (view.items ?? []).filter((entry: any) => entry.item === id).reduce((sum: number, entry: any) => sum + entry.count, 0);
try {
  const state = await fixture.start(); const [px, py, pz] = state.player.position;
  const x = Math.floor(px) + 32, y = Math.ceil(py) + 5, z = Math.floor(pz) + 32;
  const at = (dx: number, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const drive = at(1), terminal = at(2), provider = at(3), sink = at(3, 0, -1);
  const bus = at(1, 0, 1), source = at(1, 0, 2);
  await fixture.command(`tp @s ${x + 0.5} ${y} ${z + 0.5}`);
  for (const dx of [-2, 6]) for (const dz of [-3, 4]) {
    await until(async () => (await fixture.command(`execute if loaded ${at(dx, 0, dz)}`)).chat ?? [],
      lines => lines.includes('Test passed'), 'construction corner chunk loaded', 30000);
  }
  await fixture.command('clear @s'); await fixture.command(`fill ${at(-2, 0, -3)} ${at(6, 7, 4)} minecraft:air`);
  await fixture.command(`fill ${at(-2, -1, -3)} ${at(6, -1, 4)} minecraft:stone`);
  for (const [pos, id] of [[at(0), 'ae2:creative_energy_cell'], [drive, 'ae2:drive'], [terminal, 'ae2:cable_bus'],
    [provider, 'ae2:pattern_provider'], [sink, 'minecraft:chest'], [bus, 'ae2:cable_bus'], [source, 'minecraft:chest']] as const) {
    await fixture.command(`setblock ${pos} ${id}`);
  }
  for (let height = 1; height <= 4; height++) await fixture.command(`setblock ${at(2, height)} ae2:256k_crafting_storage`);
  await fixture.command(`item replace block ${drive} container.0 with ${cell}`);
  await fixture.command(`item replace block ${source} container.0 with ${exact} 64`);
  await fixture.command(`item replace block ${source} container.1 with ${exact} 64`);
  for (let i = 2; i < 27; i++) await fixture.command(`item replace block ${source} container.${i} with minecraft:dirt 64`);
  for (let i = 0; i < 27; i++) await fixture.command(`item replace block ${sink} container.${i} with minecraft:dirt 64`);
  await fixture.replace('hotbar.0', 'ae2:terminal'); await fixture.select(0, 'ae2:terminal');
  assert.equal((await fixture.ae(`grid part ${terminal} north`)).attached, true);
  await fixture.replace('hotbar.0', 'ae2:storage_bus'); await fixture.select(0, 'ae2:storage_bus');
  assert.equal((await fixture.ae(`grid part ${bus} south`)).attached, true);
  assert.equal((await fixture.ae(`bus priority ${bus} south -5`)).configured, true);
  assert.equal((await fixture.ae(`bus status ${bus} south`)).priority, -5);
  const ready = () => until(() => fixture.ae(`grid status ${drive}`), view => view.online && view.powered, 'large processing network powered', 30000);
  await until(ready, view => total(view, 'minecraft:stone') === 1_000_128, 'both exact source mounts and distinct variant are visible', 30000);
  await until(() => fixture.ae(`grid status ${provider}`), view => view.online && view.powered, 'pattern provider online', 30000);
  await fixture.replace('hotbar.0', pattern); await fixture.replace('hotbar.8', 'minecraft:blaze_rod'); await fixture.select(8, 'minecraft:blaze_rod');
  await fixture.command(`tp @s ${x + 5.5} ${y} ${z + 0.5}`); await fixture.raw(`interact ${provider} east`);
  const menu = await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot < 36 && slot.item === 'ae2:processing_pattern'), 'large source processing pattern in inventory');
  await fixture.raw(`click-slot ${menu.slots.find((slot: any) => slot.slot < 36 && slot.item === 'ae2:processing_pattern').slot} shift`);
  await until(() => fixture.raw('inventory'), view => !view.counts?.['ae2:processing_pattern'], 'large physical pattern installed'); await fixture.raw('close');
  await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`);
  const openTerminal = async () => {
    await fixture.raw(`interact ${terminal} north`);
    return until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:diamond'), 'large processing output craftable');
  };
  const submit = async () => {
    const screen = await openTerminal();
    await fixture.raw(`click-slot ${screen.slots.find((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:diamond').slot} shift`);
    await until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 Craft Amount'), 'large processing amount'); await fixture.raw('click-slot 302');
    const confirm = await until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 Craft Confirm') && /bytes|Missing materials|craft planning:/.test(JSON.stringify(view)), 'large processing confirmation', 90000);
    assert.doesNotMatch(JSON.stringify(confirm), /Missing materials|craft planning:/); await fixture.raw('click-slot 302'); await fixture.raw('close');
  };
  await submit();
  await until(ready, view => total(view, 'minecraft:stone') === 129, 'full destination leaves the large reservation in CPU custody', 120000);
  const cpuData = async () => {
    for (let height = 1; height <= 4; height++) {
      const text = (await fixture.command(`data get block ${at(2, height)}`)).chat?.join('\n') ?? '';
      if (text.includes('ae2_recursive_job')) return text;
    }
    return '';
  };
  assert.match(await cpuData(), /ae2_cpu_escrow/); assert.match(await cpuData(), /999999/);
  await fixture.restart(); assert.match(await cpuData(), /999999/, 'blocked CPU escrow survives restart');
  // Insertions choose higher priority first, so cancellation restores the
  // two source chest slots before returning the rest to the real cell.
  await fixture.ae(`bus priority ${bus} south 5`);
  await openTerminal(); await fixture.raw('click-slot 349');
  await until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 Crafting Status'), 'large CPU status');
  await fixture.raw('click-slot 303'); await fixture.raw('close');
  await until(ready, view => total(view, 'minecraft:stone') === 1_000_128, 'cancellation returns every reserved ingredient', 120000);
  const returned = await fixture.raw(`container ${source}`);
  assert.equal(returned.slots?.filter((slot: any) => slot?.item === 'minecraft:stone').reduce((sum: number, slot: any) => sum + slot.count, 0), 128);
  await fixture.ae(`bus priority ${bus} south -5`);
  await fixture.command(`item replace block ${sink} container.26 with ${exact} 63`);
  await submit();
  const providerData = async () => (await fixture.command(`data get block ${provider}`)).chat?.join('\n') ?? '';
  await until(providerData, text => text.includes('ae2_provider_send_queue') && /999998/.test(text), '999,998 unsent exact items held in the provider', 120000);
  const sourceSlots = await fixture.raw(`container ${source}`);
  assert(!sourceSlots.slots?.some((slot: any) => slot?.item === 'minecraft:stone'), 'lower-priority source bus is drained before the cell');
  const remaining = await until(ready, view => total(view, 'minecraft:stone') === 129, '128 exact and one other variant remain in the cell', 30000);
  assert.equal(total(remaining, 'minecraft:stone'), 129);
  assert.match(await providerData(), /variant.*exact/, 'queued stack retains exact source components');
  const target = await fixture.raw(`container ${sink}`); assert(target.slots?.some((slot: any) => slot?.item === 'minecraft:stone' && slot.count === 64));
  fixture.mark('bulk reservation drains the lower-priority physical chest first and queues the exact component-bearing remainder');
  await fixture.restart();
  const restored = await until(providerData, text => text.includes('ae2_provider_send_queue') && /999998/.test(text), 'large provider queue survives restart', 30000);
  assert.match(restored, /variant.*exact/); assert.equal(total(await ready(), 'minecraft:stone'), 129);
  await fixture.command(`item replace block ${sink} container.25 with minecraft:air`);
  await until(providerData, text => /999934/.test(text), 'provider retries 64 items after target space opens', 90000);
  assert.equal(total(await ready(), 'minecraft:stone'), 129);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('ordered physical queue survives restart and retries only the 64 items actually accepted');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
