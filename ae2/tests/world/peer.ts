/** A necessary LAN peer shares its host fixture's already-held rendering lease. */
import { connect } from 'node:net';
import { appendFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { WorldFixture, until } from './harness';
import { awakeTimeout } from './background';

export async function startPeer(host: WorldFixture, address: string, name = 'AE2Observer', options: { language?: string; clientConfig?: string } = {}) {
  if (!/^127\.0\.0\.1:\d+$/.test(address) || !/^[A-Za-z0-9_]{1,24}$/.test(name)) throw new Error('The fixture peer requires a localhost address and safe player name.');
  if (options.language && !/^[a-z0-9_]{1,16}$/.test(options.language)) throw new Error('Invalid peer language.');
  const dir = join(host.run, 'peer-' + name), game = join(dir, 'game');
  for (const path of [game, join(dir, 'launcher'), join(dir, 'data'), join(dir, 'config')]) mkdirSync(path, { recursive: true });
  writeFileSync(join(game, 'options.txt'), 'renderDistance:4\nsimulationDistance:5\nmaxFps:30\nenableVsync:false\npauseOnLostFocus:false\nfullscreen:false\nonboardAccessibility:false\n' + (options.language ? `lang:${options.language}\n` : ''));
  if (options.clientConfig !== undefined) {
    mkdirSync(join(game, 'config'), { recursive: true });
    writeFileSync(join(game, 'config', 'ae2-client.toml'), options.clientConfig);
  }
  const socket = join(process.platform === 'darwin' ? '/private/tmp' : '/tmp', `ae2-peer-${process.pid}-${name}.sock`);
  const child = Bun.spawn([host.binary, '--resources', host.resources, '--dev-mod', host.content, '--name', name, '--render-distance', '4', '--width', '960', '--height', '540', '--control-socket', socket, '--background', '--no-pause'], {
    cwd: game, env: { ...process.env, HORIZON_HOME: game, HORIZON_LAUNCHER_HOME: join(dir, 'launcher'), XDG_DATA_HOME: join(dir, 'data'), XDG_CONFIG_HOME: join(dir, 'config'), HORIZON_LAUNCHER_NO_SELF_UPDATE: '1', HORIZON_NO_AUDIO: '1', HORIZON_TELEMETRY_URL: 'off' }, stdout: 'pipe', stderr: 'pipe',
  });
  const logTasks = [child.stdout, child.stderr].map(async stream => { if (stream) for await (const bytes of stream) appendFileSync(join(dir, 'game.log'), bytes); });
  const raw = (line: string): Promise<any> => new Promise((resolveReply, reject) => {
    let data = '', settled = false;
    const client = connect({ path: socket });
    const finish = (error?: unknown, reply?: any) => {
      if (settled) return; settled = true; cancel(); client.destroy();
      if (error) reject(error); else resolveReply(reply);
    };
    const cancel = awakeTimeout(() => finish(new Error('Peer control timeout: ' + line)), 45000);
    client.on('connect', () => client.write(line + '\n'));
    client.on('error', error => finish(error));
    client.on('close', () => { if (!settled) finish(new Error('Peer control closed before reply: ' + line)); });
    client.on('data', bytes => {
      data += bytes.toString(); if (!data.includes('\n')) return;
      try {
        const reply = JSON.parse(data.trim());
        appendFileSync(join(dir, 'transcript.jsonl'), JSON.stringify({ line, reply }) + '\n');
        finish(reply.error ? new Error(reply.error) : undefined, reply);
      } catch (error) { finish(error); }
    });
  });
  const stop = async () => {
    if (child.exitCode === null) {
      try { await raw('quit'); } catch {}
      const cancel = awakeTimeout(() => child.kill(), 20000);
      try { await child.exited; } finally { cancel(); }
    }
    await Promise.all(logTasks);
  };
  try {
    await until(async () => {
      if (child.exitCode !== null) throw new Error('Peer exited during startup; see ' + dir);
      try { const state = await raw('status'); host.assertHidden(state); return state; }
      catch (error) { if (/ENOENT|ECONNREFUSED/.test(String(error))) return {}; throw error; }
    }, state => !!state.screen, 'hidden peer title', 180000);
    await raw(`connect ${address} ${name}`);
    await until(async () => { const state = await raw('status'); host.assertHidden(state); return state; }, state => state.player?.health > 0 && state.chunks >= 20, 'hidden peer joins actual LAN world', 180000);
    return { raw, stop, dir, name };
  } catch (error) { await stop(); throw error; }
}
