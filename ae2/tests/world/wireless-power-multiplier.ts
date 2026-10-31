/** WirelessTerminalMenuHost ignores CONFIG's power multiplier for transfers. */
import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('wireless-power-multiplier', mode);
const config = join(fixture.game, 'config'); mkdirSync(config, { recursive: true });
writeFileSync(join(config, 'ae2-common.toml'), '[powerRatios]\nusageMultiplier=3\n');
const energy = (reply: any) => {
  const match = (reply.chat ?? []).join('\n').match(/"ae2:stored_energy":\s*(-?[0-9]+(?:\.[0-9]+)?)(?:d)?/);
  assert(match, `missing physical terminal energy: ${JSON.stringify(reply)}`);
  return Number(match[1]);
};
try {
  const start = await fixture.start(), [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 24, y = Math.ceil(py) + 5, z = Math.floor(pz) + 24;
  const at = (dx: number) => `${x + dx} ${y} ${z}`, access = at(1), drive = at(-1);
  await fixture.command(`tp @s ${x + 1.5} ${y} ${z + 2.5}`);
  for (const [dx, dz] of [[-3, -3], [-3, 3], [4, -3], [4, 3]] as const) {
    await until(() => fixture.raw(`block ${x + dx} ${y} ${z + dz}`), state => state.loaded, 'fixture corner chunk loaded', 30000);
  }
  await fixture.command(`fill ${x - 3} ${y} ${z - 3} ${x + 4} ${y + 4} ${z + 3} minecraft:air`);
  await fixture.command(`fill ${x - 3} ${y - 1} ${z - 3} ${x + 4} ${y - 1} ${z + 3} minecraft:stone`);
  await fixture.command(`setblock ${at(0)} ae2:creative_energy_cell`);
  await fixture.command(`setblock ${access} ae2:wireless_access_point[facing=east]`);
  await fixture.command(`setblock ${drive} ae2:drive`);
  await fixture.command(`item replace block ${drive} container.0 with ae2:item_storage_cell_1k[ae2:storage_cell_inv=[{"#t":"ae2:i",id:"minecraft:diamond","#":3L}]]`);
  await until(() => fixture.ae(`grid status ${access}`), state => state.online && state.powered, 'powered wireless network', 30000);
  await fixture.command('clear @s');
  // Creative suppresses idle drain only, isolating the source transfer debit.
  await fixture.command('gamemode creative @s');
  const owner = `ae2:wireless_terminal[ae2:wireless_link_target={dimension:"minecraft:overworld",pos:[I;${x + 1},${y},${z}]},ae2:stored_energy=100.0d]`;
  await fixture.replace('hotbar.0', owner); await fixture.select(0, 'ae2:wireless_terminal');
  await fixture.raw('look 0 -80'); await fixture.raw('use-item');
  const view = await until(() => fixture.raw('screen'), state =>
    state.screen === 'container' && state.slots?.some((entry: any) => entry.slot >= 300 && entry.slot < 345 && entry.item === 'minecraft:diamond'), 'real stored diamonds in wireless view');
  const diamond = view.slots.find((entry: any) => entry.slot >= 300 && entry.slot < 345 && entry.item === 'minecraft:diamond');
  assert.equal(energy(await fixture.command('data get entity @s SelectedItem')), 100);
  await fixture.raw(`click-slot ${diamond.slot}`);
  await until(() => fixture.raw('inventory'), state => state.carried?.item === 'minecraft:diamond' && state.carried.count === 3, 'all three physical diamonds extracted');
  assert.equal(energy(await fixture.command('data get entity @s SelectedItem')), 97, 'wireless host debits three AE, independent of usageMultiplier=3');
  await fixture.raw('close');
  await until(() => fixture.raw('inventory'), state => !state.carried && state.counts?.['minecraft:diamond'] === 3, 'cursor diamonds returned once');
  await fixture.restart(); await fixture.select(0, 'ae2:wireless_terminal');
  assert.equal(energy(await fixture.command('data get entity @s SelectedItem')), 97);
  assert.equal((await fixture.raw('inventory')).counts?.['minecraft:diamond'], 3);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('wireless transfer debits nominal AE under a non-unit configured multiplier and preserves item and energy custody across restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
