// Lounge as the TV's Home, run as root by home.sh. Strict ES5: the TV's Node can be old.
//
// LG's Home is a native lifecycle-v2 program whose LS2 role is bound to its executable path.
// Mounting Lounge's executable over that path leaves LG's manifest, window type and role in
// place, so webOS itself starts Lounge as com.webos.app.home on every path to Home. The mount
// lives in RAM: removing the boot hook and restarting always gives back LG's Home.
'use strict';
var fs = require('fs');
var spawnSync = require('child_process').spawnSync;

var APP = 'io.github.yashwanthkamireddi.lounge';
var HOME = 'com.webos.app.home';
var EXE = '/usr/bin/' + HOME;
var MANIFEST_DIR = '/usr/palm/applications/' + HOME;
var STATE = '/var/lib/lounge';
var CONFIG = STATE + '/home.json';
var BACKUP = STATE + '/home-backup.json';
var HOOK = '/var/lib/webosbrew/init.d/10-lounge-home';
// A normal tile launch of Lounge opens the Home instance instead. The app's jail shares
// /var/run but not this mount; /var/run is RAM, like the mount.
var MARKER = '/var/run/lounge-home';
// LG snapshots its Home with CRIU: LG's Home dumps itself (Lounge never does), sam keeps the
// image in /tmp and a copy in /mnt/lg/cmn_data, and restores Home from it. With Lounge mounted
// CRIU rejects that image ("bad size"), but the failed restore leaves the next process ids low:
// Lounge then takes ids LG's input apps' images need, and their restores fail. So no image of
// LG's Home is left for sam while Lounge is Home. sam deletes both copies itself after such a
// failure; LG's Home writes them again the next time it runs (after disable, or a cold boot).
var CHECKPOINT = '/tmp/checkpoint/' + HOME;
var PERSISTENT_CHECKPOINT = '/mnt/lg/cmn_data/checkpoint/' + HOME;
var HASH_CHECK = '/tmp/checkpoint/hash_check_' + HOME;
var BOOT_COPY = '/tmp/finish_criu_boottime_copy';
var DIRS = [
  '/media/developer/apps/usr/palm/applications/' + APP,
  '/media/cryptofs/apps/usr/palm/applications/' + APP
];
// What the earlier Home Button left behind: its compositor key filter and the Home settings
// pointed at a launcher. With Lounge as com.webos.app.home they are all stock again.
var FILTER = STATE + '/home-key-filter.js';
var CHAIN = 'com.webos.surfacemanager.keyFilters';
var OLD_LAUNCHERS = [APP, 'com.ecklf.luma'];

var system = {
  read: function (file) {
    return fs.readFileSync(file, 'utf8');
  },
  exists: function (file) {
    try {
      fs.lstatSync(file);
      return true;
    } catch (error) {
      if (error.code === 'ENOENT') return false;
      throw error;
    }
  },
  stat: function (file) {
    try {
      return fs.statSync(file);
    } catch (error) {
      if (error.code === 'ENOENT' || error.code === 'ESRCH') return null;
      throw error;
    }
  },
  write: function (file, text) {
    fs.writeFileSync(file + '.new', text, { mode: 420 });
    fs.renameSync(file + '.new', file);
  },
  unlink: function (file) {
    try {
      fs.unlinkSync(file);
    } catch (error) {
      if (error.code !== 'ENOENT') throw error;
    }
  },
  run: function (file, args) {
    var result = spawnSync(file, args, { encoding: 'utf8', timeout: 10000 });
    if (result.error || result.status !== 0) {
      throw new Error(
        [file].concat(args).join(' ') +
          ' failed: ' +
          (result.error ? result.error.message : String(result.stderr || '').trim())
      );
    }
    return String(result.stdout || '');
  },
  pause: function (seconds) {
    spawnSync('/bin/sleep', [String(seconds || 1)]);
  },
  pids: function () {
    return fs.readdirSync('/proc').filter(function (name) {
      return /^\d+$/.test(name);
    });
  },
  kill: function (pid, signal) {
    try {
      process.kill(Number(pid), signal);
    } catch (error) {
      if (error.code !== 'ESRCH') throw error;
    }
  },
  log: function (line) {
    console.error('Lounge Home: ' + line);
  }
};

function json(file) {
  return JSON.parse(system.read(file));
}

function writeJson(file, value) {
  system.write(file, JSON.stringify(value, null, 2) + '\n');
}

function luna(uri, payload) {
  var output = system.run('/usr/bin/luna-send', [
    '-n',
    '1',
    '-w',
    '5000',
    uri,
    JSON.stringify(payload)
  ]);
  var start = output.indexOf('{');
  if (start === -1) throw new Error(uri + ' gave no reply');
  var reply = JSON.parse(output.slice(start));
  if (!reply || typeof reply.returnValue !== 'boolean')
    throw new Error(uri + ' gave no Luna reply');
  return reply;
}

function accepted(reply, action) {
  if (reply.returnValue !== true) throw new Error(action + ': ' + JSON.stringify(reply));
  return reply;
}

function foreground() {
  var reply = luna('luna://com.webos.applicationManager/getForegroundAppInfo', {});
  return accepted(reply, 'Read the foreground app').appId || '';
}

// The root of the mount on `target` that is on top, or null.
function mountRoot(target) {
  var root = null;
  system
    .read('/proc/self/mountinfo')
    .split('\n')
    .forEach(function (line) {
      var fields = line.split(' ');
      if (fields[4] === target) root = fields[3];
    });
  return root;
}

// Lounge's executable, also after a package update replaced the mounted file.
function ours(root) {
  return /\/io\.github\.yashwanthkamireddi\.lounge\/lounge(\/\/deleted)?$/.test(root || '');
}

// Anything mounted over LG Home's manifest belongs to another integration.
function manifestMounted() {
  return system
    .read('/proc/self/mountinfo')
    .split('\n')
    .some(function (line) {
      var target = line.split(' ')[4] || '';
      return target === MANIFEST_DIR || target.indexOf(MANIFEST_DIR + '/') === 0;
    });
}

function refuseForeign() {
  var root = mountRoot(EXE);
  if ((root && !ours(root)) || manifestMounted()) {
    throw new Error('Another integration is mounted over LG Home; nothing was changed');
  }
  return root;
}

function same(a, b) {
  return !!a && !!b && a.dev === b.dev && a.ino === b.ino;
}

function mounted(appDir) {
  return same(system.stat(appDir + '/lounge'), system.stat(EXE));
}

function unmount() {
  for (var i = 0; i < 3 && mountRoot(EXE); i++) {
    if (!ours(mountRoot(EXE))) throw new Error('Another integration is mounted over LG Home');
    try {
      system.run('/bin/umount', [EXE]);
    } catch (error) {
      // A running Home executes the mounted file: detach it instead.
      system.log(error.message + '; detaching the mount');
      system.run('/bin/umount', ['-l', EXE]);
    }
  }
  if (mountRoot(EXE)) throw new Error('The Lounge Home mount is still active');
}

function mount(appDir) {
  if (mountRoot(EXE) && !mounted(appDir)) unmount();
  if (!mounted(appDir)) system.run('/bin/mount', ['--bind', appDir + '/lounge', EXE]);
  if (!mounted(appDir)) throw new Error('The Home bind mount did not take');
}

// Files are unlinked, never followed (the /tmp pages are a symlink into the persistent copy);
// the persistent directory itself stays, as sam leaves it.
function removeCheckpoint() {
  [CHECKPOINT, PERSISTENT_CHECKPOINT].forEach(function (directory) {
    if (!system.exists(directory)) return;
    fs.readdirSync(directory).forEach(function (name) {
      system.unlink(directory + '/' + name);
    });
  });
  if (system.exists(CHECKPOINT)) fs.rmdirSync(CHECKPOINT);
  system.unlink(HASH_CHECK);
}

// Processes started as LG's Home executable. `keep`: leave a running copy of the current
// Lounge alone, so applying again never restarts a healthy Home.
function homeProcesses(keep) {
  var exe = keep ? system.stat(keep + '/lounge') : null;
  return system.pids().filter(function (pid) {
    try {
      var argv0 = system.read('/proc/' + pid + '/cmdline').split('\0')[0];
      if (argv0 !== EXE) return false;
      return !same(exe, system.stat('/proc/' + pid + '/exe'));
    } catch (error) {
      // A process may exit while /proc is read.
      if (error.code === 'ENOENT' || error.code === 'ESRCH') return false;
      throw error;
    }
  });
}

function stopHome(keep) {
  var pids = homeProcesses(keep);
  pids.forEach(function (pid) {
    system.kill(pid, 'SIGTERM');
  });
  var alive = function () {
    return pids.filter(function (pid) {
      return system.exists('/proc/' + pid);
    });
  };
  for (var i = 0; i < 8 && alive().length; i++) system.pause(1);
  alive().forEach(function (pid) {
    system.kill(pid, 'SIGKILL');
  });
  for (var j = 0; j < 3 && alive().length; j++) system.pause(1);
  if (alive().length) throw new Error('The previous Home process has not stopped');
  return pids.length;
}

function launchHome() {
  accepted(luna('luna://com.webos.applicationManager/launch', { id: HOME }), 'Open Home');
}

// Right after the previous Home stopped, sam can accept a launch without starting anything:
// Home in front is the success condition.
function openHome() {
  for (var i = 0; i < 15; i++) {
    if (i === 0 || i === 6) {
      try {
        launchHome();
      } catch (error) {
        system.log(error.message);
      }
    }
    if (foreground() === HOME) return;
    system.pause(1);
  }
  throw new Error('Home did not come to the front');
}

function hookOwned() {
  try {
    return fs.readlinkSync(HOOK) === STATE + '/home.sh';
  } catch (error) {
    if (error.code === 'ENOENT' || error.code === 'EINVAL') return false;
    throw error;
  }
}

function installHook() {
  if (!system.exists('/var/lib/webosbrew/init.d')) {
    throw new Error('The Homebrew Channel boot hooks (/var/lib/webosbrew/init.d) are missing');
  }
  if (system.exists(HOOK) && !hookOwned()) throw new Error(HOOK + ' is already in use');
  if (!hookOwned()) fs.symlinkSync(STATE + '/home.sh', HOOK);
}

function removeHook() {
  if (hookOwned()) system.unlink(HOOK);
}

function config() {
  return system.exists(CONFIG) ? json(CONFIG) : null;
}

function stockHome() {
  var stock = json(MANIFEST_DIR + '/appinfo.json');
  if (
    stock.id !== HOME ||
    stock.type !== 'native' ||
    stock.main !== 'file://' + EXE ||
    stock.nativeLifeCycleInterfaceVersion !== 2
  ) {
    throw new Error("This TV's Home is not a native lifecycle-v2 program Lounge can stand in for");
  }
  return stock;
}

function installation(appDir) {
  var candidates = appDir ? [appDir] : DIRS;
  for (var i = 0; i < candidates.length; i++) {
    try {
      var directory = fs.realpathSync(candidates[i]);
      var app = json(directory + '/appinfo.json');
      if (
        app.id === APP &&
        app.type === 'native' &&
        app.main === 'lounge' &&
        system.exists(directory + '/lounge')
      ) {
        return directory;
      }
    } catch (error) {
      if (error.code !== 'ENOENT' && !(error instanceof SyntaxError)) throw error;
    }
  }
  throw new Error('No installed native Lounge package found');
}

// Mounts Lounge in Home's place. `open`: bring Home to the front even if it was not.
function apply(settings, open) {
  refuseForeign();
  if (!system.exists(settings.appDir + '/lounge')) throw new Error('Lounge is no longer installed');
  var front = null;
  try {
    front = foreground();
  } catch (error) {
    system.log(error.message);
  }
  try {
    mount(settings.appDir);
    system.write(MARKER, APP + '\n');
    removeCheckpoint();
    var stopped = stopHome(settings.appDir);
    // Never take the screen from an input or app someone chose: only where the Home just
    // stopped was in front.
    if (open || (front === HOME && stopped)) openHome();
  } catch (error) {
    try {
      disable();
    } catch (recovery) {
      throw new Error(error.message + '; restoring LG Home also failed: ' + recovery.message);
    }
    throw error;
  }
}

// Gives Home back to LG exactly: no mount, no marker, no hook. Works with or without a config.
function restore() {
  var front = null;
  try {
    front = foreground();
  } catch (error) {
    system.log(error.message + '; restoring the files first');
  }
  var root = mountRoot(EXE);
  if (root && !ours(root)) throw new Error('Another integration is mounted over LG Home');
  system.unlink(MARKER);
  removeHook();
  if (root) unmount();
  stopHome(null);
  // Nothing in front means Home itself went away: LG's Home takes its place.
  if (front === HOME || front === '') launchHome();
}

// The chain without Lounge's old Home Button filter (Glasshouse's own entry stays).
function withoutOldFilter(chain) {
  return chain.filter(function (entry) {
    return !entry || entry.file !== FILTER;
  });
}

function cleanUpOldHome() {
  var backup = system.exists(BACKUP) ? json(BACKUP) : {};
  var notes = [];
  var configs =
    accepted(
      luna('luna://com.webos.service.config/getConfigs', {
        configNames: [CHAIN, 'profile.idleApp']
      }),
      'Read the key filter chain'
    ).configs || {};
  var chain = configs[CHAIN];
  if (Array.isArray(chain) && withoutOldFilter(chain).length !== chain.length) {
    if (!backup.keyFilters) backup.keyFilters = chain;
    writeJson(BACKUP, backup);
    var update = {};
    update[CHAIN] = withoutOldFilter(chain);
    accepted(
      luna('luna://com.webos.service.config/setConfigs', { configs: update }),
      'Remove the old Home key filter'
    );
    notes.push('old Home key filter removed');
  }
  [FILTER, STATE + '/home-key-filter.next', STATE + '/home-button.js'].forEach(system.unlink);
  var idle = configs['profile.idleApp'];
  if (idle && OLD_LAUNCHERS.indexOf(idle.appId) !== -1) {
    if (!backup.idleApp) backup.idleApp = idle;
    writeJson(BACKUP, backup);
    accepted(
      luna('luna://com.webos.service.config/setConfigs', {
        configs: { 'profile.idleApp': { appId: HOME } }
      }),
      'Restore the idle app'
    );
    notes.push('profile.idleApp is ' + HOME + ' again');
  }
  var general = luna('luna://com.webos.settingsservice/getSystemSettings', {
    category: 'general',
    keys: ['defaultApps']
  });
  var apps = general.settings && general.settings.defaultApps;
  if (apps && OLD_LAUNCHERS.indexOf(apps.home) !== -1) {
    if (!backup.defaultApps) backup.defaultApps = apps;
    writeJson(BACKUP, backup);
    var rest = {};
    Object.keys(apps).forEach(function (key) {
      if (key !== 'home') rest[key] = apps[key];
    });
    if (Object.keys(rest).length) {
      accepted(
        luna('luna://com.webos.settingsservice/setSystemSettings', {
          category: 'general',
          settings: { defaultApps: rest }
        }),
        'Restore the default Home'
      );
    } else {
      accepted(
        luna('luna://com.webos.settingsservice/deleteSystemSettings', {
          category: 'general',
          keys: ['defaultApps']
        }),
        'Restore the default Home'
      );
    }
    notes.push('general.defaultApps.home is stock again');
  }
  return notes;
}

function closeNormal() {
  var running =
    accepted(luna('luna://com.webos.applicationManager/running', {}), 'Read running apps')
      .running || [];
  var normal = running.some(function (entry) {
    return entry.id === APP;
  });
  if (normal) luna('luna://com.webos.applicationManager/closeByAppId', { id: APP });
}

function enable(appDir) {
  stockHome();
  appDir = installation(appDir);
  refuseForeign();
  if (system.exists(HOOK) && !hookOwned()) throw new Error(HOOK + ' is already in use');
  var settings = { enabled: false, method: 'executable', appDir: appDir };
  writeJson(CONFIG, settings);
  try {
    installHook();
    cleanUpOldHome().forEach(system.log);
    apply(settings, true);
  } catch (error) {
    removeHook();
    throw error;
  }
  settings.enabled = true;
  writeJson(CONFIG, settings);
  // A normal Lounge (the one that asked, say) would write the same settings: Home replaces it.
  try {
    closeNormal();
  } catch (error) {
    system.log(error.message);
  }
}

function disable() {
  var settings = config();
  if (settings) {
    settings.enabled = false;
    writeJson(CONFIG, settings);
  }
  restore();
}

// At boot, after sam is up and its own CRIU boot work is done (bounded waits).
function boot() {
  var settings = config();
  if (!settings || !settings.enabled) return;
  if (!system.exists(settings.appDir + '/lounge')) {
    system.log('Lounge is not installed; LG Home stays');
    return;
  }
  for (var i = 0; i < 30; i++) {
    try {
      foreground();
      break;
    } catch (error) {
      // sam is not up yet.
      if (i === 29) system.log(error.message);
      system.pause(2);
    }
  }
  for (var j = 0; j < 30 && system.exists('/tmp/criu_enable') && !system.exists(BOOT_COPY); j++) {
    system.pause(2);
  }
  apply(settings, false);
  homeAfterBoot();
}

// A cold boot shows Home. LG's boot manager opens the last input (or Live TV) and, unlike a
// Quick Start wake, never starts Home: once it has, Home is opened over it. Only what the boot
// manager opens is replaced; an app someone opened meanwhile stays. Bounded: 60 s at most.
function startupScreen(app) {
  return (
    app === 'com.webos.app.livetv' ||
    /^com\.webos\.app\.(hdmi\d+|externalinput\.[a-z0-9]+)$/.test(app)
  );
}

function homeAfterBoot() {
  for (var i = 0; i < 30; i++) {
    var app = '';
    try {
      app = foreground();
    } catch (error) {
      // sam is busy with the boot; ask again (and say why on the last try).
      if (i === 29) system.log(error.message);
    }
    if (app === HOME) return;
    if (app && !startupScreen(app)) {
      system.log('boot: ' + app + ' is in front; Home stays behind it');
      return;
    }
    if (app) {
      try {
        accepted(luna('luna://com.webos.applicationManager/launch', { id: HOME }), 'Open Home');
        system.log('boot: opened Home over ' + app);
      } catch (error) {
        system.log(error.message);
      }
      return;
    }
    system.pause(2);
  }
}

function status() {
  var settings = config();
  var root = mountRoot(EXE);
  var appDir = settings && settings.appDir;
  return {
    returnValue: true,
    method: 'executable',
    enabled: !!(settings && settings.enabled),
    mounted: !!appDir && ours(root) && mounted(appDir),
    stale: ours(root) && !(appDir && mounted(appDir)),
    foreign: (!!root && !ours(root)) || manifestMounted(),
    hook: hookOwned(),
    marker: system.exists(MARKER),
    checkpoint:
      system.exists(CHECKPOINT) ||
      (system.exists(PERSISTENT_CHECKPOINT) && fs.readdirSync(PERSISTENT_CHECKPOINT).length > 0),
    appDir: appDir || null
  };
}

function main(argv) {
  var command = argv[2];
  if (command === 'enable') enable(argv[3]);
  else if (command === 'disable') disable();
  else if (command === 'boot') boot();
  else if (command === 'apply') {
    var settings = config();
    if (!settings || !settings.enabled) throw new Error('Lounge is not enabled as Home');
    apply(settings, false);
  } else if (command !== 'status') throw new Error('Unknown command ' + command);
  console.log(JSON.stringify(status()));
}

if (require.main === module) {
  try {
    main(process.argv);
  } catch (error) {
    console.log(
      JSON.stringify({
        returnValue: false,
        errorText: String(error.message || error).slice(0, 400)
      })
    );
    process.exitCode = 1;
  }
}

module.exports = {
  system: system,
  main: main,
  ours: ours,
  withoutOldFilter: withoutOldFilter,
  status: status
};
