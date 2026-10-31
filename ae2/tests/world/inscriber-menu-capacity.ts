/** Inscriber source input capacities apply to all three physical inputs. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('inscriber-menu-capacity', mode);
const slot = (view: any, index: number) => view.slots?.find((entry: any) => entry.slot === index);
try {
  const start = await fixture.start(), [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 24, y = Math.ceil(py) + 5, z = Math.floor(pz) + 24, machine = `${x} ${y} ${z}`;
  await fixture.command(`tp @s ${x + .5} ${y} ${z + 2.5}`);
  for (const [dx, dz] of [[-2, -3], [-2, 3], [2, -3], [2, 3]] as const) {
    await until(() => fixture.raw(`block ${x + dx} ${y} ${z + dz}`), state => state.loaded, 'fixture corner chunk loaded', 30000);
  }
  await fixture.command(`fill ${x - 2} ${y} ${z - 3} ${x + 2} ${y + 4} ${z + 3} minecraft:air`);
  await fixture.command(`fill ${x - 2} ${y - 1} ${z - 3} ${x + 2} ${y - 1} ${z + 3} minecraft:stone`);
  await fixture.command(`setblock ${machine} ae2:inscriber`);
  await fixture.command('clear @s'); await fixture.command('gamemode survival @s');
  await fixture.replace('hotbar.0', 'ae2:silicon_press', 4);
  await fixture.replace('hotbar.1', 'ae2:silicon', 8);
  await fixture.replace('hotbar.8', 'minecraft:stick'); await fixture.select(8, 'minecraft:stick');
  await fixture.command(`tp @s ${x + .5} ${y} ${z + 2.5}`); await fixture.raw(`interact ${machine} south`);
  await until(() => fixture.raw('screen'), view => slot(view, 300) && fixture.settingIs(view, 26, 'SIXTY_FOUR'), 'physical Inscriber with default input capacity');

  await fixture.raw('click-slot 0'); await fixture.raw('click-slot 300');
  await fixture.command(`data get block ${machine}`);
  await until(() => fixture.raw('screen'), view => slot(view, 300)?.item === 'ae2:silicon_press' && slot(view, 300)?.count === 4 && !view.carried, 'default top input accepts four physical presses');
  for (const [setting, amount] of [['ONE', 1], ['FOUR', 4], ['SIXTY_FOUR', 8]] as const) {
    await fixture.pressSetting(26);
    await until(() => fixture.raw('screen'), view => fixture.settingIs(view, 26, setting), `${setting} source capacity selected`);
    await fixture.raw('click-slot 1'); await fixture.raw('click-slot 302');
    await fixture.command(`data get block ${machine}`);
    await until(() => fixture.raw('screen'), view => slot(view, 302)?.item === 'ae2:silicon' && slot(view, 302)?.count === amount && (amount === 8 ? !view.carried : view.carried?.count === 8 - amount), `${setting} limits the middle input and retains the remainder`);
    if (amount !== 8) {
      await fixture.raw('click-slot 1'); await fixture.raw('click-slot 302'); await fixture.raw('click-slot 1');
      await until(() => fixture.raw('inventory'), view => !view.carried && view.hotbar[1]?.item === 'ae2:silicon' && view.hotbar[1]?.count === 8, 'input and remainder return to player without loss');
    }
  }
  await fixture.raw('close'); await fixture.restart();
  const saved = JSON.stringify(await fixture.command(`data get block ${machine}`));
  assert.match(saved, /ae2:silicon_press/); assert.match(saved, /ae2:silicon/);
  await fixture.command(`tp @s ${x + .5} ${y} ${z + 2.5}`); await fixture.raw(`interact ${machine} south`);
  const view = await until(() => fixture.raw('screen'), view => slot(view, 300)?.count === 4 && slot(view, 302)?.count === 8, 'physical press and input survive restart');
  assert.equal(view.carried, null); fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('default press capacity and the ONE, FOUR and SIXTY_FOUR input settings preserve exact physical counts through restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
