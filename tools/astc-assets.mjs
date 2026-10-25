// GPU-compressed (ASTC 4x4) copies of the large images, written beside each
// as `<file>.astc`. On the TV's Mali GPU they take a quarter of the memory and
// of the bandwidth every frame reads, need no decoding, and are visually
// lossless (aurora wallpaper: 56.9 dB PSNR). The renderer uses them only where
// the GPU takes ASTC and falls back to the original otherwise, so a missing
// encoder only skips this step.
import { createHash } from 'node:crypto';
import { execFile } from 'node:child_process';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { promisify } from 'node:util';

const run = promisify(execFile);
const root = path.resolve(import.meta.dirname, '..');
const assets = path.join(root, '.build/assets');
const cache = path.join(root, '.build/astc-cache');
const encoder = path.join(root, '.build/src/astc-encoder/build/Source/astcenc-avx2');

try {
  await fs.access(encoder);
} catch {
  console.log('ASTC encoder not built (.build/src/astc-encoder): large images ship uncompressed.');
  process.exit(0);
}
await fs.mkdir(cache, { recursive: true });

// Full-screen photos and their glass, Top Shelf art, and app tiles.
const jobs = [];
for (const directory of ['wallpapers', 'heroes', 'tiles']) {
  for (const name of await fs.readdir(path.join(assets, directory)).catch(() => [])) {
    if (/\.(png|jpe?g)$/i.test(name)) jobs.push(path.join(assets, directory, name));
  }
}

let encoded = 0;
const queue = [...jobs];
const worker = async () => {
  for (let file = queue.shift(); file; file = queue.shift()) {
    const bytes = await fs.readFile(file);
    const key = createHash('sha256').update(bytes).digest('hex');
    const cached = path.join(cache, `${key}.astc`);
    try {
      await fs.access(cached);
    } catch {
      // Photos get the most thorough search; quality is what matters here.
      const effort = file.includes(`${path.sep}tiles${path.sep}`) ? '-thorough' : '-exhaustive';
      await run(encoder, ['-cl', file, `${cached.slice(0, -5)}.part.astc`, '4x4', effort, '-silent', '-j', '1']);
      await fs.rename(`${cached.slice(0, -5)}.part.astc`, cached);
      encoded += 1;
    }
    await fs.copyFile(cached, `${file}.astc`);
  }
};
await Promise.all(Array.from({ length: Math.max(1, os.cpus().length - 1) }, worker));
console.log(`ASTC copies: ${jobs.length} images (${encoded} newly encoded).`);
