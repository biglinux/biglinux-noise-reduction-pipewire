// Exercise the production QML status reducer without requiring a Plasma session.
const assert = require("node:assert/strict");
const { readFileSync } = require("node:fs");
const { resolve } = require("node:path");
const { runInNewContext } = require("node:vm");
const { test } = require("node:test");

const source = readFileSync(resolve(__dirname,
  "../usr/share/plasma/plasmoids/br.com.biglinux.micnoise/contents/ui/main.qml"), "utf8");
const match = source.match(/function parseStatus\(stdout\) \{([\s\S]*?)\n    \}\n\n    Plasma5Support/);
assert.ok(match, "production status reducer must be found");
function controller() {
  const state = {
    micEnabled: false, outputEnabled: false, audioAvailable: false,
    micReady: false, outputReady: false, actionError: "", statusError: "",
    i18nd: (_domain, message) => message,
  };
  runInNewContext(`this.parse = function(stdout) {${match[1]}\n}`, state);
  return state;
}
test("a successful status read clears an obsolete query error", () => {
  const state = controller();
  state.parse("{broken");
  assert.ok(state.statusError.length > 0);
  state.parse(JSON.stringify({
    mic_enabled: true, output_enabled: false, audio_available: true,
    mic_running: true, output_running: false,
  }));
  assert.equal(state.statusError, "");
  assert.equal(state.micEnabled, true);
  assert.equal(state.micReady, true);
});
test("a status refresh does not erase a distinct failed-action message", () => {
  const state = controller();
  state.actionError = "previous action failed";
  state.parse(JSON.stringify({ mic_enabled: false, output_enabled: false, audio_available: true }));
  assert.equal(state.actionError, "previous action failed");
  assert.equal(state.statusError, "");
});
