/** In-frame machine menus use the same physical transactions as server menus. */
import assert from 'node:assert/strict';
import { WorldFixture, until } from './harness';

const fixture = new WorldFixture('inframe-machine-menus', 'in-frame');
const slot = (view: any, index: number) => view.slots?.find((entry: any) => entry.slot === index);
const text = (reply: any) => (reply.chat ?? []).join('\n');
try {
  const start = await fixture.start(); const [px, py, pz] = start.player.position;
  const x = Math.floor(px) + 24, y = Math.ceil(py) + 5, z = Math.floor(pz) + 24;
  const at = (dx: number) => `${x + dx} ${y} ${z}`, inscriber=at(0), bench=at(4), iface=at(8), donor=at(10), drive=at(12), condenser=at(16), io=at(20);
  await fixture.command(`tp @s ${x+10.5} ${y} ${z+2.5}`);
  for (const [dx,dz] of [[-2,-3],[-2,3],[22,-3],[22,3]] as const) {
    await until(() => fixture.raw(`block ${x+dx} ${y} ${z+dz}`),value => value.loaded, 'machine fixture corner chunk loaded',30000);
  }
  await fixture.command(`fill ${x - 2} ${y} ${z - 3} ${x + 22} ${y + 4} ${z + 3} minecraft:air`);
  await fixture.command(`fill ${x - 2} ${y - 1} ${z - 3} ${x + 22} ${y - 1} ${z + 3} minecraft:stone`);
  for (const [target,id] of [[inscriber,'ae2:inscriber'],[bench,'ae2:cell_workbench'],[iface,'ae2:cable_bus'],[donor,'ae2:cable_bus'],[drive,'ae2:drive'],[condenser,'ae2:condenser'],[io,'ae2:io_port']] as const) await fixture.command(`setblock ${target} ${id}`);
  await fixture.command('clear @s'); await fixture.replace('hotbar.0','ae2:cable_interface'); await fixture.select(0,'ae2:cable_interface');
  assert.equal((await fixture.ae(`grid part ${iface} south`)).attached,true);
  assert.equal((await fixture.ae(`grid part ${donor} south`)).attached,true);
  await fixture.command('gamemode survival @s'); await fixture.replace('hotbar.0','minecraft:stick');
  await fixture.replace('hotbar.1','ae2:silicon',3); await fixture.replace('hotbar.2','ae2:item_storage_cell_1k');
  await fixture.replace('hotbar.3','minecraft:diamond',3); await fixture.replace('hotbar.4','ae2:item_storage_cell_4k');
  await fixture.replace('hotbar.5','ae2:cell_component_1k'); await fixture.replace('hotbar.6','ae2:item_storage_cell_16k');
  await fixture.replace('weapon.offhand','minecraft:gold_ingot');
  const open=async(target:string,dx:number)=>{
    await fixture.select(0,'minecraft:stick'); await fixture.command(`tp @s ${x+dx+.5} ${y} ${z+2.5}`); await fixture.raw(`interact ${target} south`);
    return until(() => fixture.raw('screen'),view => view.screen==='container' && slot(view,300) && slot(view,326),'in-frame physical machine menu');
  };
  const put=async(player:number,dest:number,item:string)=>{
    await fixture.raw(`click-slot ${player}`); await fixture.raw(`click-slot ${dest}`);
    await until(() => fixture.raw('screen'),view => slot(view,dest)?.item===item && !view.carried,`physical ${item} inserted`);
  };
  await open(inscriber,0); await put(1,302,'ae2:silicon'); await fixture.raw('click-slot 324');
  assert.match(text(await fixture.command(`data get block ${inscriber} ae2_lab_config`)),/auto_export/);
  await fixture.raw('close'); await fixture.replace('hotbar.1','minecraft:emerald');
  fixture.mark('Inscriber menu transfers the real input and routes a setting');
  await open(bench,4); await put(2,300,'ae2:item_storage_cell_1k');
  await fixture.raw('click-slot 3');
  let widgets=await fixture.raw('ui'); let ghost=widgets.widgets.find((entry:any)=>entry.id==='slot-305'); assert(ghost,'Workbench filter hit target');
  await fixture.raw(`click-at ${ghost.bounds[0]+8} ${ghost.bounds[1]+8}`);
  await until(() => fixture.raw('screen'),v=>slot(v,305)?.item==='minecraft:diamond' && v.carried?.count===3,'Workbench type filter with real diamond retained');
  await fixture.raw('click-slot 3'); await until(() => fixture.raw('inventory'),v=>!v.carried && v.counts?.['minecraft:diamond']===3,'Workbench cursor returned');
  await fixture.raw('key 2 tap');
  await until(() => fixture.raw('screen'),v=>slot(v,305)?.item==='minecraft:diamond' && !v.carried,'Workbench unlimited fake slot rejects physical hotbar swap');
  await fixture.raw('key f tap'); await until(() => fixture.raw('screen'),v=>!slot(v,305)?.item && !v.carried,'Workbench offhand key clears filter');
  await fixture.raw('click-slot 3'); await fixture.raw('click-slot 305 shift');
  await until(() => fixture.raw('screen'),v=>slot(v,305)?.item==='minecraft:diamond' && v.carried?.count===3,'Workbench shift fake action retains the template');
  await fixture.raw('click-slot 3');
  await fixture.raw('close'); fixture.mark('Workbench stores its physical cell and filter without consuming the template');
  await open(iface,8); await fixture.raw('click-slot 3');
  widgets=await fixture.raw('ui'); ghost=widgets.widgets.find((entry:any)=>entry.id==='slot-309'); assert(ghost,'Interface filter hit target');
  await fixture.raw(`click-at ${ghost.bounds[0]+8} ${ghost.bounds[1]+8}`);
  await until(() => fixture.raw('screen'),v=>slot(v,309)?.item==='minecraft:diamond' && slot(v,309)?.count===3 && v.carried?.count===3,'Interface part filter and cursor');
  await fixture.raw('key 2 tap'); await fixture.raw('key f tap');
  await until(() => fixture.raw('screen'),v=>slot(v,309)?.count===3 && v.carried?.count===3,'Interface keys keep the held template');
  await fixture.raw('click-slot 3'); await fixture.raw('key 2 tap');
  await until(() => fixture.raw('screen'),v=>slot(v,309)?.count===2 && !v.carried,'Interface hotbar key decrements source 64-limit filter');
  await fixture.raw('key f tap'); await until(() => fixture.raw('screen'),v=>!slot(v,309)?.item && !v.carried,'Interface offhand key clears filter');
  const equipment=await fixture.raw('inventory');assert.equal(equipment.hotbar?.[1]?.item,'minecraft:emerald');assert.equal(equipment.offhand?.item,'minecraft:gold_ingot');
  await fixture.raw('click-slot 3'); await fixture.raw('click-slot 309'); await fixture.raw('click-slot 3');
  await fixture.raw('click-slot 309 shift');
  await until(() => fixture.raw('screen'),v=>String(slot(v,308)?.components ?? '').includes('Set stock'),'local stock editor');
  await fixture.raw('type =2*3'); await fixture.raw('ui-key Enter');
  await until(() => fixture.raw('screen'),v=>slot(v,309)?.count===6,'local stock expression committed'); await fixture.raw('close');
  fixture.mark('Interface face configures a ghost and uses the owned stock editor');
  await open(drive,12); await put(4,300,'ae2:item_storage_cell_4k'); await fixture.raw('click-slot 326');
  await until(() => fixture.raw('screen'),v=>String(slot(v,300)?.components ?? '').includes('Priority +1'),'local drive priority editor');
  await fixture.raw('type =2*3'); await fixture.raw('ui-key Enter');
  assert.match(text(await fixture.command(`data get block ${drive} ae2_lab_config`)),/priority:\s*6\b/); await fixture.raw('close');
  await open(condenser,16); await put(5,302,'ae2:cell_component_1k'); await fixture.raw('close');
  await open(io,20); await put(6,300,'ae2:item_storage_cell_16k'); await fixture.raw('close');
  fixture.mark('Drive, Condenser and IO Port use real owned slots; Drive priority is applied');
  await fixture.restart();
  assert.match(text(await fixture.command(`data get block ${inscriber}`)),/ae2:silicon/);
  assert.match(text(await fixture.command(`data get block ${bench}`)),/ae2:item_storage_cell_1k/);
  assert.match(text(await fixture.command(`data get block ${iface} ae2_lab_config.parts`)),/minecraft:diamond/);
  assert.match(text(await fixture.command(`data get block ${drive}`)),/ae2:item_storage_cell_4k/);
  assert.match(text(await fixture.command(`data get block ${condenser}`)),/ae2:cell_component_1k/);
  assert.match(text(await fixture.command(`data get block ${io}`)),/ae2:item_storage_cell_16k/);
  assert.equal((await fixture.raw('inventory')).counts?.['minecraft:diamond'],3);
  fixture.mark('machine contents, settings and player template survive restart');
  await open(iface,8); await fixture.raw('click-slot 3');
  await until(() => fixture.raw('inventory'),v=>v.carried?.item==='minecraft:diamond' && v.carried.count===3,'real cursor before Interface replacement');
  await fixture.command(`data modify block ${iface} ae2_lab_config.parts[{face:"south"}] set from block ${donor} ae2_lab_config.parts[{face:"south"}]`);
  await until(() => fixture.raw('screen'),v=>v.screen==='playing','replaced in-frame Interface closes its old session');
  const custody=await fixture.raw('inventory');assert.equal(custody.counts?.['minecraft:diamond'],3);assert(!custody.carried);
  assert(!text(await fixture.command(`data get block ${iface} ae2_lab_config.parts`)).includes('minecraft:diamond'));
  fixture.assertHidden(await fixture.raw('status'));fixture.mark('replacement closes the face-owned menu and returns the exact cursor');fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
