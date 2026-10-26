/* Lounge: the remote's Home key opens Lounge.
 *
 * A key filter in LG's compositor chain (com.webos.surfacemanager.keyFilters),
 * put there by Settings › General › Home Button and taken out by the same
 * switch. It catches Home only; every other key goes on to the next filter
 * untouched. If Lounge cannot be opened (removed, or the app manager says
 * no), it opens LG's Home instead, so the key always lands somewhere. Any
 * error passes the key on to LG. Strict ES5: LG's compositor runs it. */

/* eslint-disable no-unused-vars -- LG's compositor calls handleLoungeHome by
 * name, and ES5 has no catch without a binding. */

var loungeHomeHeld = false;

function loungeOpen() {
  lunaCall.send(
    'com.webos.applicationManager',
    '/launch',
    JSON.stringify({ id: 'io.github.yashwanthkamireddi.lounge' }),
    function (payload) {
      try {
        var reply = typeof payload === 'string' ? JSON.parse(payload) : payload;
        if (reply && reply.returnValue === false) {
          applicationManager.launch('com.webos.app.home', '{}');
        }
      } catch (e) {
        applicationManager.launch('com.webos.app.home', '{}');
      }
    },
  );
}

function handleLoungeHome(key, pressed, autoRepeat) {
  try {
    if (key !== Qt.Key_Super_L) return KeyPolicy.NextPolicy;
    if (pressed) {
      if (!autoRepeat) {
        loungeHomeHeld = true;
        loungeOpen();
      }
      return KeyPolicy.Accepted;
    }
    // LG's filters open their Home on the release: the release of a press
    // kept here is kept too.
    if (loungeHomeHeld) {
      loungeHomeHeld = false;
      return KeyPolicy.Accepted;
    }
    return KeyPolicy.NextPolicy;
  } catch (e) {
    console.warn('lounge keyfilter: ' + e + '; key passed on');
    return KeyPolicy.NextPolicy;
  }
}
