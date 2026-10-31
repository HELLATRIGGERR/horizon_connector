/** Source terminal search operators over real cell contents, in both paths. */
import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { WorldFixture, until, type Mode } from './harness';
import { startPeer } from './peer';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('terminal-search', mode);
let peer: Awaited<ReturnType<typeof startPeer>> | undefined;
const ids = ['minecraft:oak_log', 'minecraft:iron_ingot', 'ae2:fluix_crystal'];
const shown = (view: any) => ids.filter(id => view.slots?.some((slot: any) => slot.slot >= 300 && slot.slot < 345 && slot.item === id)).sort();
try {
  mkdirSync(join(fixture.game, 'config'), { recursive: true });
  writeFileSync(join(fixture.game, 'config', 'ae2-client.toml'), '[search]\nrememberLastSearch = false\nautoFocusSearch = true\n');
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 32, y = Math.ceil(py) + 5, z = Math.floor(pz) + 32;
  const at = (dx: number) => `${x + dx} ${y} ${z}`, drive = at(1), terminal = at(2);
  await fixture.command(`fill ${x - 2} ${y} ${z - 3} ${x + 4} ${y + 4} ${z + 3} minecraft:air`);
  await fixture.command(`fill ${x - 2} ${y - 1} ${z - 3} ${x + 4} ${y - 1} ${z + 3} minecraft:stone`);
  for (const [pos, id] of [[at(0), 'creative_energy_cell'], [drive, 'drive'], [terminal, 'cable_bus']] as const) await fixture.command(`setblock ${pos} ae2:${id}`);
  await fixture.command(`item replace block ${drive} container.0 with ae2:item_storage_cell_1k`);
  await fixture.command('clear @s'); await fixture.replace('hotbar.0', 'ae2:terminal'); await fixture.select(0, 'ae2:terminal');
  assert.equal((await fixture.ae(`grid part ${terminal} north`)).attached, true);
  await until(() => fixture.ae(`grid status ${drive}`), state => state.online && state.powered, 'powered search grid', 30000);
  for (const id of ids) {
    await fixture.replace('hotbar.0', id); await fixture.select(0, id);
    assert.equal((await fixture.ae(`grid deposit ${drive}`)).inserted, 1);
  }
  await fixture.replace('hotbar.8', 'minecraft:stick'); await fixture.select(8, 'minecraft:stick');
  await fixture.command(`tp @s ${x + 2.5} ${y} ${z - 2.5}`);
  for (const [query, expected] of [
    ['oak', ['minecraft:oak_log']],
    ['*minecraft:iron_ingot', ['minecraft:iron_ingot']],
    ['@ae2', ['ae2:fluix_crystal']],
    ['#minecraft:logs', ['minecraft:oak_log']],
    ['$ingot', ['minecraft:iron_ingot']],
    ['@minecraft log', ['minecraft:oak_log']],
    ['oak | iron', ['minecraft:oak_log', 'minecraft:iron_ingot']],
  ] as const) {
    await fixture.raw(`interact ${terminal} north`);
    await until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 ME Terminal') && shown(view).length === ids.length, 'unfiltered terminal');
    await fixture.raw(`type ${query}`);
    const actual = await until(() => fixture.raw('screen'), view => JSON.stringify(shown(view)) === JSON.stringify([...expected].sort()), `search ${query}`);
    assert.deepEqual(shown(actual), [...expected].sort());
    await fixture.raw('close');
  }
  if (mode === 'server-thread') {
    await fixture.raw('lan open 0 commands');
    const lan = await fixture.raw('lan status');
    peer = await startPeer(fixture, `127.0.0.1:${lan.port}`, 'AE2SearchPeer', {
      language: 'pt_br',
      clientConfig: '[search]\nrememberLastSearch = false\nautoFocusSearch = true\nsearchModNameInTooltips = true\n',
    });
    // Keep two real player hitboxes apart so neither obscures the other's
    // ray to the same terminal face.
    await fixture.command(`tp AE2SearchPeer ${x + 3.3} ${y} ${z - 2.5}`);
    await until(() => peer!.raw('status'), state => Math.abs(state.player.position[0] - x - 3.3) < 0.3 && Math.abs(state.player.position[2] - z + 2.5) < 0.3, 'second player reaches terminal');
    await fixture.raw(`interact ${terminal} north`);
    await until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 ME Terminal') && shown(view).length === ids.length, 'owner opens unfiltered view');
    await fixture.raw('type $applied');
    await until(() => fixture.raw('screen'), view => shown(view).length === 0, 'owner excludes mod name from tooltip search');
    await peer.raw(`interact ${terminal} north`);
    await until(() => peer!.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 ME Terminal') && shown(view).length === ids.length, 'peer opens independent unfiltered view');
    await peer.raw('type $applied');
    await until(() => peer!.raw('screen'), view => JSON.stringify(shown(view)) === JSON.stringify(['ae2:fluix_crystal']), 'peer includes mod name in tooltip search');
    assert.equal(shown(await fixture.raw('screen')).length, 0, 'peer preference does not change owner filter');
    await peer.raw('close'); await fixture.raw('close');

    await peer.raw(`interact ${terminal} north`);
    await until(() => peer!.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 ME Terminal') && shown(view).length === ids.length, 'peer reopens unfiltered terminal');
    await peer.raw('type tronco');
    await until(() => peer!.raw('screen'), view => JSON.stringify(shown(view)) === JSON.stringify(['minecraft:oak_log']), 'Portuguese display name matches for peer');
    await fixture.raw(`interact ${terminal} north`);
    await until(() => fixture.raw('screen'), view => JSON.stringify(view.title ?? '').includes('AE2 ME Terminal') && shown(view).length === ids.length, 'owner reopens unfiltered terminal');
    await fixture.raw('type tronco');
    await until(() => fixture.raw('screen'), view => shown(view).length === 0, 'English owner does not use peer language');
    assert.deepEqual(shown(await peer.raw('screen')), ['minecraft:oak_log'], 'owner query does not change peer filter');
    await fixture.raw('close'); await peer.raw('close');
    fixture.mark('simultaneous peers retain independent language and tooltip-search settings');
  }
  const final = await fixture.ae(`grid status ${drive}`);
  for (const id of ids) assert.equal(final.items?.find((item: any) => item.item === id)?.count, 1, `search leaves ${id} in its cell`);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('name, item ID, mod ID, tag, tooltip, AND and OR filters show the source-matching physical entries');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { if (peer) await peer.stop(); await fixture.close(); }
