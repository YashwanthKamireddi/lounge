import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

// tv/home.sh as the TV runs it, in a temporary root: the copied script behind the boot hook's
// symlink, a stand-in for Lounge's executable that records how it is called, stubs for the
// root-only commands (id, mount, umount, luna-send), a model /proc and mountinfo. Real
// processes stand in for the Home the script stops. The Home logic itself (src/home-mount.rs)
// is tested in Rust; nothing here touches this machine's mounts or a real TV.
const source = await fs.readFile(new URL('../tv/home.sh', import.meta.url), 'utf8');
const APP = 'io.github.yashwanthkamireddi.lounge';
const EXE = '/usr/bin/com.webos.app.home';
const FULL_HOME = 'com.webos.app.fullhome';
const FULL_EXE = `/usr/bin/${FULL_HOME}`;
const HOME_DIR = '/usr/palm/applications/com.webos.app.home';
const STAGED_MOUNT = `31 20 179:54 /lib/lounge/app ${HOME_DIR} rw - ext4`;

const HELPER_OK = `printf '%s\\n' '{"returnValue":true,"method":"executable","enabled":false}'`;

async function fixture({
  helper = HELPER_OK,
  enabled = true,
  relative = false,
  mounted = true,
  foreign = false,
  busy = false,
  front = 'com.webos.app.home',
  method = 'executable',
  homeId = null,
  staged = false,
  samReadsLounge = false,
  forceBlock = false,
} = {}) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'lounge-home-script-'));
  const dir = (name) => path.join(root, name);
  const state = dir('state');
  const app = dir(`apps/${APP}`);
  const calls = dir('calls');
  const mountinfo = dir('mountinfo');
  for (const name of ['state', `apps/${APP}`, 'bin', 'init.d', 'proc', 'run']) {
    await fs.mkdir(dir(name), { recursive: true });
  }
  let script = source;
  for (const [name, value] of Object.entries({
    STATE: state,
    HOOK: dir('init.d/10-lounge-home'),
    MARKER: dir('run/lounge-home'),
    MOUNTS: mountinfo,
    PROC: dir('proc'),
    LOCK: dir('lock'),
    BOOT_LOG: dir('boot.log'),
    LUNA: dir('bin/luna-send'),
    TARGET_MARKER: dir('run/lounge-home-target'),
    STAGE: dir('stage'),
    SCHEMA: dir('schema'),
    DIRS: `'${dir('missing')} ${app}'`,
  })) {
    const line = new RegExp(`^${name}=.*$`, 'm');
    assert.match(script, line, name);
    script = script.replace(line, `${name}=${value}`);
  }
  await fs.writeFile(path.join(state, 'home.sh'), script, { mode: 0o755 });
  await fs.writeFile(
    path.join(state, 'home.json'),
    `${JSON.stringify({ enabled, method, appDir: app, ...(homeId ? { homeId } : {}) }, null, 2)}\n`,
  );
  await fs.writeFile(dir('run/lounge-home'), `${APP}\n`);
  if (homeId) await fs.writeFile(dir('run/lounge-home-target'), `${homeId}\n`);
  await fs.writeFile(dir('sam'), samReadsLounge ? `${HOME_DIR}/lounge` : `file://${EXE}`);
  if (forceBlock) {
    await fs.writeFile(dir('schema'), '{"properties": {"id": {}, "forceBlockSystemApp": {}}}');
  }
  await fs.writeFile(dir('front'), front);
  if (busy) await fs.writeFile(dir('busy'), '');
  const record = `printf '%s' "$(basename "$0")" >>'${calls}'; for a; do printf ' [%s]' "$a"; done >>'${calls}'; echo >>'${calls}'`;
  if (helper !== null) {
    await fs.writeFile(path.join(app, 'lounge'), `#!/bin/sh\n${record}\n${helper}\n`, {
      mode: 0o755,
    });
  }
  const lines = [];
  const exe = homeId ? `/usr/bin/${homeId}` : EXE;
  if (foreign) lines.push(`31 20 179:54 /var/lib/other/launcher ${exe} rw - ext4`);
  else if (mounted && !staged)
    lines.push(`31 20 179:54 /apps/usr/palm/applications/${APP}/lounge ${exe} rw - ext4`);
  if (staged) lines.push(STAGED_MOUNT);
  await fs.writeFile(mountinfo, lines.map((line) => `${line}\n`).join(''));
  for (const [name, body] of [
    ['id', 'echo 0'],
    ['sleep', 'exit 0'],
    ['node', `${record}\nexit 1`],
    ['mount', `${record}`],
    [
      'umount',
      `${record}\n[ "$1" != -l ] && [ -e '${dir('busy')}' ] && exit 32\n` +
        `for t; do :; done\n` +
        `awk -v t="$t" '$5 != t' '${mountinfo}' >'${mountinfo}.new'; mv '${mountinfo}.new' '${mountinfo}'`,
    ],
    [
      'luna-send',
      `${record}\ncase "$5" in\n` +
        `  */getForegroundAppInfo) printf '{\\n  "returnValue": true,\\n  "appId": "%s"\\n}\\n' "$(cat '${dir('front')}')" ;;\n` +
        `  */getAppInfo) printf '{\\n  "returnValue": true,\\n  "appInfo": {\\n    "main": "%s"\\n  }\\n}\\n' "$(cat '${dir('sam')}')" ;;\n` +
        // sam reads the manifest that is there now.
        `  */releaseBlockSystemApp) grep -q ' ${HOME_DIR} ' '${mountinfo}' || printf '%s' 'file://${EXE}' >'${dir('sam')}'\n` +
        `    printf '{"returnValue": true}\\n' ;;\n` +
        `  *) printf '{"returnValue": true}\\n' ;;\nesac`,
    ],
  ]) {
    await fs.writeFile(dir(`bin/${name}`), `#!/bin/sh\n${body}\n`, { mode: 0o755 });
  }
  const hook = dir('init.d/10-lounge-home');
  await fs.symlink(relative ? '../state/home.sh' : path.join(state, 'home.sh'), hook);
  // Processes in the model /proc are real ones, so the script's signals land somewhere.
  const processes = [];
  const start = async (argv0, ...rest) => {
    const started = spawnSync('/bin/sh', ['-c', '/usr/bin/sleep 60 >/dev/null 2>&1 & echo $!'], {
      encoding: 'utf8',
    });
    const pid = Number(started.stdout.trim());
    processes.push(pid);
    await fs.mkdir(dir(`proc/${pid}`));
    const args = rest.length ? rest : ['{"appId":"com.webos.app.home"}'];
    await fs.writeFile(dir(`proc/${pid}/cmdline`), [argv0, ...args].map((a) => `${a}\0`).join(''));
    return pid;
  };
  const alive = (pid) => {
    try {
      process.kill(pid, 0);
      return true;
    } catch {
      return false;
    }
  };
  const run = (...args) =>
    spawnSync('/bin/sh', [hook, ...args], {
      encoding: 'utf8',
      env: { ...process.env, PATH: `${dir('bin')}:${process.env.PATH}` },
      timeout: 30000,
    });
  const recorded = async () =>
    (await fs.readFile(calls, 'utf8').catch(() => '')).trim().split('\n').filter(Boolean);
  const settle = async (count) => {
    for (let i = 0; i < 200; i++) {
      if ((await recorded()).length >= count) break;
      await new Promise((resolve) => setTimeout(resolve, 25));
    }
    return recorded();
  };
  const exists = (name) =>
    fs.lstat(name).then(
      () => true,
      () => false,
    );
  const cleanup = async () => {
    for (const pid of processes) if (alive(pid)) process.kill(pid, 'SIGKILL');
    await fs.rm(root, { recursive: true, force: true });
  };
  return {
    root,
    state,
    app,
    hook,
    mountinfo,
    dir,
    run,
    recorded,
    settle,
    start,
    alive,
    exists,
    cleanup,
  };
}

for (const relative of [false, true]) {
  test(`status through the ${relative ? 'relative' : 'absolute'} boot-hook symlink runs Lounge`, async () => {
    const tv = await fixture({ relative });
    try {
      const status = tv.run('status');
      assert.equal(status.status, 0, status.stderr);
      assert.equal(JSON.parse(status.stdout).returnValue, true);
      assert.deepEqual(await tv.recorded(), ['lounge [--home] [status] []']);
    } finally {
      await tv.cleanup();
    }
  });
}

for (const relative of [false, true]) {
  test(`a cold boot through the ${relative ? 'relative' : 'absolute'} hook symlink starts the boot apply`, async () => {
    const tv = await fixture({ relative });
    try {
      const boot = tv.run();
      assert.equal(boot.status, 0, boot.stderr);
      // In the background, so other boot hooks never wait; its output goes to the boot log.
      assert.deepEqual(await tv.settle(1), ['lounge [--home] [boot]']);
      for (
        let i = 0;
        i < 200 && !(await fs.readFile(tv.dir('boot.log'), 'utf8').catch(() => ''));
        i++
      ) {
        await new Promise((resolve) => setTimeout(resolve, 25));
      }
      assert.match(await fs.readFile(tv.dir('boot.log'), 'utf8'), /"returnValue":true/);
    } finally {
      await tv.cleanup();
    }
  });
}

test('a boot does nothing when Lounge is not enabled or no longer installed', async () => {
  for (const options of [{ enabled: false }, { helper: null }]) {
    const tv = await fixture(options);
    try {
      const boot = tv.run('boot');
      assert.equal(boot.status, 0, boot.stderr);
      await new Promise((resolve) => setTimeout(resolve, 200));
      assert.deepEqual(await tv.recorded(), []);
    } finally {
      await tv.cleanup();
    }
  }
});

test('enable passes its package on, and a missing executable is reported before any change', async () => {
  const tv = await fixture();
  try {
    assert.equal(tv.run('enable', tv.app).status, 0);
    assert.deepEqual(await tv.recorded(), [`lounge [--home] [enable] [${tv.app}]`]);
  } finally {
    await tv.cleanup();
  }
  const missing = await fixture({ helper: null });
  try {
    const status = missing.run('status');
    assert.equal(status.status, 1);
    assert.match(status.stdout, /No installed native Lounge package found/);
    assert.equal(await missing.exists(missing.dir('run/lounge-home')), true);
  } finally {
    await missing.cleanup();
  }
});

test('disable is Lounge’s own when it answers, and the script then changes nothing', async () => {
  const tv = await fixture();
  try {
    const home = await tv.start(EXE);
    const disable = tv.run('disable');
    assert.equal(disable.status, 0, disable.stderr);
    assert.equal(disable.stdout, '{"returnValue":true,"method":"executable","enabled":false}\n');
    assert.doesNotMatch(disable.stderr, /restoring LG Home from the script/);
    assert.deepEqual(await tv.recorded(), ['lounge [--home] [disable]']);
    assert.equal(tv.alive(home), true);
    assert.match(await fs.readFile(tv.mountinfo, 'utf8'), /com\.webos\.app\.home/);
  } finally {
    await tv.cleanup();
  }
});

// Restoring LG's Home never depends on Lounge's executable working.
for (const [name, helper] of [
  ['missing', null],
  ['crashing', 'echo "error while loading shared libraries" >&2; exit 127'],
  ['refusing', `printf '%s\\n' '{"returnValue":false,"errorText":"bug"}'; exit 1`],
  ['garbled', `printf 'Segmentation fault'`],
]) {
  test(`disable with a ${name} executable gives Home back to LG from the script`, async () => {
    const tv = await fixture({ helper, busy: name === 'crashing' });
    try {
      const home = await tv.start(EXE);
      const other = await tv.start('/usr/bin/sam');
      const disable = tv.run('disable');
      assert.equal(disable.status, 0, disable.stderr);
      assert.match(disable.stderr, /restoring LG Home from the script/);
      const reply = JSON.parse(disable.stdout);
      assert.deepEqual(
        { ...reply, checkpoint: false },
        {
          returnValue: true,
          method: 'executable',
          enabled: false,
          mounted: false,
          stale: false,
          foreign: false,
          hook: false,
          marker: false,
          checkpoint: false,
          appDir: tv.app,
        },
      );
      // No mount, marker or hook; the settings say disabled; the Home process stopped and LG's
      // Home opened where Home was in front. Nothing else was signalled.
      assert.equal(await fs.readFile(tv.mountinfo, 'utf8'), '');
      assert.equal(await tv.exists(tv.dir('run/lounge-home')), false);
      assert.equal(await tv.exists(tv.hook), false);
      assert.match(await fs.readFile(path.join(tv.state, 'home.json'), 'utf8'), /"enabled": false/);
      assert.equal(tv.alive(home), false);
      assert.equal(tv.alive(other), true);
      const calls = await tv.recorded();
      assert.ok(
        calls.includes(
          `luna-send [-n] [1] [-w] [5000] [luna://com.webos.applicationManager/launch] [{"id":"com.webos.app.home"}]`,
        ),
      );
      assert.ok(calls.includes(`umount [${EXE}]`));
      if (name === 'crashing') assert.ok(calls.includes(`umount [-l] [${EXE}]`));
      assert.ok(!calls.some((call) => call.startsWith('node')));
    } finally {
      await tv.cleanup();
    }
  });
}

test('the script’s own disable leaves an app in front and a foreign mount alone', async () => {
  const app = await fixture({ helper: null, front: 'netflix' });
  try {
    assert.equal(app.run('disable').status, 0);
    assert.ok(!(await app.recorded()).some((call) => call.includes('/launch]')));
  } finally {
    await app.cleanup();
  }
  const foreign = await fixture({ helper: null, foreign: true });
  try {
    const disable = foreign.run('disable');
    assert.equal(disable.status, 1);
    assert.match(disable.stdout, /Another integration is mounted over LG Home/);
    assert.match(await fs.readFile(foreign.mountinfo, 'utf8'), /other\/launcher/);
    assert.ok(!(await foreign.recorded()).some((call) => call.startsWith('umount')));
  } finally {
    await foreign.cleanup();
  }
});

test('the hook is removed only when it is Lounge’s', async () => {
  const tv = await fixture({ helper: null });
  try {
    await fs.rm(tv.hook);
    await fs.symlink('/var/lib/other/hook.sh', tv.hook);
    const disable = spawnSync('/bin/sh', [path.join(tv.state, 'home.sh'), 'disable'], {
      encoding: 'utf8',
      env: { ...process.env, PATH: `${tv.dir('bin')}:${process.env.PATH}` },
    });
    assert.equal(disable.status, 0, disable.stderr);
    assert.equal(await fs.readlink(tv.hook), '/var/lib/other/hook.sh');
  } finally {
    await tv.cleanup();
  }
});

test('enable passes the Home it is asked to stand in for on', async () => {
  const tv = await fixture();
  try {
    assert.equal(tv.run('enable', tv.app, FULL_HOME).status, 0);
    assert.deepEqual(await tv.recorded(), [`lounge [--home] [enable] [${tv.app}] [${FULL_HOME}]`]);
  } finally {
    await tv.cleanup();
  }
});

test('the script’s own disable gives back the full-screen Home of a webOS 5 or 6 TV', async () => {
  const tv = await fixture({ helper: null, homeId: FULL_HOME, front: FULL_HOME });
  try {
    const full = await tv.start(FULL_EXE, `{"appId":"${FULL_HOME}"}`);
    const home = await tv.start(EXE);
    const disable = tv.run('disable');
    assert.equal(disable.status, 0, disable.stderr);
    const reply = JSON.parse(disable.stdout);
    assert.equal(reply.method, 'executable');
    assert.equal(reply.homeId, FULL_HOME);
    assert.equal(reply.mounted, false);
    assert.equal(await fs.readFile(tv.mountinfo, 'utf8'), '');
    assert.equal(await tv.exists(tv.dir('run/lounge-home-target')), false);
    assert.equal(await tv.exists(tv.dir('run/lounge-home')), false);
    // Only that Home stopped; LG's other Home was never Lounge's.
    assert.equal(tv.alive(full), false);
    assert.equal(tv.alive(home), true);
    const calls = await tv.recorded();
    assert.ok(calls.includes(`umount [${FULL_EXE}]`));
    assert.ok(
      calls.includes(
        `luna-send [-n] [1] [-w] [5000] [luna://com.webos.applicationManager/launch] [{"id":"${FULL_HOME}"}]`,
      ),
    );
  } finally {
    await tv.cleanup();
  }
});

for (const forceBlock of [false, true]) {
  test(`the script’s own disable takes the staged Home off a webOS ${forceBlock ? 11 : 10} TV and sam reads LG’s again`, async () => {
    const tv = await fixture({
      helper: null,
      method: 'manifest',
      staged: true,
      samReadsLounge: true,
      forceBlock,
    });
    try {
      // A package file that is no working executable (so the script restores by itself).
      await fs.writeFile(path.join(tv.app, 'lounge'), 'package', { mode: 0o644 });
      await fs.mkdir(tv.dir('stage'));
      await fs.symlink(path.join(tv.app, 'lounge'), tv.dir('stage/lounge'));
      await fs.writeFile(tv.dir('stage/appinfo.json'), '{}');
      const staged = await tv.start(`${HOME_DIR}/lounge`, '{"nid":"com.webos.app.home"}');
      const flutter = await tv.start('/usr/bin/flutter-client', '-i', 'com.webos.app.home');
      const normal = await tv.start(path.join(tv.app, 'lounge'), `{"nid": "${APP}"}`);
      const disable = tv.run('disable');
      assert.equal(disable.status, 0, disable.stderr);
      const reply = JSON.parse(disable.stdout);
      assert.equal(reply.method, 'manifest');
      assert.equal(reply.foreign, false);
      assert.equal(reply.homeId, undefined);
      assert.equal(await fs.readFile(tv.mountinfo, 'utf8'), '');
      assert.equal(await fs.readFile(tv.dir('sam'), 'utf8'), `file://${EXE}`);
      // The stage is gone; its links were never followed into the package.
      assert.equal(await tv.exists(tv.dir('stage')), false);
      assert.equal(await tv.exists(path.join(tv.app, 'lounge')), true);
      assert.equal(tv.alive(staged), false);
      assert.equal(tv.alive(flutter), false);
      assert.equal(tv.alive(normal), true);
      const calls = await tv.recorded();
      assert.ok(calls.includes(`umount [${HOME_DIR}]`));
      const block = calls.find((call) => call.includes('/blockSystemApp]'));
      assert.ok(block);
      assert.equal(block.includes('"forceBlockSystemApp":false'), forceBlock);
      const release = calls.findIndex((call) => call.includes('/releaseBlockSystemApp]'));
      assert.ok(release > calls.indexOf(block));
    } finally {
      await tv.cleanup();
    }
  });
}

test('the script’s own disable never touches a staged Home it did not mount', async () => {
  const tv = await fixture({ helper: null });
  try {
    await fs.writeFile(tv.mountinfo, `31 20 179:54 /tvweb/home-takeover ${HOME_DIR} rw - ext4\n`);
    const disable = tv.run('disable');
    assert.equal(disable.status, 0, disable.stderr);
    assert.equal(JSON.parse(disable.stdout).foreign, true);
    assert.match(await fs.readFile(tv.mountinfo, 'utf8'), /home-takeover/);
    const calls = await tv.recorded();
    assert.ok(!calls.some((call) => call.includes('BlockSystemApp')));
  } finally {
    await tv.cleanup();
  }
});
