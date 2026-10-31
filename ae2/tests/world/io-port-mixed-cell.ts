/** An I/O Port drains both stored key channels without discarding a blocked key. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';
const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture=new WorldFixture('io-port-mixed-cell',mode);
const count=(status:any,id:string)=>status.items?.find((entry:any)=>entry.item===id)?.count??0;
try {
  const start=await fixture.start(), [px,py,pz]=start.player.position;
  const x=Math.floor(px)+32,y=Math.ceil(py)+5,z=Math.floor(pz)+32;
  const at=(dx:number,dy=0,dz=0)=>`${x+dx} ${y+dy} ${z+dz}`;
  await fixture.command(`tp @s ${x+.5} ${y} ${z+.5}`);
  for(const dx of [-2,6]) for(const dz of [-2,2]) await until(async()=>(await fixture.command(`execute if loaded ${at(dx,0,dz)}`)).chat??[],lines=>lines.includes('Test passed'),'loaded I/O corner',30000);
  await fixture.command(`fill ${at(-2,0,-2)} ${at(6,4,2)} minecraft:air`);
  await fixture.command(`fill ${at(-2,-1,-2)} ${at(6,-1,2)} minecraft:stone`);
  const drive=at(1),port=at(2);
  await fixture.command(`setblock ${at(0)} ae2:creative_energy_cell`);
  await fixture.command(`setblock ${drive} ae2:drive`);
  await fixture.command(`setblock ${port} ae2:io_port`);
  await fixture.hold();
  await fixture.command(`item replace block ${drive} container.0 with ae2:item_storage_cell_1k`);
  const mixed='ae2:fluid_storage_cell_1k[ae2:storage_cell_inv=[{"#t":"ae2:f",id:"minecraft:water","#":1000L},{"#t":"ae2:i",id:"removed:ore","#":3L}]]';
  await fixture.command(`item replace block ${port} container.0 with ${mixed}`);
  await fixture.hold(false);
  const status=()=>fixture.ae(`grid status ${drive}`), fluids=()=>fixture.ae(`fluid grid status ${drive}`);
  await until(status,view=>view.online&&view.powered&&count(view,'ae2:missing_content')===3,'I/O transfers the recoverable item past blocked water',30000);
  assert(!(await fluids()).entries?.some((entry:any)=>entry.fluid==='minecraft:water'));
  let raw=JSON.stringify(await fixture.command(`data get block ${port}`));assert.match(raw,/minecraft:water/);
  fixture.mark('a blocked fluid does not prevent the I/O Port from transferring its recoverable item');
  await fixture.restart();
  await until(status,view=>view.online&&view.powered&&count(view,'ae2:missing_content')===3,'transferred item survives restart',30000);
  raw=JSON.stringify(await fixture.command(`data get block ${port}`));assert.match(raw,/minecraft:water/);
  await fixture.command(`item replace block ${drive} container.1 with ae2:fluid_storage_cell_1k`);
  await until(fluids,view=>view.entries?.some((entry:any)=>entry.fluid==='minecraft:water'&&entry.amount_mb===1000),'I/O transfers retained water once space exists',30000);
  await until(()=>fixture.command(`data get block ${port}`),reply=>/Slot:\s*6b/.test(JSON.stringify(reply)),'empty source cell reaches I/O output',30000);
  await fixture.restart();
  await until(status,view=>view.online&&view.powered&&count(view,'ae2:missing_content')===3,'item custody after second restart',30000);
  assert((await fluids()).entries.some((entry:any)=>entry.fluid==='minecraft:water'&&entry.amount_mb===1000));
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('retained water, the recovered item and the empty physical cell survive restart');
  fixture.report('passed');
} catch(error) { fixture.report('failed',error); throw error; }
finally { await fixture.close(); }
