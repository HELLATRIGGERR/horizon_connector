import { resolve, join } from 'node:path';
export const root = resolve(import.meta.dir, '../..');
export const engine = root;
export const manifest = join(engine, 'Cargo.toml');
export async function run(command: string[], cwd = root, env = process.env): Promise<void> {
  const child = Bun.spawn(command, { cwd, env, stdin: 'inherit', stdout: 'inherit', stderr: 'inherit' });
  const code = await child.exited;
  if (code !== 0) throw new Error(`${command[0]} exited with status ${code}`);
}
export function help(message: string): boolean {
  if (!process.argv.slice(2).some(arg => arg === '--help' || arg === '-h')) return false;
  console.log(message);
  return true;
}
