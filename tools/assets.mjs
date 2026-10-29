import fs from 'node:fs/promises';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import sharp from 'sharp';
import { generateVectorAssets } from './vector-assets.mjs';

const root = path.resolve(import.meta.dirname, '..');
const source = path.join(root, 'assets');
const out = path.join(root, '.build/assets');
const highResolution = process.argv.includes('--4k');
for (const directory of [
  'wallpapers',
  'thumbnails',
  'logos',
  'app-icons',
  'ui-icons',
  'surfaces',
]) {
  await fs.mkdir(path.join(out, directory), { recursive: true });
}

// Font outputs are rebuilt from scratch, so a face no longer generated never ships.
for (const file of await fs.readdir(out)) {
  if (file.startsWith('font')) await fs.rm(path.join(out, file));
}
// Apple's SF Pro when the owner keeps a copy in untracked local/fonts (it is
// licensed for Apple platforms, so it is never committed); otherwise Inter
// (OFL), cut from the official variable font at its text optical size to
// Latin and Latin Extended, so accented names (Łódź, Pokémon) render.
const sfPro = path.join(root, 'local/fonts/SF-Pro.ttf');
const useSfPro = await fs.access(sfPro).then(
  () => true,
  () => false,
);
if (!useSfPro) {
  const font = path.join(out, 'font.woff2');
  await fs.copyFile(path.join(source, 'fonts/inter.woff2'), font);
  const decoded = spawnSync('woff2_decompress', [font], { encoding: 'utf8' });
  if (decoded.error || decoded.status !== 0) {
    throw new Error('Font conversion needs woff2_decompress. Run pnpm assets inside nix develop.', {
      cause: decoded.error ?? new Error(decoded.stderr),
    });
  }
  await fs.rm(font);
}
const weights = spawnSync(
  'python3',
  [path.join(import.meta.dirname, 'font-weights.py'), out, ...(useSfPro ? [sfPro] : [])],
  { encoding: 'utf8' },
);
if (weights.error || weights.status !== 0) {
  throw new Error('Font weights need Python/fontTools. Run pnpm assets inside nix develop.', {
    cause: weights.error ?? new Error(weights.stderr),
  });
}
if (useSfPro) {
  await fs.writeFile(
    path.join(out, 'font-license.txt'),
    'SF Pro is copyright Apple Inc. and licensed by Apple for its platforms. This copy was\n' +
      'supplied by the device owner from local/fonts for personal use; it is not part of Lounge.\n',
  );
} else {
  await fs.copyFile(path.join(source, 'fonts/ofl.txt'), path.join(out, 'font-license.txt'));
}
console.log(`Font: ${useSfPro ? 'SF Pro (local/fonts)' : 'Inter'}`);

for (const file of await fs.readdir(path.join(source, 'wallpapers'))) {
  if (!/\.(jpg|svg)$/.test(file)) continue;
  const id = path.parse(file).name;
  const input = path.join(source, 'wallpapers', file);
  for (const scale of highResolution ? [1, 2] : [1]) {
    const width = 1920 * scale;
    const height = 1080 * scale;
    // Match background-size:cover and each CSS background-position exactly.
    const photo = await sharp(input, { density: 72 * scale })
      .resize(width, height, { fit: 'outside' })
      .png()
      .toBuffer({ resolveWithObject: true });
    const position = id === 'dunes' || id === 'summit' ? 0.4 : id === 'san-francisco' ? 0.42 : 0.45;
    let image = sharp(photo.data).extract({
      left: Math.round((photo.info.width - width) / 2),
      top: Math.round((photo.info.height - height) * position),
      width,
      height,
    });
    const saturation =
      id === 'san-francisco'
        ? 0.75
        : id === 'summit'
          ? 0.7
          : ['forest', 'yosemite', 'northern-lights'].includes(id)
            ? 0.8
            : 1;
    const brightness = id === 'san-francisco' ? 0.82 : id === 'summit' ? 0.8 : 1;
    if (saturation !== 1) {
      const s = saturation;
      image = image.recomb([
        [0.213 + 0.787 * s, 0.715 - 0.715 * s, 0.072 - 0.072 * s],
        [0.213 - 0.213 * s, 0.715 + 0.285 * s, 0.072 - 0.072 * s],
        [0.213 - 0.213 * s, 0.715 - 0.715 * s, 0.072 + 0.928 * s],
      ]);
    }
    const data = await image.linear(brightness).png().toBuffer();
    if (scale === 1) {
      // Glass/backdrop blur from the clean photo (Apple blurs the image itself,
      // then veils it), not from the vignetted wallpaper, which goes near-black.
      await sharp(data)
        .resize(640, 360)
        .blur(12)
        // Lossless: JPEG blocks and banding show once a blur is stretched 3x.
        .png({ compressionLevel: 9 })
        .toFile(path.join(out, 'wallpapers', `${id}-glass.png`));
    }
    const shaded = `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 1920 1080"><defs>
    <linearGradient id="v" x2="0" y2="1"><stop stop-color="#0d1013" stop-opacity=".24"/><stop offset=".19" stop-color="#0e1115" stop-opacity=".08"/><stop offset=".35" stop-color="#0f1114" stop-opacity=".35"/><stop offset=".63" stop-color="#111317" stop-opacity=".86"/><stop offset=".85" stop-color="#17191d" stop-opacity=".92157"/><stop offset="1" stop-color="#17191d" stop-opacity=".96078"/></linearGradient>
    <linearGradient id="h"><stop stop-color="#111417" stop-opacity=".42"/><stop offset=".75" stop-color="#111417" stop-opacity="0"/></linearGradient></defs>
    <path fill="#17191c" d="M0 0h1920v1080H0z"/><image href="data:image/png;base64,${data.toString('base64')}" width="1920" height="1080" opacity=".92"/><path fill="url(#h)" d="M0 0h1920v1080H0z"/><path fill="url(#v)" d="M0 0h1920v1080H0z"/></svg>`;
    // This generated SVG embeds a trusted bundled photograph; its data URL can
    // exceed libxml's default 10 MiB attribute limit at 4K.
    const wallpaper = path.join(out, 'wallpapers', `${id}${scale === 2 ? '@2x' : ''}.jpg`);
    await sharp(Buffer.from(shaded), { unlimited: true }).jpeg({ quality: 96 }).toFile(wallpaper);
    if (scale === 1) {
      // Home glass (dock, time pill) blurs the wallpaper exactly as shown,
      // shading included, so it reads as glass over the photo, not a slab.
      await sharp(wallpaper)
        .resize(640, 360)
        .blur(12)
        .png({ compressionLevel: 9 })
        .toFile(path.join(out, 'wallpapers', `${id}-home-glass.png`));
    }
  }
  // The pre-blurred "glass" backdrop is written above from the clean photo:
  // panels sample it at their own screen position (no runtime blur).
  await sharp(input)
    .resize(480, 270, { fit: 'cover' })
    .jpeg({ quality: 82 })
    .toFile(path.join(out, 'thumbnails', `${id}.jpg`));
}
const icons = {};
const map = JSON.parse(await fs.readFile(path.join(source, 'app-icons.json'), 'utf8'));
for (const [id, relative] of Object.entries(map)) {
  const clean = relative.replace(/^\.\//, '');
  if (!/^app-icons\/[a-zA-Z0-9._-]+\.(png|jpg)$/.test(clean)) continue;
  try {
    await fs.copyFile(path.join(source, clean), path.join(out, clean));
    icons[id] = clean;
  } catch (error) {
    if (error.code !== 'ENOENT') throw error;
  }
}
await fs.writeFile(path.join(out, 'icons.json'), `${JSON.stringify(icons, null, 2)}\n`);

// Full-bleed tvOS card artwork named by exact app ID: the bundled set in
// assets/tiles/, then any owner overrides in untracked local/tiles/. Baked into
// the package (and ASTC), so every card is on screen from the first frame.
// Rendered at 2x the 270x154.4 card, as GLES2 has no mipmaps here.
const tiles = path.join(out, 'tiles');
await fs.rm(tiles, { recursive: true, force: true });
await fs.mkdir(tiles);
const tileColors = {};
const tileSources = new Map();
for (const directory of ['assets/tiles', 'local/tiles']) {
  for (const file of await fs.readdir(path.join(root, directory)).catch(() => [])) {
    const id = file.replace(/\.(png|jpe?g)$/i, '');
    if (id !== file && /^[a-zA-Z0-9._-]+$/.test(id))
      tileSources.set(id, path.join(root, directory, file));
  }
}
for (const [id, source] of tileSources) {
  const input = sharp(source);
  const { width, height } = await input.metadata();
  const [w, h] = [540, 309];
  // Square store icons keep the whole mark; their edge colour fills the wider card.
  const image =
    width / height > 1.2
      ? input.resize(w, h, { fit: 'cover', kernel: 'lanczos3' })
      : input.resize({ height: h, kernel: 'lanczos3' }).extend({
          left: Math.floor((w - h) / 2),
          right: Math.ceil((w - h) / 2),
          extendWith: 'copy',
        });
  await image.png({ compressionLevel: 9 }).toFile(path.join(tiles, `${id}.png`));
  const { dominant } = await sharp(path.join(tiles, `${id}.png`)).stats();
  tileColors[id] = [dominant.r, dominant.g, dominant.b];
}
await fs.writeFile(path.join(out, 'tiles.json'), `${JSON.stringify(tileColors, null, 2)}\n`);

// Top Shelf art named by app ID, as the tiles: the bundled set in
// assets/heroes/, then owner overrides in untracked local/heroes/.
// 1280x720 keeps each texture at 3.7 MB on the TV.
const heroes = path.join(out, 'heroes');
await fs.rm(heroes, { recursive: true, force: true });
await fs.mkdir(heroes);
const heroSources = new Map();
for (const directory of ['assets/heroes', 'local/heroes']) {
  for (const file of await fs.readdir(path.join(root, directory)).catch(() => [])) {
    const id = file.replace(/\.(png|jpe?g)$/i, '');
    if (id !== file && /^[a-zA-Z0-9._-]+$/.test(id))
      heroSources.set(id, path.join(root, directory, file));
  }
}
const heroIds = [...heroSources.keys()];
for (const [id, source] of heroSources) {
  await sharp(source)
    .resize(1280, 720, { fit: 'cover', kernel: 'lanczos3' })
    .jpeg({ quality: 88 })
    .toFile(path.join(heroes, `${id}.jpg`));
  // The dock's glass blurs whatever is behind it, so each hero has a backdrop.
  await sharp(path.join(heroes, `${id}.jpg`))
    .resize(640, 360)
    .blur(12)
    .png({ compressionLevel: 9 })
    .toFile(path.join(heroes, `${id}-glass.png`));
}
await fs.writeFile(path.join(out, 'heroes.json'), `${JSON.stringify(heroIds.sort())}\n`);
await fs.copyFile(path.join(source, 'icon-colours.json'), path.join(out, 'colors.json'));
await fs.writeFile(
  path.join(out, 'version.txt'),
  JSON.parse(await fs.readFile(path.join(root, 'appinfo.json'), 'utf8')).version,
);

const surfaces = {
  generic: ['#4e5c6b', '#243c4e'],
  'live-tv': ['#ff5a4e', '#c8102e'],
  netflix: ['#141414', '#141414'],
  youtube: ['#0f0f0f', '#0f0f0f'],
  'apple-tv': ['#252525', '#080808'],
  'disney-plus': ['#0b244c', '#091936'],
  'prime-video': ['#0786bd', '#005ba7'],
  arte: ['#242323', '#121212'],
  zdf: ['#121b22', '#121b22'],
  'das-erste': ['#064c84', '#00254e'],
  plex: ['#343434', '#232323'],
  spotify: ['#000000', '#121212'],
  jellyfin: ['#25283f', '#101422'],
  homebrew: ['#7c6042', '#3f392d'],
  kodi: ['#0a749b', '#08445e'],
  moonlight: ['#435d8f', '#252e59'],
  gaming: ['#435d8f', '#252e59'],
  browser: ['#3a8dff', '#0b4bd1'],
  'hbo-max': ['#181818', '#080808'],
  'paramount-plus': ['#075bda', '#00298e'],
  peacock: ['#171717', '#080808'],
  hulu: ['#152d23', '#091710'],
  crunchyroll: ['#141414', '#141414'],
  twitch: ['#6330b8', '#32165c'],
  dazn: ['#252623', '#10110e'],
  'rakuten-tv': ['#bf143a', '#620c20'],
  'apple-music': ['#252525', '#080808'],
  'amazon-music': ['#183c4a', '#102128'],
  'bbc-iplayer': ['#4a162e', '#230b16'],
  itvx: ['#24341b', '#111a0c'],
  'channel-4': ['#20362c', '#0c1a14'],
  'now-tv': ['#064c45', '#001e1a'],
  'rtl-plus': ['#262029', '#121014'],
  joyn: ['#000000', '#000000'],
  'waipu-tv': ['#154e8e', '#0a2544'],
  zattoo: ['#353031', '#151314'],
  skyshowtime: ['#24202e', '#100d17'],
  'coupang-play': ['#143b4a', '#091d25'],
  tving: ['#3f131d', '#1a090e'],
  wavve: ['#10399e', '#0a1e53'],
  watcha: ['#3d1225', '#1e0912'],
  'movistar-plus': ['#163b35', '#091e1a'],
  filmin: ['#10372f', '#091c18'],
};
for (const [brand, [top, bottom]] of Object.entries(surfaces)) {
  const fill =
    brand === 'disney-plus'
      ? '<radialGradient id="base" cx="50%" cy="110%" r="110%"><stop stop-color="#16466b"/><stop offset=".6" stop-color="#0b244c"/><stop offset="1" stop-color="#091936"/></radialGradient>'
      : `<linearGradient id="base" x2="1" y2="1"><stop stop-color="${top}"/><stop offset="1" stop-color="${bottom}"/></linearGradient>`;
  await sharp(
    Buffer.from(
      `<svg xmlns="http://www.w3.org/2000/svg" width="540" height="309"><defs>${fill}<linearGradient id="sheen" x2="1" y2="1"><stop stop-color="white" stop-opacity=".04"/><stop offset=".48" stop-color="white" stop-opacity="0"/></linearGradient></defs><path fill="url(#base)" d="M0 0h540v309H0z"/><path fill="url(#sheen)" d="M0 0h540v309H0z"/></svg>`,
    ),
  )
    .png()
    .toFile(path.join(out, 'surfaces', `${brand}.png`));
}
await generateVectorAssets();
// LG's app list rounds an app's icon itself and expects it full-bleed and opaque, as the
// store apps ship theirs: the icon drops the logo's own rounded tile (rounded twice, it read as a
// smaller square inside LG's card) and the sofa fills more of it, as other apps' marks do.
const appIcon = Buffer.from(
  (await fs.readFile(path.join(source, 'app-icon.svg'), 'utf8'))
    .replace('<g clip-path="url(#tile)">', '<g>')
    .replace('scale(1.1)', 'scale(1.32)'),
);
await sharp(appIcon, { density: 300 }).resize(80, 80).png().toFile(path.join(out, 'app-icon.png'));
await sharp(appIcon, { density: 300 })
  .resize(130, 130)
  .png()
  .toFile(path.join(out, 'app-icon-large.png'));
console.log(`Native artwork ready: ${path.relative(root, out)}`);
