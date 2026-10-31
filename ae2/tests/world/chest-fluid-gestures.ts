/** ME Chest fluid actions and uncharged borrowed buckets use its physical cell. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';
const mode: Mode=process.argv.includes('--in-frame')?'in-frame':'server-thread';
const fixture=new WorldFixture('chest-fluid-gestures',mode);
const water=(view:any)=>view.slots?.find((slot:any)=>slot.slot>=300&&slot.slot<345&&slot.item==='minecraft:water_bucket');
const amount=(view:any,n:number)=>String(water(view)?.components??'').includes(`${n} mB`);
try {
  const start=await fixture.start(),[px,py,pz]=start.player.position;
  const x=Math.floor(px)+24,y=Math.ceil(py)+5,z=Math.floor(pz)+24,chest=`${x+1} ${y} ${z}`;
  await fixture.command(`tp @s ${x+1.5} ${y} ${z+2.5}`);
  for(const [dx,dz] of [[-2,-3],[-2,3],[4,-3],[4,3]] as const) await until(()=>fixture.raw(`block ${x+dx} ${y} ${z+dz}`),s=>s.loaded,'chest fixture chunk loaded',30000);
  await fixture.command(`fill ${x-2} ${y} ${z-3} ${x+4} ${y+4} ${z+3} minecraft:air`);
  await fixture.command(`fill ${x-2} ${y-1} ${z-3} ${x+4} ${y-1} ${z+3} minecraft:stone`);
  await fixture.command(`setblock ${x} ${y} ${z} ae2:creative_energy_cell`);await fixture.command(`setblock ${chest} ae2:chest`);
  await fixture.command(`item replace block ${chest} container.0 with ae2:fluid_storage_cell_1k[ae2:storage_cell_inv=[{"#t":"ae2:f",id:"minecraft:water","#":2000L},{"#t":"ae2:i",id:"minecraft:bucket","#":2L}]]`);
  await until(()=>fixture.ae(`grid status ${chest}`),s=>s.online&&s.powered,'powered ME Chest',30000);
  await fixture.command('clear @s'); await fixture.command('gamemode survival @s'); await fixture.replace('hotbar.0','minecraft:bucket'); await fixture.replace('hotbar.8','minecraft:stick');
  const open=async()=>{await fixture.select(8,'minecraft:stick');await fixture.command(`tp @s ${x+1.5} ${y} ${z+2.5}`);await fixture.raw(`interact ${chest} south`);return until(()=>fixture.raw('screen'),s=>water(s),'ME Chest water view',30000);};
  let view=await open(); assert(amount(view,2000)); await fixture.raw('click-slot 0');
  await until(()=>fixture.raw('inventory'),s=>s.carried?.item==='minecraft:bucket','empty bucket held');
  await fixture.raw(`click-slot ${water(view).slot} right`); await fixture.command(`data get block ${chest}`);
  assert.equal((await fixture.raw('inventory')).carried?.item,'minecraft:bucket'); assert(amount(await fixture.raw('screen'),2000));
  await fixture.raw(`click-slot ${water(view).slot}`); await until(()=>fixture.raw('inventory'),s=>s.carried?.item==='minecraft:water_bucket','left click fills');
  view=await until(()=>fixture.raw('screen'),s=>amount(s,1000),'1000 mB remains');
  await fixture.raw(`click-slot ${water(view).slot}`); await fixture.command(`data get block ${chest}`);
  assert.equal((await fixture.raw('inventory')).carried?.item,'minecraft:water_bucket'); assert(amount(await fixture.raw('screen'),1000));
  await fixture.raw(`click-slot ${water(view).slot} right`); await until(()=>fixture.raw('inventory'),s=>s.carried?.item==='minecraft:bucket','right click empties');
  await fixture.raw('click-slot 0'); await until(()=>fixture.raw('inventory'),s=>!s.carried&&s.hotbar?.[0]?.item==='minecraft:bucket','bucket returned');
  await fixture.replace('hotbar.0','minecraft:air'); view=await until(()=>fixture.raw('screen'),s=>amount(s,2000),'fluid restored');
  await fixture.raw(`click-slot ${water(view).slot}`); await until(()=>fixture.raw('inventory'),s=>s.carried?.item==='minecraft:water_bucket','foreign stored bucket filled');
  view=await until(()=>fixture.raw('screen'),s=>amount(s,1000),'borrowed bucket uses 1000 mB');
  await fixture.raw(`click-slot ${water(view).slot} right`); await until(()=>fixture.raw('inventory'),s=>s.carried?.item==='minecraft:bucket','borrowed bucket emptied');
  await fixture.raw('click-slot 0'); await until(()=>fixture.raw('inventory'),s=>!s.carried&&s.hotbar?.[0]?.item==='minecraft:bucket','first borrowed bucket returned');
  view=await until(()=>fixture.raw('screen'),s=>amount(s,2000),'fluid restored for shift-click');
  await fixture.raw(`click-slot ${water(view).slot} shift`); await until(()=>fixture.raw('inventory'),s=>!s.carried&&s.counts?.['minecraft:water_bucket']===1,'second borrowed bucket is filled into player inventory');
  await until(()=>fixture.raw('screen'),s=>amount(s,1000),'1000 mB remains'); await fixture.raw('close'); await fixture.restart();
  await until(()=>fixture.ae(`grid status ${chest}`),s=>s.online&&s.powered,'reloaded chest powered',30000); view=await open(); assert(amount(view,1000));
  const inventory=await fixture.raw('inventory'); assert.equal(inventory.counts?.['minecraft:bucket'],1); assert.equal(inventory.counts?.['minecraft:water_bucket'],1);
  const saved=JSON.stringify(await fixture.command(`data get block ${chest}`)); assert.doesNotMatch(saved,/minecraft:bucket/);
  fixture.assertHidden(await fixture.raw('status')); fixture.mark('ME Chest mouse direction and both storage-borrowed buckets preserve physical custody across restart'); fixture.report('passed');
} catch(error) { fixture.report('failed',error); throw error; }
finally {await fixture.close();}
