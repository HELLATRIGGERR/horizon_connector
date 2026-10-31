/** Source link-status reasons are rendered and synchronized per menu window. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('wireless-menu-status', mode);
const slot = (view: any, index: number) => view.slots?.find((entry: any) => entry.slot === index);
try {
  const start = await fixture.start(), [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 24, y = Math.ceil(py) + 5, z = Math.floor(pz) + 24;
  const at = (dx: number) => `${x + dx} ${y} ${z}`, access = at(1);
  await fixture.command(`tp @s ${x + 1.5} ${y} ${z + 2.5}`);
  for (const [dx, dz] of [[-3, -3], [-3, 3], [4, -3], [4, 3]] as const) {
    await until(() => fixture.raw(`block ${x + dx} ${y} ${z + dz}`), state => state.loaded, 'fixture corner chunk loaded', 30000);
  }
  await fixture.command(`fill ${x - 3} ${y} ${z - 3} ${x + 4} ${y + 4} ${z + 3} minecraft:air`);
  await fixture.command(`fill ${x - 3} ${y - 1} ${z - 3} ${x + 4} ${y - 1} ${z + 3} minecraft:stone`);
  await fixture.command(`setblock ${at(0)} ae2:creative_energy_cell`);
  await fixture.command(`setblock ${access} ae2:wireless_access_point[facing=east]`);
  await until(() => fixture.ae(`grid status ${access}`), state => state.online && state.powered, 'powered wireless access point', 30000);
  await fixture.command('clear @s'); await fixture.command('gamemode survival @s');
  const owner = (target: number | null, power = 10000) => `ae2:wireless_terminal[ae2:stored_energy=${power}.0d${target === null ? '' : `,ae2:wireless_link_target={dimension:"minecraft:overworld",pos:[I;${x + target},${y},${z}]}`}]`;
  await fixture.replace('hotbar.0', owner(null)); await fixture.select(0, 'ae2:wireless_terminal');
  await fixture.raw('look 0 -80'); await fixture.raw('use-item');
  const expectStatus = async (code: number, text: string | null) => {
    const view = await until(() => fixture.raw('screen'), state => state.screen === 'container' && slot(state, 352) && state.wireless_status?.code === code, `source link status ${code}`, 30000);
    assert.equal(view.wireless_status.description, text);
  };
  await expectStatus(1, 'Device is not linked.');
  await fixture.replace('hotbar.0', owner(4)); await expectStatus(2, 'Linked network cannot be found');
  await fixture.replace('hotbar.0', owner(1, 0)); await expectStatus(4, 'Out of Power');
  await fixture.raw('click-slot 352');
  await until(() => fixture.raw('screen'), state => String(slot(state, 300)?.components ?? '').includes('Items: enabled') && (!state.wireless_status || state.wireless_status.code === 0), 'key-type settings are not obscured by the storage overlay');
  await fixture.raw('click-slot 302'); await expectStatus(4, 'Out of Power');
  await fixture.replace('hotbar.0', owner(1)); await expectStatus(0, null);
  await fixture.command(`tp @s ${x + 128.5} ${y} ${z + 2.5}`);
  await expectStatus(3, 'Wireless Out Of Range.');
  await fixture.command(`tp @s ${x + 1.5} ${y} ${z + 2.5}`); await expectStatus(0, null);
  await fixture.raw('close'); await fixture.restart(); await fixture.select(0, 'ae2:wireless_terminal');
  await until(() => fixture.ae(`grid status ${access}`), state => state.online && state.powered, 'reloaded access point', 30000);
  await fixture.raw('look 0 -80'); await fixture.raw('use-item'); await expectStatus(0, null);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('unlinked, missing network, no power and range statuses update live and reconnect in the same window');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
