/** Break a real CPU with no storage: AE2's per-key physical drop cap and max stack apply. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';
const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('crafting-break-large-escrow', mode);
const stonePatch = '{"minecraft:custom_data":{variant:"break-large"},"minecraft:max_stack_size":16}';
const dirtPatch = '{"minecraft:custom_data":{variant:"break-small"},"minecraft:max_stack_size":8}';
const cell = `ae2:item_storage_cell_256k[ae2:storage_cell_inv=[{"#t":"ae2:i",id:"minecraft:stone",components:${stonePatch},"#":999999L},{"#t":"ae2:i",id:"minecraft:dirt",components:${dirtPatch},"#":65L}]]`;
const pattern = `ae2:processing_pattern[ae2:encoded_processing_pattern={sparseInputs:[{"#t":"ae2:i",id:"minecraft:stone",components:${stonePatch},"#":999999L},{"#t":"ae2:i",id:"minecraft:dirt",components:${dirtPatch},"#":65L}],sparseOutputs:[{"#t":"ae2:i",id:"minecraft:diamond","#":1L}]}]`;
const total = (view: any, id: string) => (view.items ?? []).filter((entry: any) => entry.item === id).reduce((sum: number, entry: any) => sum + entry.count, 0);
try {
  const state = await fixture.start(), [px, py, pz] = state.player.position;
  const x = Math.floor(px) + 32, y = Math.ceil(py) + 5, z = Math.floor(pz) + 32;
  const at = (dx: number, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const drive = at(1), terminal = at(2), provider = at(3);
  await fixture.command(`tp @s ${x + 0.5} ${y} ${z + 0.5}`);
  for (const dx of [-2, 6]) for (const dz of [-3, 3]) {
    await until(async () => (await fixture.command(`execute if loaded ${at(dx, 0, dz)}`)).chat ?? [],
      lines => lines.includes('Test passed'), 'construction corner chunk loaded', 30000);
  }
  await fixture.command('clear @s'); await fixture.command(`fill ${at(-2, 0, -3)} ${at(6, 7, 3)} minecraft:air`);
  await fixture.command(`fill ${at(-2, -1, -3)} ${at(6, -1, 3)} minecraft:stone`);
  for (const [pos, id] of [[at(0), 'creative_energy_cell'], [drive, 'drive'], [terminal, 'cable_bus'], [provider, 'pattern_provider']] as const) {
    await fixture.command(`setblock ${pos} ae2:${id}`);
  }
  for (let height = 1; height <= 4; height++) await fixture.command(`setblock ${at(2, height)} ae2:256k_crafting_storage`);
  await fixture.command(`item replace block ${drive} container.0 with ${cell}`);
  await fixture.replace('hotbar.0', 'ae2:terminal'); await fixture.select(0, 'ae2:terminal');
  assert.equal((await fixture.ae(`grid part ${terminal} north`)).attached, true);
  const ready = () => until(() => fixture.ae(`grid status ${drive}`), view => view.online && view.powered, 'break regression network powered', 30000);
  await until(ready, view => total(view, 'minecraft:stone') === 999999 && total(view, 'minecraft:dirt') === 65, 'both exact physical keys mounted');
  await until(() => fixture.ae(`grid status ${provider}`), view => view.online && view.powered, 'provider online', 30000);
  await fixture.replace('hotbar.0', pattern); await fixture.replace('hotbar.8', 'minecraft:blaze_rod'); await fixture.select(8, 'minecraft:blaze_rod');
  await fixture.command(`tp @s ${x + 5.5} ${y} ${z + 0.5}`); await fixture.raw(`interact ${provider} east`);
  const menu = await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot < 36 && slot.item === 'ae2:processing_pattern'), 'break pattern in inventory');
  await fixture.raw(`click-slot ${menu.slots.find((slot: any) => slot.slot < 36 && slot.item === 'ae2:processing_pattern').slot} shift`);
  await until(() => fixture.raw('inventory'), view => !view.counts?.['ae2:processing_pattern'], 'physical pattern installed'); await fixture.raw('close');
  await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`); await fixture.raw(`interact ${terminal} north`);
  const screen = await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:diamond'), 'large processing output craftable');
  await fixture.raw(`click-slot ${screen.slots.find((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:diamond').slot} shift`);
  await until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 Craft Amount'), 'break regression amount'); await fixture.raw('click-slot 302');
  const confirmation = await until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 Craft Confirm') && /bytes|Missing materials|craft planning:/.test(JSON.stringify(view)), 'break regression confirmation', 90000);
  assert.doesNotMatch(JSON.stringify(confirmation), /Missing materials|craft planning:/); await fixture.raw('click-slot 302'); await fixture.raw('close');
  await until(ready, view => total(view, 'minecraft:stone') === 0 && total(view, 'minecraft:dirt') === 0, 'both keys reserved in the real CPU', 120000);
  const cpuData = async () => {
    for (let height = 1; height <= 4; height++) {
      const text = (await fixture.command(`data get block ${at(2, height)}`)).chat?.join('\n') ?? '';
      if (text.includes('ae2_recursive_job')) return text;
    }
    return '';
  };
  const owned = await cpuData(); assert.match(owned, /ae2_cpu_escrow/); assert.match(owned, /999999/); assert.match(owned, /break-small/);
  // Removing the now-empty drive leaves no mounted storage; keep the player
  // far enough away that the real item entities cannot be picked up.
  await fixture.command(`tp @s ${x + 12.5} ${y} ${z + 4.5}`);
  await fixture.command(`setblock ${drive} minecraft:air`);
  await until(() => fixture.command(`execute if block ${drive} minecraft:air`), reply => reply.chat?.includes('Test passed'), 'storage drive removed', 30000);
  await fixture.command(`setblock ${at(2, 1)} minecraft:air`);
  await until(() => fixture.command(`execute if block ${at(2, 1)} minecraft:air`), reply => reply.chat?.includes('Test passed'), 'formed CPU block removed', 30000);
  const drops = async () => (await fixture.raw('entities 32 minecraft:item')).entities.filter((entity: any) =>
    entity.item?.components?.includes('break-large') || entity.item?.components?.includes('break-small'));
  const check = (entities: any[]) => {
    const stone = entities.filter(entity => entity.item.item === 'minecraft:stone' && entity.item.components.includes('break-large'));
    const dirt = entities.filter(entity => entity.item.item === 'minecraft:dirt' && entity.item.components.includes('break-small'));
    assert.equal(stone.length, 1001, 'source shared-list cap permits 1,001 stone stacks');
    assert(stone.every(entity => entity.item.count === 16), 'no source stone stack exceeds its component maximum');
    assert.equal(dirt.reduce((sum, entity) => sum + entity.item.count, 0), 65, 'the second key gets a fresh source drop list');
    assert(dirt.every(entity => entity.item.count > 0 && entity.item.count <= 8), 'each dirt stack respects its component maximum');
  };
  const entities = await until(drops, list => list.length >= 1010, 'all source-capped physical entities arrive', 120000); check(entities);
  fixture.mark('CPU break drops 1,001 component-limited stone stacks and independently preserves the second exact key');
  await fixture.restart();
  const restored = await until(drops, list => list.length >= 1010, 'source-capped entities survive restart', 120000); check(restored);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('the bounded physical entities retain their exact keys and counts after restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
