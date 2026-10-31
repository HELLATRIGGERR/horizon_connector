/** Compile the pinned AE2 GuideME corpus into the native reader contract.
 * This is a development tool, not player runtime code. The runtime consumes
 * the resulting JSON plus the unmodified source assets. Dependencies can be
 * installed normally or supplied as AE2_MARKED_MODULE / AE2_YAML_MODULE.
 */
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,readdirSync,writeFileSync,existsSync} from 'node:fs';
import {join,relative,posix,resolve} from 'node:path';

const markedModule=await import(process.env.AE2_MARKED_MODULE??'marked');
const yamlModule=await import(process.env.AE2_YAML_MODULE??'yaml');
const marked=markedModule.marked;
const parseYaml=yamlModule.parse;
const repoRoot=resolve(import.meta.dir,'../../..');
const guideRoot=resolve(process.env.AE2_GUIDE_ROOT??join(repoRoot,'.local/guidebook/ae2guide'));
const output=join(guideRoot,'compiled-guide.json');
const supported=new Set(['GameScene','ImportStructure','IsometricCamera','BoxAnnotation',
  'LineAnnotation','DiamondAnnotation','ItemLink','ItemImage','ItemIcon','BlockImage',
  'RecipeFor','Recipe','Row','Column','SubPages','CategoryIndex','ItemGrid','ae2:ConfigValue',
  'a','br','div']);
const resourceTags=new Set(['ItemLink','ItemImage','ItemIcon','BlockImage','RecipeFor','Recipe']);
const voidTags=new Set(['br']);
const diagnostics:{page:string;kind:string;value:string}[]=[];
const counts:Record<string,number>={};

type JsonValue=null|boolean|number|string|JsonValue[]|{[key:string]:JsonValue};
type Node={type:string;[key:string]:unknown};
type ElementNode={type:'element';name:string;attrs:Record<string,string|boolean>;
  children:Node[];scene_id?:string;asset?:string;resource_id?:string;target?:JsonValue;anchor?:string};
type Piece=string|ElementNode;
type Page={id:string;source_sha256:string;navigation:Record<string,JsonValue>;categories:string[];
  item_ids:string[];metadata:Record<string,JsonValue>;anchors:string[];nodes:Node[]};
type Token={type:string;raw?:string;text?:string;tokens?:Token[];depth?:number;ordered?:boolean;
  start?:number|string;items?:Token[];href?:string;title?:string|null;lang?:string;
  header?:{tokens?:Token[]}[];rows?:{tokens?:Token[]}[][];align?:(string|null)[]};

function resourceId(value:string):string {return value.includes(':')?value:`ae2:${value}`;}
function normalizeAsset(page:string,src:string):string {
  assert(!src.startsWith('/')&&!src.includes('\\'),'absolute or platform-specific guide path');
  const normalized=posix.normalize(posix.join(posix.dirname(page),src));
  assert(normalized!=='.'&&!normalized.startsWith('../')&&!normalized.startsWith('/'),`guide path escapes root: ${page}: ${src}`);
  return normalized;
}
function targetFor(page:string,href:string):JsonValue {
  if(/^(?:[a-z][a-z0-9+.-]*:|\/\/)/i.test(href))return {kind:'external',url:href};
  const hash=href.indexOf('#');const path=hash<0?href:href.slice(0,hash);
  const anchor=hash<0?null:decodeURIComponent(href.slice(hash+1));
  if(!path)return {kind:'page',page,anchor};
  const decoded=decodeURIComponent(path);
  if(decoded.endsWith('.md'))return {kind:'page',page:normalizeAsset(page,decoded),anchor};
  return {kind:'asset',asset:normalizeAsset(page,decoded),anchor};
}
function attrsOf(raw:string,page:string,name:string):Record<string,string|boolean> {
  const out:Record<string,string|boolean>={};let at=0;
  const attr=/\s*([A-Za-z_][\w:.-]*)\s*=\s*(?:"([^"]*)"|'([^']*)'|\{(true|false)\}|([^\s/>]+))/gy;
  while(at<raw.length){
    if(/^\s*$/.test(raw.slice(at)))break;
    attr.lastIndex=at;const m=attr.exec(raw);
    assert(m,`invalid ${name} attributes at ${page}: ${raw.slice(at)}`);
    const key=m[1];assert(!(key in out),`duplicate ${name}.${key} in ${page}`);
    out[key]=m[4]===undefined?(m[2]??m[3]??m[5]):m[4]==='true';at=attr.lastIndex;
  }
  return out;
}

/** Find complete tags without interpreting punctuation inside quoted values. */
function tagAt(text:string,start:number):{end:number;closing:boolean;name:string;attrs:string;selfClosing:boolean}|null {
  if(text[start]!=='<')return null;
  let i=start+1,closing=false;if(text[i]==='/'){closing=true;i++;}
  while(/\s/.test(text[i]??''))i++;
  const begin=i;while(/[A-Za-z0-9_:.-]/.test(text[i]??''))i++;
  const name=text.slice(begin,i);if(!supported.has(name))return null;
  const attrsStart=i;let quote='';let brace=0;
  for(;i<text.length;i++){
    const c=text[i];if(quote){if(c===quote)quote='';continue}
    if(c==='"'||c==="'"){quote=c;continue}if(c==='{'){brace++;continue}if(c==='}'){brace--;continue}
    if(c==='>'&&brace===0){let attrs=text.slice(attrsStart,i),selfClosing=/\/\s*$/.test(attrs)||voidTags.has(name);
      if(selfClosing)attrs=attrs.replace(/\/\s*$/,'');return {end:i+1,closing,name,attrs,selfClosing};}
  }
  throw Error(`unterminated ${name} tag`);
}
function inlineCodeEnd(text:string,at:number):number {
  let end=at;while(text[end]==='`')end++;const ticks=text.slice(at,end);
  const close=text.indexOf(ticks,end);return close<0?end:close+ticks.length;
}

/** Split custom tags before Markdown lexing, so nested scenes survive even
 * where CommonMark considers their blank lines separate HTML blocks. */
function parseMarkup(text:string,page:string):{pieces:Piece[];children:WeakMap<ElementNode,Piece[]>} {
  const root:Piece[]=[];const stack:{name:string;pieces:Piece[];element?:ElementNode}[]=[{name:'',pieces:root}];
  const children=new WeakMap<ElementNode,Piece[]>();
  let from=0,i=0,scene=0;
  while(i<text.length){
    if(text[i]==='`'){i=inlineCodeEnd(text,i);continue}
    if(text[i]!=='<'){i++;continue}
    const tag=tagAt(text,i);if(!tag){i++;continue}
    const current=stack[stack.length-1].pieces;
    if(i>from)current.push(text.slice(from,i));
    if(tag.closing){
      assert(stack.length>1&&stack[stack.length-1].name===tag.name,`unbalanced closing ${tag.name} in ${page}`);
      const closed=stack.pop();assert(closed?.element);children.set(closed.element,closed.pieces);
    } else {
      const element:ElementNode={type:'element',name:tag.name,attrs:attrsOf(tag.attrs,page,tag.name),children:[]};
      counts[tag.name]=(counts[tag.name]??0)+1;
      if(tag.name==='GameScene')element.scene_id=`${page}#scene-${scene++}`;
      if(resourceTags.has(tag.name)){
        assert(typeof element.attrs.id==='string',`${page}: ${tag.name} lacks id`);
        element.resource_id=resourceId(element.attrs.id);
      }
      if(tag.name==='ImportStructure'){
        assert(typeof element.attrs.src==='string',`${page}: missing structure path`);
        element.asset=normalizeAsset(page,element.attrs.src);
        if(!existsSync(join(guideRoot,element.asset)))diagnostics.push({page,kind:'missing_structure',value:element.asset});
      }
      if(tag.name==='a'&&typeof element.attrs.href==='string')element.target=targetFor(page,element.attrs.href);
      if(tag.name==='a'&&typeof element.attrs.name==='string')element.anchor=element.attrs.name;
      current.push(element);
      if(!tag.selfClosing){const pieces:Piece[]=[];stack.push({name:tag.name,pieces,element});}
    }
    i=tag.end;from=i;
  }
  if(from<text.length)stack[stack.length-1].pieces.push(text.slice(from));
  assert.equal(stack.length,1,`unclosed ${stack[stack.length-1].name} in ${page}`);
  return {pieces:root,children};
}

function dedent(text:string,preferFirstIndent=false):string {
  const lines=text.split('\n');const indents=lines.filter(line=>line.trim()).map(line=>(line.match(/^ */)?.[0].length??0));
  const width=indents.length?(preferFirstIndent?indents[0]:Math.min(...indents)):0;
  return width?lines.map(line=>line.slice(Math.min(width,line.match(/^ */)?.[0].length??0))).join('\n'):text;
}
function plain(nodes:Node[]):string {
  return nodes.map(node=>{
    if(node.type==='text'||node.type==='code')return String(node.text??'');
    if(node.type==='element'&&typeof node.resource_id==='string')return node.resource_id.split(':').at(-1)??'';
    return Array.isArray(node.children)?plain(node.children as Node[]):'';
  }).join('');
}
function slug(text:string):string {
  return text.toLowerCase().replace(/[^\p{L}\p{N}\s_-]/gu,'').trim().replace(/\s+/g,'-');
}
function compileBody(text:string,page:string):{nodes:Node[];anchors:string[]} {
  const markup=parseMarkup(text,page);const slots=new Map<number,ElementNode>();let nextSlot=0;
  const anchors=new Set<string>();const slugs=new Map<string,number>();
  const splitText=(text:string):Node[]=>{
    const out:Node[]=[];const regex=/\uE000([0-9]+)\uE001/g;let from=0;
    for(const m of text.matchAll(regex)){
      const at=m.index; if(at>from)out.push({type:'text',text:text.slice(from,at)});
      const node=slots.get(Number(m[1]));assert(node,`${page}: unknown markup slot ${m[1]}`);out.push(node);from=at+m[0].length;
    }
    if(from<text.length)out.push({type:'text',text:text.slice(from)});return out;
  };
  const compilePieces=(pieces:Piece[],nested:boolean,owner?:string):Node[]=>{
    let source='';
    for(const piece of pieces){
      if(typeof piece==='string'){source+=piece;continue}
      // Adjacent standalone tags are one CommonMark paragraph after
      // placeholder substitution. Unwrap an element-only paragraph so scene
      // instructions and Row/Column children remain direct ordered nodes.
      piece.children=compilePieces(markup.children.get(piece)??[],true,piece.name).flatMap(node=>{
        if(node.type!=='paragraph'||!Array.isArray(node.children))return [node];
        const children=node.children as Node[];
        if(!children.every(child=>child.type==='element'||(child.type==='text'&&!String(child.text??'').trim())))return [node];
        return children.filter(child=>child.type==='element');
      });
      if(piece.anchor)anchors.add(piece.anchor);
      const key=nextSlot++;slots.set(key,piece);source+=`\uE000${key}\uE001`;
    }
    if(nested)source=dedent(source,owner!==undefined&&['BoxAnnotation','LineAnnotation','DiamondAnnotation'].includes(owner));
    // GameScene instruction elements are structural XML children.  The
    // source permits uneven indentation; four spaces before a placeholder
    // must not turn an annotation and its tooltip into a Markdown code block.
    if(owner==='GameScene')source=source.replace(/^[ \t]+(?=\uE000[0-9]+\uE001[ \t]*\r?$)/gm,'');
    const lexed:unknown=marked.lexer(source,{gfm:true});assert(Array.isArray(lexed));
    return convert(lexed as Token[]);
  };
  const inline=(token:Token):Node[]=>token.tokens?convert(token.tokens):splitText(token.text??'');
  const convert=(tokens:Token[]):Node[]=>{
    const out:Node[]=[];
    for(const token of tokens){
      switch(token.type){
        case 'space':break;
        case 'text':case 'escape':out.push(...inline(token));break;
        case 'paragraph':{
          const children=inline(token);
          if(children.length===1&&children[0].type==='element')out.push(children[0]);
          else if(children.length)out.push({type:'paragraph',children});break;
        }
        case 'heading':{
          const children=inline(token),base=slug(plain(children))||'section',n=slugs.get(base)??0;
          slugs.set(base,n+1);const anchor=n?`${base}-${n}`:base;anchors.add(anchor);
          out.push({type:'heading',level:token.depth??1,anchor,children});break;
        }
        case 'em':out.push({type:'emphasis',children:inline(token)});break;
        case 'strong':out.push({type:'strong',children:inline(token)});break;
        case 'del':out.push({type:'strikethrough',children:inline(token)});break;
        case 'codespan':out.push({type:'code',text:token.text??''});break;
        case 'code':out.push({type:'code_block',text:token.text??'',language:token.lang??null});break;
        case 'br':out.push({type:'break'});break;
        case 'hr':out.push({type:'rule'});break;
        case 'blockquote':out.push({type:'quote',children:convert(token.tokens??[])});break;
        case 'link':{
          const href=token.href??'';out.push({type:'link',href,target:targetFor(page,href),children:inline(token)});break;
        }
        case 'image':{
          const src=token.href??'';const target=targetFor(page,src);
          const external=typeof target==='object'&&target!==null&&!Array.isArray(target)&&target.kind==='external';
          const asset=external?null:normalizeAsset(page,decodeURIComponent(src.split('#')[0]));
          if(asset&&!existsSync(join(guideRoot,asset)))diagnostics.push({page,kind:'missing_image',value:asset});
          out.push({type:'image',src,alt:token.text??'',title:token.title??null,asset,target});break;
        }
        case 'list':out.push({type:'list',ordered:!!token.ordered,start:token.ordered?Number(token.start??1):null,
          items:(token.items??[]).map(item=>({type:'list_item',children:convert(item.tokens??[])}))});break;
        case 'table':out.push({type:'table',align:token.align??[],
          header:(token.header??[]).map(cell=>convert(cell.tokens??[])),
          rows:(token.rows??[]).map(row=>row.map(cell=>convert(cell.tokens??[])))});break;
        case 'html':diagnostics.push({page,kind:'unparsed_html',value:token.raw??''});
          out.push({type:'raw_html',raw:token.raw??''});break;
        default:diagnostics.push({page,kind:'unparsed_markdown',value:token.type});
          out.push({type:'unparsed',token:token.type,raw:token.raw??token.text??''});
      }
    }
    return out;
  };
  const nodes=compilePieces(markup.pieces,false);return {nodes,anchors:[...anchors]};
}

function listFiles(dir:string):string[] {
  return readdirSync(dir,{withFileTypes:true}).sort((a,b)=>a.name<b.name?-1:a.name>b.name?1:0).flatMap(entry=>{
    const file=join(dir,entry.name);return entry.isDirectory()?listFiles(file):entry.isFile()?[file]:[];
  });
}
const pages:Record<string,Page>={};
for(const file of listFiles(guideRoot).filter(f=>f.endsWith('.md'))){
  const id=relative(guideRoot,file).split('\\').join('/');const source=readFileSync(file,'utf8');
  const front=source.match(/^---\s*\r?\n([\s\S]*?)\r?\n---\s*(?:\r?\n|$)/);assert(front,`${id}: missing frontmatter`);
  const metadata:unknown=parseYaml(front[1]);assert(metadata&&typeof metadata==='object'&&!Array.isArray(metadata),`${id}: bad metadata`);
  const meta=metadata as Record<string,JsonValue>;const nav=meta.navigation;
  assert(nav&&typeof nav==='object'&&!Array.isArray(nav),`${id}: missing navigation`);
  const categories=meta.categories??[],itemIds=meta.item_ids??[];
  assert(Array.isArray(categories)&&categories.every(x=>typeof x==='string'));
  assert(Array.isArray(itemIds)&&itemIds.every(x=>typeof x==='string'));
  const compiled=compileBody(source.slice(front[0].length),id);
  pages[id]={id,source_sha256:createHash('sha256').update(source).digest('hex'),navigation:nav,
    categories,item_ids:itemIds.map(resourceId),metadata:meta,anchors:compiled.anchors,nodes:compiled.nodes};
}

const itemIndex:Record<string,string[]>={},categories:Record<string,string[]>={};
for(const page of Object.values(pages)){
  for(const id of page.item_ids)(itemIndex[id]??=[]).push(page.id);
  for(const category of page.categories)(categories[category]??=[]).push(page.id);
  const parent=page.navigation.parent;
  if(typeof parent==='string'&&!pages[parent])diagnostics.push({page:page.id,kind:'missing_navigation_parent',value:parent});
}
function inspectLinks(page:Page,value:unknown):void {
  if(!value||typeof value!=='object')return;
  if(Array.isArray(value)){for(const item of value)inspectLinks(page,item);return}
  const obj=value as Record<string,unknown>,target=obj.target;
  if(target&&typeof target==='object'&&!Array.isArray(target)){
    const t=target as Record<string,unknown>;
    if(t.kind==='page'&&typeof t.page==='string'){
      if(!pages[t.page])diagnostics.push({page:page.id,kind:'missing_page',value:t.page});
      else if(typeof t.anchor==='string'&&t.anchor&&!pages[t.page].anchors.includes(t.anchor))
        diagnostics.push({page:page.id,kind:'missing_anchor',value:`${t.page}#${t.anchor}`});
    }
    if(t.kind==='asset'&&typeof t.asset==='string'&&!existsSync(join(guideRoot,t.asset)))
      diagnostics.push({page:page.id,kind:'missing_asset',value:t.asset});
  }
  for(const [key,item] of Object.entries(obj))if(key!=='target')inspectLinks(page,item);
}
for(const page of Object.values(pages))inspectLinks(page,page.nodes);

// Every parsed component must survive Markdown conversion, including nested
// annotations. Counting at parse time alone can miss placeholders interpreted
// as indented code. Scene IDs and direct structure instructions are also part
// of the renderer contract.
const emitted:Record<string,number>={};const sceneIds=new Set<string>();
function inspectEmitted(value:unknown):void {
  if(!value||typeof value!=='object')return;
  if(Array.isArray(value)){for(const item of value)inspectEmitted(item);return}
  const obj=value as Record<string,unknown>;
  if(obj.type==='element'){
    assert(typeof obj.name==='string');emitted[obj.name]=(emitted[obj.name]??0)+1;
    if(obj.name==='GameScene'){
      assert(typeof obj.scene_id==='string'&&!sceneIds.has(obj.scene_id),'invalid or duplicate scene ID');
      sceneIds.add(obj.scene_id);
      assert(Array.isArray(obj.children)&&obj.children.filter((child:unknown)=>
        !!child&&typeof child==='object'&&(child as Record<string,unknown>).name==='ImportStructure').length===1,
        `${obj.scene_id}: expected one direct structure import`);
    }
  }
  for(const item of Object.values(obj)){
    if(typeof item==='string')assert(!/[\uE000\uE001]/.test(item),'unresolved markup placeholder');
    else inspectEmitted(item);
  }
}
for(const page of Object.values(pages))inspectEmitted(page.nodes);
for(const [name,n] of Object.entries(counts))assert.equal(emitted[name],n,`lost emitted ${name}`);
assert.equal(sceneIds.size,counts.GameScene);

// Verify the entire pinned source corpus and semantic components.
assert.equal(Object.keys(pages).length,125);assert(pages['index.md']);
for(const [name,n] of Object.entries({GameScene:139,ImportStructure:139,ItemLink:728,RecipeFor:217,Recipe:30,'ae2:ConfigValue':1}))assert.equal(counts[name],n,`lost ${name}`);
assert.equal(diagnostics.length,0,JSON.stringify(diagnostics));
const document={version:1,guide_id:'ae2:guide',root:'index.md',pages,item_index:itemIndex,categories,
  corpus:{pages:Object.keys(pages).length,components:counts,diagnostics}};
writeFileSync(output,JSON.stringify(document,null,2)+'\n');
console.log(JSON.stringify({output,pages:Object.keys(pages).length,components:counts,diagnostics},null,2));
