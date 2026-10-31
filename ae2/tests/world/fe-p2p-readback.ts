/** P2P energy capability readback uses AE2's signed Java int sums. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('fe-p2p-readback', mode);
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 6, y = Math.ceil(py) + 3, z = Math.floor(pz) + 6;
  const at = (dx: number, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const input = at(0), outputA = at(2), outputB = at(4);
  const read = () => fixture.ae(`power ${input} south`);
  await fixture.command(`fill ${at(-2, -2, -2)} ${at(6, 4, 4)} minecraft:air`);
  await fixture.command(`fill ${at(-2, -2, -2)} ${at(6, -2, 4)} minecraft:stone`);
  await fixture.command('clear @s'); await fixture.raw('hotbar 1');
  for (let dx = 0; dx <= 4; dx++) await fixture.command(`setblock ${at(dx)} ae2:cable_bus`);
  await fixture.command(`setblock ${at(2, -1)} ae2:creative_energy_cell`);
  for (const pos of [input, outputA, outputB]) {
    await fixture.replace('hotbar.0', 'ae2:fe_p2p_tunnel'); await fixture.select(0, 'ae2:fe_p2p_tunnel');
    assert.equal((await fixture.ae(`grid part ${pos} south`)).attached, true);
  }
  await fixture.command(`tp @s ${x + 1.5} ${y} ${z + 1.5}`);
  await until(() => fixture.raw('status'), s => Math.abs(s.player.position[0] - (x + 1.5)) < 0.2, 'input teleport');
  await fixture.replace('hotbar.0', 'ae2:memory_card'); await fixture.select(0, 'ae2:memory_card');
  await fixture.raw('key shift down');
  try { await fixture.raw(`interact ${input} south`); } finally { await fixture.raw('key shift up'); }
  await until(() => fixture.command('data get entity @s SelectedItem'), r => JSON.stringify(r).includes('ae2_p2p_card'), 'input frequency saved to physical card');
  await fixture.raw(`interact ${outputA} south`);
  await fixture.command(`setblock ${at(2, 0, 1)} ae2:debug_energy_gen`);
  const one = await until(read, value => value.fe?.stored === 2147483647 && value.fe?.capacity === 2147483647, 'one output exposes maximum signed FE', 30000);
  assert.equal(one.fe.can_receive, true);
  fixture.mark('one physical output exposes the source maximum FE readback');

  await fixture.command(`tp @s ${x + 4.5} ${y} ${z + 1.5}`);
  await until(() => fixture.raw('status'), s => Math.abs(s.player.position[0] - (x + 4.5)) < 0.2, 'second output teleport');
  await fixture.raw(`interact ${outputB} south`);
  await fixture.command(`setblock ${at(4, 0, 1)} ae2:debug_energy_gen`);
  await until(read, value => value.fe?.stored === -2 && value.fe?.capacity === -2, 'two source maximum FE outputs wrap to -2', 30000);
  fixture.mark('two physical maximum FE outputs produce Java int wraparound, not a clamp');
  await fixture.restart();
  const saved = await until(read, value => value.fe?.stored === -2 && value.fe?.capacity === -2, 'paired FE readback after restart', 30000);
  assert.equal(saved.fe.can_receive, true);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('frequency pairing and signed FE readback survive restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
