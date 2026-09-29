//! A paint program whose undo has no bottom.
//!
//! Every stroke is a commit. The timeline under the canvas is the database's
//! own commit history, and dragging it is an as-of read: the canvas you see is
//! the one the database reconstructs for that point, not a snapshot the app
//! kept to one side.
//!
//! That is the whole claim. Procreate records a video to play your painting
//! back; here the playback is the storage. Nothing is captured, because
//! nothing was ever thrown away.
//!
//! And because nothing was thrown away, the past is somewhere you can work:
//! scrub back and start drawing and the app forks the branch under you, so
//! the two versions both exist rather than one replacing the other.

pub mod store;
pub mod stroke;

#[cfg(feature = "wasm")]
mod wasm;

pub use store::Commit;
pub use stroke::Stroke;
