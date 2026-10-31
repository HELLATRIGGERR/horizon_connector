/** Two physical patterns with different inputs combine for one autocrafting request. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('crafting-alternatives', mode);
const chat = (reply: any) => (reply.chat ?? []).join('\n');
const count = (status: any, item: string) => status.items?.find((entry: any) => entry.item === item)?.count ?? 0;

try {
  const state = await fixture.start();
  const [px, py, pz] = state.player.position;
  const x = Math.floor(px) + 32, y = Math.ceil(py) + 5, z = Math.floor(pz) + 32;
  const at = (dx: number, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const drive = at(1), terminal = at(2), cpu = at(2, 1);
  const firstProvider = at(3), secondProvider = at(3, 0, 1);
  const firstAssembler = at(3, 0, -1), secondAssembler = at(4, 0, 1);

  await fixture.command('clear @s');
  await fixture.command(`fill ${at(-2, 0, -3)} ${at(6, 5, 4)} minecraft:air`);
  await fixture.command(`fill ${at(-2, -1, -3)} ${at(6, -1, 4)} minecraft:stone`);
  for (const [pos, id] of [
    [at(0), 'creative_energy_cell'], [drive, 'drive'], [terminal, 'cable_bus'],
    [cpu, '64k_crafting_storage'], [firstProvider, 'pattern_provider'],
    [secondProvider, 'pattern_provider'], [firstAssembler, 'molecular_assembler'],
    [secondAssembler, 'molecular_assembler'],
  ] as const) await fixture.command(`setblock ${pos} ae2:${id}`);
  await fixture.command(`item replace block ${drive} container.0 with ae2:item_storage_cell_1k`);
  await fixture.replace('hotbar.0', 'ae2:terminal');
  await fixture.select(0, 'ae2:terminal');
  assert.equal((await fixture.ae(`grid part ${terminal} north`)).attached, true);
  const ready = () => until(() => fixture.ae(`grid status ${drive}`), s => s.online && s.powered, 'powered drive', 30000);
  await ready();
  for (const pos of [firstProvider, secondProvider, firstAssembler, secondAssembler]) {
    await until(() => fixture.ae(`grid status ${pos}`), s => s.online && s.powered, `online device ${pos}`, 30000);
  }

  // Both source patterns are exact: the encode command disables substitutions.
  // The vanilla oak-planks recipe accepts either physical input and produces four.
  for (const [provider, input, playerX, playerZ, face] of [
    [firstProvider, 'minecraft:oak_log', x + 5.5, z + 0.5, 'east'],
    [secondProvider, 'minecraft:stripped_oak_log', x + 3.5, z + 2.5, 'south'],
  ] as const) {
    await fixture.replace('hotbar.0', 'ae2:blank_pattern');
    await fixture.select(0, 'ae2:blank_pattern');
    const encoded = await fixture.ae(`crafting encode ${input} - - - - - - - -`);
    assert.deepEqual([encoded.encoded, encoded.output, encoded.count], [true, 'minecraft:oak_planks', 4]);
    await until(() => fixture.raw('inventory'), view => view.hotbar[0]?.item === 'ae2:crafting_pattern', `encoded ${input} pattern`);
    await fixture.command(`tp @s ${playerX} ${y} ${playerZ}`);
    await fixture.raw(`interact ${provider} ${face}`);
    const menu = await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot < 36 && slot.item === 'ae2:crafting_pattern'), 'physical provider menu', 5000);
    await fixture.raw(`click-slot ${menu.slots.find((slot: any) => slot.slot < 36 && slot.item === 'ae2:crafting_pattern').slot} shift`);
    await until(() => fixture.raw('inventory'), view => !view.counts?.['ae2:crafting_pattern'], `pattern installed in ${provider}`);
    await fixture.raw('close');
    assert.match(chat(await fixture.command(`data get block ${provider}`)), /ae2:crafting_pattern/);
  }
  async function deposit(item: string, amount: number) {
    await fixture.replace('hotbar.0', item, amount);
    await fixture.select(0, item);
    try {
      assert.equal((await fixture.ae(`grid deposit ${drive}`)).inserted, amount);
    } catch (error) {
      // Record the very next observed state; do not retry the failed deposit
      // or wait for the network to become ready before gathering evidence.
      const diagnostic: Record<string, unknown> = {};
      for (const [label, command] of [
        ['drive', `ae2 grid status ${drive}`],
        ['first_provider', `ae2 grid status ${firstProvider}`],
        ['second_provider', `ae2 grid status ${secondProvider}`],
        ['provider_observer', `ae2 provider-shadow ${firstProvider} minecraft:oak_planks`],
        ['power', `ae2 power ${drive} north`],
      ] as const) {
        try { diagnostic[label] = (await fixture.command(command)).chat; }
        catch (failure) { diagnostic[label] = String(failure); }
      }
      console.error('DEPOSIT_DIAGNOSTIC', JSON.stringify(diagnostic));
      throw error;
    }
  }
  await deposit('minecraft:oak_log', 1);
  await deposit('minecraft:stripped_oak_log', 1);
  await deposit('minecraft:oak_planks', 4);
  const before = await ready();
  assert.equal(count(before, 'minecraft:oak_log'), 1);
  assert.equal(count(before, 'minecraft:stripped_oak_log'), 1);
  assert.equal(count(before, 'minecraft:oak_planks'), 4);
  fixture.mark('two real exact patterns, two different logs and four existing planks are present');

  const donor = at(6, 0, 3);
  if (mode === 'in-frame') {
    await fixture.command(`setblock ${donor} ae2:cable_bus`);
    await fixture.replace('hotbar.0', 'ae2:terminal'); await fixture.select(0, 'ae2:terminal');
    assert.equal((await fixture.ae(`grid part ${donor} north`)).attached, true);
  }
  await fixture.replace('hotbar.8', 'minecraft:blaze_rod');
  await fixture.select(8, 'minecraft:blaze_rod');
  await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`);
  await fixture.raw(`interact ${terminal} north`);
  let terminalView = await until(() => fixture.raw('screen'), view => view.slots?.some((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:oak_planks'), 'stored and craftable planks');
  let plankSlot = terminalView.slots.find((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:oak_planks').slot;
  if (mode === 'in-frame') {
    // A leading '=' asks for the shortfall. Four are already stored, so the
    // overlay must return to the same terminal without submitting a job.
    await fixture.raw(`click-slot ${plankSlot} shift`);
    await until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 Craft Amount'), 'shortfall amount');
    await fixture.raw('ui-key right'); await fixture.raw('ui-key backspace'); await fixture.raw('type =4');
    await fixture.raw('click-slot 302');
    terminalView = await until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 ME Terminal') && view.slots?.some((slot: any) => slot.item === 'minecraft:oak_planks'), 'zero shortfall returns to original terminal');
    const unchanged = await ready();
    assert.equal(count(unchanged, 'minecraft:oak_log'), 1);
    assert.equal(count(unchanged, 'minecraft:stripped_oak_log'), 1);
    assert.equal(count(unchanged, 'minecraft:oak_planks'), 4);
    assert(!(await fixture.raw('ae2-job-ui')).events.some((event: any) => event.status === 'started'), 'zero shortfall emits no job');
    plankSlot = terminalView.slots.find((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:oak_planks').slot;
    fixture.mark('a zero shortfall returns to the same terminal without reserving materials');

    // A planned request belongs to its original face. Replacing that face
    // while the confirmation is open must neither submit nor move inputs.
    await fixture.raw(`click-slot ${plankSlot} shift`);
    await until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 Craft Amount'), 'stale-owner amount');
    await fixture.raw('ui-key right'); await fixture.raw('ui-key backspace'); await fixture.raw('type 8'); await fixture.raw('click-slot 302');
    await until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 Craft Confirm') && /bytes/.test(JSON.stringify(view)), 'stale-owner planned confirmation');
    await fixture.command(`data modify block ${terminal} ae2_lab_config.parts[{face:"north"}] set from block ${donor} ae2_lab_config.parts[{face:"north"}]`);
    await until(() => fixture.raw('screen'), view => view.screen === 'playing', 'replacement closes planned confirmation');
    const afterReplacement = await ready();
    assert.equal(count(afterReplacement, 'minecraft:oak_log'), 1);
    assert.equal(count(afterReplacement, 'minecraft:stripped_oak_log'), 1);
    assert.equal(count(afterReplacement, 'minecraft:oak_planks'), 4);
    assert(!(await fixture.raw('ae2-job-ui')).events.some((event: any) => event.status === 'started'), 'replacement emitted no job');
    await fixture.select(8, 'minecraft:blaze_rod'); await fixture.raw(`interact ${terminal} north`);
    terminalView = await until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 ME Terminal') && view.slots?.some((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:oak_planks'), 'replacement terminal can be opened');
    plankSlot = terminalView.slots.find((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === 'minecraft:oak_planks').slot;
    fixture.mark('replacing the original terminal face invalidates its unsubmitted plan without moving materials');
  }
  await fixture.raw(`click-slot ${plankSlot} shift`);
  await until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 Craft Amount'), 'craft amount');
  await fixture.raw('ui-key right');
  await fixture.raw('ui-key backspace');
  await fixture.raw('type 8');
  await fixture.raw('wait 5');
  await fixture.raw('click-slot 302');
  const confirm = await until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 Craft Confirm') && /bytes|Missing materials|craft planning:/.test(JSON.stringify(view)), 'craft confirmation');
  assert.doesNotMatch(JSON.stringify(confirm), /Missing materials/);
  assert.match(JSON.stringify(confirm), /Planned 8 x minecraft:oak_planks/);
  const preview = await ready();
  assert.equal(count(preview, 'minecraft:oak_log'), 1);
  assert.equal(count(preview, 'minecraft:stripped_oak_log'), 1);
  assert.equal(count(preview, 'minecraft:oak_planks'), 4);
  await fixture.raw('click-slot 302');
  let localJob: unknown;
  if (mode === 'in-frame') {
    const trace = await until(() => fixture.raw('ae2-job-ui'), state => state.events.some((event: any) => event.status === 'started' && event.requested === 8), 'local owner STARTED event');
    localJob = trace.events.find((event: any) => event.status === 'started' && event.requested === 8).job_id;
    assert(trace.pins.some((pin: any) => pin.item === 'minecraft:oak_planks'));
  }
  await fixture.raw('close');
  const finished = await until(() => fixture.ae(`grid status ${drive}`), s => count(s, 'minecraft:oak_planks') === 12 && count(s, 'minecraft:oak_log') === 0 && count(s, 'minecraft:stripped_oak_log') === 0, 'two-provider craft completes with exact physical stock', 90000);
  assert.equal(count(finished, 'minecraft:oak_planks'), 12);
  assert.equal(count(finished, 'minecraft:oak_log'), 0);
  assert.equal(count(finished, 'minecraft:stripped_oak_log'), 0);
  if (mode === 'in-frame') {
    const trace = await until(() => fixture.raw('ae2-job-ui'), state => state.events.some((event: any) => event.status === 'finished' && JSON.stringify(event.job_id) === JSON.stringify(localJob)), 'local owner FINISHED event');
    assert(!trace.pending.some((id: any) => JSON.stringify(id) === JSON.stringify(localJob)));
    fixture.mark('the in-frame owner receives STARTED and FINISHED for the same physical job');
  }
  const inventory = await fixture.raw('inventory');
  for (const item of ['minecraft:oak_log', 'minecraft:stripped_oak_log', 'minecraft:oak_planks']) assert.equal(inventory.counts?.[item] ?? 0, 0);
  fixture.mark('one log from each pattern produces eight new planks while four existing planks remain in storage');

  await fixture.restart();
  const saved = await ready();
  assert.equal(count(saved, 'minecraft:oak_planks'), 12);
  assert.equal(count(saved, 'minecraft:oak_log'), 0);
  assert.equal(count(saved, 'minecraft:stripped_oak_log'), 0);
  for (const provider of [firstProvider, secondProvider]) assert.match(chat(await fixture.command(`data get block ${provider}`)), /ae2:crafting_pattern/);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('both provider patterns and the conserved physical result survive restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
