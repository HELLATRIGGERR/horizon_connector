import { engine, manifest, run, help } from './common';
if (!help('Usage: bun run ae2/scripts/build.ts [additional cargo build flags]\nBuilds the native AE2 client and horizon-ctl with the Rust toolchain the Horizon launcher pins (1.99.1).')) {
  await run(['cargo', 'build', '--manifest-path', manifest, '--locked', '--profile', 'fast', '-p', 'horizon-client', '--no-default-features', '--features', 'ae2-lab', '--bins', '-j', '3', ...process.argv.slice(2)], engine);
}
