pub mod cache;
pub mod compose;
pub mod model;
pub mod store;

pub use cache::EmbeddingCache;
pub use compose::compose_text;
pub use model::EmbeddingModel;
pub use store::{find_similar, store_embedding};
