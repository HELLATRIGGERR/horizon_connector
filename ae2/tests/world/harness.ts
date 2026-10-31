/** Native gameplay checks with hidden windows, virtual input and isolated saves. */
import assert from 'node:assert/strict';
import { connect } from 'node:net';
import { createHash } from 'node:crypto';
import { appendFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { root, engine } from '../../scripts/common';
import { acquireRenderLease, awakeNowMs, awakeTimeout } from './background';

export type Mode = 'server-thread' | 'in-frame';
export async function until<T>(read: () => Promise<T>, accept: (value: T) => boolean, label: string, timeout = 15000): Promise<T> {
  const end = awakeNowMs() + timeout;
  let last: T | undefined;
  while (awakeNowMs() < end) { last = await read(); if (accept(last)) return last; await Bun.sleep(100); }
  throw new Error(`${label}: ${JSON.stringify(last)}`);
}

export class WorldFixture {
  readonly binary = resolve(process.env.AE2_TEST_BINARY ?? join(engine, 'target/fast/horizon-client'));
  readonly resources = resolve(process.env.AE2_RESOURCES ?? join(root, '.local/resources/26.3'));
  readonly content: string;
  readonly run: string;
  readonly game: string;
  readonly socket: string;
  private child?: ReturnType<typeof Bun.spawn>;
  private logs: Promise<void>[] = [];
  private release?: () => void;
  private phase = 0;
  readonly checks: string[] = [];

  constructor(readonly label: string, readonly mode: Mode = 'server-thread', options: { content?: string } = {}) {
    this.content = resolve(options.content ?? process.env.AE2_CONTENT_DIR ?? join(root, 'content/ae2'));
    assert(/^[a-z0-9-]+$/.test(label), 'fixture label must be a safe directory name');
    const tests = join(root, '.local/world-tests'); mkdirSync(tests, { recursive: true });
    this.run = mkdtempSync(join(tests, `${label}-`)); this.game = join(this.run, 'game');
    this.socket = join(process.platform === 'darwin' ? '/private/tmp' : '/tmp', `ae2-${process.pid}-${label.slice(0, 20)}.sock`);
    for (const name of ['game', 'launcher', 'data', 'config']) mkdirSync(join(this.run, name), { recursive: true });
    writeFileSync(join(this.game, 'options.txt'), 'renderDistance:4\nsimulationDistance:5\nmaxFps:30\nenableVsync:false\npauseOnLostFocus:false\nfullscreen:false\nonboardAccessibility:false\n');
  }

  async raw(line: string): Promise<any> {
    return new Promise((resolveReply, reject) => {
      let data = '', settled = false;
      const socket = connect({ path: this.socket });
      const finish = (error?: unknown, reply?: any) => {
        if (settled) return; settled = true; cancel(); socket.destroy();
        if (error) reject(error); else resolveReply(reply);
      };
      const cancel = awakeTimeout(() => finish(new Error(`Control timeout: ${line}`)), 45000);
      socket.on('connect', () => socket.write(line + '\n'));
      socket.on('error', error => finish(error));
      socket.on('close', () => { if (!settled) finish(new Error(`Control closed before reply: ${line}`)); });
      socket.on('data', bytes => {
        data += bytes.toString(); if (!data.includes('\n')) return;
        try {
          const reply = JSON.parse(data.trim());
          appendFileSync(join(this.run, 'transcript.jsonl'), JSON.stringify({ phase: this.phase, line, reply }) + '\n');
          finish(reply.error ? new Error(reply.error) : undefined, reply);
        } catch (error) { finish(error); }
      });
    });
  }

  assertHidden(status: any): void {
    assert.deepEqual(status.window, { offscreen: true, visible: false, focused: false, fullscreen: false, system_pointer_captured: false }, 'hidden test window isolation');
  }

  async start(): Promise<any> {
    for (const path of [this.binary, this.resources, this.content]) assert(existsSync(path), `Missing fixture prerequisite: ${path}`);
    this.release ??= await acquireRenderLease(); this.phase++;
    const binarySha = createHash('sha256').update(readFileSync(this.binary)).digest('hex');
    if (process.env.AE2_TEST_BINARY_SHA256) assert.equal(binarySha, process.env.AE2_TEST_BINARY_SHA256, 'fixture binary matches the selected build snapshot');
    writeFileSync(join(this.run, `runtime-${this.phase}.json`), JSON.stringify({ phase: this.phase, mode: this.mode, binary_sha256: binarySha, fixture_source_sha256: createHash('sha256').update(readFileSync(process.argv[1])).digest('hex'), harness_source_sha256: createHash('sha256').update(readFileSync(import.meta.path)).digest('hex'), background: true, max_fps: 30 }, null, 2) + '\n');
    const env = { ...process.env, HORIZON_HOME: this.game, HORIZON_LAUNCHER_HOME: join(this.run, 'launcher'), XDG_DATA_HOME: join(this.run, 'data'), XDG_CONFIG_HOME: join(this.run, 'config'), HORIZON_LAUNCHER_NO_SELF_UPDATE: '1', HORIZON_NO_AUDIO: '1', HORIZON_TELEMETRY_URL: 'off', HORIZON_SERVER_THREAD: this.mode === 'server-thread' ? '1' : '0' };
    this.child = Bun.spawn([this.binary, '--resources', this.resources, '--dev-mod', this.content, '--world', 'AE2 Fixture', '--seed', '1234', '--creative', '--name', 'AE2Fixture', '--render-distance', '4', '--width', '960', '--height', '540', '--control-socket', this.socket, '--background', '--no-pause', this.mode === 'server-thread' ? '--server-thread' : '--in-frame'], { cwd: this.game, env, stdout: 'pipe', stderr: 'pipe' });
    const log = join(this.run, `game-${this.phase}.log`);
    this.logs = [this.child.stdout, this.child.stderr].map(async stream => { if (stream) for await (const bytes of stream) appendFileSync(log, bytes); });
    try {
      return await until(async () => {
        if (this.child?.exitCode !== null) throw new Error(`Client exited; see ${log}`);
        let status;
        try { status = await this.raw('status'); }
        catch (error) { if (/ENOENT|ECONNREFUSED/.test(String(error))) return {}; throw error; }
        this.assertHidden(status);
        if (status.player && status.screen === 'advancements') await this.raw('close');
        return status;
      }, status => !!status.player && status.player.health > 0 && status.chunks >= 30 && status.server?.loaded !== false, 'hidden world startup', 180000);
    } catch (error) { await this.close(); throw error; }
  }

  async command(command: string): Promise<any> {
    const reply = await this.raw('command ' + command); assert.notEqual(reply.ok, false, JSON.stringify(reply));
    if (this.mode === 'in-frame' && reply.chat) reply.chat = reply.chat.slice(0, 1);
    if (/^(?:execute in \S+ run )?(?:fill|setblock|item replace block|data (?:merge|modify|remove) block)\b/.test(command)) {
      assert(!reply.chat?.some((line: string) => line.includes('That position is not loaded')), `Unloaded setup write: ${command}: ${JSON.stringify(reply.chat)}`);
    }
    const teleport = command.match(/^(?:execute in (\S+) run )?tp @s (-?[\d.]+) (-?[\d.]+) (-?[\d.]+)$/);
    if (teleport) {
      const dimension = teleport[1], [x, y, z] = teleport.slice(2).map(Number);
      await until(() => this.raw('status'), status => status.player?.health > 0 && status.server?.loaded !== false && (!dimension || status.dimension === dimension) && Math.abs(status.player.position[0] - x) < 0.2 && Math.abs(status.player.position[1] - y) < 2 && Math.abs(status.player.position[2] - z) < 0.2, 'authoritative teleport reaches the client');
    }
    return reply;
  }
  async step(ticks: number): Promise<void> {
    assert(Number.isInteger(ticks) && ticks > 0);
    const before = await this.raw('simulation'); await this.command(`tick step ${ticks}`);
    await until(() => this.raw('simulation'), state => state.game_time >= before.game_time + ticks, `authoritative ${ticks} simulation ticks`, Math.max(15000, ticks * 100));
  }
  async hold(held = true): Promise<void> { await this.raw(`server-hold ${held ? 'on' : 'off'}`); }
  async stepServer(ticks = 1): Promise<void> {
    assert(Number.isInteger(ticks) && ticks >= 1 && ticks <= 1000);
    const reply = await this.raw(`server-step ${ticks}`);
    assert.equal(reply.steps, ticks); assert.equal(reply.held, true);
  }
  async ae(command: string): Promise<any> {
    const reply = await this.command('ae2 ' + command);
    const line = reply.chat?.findLast((text: string) => text.startsWith('AE2 {'));
    assert(line, `AE2 command failed: ${JSON.stringify(reply)}`); return JSON.parse(line.slice(4));
  }
  async replace(slot: string, item: string, count = 1): Promise<void> {
    await this.command(`item replace entity @s ${slot} with ${item} ${count}`);
    const hotbar = slot.match(/^hotbar\.(\d+)$/);
    await until(() => this.raw('inventory'), view => {
      const stack = slot === 'weapon.offhand' ? view.offhand : hotbar ? view.hotbar[Number(hotbar[1])] : undefined;
      return item === 'minecraft:air' ? !stack?.item || stack.item === item || stack.count === 0 : stack?.item === item.split('[')[0] && stack.count === count;
    }, `authoritative inventory ${slot}`);
  }
  async select(index: number, item: string): Promise<void> {
    await this.raw(`hotbar ${index + 1}`);
    await until(() => this.command('data get entity @s SelectedItem'), reply => (reply.chat ?? []).join('\n').includes(item), 'authoritative selected item');
  }
  mark(message: string): void { this.checks.push(message); console.log('PASS', message); }
  async stop(): Promise<void> {
    if (!this.child) return;
    const child = this.child;
    if (child.exitCode === null) {
      try { await this.raw('quit'); } catch {}
      const cancel = awakeTimeout(() => child.kill(), 20000);
      try { await child.exited; } finally { cancel(); }
    }
    await Promise.all(this.logs); this.child = undefined;
  }
  async restart(): Promise<any> { await this.raw('save'); await this.stop(); return this.start(); }
  async close(): Promise<void> { await this.stop(); this.release?.(); this.release = undefined; }
  report(status: 'passed' | 'failed', error?: unknown): void {
    writeFileSync(join(this.run, 'result.json'), JSON.stringify({ status, mode: this.mode, checks: this.checks, error: error === undefined ? undefined : String(error) }, null, 2) + '\n');
    console.log(status.toUpperCase(), this.label, this.mode, this.run);
  }
}
