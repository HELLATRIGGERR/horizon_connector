/** Construct the verification sample from the pinned corpus, never from shipped assets. */
import assert from 'node:assert/strict';
import {createHash} from 'node:crypto';
import {readFileSync,writeFileSync} from 'node:fs';
import {join,resolve} from 'node:path';

const root=resolve(import.meta.dir,'../../..');
const guide=resolve(process.env.AE2_GUIDE_ROOT??join(root,'.local/guidebook/ae2guide'));
const out=resolve(process.env.AE2_GUIDE_SAMPLE??join(root,'.local/guidebook/compiled-sample.json'));
const d=JSON.parse(readFileSync(join(guide,'compiled-guide.json'),'utf8'));
const page_id='items-blocks-machines/interface.md';
const sample={version:1,navigation_example:d.pages[page_id].navigation,page_id,
  link:d.pages['getting-started.md'].nodes[4].children[1],
  image:d.pages['index.md'].nodes[0].children[0],
  recipe:d.pages['items-blocks-machines/crystal_resonance_generator.md'].nodes[5],
  config:d.pages['items-blocks-machines/crystal_resonance_generator.md'].nodes[3].children[2],
  index_scene:d.pages['index.md'].nodes[7],nested_annotated_scene:d.pages[page_id].nodes[6]};
const bytes=JSON.stringify(sample,null,2)+'\n';
assert.equal(createHash('sha256').update(bytes).digest('hex'),'6abc978017c3509f41dcefdc136b49273540db0128dab0d93055590445961826','sample differs from pinned Guide corpus');
writeFileSync(out,bytes);
