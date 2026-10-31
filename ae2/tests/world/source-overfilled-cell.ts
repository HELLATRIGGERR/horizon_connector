/** Source cells retain over-capacity contents and allow extraction. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';
const mode:Mode=process.argv.includes('--in-frame')?'in-frame':'server-thread';
const fixture=new WorldFixture('source-overfilled-cell',mode);
const count=(view:any)=>view.items?.find((entry:any)=>entry.item==='minecraft:stone')?.count??0;
try {
  const start=await fixture.start(),[px,py,pz]=start.player.position;
  const x=Math.floor(px)+32,y=Math.ceil(py)+5,z=Math.floor(pz)+32;
  const at=(dx:number,dy=0,dz=0)=>`${x+dx} ${y+dy} ${z+dz}`;
  await fixture.command(`tp @s ${x+.5} ${y} ${z+.5}`);
  for(const dx of [-2,4]) for(const dz of [-2,2]) await until(async()=>(await fixture.command(`execute if loaded ${at(dx,0,dz)}`)).chat??[],lines=>lines.includes('Test passed'),'loaded overfilled-cell corner',30000);
  await fixture.command(`fill ${at(-2,0,-2)} ${at(4,3,2)} minecraft:air`);
  await fixture.command(`fill ${at(-2,-1,-2)} ${at(4,-1,2)} minecraft:stone`);
  const drive=at(1);await fixture.command(`setblock ${at(0)} ae2:creative_energy_cell`);await fixture.command(`setblock ${drive} ae2:drive`);
  const cell='ae2:item_storage_cell_1k[ae2:storage_cell_inv=[{"#t":"ae2:i",id:"minecraft:stone","#":9000L}]]';
  await fixture.command(`item replace block ${drive} container.0 with ${cell}`);await fixture.command('clear @s');
  const status=()=>fixture.ae(`grid status ${drive}`);
  const view=await until(status,value=>value.online&&value.powered&&count(value)===9000,'overfilled source contents are visible',30000);
  const index=view.items.findIndex((entry:any)=>entry.item==='minecraft:stone');assert(index>=0);
  assert.equal((await fixture.ae(`grid withdraw ${drive} ${index} 64`)).extracted,64);
  const inventory=await until(()=>fixture.raw('inventory'),value=>value.counts?.['minecraft:stone']===64,'64 physical items are recovered');
  const slot=inventory.hotbar.findIndex((entry:any)=>entry?.item==='minecraft:stone');assert(slot>=0);
  await fixture.select(slot,'minecraft:stone');assert.equal((await fixture.ae(`grid deposit ${drive}`)).inserted,0);
  assert.equal(count(await status()),8936);fixture.mark('the overfilled cell exposes its contents, allows extraction and refuses additional insertion');
  await fixture.restart();
  await until(status,value=>value.online&&value.powered&&count(value)===8936,'overfilled cell survives restart',30000);
  assert.equal((await fixture.raw('inventory')).counts?.['minecraft:stone'],64);
  fixture.assertHidden(await fixture.raw('status'));fixture.mark('the 8,936 stored and 64 recovered items survive restart');fixture.report('passed');
}catch(error){fixture.report('failed',error);throw error;}finally{await fixture.close();}
