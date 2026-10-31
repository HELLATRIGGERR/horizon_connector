/** AEBaseMenu instances retain the original block/part owner and real cursor. */
import assert from 'node:assert/strict';
import { WorldFixture, until } from './harness';

const fixture = new WorldFixture('machine-menu-identity', 'server-thread');
const entry = (view: any, index: number) => view.slots?.find((slot: any) => slot.slot === index);
const text = (reply: any) => (reply.chat ?? []).join('\n');
try {
  const start = await fixture.start(); const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 24, y = Math.ceil(py) + 5, z = Math.floor(pz) + 24;
  const at = (dx: number, dz = 0) => `${x + dx} ${y} ${z + dz}`;
  await fixture.command(`fill ${x - 2} ${y} ${z - 3} ${x + 17} ${y + 4} ${z + 3} minecraft:air`);
  await fixture.command(`fill ${x - 2} ${y - 1} ${z - 3} ${x + 17} ${y - 1} ${z + 3} minecraft:stone`);
  const machines = [[0, 'ae2:inscriber'], [3, 'ae2:io_port'], [6, 'ae2:cell_workbench']] as const;
  for (const [dx, id] of machines) {
    await fixture.command(`setblock ${at(dx)} ${id}`); await fixture.command(`setblock ${at(dx, -1)} ${id}`);
    await fixture.command(`data modify block ${at(dx, -1)} ae2_lab_config.menu_identity_fixture set value 1`);
  }
  const part = at(9), donor = at(10), bus = at(13), busDonor = at(14);
  for (const target of [part, donor, bus, busDonor]) await fixture.command(`setblock ${target} ae2:cable_bus`);
  await fixture.command('clear @s'); await fixture.replace('hotbar.0', 'ae2:cable_interface'); await fixture.select(0, 'ae2:cable_interface');
  assert.equal((await fixture.ae(`grid part ${part} south`)).attached, true);
  assert.equal((await fixture.ae(`grid part ${donor} south`)).attached, true);
  await fixture.replace('hotbar.0', 'ae2:import_bus'); await fixture.select(0, 'ae2:import_bus');
  assert.equal((await fixture.ae(`grid part ${bus} south`)).attached, true);
  assert.equal((await fixture.ae(`grid part ${busDonor} south`)).attached, true);
  await fixture.command('gamemode survival @s');
  const open = async (target: string, dx: number, name: RegExp) => {
    await fixture.command('clear @s'); await fixture.replace('hotbar.0', 'minecraft:diamond', 3); await fixture.replace('hotbar.8', 'minecraft:stick');
    await fixture.select(8, 'minecraft:stick'); await fixture.command(`tp @s ${x + dx + .5} ${y} ${z + 2.5}`);
    await fixture.raw(`interact ${target} south`);
    await until(() => fixture.raw('screen'), view => name.test(JSON.stringify(view.title ?? '')) && entry(view, 300), 'physical machine menu');
    await fixture.raw('click-slot 0');
    await until(() => fixture.raw('inventory'), view => view.carried?.item === 'minecraft:diamond' && view.carried.count === 3, 'real cursor sentinel');
  };
  const closed = async (target: string, label: string) => {
    await until(() => fixture.raw('screen'), view => view.screen === 'playing', `${label} closes after owner replacement`);
    await fixture.command('data get entity @s SelectedItem');
    const inventory = await until(() => fixture.raw('inventory'), view => !view.carried && view.counts?.['minecraft:diamond'] === 3, `${label} returns the exact cursor stack`);
    assert.equal(inventory.counts?.['minecraft:diamond'], 3);
    assert(!text(await fixture.command(`data get block ${target}`)).includes('minecraft:diamond'), `${label} replacement did not acquire the stack`);
  };
  for (const [dx, id] of machines) {
    await open(at(dx), dx, /Inscriber|AE2 Machine/i);
    // The actual block-entity load replaces the native owner while retaining
    // identical physical slots; copying a marker makes the data update visible.
    await fixture.command(`data modify block ${at(dx)} ae2_lab_config set from block ${at(dx, -1)} ae2_lab_config`);
    await closed(at(dx), id);
  }
  fixture.mark('Inscriber, IO Port and Workbench parent menus close on same-kind block-entity replacement with exact cursor custody');
  await open(part, 9, /Interface/i);
  await fixture.command(`data modify block ${part} ae2_lab_config.parts[{face:"south"}] set from block ${donor} ae2_lab_config.parts[{face:"south"}]`);
  await closed(part, 'Interface face');
  fixture.mark('replacing the mounted Interface face closes its parent menu without transferring the carried stack');
  await open(bus, 13, /AE2 Bus/i);
  await fixture.command(`data modify block ${bus} ae2_lab_config.parts[{face:"south"}] set from block ${busDonor} ae2_lab_config.parts[{face:"south"}]`);
  await closed(bus, 'Import Bus face');
  fixture.mark('replacing a Bus face also closes the old owner session with its real cursor intact');
  await fixture.restart();
  const after = await fixture.raw('inventory'); assert.equal(after.counts?.['minecraft:diamond'], 3); assert(!after.carried);
  fixture.assertHidden(await fixture.raw('status')); fixture.mark('the final cursor sentinel survives restart'); fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
