/** Locally generate the source-derived AE2 overlay. Not gameplay parity. */
import { copyFileSync, cpSync, existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { execFileSync } from 'node:child_process';
import { syncGuideAssets } from './guidebook/sync-guide.ts';
import { requirePinnedReference } from './reference.ts';

const root = resolve(import.meta.dir, '../..');
const reference = resolve(process.env.AE2_REFERENCE_DIR ?? join(root, 'ae2-reference'));
const content = process.env.AE2_CONTENT_DIR ?? join(root, 'content/ae2');
const assets = join(content, 'assets/ae2');
const coverage = JSON.parse(readFileSync(join(import.meta.dir, 'ae2-content-catalog.json'), 'utf8'));
const expected = 'db17504a86128fdf3dae31f5fb7a112a646e0b93';
if (coverage.reference.commit !== expected) throw new Error('Unexpected AE2 catalog revision');
requirePinnedReference(reference);
const vanillaResources = resolve(process.env.AE2_VANILLA_RESOURCES ?? join(root, '.local/resources/26.3'));
if (!existsSync(join(vanillaResources, 'assets/minecraft/models'))) throw new Error('Prepare your own Minecraft 26.3 resources first, or set AE2_VANILLA_RESOURCES.');
for (const path of ['models/item/horizon_static','models/block/horizon_static']) rmSync(join(assets,path), {recursive:true,force:true});
const report: any = { source: coverage.reference, generated_at: new Date().toISOString(), scope: 'Static catalog projection, not a runtime registry dump or gameplay implementation.', blocks: [], items: [], excluded: [], errors: [], unsupported_model_loaders: [], limitations: [ 'Every block uses minecraft:iron_block behavior, cube collision and no AE2 blockstate properties.', 'Static source item model used as block model where resolvable. Animated states, orientations, cables, cell contents and dynamic block entities are not reproduced.', 'Custom loaders and procedural models use explicit static texture fallback. Multipart item registrations are not placed multipart blocks.', 'Item templates are generic; item mechanics, recipes, upgrades, max stack sizes and gameplay are not generally reproduced.', 'Registry coverage comes from static source, not a confirmed game runtime dump.' ] };
function put(path: string, value: unknown) { mkdirSync(dirname(path), { recursive: true }); writeFileSync(path, JSON.stringify(value, null, 2) + '\n'); }
for (const type of ['models', 'textures', 'blockstates', 'lang']) {
  for (const sub of ['generated', 'main']) {
    const src = join(reference, 'src', sub, 'resources/assets/ae2', type);
    if (existsSync(src)) { mkdirSync(join(assets,type), {recursive:true}); cpSync(src, join(assets,type), {recursive:true,force:true}); }
  }
}
// Gameplay data is independent of the visual registry catalog. Preserve the
// upstream namespace and nested paths so the host discovers supported recipes.
let recipeFiles=0, tagFiles=0, lootFiles=0,damageTypeFiles=0,advancementFiles=0;
const omittedNamespaces=new Set<string>();
rmSync(join(content,'data/curios'),{recursive:true,force:true});
for(const sub of ['main','generated']){
 const base=join(reference,'src',sub,'resources/data');
 if(!existsSync(base))continue;
 for(const namespace of readdirSync(base))for(const type of ['recipe','tags','loot_table','damage_type','advancement']){
  if(!['ae2','minecraft','c'].includes(namespace)){omittedNamespaces.add(namespace);continue;}
  if((type==='loot_table' || type==='advancement') && namespace!=='ae2')continue;
  const src=join(base,namespace,type);if(!existsSync(src))continue;
  for(const p of walk(src))if(p.endsWith('.json')){const dst=join(content,'data',namespace,type,p.slice(src.length+1));mkdirSync(dirname(dst),{recursive:true});writeFileSync(dst,readFileSync(p));if(type==='recipe')recipeFiles++;else if(type==='tags')tagFiles++;else if(type==='damage_type')damageTypeFiles++;else if(type==='advancement')advancementFiles++;else lootFiles++;}
 }
}
// Native structure extension uses these pinned source declarations directly.
for(const type of ['structure','structure_set']) {
 const src=join(reference,'src/generated/resources/data/ae2/worldgen',type,'meteorite.json');
 const dst=join(content,'data/ae2/worldgen',type,'meteorite.json');
 mkdirSync(dirname(dst),{recursive:true});writeFileSync(dst,readFileSync(src));
}
// Spatial storage is a real extra dimension, with the upstream dimension type.
const spatialType=join(reference,'src/generated/resources/data/ae2/dimension_type/spatial_storage.json');
put(join(content,'data/ae2/dimension_type/spatial_storage.json'),JSON.parse(readFileSync(spatialType,'utf8')));
// Adapt the pinned 1.21 biome to the 26.3 biome/attribute schema used by Horizon.
const biome=JSON.parse(readFileSync(join(reference,'src/generated/resources/data/ae2/worldgen/biome/spatial_storage.json'),'utf8'));
const hex=(n:number)=>'#'+n.toString(16).padStart(6,'0');
const adaptedBiome={temperature:biome.temperature,downfall:biome.downfall,has_precipitation:biome.has_precipitation,
  carvers:[],features:biome.features,effects:{water_color:biome.effects.water_color},attributes:{
    'minecraft:visual/sky_color':hex(biome.effects.sky_color),'minecraft:visual/fog_color':hex(biome.effects.fog_color),
    'minecraft:visual/water_fog_color':hex(biome.effects.water_fog_color),
    'minecraft:gameplay/creature_world_gen_spawn_probability':biome.creature_spawn_probability,
    'minecraft:gameplay/natural_mob_spawns':{spawns_by_category:biome.spawners,spawn_costs:biome.spawn_costs}}};
const biomePath=join(content,'data/ae2/worldgen/biome/spatial_storage.json');mkdirSync(dirname(biomePath),{recursive:true});
writeFileSync(biomePath,JSON.stringify(adaptedBiome,null,2).replace(/("minecraft:gameplay\/creature_world_gen_spawn_probability": )0(?=,)/,(_,prefix)=>prefix+'0.0')+'\n');
// AE2 depends on NeoForge's conventional tags. Read that dependency from the
// pinned NeoForge JAR; merge rather than erase AE2 additions.
const neoForgeJar=process.env.AE2_NEOFORGE_JAR ?? join(root,'.local/downloads/neoforge-21.1.259-universal.jar');
if(!existsSync(neoForgeJar))throw new Error('Set AE2_NEOFORGE_JAR to the installed NeoForge 21.1.259 universal JAR for conventional item tags');
const commonTags=execFileSync('unzip',['-Z1',neoForgeJar],{encoding:'utf8'}).split('\n').filter(p=>/^data\/c\/tags\/item\/.+\.json$/.test(p));
for(const path of commonTags){
 const dependency=JSON.parse(execFileSync('unzip',['-p',neoForgeJar,path],{encoding:'utf8'}));
 const dst=join(content,path);const own=existsSync(dst)?JSON.parse(readFileSync(dst,'utf8')):{values:[]};
 const values=[...new Map([...(dependency.values??[]),...(own.values??[])].map((v:unknown)=>[JSON.stringify(v),v])).values()];
 put(dst,{...dependency,...own,replace:false,values});
}
// Block tags are equally important: AE2's growth_acceleratable tag includes
// #c:budding_blocks, to which NeoForge adds vanilla budding amethyst.
const commonBlockTags=execFileSync('unzip',['-Z1',neoForgeJar],{encoding:'utf8'}).split('\n').filter(p=>/^data\/c\/tags\/block\/.+\.json$/.test(p));
for(const path of commonBlockTags){
 const dependency=JSON.parse(execFileSync('unzip',['-p',neoForgeJar,path],{encoding:'utf8'}));
 const dst=join(content,path);const own=existsSync(dst)?JSON.parse(readFileSync(dst,'utf8')):{values:[]};
 const values=[...new Map([...(dependency.values??[]),...(own.values??[])].map((v:unknown)=>[JSON.stringify(v),v])).values()];
 put(dst,{...dependency,...own,replace:false,values});
}
// Biome tag definitions are required for AE2 meteorite ocean, sand and cold rules.
const commonBiomeTags=execFileSync('unzip',['-Z1',neoForgeJar],{encoding:'utf8'}).split('\n').filter(p=>/^data\/c\/tags\/worldgen\/biome\/.+\.json$/.test(p));
for(const path of commonBiomeTags){
 const dependency=JSON.parse(execFileSync('unzip',['-p',neoForgeJar,path],{encoding:'utf8'}));
 const dst=join(content,path);const own=existsSync(dst)?JSON.parse(readFileSync(dst,'utf8')):{values:[]};
 const values=[...new Map([...(dependency.values??[]),...(own.values??[])].map((v:unknown)=>[JSON.stringify(v),v])).values()];
 put(dst,{...dependency,...own,replace:false,values});
}
report.gameplay_data={ae2_recipe_files:recipeFiles,ae2_tag_files:tagFiles,ae2_loot_files:lootFiles,common_item_tags:commonTags.length,common_block_tags:commonBlockTags.length,common_biome_tags:commonBiomeTags.length,omitted_optional_dependency_namespaces:[...omittedNamespaces],common_tag_source:'Pinned NeoForged NeoForge 21.1.259 universal JAR from official Maven; https://github.com/neoforged/NeoForge',note:'Copied definitions do not imply custom recipe handlers or world mechanics are implemented.'};
function modelStatus(name: string, seen = new Set<string>(), inheritedGeometry = false): string | null {
  const key = name.includes(':') ? name : 'minecraft:' + name;
  if (!key.startsWith('ae2:')) return key.startsWith('minecraft:') ? null : `external parent ${key}`;
  if (seen.has(key)) return `parent cycle ${key}`;
  seen.add(key);
  const file = join(assets, 'models', key.slice(4) + '.json');
  if (!existsSync(file)) return `missing parent ${key}`;
  let obj: any; try { obj=JSON.parse(readFileSync(file,'utf8')); } catch { return `invalid model ${key}`; }
  if (obj.loader) return `custom loader ${obj.loader}`;
  const hasGeometry = inheritedGeometry || !!obj.elements?.length;
  if (!obj.parent && !hasGeometry) return `dynamic or empty model ${key}`;
  return obj.parent ? modelStatus(obj.parent, seen, hasGeometry) : null;
}
// Block models use a resolver without the special item generator. Follow the
// entire chain including vanilla parents; missing built-ins cannot be used here.
function blockModelStatus(name: string, seen = new Set<string>(), hasGeometry = false): string | null {
  const key = name.includes(':') ? name : 'minecraft:' + name;
  if (key === 'minecraft:builtin/generated' || key === 'minecraft:builtin/entity') return `unsupported builtin in block model: ${key}`;
  if (seen.has(key)) return `parent cycle ${key}`;
  seen.add(key);
  const [namespace,path]=key.split(':');
  const file = namespace === 'ae2' ? join(assets,'models',path+'.json') : namespace === 'minecraft' ? join(vanillaResources,'assets/minecraft/models',path+'.json') : '';
  if (!file || !existsSync(file)) return `missing block model parent ${key}`;
  let obj: any; try {obj=JSON.parse(readFileSync(file,'utf8'));} catch {return `invalid block model ${key}`;}
  if (obj.loader) return `unsupported block model loader ${obj.loader}`;
  const geometry=hasGeometry || !!obj.elements?.length;
  if (!obj.parent && !geometry) return `block model has no static geometry: ${key}`;
  return obj.parent ? blockModelStatus(obj.parent,seen,geometry) : null;
}
function namedTexture(id: string): string | null {
  for (const n of [id, id.replace(/_storage$/, ''), `${id}_0`, `${id}/front`, `${id}/${id}_front`, 'generics/side']) {
    if (existsSync(join(assets,'textures/block',n+'.png'))) return 'ae2:block/'+n;
  }
  return null;
}
const entries: any[] = coverage.entries;
const blocks = new Map(entries.filter(x=>x.kind==='block' && (!x.id.includes(':debug_') || x.id==='ae2:debug_energy_gen' || x.id==='ae2:debug_item_gen' || x.id==='ae2:debug_cube_gen' || x.id==='ae2:debug_phantom_node')).map(x=>[x.id.slice(4),x]));
const blockItems = new Set(entries.filter(x=>x.kind==='block_item' && (!x.id.includes(':debug_') || x.id==='ae2:debug_energy_gen' || x.id==='ae2:debug_item_gen' || x.id==='ae2:debug_cube_gen' || x.id==='ae2:debug_phantom_node')).map(x=>x.id.slice(4)));
// Source-registered singleton items. AEItems explicitly sets stacksTo(1)
// for cells/tools; encoded patterns use EncodedPatternItemBuilder's default.
const definitions=readFileSync(join(reference,'src/main/java/appeng/core/definitions/AEItems.java'),'utf8');
const itemIds=readFileSync(join(reference,'src/main/java/appeng/api/ids/AEItemIds.java'),'utf8');
const idsBySymbol=new Map([...itemIds.matchAll(/ResourceLocation\s+(\w+)\s*=\s*id\("([^" ]+)"\)/g)].map(m=>[m[1],m[2]]));
const singletonIds=new Set<string>();
for (const line of definitions.split('\n')) {
  if (!line.includes('stacksTo(1)') && !line.includes('encodedPatternItemBuilder') && !line.includes('makePortableItemCell(AEItemIds.') && !line.includes('makePortableFluidCell(AEItemIds.') && !line.includes('.durability(')) continue;
  const symbol=line.match(/AEItemIds\.(\w+)/)?.[1]; const id=symbol && idsBySymbol.get(symbol); if(id) singletonIds.add(id);
}
// Constructors for Minecraft tiered tools impose a single-item durability stack.
for (const material of ['certus_quartz','nether_quartz','fluix']) for (const kind of ['axe','hoe','shovel','pickaxe','sword']) singletonIds.add(`${material}_${kind}`);
singletonIds.add('wrapped_generic_stack'); // WrappedGenericStack constructor
const items = new Map(entries.filter(x=>['item','part_item'].includes(x.kind) && (!x.id.includes(':debug_') || x.id==='ae2:debug_card' || x.id==='ae2:debug_eraser' || x.id==='ae2:debug_meteorite_placer' || x.id==='ae2:debug_replicator_card') && (x.kind !== 'item' || x.name)).map(x=>[x.id.slice(4),x]));
for (const x of entries.filter(x=>x.kind==='item' && !x.name && !x.id.includes(':debug_'))) {
  const id=x.id.slice(4); report.excluded.push({id:x.id,kind:x.kind,reason:'API identifier absent from AEItems registration; not assumed valid'});
  for (const p of [join(content,'data/ae2/horizon/item',id+'.json'),join(assets,'items',id+'.json'),join(assets,'models/item/horizon_static',id+'.json')]) rmSync(p,{force:true});
}
for (const x of entries.filter(x=>x.id.includes(':debug_') && x.id!=='ae2:debug_energy_gen' && x.id!=='ae2:debug_item_gen' && x.id!=='ae2:debug_card' && x.id!=='ae2:debug_cube_gen' && x.id!=='ae2:debug_eraser' && x.id!=='ae2:debug_meteorite_placer' && x.id!=='ae2:debug_replicator_card' && x.id!=='ae2:debug_phantom_node')) report.excluded.push({id:x.id,kind:x.kind,reason:'debug-only source registration excluded'});
// These source-backed state schemas reflect AE2 19.2.18 block classes.
const faces=['down','up','north','south','west','east'];
// Source AEBaseBlock.defaultProps and AEBlocks material groups. Tool tags are
// copied from the pinned datapack; requires_tool controls the separate drop gate.
function physicalProperties(id:string):Record<string,unknown>|undefined {
 const metal={hardness:2.2,explosion_resistance:11,requires_tool:false,sounds:'minecraft:iron_block'};
 const machines=new Set(['debug_energy_gen','debug_item_gen','debug_cube_gen','debug_phantom_node','drive','charger','inscriber','cell_workbench','chest','condenser','crafting_accelerator','crafting_monitor','crafting_unit','1k_crafting_storage','4k_crafting_storage','16k_crafting_storage','64k_crafting_storage','256k_crafting_storage','energy_acceptor','growth_accelerator','interface','io_port','molecular_assembler','pattern_provider','quantum_ring','spatial_anchor','spatial_io_port']);
 if(id==='inscriber')return {...metal,no_occlusion:true};
 if(machines.has(id))return metal;
 if(id==='mysterious_cube'||id==='not_so_mysterious_cube')return {...metal,hardness:10,explosion_resistance:1000,redstone_conductor:false};
 if(id==='controller')return {...metal,hardness:6,explosion_resistance:6};
 if(id==='vibration_chamber')return {...metal,hardness:4.2,explosion_resistance:4.2};
 if(id==='matrix_frame')return {...metal,sounds:'minecraft:stone',hardness:-1,explosion_resistance:6000000,no_occlusion:true,outline_empty:true};
 if(id==='tiny_tnt')return {hardness:0,explosion_resistance:0,requires_tool:false,sounds:'minecraft:gravel'};
 if(id==='sky_stone_block')return {hardness:50,explosion_resistance:150,requires_tool:true,sounds:'minecraft:stone'};
 if(id.startsWith('sky_stone_')||id.startsWith('smooth_sky_stone_'))return {hardness:5,explosion_resistance:150,requires_tool:!['sky_stone_chest','smooth_sky_stone_chest','sky_stone_tank'].includes(id),sounds:'minecraft:stone'};
 if(id.endsWith('_budding_quartz')||id==='fluix_block'||id.startsWith('fluix_')||id.includes('quartz')&&!['quartz_glass','quartz_vibrant_glass','quartz_fixture','small_quartz_bud','medium_quartz_bud','large_quartz_bud','quartz_cluster'].includes(id))return {hardness:3,explosion_resistance:5,requires_tool:true,sounds:'minecraft:stone',...(id.endsWith('_budding_quartz')?{push_reaction:'POPPED'}:{})};
 if(id==='quartz_glass'||id==='quartz_vibrant_glass')return {hardness:2.2,explosion_resistance:11,requires_tool:false,sounds:'minecraft:glass',...(id==='quartz_vibrant_glass'?{light:15}:{})};
 if(['energy_cell','dense_energy_cell','creative_energy_cell','quantum_link','spatial_pylon','wireless_access_point','crystal_resonance_generator'].includes(id))return {...metal,sounds:'minecraft:glass'};
 if(id==='quartz_fixture'||id==='light_detector')return {...metal,sounds:'minecraft:glass',no_collision:true,no_support:true,no_occlusion:true,redstone_conductor:false,solid:false,suffocating:false,push_reaction:'POPPED',...(id==='quartz_fixture'?{light:15}:{})};
 if(id==='crank')return {...metal,sounds:'minecraft:oak_planks',no_collision:true,no_support:true,no_occlusion:true,redstone_conductor:false,solid:false,suffocating:false};
}
function sourceShapeTemplate(id:string):string|undefined {
 if(id==='quartz_pillar')return 'minecraft:quartz_pillar';
 if(id.endsWith('_stairs'))return 'minecraft:stone_stairs';
 if(id.endsWith('_slab'))return 'minecraft:stone_slab';
 if(id.endsWith('_wall'))return 'minecraft:stone_brick_wall';
 if(id==='quartz_glass'||id==='quartz_vibrant_glass'||id==='matrix_frame'||id==='sky_stone_tank'||id==='sky_stone_chest'||id==='smooth_sky_stone_chest'||id==='molecular_assembler')return 'minecraft:glass';
}
function facingOutline(min:number,max:number,offset:number):Record<string,number[]> {
 const dirs:Record<string,number[]>={down:[0,-1,0],up:[0,1,0],north:[0,0,-1],south:[0,0,1],west:[-1,0,0],east:[1,0,0]};
 return Object.fromEntries(Object.entries(dirs).map(([face,d])=>[`facing=${face}`,[...d.map(v=>min-offset*v),...d.map(v=>max-offset*v)]]));
}
const representativeStates:Record<string,{state_properties:Record<string,string[]>,state_defaults:Record<string,string>,collision_boxes?:Record<string,number[]>,outline_boxes?:Record<string,number[]>}>={
 drive:{state_properties:{facing:faces,spin:['0','1','2','3']},state_defaults:{facing:'north',spin:'0'}},
 charger:{state_properties:{facing:faces,spin:['0','1','2','3']},state_defaults:{facing:'north',spin:'0'},outline_boxes:Object.fromEntries(faces.flatMap(face=>['0','1','2','3'].map(spin=>{
  const lo=[2/16,2/16,2/16],hi=[14/16,14/16,14/16];
  const forward=({down:[1,1],up:[1,-1],north:[2,1],south:[2,-1],east:[0,-1],west:[0,1]} as Record<string,number[]>)[face];
  const upAxis=face==='up'||face==='down' ? (Number(spin)%2===0?2:0) : (Number(spin)%2===0?1:(forward[0]===0?2:0));
  lo[upAxis]=0;hi[upAxis]=1;if(forward[1]>0)hi[forward[0]]=1;else lo[forward[0]]=0;
  return [`facing=${face},spin=${spin}`,[...lo,...hi]];
 })))},
 inscriber:{state_properties:{facing:faces,spin:['0','1','2','3'],waterlogged:['false','true']},state_defaults:{facing:'north',spin:'0',waterlogged:'false'}},
 crafting_monitor:{state_properties:{facing:faces,spin:['0','1','2','3'],formed:['false','true'],powered:['false','true']},state_defaults:{facing:'north',spin:'0',formed:'false',powered:'false'}},
 quartz_fixture:{state_properties:{facing:faces,odd:['false','true'],waterlogged:['false','true']},state_defaults:{facing:'up',odd:'false',waterlogged:'false'},outline_boxes:facingOutline(0.3,0.7,0.3)},
 light_detector:{state_properties:{facing:faces,odd:['false','true'],waterlogged:['false','true']},state_defaults:{facing:'up',odd:'false',waterlogged:'false'},outline_boxes:facingOutline(0.3,0.7,0.3)},
 crank:{state_properties:{facing:faces},state_defaults:{facing:'north'},outline_boxes:facingOutline(0.15,0.85,0.15)},
 growth_accelerator:{state_properties:{facing:faces,powered:['false','true']},state_defaults:{facing:'north',powered:'false'}},
 energy_cell:{state_properties:{fullness:['0','1','2','3','4']},state_defaults:{fullness:'0'}},
 dense_energy_cell:{state_properties:{fullness:['0','1','2','3','4']},state_defaults:{fullness:'0'}},
 vibration_chamber:{state_properties:{facing:faces,spin:['0','1','2','3'],active:['false','true']},state_defaults:{facing:'north',spin:'0',active:'false'}},
 io_port:{state_properties:{facing:faces,spin:['0','1','2','3'],powered:['false','true']},state_defaults:{facing:'north',spin:'0',powered:'false'}},
 spatial_io_port:{state_properties:{facing:faces,spin:['0','1','2','3'],powered:['false','true']},state_defaults:{facing:'north',spin:'0',powered:'false'}},
 cell_workbench:{state_properties:{facing:faces,spin:['0','1','2','3']},state_defaults:{facing:'north',spin:'0'}},
 chest:{state_properties:{facing:faces,spin:['0','1','2','3'],lights_on:['false','true']},state_defaults:{facing:'north',spin:'0',lights_on:'false'}},
 spatial_anchor:{state_properties:{facing:faces,powered:['false','true']},state_defaults:{facing:'north',powered:'false'}},
 crystal_resonance_generator:{state_properties:{facing:faces,waterlogged:['false','true']},state_defaults:{facing:'north',waterlogged:'false'},collision_boxes:{
  'facing=down':[2/16,1/16,2/16,14/16,1,14/16], 'facing=up':[2/16,0,2/16,14/16,15/16,14/16],
  'facing=north':[2/16,2/16,1/16,14/16,14/16,1], 'facing=south':[2/16,2/16,0,14/16,14/16,15/16],
  'facing=west':[1/16,2/16,2/16,1,14/16,14/16], 'facing=east':[0,2/16,2/16,15/16,14/16,14/16],
 }},
 ...Object.fromEntries(['sky_stone_chest','smooth_sky_stone_chest'].map(id=>[id,{state_properties:{facing:['north','east','south','west'],waterlogged:['false','true']},state_defaults:{facing:'north',waterlogged:'false'},collision_boxes:{'*':[0.06,0,0.06,0.94,15/16,0.94]}}])),
 molecular_assembler:{state_properties:{powered:['false','true']},state_defaults:{powered:'false'}},
 pattern_provider:{state_properties:{push_direction:['all',...faces]},state_defaults:{push_direction:'all'}},
 ...Object.fromEntries(['crafting_unit','crafting_accelerator','1k_crafting_storage','4k_crafting_storage','16k_crafting_storage','64k_crafting_storage','256k_crafting_storage'].map(id=>[id,{state_properties:{formed:['false','true'],powered:['false','true']},state_defaults:{formed:'false',powered:'false'}}])),
 controller:{state_properties:{state:['offline','online','conflicted'],type:['block','column_x','column_y','column_z','inside_a','inside_b']},state_defaults:{state:'offline',type:'block'}},
 quantum_ring:{state_properties:{formed:['false','true'],waterlogged:['false','true']},state_defaults:{formed:'false',waterlogged:'false'},collision_boxes:{'*':[2/16,2/16,2/16,14/16,14/16,14/16]}},
 quantum_link:{state_properties:{formed:['false','true'],waterlogged:['false','true']},state_defaults:{formed:'false',waterlogged:'false'},collision_boxes:{'*':[2/16,2/16,2/16,14/16,14/16,14/16]}},
 wireless_access_point:{state_properties:{facing:faces,state:['off','on','has_channel'],waterlogged:['false','true']},state_defaults:{facing:'north',state:'off',waterlogged:'false'},collision_boxes:{
  'facing=down':[3/16,5/16,3/16,13/16,1,13/16], 'facing=up':[3/16,0,3/16,13/16,11/16,13/16],
  'facing=north':[3/16,3/16,5/16,13/16,13/16,1], 'facing=south':[3/16,3/16,0,13/16,13/16,11/16],
  'facing=west':[5/16,3/16,3/16,1,13/16,13/16], 'facing=east':[0,3/16,3/16,11/16,13/16,13/16],
 }},
 spatial_pylon:{state_properties:{powered_on:['false','true'],axis:['x','y','z'],line:['none','start','middle','end'],online:['false','true']},state_defaults:{powered_on:'false',axis:'x',line:'none',online:'false'}},
};
// SpatialPylonBakedModel builds two full cubes with axis/line textures.
// Preserve its source texture selection in deterministic vanilla model JSON;
// source UV flips and emissive lighting require later native rendering.
function pylonStateModels():any {
 const variants:any={};
 const directions=['down','up','north','south','west','east'];
 for(const axis of ['x','y','z'])for(const line of ['none','start','middle','end'])for(const online of ['false','true']) {
  const suffix=line==='middle'?'_spanned':line==='start'||line==='end'?'_end':'';
  const inside=online==='true'?'dim':'red';
  const axial:Record<string,string[]>={x:['east','west'],y:['up','down'],z:['north','south']};
  const textures:any={};const outer:any={},inner:any={};
  for(const d of directions){
   const parallel=axial[axis].includes(d), outside=line==='none'||parallel?'base':'base'+suffix;
   const overlay=line==='none'?'dim':inside+(parallel?'':suffix);
   textures[`o_${d}`]=`ae2:block/spatial_pylon/${outside}`;
   textures[`i_${d}`]=`ae2:block/spatial_pylon/${overlay}`;
   outer[d]={texture:`#o_${d}`};inner[d]={texture:`#i_${d}`};
  }
  const model=`spatial_pylon_${axis}_${line}_${online}`;
  put(join(assets,'models/block/horizon_static',model+'.json'),{parent:'minecraft:block/block',render_type:'cutout',textures,elements:[{from:[0,0,0],to:[16,16,16],faces:outer},{from:[0,0,0],to:[16,16,16],faces:inner}]});
  variants[`axis=${axis},line=${line},online=${online}`]={model:`ae2:block/horizon_static/${model}`};
 }
 return {variants};
}
function convertSourceState(value:any,id:string):any {
 if(Array.isArray(value)) return value.map(v=>convertSourceState(v,id));
 if(value===null || typeof value!=='object') return value;
 const out:any={};
 for(const [key,v] of Object.entries(value)){
   if(key==='ae2:z') {out.z=v;continue;}
   if(key==='model' && id==='drive' && v==='ae2:block/drive') {out.model='ae2:block/drive/drive_base';continue;}
   if(key==='model' && (id==='quantum_ring'||id==='quantum_link') && v==='ae2:block/qnb/qnb_formed') {out.model=`ae2:block/qnb/${id==='quantum_ring'?'ring':'link'}`;continue;}
   if(key==='model' && id==='spatial_pylon' && v==='ae2:block/spatial_pylon') {out.model='ae2:block/horizon_static/spatial_pylon';continue;}
   out[key]=convertSourceState(v,id);
 }
 return out;
}
for (const [id, entry] of blocks) {
  if (id==='paint') {
    const model='ae2:block/horizon_static/paint_empty';
    put(join(assets,'models/block/horizon_static/paint_empty.json'),{textures:{particle:'ae2:block/paint1'},elements:[]});
    put(join(assets,'blockstates/paint.json'),{variants:Object.fromEntries(['0','1','2'].map(v=>[`light_level=${v}`,{model}]))});
    put(join(content,'data/ae2/horizon/block/paint.json'),{template:'minecraft:air',item:false,properties:{hardness:2.2,explosion_resistance:11,requires_tool:false,replaceable:false,no_collision:true,no_support:true,no_occlusion:true,outline_empty:true,solid:false,suffocating:false,redstone_conductor:false},state_properties:{facing:faces,light_level:['0','1','2']},state_defaults:{facing:'north',light_level:'0'},light_emission:{'light_level=0':0,'light_level=1':12,'light_level=2':15}});
    report.blocks.push({id:'ae2:paint',template:'minecraft:air',model,visual:'dynamic_paint_splotches',source:entry.source_files,behavior:'native_paint_partial'});continue;
  }
  if (id==='cable_bus') {
    // The host is a dynamic multipart container, not a solid chassis. Its
    // cable and parts are rendered from server-approved client visual data.
    const model='ae2:block/horizon_static/cable_bus_empty';
    put(join(assets,'models/block/horizon_static/cable_bus_empty.json'),{textures:{particle:'minecraft:block/iron_block'},elements:[]});
    const levels=Array.from({length:16},(_,i)=>String(i));
    const variants=Object.fromEntries(levels.flatMap(light=>['false','true'].map(waterlogged=>[`light=${light},waterlogged=${waterlogged}`,{model}])));
    const light_emission=Object.fromEntries(levels.map(n=>[`light=${n}`,Number(n)]));
    put(join(assets,'blockstates/cable_bus.json'),{variants});
    put(join(content,'data/ae2/horizon/block/cable_bus.json'),{template:'minecraft:glass',item:blockItems.has(id),properties:{hardness:2.2,explosion_resistance:11,requires_tool:false,no_support:true},state_properties:{light:levels,waterlogged:['false','true']},state_defaults:{light:'0',waterlogged:'false'},light_emission});
    report.blocks.push({id:'ae2:cable_bus',template:'minecraft:glass',model,visual:'dynamic_multipart',source:entry.source_files,behavior:'source_geometry_partial; dynamic_collision_pending'});
    continue;
  }
  if(representativeStates[id]) {
    const candidates=['main','generated'].map(sub=>join(reference,'src',sub,'resources/assets/ae2/blockstates',id+'.json'));
    const src=candidates.find(existsSync);if(!src)throw new Error(`missing upstream blockstate: ${id}`);
    let state=id==='spatial_pylon' ? pylonStateModels() : convertSourceState(JSON.parse(readFileSync(src,'utf8')),id);
    if(id==='crank') {
      const model='ae2:block/horizon_static/crank_empty';
      put(join(assets,'models/block/horizon_static/crank_empty.json'),{textures:{particle:'ae2:block/crank'},elements:[]});
      state={variants:{'':{model}}};
    }
    if(id==='crafting_monitor') {
      // The same dynamic CPU renderer draws the monitor's formed casing.
      const model='ae2:block/horizon_static/cpu_empty';
      put(join(assets,'models/block/horizon_static/cpu_empty.json'),{textures:{particle:'ae2:block/crafting/ring_corner'},elements:[]});
      for(const face of faces) state.variants[`facing=${face},formed=true`]={model};
    }
    if(['crafting_unit','crafting_accelerator','1k_crafting_storage','4k_crafting_storage','16k_crafting_storage','64k_crafting_storage','256k_crafting_storage'].includes(id)) {
      // Formed blocks are drawn by the dynamic CPU renderer, with a shared
      // empty static model so the unformed chassis cannot obscure its faces.
      const model='ae2:block/horizon_static/cpu_empty';
      put(join(assets,'models/block/horizon_static/cpu_empty.json'),{textures:{particle:'ae2:block/crafting/ring_corner'},elements:[]});
      state.variants['formed=true']={model};
    }
    const models:string[]=[];const collect=(v:any)=>{if(Array.isArray(v)){v.forEach(collect);return;}if(v&&typeof v==='object'){if(typeof v.model==='string')models.push(v.model);Object.values(v).forEach(collect);}};collect(state);
    for(const model of new Set(models)){if((id==='crank' && model==='ae2:block/horizon_static/crank_empty') || model==='ae2:block/horizon_static/cpu_empty')continue;const issue=blockModelStatus(model);if(issue)throw new Error(`${id}: ${issue}`);}
    put(join(assets,'blockstates',id+'.json'),state);
    // These source blocks have no collision and no occlusion. A transparent
    // base carries the native skylight and light-dampening behavior; their
    // source selection shapes and other properties are declared below.
    const transparentBase=['charger','quartz_fixture','light_detector','crank','quantum_ring','quantum_link','crystal_resonance_generator','sky_stone_chest','smooth_sky_stone_chest','molecular_assembler'].includes(id);
    const properties={...physicalProperties(id),
      ...(id==='charger'?{light_dampening:2,skylight_down:true}:{}),
      ...(['wireless_access_point','crystal_resonance_generator'].includes(id)?{light_dampening:0,skylight_down:true,no_support:true}:{}),
      ...(['sky_stone_chest','smooth_sky_stone_chest'].includes(id)?{no_support:true,light_dampening:0,skylight_down:true}:{}),
      ...(id==='molecular_assembler'?{light_dampening:1,skylight_down:false}:{}),
      ...(['quantum_ring','quantum_link'].includes(id)?{can_occlude:true,no_support:true}:{}),
    };
    const light_emission=id==='spatial_pylon'?{'powered_on=false':0,'powered_on=true':8}:undefined;
    const light_dampening_states=['quantum_ring','quantum_link'].includes(id)?{'waterlogged=false':0,'waterlogged=true':1}:undefined;
    put(join(content,'data/ae2/horizon/block',id+'.json'),{template:transparentBase?'minecraft:glass':'minecraft:iron_block',item:blockItems.has(id),...(properties?{properties}:{}),...representativeStates[id],...(light_emission?{light_emission}:{}),...(light_dampening_states?{light_dampening_states}:{})});
    const itemFallback:Record<string,string>={drive:'ae2:block/drive/drive_base',charger:'ae2:block/charger',inscriber:'ae2:block/inscriber',crafting_monitor:'ae2:block/crafting/monitor',controller:'ae2:block/controller/controller_block_offline',quantum_ring:'ae2:block/qnb/ring',quantum_link:'ae2:block/qnb/link',wireless_access_point:'ae2:block/wireless_access_point_chassis',spatial_pylon:'ae2:block/horizon_static/spatial_pylon_x_none_false'};
    report.blocks.push({id:'ae2:'+id,template:'minecraft:iron_block',item:blockItems.has(id),model:itemFallback[id],visual:'source_blockstates_and_static_geometry',source:entry.source_files,behavior:'source_state_schema; live state updates and exact dynamic rendering remain partial',reason:id==='drive'?'static source chassis; cells are not rendered dynamically':id==='crafting_monitor'?'formed dynamic CPU frame and monitor display require native rendering':id.startsWith('quantum_')?'formed dynamic model approximated by unformed source geometry':id==='spatial_pylon'?'source pylon texture selection projected; UV flips and emissive light remain partial':null});
    continue;
  }
  const crystalTemplates:Record<string,string>={small_quartz_bud:'small_amethyst_bud',medium_quartz_bud:'medium_amethyst_bud',large_quartz_bud:'large_amethyst_bud',quartz_cluster:'amethyst_cluster'};
  if(crystalTemplates[id]){
    const model=`ae2:block/${id}`;const issue=blockModelStatus(model);if(issue)throw new Error(`${id}: ${issue}`);
    const sourceState=join(reference,'src/generated/resources/assets/ae2/blockstates',id+'.json');
    put(join(assets,'blockstates',id+'.json'),JSON.parse(readFileSync(sourceState,'utf8')));
    const template=`minecraft:${crystalTemplates[id]}`;
    put(join(content,'data/ae2/horizon/block',id+'.json'),{template,item:blockItems.has(id),properties:{hardness:1.5,explosion_resistance:1.5,requires_tool:true,can_occlude:true}});
    report.blocks.push({id:'ae2:'+id,template,item:blockItems.has(id),model,visual:'source_directional_waterlogged_model',reason:null,source:entry.source_files,behavior:'native_bud_growth_and_vanilla_cluster_physics'});
    continue;
  }
  const sourceItem = 'ae2:item/' + id;
  let selected = sourceItem, issue = blockModelStatus(sourceItem);
  if (issue) {
    const texture = namedTexture(id);
    selected = `ae2:block/horizon_static/${id}`;
    put(join(assets,'models/block/horizon_static',id+'.json'), {parent:'minecraft:block/cube_all', textures:{all:texture ?? 'minecraft:block/iron_block'}});
    if (!texture) report.errors.push({id:'ae2:'+id,reason:'no matching AE2 texture; vanilla iron texture fallback'});
  }
  const directionalShape=/(stairs|slab|wall)$/.test(id) || id==='quartz_pillar';
  const shapeSource=['main','generated'].map(sub=>join(reference,'src',sub,'resources/assets/ae2/blockstates',id+'.json')).find(existsSync);
  if(directionalShape && shapeSource) {
    const shapeState=convertSourceState(JSON.parse(readFileSync(shapeSource,'utf8')),id);
    const models:string[]=[];const collect=(v:any)=>{if(Array.isArray(v)){v.forEach(collect);return;}if(v&&typeof v==='object'){if(typeof v.model==='string')models.push(v.model);Object.values(v).forEach(collect);}};collect(shapeState);
    if(models.every(model=>!blockModelStatus(model))) put(join(assets,'blockstates',id+'.json'),shapeState);
    else put(join(assets,'blockstates',id+'.json'), {variants:{'':{model:selected}}});
  } else put(join(assets,'blockstates',id+'.json'), {variants:{'':{model:selected}}});
  if (id === 'tiny_tnt') {
    put(join(content,'data/ae2/horizon/block',id+'.json'), {template:'minecraft:tnt',item:blockItems.has(id),properties:{...physicalProperties(id),skylight_down:true,no_support:true},collision_boxes:{'*':[0.25,0,0.25,0.75,0.5,0.75]}});
    report.blocks.push({id:'ae2:tiny_tnt',template:'minecraft:tnt',model:selected,visual:'source_static',source:entry.source_files,behavior:'tiny_tnt_native_partial'});continue;
  }
  const template=sourceShapeTemplate(id) ?? 'minecraft:iron_block';
  const quartzDecorative=/^(?:quartz_block|cut_quartz_block|smooth_quartz_block|quartz_bricks|quartz_pillar|chiseled_quartz_block|(?:quartz|cut_quartz|smooth_quartz|quartz_brick|quartz_pillar|chiseled_quartz)_(?:slab|stairs|wall))$/.test(id);
  const properties={...physicalProperties(id),...(quartzDecorative?{random_ticks:true}:{}),...(['sky_stone_tank','molecular_assembler'].includes(id)?{light_dampening:1,skylight_down:false}:{})};
  put(join(content,'data/ae2/horizon/block',id+'.json'), {template,item:blockItems.has(id),...(properties?{properties}:{})});
  report.blocks.push({id:'ae2:'+id,template:'minecraft:iron_block',item:blockItems.has(id),model:selected,visual:issue?'static_texture_fallback':'source_item_model_static',reason:issue,source:entry.source_files,behavior:'unimplemented_unless_native_host_explicitly_implements_it'});
}
// The tested Phantom block uses its source blockstate, not the item model.
copyFileSync(join(reference,'src/generated/resources/assets/ae2/blockstates/debug_phantom_node.json'),join(assets,'blockstates/debug_phantom_node.json'));
for (const [id, entry] of items) {
  if (blockItems.has(id)) {report.excluded.push({id:'ae2:'+id,kind:entry.kind,reason:'already registered as block item'});continue;}
  const model = 'ae2:item/'+id; const issue = modelStatus(model);
  let chosen = issue ? `ae2:item/horizon_static/${id}` : model;
  if (id === 'facade') {
    // FacadeItemModel bakes this source anchor before adding the sliced
    // texture item in the native renderer. An empty facade uses the anchor.
    const base = 'ae2:item/facade_base', problem = modelStatus(base);
    if (problem || !existsSync(join(assets,'textures/part/cable_anchor.png'))) {
      throw new Error(`facade source assets unavailable: ${problem ?? 'missing anchor texture'}`);
    }
    chosen = base;
  } else if (id === 'memory_card') {
    // The custom loader delegates its card body to this ordinary source model;
    // the native renderer adds the eight tinted cubes from MemoryCardBakedModel.
    const base = 'ae2:item/memory_card_base', problem = modelStatus(base);
    if (problem || !['memory_card_base','memory_card_led','memory_card_hash'].every(name=>existsSync(join(assets,'textures/item',name+'.png')))) {
      throw new Error(`memory card source assets unavailable: ${problem ?? 'missing texture'}`);
    }
    chosen = base;
  } else if (id === 'wrapped_generic_stack') {
    // Source registers an empty item model; AEBaseScreen uses the contained
    // GenericStack key to draw GUI slots. Never replace it with an iron ingot.
    put(join(assets,'models/item/horizon_static',id+'.json'), {parent:'minecraft:block/block',gui_light:'front',elements:[]});
    chosen = `ae2:item/horizon_static/${id}`;
  } else if (issue) {
    const texture = existsSync(join(assets,'textures/item',id+'.png')) ? 'ae2:item/'+id : 'minecraft:item/iron_ingot';
    put(join(assets,'models/item/horizon_static',id+'.json'), {parent:'minecraft:item/generated', textures:{layer0:texture}});
    if (texture.startsWith('minecraft:')) report.errors.push({id:'ae2:'+id,reason:`${issue}; generic item fallback`});
  }
  put(join(assets,'items',id+'.json'), {model:{type:'minecraft:model',model:chosen}});
  // QuartzToolType uses iron's tier. Fluix inherits iron's mining tags;
  // native item projection supplies source Fluix durability, speed and attack,
  // and runtime enchantment lookup supplies its intrinsic Fortune/Looting.
  const tool = id.match(/^(certus_quartz|nether_quartz|fluix)_(axe|hoe|shovel|pickaxe|sword)$/);
  const template = tool ? `minecraft:iron_${tool[2]}` : 'minecraft:iron_ingot';
  const obj: any = {template};
  if (singletonIds.has(id)) obj.max_stack_size=1;
  if (id === 'item_storage_cell_1k') obj.rarity='uncommon';
  put(join(content,'data/ae2/horizon/item',id+'.json'), obj);
  const visual=id==='facade'?'source_anchor_native_dynamic_facade':id==='memory_card'?'source_card_base_native_dynamic_leds':id==='wrapped_generic_stack'?'source_empty_model_native_gui_key':issue?'static_texture_fallback':'source_model';
  const reason=id==='facade'?'textured facade requires the native item renderer':id==='memory_card'?'LED cubes require the native item renderer':id==='wrapped_generic_stack'?'GUI key drawing requires the native item renderer':issue;
  report.items.push({id:'ae2:'+id,kind:entry.kind,template,model:chosen,max_stack_size:obj.max_stack_size ?? 64,visual,reason,behavior:'unimplemented_unless_native_host_explicitly_implements_it'});
}
// Source ColorApplicatorModel chooses its colored variant only with an active
// stored color. The native renderer supplies a render-only custom-model flag.
const coloredApplicator=JSON.parse(readFileSync(join(assets,'models/item/color_applicator_colored.json'),'utf8'));
for(const element of coloredApplicator.elements??[])for(const face of Object.values(element.faces??{}) as any[]){
  const tint=({'#dark':1,'#medium':2,'#bright':3} as Record<string,number>)[face.texture];
  if(tint!==undefined)face.tintindex=tint;
}
coloredApplicator.gui_light='front';
put(join(assets,'models/item/color_applicator_colored.json'),coloredApplicator);
put(join(assets,'items/color_applicator.json'),{model:{type:'minecraft:condition',property:'minecraft:custom_model_data',index:0,
  on_true:{type:'minecraft:model',model:'ae2:item/color_applicator_colored'},
  on_false:{type:'minecraft:model',model:'ae2:item/color_applicator'}}});
// AE2's custom compass loader combines its source base and independent
// pointer. The host applies the live server-directed pointer rotation.
const compassBase=JSON.parse(readFileSync(join(assets,'models/item/meteorite_compass_base.json'),'utf8'));
const compassPointer=JSON.parse(readFileSync(join(assets,'models/item/meteorite_compass_pointer.json'),'utf8'));
compassPointer.display=compassBase.display;
put(join(assets,'models/item/horizon_static/meteorite_compass_pointer.json'),compassPointer);
put(join(assets,'items/meteorite_compass.json'),{model:{type:'minecraft:composite',models:[
  {type:'minecraft:model',model:'ae2:item/meteorite_compass_base'},
  {type:'minecraft:model',model:'ae2:item/horizon_static/meteorite_compass_pointer'}
]}});
for (const [id] of blocks) if (blockItems.has(id)) {
 const itemModel = 'ae2:item/'+id; const issue=modelStatus(itemModel); const block=report.blocks.find((x:any)=>x.id==='ae2:'+id);
 put(join(assets,'items',id+'.json'), {model:{type:'minecraft:model',model:issue?block.model:itemModel}});
}
function walk(path: string): string[] {return existsSync(path)?readdirSync(path,{withFileTypes:true}).flatMap(d=>d.isDirectory()?walk(join(path,d.name)):[join(path,d.name)]):[];}
for (const p of walk(join(assets,'models')).filter(p=>p.endsWith('.json'))) { try {const m=JSON.parse(readFileSync(p,'utf8'));if(m.loader) report.unsupported_model_loaders.push({file:p.slice(assets.length+1),loader:m.loader});} catch {report.errors.push({file:p.slice(assets.length+1),reason:'invalid model JSON'});} }
// Include the validated, source-preserving Guide assets in clean projections.
syncGuideAssets(reference, content);
report.counts = { advancement_files:advancementFiles, singleton_items:report.items.filter((x:any)=>x.max_stack_size===1).length, blocks:report.blocks.length,items:report.items.length,block_items:blockItems.size,errors:report.errors.length,excluded:report.excluded.length,model_loader_files:report.unsupported_model_loaders.length };
writeFileSync(join(content,'horizon-mod.toml'),`id = "ae2"\nname = "AE2 Content Experiment"\nversion = "0.2.0"\ndescription = "AE2 source visual catalog; native behavior is separately implemented and incomplete."\nauthors = ["Team Applied Energistics", "Adam's Codex experiment"]\napi = "0.2"\ncapabilities = ["registries"]\n`);
writeFileSync(join(content,'ATTRIBUTION.md'),`# Attribution\n\nAssets generated locally from Applied Energistics 2, 1.21.1 branch, commit db17504a86128fdf3dae31f5fb7a112a646e0b93: https://github.com/AppliedEnergistics/Applied-Energistics-2\n\nAE2 textures and models: © 2020 Ridanisaurus Rid; © 2013–2020 AlgorithmX2 et al., Creative Commons Attribution-NonCommercial-ShareAlike 3.0: https://creativecommons.org/licenses/by-nc-sa/3.0/ . Language text and translations: CC0 per upstream README. Source code LGPL-3.0-or-later. Distribution and use must observe the applicable licenses.\n`);
put(join(content,'content-inventory.json'),report);
appendAttribution();
console.log(JSON.stringify(report.counts,null,2));
function appendAttribution(){const p=join(content,'ATTRIBUTION.md');writeFileSync(p,readFileSync(p,'utf8')+'\nConventional item, block and biome tag JSONs merge NeoForge 21.1.259 data from its pinned official Maven JAR. Source: https://github.com/neoforged/NeoForge/tree/1.21.1/src/generated/resources/data/c/tags . NeoForge is LGPL-2.1-only: https://github.com/neoforged/NeoForge/blob/1.21.1/LICENSE.txt . No Java classes or Mojang prepared resources are redistributed in this content folder.\n');}
