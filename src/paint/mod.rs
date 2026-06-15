//! Backend-agnostic paint commands and the tessellator that turns them into
//! a single vertex/index buffer.

mod command;
mod mesh;
mod painter;
mod tessellator;
mod texture;

pub use command::{PaintCmd, RectShape};
pub use mesh::{Mesh, Vertex};
pub use painter::Painter;
pub use tessellator::Tessellator;
pub use texture::{TextureFilter, TextureId};
