#!/usr/bin/env python3
"""Create a conservative source-backed coverage snapshot; not a runtime registry dump."""
import json, pathlib, re
root=pathlib.Path(__file__).resolve().parent.parent
src=root/'ae2-reference/src'
api=src/'main/java/appeng/api/ids'
reg={}
for kind, filename in [('item','AEItemIds.java'),('part_item','AEPartIds.java'),('block','AEBlockIds.java')]:
 text=(api/filename).read_text()
 for symbol,name in re.findall(r'public static final ResourceLocation\s+(\w+)\s*=\s*id\("([^"]+)"\)',text):
  reg.setdefault((kind,name),{'id':'ae2:'+name,'kind':kind,'api_symbols':[],'status':'unverified_in_world','source_files':[]})
  item=reg[kind,name]; item['api_symbols'].append(symbol); item['source_files'].append('src/main/java/appeng/api/ids/'+filename)
# Direct internal/debug definitions bypass the public API ID classes.
for kind, filename in [('item','AEItems.java'),('block','AEBlocks.java')]:
 text=(src/'main/java/appeng/core/definitions'/filename).read_text()
 for name in re.findall(r'AppEng\.makeId\("([^"]+)"\)', text):
  reg.setdefault((kind,name),{'id':'ae2:'+name,'kind':kind,'api_symbols':[],'status':'unverified_in_world','source_files':['src/main/java/appeng/core/definitions/'+filename]})
# AEBlocks.block registers a matching BlockItem in AEItems.DR for each block definition.
for (kind,name),entry in list(reg.items()):
 if kind == 'block':
  reg['block_item',name]={'id':'ae2:'+name,'kind':'block_item','api_symbols':entry['api_symbols'].copy(),'status':'unverified_in_world','source_files':['src/main/java/appeng/core/definitions/AEBlocks.java']}
# Parts are item registrations.
lang=json.loads((src/'generated/resources/assets/ae2/lang/en_us.json').read_text())
for (kind,name),v in reg.items():
 v['name']=lang.get(('block' if kind=='block' else 'item')+'.ae2.'+name)
 v['behavior_status']='not_verified'; v['notes']='API id; existence or equivalence in the Horizon world has not been verified.'
models=src/'generated/resources/assets/ae2/models/item'
blocks=src/'generated/resources/assets/ae2/blockstates'
recipes=src/'generated/resources/data/ae2/recipe'
result={'snapshot':'2026-10-26','reference':{'project':'Applied Energistics 2','version':'19.2.18','commit':'db17504a86128fdf3dae31f5fb7a112a646e0b93','repository':'https://github.com/AppliedEnergistics/Applied-Energistics-2','license':'LGPL-3.0-or-later (some API files MIT)','scope_warning':'Static API and literal internal identifiers, not an authoritative runtime registry dump. Dynamic and internal registrations may be absent. Block items follow AEBlocks.block registration in the source. No item or block has verified full parity in a Horizon world.'},
'counts':{'source_item_ids_including_internal':sum(k[0]=='item' for k in reg),'static_part_item_ids':sum(k[0]=='part_item' for k in reg),'source_block_item_ids':sum(k[0]=='block_item' for k in reg),'source_block_ids_including_internal':sum(k[0]=='block' for k in reg),'generated_item_model_json':len(list(models.rglob('*.json'))),'generated_blockstate_json':len(list(blocks.rglob('*.json'))),'generated_recipe_json':len(list(recipes.rglob('*.json')))},
'entries':sorted(reg.values(),key=lambda x:(x['kind'],x['id'])),
'behavior_families':[
{'family':'network graph, channels, power, topology, security, grids','status':'partial prototype; no full-world parity verified','source':'src/main/java/appeng/me'},
{'family':'storage cells, item/fluid identity, filtering, capacity and priority','status':'partial prototype; integration and parity unverified','source':'src/main/java/appeng/items/storage'},
{'family':'crafting patterns and substitution','status':'partial prototype: pattern alternatives, ingredients and multiple outputs; no slot splitting, recipe remainder, tags or full backtracking','source':'src/main/java/appeng/crafting/pattern'},
{'family':'crafting planning and reservations','status':'partial prototype: recursive plans, byproducts, atomic submit, cycle/missing detection; no AE2 byte parity, true simulation or full backtracking','source':'src/main/java/appeng/crafting/CraftingCalculation.java'},
{'family':'crafting CPU execution and persistence','status':'partial prototype: multi-CPU jobs, in-flight work, partial output, serialization, cancellation and coarse 3-tick capacity; no world integration, exact CPU byte model, per-push budget, power or AE2 parity','source':'src/main/java/appeng/crafting/execution/CraftingCpuLogic.java'},
{'family':'terminals, menus and wireless access','status':'unverified','source':'src/main/java/appeng/menu/me'},
{'family':'machines, inscriber, charger, growth, condenser, IO','status':'unverified','source':'src/main/java/appeng/block/misc'},
{'family':'cables, buses, interface, facades, multipart rendering','status':'unverified','source':'src/main/java/appeng/parts'},
{'family':'spatial storage, quantum bridge, world generation and meteorites','status':'unverified','source':'src/main/java/appeng/spatial'},
{'family':'tools, armor, powers, upgrades, recipes, advancements and guidebook','status':'unverified','source':'src/main/java/appeng/items'}]}
(root/'ae2-feature-coverage.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result['counts'],indent=2))
