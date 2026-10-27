// Settings › General › Home Button, on the TV. Lounge runs this as root
// through the Homebrew Channel's exec, with "on", "off" or "status":
//   on      puts Lounge's Home-key filter in LG's compositor chain (after
//           Glasshouse's, where that is installed) and makes Lounge the TV's
//           default Home;
//   off     takes the filter out and gives Home back to LG;
//   status  changes nothing.
// It prints one JSON line: {"returnValue":true,"on":bool} or an error.
'use strict';
var fs = require('fs');
var execFileSync = require('child_process').execFileSync;

var DIR = '/var/lib/lounge';
var FILE = DIR + '/home-key-filter.js';
var NEXT = DIR + '/home-key-filter.next';
var CHAIN = 'com.webos.surfacemanager.keyFilters';
var LOUNGE = 'io.github.yashwanthkamireddi.lounge';
var LG_HOME = 'com.webos.app.home';

// luna-send answers when run from node's execFile (with stdin closed over
// SSH it prints nothing).
function luna(uri, payload) {
  var out = execFileSync(
    '/usr/bin/luna-send',
    ['-n', '1', '-w', '5000', uri, JSON.stringify(payload)],
    {
      timeout: 8000,
    },
  );
  var reply = JSON.parse(out.toString());
  if (reply.returnValue === false) throw new Error(uri + ': ' + (reply.errorText || 'refused'));
  return reply;
}

function readChain() {
  var configs =
    luna('luna://com.webos.service.config/getConfigs', { configNames: [CHAIN] }).configs || {};
  return configs[CHAIN];
}

function writeChain(list) {
  var configs = {};
  configs[CHAIN] = list;
  luna('luna://com.webos.service.config/setConfigs', { configs: configs });
}

function setHome(id) {
  luna('luna://com.webos.settingsservice/setSystemSettings', {
    category: 'general',
    settings: { defaultApps: { home: id } },
  });
}

function ours(entry) {
  return !!entry && entry.file === FILE;
}

try {
  var mode = process.argv[2];
  var chain = readChain();
  if (!Array.isArray(chain)) throw new Error('this TV has no key filter chain');
  var inChain = chain.some(ours);
  var rest = chain.filter(function (entry) {
    return !ours(entry);
  });
  var changed =
    !fs.existsSync(FILE) || fs.readFileSync(FILE, 'utf8') !== fs.readFileSync(NEXT, 'utf8');
  // A Lounge update carries a newer filter: where it is switched on, the read
  // at start-up puts the new one in (only when it differs, as each chain
  // write reloads every filter).
  if (mode === 'status' && inChain && changed) mode = 'on';
  if (mode === 'on') {
    changed = changed || !inChain;
    fs.renameSync(NEXT, FILE);
    // After Glasshouse's filter: where Glasshouse maps Home itself, it keeps the key.
    var at =
      rest.findIndex(function (entry) {
        return entry && entry.handler === 'handleGlasshouseKeys';
      }) + 1;
    var full = rest
      .slice(0, at)
      .concat([{ file: FILE, handler: 'handleLoungeHome' }], rest.slice(at));
    // configd tells the compositor nothing when the same list is written again,
    // so a changed file goes out of the chain and back in. Each write reloads
    // every filter, so the chain is written only when it has to be.
    if (inChain && changed) writeChain(rest);
    if (!inChain || changed) writeChain(full);
    setHome(LOUNGE);
  } else if (mode === 'off') {
    if (inChain) writeChain(rest);
    setHome(LG_HOME);
  } else if (mode !== 'status') {
    throw new Error('unknown mode ' + mode);
  }
  console.log(JSON.stringify({ returnValue: true, on: (readChain() || []).some(ours) }));
} catch (error) {
  console.log(
    JSON.stringify({ returnValue: false, errorText: String(error.message || error).slice(0, 240) }),
  );
}
