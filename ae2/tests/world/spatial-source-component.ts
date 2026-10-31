/** Source SpatialPlotInfo on a real spatial exchange, including legacy cells. */
import assert from 'node:assert/strict';
import { WorldFixture, until, type Mode } from './harness';

const mode: Mode = process.argv.includes('--in-frame') ? 'in-frame' : 'server-thread';
const fixture = new WorldFixture('spatial-source-component', mode);
try {
  const start = await fixture.start();
  const [px, py, pz] = start.player.position;
  const x = Math.floor(px), y = Math.ceil(py) + 8, z = Math.floor(pz);
  const at = (dx: number, dy = 0, dz = 0) => `${x + dx} ${y + dy} ${z + dz}`;
  const port = at(-1, -1, 1), box = at(1, 1, 1);
  const data = async (pos: string) => JSON.stringify(await fixture.command(`data get block ${pos}`));
  const assertPlotComponent = (output: string, plot: number) => {
    const fields = output.match(/ae2:spatial_plot_info[^{}]*\{([^{}]*)\}/)?.[1];
    assert(fields, `source plot component: ${output}`);
    assert.match(fields, new RegExp(`\\bid:\\s*${plot}\\b`));
    assert.match(fields, /size:\s*\[I;\s*2,\s*2,\s*2\]/);
  };
  const passed = async (command: string) => {
    const result = await fixture.command(command);
    assert.match((result.chat ?? []).join(' '), /^Test passed/);
  };
  await fixture.command(`fill ${at(-5, -2, -4)} ${at(7, 5, 7)} minecraft:air`);
  await fixture.command(`fill ${at(-5, -2, -4)} ${at(7, -2, 7)} minecraft:smooth_stone`);
  for (const [dx, dz] of [[0, 0], [0, 3], [3, 0], [3, 3]]) for (let dy = 0; dy < 4; dy++) {
    await fixture.command(`setblock ${at(dx, dy, dz)} ae2:spatial_pylon`);
  }
  for (let i = 0; i <= 3; i++) for (const [dx, dz] of [[i, 0], [i, 3], [0, i], [3, i]]) {
    await fixture.command(`setblock ${at(dx, -1, dz)} ae2:cable_bus`);
  }
  await fixture.command(`setblock ${at(-1, -1, 0)} ae2:controller`);
  await fixture.command(`setblock ${at(-2, -1, 0)} ae2:creative_energy_cell`);
  await fixture.command(`setblock ${port} ae2:spatial_io_port`);
  await fixture.command(`item replace block ${port} container.0 with ae2:spatial_storage_cell_2`);
  await until(() => fixture.ae(`grid status ${port}`), s => s.online && s.powered, 'powered spatial port', 30000);
  await fixture.command(`setblock ${box} minecraft:barrel`);
  await fixture.command(`item replace block ${box} container.0 with minecraft:emerald 7`);
  await fixture.command(`tp @s ${x - 3.5} ${y} ${z - 2.5}`);
  const first = await fixture.ae(`spatial transition ${port}`);
  assert.deepEqual(first.size, [2, 2, 2]);
  await passed(`execute if block ${box} minecraft:air`);
  let output = await data(port);
  assertPlotComponent(output, first.plot);
  assert(!output.includes('ae2_spatial_plot'), 'a newly formatted cell stores the source component directly');
  fixture.mark('physical exchange formats the cell with the source SpatialPlotInfo component');

  await fixture.restart();
  output = await data(port);
  assertPlotComponent(output, first.plot);
  await fixture.command(`item replace block ${port} container.0 from block ${port} container.1`);
  await fixture.command(`item replace block ${port} container.1 with minecraft:air`);
  // Exercise the actual source component argument and its codec too: replace
  // the single stack with an equivalent source-shaped plot reference.
  await fixture.command(`item replace block ${port} container.0 with ae2:spatial_storage_cell_2[ae2:spatial_plot_info={id:${first.plot},size:[I;2,2,2]},minecraft:custom_data={source_probe:51}]`);
  assert.match(await data(port), /source_probe.{0,4}51/, 'the source-component item command executed');
  await until(() => fixture.ae(`grid status ${port}`), s => s.online && s.powered, 'spatial port after restart', 30000);
  const second = await fixture.ae(`spatial transition ${port}`);
  assert.equal(second.plot, first.plot, 'the direct component resolves the original plot');
  await passed(`execute if block ${box} minecraft:barrel`);
  assert.match(await data(box), /minecraft:emerald.*count.{0,4}7|count.{0,4}7.*minecraft:emerald/);
  fixture.mark('a restarted source-component cell resolves the same plot and restores seven physical emeralds');

  // Seed the equivalent old native cell at the same plot, without retaining
  // an extra copy of the source cell in the test inventory.
  await fixture.command(`data merge block ${port} {Items:[{Slot:0b,id:"ae2:spatial_storage_cell_2",count:1,components:{"minecraft:custom_data":{ae2_spatial_plot:${first.plot},ae2_spatial_size:[I;2,2,2],marker:73}}}]}`);
  await until(() => fixture.ae(`grid status ${port}`), s => s.online && s.powered, 'legacy-cell port ready', 30000);
  const third = await fixture.ae(`spatial transition ${port}`);
  assert.equal(third.plot, first.plot, 'the old native wrapper still selects its original plot');
  output = await data(port);
  assertPlotComponent(output, first.plot);
  assert.match(output, /marker.{0,4}73/);
  await passed(`execute if block ${box} minecraft:air`);
  fixture.assertHidden(await fixture.raw('status'));
  fixture.mark('an old native cell reuses its plot and gains the real component without losing custom data');
  fixture.report('passed');
} catch (error) { fixture.report('failed', error); throw error; }
finally { await fixture.close(); }
