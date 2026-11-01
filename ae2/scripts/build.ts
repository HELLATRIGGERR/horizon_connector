import { engine, manifest, run, help, packFeatures } from './common';
if (!help('Usage: bun run ae2/scripts/build.ts [additional cargo build flags]\nBuilds the native AE2 client and horizon-ctl with the Rust toolchain the Horizon launcher pins (1.99.1),\nwith the Cargo features of the mods pack/pack.toml turns on (AE2 and its addons).')) {
  const extra = process.argv.slice(2);
  const jobs = extra.some(arg => arg === '-j' || arg.startsWith('-j') || arg.startsWith('--jobs')) ? [] : ['-j', '3'];
  await run(['cargo', 'build', '--manifest-path', manifest, '--locked', '--profile', 'fast', '-p', 'horizon-client', '--no-default-features', '--features', packFeatures(), '--bins', ...jobs, ...extra], engine);
}
