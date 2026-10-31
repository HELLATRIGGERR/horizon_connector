/** The QNB center's sided inventory and source-ordered hopper callbacks. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('quantum-hopper-inventory', mode);
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 5, y = Math.ceil(py) + 6, z = Math.floor(pz) + 5;
  const at = (dx: number, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const input = at(0, 1), output = at(0, -1), ringInput = at(1, 1), remote = at(12);
  const slot = async (pos: string, index = 0) => (await fixture.raw(`container ${pos}`)).slots?.[index];
  const paired = (count: number) => `{Slot:0b,id:"ae2:quantum_entangled_singularity",count:${count},components:{"ae2:entangled_singularity_id":42L}}`;
  const put = (pos: string, item: string) => fixture.command(`data merge block ${pos} {Items:[${item}]}`);
  const centerNbt = () => fixture.command(`data get block ${at(0)}`).then(v => JSON.stringify(v));
  await fixture.command(`fill ${at(-4, -3, -4)} ${at(15, -3, 4)} minecraft:stone`);
  await fixture.command(`fill ${at(-3, -2, -3)} ${at(14, 3, 3)} minecraft:air`);
  await fixture.command(`tp @s ${x + 2.5} ${y + 1} ${z - 2.5}`);
  for (const dx of [0, 10]) for (let a = -1; a <= 1; a++) for (let b = -1; b <= 1; b++) {
    const pos = at(dx + a, 0, b);
    await fixture.command(`setblock ${pos} ae2:${a === 0 && b === 0 ? 'quantum_link' : 'quantum_ring'}`);
    await fixture.ae(`grid status ${pos}`);
  }
  await fixture.command(`setblock ${at(-2)} ae2:creative_energy_cell`);
  await fixture.command(`setblock ${remote} ae2:drive`);
  // The second side uses the documented old native wrapper; the input hopper
  // below exercises the newly fixed real source component.
  await fixture.command(`item replace block ${at(10)} container.0 with ae2:quantum_entangled_singularity[minecraft:custom_data={"ae2:entangled_singularity_id":42L}]`);
  await until(() => fixture.ae(`grid status ${at(-2)}`), s => s.online && s.powered, 'first quantum frame finishes booting', 30000);
  await fixture.hold();
  await fixture.command(`setblock ${input} minecraft:hopper[facing=down]`);
  await fixture.command(`setblock ${ringInput} minecraft:hopper[facing=down]`);
  await fixture.command(`setblock ${output} minecraft:hopper[facing=down]`);
  await fixture.command(`setblock ${at(1, -1)} minecraft:redstone_block`);
  await put(input, '{Slot:0b,id:"minecraft:diamond",count:1}');
  await put(ringInput, paired(1));
  await fixture.stepServer(40);
  assert.equal((await slot(input))?.item, 'minecraft:diamond', 'invalid center input remains in the hopper');
  assert.equal((await slot(ringInput))?.item, 'ae2:quantum_entangled_singularity', 'the ring exposes no sided inventory');
  assert(!((await centerNbt()).includes('ae2:entangled_singularity_id')), 'invalid input did not enter the center');
  fixture.mark('hoppers reject an invalid center input and cannot insert into a ring');

  await put(input, paired(2));
  await fixture.stepServer(160);
  assert.equal((await slot(input))?.count, 1, 'the source pair is split at the one-item limit');
  assert.match(await centerNbt(), /ae2:entangled_singularity_id.*42/);
  assert((await fixture.ae(`grid status ${remote}`)).online, 'committed hopper insertion connects the remote drive');
  fixture.mark('one real source-component singularity enters the center and connects the remote grid');

  // Lock input before unlocking the output so the remaining singularity cannot
  // refill the center during the removal check.
  await fixture.command(`setblock ${at(0, 1, 1)} minecraft:redstone_block`);
  await fixture.command(`setblock ${at(1, -1)} minecraft:air`);
  await fixture.stepServer(160);
  assert.equal((await slot(output))?.item, 'ae2:quantum_entangled_singularity');
  assert(!((await centerNbt()).includes('ae2:entangled_singularity_id')));
  assert(!(await fixture.ae(`grid status ${remote}`)).online, 'committed extraction disconnects the remote drive');
  fixture.mark('extracting the center item commits the matching disconnection without losing either half');

  await fixture.hold(false);
  await fixture.restart();
  await fixture.hold();
  assert.equal((await slot(output))?.item, 'ae2:quantum_entangled_singularity');
  assert.equal((await slot(input))?.item, 'ae2:quantum_entangled_singularity');
  await fixture.command(`setblock ${at(1, -1)} minecraft:redstone_block`);
  await fixture.command(`setblock ${at(0, 1, 1)} minecraft:air`);
  await fixture.stepServer(200);
  assert((await fixture.ae(`grid status ${remote}`)).online, 'the other physical half reconnects after restart');
  assert.match(await centerNbt(), /ae2:entangled_singularity_id.*42/);
  assert.equal((await slot(output))?.item, 'ae2:quantum_entangled_singularity');
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('physical hopper custody and source-component reconnection survive restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
