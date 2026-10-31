/** Check the selected checkout's HEAD without mutating it. */
import { existsSync, readFileSync, statSync } from 'node:fs';
import { dirname, isAbsolute, join, resolve } from 'node:path';

export const AE2_COMMIT = 'db17504a86128fdf3dae31f5fb7a112a646e0b93';
export const AE2_SOURCE = 'https://github.com/AppliedEnergistics/Applied-Energistics-2.git';

function gitDirectory(checkout: string): string {
  const marker = join(checkout, '.git');
  if (!existsSync(marker)) throw new Error('AE2_REFERENCE_DIR must be a Git checkout of the pinned source.');
  if (statSync(marker).isDirectory()) return marker;
  const text = readFileSync(marker, 'utf8').trim();
  if (!text.startsWith('gitdir: ')) throw new Error('Invalid AE2 checkout metadata.');
  const value = text.slice('gitdir: '.length);
  return isAbsolute(value) ? value : resolve(dirname(marker), value);
}

export function requirePinnedReference(checkout: string): void {
  const gitdir = gitDirectory(checkout);
  const commonFile = join(gitdir, 'commondir');
  const common = existsSync(commonFile) ? resolve(gitdir, readFileSync(commonFile, 'utf8').trim()) : gitdir;
  let value = readFileSync(join(gitdir, 'HEAD'), 'utf8').trim();
  for (let depth = 0; value.startsWith('ref: ') && depth < 8; depth++) {
    const ref = value.slice(5);
    if (!/^refs\/[A-Za-z0-9_./-]+$/.test(ref) || ref.includes('..')) throw new Error('Invalid AE2 checkout ref.');
    const local = join(gitdir, ref), shared = join(common, ref);
    if (existsSync(local) || existsSync(shared)) value = readFileSync(existsSync(local) ? local : shared, 'utf8').trim();
    else {
      const packed = join(common, 'packed-refs');
      const line = existsSync(packed) ? readFileSync(packed, 'utf8').split('\n').find(line => line.endsWith(` ${ref}`)) : undefined;
      if (!line) throw new Error('AE2 checkout HEAD could not be resolved.');
      value = line.split(' ')[0];
    }
  }
  if (value !== AE2_COMMIT) throw new Error(`AE2 checkout must be at ${AE2_COMMIT}; found ${value}.`);
}
