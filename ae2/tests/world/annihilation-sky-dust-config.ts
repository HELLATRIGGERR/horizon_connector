/** Sky-dust generation samples its setting when the physical plane is added. */
import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('annihilation-sky-dust-config', mode);
const dir = join(fixture.game, 'config'); mkdirSync(dir, { recursive: true });
const path = join(dir, 'ae2-common.toml');
const config = (enabled: boolean, ratio: number) => `[crafting]\nannihilationPlaneSkyDustGeneration=${enabled}\n[powerRatios]\nforgeEnergy=${ratio}\n`;
writeFileSync(path, config(true, 0.5));
try {
  const start = await fixture.start();
  const [px, , pz] = start.player.position;
  const x = Math.floor(px) + 6, z = Math.floor(pz) + 6, y = 319;
  const at = (dx: number, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const host = at(0), power = at(-1), drive = at(1), charger = at(3);
  const count = async () => ((await fixture.ae(`grid status ${drive}`)).items ?? [])
    .find((v: any) => v.item === 'ae2:sky_dust')?.count ?? 0;
  const capacity = () => fixture.ae(`power ${charger} east`);
  async function attachPlane() {
    await fixture.command(`setblock ${host} ae2:cable_bus`);
    assert.equal((await fixture.ae(`grid part ${host} up`)).attached, true);
  }
  await fixture.command(`tp @s ${x + 2.5} ${y - 2} ${z + 2.5}`);
  await fixture.command('clear @s'); await fixture.raw('hotbar 1');
  await fixture.replace('hotbar.0', 'ae2:annihilation_plane', 3);
  await fixture.select(0, 'ae2:annihilation_plane');
  await fixture.hold();
  await fixture.command(`setblock ${power} ae2:creative_energy_cell`);
  await fixture.command(`setblock ${drive} ae2:drive`);
  await fixture.command(`setblock ${charger} ae2:charger[facing=north]`);
  await fixture.command(`item replace block ${drive} container.0 with ae2:item_storage_cell_1k`);
  await attachPlane();
  await fixture.stepServer(40);
  await until(() => fixture.ae(`grid status ${drive}`), s => s.online && s.powered, 'height-limit drive online');
  const active = await fixture.ae(`grid part-status ${host} up`);
  assert(active.online && active.powered && active.channel, 'height-limit plane has its source grid channel');
  // The source accumulates 200 ticks, but IDLE schedules the next callback up
  // to 120 ticks later; allow for that callback and the initial grid boot.
  await fixture.stepServer(400);
  const enabledCount = await count();
  assert(enabledCount >= 1, 'an upward plane at the height limit deposits real sky dust');
  fixture.mark('an enabled, upward plane at the build limit deposits physical sky dust');

  writeFileSync(path, config(false, 2));
  await until(capacity, s => s.fe?.capacity === 800, 'disabled setting reloaded', 10000);
  await fixture.command(`setblock ${host} minecraft:air`);
  await attachPlane();
  await fixture.stepServer(450);
  assert.equal(await count(), enabledCount, 'a newly added plane does not generate with the setting disabled');
  fixture.mark('a new plane does not generate when the hot-reloaded setting is disabled');

  writeFileSync(path, config(true, 0.5));
  await until(capacity, s => s.fe?.capacity === 3200, 'enabled setting reloaded', 10000);
  await fixture.command(`setblock ${host} minecraft:air`);
  await attachPlane();
  await fixture.stepServer(400);
  const again = await count();
  assert(again > enabledCount, 'a newly added enabled plane resumes real dust generation');
  await fixture.hold(false); await fixture.restart();
  await until(() => fixture.ae(`grid status ${drive}`), s => s.online && s.powered,
    'height-limit drive online after restart', 30000);
  assert(await count() >= again, 'the generated dust survives process restart');
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('re-enabled generation deposits additional dust and stored output survives restart');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
