/* The browser transport.
 *
 * The native app answers HTTP and pushes snapshots over a WebSocket. Here the
 * page holds the World directly: `command` is the route table, and the ticker
 * that tokio runs server-side is a requestAnimationFrame loop.
 *
 * app.js does not know which of the two it is talking to. It asks for
 * `globalThis.KSP_LOCAL` and falls back to fetch when there isn't one.
 */
import init, { Ksp } from './strata_ksp.js';

const boot = document.getElementById('boot');
const say = (text) => {
  if (boot) boot.textContent = text;
};

try {
  say('Loading the engine…');
  await init();
  const ksp = new Ksp();

  // The engine tick rate, not the display rate. Stepping once per animation
  // frame would run the simulation at whatever the monitor happens to do;
  // the server build advances on a fixed period and so does this.
  const period = 1000 / (ksp.hz() || 30);

  globalThis.KSP_LOCAL = {
    post(path, body) {
      try {
        return Promise.resolve(JSON.parse(ksp.command(path, JSON.stringify(body ?? {}))));
      } catch (error) {
        return Promise.resolve({
          error: { code: 'internal.ksp.bridge', message: String(error) },
        });
      }
    },
    subscribe(onFrame) {
      let last = 0;
      const frame = (now) => {
        if (now - last >= period) {
          last = now;
          try {
            onFrame(JSON.parse(ksp.tick()));
          } catch {
            /* a bad frame is not a reason to stop the clock */
          }
        }
        requestAnimationFrame(frame);
      };
      requestAnimationFrame(frame);
    },
  };

  if (boot) boot.remove();
  await import('./app.js');
} catch (error) {
  say(`The engine did not load: ${error}`);
}
