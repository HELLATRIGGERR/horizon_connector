/** A fluid cell exposes and preserves source Missing Content across channels. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';
const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('fluid-cell-missing-content', mode);
const itemCount = (status: any, id: string) => status.items?.find((entry: any) => entry.item === id)?.count ?? 0;
try {
  const start = await fixture.start(), [px,py,pz] = start.player.position;
  const x=Math.floor(px)+32, y=Math.ceil(py)+5, z=Math.floor(pz)+32;
  const at=(dx:number,dy=0,dz=0)=>`${x+dx} ${y+dy} ${z+dz}`;
  await fixture.command(`tp @s ${x+.5} ${y} ${z+.5}`);
  for(const dx of [-2,6]) for(const dz of [-2,2]) await until(
    async()=>(await fixture.command(`execute if loaded ${at(dx,0,dz)}`)).chat??[],
    lines=>lines.includes('Test passed'),'loaded cell-test corner',30000);
  await fixture.command(`fill ${at(-2,0,-2)} ${at(6,4,2)} minecraft:air`);
  await fixture.command(`fill ${at(-2,-1,-2)} ${at(6,-1,2)} minecraft:stone`);
  const drive=at(1);
  await fixture.command(`setblock ${at(0)} ae2:creative_energy_cell`);
  await fixture.command(`setblock ${drive} ae2:drive`);
  const cell='ae2:fluid_storage_cell_1k[ae2:storage_cell_inv=[{"#t":"ae2:f",id:"minecraft:water","#":1000L},{"#t":"ae2:i",id:"removed:ore","#":3L}]]';
  await fixture.command(`item replace block ${drive} container.0 with ${cell}`);
  await fixture.command('clear @s');
  const status=()=>fixture.ae(`grid status ${drive}`);
  let view=await until(status,value=>value.online&&value.powered&&itemCount(value,'ae2:missing_content')===3,
    'the fluid cell exposes its recoverable Missing Content item',30000);
  const fluids=()=>fixture.ae(`fluid grid status ${drive}`);
  assert((await fluids()).entries?.some((entry:any)=>entry.fluid==='minecraft:water'&&entry.amount_mb===1000));
  const index=view.items.findIndex((entry:any)=>entry.item==='ae2:missing_content');
  assert(index>=0);assert.equal((await fixture.ae(`grid withdraw ${drive} ${index} 2`)).extracted,2);
  await until(()=>fixture.raw('inventory'),inventory=>inventory.counts?.['ae2:missing_content']===2,'two recovered physical placeholders reach the player');
  view=await status();assert.equal(itemCount(view,'ae2:missing_content'),1);
  assert((await fluids()).entries.some((entry:any)=>entry.fluid==='minecraft:water'&&entry.amount_mb===1000));
  const saved=JSON.stringify(await fixture.command(`data get block ${drive}`));
  assert.match(saved,/removed:ore/);assert.match(saved,/minecraft:water/);
  fixture.mark('the fluid cell exposes and partially extracts the item placeholder while retaining its water');
  await fixture.restart();
  view=await until(status,value=>value.online&&value.powered&&itemCount(value,'ae2:missing_content')===1,'mixed cell reloads',30000);
  assert((await fluids()).entries.some((entry:any)=>entry.fluid==='minecraft:water'&&entry.amount_mb===1000));
  assert.equal((await fixture.raw('inventory')).counts?.['ae2:missing_content'],2);
  const after=JSON.stringify(await fixture.command(`data get block ${drive}`));assert.match(after,/removed:ore/);assert.match(after,/minecraft:water/);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('both channels and the original unavailable key survive physical extraction and restart');
  fixture.report('passed');
} catch(error) { fixture.report('failed',error); throw error; }
finally { await fixture.close(); }
