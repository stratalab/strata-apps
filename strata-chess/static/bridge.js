// Boots the rules engine, then the UI.
//
// Stockfish is not started here: it is a separate program with its own licence
// and its own worker, and app.js owns it. See stockfish/README.md, which ships
// beside the binary.
import init, { Chess } from './strata_chess.js';

const boot = document.querySelector('[data-boot]');

try {
  await init();
  window.CHESS = new Chess();
  await import('./app.js');
  if (boot) boot.remove();
  window.CHESS_READY = true;
  dispatchEvent(new Event('chess:ready'));
} catch (error) {
  console.error(error);
  if (boot) {
    boot.textContent = `The board did not load: ${error?.message ?? error}`;
    boot.dataset.failed = '';
  }
}
