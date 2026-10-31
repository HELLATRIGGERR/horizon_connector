import { engine, manifest, run, help } from './common';
if (!help('Usage: bun run ae2/scripts/build.ts [additional cargo build flags]\nBuilds the native AE2 client and horizon-ctl with Rust 1.95.0.')) {
  await run(['cargo', '+1.95.0', 'build', '--manifest-path', manifest, '--locked', '--profile', 'fast', '-p', 'horizon-client', '--no-default-features', '--features', 'ae2-lab', '--bins', '-j', '3', ...process.argv.slice(2)], engine);
}
