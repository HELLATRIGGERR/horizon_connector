/** Inscriber inputs follow source partial-recipe and Name Press filtering. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('inscriber-menu-filter', mode);
const slot = (view: any, index: number) => view.slots?.find((entry: any) => entry.slot === index);
try {
  const start = await fixture.start(), [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 24, y = Math.ceil(py) + 5, z = Math.floor(pz) + 24;
  const first = `${x} ${y} ${z}`, second = `${x + 4} ${y} ${z}`;
  await fixture.command(`tp @s ${x + 2.5} ${y} ${z + 2.5}`);
  for (const [dx, dz] of [[-2, -3], [-2, 3], [6, -3], [6, 3]] as const) {
    await until(() => fixture.raw(`block ${x + dx} ${y} ${z + dz}`), state => state.loaded, 'fixture corner chunk loaded', 30000);
  }
  await fixture.command(`fill ${x - 2} ${y} ${z - 3} ${x + 6} ${y + 4} ${z + 3} minecraft:air`);
  await fixture.command(`fill ${x - 2} ${y - 1} ${z - 3} ${x + 6} ${y - 1} ${z + 3} minecraft:stone`);
  await fixture.command(`setblock ${first} ae2:inscriber`); await fixture.command(`setblock ${second} ae2:inscriber`);
  await fixture.command('clear @s'); await fixture.command('gamemode survival @s');
  await fixture.replace('hotbar.0', 'minecraft:dirt', 2); await fixture.replace('hotbar.1', 'ae2:silicon', 3);
  await fixture.replace('hotbar.2', 'ae2:silicon_press'); await fixture.replace('hotbar.3', 'ae2:logic_processor_press');
  await fixture.replace('hotbar.4', 'ae2:name_press'); await fixture.replace('hotbar.8', 'minecraft:stick');
  const open = async (target: string, dx: number) => {
    await fixture.select(8, 'minecraft:stick'); await fixture.command(`tp @s ${x + dx + .5} ${y} ${z + 2.5}`);
    await fixture.raw(`interact ${target} south`);
    await until(() => fixture.raw('screen'), view => slot(view, 302) && fixture.settingIs(view, 26, 'SIXTY_FOUR'), 'physical Inscriber');
  };
  await open(first, 0);
  await fixture.raw('click-slot 0'); await fixture.raw('click-slot 302'); await fixture.command(`data get block ${first}`);
  await until(() => fixture.raw('screen'), view => !slot(view, 302)?.item && view.carried?.item === 'minecraft:dirt' && view.carried.count === 2, 'an unrelated middle item is rejected with its cursor intact');
  await fixture.raw('click-slot 0'); await fixture.raw('click-slot 1'); await fixture.raw('click-slot 302');
  await until(() => fixture.raw('screen'), view => slot(view, 302)?.item === 'ae2:silicon' && slot(view, 302)?.count === 3 && !view.carried, 'source silicon ingredient accepted');
  await fixture.raw('click-slot 2'); await fixture.raw('click-slot 300');
  await until(() => fixture.raw('screen'), view => slot(view, 300)?.item === 'ae2:silicon_press' && !view.carried, 'matching press accepted');
  await fixture.raw('click-slot 3'); await fixture.raw('click-slot 301'); await fixture.command(`data get block ${first}`);
  await until(() => fixture.raw('screen'), view => !slot(view, 301)?.item && view.carried?.item === 'ae2:logic_processor_press', 'incompatible second press is rejected');
  await fixture.raw('click-slot 3'); await fixture.raw('close');

  await open(second, 4); await fixture.raw('click-slot 4'); await fixture.raw('click-slot 300');
  await until(() => fixture.raw('screen'), view => slot(view, 300)?.item === 'ae2:name_press' && !view.carried, 'Name Press accepted in top input');
  await fixture.raw('click-slot 0'); await fixture.raw('click-slot 302');
  await until(() => fixture.raw('screen'), view => slot(view, 302)?.item === 'minecraft:dirt' && slot(view, 302)?.count === 2 && !view.carried, 'Name Press permits the otherwise invalid rename input');
  await fixture.raw('close'); await fixture.restart();
  const a = JSON.stringify(await fixture.command(`data get block ${first}`)); const b = JSON.stringify(await fixture.command(`data get block ${second}`));
  assert.match(a, /ae2:silicon_press/); assert.match(a, /ae2:silicon/); assert.doesNotMatch(a, /minecraft:dirt|logic_processor_press/);
  assert.match(b, /ae2:name_press/); assert.match(b, /minecraft:dirt/);
  assert.equal((await fixture.raw('inventory')).counts?.['ae2:logic_processor_press'], 1);
  fixture.assertHidden(await fixture.raw('status')); fixture.mark('partial recipe and Name Press filters preserve every physical stack across restart'); fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
