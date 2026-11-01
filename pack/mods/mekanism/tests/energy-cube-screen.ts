/** The Energy Cube's screen (`mek_ui`), as a player uses it.
 *
 * Creative cube, basic cube A north of it, basic cube B north of A (fronts north). The player opens A:
 * - the screen shows the cube's two slots (discharge, charge) and none of the hopper menu's other three;
 * - the side configuration tab opens the energy page; right-clicking the front side button turns it from Output
 *   to Input: the block's north face shows `input`, the save says so, and A stops ejecting into B;
 *   left-clicking it makes it Output again and B fills again at 4000 J a tick;
 * - the auto-eject button stops and restarts the ejector; the items page changes the item side config.
 * Screenshots of the three pages go to the run folder (`cube-screen-*.png`).
 *
 * Needs what `energy-cube.ts` needs; `pack/scripts/mekanism.ts world` runs both. */
import assert from 'node:assert/strict';
import { join, resolve } from 'node:path';
import { WorldFixture, until } from '../../../../ae2/tests/world/harness';
import { root } from '../../../../ae2/scripts/common';

const content = resolve(process.env.MEK_CONTENT_DIR ?? join(root, '../.build/content/mekanism'));
const fixture = new WorldFixture('mekanism-cube-screen', 'server-thread', { devMods: [content] });

async function saved(pos: string): Promise<string> {
  const reply = await fixture.command(`data get block ${pos}`);
  const line = (reply.chat ?? []).find((text: string) => text.includes('has the following block data'));
  assert(line, `no block entity at ${pos}: ${JSON.stringify(reply.chat)}`);
  return line;
}
async function energy(pos: string): Promise<number> {
  const stored = (await saved(pos)).match(/stored: (\d+)L/);
  return stored ? Number(stored[1]) : 0;
}
/** B's gain over `ticks` held ticks. */
async function gain(b: string, ticks: number): Promise<number> {
  await fixture.hold();
  const before = await energy(b);
  await fixture.stepServer(ticks);
  const after = await energy(b);
  await fixture.hold(false);
  return after - before;
}

const log: Record<string, unknown> = {};
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 3, y = Math.floor(py) + 1, z = Math.floor(pz) - 2;
  const creative = `${x} ${y} ${z}`, a = `${x} ${y} ${z - 1}`, b = `${x} ${y} ${z - 2}`;
  // A floor east of the cubes to stand on, two blocks from A.
  await fixture.command(`fill ${x + 1} ${y} ${z - 6} ${x + 5} ${y + 3} ${z + 1} minecraft:air`);
  await fixture.command(`fill ${x - 1} ${y - 1} ${z - 6} ${x + 5} ${y - 1} ${z + 1} minecraft:stone`);
  await fixture.command(`setblock ${creative} mekanism:creative_energy_cube`);
  await fixture.command(`setblock ${a} mekanism:basic_energy_cube`);
  await fixture.command(`setblock ${b} mekanism:basic_energy_cube`);
  await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 0.5}`);
  const blockState = async () => (await fixture.raw(`block ${a}`)).state as string;
  await until(blockState, s => String(s).includes('north=output'), 'cube A reaches the client');

  // Open A: the cube's own two slots, not the hopper menu's other three.
  await fixture.raw(`interact ${a} east`);
  const view = await until(() => fixture.raw('screen'), v => v.screen === 'container' && (v.slots ?? []).some((s: any) => s.slot === 300), 'the cube screen opens');
  // Container slots are numbered from 300.
  const own = (view.slots as any[]).filter(s => s.slot >= 300 && s.slot < 400).map(s => s.slot - 300);
  log.container_slots = own;
  assert.deepEqual(own, [0, 1], `only the discharge and charge slots: ${JSON.stringify(view.slots)}`);
  assert.equal(view.title?.[0]?.text, 'Basic Energy Cube');
  // The 960x540 fixture window at vanilla's automatic GUI scale (2): the 176x166 screen is centred in 480x270.
  const scale = Math.max(1, Math.min(Math.floor(960 / 320), Math.floor(540 / 240)));
  const [ox, oy] = [Math.trunc((960 / scale - 176) / 2), Math.trunc((540 / scale - 166) / 2)];
  const click = (dx: number, dy: number, right = false) => fixture.raw(`click-at ${ox + dx} ${oy + dy}${right ? ' right' : ''}`);
  const shots: string[] = [];
  log.screenshots = shots;
  const shot = async (name: string) => {
    await Bun.sleep(1500);
    const path = join(fixture.run, `cube-screen-${name}.png`);
    await fixture.raw(`screenshot ${path}`);
    shots.push(path);
  };
  await Bun.sleep(1500);
  await shot('main');
  fixture.mark('the screen shows the discharge and charge slots only');

  // Energy flows A -> B at the basic output.
  assert.equal(await gain(b, 5), 20000);

  // The side configuration tab, then a right click on the front side button: Output -> Input.
  await click(-12, 19);
  await shot('energy-config');
  await click(95, 56, true);
  await until(blockState, s => String(s).includes('north=input'), 'the front face shows input');
  assert(/config0: ?\[I; ?1,/.test(await saved(a)), 'the front is saved as INPUT');
  assert.equal(await gain(b, 10), 0, 'A no longer ejects through its front');
  fixture.mark('right-clicking the front side button stops A ejecting into B (north=input)');
  await click(95, 56);
  await until(blockState, s => String(s).includes('north=output'), 'the front face shows output again');
  assert.equal(await gain(b, 10), 40000);
  fixture.mark('left-clicking it makes the front Output again: B gains 4000 J a tick');

  // Auto-eject off and on.
  await click(129, 23);
  await until(() => saved(a), s => /eject0: ?0b/.test(s), 'auto-eject off is saved');
  assert.equal(await gain(b, 10), 0, 'no ejecting with auto-eject off');
  await click(129, 23);
  await until(() => saved(a), s => /eject0: ?1b/.test(s), 'auto-eject on again');
  assert.equal(await gain(b, 10), 40000);
  fixture.mark('the auto-eject button stops and restarts the ejector');

  // The items page: the top side's item type steps from Input to Output.
  await click(49, 63);
  await click(95, 39);
  await until(() => saved(a), s => /config3: ?\[I; ?4, ?1, ?1, ?1, ?4, ?1\]/.test(s), 'the item side config changes');
  await shot('items-config');
  await click(44, 10);
  fixture.mark('the items page changes the item side config; close returns to the cube');
  log.saved_a = await saved(a);
  await fixture.raw('close');
  fixture.assertHidden(await fixture.raw('status'));
  console.log('MEK screen', JSON.stringify(log));
  fixture.report('passed');
} catch (error) {
  console.log('MEK screen', JSON.stringify(log));
  fixture.report('failed', error); throw error;
} finally { await fixture.close(); }
