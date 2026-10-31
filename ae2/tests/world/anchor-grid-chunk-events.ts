/** Spatial Anchor reacts to the last in-world node leaving a grid chunk. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('anchor-grid-chunk-events', mode);
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px / 16) * 16, z = Math.floor(pz / 16) * 16, y = Math.ceil(py) + 5;
  const pos = (dx: number, dz = 6, dy = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const anchor = pos(15), power = pos(14), bridge = pos(15, 7), first = pos(16), second = pos(16, 7);
  const seed = async (where: string) => {
    const reply = await fixture.command(`ae2 grid status ${where}`);
    assert((reply.chat ?? []).some((line: string) => line.startsWith('AE2 {') || line === 'AE2 missing graph node'),
      `AE2 node initialization failed: ${JSON.stringify(reply)}`);
  };
  await fixture.command(`fill ${pos(11, 3, -2)} ${pos(19, 9, -2)} minecraft:stone`);
  await fixture.command(`fill ${pos(11, 3, -1)} ${pos(19, 9, 3)} minecraft:air`);
  for (const [where, id] of [[anchor, 'ae2:spatial_anchor'], [power, 'ae2:creative_energy_cell'],
    [bridge, 'ae2:energy_cell'], [first, 'ae2:drive'], [second, 'ae2:drive']]) {
    await fixture.command(`setblock ${where} ${id}`); await seed(where);
  }
  await until(() => fixture.ae(`anchor status ${anchor}`), s => s.count === 2 && s.idle === 83, 'anchor owns both physical grid chunks', 30000);
  await fixture.hold();
  const before = await fixture.ae(`anchor status ${anchor}`);
  await fixture.command(`setblock ${first} minecraft:air`);
  await fixture.ae(`grid status ${anchor}`);
  const oneLeft = await fixture.ae(`anchor status ${anchor}`);
  assert.equal(oneLeft.server_tick, before.server_tick, 'no server tick has run during the membership change');
  assert.equal(oneLeft.count, 2, 'the other in-world node keeps the second chunk registered');
  assert.equal(oneLeft.idle, 83);
  fixture.mark('removing one of two physical nodes preserves the shared chunk ticket');

  await fixture.command(`setblock ${second} minecraft:air`);
  await fixture.ae(`grid status ${anchor}`);
  const lastRemoved = await fixture.ae(`anchor status ${anchor}`);
  assert.equal(lastRemoved.server_tick, before.server_tick, 'the owner callback is checked before the next tick');
  assert.equal(lastRemoved.count, 1, 'last node removal immediately releases the chunk');
  assert.equal(lastRemoved.idle, 81, 'the source owner callback immediately updates idle power');
  fixture.mark('the last physical node releases its ticket and updates idle before another server tick');

  await fixture.hold(false); await fixture.restart();
  await until(() => fixture.ae(`anchor status ${anchor}`), s => s.count === 1 && s.idle === 81, 'only the retained owner ticket survives restart', 30000);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('the physical anchor retains only the remaining ticket after restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
