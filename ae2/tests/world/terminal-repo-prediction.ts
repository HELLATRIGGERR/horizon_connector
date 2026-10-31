/** Source repository slots never act as vanilla physical client slots. */
import assert from 'node:assert/strict';
import { WorldFixture, until } from './harness';
const fixture=new WorldFixture('terminal-repo-prediction','server-thread');
const entry=(view:any,id:string)=>view.slots?.find((slot:any)=>slot.slot>=300&&slot.slot<345&&slot.item===id);
const slot=(view:any,id:number)=>view.slots?.find((slot:any)=>slot.slot===id);
try {
  const start=await fixture.start(),[px,py,pz]=start.player.position;
  const x=Math.floor(px)+24,y=Math.ceil(py)+5,z=Math.floor(pz)+24,chest=`${x+1} ${y} ${z}`;
  await fixture.command(`tp @s ${x+1.5} ${y} ${z+2.5}`);
  for(const [dx,dz] of [[-2,-3],[-2,3],[4,-3],[4,3]] as const) await until(()=>fixture.raw(`block ${x+dx} ${y} ${z+dz}`),s=>s.loaded,'fixture chunk loaded',30000);
  await fixture.command(`fill ${x-2} ${y} ${z-3} ${x+4} ${y+4} ${z+3} minecraft:air`);
  await fixture.command(`fill ${x-2} ${y-1} ${z-3} ${x+4} ${y-1} ${z+3} minecraft:stone`);
  await fixture.command(`setblock ${x} ${y} ${z} ae2:creative_energy_cell`);await fixture.command(`setblock ${chest} ae2:chest`);
  await fixture.command(`item replace block ${chest} container.0 with ae2:item_storage_cell_1k[ae2:storage_cell_inv=[{"#t":"ae2:i",id:"minecraft:iron_ingot","#":3L}]]`);
  await until(()=>fixture.ae(`grid status ${chest}`),s=>s.online&&s.powered,'powered ME Chest',30000);
  await fixture.command('clear @s');await fixture.command('gamemode survival @s');await fixture.replace('hotbar.0','minecraft:gold_ingot',2);await fixture.replace('hotbar.8','minecraft:stick');
  await fixture.select(8,'minecraft:stick');await fixture.command(`tp @s ${x+1.5} ${y} ${z+2.5}`);await fixture.raw(`interact ${chest} south`);
  await until(()=>fixture.raw('screen'),s=>entry(s,'minecraft:iron_ingot')&&!slot(s,310)?.item,'terminal with an empty repository slot',30000);
  await fixture.raw('click-slot 0');await fixture.command(`data get block ${chest}`);
  await until(()=>fixture.raw('inventory'),s=>s.carried?.item==='minecraft:gold_ingot'&&s.carried.count===2,'physical player pickup processed');
  await fixture.hold();
  try {
    await fixture.raw('click-slot 310');
    const pending=await fixture.raw('screen');
    assert.equal(pending.carried?.item,'minecraft:gold_ingot','client keeps the real carried item until server response');
    assert.equal(pending.carried?.count,2);assert.equal(slot(pending,310)?.item,null,'client does not predict an item in the virtual slot');
  } finally {await fixture.hold(false);}
  await until(async()=>JSON.stringify(await fixture.command(`data get block ${chest}`)),text=>text.includes('minecraft:gold_ingot'),'authoritative physical cell receives the gold');
  let view=await until(()=>fixture.raw('screen'),s=>entry(s,'minecraft:gold_ingot')&&!s.carried,'sorted server view shows deposited gold');
  await fixture.raw(`click-slot ${entry(view,'minecraft:gold_ingot').slot}`);
  await until(async()=>JSON.stringify(await fixture.command(`data get block ${chest}`)),text=>!text.includes('minecraft:gold_ingot'),'authoritative cell no longer contains recovered gold');
  await until(()=>fixture.raw('inventory'),s=>s.carried?.item==='minecraft:gold_ingot'&&s.carried.count===2,'real stack returns to cursor');
  await fixture.raw('close');await fixture.restart();assert.equal((await fixture.raw('inventory')).counts?.['minecraft:gold_ingot'],2);
  fixture.assertHidden(await fixture.raw('status'));fixture.mark('repository click waits for the authoritative sorted view and preserves exact gold custody');fixture.report('passed');
} catch(error) {fixture.report('failed',error);throw error;}
finally {await fixture.close();}
