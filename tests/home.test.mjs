import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

// tv/home.cjs runs as root on the TV. Here it runs in a sandbox against a model TV: its files,
// mounts, processes and Luna services. Nothing touches this machine or a real TV.
const source = await fs.readFile(new URL('../tv/home.cjs', import.meta.url), 'utf8');
const APP = 'io.github.yashwanthkamireddi.lounge';
const APP_DIR = `/media/developer/apps/usr/palm/applications/${APP}`;
const EXE = '/usr/bin/com.webos.app.home';
const CHECKPOINT = '/tmp/checkpoint/com.webos.app.home';
const HOOK = '/var/lib/webosbrew/init.d/10-lounge-home';
const MARKER = '/var/run/lounge-home';
const CONFIG = '/var/lib/lounge/home.json';
const FILTER = '/var/lib/lounge/home-key-filter.js';
const LG_INODE = 100;
const LOUNGE_INODE = 200;
const GLASSHOUSE = { handler: 'handleGlasshouseKeys', file: '/var/lib/tvweb/remotekeys-filter.js' };
const OLD_FILTER = { handler: 'handleLoungeHome', file: FILTER };
const STOCK_FILTER = {
  handler: 'handleSystemKeys',
  file: '/usr/lib/qt5/qml/KeyFilters/systemUi.js',
};

function failure(code) {
  return Object.assign(new Error(code), { code });
}

function tv({
  front = 'com.webos.app.livetv',
  mounts = [],
  config = null,
  oldHome = true,
  normalRunning = false,
} = {}) {
  const files = new Map([
    [
      '/usr/palm/applications/com.webos.app.home/appinfo.json',
      JSON.stringify({
        id: 'com.webos.app.home',
        type: 'native',
        main: `file://${EXE}`,
        nativeLifeCycleInterfaceVersion: 2,
      }),
    ],
    [`${APP_DIR}/appinfo.json`, JSON.stringify({ id: APP, type: 'native', main: 'lounge' })],
    [`${APP_DIR}/lounge`, 'executable'],
    [`${CHECKPOINT}/MD5SUM`, 'sums'],
    [`${CHECKPOINT}/pages-1.img`, 'link into /mnt/lg/cmn_data'],
    ['/tmp/checkpoint/hash_check_com.webos.app.home', '0'],
    ['/mnt/lg/cmn_data/checkpoint/com.webos.app.home/pages-1.img.ori', 'pages'],
    ['/mnt/lg/cmn_data/checkpoint/com.webos.app.inputcommon/pages-1.img.ori', 'pages'],
    [FILTER, 'filter'],
    ['/var/lib/lounge/home-button.js', 'helper'],
  ]);
  if (config) files.set(CONFIG, JSON.stringify(config, null, 2));
  const directories = new Set([
    '/var/lib/webosbrew/init.d',
    CHECKPOINT,
    '/mnt/lg/cmn_data/checkpoint/com.webos.app.home',
    '/var/lib/lounge',
  ]);
  const links = new Map();
  const inodes = new Map([[`${APP_DIR}/lounge`, LOUNGE_INODE]]);
  const state = {
    front,
    mounts: [...mounts],
    processes: new Map([[1981, { argv0: EXE, inode: LG_INODE }]]),
    calls: [],
    chain: oldHome ? [GLASSHOUSE, OLD_FILTER, STOCK_FILTER] : [GLASSHOUSE, STOCK_FILTER],
    idleApp: { appId: oldHome ? 'com.ecklf.luma' : 'com.webos.app.home' },
    defaultApps: oldHome ? { home: APP } : null,
    mountTakes: true,
    normalRunning,
    nextPid: 3000,
  };
  const exeInode = () => {
    const top = state.mounts.filter((mount) => mount.target === EXE).at(-1);
    return top ? top.inode : LG_INODE;
  };
  const fakeFs = {
    readFileSync(file) {
      if (file === '/proc/self/mountinfo') {
        return state.mounts
          .map((mount, index) => `${30 + index} 20 179:54 ${mount.root} ${mount.target} rw - ext4`)
          .join('\n');
      }
      const proc = /^\/proc\/(\d+)\/cmdline$/.exec(file);
      if (proc) {
        const process = state.processes.get(Number(proc[1]));
        if (!process) throw failure('ENOENT');
        return `${process.argv0}\0{"appId":"com.webos.app.home"}\0`;
      }
      if (!files.has(file)) throw failure('ENOENT');
      return files.get(file);
    },
    lstatSync(file) {
      const proc = /^\/proc\/(\d+)$/.exec(file);
      if (
        proc
          ? state.processes.has(Number(proc[1]))
          : files.has(file) || directories.has(file) || links.has(file)
      ) {
        return {};
      }
      throw failure('ENOENT');
    },
    statSync(file) {
      const proc = /^\/proc\/(\d+)\/exe$/.exec(file);
      if (proc) {
        const process = state.processes.get(Number(proc[1]));
        if (!process) throw failure('ENOENT');
        return { dev: 1, ino: process.inode };
      }
      if (file === EXE) return { dev: 1, ino: exeInode() };
      if (!files.has(file)) throw failure('ENOENT');
      return { dev: 1, ino: inodes.get(file) ?? 1000 + [...files.keys()].indexOf(file) };
    },
    writeFileSync(file, text) {
      files.set(file, String(text));
    },
    renameSync(from, to) {
      files.set(to, files.get(from));
      files.delete(from);
    },
    unlinkSync(file) {
      if (links.delete(file)) return;
      if (!files.delete(file)) throw failure('ENOENT');
    },
    readdirSync(directory) {
      if (directory === '/proc') return [...state.processes.keys()].map(String).concat(['self']);
      return [...files.keys()]
        .filter((file) => file.startsWith(`${directory}/`))
        .map((file) => file.slice(directory.length + 1));
    },
    rmdirSync(directory) {
      directories.delete(directory);
    },
    symlinkSync(target, link) {
      links.set(link, target);
    },
    readlinkSync(link) {
      if (links.has(link)) return links.get(link);
      throw failure(files.has(link) ? 'EINVAL' : 'ENOENT');
    },
    realpathSync(file) {
      if (!files.has(`${file}/appinfo.json`)) throw failure('ENOENT');
      return file;
    },
  };
  const reply = (value) => ({ status: 0, stdout: JSON.stringify(value), stderr: '' });
  const luna = (uri, payload) => {
    const method = uri.replace(/^luna:\/\/[^/]+\//, '');
    if (method === 'getForegroundAppInfo') return { returnValue: true, appId: state.front };
    state.calls.push(payload.id ? `${method} ${payload.id}` : method);
    if (method === 'launch') {
      state.front = payload.id;
      return { returnValue: true };
    }
    if (method === 'running') {
      return { returnValue: true, running: state.normalRunning ? [{ id: APP }] : [] };
    }
    if (method === 'closeByAppId') {
      state.normalRunning = false;
      return { returnValue: true };
    }
    if (method === 'getConfigs') {
      return {
        returnValue: true,
        configs: {
          'com.webos.surfacemanager.keyFilters': state.chain,
          'profile.idleApp': state.idleApp,
        },
      };
    }
    if (method === 'setConfigs') {
      if (payload.configs['com.webos.surfacemanager.keyFilters']) {
        state.chain = payload.configs['com.webos.surfacemanager.keyFilters'];
      }
      if (payload.configs['profile.idleApp']) state.idleApp = payload.configs['profile.idleApp'];
      return { returnValue: true };
    }
    if (method === 'getSystemSettings') {
      return {
        returnValue: true,
        settings: state.defaultApps ? { defaultApps: state.defaultApps } : {},
      };
    }
    if (method === 'deleteSystemSettings') {
      state.defaultApps = null;
      return { returnValue: true };
    }
    throw new Error(`Unexpected Luna call ${uri}`);
  };
  const spawnSync = (file, args) => {
    if (file === '/bin/sleep') return { status: 0 };
    if (file === '/usr/bin/luna-send') return reply(luna(args[4], JSON.parse(args[5])));
    if (file === '/bin/mount') {
      state.calls.push(`mount ${args.join(' ')}`);
      if (state.mountTakes) {
        state.mounts.push({
          target: args[2],
          root: `/apps/usr/palm/applications/${APP}/lounge`,
          inode: inodes.get(args[1]),
        });
      }
      return { status: 0 };
    }
    if (file === '/bin/umount') {
      state.calls.push(`umount ${args.join(' ')}`);
      const index = state.mounts.findLastIndex((mount) => mount.target === args.at(-1));
      state.mounts.splice(index, 1);
      return { status: 0 };
    }
    throw new Error(`Unexpected command ${file}`);
  };
  const context = vm.createContext({
    require(name) {
      if (name === 'fs') return fakeFs;
      assert.equal(name, 'child_process');
      return { spawnSync };
    },
    module: { exports: {} },
    process: {
      kill(pid, signal) {
        state.calls.push(`kill ${pid} ${signal}`);
        state.processes.delete(pid);
      },
    },
    console: { log() {}, error() {} },
  });
  vm.runInContext(source, context, { timeout: 2000 });
  const home = context.module.exports;
  return {
    home,
    state,
    files,
    links,
    // Lounge started as Home by sam.
    startLoungeAsHome() {
      state.processes.set(state.nextPid++, { argv0: EXE, inode: exeInode() });
    },
    run: (...args) => home.main(['node', 'home.cjs', ...args]),
  };
}

const enabled = { enabled: true, method: 'executable', appDir: APP_DIR };
const ourMount = {
  target: EXE,
  root: `/apps/usr/palm/applications/${APP}/lounge`,
  inode: LOUNGE_INODE,
};

test('enable puts Lounge in Home place, removes the old Home Button and opens Home', () => {
  const { run, state, files, links, home } = tv({ normalRunning: true });
  run('enable');
  assert.deepEqual(state.calls, [
    'getConfigs',
    'setConfigs',
    'setConfigs',
    'getSystemSettings',
    'deleteSystemSettings',
    `mount --bind ${APP_DIR}/lounge ${EXE}`,
    'kill 1981 SIGTERM',
    'launch com.webos.app.home',
    'running',
    `closeByAppId ${APP}`,
  ]);
  // LG's own filters and Glasshouse's stay, in order; the Home settings are stock again.
  assert.deepEqual(state.chain, [GLASSHOUSE, STOCK_FILTER]);
  assert.deepEqual(state.idleApp, { appId: 'com.webos.app.home' });
  assert.equal(state.defaultApps, null);
  assert.ok(!files.has(FILTER) && !files.has('/var/lib/lounge/home-button.js'));
  const backup = JSON.parse(files.get('/var/lib/lounge/home-backup.json'));
  assert.equal(backup.idleApp.appId, 'com.ecklf.luma');
  assert.equal(backup.defaultApps.home, APP);
  // No image of LG's Home is left for sam to restore over Lounge, in RAM or persistent.
  assert.ok(![...files.keys()].some((file) => file.startsWith(`${CHECKPOINT}/`)));
  assert.ok(!files.has('/tmp/checkpoint/hash_check_com.webos.app.home'));
  assert.ok(!files.has('/mnt/lg/cmn_data/checkpoint/com.webos.app.home/pages-1.img.ori'));
  assert.ok(files.has('/mnt/lg/cmn_data/checkpoint/com.webos.app.inputcommon/pages-1.img.ori'));
  assert.equal(home.status().checkpoint, false);
  assert.equal(links.get(HOOK), '/var/lib/lounge/home.sh');
  assert.equal(files.get(MARKER), `${APP}\n`);
  assert.equal(JSON.parse(files.get(CONFIG)).enabled, true);
  assert.equal(state.front, 'com.webos.app.home');
  const status = home.status();
  assert.equal(status.mounted, true);
  assert.equal(status.hook, true);
  assert.equal(status.method, 'executable');
});

test('enable on a clean TV changes no settings', () => {
  const { run, state } = tv({ oldHome: false });
  run('enable');
  assert.deepEqual(state.calls.slice(0, 3), [
    'getConfigs',
    'getSystemSettings',
    `mount --bind ${APP_DIR}/lounge ${EXE}`,
  ]);
});

test('another integration over LG Home stops enable before anything changes', () => {
  for (const mount of [
    { target: '/usr/palm/applications/com.webos.app.home', root: '/tvweb/home-takeover', inode: 7 },
    { target: EXE, root: '/var/lib/other/launcher', inode: 7 },
  ]) {
    const { run, state, links, files } = tv({ mounts: [mount] });
    assert.throws(() => run('enable'), /Another integration/);
    assert.deepEqual(state.calls, []);
    assert.equal(links.size, 0);
    assert.ok(!files.has(MARKER));
  }
});

test('a mount that does not take is rolled back to stock LG Home', () => {
  const { run, state, links, files, home } = tv({ oldHome: false });
  state.mountTakes = false;
  assert.throws(() => run('enable'), /did not take/);
  assert.equal(links.size, 0);
  assert.ok(!files.has(MARKER));
  assert.equal(JSON.parse(files.get(CONFIG)).enabled, false);
  assert.equal(home.status().mounted, false);
  assert.equal(state.mounts.length, 0);
});

test('a cold boot opens Lounge over the input or Live TV the boot manager chose', () => {
  for (const front of ['com.webos.app.hdmi2', 'com.webos.app.livetv']) {
    const { run, state } = tv({ config: enabled, front });
    run('boot');
    assert.deepEqual(state.calls, [
      `mount --bind ${APP_DIR}/lounge ${EXE}`,
      'kill 1981 SIGTERM',
      'launch com.webos.app.home',
    ]);
    assert.equal(state.front, 'com.webos.app.home');
  }
});

test('a cold boot leaves an app someone opened in front', () => {
  const { run, state } = tv({ config: enabled, front: 'netflix' });
  run('boot');
  assert.deepEqual(state.calls, [`mount --bind ${APP_DIR}/lounge ${EXE}`, 'kill 1981 SIGTERM']);
  assert.equal(state.front, 'netflix');
});

test('the boot apply opens Lounge where LG Home was in front', () => {
  const { run, state } = tv({ config: enabled, front: 'com.webos.app.home' });
  run('boot');
  assert.deepEqual(state.calls, [
    `mount --bind ${APP_DIR}/lounge ${EXE}`,
    'kill 1981 SIGTERM',
    'launch com.webos.app.home',
  ]);
});

test('boot does nothing when disabled or when Lounge was removed', () => {
  for (const config of [null, { ...enabled, enabled: false }]) {
    const { run, state } = tv({ config });
    run('boot');
    assert.deepEqual(state.calls, []);
  }
  const removed = tv({ config: enabled });
  removed.files.delete(`${APP_DIR}/lounge`);
  removed.run('boot');
  assert.deepEqual(removed.state.calls, []);
});

test('applying again keeps a running Lounge Home', () => {
  const { run, state, startLoungeAsHome } = tv({
    config: enabled,
    mounts: [ourMount],
    front: 'com.webos.app.home',
  });
  state.processes.clear();
  startLoungeAsHome();
  run('apply');
  assert.deepEqual(state.calls, []);
  assert.equal(state.processes.size, 1);
});

test('after a package update the mount is stale, and applying mounts the new file', () => {
  const stale = {
    target: EXE,
    root: `/apps/usr/palm/applications/${APP}/lounge//deleted`,
    inode: 150,
  };
  const { run, state, home } = tv({ config: enabled, mounts: [stale] });
  state.processes.clear();
  assert.equal(home.status().stale, true);
  run('apply');
  assert.deepEqual(state.calls, [`umount ${EXE}`, `mount --bind ${APP_DIR}/lounge ${EXE}`]);
  assert.equal(home.status().mounted, true);
});

test('disable gives Home back to LG exactly', () => {
  const { run, state, links, files, startLoungeAsHome } = tv({
    config: enabled,
    mounts: [ourMount],
    front: 'com.webos.app.home',
  });
  state.processes.clear();
  startLoungeAsHome();
  links.set(HOOK, '/var/lib/lounge/home.sh');
  files.set(MARKER, `${APP}\n`);
  run('disable');
  assert.deepEqual(state.calls, [
    `umount ${EXE}`,
    'kill 3000 SIGTERM',
    'launch com.webos.app.home',
  ]);
  assert.equal(state.mounts.length, 0);
  assert.equal(links.size, 0);
  assert.ok(!files.has(MARKER));
  assert.equal(JSON.parse(files.get(CONFIG)).enabled, false);
});

test('disable leaves an app in front, and opens LG Home where Home itself had gone', () => {
  const app = tv({ config: enabled, mounts: [ourMount], front: 'youtube.leanback.v4' });
  app.run('disable');
  assert.ok(!app.state.calls.includes('launch com.webos.app.home'));
  const gone = tv({ config: enabled, mounts: [ourMount], front: '' });
  gone.run('disable');
  assert.ok(gone.state.calls.includes('launch com.webos.app.home'));
});

test('disable works without any record of an enable and never unmounts a foreign mount', () => {
  const { run } = tv();
  run('disable');
  const foreign = tv({ mounts: [{ target: EXE, root: '/var/lib/other/launcher', inode: 7 }] });
  assert.throws(() => foreign.run('disable'), /Another integration/);
  assert.equal(foreign.state.mounts.length, 1);
});

test('only a mount of Lounge executable is Lounge, also once replaced', () => {
  const { home } = tv();
  assert.ok(home.ours(`/apps/usr/palm/applications/${APP}/lounge`));
  assert.ok(home.ours(`/cryptofs/apps/usr/palm/applications/${APP}/lounge//deleted`));
  assert.ok(!home.ours('/var/lib/tvweb/home-takeover'));
  assert.ok(!home.ours(null));
});
