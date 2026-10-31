/** Configured FE and item P2P taxes debit a real, finite carrier grid. */
import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('p2p-configured-tax', mode);
const dir = join(fixture.game, 'config'); mkdirSync(dir, { recursive: true });
writeFileSync(join(dir, 'ae2-common.toml'), '[powerRatios]\nforgeEnergy=0.5\nusageMultiplier=2\np2pTunnelEnergyTax=0.5\np2pTunnelTransportTax=0.5\n');
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 6, y = Math.ceil(py) + 3, z = Math.floor(pz) + 6;
  const at = (dx: number, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const carrier = at(3, -1), dest = at(7, -1), barrel = at(2, 0, 1), hopper = at(4, -1), generator = at(0, 0, 1);
  const total = (s: any) => s.energy + s.energy_service.buffer;
  const carrierPower = () => fixture.ae(`power ${carrier} north`);
  const destinationPower = () => fixture.ae(`power ${dest} north`);
  const diamondCount = async (pos: string) => ((await fixture.raw(`container ${pos}`)).slots ?? [])
    .filter((s: any) => s?.item === 'minecraft:diamond').reduce((n: number, s: any) => n + s.count, 0);
  const attach = async (pos: string, face: string, item: string) => {
    await fixture.replace('hotbar.0', item); await fixture.select(0, item);
    assert.equal((await fixture.ae(`grid part ${pos} ${face}`)).attached, true);
  };
  const pair = async (input: string, inputFace: string, output: string, outputFace: string, inputX: number, outputX: number) => {
    await fixture.replace('hotbar.0', 'ae2:memory_card'); await fixture.select(0, 'ae2:memory_card');
    await fixture.command(`tp @s ${x + inputX + 0.5} ${y} ${z + 1.5}`);
    await fixture.raw('key shift down');
    try { await fixture.raw(`interact ${input} ${inputFace}`); } finally { await fixture.raw('key shift up'); }
    await until(() => fixture.command('data get entity @s SelectedItem'), r => JSON.stringify(r).includes('ae2_p2p_card'), 'physical P2P frequency saved');
    await fixture.command(`tp @s ${x + outputX + 0.5} ${y - 2} ${z + 0.5}`);
    await fixture.raw(`interact ${output} ${outputFace}`);
  };
  await fixture.command(`fill ${at(-3, -3, -3)} ${at(10, 3, 4)} minecraft:air`);
  await fixture.command(`fill ${at(-3, -3, -3)} ${at(10, -3, 4)} minecraft:stone`);
  await fixture.command('clear @s'); await fixture.raw('hotbar 1');
  for (let dx = 0; dx <= 6; dx++) await fixture.command(`setblock ${at(dx)} ae2:cable_bus`);
  await fixture.command(`setblock ${carrier} ae2:energy_cell`);
  await fixture.command(`data merge block ${carrier} {ae2_lab_energy:100000.0d}`);
  await attach(at(0), 'south', 'ae2:fe_p2p_tunnel');
  await attach(at(6), 'down', 'ae2:fe_p2p_tunnel');
  await attach(at(2), 'south', 'ae2:item_p2p_tunnel');
  await attach(at(4), 'down', 'ae2:item_p2p_tunnel');
  await pair(at(0), 'south', at(6), 'down', 0, 6);
  await pair(at(2), 'south', at(4), 'down', 2, 4);
  await fixture.command(`setblock ${at(6, -1)} ae2:energy_acceptor`);
  await fixture.command(`setblock ${dest} ae2:energy_cell`);
  await fixture.command(`setblock ${barrel} minecraft:barrel`);
  await fixture.command(`setblock ${hopper} minecraft:hopper`);
  await until(() => fixture.ae(`grid status ${carrier}`), s => s.powered && s.online, 'finite P2P carrier online', 30000);
  await until(async () => (await carrierPower()).energy_service, s => !!s?.members, 'carrier energy service', 30000);
  await until(async () => (await destinationPower()).energy_service, s => !!s?.members, 'destination energy service', 30000);
  await until(() => fixture.ae(`power ${at(0)} south`), s => s.fe?.can_receive, 'paired FE input capability', 30000);
  await fixture.hold();
  const preIdle = total(await carrierPower()); await fixture.stepServer(1);
  const idle = preIdle - total(await carrierPower());
  const destinationBefore = total(await destinationPower());
  await fixture.command(`setblock ${generator} ae2:debug_energy_gen`);
  const feBefore = total(await carrierPower()); await fixture.stepServer(1);
  const feLoss = feBefore - total(await carrierPower());
  const destinationAfter = total(await destinationPower());
  assert(Math.abs((destinationAfter - destinationBefore) - 4) < 1e-6, '8 FE moves as 4 AE into the real destination');
  assert(Math.abs((feLoss - idle) - 4) < 1e-6, '8 FE × 0.5 AE/FE × 0.5 tax × 2 usage charges 4 AE');
  await fixture.command(`setblock ${generator} minecraft:air`);
  fixture.mark('configured FE tax debits the physical carrier while FE reaches the remote grid');

  await fixture.command(`item replace block ${barrel} container.0 with minecraft:diamond`);
  let itemTax: number | undefined;
  for (let i = 0; i < 24; i++) {
    const before = total(await carrierPower()); await fixture.stepServer(1);
    const lost = before - total(await carrierPower());
    if (await diamondCount(hopper) === 1) { itemTax = lost - idle; break; }
  }
  assert.equal(await diamondCount(barrel), 0);
  assert.equal(await diamondCount(hopper), 1);
  assert(itemTax !== undefined && Math.abs(itemTax - 1) < 1e-6,
    'one item × 0.5 transport tax × 2 usage charges 1 AE');
  await fixture.hold(false); await fixture.restart();
  assert.equal(await diamondCount(hopper), 1);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('configured item tax accompanies one physical transferred diamond and survives restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
