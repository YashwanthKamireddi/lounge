import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import test from 'node:test';
import vm from 'node:vm';

// The filter runs in LG's compositor: a QML JavaScript scope with KeyPolicy,
// Qt, lunaCall and applicationManager. These stand in for them.
const source = await fs.readFile(new URL('../tv/home-key-filter.js', import.meta.url), 'utf8');
const HOME = 0x01000053; // Qt.Key_Super_L
const OTHER = 0x01000012; // Qt.Key_Left

function compositor(launchReply = { returnValue: true }) {
  const launched = [];
  const warned = [];
  const context = {
    KeyPolicy: { NotAccepted: 0, Accepted: 1, NextPolicy: 2 },
    Qt: { Key_Super_L: HOME },
    JSON,
    console: { warn: (message) => warned.push(message) },
    lunaCall: {
      send(service, method, payload, callback) {
        launched.push({ service, method, id: JSON.parse(payload).id });
        callback(typeof launchReply === 'function' ? launchReply() : JSON.stringify(launchReply));
      },
    },
    applicationManager: {
      launch(id) {
        launched.push({ fallback: id });
      },
    },
  };
  vm.createContext(context);
  vm.runInContext(source, context);
  return { context, launched, warned, key: (...args) => context.handleLoungeHome(...args) };
}

test('the filter is plain ES5 that LG compositor can load', () => {
  assert.doesNotThrow(() => new vm.Script(source));
  assert.doesNotMatch(source, /=>|\blet\b|\bconst\b|`/, 'no ES2015 syntax');
});

test('Home opens Lounge and its press and release never reach LG', () => {
  const { key, launched } = compositor();
  assert.equal(key(HOME, true, false), 1);
  assert.equal(key(HOME, false, false), 1);
  assert.deepEqual(launched, [
    {
      service: 'com.webos.applicationManager',
      method: '/launch',
      id: 'io.github.yashwanthkamireddi.lounge',
    },
  ]);
});

test('holding Home opens Lounge once', () => {
  const { key, launched } = compositor();
  key(HOME, true, false);
  assert.equal(key(HOME, true, true), 1);
  assert.equal(key(HOME, true, true), 1);
  key(HOME, false, false);
  assert.equal(launched.length, 1);
});

test('every other key goes on to the next filter untouched', () => {
  const { key, launched } = compositor();
  assert.equal(key(OTHER, true, false), 2);
  assert.equal(key(OTHER, false, false), 2);
  assert.equal(launched.length, 0);
});

test("a release whose press it did not keep is LG's", () => {
  const { key } = compositor();
  assert.equal(key(HOME, false, false), 2);
});

test("if Lounge cannot open, LG's Home opens instead", () => {
  const refused = compositor({ returnValue: false, errorText: 'not installed' });
  refused.key(HOME, true, false);
  assert.deepEqual(refused.launched.at(-1), { fallback: 'com.webos.app.home' });
  const garbled = compositor(() => 'not json');
  garbled.key(HOME, true, false);
  assert.deepEqual(garbled.launched.at(-1), { fallback: 'com.webos.app.home' });
});

test('an error inside the filter passes the key on', () => {
  const { context, key, warned } = compositor();
  context.lunaCall = undefined;
  assert.equal(key(HOME, true, false), 2);
  assert.equal(warned.length, 1);
});
