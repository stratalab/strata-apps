// Boots the engine, then hands the UI a handle to it.
//
// There is no server build of this app, so this is the only entry point: load
// the wasm, construct one `Paint`, publish it, and only then import the UI.
// app.js can assume the database is up rather than guarding every call.
import init, { Paint } from './strata_paint.js';

const boot = document.querySelector('[data-boot]');

try {
  await init();
  window.PAINT = new Paint();
  if (boot) boot.remove();
  await import('./app.js');
  window.PAINT_READY = true;
  dispatchEvent(new Event('paint:ready'));
} catch (error) {
  // The engine failing to start is the one error worth taking the page over:
  // everything below this line needs it, and a canvas that silently will not
  // commit looks like a drawing bug rather than a missing database.
  console.error(error);
  if (boot) {
    boot.textContent = `The engine did not start: ${error?.message ?? error}`;
    boot.dataset.failed = '';
  }
}
