/** Mekanism's energy cubes, as the `mekanism` native bundle and its content mod make them.
 *
 * A creative cube, basic cube A north of it and basic cube B north of A, all as placed (front = north, ejecting):
 * - a cube shows its six face states and breaks into its item;
 * - the creative cube fills A, and A's ejector gives B the basic tier's output, 4000 J a tick;
 * - `/mek probe` answers a player with the cube's state;
 * - with the creative cube gone and A's ejector off, both keep their Joules through a save and a restart,
 *   and A ejects into B again once its ejector is back on.
 *
 * Joules are read from the saved compound (`data get block`, Mekanism's `energy_containers[{stored}]`).
 * Needs the staged native build (`AE2_TEST_BINARY`) and the content mod from `mekanism-content`
 * (`MEK_CONTENT_DIR`); `pack/scripts/mekanism.ts world` sets both. With `ATM10_DIR` the world reads ATM10's own
 * `config/Mekanism/general.toml` and `tiers.toml`. */
import assert from 'node:assert/strict';
import { copyFileSync, existsSync, mkdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { WorldFixture, until } from '../../../../ae2/tests/world/harness';
import { root } from '../../../../ae2/scripts/common';

const content = resolve(process.env.MEK_CONTENT_DIR ?? join(root, '../.build/content/mekanism'));
const fixture = new WorldFixture('mekanism-energy-cube', 'server-thread', { devMods: [content] });
const atm10 = process.env.ATM10_DIR;
const fromAtm10 = !!atm10 && existsSync(join(atm10, 'config/Mekanism/tiers.toml'));
if (fromAtm10) {
  const config = join(fixture.game, 'config/Mekanism');
  mkdirSync(config, { recursive: true });
  for (const file of ['general.toml', 'tiers.toml']) copyFileSync(join(atm10!, 'config/Mekanism', file), join(config, file));
}

/** The block entity's compound on the server, as `data get block` prints it. */
async function saved(pos: string): Promise<string> {
  const reply = await fixture.command(`data get block ${pos}`);
  const line = (reply.chat ?? []).find((text: string) => text.includes('has the following block data'));
  assert(line, `no block entity at ${pos}: ${JSON.stringify(reply.chat)}`);
  return line;
}
/** Joules saved in the cube; none saved is an empty container. */
async function energy(pos: string): Promise<number> {
  const stored = (await saved(pos)).match(/stored: (\d+)L/);
  return stored ? Number(stored[1]) : 0;
}
/** `/mek probe` as the player types it; its answer comes back in chat. Once per position. */
async function probe(pos: string): Promise<{ value: any; line: string }> {
  const needle = `"pos":[${pos.split(' ').join(',')}]`;
  await fixture.raw(`chat /mek probe ${pos}`);
  const found = (lines: string[] = []) => lines.findLast(text => text.startsWith('MEK {') && text.includes(needle));
  const view = await until(() => fixture.raw('chat-log 20'), v => !!found(v.chat), `/mek probe ${pos} answers in chat`);
  const line = found(view.chat)!;
  const value = JSON.parse(line.slice(4));
  assert(!value.error, line);
  return { value, line };
}

const LONG_MAX = '9223372036854775807';
const DEFAULT_FACES = { down: 'input', up: 'input', north: 'output', south: 'input', west: 'input', east: 'input' };
const log: Record<string, unknown> = {};
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 3, y = Math.floor(py) + 1, z = Math.floor(pz) - 2;
  const creative = `${x} ${y} ${z}`, a = `${x} ${y} ${z - 1}`, b = `${x} ${y} ${z - 2}`, loose = `${x + 2} ${y} ${z}`;

  // S0, with the world running (a held server sends the client nothing): the block as declared, its face states
  // shown, its item dropped.
  await fixture.command(`setblock ${loose} mekanism:basic_energy_cube`);
  const shown = await until(() => fixture.raw(`block ${loose}`), v => v.block === 'mekanism:basic_energy_cube', 'a placed cube reaches the client');
  for (const [face, value] of Object.entries(DEFAULT_FACES)) assert(String(shown.state).includes(`${face}=${value}`), `${face}=${value} in ${shown.state}`);
  log.block_state = shown.state;
  fixture.mark('a placed cube shows its default face states (front north lit)');
  await fixture.command(`setblock ${loose} minecraft:air destroy`);
  const dropped = await fixture.command(`execute positioned ${loose} run data get entity @e[type=minecraft:item,limit=1,sort=nearest,distance=..3] Item.id`);
  assert((dropped.chat ?? []).join(' ').includes('mekanism:basic_energy_cube'), `drop: ${JSON.stringify(dropped.chat)}`);
  await fixture.command(`kill @e[type=minecraft:item,distance=..64]`);
  fixture.mark('a broken cube drops its item');

  // S1: the entities as placed, with the item's default config (energy out of the front, ejecting).
  await fixture.hold();
  await fixture.command(`setblock ${creative} mekanism:creative_energy_cube`);
  await fixture.command(`setblock ${a} mekanism:basic_energy_cube`);
  await fixture.command(`setblock ${b} mekanism:basic_energy_cube`);
  const placed = await saved(a);
  log.placed_nbt = placed;
  for (const part of [/eject0: ?1b/, /config0: ?\[I; ?4, ?1, ?1, ?1, ?1, ?1\]/, /facing: ?["']north["']/]) assert(part.test(placed), `${part} in ${placed}`);
  assert(!placed.includes('energy_containers'), 'a new cube is empty');
  assert(!(await saved(creative)).includes('energy_containers'), 'a new creative cube saves nothing (and counts as full)');
  fixture.mark('cubes are placed with the default side config and no energy saved');

  // One tick: the creative cube fills A, A starts on B (in whichever order the cubes tick).
  await fixture.stepServer(1);
  const warm = { a: await energy(a), b: await energy(b) };
  log.after_first_tick = warm;
  assert(warm.a >= 3996000 && warm.a <= 4000000, `A filled from the creative cube: ${warm.a}`);
  // Then B gains exactly A's output each tick while the creative cube keeps A topped up.
  const perTick: number[] = [];
  let last = warm.b;
  const aSeen = new Set<number>();
  for (let i = 0; i < 5; i++) {
    await fixture.stepServer(1);
    const now = await energy(b);
    perTick.push(now - last); last = now; aSeen.add(await energy(a));
  }
  log.b_per_tick = perTick; log.a_each_tick = [...aSeen];
  assert.deepEqual(perTick, [4000, 4000, 4000, 4000, 4000]);
  assert.equal(aSeen.size, 1, `A stays topped up: ${[...aSeen]}`);
  await fixture.stepServer(100);
  const b100 = await energy(b);
  assert.equal(b100 - last, 400000, '100 ticks give 400 kJ');
  assert(!(await saved(creative)).includes('energy_containers'), 'the creative cube never changes');
  log.b_after_106_ticks = b100;
  fixture.mark(`B gains 4000 J a tick from A (${b100} J after 106 ticks)`);

  // With the world running: /mek probe as a player, and a picture of the cubes (fronts north, so seen from the
  // north-east) to compare with ATM10's.
  await fixture.hold(false);
  await until(() => fixture.raw(`block ${creative}`), v => v.block === 'mekanism:creative_energy_cube', 'the cubes reach the client');
  const pa = (await probe(a)).value, pc = await probe(creative);
  log.probe_a = pa;
  assert.equal(pa.tier, 'basic'); assert.equal(pa.capacity, 4000000); assert.equal(pa.output, 4000);
  assert(pa.energy >= 3996000, `A stays full: ${pa.energy}`);
  assert.equal(pa.facing, 'north'); assert.equal(pa.eject, true);
  assert.deepEqual(pa.faces, DEFAULT_FACES);
  assert.equal(pa.config.from_files, fromAtm10);
  assert.equal(pa.config.fe_conversion_rate, 2.5);
  assert(pc.line.includes(`"energy":${LONG_MAX}`), `the creative cube is full: ${pc.line}`);
  fixture.mark(`/mek probe answers a player (basic 4 MJ, 4000 J/t${fromAtm10 ? ' from ATM10\'s tiers.toml' : ''}; creative full)`);
  try {
    // Room to look from (the cubes are south of it), then 11 s for the chat to fade.
    await fixture.command(`fill ${x - 1} ${y} ${z - 8} ${x + 5} ${y + 4} ${z - 3} minecraft:air`);
    await fixture.command(`fill ${x - 1} ${y - 1} ${z - 8} ${x + 5} ${y - 1} ${z - 3} minecraft:stone`);
    await fixture.command(`tp @s ${x + 3.5} ${y} ${z - 5.5} 31 15`);
    await Bun.sleep(11000);
    const shot = join(fixture.run, 'cubes.png');
    await fixture.raw(`screenshot ${shot}`);
    log.screenshot = shot;
  } catch (error) { log.screenshot = `not taken: ${error}`; }
  await fixture.hold();

  // Freeze both: the source goes, A stops ejecting; then save and restart.
  await fixture.command(`setblock ${creative} minecraft:air`);
  await fixture.command(`data merge block ${a} {component_config:{eject0:0b}}`);
  await fixture.stepServer(3);
  const before = { a: await energy(a), b: await energy(b) };
  await fixture.stepServer(3);
  assert.deepEqual({ a: await energy(a), b: await energy(b) }, before, 'nothing moves with A not ejecting');
  log.before_restart = before;
  await fixture.hold(false);
  await fixture.restart();
  const after = { a: await energy(a), b: await energy(b) };
  log.after_restart = after;
  assert.deepEqual(after, before, 'both cubes keep their Joules through save and restart');
  assert(/eject0: ?0b/.test(await saved(a)), 'A keeps its ejector setting');
  assert(/eject0: ?1b/.test(await saved(b)));
  fixture.mark(`A ${after.a} J and B ${after.b} J survive save and restart`);

  // A's ejector back on: B fills from A again.
  await fixture.hold();
  await fixture.command(`data merge block ${a} {component_config:{eject0:1b}}`);
  const resumed = { a: await energy(a), b: await energy(b) };
  await fixture.stepServer(10);
  const later = { a: await energy(a), b: await energy(b) };
  log.after_resume_10_ticks = later;
  assert.equal(later.b - resumed.b, 40000); assert.equal(resumed.a - later.a, 40000);
  fixture.mark('after the restart A ejects 4000 J a tick into B again');
  fixture.assertHidden(await fixture.raw('status'));
  console.log('MEK numbers', JSON.stringify(log));
  fixture.report('passed');
} catch (error) {
  console.log('MEK numbers', JSON.stringify(log));
  fixture.report('failed', error); throw error;
} finally { await fixture.close(); }
