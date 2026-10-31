/** Hopper minecarts keep component-limited physical item entities in bounded slots. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';
const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('hopper-minecart-component-limit', mode);
try {
  const start = await fixture.start(), [px,py,pz] = start.player.position;
  const x = Math.floor(px)+32, y = Math.ceil(py)+5, z = Math.floor(pz)+32;
  const at = (dx: number, dy=0, dz=0) => `${x+dx} ${y+dy} ${z+dz}`;
  await fixture.command(`tp @s ${x+.5} ${y} ${z+.5}`);
  for (const dx of [-2,10]) for (const dz of [-2,2]) await until(
    async () => (await fixture.command(`execute if loaded ${at(dx,0,dz)}`)).chat ?? [],
    lines => lines.includes('Test passed'), 'loaded minecart corner', 30000);
  await fixture.command(`fill ${at(-2,0,-2)} ${at(10,3,2)} minecraft:air`);
  await fixture.command(`fill ${at(-2,-1,-2)} ${at(10,-1,2)} minecraft:stone`);
  await fixture.command(`setblock ${at(2)} minecraft:rail`);
  await fixture.command(`tp @s ${x+9.5} ${y} ${z+.5}`);
  await fixture.hold();
  const components = '{"minecraft:max_stack_size":16,"minecraft:custom_data":{variant:"cart-max"}}';
  await fixture.command(`summon minecraft:hopper_minecart ${x+2.5} ${y+.1} ${z+.5} {Items:[{Slot:0b,id:"minecraft:stone",count:15,components:${components}}]}`);
  await fixture.command(`summon minecraft:item ${x+2.5} ${y+.3} ${z+.5} {Item:{id:"minecraft:stone",count:16,components:${components}},PickupDelay:0s}`);
  await fixture.stepServer(10);
  const read = async () => ((await fixture.command('data get entity @e[type=minecraft:hopper_minecart,limit=1,sort=nearest] Items')).chat ?? []).join('\n');
  const counts = (text: string) => [...text.matchAll(/count:\s*(\d+)/g)].map(match => Number(match[1]));
  const before = await read();
  assert.deepEqual(counts(before).sort((a,b)=>a-b), [15,16], `cart should retain two bounded stacks: ${before}`);
  assert.equal((before.match(/cart-max/g) ?? []).length, 2);
  fixture.mark('the hopper minecart fills its max-16 slot and stores the remaining 15 items separately');
  await fixture.hold(false); await fixture.restart();
  const after = await until(read, text => counts(text).reduce((sum,n)=>sum+n,0)===31, 'minecart item stacks reload');
  assert.deepEqual(counts(after).sort((a,b)=>a-b), [15,16]);
  assert.equal((after.match(/cart-max/g) ?? []).length, 2);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('both exact component-bearing minecart stacks survive restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
