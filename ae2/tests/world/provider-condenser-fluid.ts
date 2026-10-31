/** Direct Pattern Provider -> Condenser uses its AE2 ME-storage capability. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('provider-condenser-fluid', mode);
const cell = 'ae2:fluid_storage_cell_1k[ae2:storage_cell_inv=[{"#t":"ae2:f",id:"minecraft:water","#":1377L}]]';
const pattern = 'ae2:processing_pattern[ae2:encoded_processing_pattern={sparseInputs:[{"#t":"ae2:f",id:"minecraft:water","#":1377L}],sparseOutputs:[{"#t":"ae2:i",id:"minecraft:diamond","#":1L}]}]';
try {
  const state = await fixture.start(); const [px, py, pz] = state.player.position;
  const x = Math.floor(px) + 32, y = Math.ceil(py) + 5, z = Math.floor(pz) + 32;
  const at = (dx: number, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const drive = at(1), terminal = at(2), cpu = at(2, 1), provider = at(3), condenser = at(3, 0, -1);
  await fixture.command('clear @s'); await fixture.command(`fill ${at(-2, 0, -3)} ${at(6, 5, 3)} minecraft:air`);
  await fixture.command(`fill ${at(-2, -1, -3)} ${at(6, -1, 3)} minecraft:stone`);
  for (const [pos, id] of [[at(0), 'creative_energy_cell'], [drive, 'drive'], [terminal, 'cable_bus'],
    [cpu, '64k_crafting_storage'], [provider, 'pattern_provider'], [condenser, 'condenser']] as const) {
    await fixture.command(`setblock ${pos} ae2:${id}`);
  }
  await fixture.command(`item replace block ${drive} container.0 with ${cell}`);
  await fixture.command(`item replace block ${drive} container.1 with ae2:item_storage_cell_1k`);
  await fixture.command(`item replace block ${condenser} container.1 with ae2:cell_component_1k`);
  assert.equal((await fixture.ae(`condenser output ${condenser} matter_balls`)).output, 'matter_balls');
  await fixture.command(`item replace block ${condenser} container.0 with ae2:matter_ball 64`);
  await fixture.replace('hotbar.0', 'ae2:terminal'); await fixture.select(0, 'ae2:terminal');
  assert.equal((await fixture.ae(`grid part ${terminal} north`)).attached, true);
  const ready = () => until(() => fixture.ae(`grid status ${drive}`), view => view.online && view.powered, 'powered processing network', 30000);
  const fluids = () => fixture.ae(`fluid grid status ${drive}`);
  const matter = () => fixture.ae(`condenser status ${condenser}`);
  await ready(); await until(() => fixture.ae(`grid status ${provider}`), view => view.online && view.powered, 'online pattern provider', 30000);
  await until(fluids, view => view.entries?.some((entry: any) => entry.fluid === 'minecraft:water' && entry.amount_mb === 1377), 'physical water cell is mounted');
  await fixture.replace('hotbar.0', pattern); await fixture.replace('hotbar.8', 'minecraft:blaze_rod'); await fixture.select(8, 'minecraft:blaze_rod');
  await fixture.command(`tp @s ${x + 5.5} ${y} ${z + 0.5}`); await fixture.raw(`interact ${provider} east`);
  const menu = await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot < 36 && slot.item === 'ae2:processing_pattern'), 'physical processing pattern in player inventory');
  await fixture.raw(`click-slot ${menu.slots.find((slot: any) => slot.slot < 36 && slot.item === 'ae2:processing_pattern').slot} shift`);
  await until(() => fixture.raw('inventory'), view => !view.counts?.['ae2:processing_pattern'], 'physical processing pattern installed'); await fixture.raw('close');
  await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`); await fixture.raw(`interact ${terminal} north`);
  const screen = await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:diamond'), 'fluid processing output is craftable');
  await fixture.raw(`click-slot ${screen.slots.find((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:diamond').slot} shift`);
  await until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 Craft Amount'), 'processing amount'); await fixture.raw('click-slot 302');
  const confirmation = await until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 Craft Confirm') && /bytes|Missing materials|craft planning:/.test(JSON.stringify(view)), 'processing confirmation');
  assert.doesNotMatch(JSON.stringify(confirmation), /Missing materials|craft planning:/);
  await fixture.raw('click-slot 302'); await fixture.raw('close');
  await until(fluids, view => !view.entries?.some((entry: any) => entry.fluid === 'minecraft:water' && entry.amount_mb), 'water reserved into physical CPU escrow', 90000);
  assert.equal((await matter()).stored, 0, 'a full Condenser output blocks its ME storage insertion');
  assert.match((await fixture.command(`data get block ${cpu}`)).chat?.join('\n') ?? '', /ae2_cpu_fluid_escrow/, 'unaccepted water remains owned by the CPU');
  await fixture.restart(); assert.equal((await matter()).stored, 0);
  assert.match((await fixture.command(`data get block ${cpu}`)).chat?.join('\n') ?? '', /ae2_cpu_fluid_escrow/, 'blocked fluid survives restart');
  await fixture.command(`item replace block ${condenser} container.0 with ae2:matter_ball 63`);
  await until(matter, view => view.stored === 11, 'the full 1377 mB is accepted and integer-divided by 125', 90000);
  assert(!((await fluids()).entries ?? []).some((entry: any) => entry.fluid === 'minecraft:water' && entry.amount_mb));
  await fixture.restart(); assert.equal((await matter()).stored, 11);
  assert.match((await fixture.command(`data get block ${provider}`)).chat?.join('\n') ?? '', /ae2:encoded_processing_pattern/, 'physical processing pattern survives restart');
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('blocked fluid stays in CPU escrow; the Condenser accepts the full amount and retains integer-scaled power across restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
