/** AE2's machine screens open on their menus with their styles, and the priority screen edits a Drive's priority. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('machine-screens', mode);
const centre = ([x, y, w, h]: number[]) => `${x + w / 2} ${y + h / 2}`;
// Each machine, the style its screen loads, and its x offset in the row.
const machines = [
  ['inscriber', 'inscriber.json'], ['drive', 'drive.json'], ['vibration_chamber', 'vibration_chamber.json'],
  ['io_port', 'io_port.json'], ['condenser', 'condenser.json'], ['molecular_assembler', 'molecular_assembler.json'],
  ['pattern_provider', 'pattern_provider.json'], ['interface', 'interface.json'], ['quantum_link', 'qnb.json'],
  ['spatial_io_port', 'spatial_io_port.json'], ['wireless_access_point', 'wireless_access_point.json'],
] as const;
try {
  const start = await fixture.start(), [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 24, y = Math.ceil(py) + 5, z = Math.floor(pz) + 24;
  const width = machines.length * 2;
  for (const [dx, dz] of [[-2, -3], [-2, 3], [width, -3], [width, 3]] as const) {
    await until(() => fixture.raw(`block ${x + dx} ${y} ${z + dz}`), state => state.loaded, 'chunk loaded', 30000);
  }
  await fixture.command(`fill ${x - 2} ${y} ${z - 3} ${x + width} ${y + 4} ${z + 3} minecraft:air`);
  await fixture.command(`fill ${x - 2} ${y - 1} ${z - 3} ${x + width} ${y - 1} ${z + 3} minecraft:stone`);
  for (const [i, [id]] of machines.entries()) await fixture.command(`setblock ${x + 2 * i} ${y} ${z} ae2:${id}`);
  await fixture.command('clear @s');
  await fixture.replace('hotbar.1', 'ae2:wireless_booster', 2);
  await fixture.replace('hotbar.8', 'minecraft:stick');
  const open = async (i: number) => {
    await fixture.select(8, 'minecraft:stick');
    await fixture.command(`tp @s ${x + 2 * i + 0.5} ${y} ${z + 2.5}`);
    await fixture.raw(`interact ${x + 2 * i} ${y} ${z} south`);
    return until(() => fixture.raw('screen'), view => view.ae2_screen, `${machines[i][0]} screen`, 15000);
  };
  for (const [i, [id, style]] of machines.entries()) {
    const view = await open(i);
    assert.equal(view.ae2_screen.style, style, id);
    assert.equal(view.ae2_screen.toolbar?.[0]?.button, 'help', `${id} help button`);
    if (id === 'wireless_access_point') {
      // Two boosters: 16 + 2^1.5 blocks (tenths) and 8 + 2^(1 + 2/64) AE/t (hundredths).
      await fixture.raw('click-slot 1');
      await fixture.raw('click-slot 300');
      const boosted = await until(() => fixture.raw('screen'), v => v.ae2_screen?.data?.[0] === 188, 'range with two boosters', 15000);
      assert.equal(boosted.ae2_screen.data[4], 1004);
    }
    await fixture.raw('close');
  }
  fixture.mark('every AE2 machine opens its own screen style with the help button; the WAP shows its boosters\' range and drain');

  // The Drive's priority tab opens the priority screen; its buttons, field and back tab work.
  const drive = machines.findIndex(([id]) => id === 'drive');
  const driveView = await open(drive);
  await fixture.raw(`click-at ${centre(driveView.ae2_screen.priority)}`);
  let priority = (await until(() => fixture.raw('screen'), v => v.ae2_priority, 'priority screen', 15000)).ae2_priority;
  assert.equal(priority.text, '0');
  await fixture.raw(`click-at ${centre(priority.buttons.find((b: any) => b.entry === 2).rect)}`);
  await until(() => fixture.raw('screen'), v => v.ae2_priority?.text === '100', 'plus 100', 15000);
  await fixture.raw(`click-at ${centre(priority.buttons.find((b: any) => b.entry === 4).rect)}`);
  priority = (await until(() => fixture.raw('screen'), v => v.ae2_priority?.text === '99', 'minus 1', 15000)).ae2_priority;
  await fixture.raw(`click-at ${centre(priority.field)}`);
  for (let n = 0; n < priority.text.length; n++) await fixture.raw('type \\b');
  await fixture.raw('type 12x');
  priority = (await until(() => fixture.raw('screen'), v => v.ae2_priority?.text === '12x', 'typed text', 15000)).ae2_priority;
  assert.equal(priority.valid, false);
  for (let n = 0; n < 3; n++) await fixture.raw('type \\b');
  await fixture.raw('type 42');
  await until(() => fixture.raw('screen'), v => v.ae2_priority?.text === '42' && v.ae2_priority.valid, 'typed 42', 15000);
  await fixture.raw(`click-at ${centre(priority.back)}`);
  const back = await until(() => fixture.raw('screen'), v => v.ae2_screen && !v.ae2_priority, 'back to the drive', 15000);
  assert.equal(back.ae2_screen.style, 'drive.json');
  await fixture.raw('close');
  await fixture.restart();
  const reopened = await open(drive);
  await fixture.raw(`click-at ${centre(reopened.ae2_screen.priority)}`);
  priority = (await until(() => fixture.raw('screen'), v => v.ae2_priority, 'priority screen again', 15000)).ae2_priority;
  assert.equal(priority.text, '42');
  await fixture.raw('close');
  fixture.mark('the priority screen adds and takes away, flags text that is no number, goes back, and the Drive keeps 42 across restart');
  fixture.assertHidden(await fixture.raw('status')); fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
