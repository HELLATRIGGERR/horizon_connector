/** Physical round-robin export preflight, requester custody and restart. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('export-bus-crafting-schedule', mode);
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 6, y = Math.ceil(py) + 3, z = Math.floor(pz) + 6;
  const at = (dx: number, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const power = at(0), drive = at(1), host = at(2), cpu = at(2, 1);
  const bus = at(3), target = at(4), assembler = at(2, 0, -1);
  const busStatus = () => fixture.ae(`bus status ${bus} east`);
  const chestCount = async (item: string) => ((await fixture.raw(`container ${target}`)).slots ?? [])
    .filter((slot: any) => slot?.item === item).reduce((n: number, slot: any) => n + slot.count, 0);
  const gridCount = async (item: string) => ((await fixture.ae(`grid status ${drive}`)).items ?? [])
    .find((entry: any) => entry.item === item)?.count ?? 0;
  // Hold the whole server: AE2 grid services intentionally continue during
  // vanilla /tick freeze. Step the ordinary server path synchronously.
  async function stepUntilCursor(expected: number): Promise<void> {
    for (let i = 0; i < 100; i++) {
      await fixture.stepServer(1);
      if ((await busStatus()).state.next_slot === expected) return;
    }
    throw new Error(`round-robin cursor did not advance to ${expected}`);
  }
  async function linkCount(): Promise<number> {
    const reply = await fixture.command(`data get block ${bus}`);
    return ((reply.chat ?? []).join('\n').match(/cpu:/g) ?? []).length;
  }

  await fixture.command(`fill ${at(-3, -1, -4)} ${at(6, -1, 4)} minecraft:stone`);
  await fixture.command(`fill ${at(-3, 0, -4)} ${at(6, 4, 4)} minecraft:air`);
  await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`);
  await until(() => fixture.raw('status'), s => Math.abs(s.player.position[0] - (x + 2.5)) < 0.2, 'fixture teleport');
  await fixture.command('clear @s'); await fixture.raw('hotbar 1');
  for (const [pos, id] of [[power, 'creative_energy_cell'], [drive, 'drive'], [host, 'cable_bus'],
    [cpu, '1k_crafting_storage'], [bus, 'cable_bus'], [at(1, 0, -1), 'cable_bus']] as const) {
    await fixture.command(`setblock ${pos} ae2:${id}`);
  }
  await fixture.command(`item replace block ${drive} container.0 with ae2:item_storage_cell_1k`);
  await fixture.replace('hotbar.0', 'ae2:cable_pattern_provider'); await fixture.select(0, 'ae2:cable_pattern_provider');
  assert.equal((await fixture.ae(`grid part ${host} north`)).attached, true);
  await fixture.replace('hotbar.0', 'ae2:blank_pattern'); await fixture.select(0, 'ae2:blank_pattern');
  assert.equal((await fixture.ae('crafting encode minecraft:oak_log - - - - - - - -')).encoded, true);
  await until(() => fixture.raw('inventory'), s => s.held?.item === 'ae2:crafting_pattern', 'encoded plank pattern');
  await fixture.raw(`interact ${host} north`);
  const menu = await until(() => fixture.raw('screen'), s => s.slots?.some((v: any) => v.item === 'ae2:crafting_pattern' && v.slot < 36), 'provider menu');
  await fixture.raw(`click-slot ${menu.slots.find((v: any) => v.item === 'ae2:crafting_pattern' && v.slot < 36).slot} shift`);
  await until(() => fixture.raw('inventory'), s => !s.counts?.['ae2:crafting_pattern'], 'pattern installed');
  await fixture.raw('close');
  await fixture.replace('hotbar.0', 'minecraft:oak_log'); await fixture.select(0, 'minecraft:oak_log');
  await until(() => fixture.ae(`grid status ${drive}`), state => state.online && state.powered, 'drive finishes source grid boot before deposit', 30000);
  assert.equal((await fixture.ae(`grid deposit ${drive}`)).inserted, 1);
  await fixture.replace('hotbar.0', 'ae2:export_bus'); await fixture.select(0, 'ae2:export_bus');
  assert.equal((await fixture.ae(`grid part ${bus} east`)).attached, true);
  await until(() => fixture.ae(`grid status ${bus}`), s => s.online && s.powered, 'export carrier online', 30000);
  await fixture.replace('hotbar.0', 'ae2:crafting_card'); await fixture.select(0, 'ae2:crafting_card');
  await fixture.ae(`bus install ${bus} east`);
  await fixture.replace('hotbar.0', 'minecraft:oak_planks'); await fixture.select(0, 'minecraft:oak_planks');
  await fixture.ae(`bus filter ${bus} east 0`);
  await fixture.replace('hotbar.0', 'minecraft:diamond'); await fixture.select(0, 'minecraft:diamond');
  await fixture.ae(`bus filter ${bus} east 1`);
  await fixture.ae(`bus schedule ${bus} east round_robin`);
  await fixture.hold();
  await fixture.command(`setblock ${target} minecraft:chest`);
  assert.equal((await busStatus()).state.speed_cards, 0, 'one operation per bus tick');
  assert.equal((await busStatus()).state.next_slot, 0);

  await stepUntilCursor(1);
  assert.equal(await linkCount(), 0, 'first slot starts a calculation without immediately linking a CPU');
  assert.equal(await chestCount('minecraft:diamond'), 0);
  fixture.mark('the first craft preflight advances round robin and leaves a calculation pending');

  // Remove and return the same physical card before the requester next polls
  // its completed future. A removed/reinstalled card must not inherit it.
  await fixture.command('item replace entity @s hotbar.0 with minecraft:air');
  assert.equal((await fixture.ae(`bus remove ${bus} east 0`)).removed, 'ae2:crafting_card');
  assert((await fixture.command('data get entity @s SelectedItem')).chat.join('\n').includes('ae2:crafting_card'));
  assert.equal((await fixture.ae(`bus install ${bus} east`)).installed, 'ae2:crafting_card');

  // There is no diamond recipe, but the target accepts a preflight and the
  // consumed operation still advances the cursor past slot 1.
  await stepUntilCursor(2);
  assert.equal(await linkCount(), 0, 'removed card does not commit its old calculation');
  await stepUntilCursor(1);
  assert.equal(await linkCount(), 0, 'reinstalled card starts a new calculation');
  await stepUntilCursor(2);
  assert.equal(await linkCount(), 0, 'failed diamond planning does not add a CPU link');
  await stepUntilCursor(1);
  assert.equal(await linkCount(), 1, 'the new card submits its own completed calculation');
  await stepUntilCursor(2);
  assert.equal(await linkCount(), 1, 'diamond planning does not add another CPU link');
  fixture.mark('removing and reinstalling a crafting card cancels its earlier pending calculation');
  // This command changes the real server inventory. The selected slot is
  // already zero; held simulation intentionally defers client packets.
  await fixture.command('item replace entity @s hotbar.0 with minecraft:diamond 3');
  assert.equal((await fixture.ae(`grid deposit ${drive}`)).inserted, 3);

  // From cursor 2, the next attempt visits the already-linked slot 0 before
  // slot 1. A busy tracker must still spend the preflighted budget.
  await stepUntilCursor(1);
  assert.equal(await linkCount(), 1, 'busy preflight does not duplicate the job');
  assert.equal(await chestCount('minecraft:diamond'), 0, 'busy slot uses the entire operation');
  assert.equal(await gridCount('minecraft:diamond'), 3);
  await stepUntilCursor(2);
  assert.equal(await chestCount('minecraft:diamond'), 1, 'next tick physically exports slot 1');
  assert.equal(await gridCount('minecraft:diamond'), 2);
  fixture.mark('a busy first slot consumes its preflight budget before a later physical export');

  await fixture.hold(false);
  await fixture.restart();
  await until(() => fixture.ae(`grid status ${drive}`), state => state.online && state.powered, 'restarted grid finishes source boot', 30000);
  assert.equal(await linkCount(), 1, 'pending job remains linked after restart');
  assert.equal(await chestCount('minecraft:diamond') + await gridCount('minecraft:diamond'), 3, 'diamond custody survives restart');
  await fixture.command(`setblock ${assembler} ae2:molecular_assembler`);
  await until(() => chestCount('minecraft:oak_planks'), count => count >= 1, 'crafted plank reaches real chest', 60000);
  assert.equal(await chestCount('minecraft:oak_planks') + await gridCount('minecraft:oak_planks'), 4, 'one log produced four conserved planks');
  assert.equal(await chestCount('minecraft:diamond') + await gridCount('minecraft:diamond'), 3);
  await fixture.restart();
  await until(() => fixture.ae(`grid status ${drive}`), state => state.online && state.powered, 'finished network restarts online', 30000);
  assert.equal(await chestCount('minecraft:oak_planks') + await gridCount('minecraft:oak_planks'), 4);
  assert.equal(await chestCount('minecraft:diamond') + await gridCount('minecraft:diamond'), 3);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('pending crafting and all physical outputs retain custody through restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
