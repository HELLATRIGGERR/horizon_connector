/** Forge Energy between Mekanism's energy cubes and AE2, through the engine's FE capability (hook points API 63).
 *
 * - AE2 powers a cube: the debug energy generator pushes 8 FE a tick into the cube beside it, which Mekanism
 *   takes as 20 J (2.5 J per FE, ATM10's feConversionRate).
 * - AE2 finds the cube's FE store per side: `/ae2 power` reads it as `IEnergyStorage` (in whole FE), and a side
 *   configured NONE has none.
 * - A cube feeds AE2: a full basic cube's front against an Energy Acceptor ejects its tier output, 4000 J =
 *   1600 FE a tick, which the grid stores as 800 AE.
 *
 * Joules are read from the cube's saved compound (`energy_containers[{stored}]`), AE from the energy cell.
 * Needs the build of `bun run pack/scripts/fe-test.ts stage build` (`AE2_TEST_BINARY`) and the Mekanism content
 * mod (`MEK_CONTENT_DIR`); `fe-test.ts world` sets both. */
import assert from 'node:assert/strict';
import { join, resolve } from 'node:path';
import { WorldFixture, until } from '../../../ae2/tests/world/harness';
import { root } from '../../../ae2/scripts/common';

const content = resolve(process.env.MEK_CONTENT_DIR ?? join(root, '../.build/content/mekanism'));
const fixture = new WorldFixture('mekanism-fe-ae2', 'server-thread', { devMods: [content] });

async function energy(pos: string): Promise<number> {
  const reply = await fixture.command(`data get block ${pos}`);
  const line = (reply.chat ?? []).find((text: string) => text.includes('has the following block data'));
  assert(line, `no block entity at ${pos}: ${JSON.stringify(reply.chat)}`);
  const stored = line.match(/stored: (\d+)L/);
  return stored ? Number(stored[1]) : 0;
}

const log: Record<string, unknown> = {};
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 6, y = Math.ceil(py) + 3, z = Math.floor(pz) + 6;
  const at = (dx: number, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  // Cube C with the generator on its east (an input side); cube A, its front (north) against the acceptor, with
  // an energy cell north of that. All cubes as placed: front north, ejecting, the five other sides input.
  const c = at(0), generator = at(1), a = at(0, 0, 8), acceptor = at(0, 0, 7), cell = at(0, 0, 6);
  await fixture.hold();
  await fixture.command(`fill ${at(-2, -2, -2)} ${at(3, 2, 10)} minecraft:air`);

  // AE2 → cube.
  await fixture.command(`setblock ${c} mekanism:basic_energy_cube`);
  await fixture.command(`setblock ${generator} ae2:debug_energy_gen`);
  assert.equal(await energy(c), 0);
  const fromGenerator: number[] = [];
  let last = 0;
  for (let i = 0; i < 5; i++) {
    await fixture.stepServer(1);
    const now = await energy(c);
    fromGenerator.push(now - last); last = now;
  }
  log.cube_from_generator_per_tick = fromGenerator;
  assert.deepEqual(fromGenerator, [20, 20, 20, 20, 20], '8 FE a tick are 20 J');
  fixture.mark(`the debug energy generator fills the cube by 20 J (8 FE) a tick: ${last} J after 5 ticks`);

  // AE2 reads the cube's FE store, side by side.
  const east = await fixture.ae(`power ${c} east`);
  log.cube_fe_east = east.fe;
  assert.deepEqual(east.fe, { stored: 40, capacity: 1600000, can_receive: true, can_extract: true },
    '100 J are 40 FE; 4 MJ are 1.6 M FE');
  assert.equal((await fixture.ae(`power ${c} north`)).fe?.stored, 40, 'the front (an output) exposes it too');
  // config0 by RelativeSide (front, left, right, back, top, bottom): the bottom NONE.
  await fixture.command(`data merge block ${c} {component_config:{config0:[I;4,1,1,1,1,0]}}`);
  const down = await fixture.ae(`power ${c} down`);
  log.cube_fe_down_none = down.fe;
  assert.equal(down.fe, null, 'a NONE side has no FE capability');
  await fixture.command(`setblock ${generator} minecraft:air`);
  fixture.mark('AE2 reads the cube as IEnergyStorage per side: 40 of 1 600 000 FE, none on a NONE side');

  // Cube → AE2: the grid first, then the cube's energy.
  await fixture.command(`setblock ${cell} ae2:energy_cell`);
  await fixture.command(`setblock ${acceptor} ae2:energy_acceptor`);
  await fixture.command(`setblock ${a} mekanism:basic_energy_cube`);
  await fixture.hold(false);
  const grid = () => fixture.ae(`power ${cell} north`);
  await until(grid, s => s.energy_service?.members > 0, 'the acceptor and cell form a grid', 30000);
  await fixture.hold();
  await fixture.command(`data merge block ${a} {energy_containers:[{container:0b,stored:4000000L}]}`);
  const total = (s: any) => s.energy + (s.energy_service?.buffer ?? 0);
  const cubeLoss: number[] = [], gridGain: number[] = [];
  let cubeBefore = await energy(a), gridBefore = total(await grid());
  assert.equal(cubeBefore, 4000000);
  for (let i = 0; i < 5; i++) {
    await fixture.stepServer(1);
    const cubeNow = await energy(a), gridNow = total(await grid());
    cubeLoss.push(cubeBefore - cubeNow); gridGain.push(Math.round((gridNow - gridBefore) * 1000) / 1000);
    cubeBefore = cubeNow; gridBefore = gridNow;
  }
  log.cube_to_acceptor_joules_per_tick = cubeLoss; log.grid_ae_per_tick = gridGain;
  assert.deepEqual(cubeLoss, [4000, 4000, 4000, 4000, 4000], 'the basic tier output, 4000 J a tick');
  for (const gain of gridGain) assert(Math.abs(gain - 800) < 1, `1600 FE are 800 AE: ${gridGain}`);
  fixture.mark(`a full basic cube feeds the energy acceptor 4000 J = 1600 FE = ${gridGain[gridGain.length - 1]} AE a tick`);

  fixture.assertHidden(await fixture.raw('status'));
  console.log('MEK-FE numbers', JSON.stringify(log));
  fixture.report('passed');
} catch (error) {
  console.log('MEK-FE numbers', JSON.stringify(log));
  fixture.report('failed', error); throw error;
} finally { await fixture.close(); }
