/** Hoppers respect an item's actual max-stack component during merging. */
import assert from 'node:assert/strict';
import { WorldFixture, type Mode } from './harness';
const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('hopper-component-stack-limit', mode);
try {
  const start=await fixture.start(), [px,py,pz]=start.player.position;
  const x=Math.floor(px)+5, y=Math.ceil(py)+5, z=Math.floor(pz)+5;
  const chest=`${x} ${y} ${z}`, hopper=`${x} ${y+1} ${z}`;
  await fixture.command(`fill ${x-2} ${y-1} ${z-2} ${x+2} ${y+3} ${z+2} minecraft:air`);
  await fixture.command(`fill ${x-2} ${y-1} ${z-2} ${x+2} ${y-1} ${z+2} minecraft:stone`);
  await fixture.command(`setblock ${chest} minecraft:chest`);
  await fixture.command(`setblock ${hopper} minecraft:hopper[facing=down]`);
  await fixture.hold();
  const item=(count:number)=>`{Slot:0b,id:"minecraft:gold_ingot",count:${count},components:{"minecraft:max_stack_size":5}}`;
  await fixture.command(`data merge block ${chest} {Items:[${item(4)}]}`);
  await fixture.command(`data merge block ${hopper} {Items:[${item(2)}]}`);
  await fixture.stepServer(100);
  const contents=await fixture.raw(`container ${chest}`);
  assert.equal(contents.slots?.[0]?.item,'minecraft:gold_ingot');
  assert.equal(contents.slots?.[0]?.count,5,'the first physical stack stops at its component maximum');
  assert.equal(contents.slots?.[1]?.item,'minecraft:gold_ingot');
  assert.equal(contents.slots?.[1]?.count,1,'the remaining ingot occupies another physical slot');
  assert(!((await fixture.raw(`container ${hopper}`)).slots ?? []).some((item:any)=>item?.item==='minecraft:gold_ingot'));
  fixture.mark('the hopper fills a component-limited stack to five and places the remaining physical item in a second slot');
  await fixture.hold(false); await fixture.restart();
  const saved=await fixture.raw(`container ${chest}`);
  assert.equal(saved.slots?.[0]?.count,5); assert.equal(saved.slots?.[1]?.count,1);
  const nbt=JSON.stringify(await fixture.command(`data get block ${chest}`));
  assert.match(nbt,/minecraft:max_stack_size/);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('both exact component-bearing physical stacks survive restart');
  fixture.report('passed');
} catch(error) { fixture.report('failed',error); throw error; }
finally { await fixture.close(); }
