//! Recursive text chunker. Pure, deterministic, no I/O.

use sha2::{Digest, Sha256};

use crate::models::{Chunk, ChunkingConfig};

/// Splits documents into overlapping chunks suitable for embedding.
pub struct Chunker {
    config: ChunkingConfig,
}

impl Chunker {
    pub fn new(config: ChunkingConfig) -> Self {
        Self { config }
    }

    /// Chunk raw document text into indexed pieces.
    /// The document row itself stores only the hash — the text lives in chunks.
    pub fn chunk_document(&self, document_id: uuid::Uuid, content: &str) -> Vec<Chunk> {
        self.split_text(content)
            .into_iter()
            .enumerate()
            .map(|(index, content)| Chunk {
                document_id,
                chunk_index: index as i32,
                word_count: content.split_whitespace().count() as i32,
                content,
            })
            .collect()
    }

    /// Split on paragraph boundaries first, then on whitespace for oversized blocks.
    fn split_text(&self, text: &str) -> Vec<String> {
        let mut chunks = Vec::new();
        let mut current = String::new();

        for paragraph in text.split(&self.config.separator) {
            let paragraph = paragraph.trim();
            if paragraph.is_empty() {
                continue;
            }
            if !current.is_empty()
                && current.len() + paragraph.len() + self.config.separator.len()
                    > self.config.chunk_size
            {
                chunks.push(std::mem::take(&mut current));
            }
            if !current.is_empty() {
                current.push_str(&self.config.separator);
            }
            current.push_str(paragraph);
        }
        if !current.is_empty() {
            chunks.push(current);
        }

        // Split anything still oversized on whitespace.
        chunks
            .into_iter()
            .flat_map(|chunk| {
                if chunk.len() <= self.config.chunk_size {
                    vec![chunk]
                } else {
                    self.split_on_whitespace(&chunk)
                }
            })
            .collect()
    }

    fn split_on_whitespace(&self, text: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut current = String::new();
        for word in text.split_whitespace() {
            let extra = if current.is_empty() { 0 } else { 1 };
            if current.len() + extra + word.len() > self.config.chunk_size {
                match word.char_indices().nth(self.config.chunk_size) {
                    // Single token longer than chunk_size: hard-cut at a char boundary.
                    Some((idx, _)) if current.is_empty() => {
                        out.push(word[..idx].to_string());
                        current = word[idx..].to_string();
                        continue;
                    }
                    _ => {}
                }
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }
        if !current.is_empty() {
            out.push(current);
        }
        out
    }

    /// SHA-256 of content, used to dedupe documents per tenant.
    pub fn content_hash(content: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        format!("{:x}", hasher.finalize())
    }
}

impl Default for Chunker {
    fn default() -> Self {
        Self::new(ChunkingConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn short_text_is_one_chunk() {
        let chunks = Chunker::default().chunk_document(Uuid::nil(), "hello world");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].word_count, 2);
        assert_eq!(chunks[0].chunk_index, 0);
    }

    #[test]
    fn long_text_splits_under_chunk_size() {
        let config = ChunkingConfig {
            chunk_size: 50,
            overlap: 0,
            separator: "\n\n".into(),
        };
        let text = "a".repeat(40) + "\n\n" + &"b".repeat(40) + "\n\n" + &"c".repeat(40);
        let chunks = Chunker::new(config).chunk_document(Uuid::nil(), &text);
        assert!(chunks.len() > 1);
        assert!(chunks.iter().all(|c| c.content.len() <= 50));
    }

    #[test]
    fn content_hash_is_stable_and_distinct() {
        assert_eq!(Chunker::content_hash("abc"), Chunker::content_hash("abc"));
        assert_ne!(Chunker::content_hash("abc"), Chunker::content_hash("abd"));
    }
}
