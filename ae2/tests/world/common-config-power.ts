/** Live AEConfig power ratios against physical FE, grid storage and idle drain. */
import assert from 'node:assert/strict';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('common-config-power', mode);
const configDir = join(fixture.game, 'config'); mkdirSync(configDir, { recursive: true });
const configFile = join(configDir, 'ae2-common.toml');
const config = (ratio: number, usage: number, perNode: number) =>
  `[powerRatios]\nforgeEnergy=${ratio}\nusageMultiplier=${usage}\ngridEnergyStoragePerNode=${perNode}\n[wireless]\nwirelessBaseRange=42\n`;
writeFileSync(configFile, config(2, 2, 100));
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 8, y = Math.ceil(py) + 3, z = Math.floor(pz) + 8;
  const at = (dx: number, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const charger = at(0), chargerGenerator = at(-1), chest = at(0, 0, 3);
  const cell = at(8), drive = at(9), acceptor = at(7), generator = at(6), extra = at(10);
  const chargerPower = () => fixture.ae(`power ${charger} east`);
  const gridPower = () => fixture.ae(`power ${cell} north`);
  await fixture.command(`fill ${at(-3, -1, -3)} ${at(12, -1, 5)} minecraft:stone`);
  await fixture.command(`fill ${at(-3, 0, -3)} ${at(12, 4, 5)} minecraft:air`);
  await fixture.hold();
  await fixture.command(`setblock ${charger} ae2:charger[facing=north]`);
  await fixture.command(`setblock ${chest} ae2:chest`);
  await fixture.command(`setblock ${chargerGenerator} ae2:debug_energy_gen`);
  const initial = await chargerPower();
  assert.equal(initial.fe.capacity, 800, '1600 AE / 2 AE per FE');
  assert.equal((await fixture.ae(`power ${chest} east`)).fe.capacity, 500, 'ME Chest buffer uses configured multiplier');
  await fixture.stepServer(1);
  const received = await chargerPower();
  assert.equal(received.energy, 16, '8 physical FE inject 16 AE at the configured ratio');
  assert.equal(received.fe.stored, 8);
  await fixture.command(`setblock ${chargerGenerator} minecraft:air`);
  fixture.mark('the configured FE ratio controls physical injection and charger/chest capabilities');

  await fixture.command(`setblock ${cell} ae2:energy_cell`);
  await fixture.command(`setblock ${drive} ae2:drive`);
  await fixture.command(`setblock ${acceptor} ae2:energy_acceptor`);
  await fixture.command(`setblock ${generator} ae2:debug_energy_gen`);
  await fixture.hold(false);
  await until(() => fixture.ae(`grid status ${drive}`), s => s.online && s.powered, 'finite-energy drive online', 30000);
  await fixture.hold();
  await fixture.command(`setblock ${generator} minecraft:air`);
  const oldCapacity = await until(gridPower, s => s.energy_service?.members > 0, 'ordered grid energy service');
  assert.equal(oldCapacity.energy_service.buffer_capacity, oldCapacity.energy_service.members * 100);

  writeFileSync(configFile, config(0.5, 3, 200));
  const updated = await until(chargerPower, s => s.fe?.capacity === 3200, 'hot-reloaded FE ratio', 10000);
  assert.equal(updated.energy, received.energy, 'changing a ratio does not create stored energy');
  assert.equal(updated.fe.stored, 32);
  assert.equal((await fixture.ae(`power ${chest} east`)).fe.capacity, 2000,
    'the existing chest retains the buffer capacity sampled at construction');
  assert.match(readFileSync(configFile, 'utf8'), /\[wireless\]\nwirelessBaseRange=42/);
  const before = await gridPower();
  assert.equal(before.energy_service.buffer_capacity, before.energy_service.members * 100,
    'existing node capacity changes on the next membership event');
  await fixture.stepServer(1);
  const after = await gridPower();
  const total = (s: any) => s.energy + s.energy_service.buffer;
  const expectedIdle = (2 + 3 * 1 / 128) * 3;
  assert(Math.abs((total(before) - total(after)) - expectedIdle) < 1e-6,
    'drive idle and ad-hoc channel pathing power are charged at 3x');
  await fixture.command(`setblock ${extra} ae2:cable_bus`);
  await fixture.stepServer(1);
  const expanded = await until(gridPower, s => s.energy_service?.members > before.energy_service.members,
    'new physical grid member', 30000);
  assert.equal(expanded.energy_service.buffer_capacity, expanded.energy_service.members * 200);
  fixture.mark('hot config changes the actual idle debit and node-event buffer capacity while retaining unrelated settings');

  await fixture.hold(false); await fixture.restart();
  assert.equal((await chargerPower()).fe.capacity, 3200);
  assert.equal((await fixture.ae(`power ${chest} east`)).fe.capacity, 3000,
    'the reloaded chest samples the new multiplier at construction');
  assert.match(readFileSync(configFile, 'utf8'), /\[wireless\]\nwirelessBaseRange=42/);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('configured ratios and unrelated sections survive process restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
