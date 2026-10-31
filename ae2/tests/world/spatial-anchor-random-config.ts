/** Owned remote chunks keep scheduled ticks; their random-tick gate is configurable. */
import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('spatial-anchor-random-config', mode);
const dir = join(fixture.game, 'config'); mkdirSync(dir, { recursive: true });
const path = join(dir, 'ae2-common.toml');
const config = (enabled: boolean, ratio: number) => `[general]\nspatialAnchorEnableRandomTicks=${enabled}\n[powerRatios]\nforgeEnergy=${ratio}\n`;
writeFileSync(path, config(false, 0.5));
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px / 16) * 16, z = Math.floor(pz / 16) * 16, y = Math.ceil(py) + 4;
  const at = (dx: number, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const anchor = at(2, 0, 2), power = at(3, 0, 2), furnace = at(4, 0, 2), charger = at(5, 0, 2);
  const sectionY = Math.floor(y / 16) * 16 + 16;
  const sectionHalvesEqual = async () => {
    const result = await fixture.command(`execute if blocks ${x} ${sectionY} ${z} ${x + 7} ${sectionY + 15} ${z + 15} ${x + 8} ${sectionY} ${z} all`);
    return (result.chat ?? []).some((line: string) => line.includes('Test passed'));
  };
  await fixture.command(`fill ${at(0, -1, 0)} ${at(12, -1, 8)} minecraft:stone`);
  await fixture.command(`fill ${at(0, 0, 0)} ${at(12, 4, 8)} minecraft:air`);
  await fixture.command('gamerule randomTickSpeed 0');
  await fixture.command(`setblock ${anchor} ae2:spatial_anchor`);
  await fixture.command(`setblock ${power} ae2:creative_energy_cell`);
  await fixture.command(`setblock ${charger} ae2:charger[facing=north]`);
  await until(() => fixture.ae(`anchor status ${anchor}`), s => s.count >= 1, 'physical anchor owns a chunk', 30000);
  await fixture.command('gamemode spectator');
  await fixture.command(`tp @s ${x + 2048.5} 180 ${z + 0.5}`);
  await fixture.hold();
  await fixture.command(`setblock ${furnace} minecraft:furnace[facing=north]`);
  await fixture.command(`data merge block ${furnace} {Items:[{Slot:0b,id:"minecraft:sand",count:1},{Slot:1b,id:"minecraft:coal",count:1}]}`);
  await fixture.stepServer(220);
  const smelted = await fixture.command(`data get block ${furnace}`);
  assert.match(JSON.stringify(smelted), /minecraft:glass/, 'remote furnace keeps ordinary ticking while anchor random ticks are disabled');
  fixture.mark('a remote anchor-owned furnace ticks with random ticks disabled');

  // Every position in this complete 16x16x16 section is a random-tickable
  // lit redstone ore block. Exactly one random selection must therefore turn
  // off exactly one block and make the initially identical halves differ.
  await fixture.command(`fill ${x} ${sectionY} ${z} ${x + 15} ${sectionY + 15} ${z + 15} minecraft:redstone_ore[lit=true]`);
  assert(await sectionHalvesEqual());
  await fixture.command('gamerule randomTickSpeed 1');
  await fixture.stepServer(1);
  assert(await sectionHalvesEqual(), 'disabled anchor keeps the entire remote section unchanged');
  fixture.mark('a full remote section receives no random selection when the anchor setting is disabled');

  writeFileSync(path, config(true, 2));
  await until(() => fixture.ae(`power ${charger} east`), s => s.fe?.capacity === 800, 'anchor setting reloaded', 10000);
  await fixture.stepServer(1);
  assert(!(await sectionHalvesEqual()), 'one guaranteed selection changes the full random-tickable section');
  await fixture.command('gamerule randomTickSpeed 0');
  await fixture.hold(false); await fixture.restart();
  await until(() => fixture.ae(`anchor status ${anchor}`), s => s.count >= 1, 'owned remote chunk restored', 30000);
  assert(!(await sectionHalvesEqual()), 'the physical redstone-ore state change survives restart');
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('one enabled random selection changes a real block and survives restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
