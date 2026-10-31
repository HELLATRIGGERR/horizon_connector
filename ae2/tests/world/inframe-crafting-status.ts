/** In-frame CPU status, suspension and cancellation of a real physical job. */
import assert from 'node:assert/strict';
import { WorldFixture, until } from './harness';

const fixture = new WorldFixture('inframe-crafting-status', 'in-frame');
const count = (state: any, id: string) => state.items?.find((item: any) => item.item === id)?.count ?? 0;
const sameId = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);
const text = (view: any) => JSON.stringify(view.slots ?? []);
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 32, y = Math.ceil(py) + 5, z = Math.floor(pz) + 32;
  const at = (dx: number, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const drive = at(1), terminal = at(2), cpu = at(2, 1), provider = at(3), assembler = at(3, 0, -1);
  await fixture.command(`fill ${at(-2, 0, -3)} ${at(6, 5, 3)} minecraft:air`);
  await fixture.command(`fill ${at(-2, -1, -3)} ${at(6, -1, 3)} minecraft:stone`);
  for (const [pos, id] of [[at(0), 'creative_energy_cell'], [drive, 'drive'], [terminal, 'cable_bus'], [cpu, '1k_crafting_storage'], [provider, 'pattern_provider'], [assembler, 'molecular_assembler']] as const) await fixture.command(`setblock ${pos} ae2:${id}`);
  await fixture.command(`item replace block ${drive} container.0 with ae2:item_storage_cell_1k`);
  await fixture.command('clear @s'); await fixture.replace('hotbar.0', 'ae2:terminal'); await fixture.select(0, 'ae2:terminal');
  assert.equal((await fixture.ae(`grid part ${terminal} north`)).attached, true);
  const grid = () => until(() => fixture.ae(`grid status ${drive}`), state => state.online && state.powered, 'powered status grid', 30000);
  await grid(); await until(() => fixture.ae(`grid status ${assembler}`), state => state.online && state.powered, 'assembler online', 30000);
  await fixture.replace('hotbar.0', 'ae2:blank_pattern'); await fixture.select(0, 'ae2:blank_pattern');
  assert.equal((await fixture.ae('crafting encode minecraft:oak_log - - - - - - - -')).encoded, true);
  await fixture.command(`tp @s ${x + 3.5} ${y} ${z + 2.5}`); await fixture.raw(`interact ${provider} south`);
  const providerView = await until(() => fixture.raw('screen'), state => state.slots?.some((slot: any) => slot.slot < 36 && slot.item === 'ae2:crafting_pattern'), 'physical provider pattern');
  await fixture.raw(`click-slot ${providerView.slots.find((slot: any) => slot.slot < 36 && slot.item === 'ae2:crafting_pattern').slot} shift`);
  await until(() => fixture.raw('inventory'), state => !state.counts?.['ae2:crafting_pattern'], 'pattern installed'); await fixture.raw('close');
  await fixture.command('gamemode survival @s');
  for (let i = 0; i < 3; i++) {
    await fixture.replace('hotbar.0', 'minecraft:oak_log', 64); await fixture.select(0, 'minecraft:oak_log');
    assert.equal((await fixture.ae(`grid deposit ${drive}`)).inserted, 64);
  }
  await fixture.replace('hotbar.8', 'minecraft:stick'); await fixture.select(8, 'minecraft:stick');
  await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`);
  const openTerminal = async () => {
    await fixture.raw(`interact ${terminal} north`);
    return until(() => fixture.raw('screen'), state => JSON.stringify(state.title ?? '').includes('AE2 ME Terminal') && state.slots?.some((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:oak_planks'), 'craftable terminal entry');
  };
  const terminalView = await openTerminal();
  const output = terminalView.slots.find((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:oak_planks');
  await fixture.raw(`click-slot ${output.slot} shift`);
  await until(() => fixture.raw('screen'), state => JSON.stringify(state.title ?? '').includes('AE2 Craft Amount'), 'craft amount');
  await fixture.raw('ui-key right'); await fixture.raw('ui-key backspace'); await fixture.raw('type 672'); await fixture.raw('click-slot 302');
  await until(() => fixture.raw('screen'), state => JSON.stringify(state.title ?? '').includes('AE2 Craft Confirm') && /bytes/.test(text(state)), 'finished plan');
  await fixture.raw('click-slot 302');
  const started = await until(() => fixture.raw('ae2-job-ui'), state => state.events.some((event: any) => event.status === 'started' && event.requested === 672), 'local STARTED event');
  const job = started.events.find((event: any) => event.status === 'started' && event.requested === 672).job_id;
  await openTerminal(); await fixture.raw('click-slot 349');
  await until(() => fixture.raw('screen'), state => JSON.stringify(state.title ?? '').includes('AE2 Crafting Status') && /Running/.test(text(state)), 'physical CPU status');
  await fixture.raw('click-slot 302');
  await until(() => fixture.raw('screen'), state => /Suspended/.test(text(state)), 'CPU suspended');
  await fixture.raw('click-slot 302');
  await until(() => fixture.raw('screen'), state => /Running/.test(text(state)), 'CPU resumed');
  await fixture.raw('click-slot 303');
  const cancelled = await until(() => fixture.raw('ae2-job-ui'), state => state.events.some((event: any) => event.status === 'cancelled' && sameId(event.job_id, job)), 'local CANCELLED event');
  assert(!cancelled.pending.some((id: any) => sameId(id, job)));
  await fixture.raw('close');
  await until(grid, state => {
    const logs = count(state, 'minecraft:oak_log'), planks = count(state, 'minecraft:oak_planks');
    return planks % 4 === 0 && logs + planks / 4 === 192;
  }, 'cancelled job returns all unused logs or crafted planks', 120000);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('in-frame status suspends, resumes and cancels the same owner job while conserving ingredients');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
