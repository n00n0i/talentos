use crate::models::{Chunk, ChunkingConfig, Document};
use uuid::Uuid;
use sha2::{Sha256, Digest};

/// Text chunker with recursive splitting strategy
pub struct Chunker {
    config: ChunkingConfig,
}

impl Chunker {
    pub fn new(config: ChunkingConfig) -> Self {
        Self { config }
    }

    pub fn default() -> Self {
        Self::new(ChunkingConfig::default())
    }

    /// Chunk a document into pieces suitable for embedding
    pub fn chunk_document(&self, document: &Document) -> Vec<Chunk> {
        let chunks = self.split_text(&document.content);
        chunks
            .into_iter()
            .enumerate()
            .map(|(index, content)| Chunk {
                document_id: document.id,
                chunk_index: index as i32,
                content,
                word_count: self.count_words(&content),
            })
            .collect()
    }

    /// Split text using recursive strategy
    fn split_text(&self, text: &str) -> Vec<String> {
        let mut result = Vec::new();
        let separator = &self.config.separator;

        // First split by separator (paragraph boundaries)
        let paragraphs: Vec<&str> = text.split(separator).collect();
        let mut current_chunk = String::new();

        for paragraph in paragraphs {
            let paragraph = paragraph.trim();
            if paragraph.is_empty() {
                continue;
            }

            // If adding this paragraph would exceed chunk_size, flush current chunk
            if !current_chunk.is_empty() && current_chunk.len() + paragraph.len() + separator.len() > self.config.chunk_size {
                result.push(current_chunk.trim().to_string());
                current_chunk = paragraph.to_string();
            } else {
                if !current_chunk.is_empty() {
                    current_chunk.push_str(separator);
                }
                current_chunk.push_str(paragraph);
            }
        }

        // Don't forget the last chunk
        if !current_chunk.is_empty() {
            result.push(current_chunk.trim().to_string());
        }

        // Now handle chunks that are still too large - split on whitespace
        let mut final_chunks = Vec::new();
        for chunk in result {
            if chunk.len() <= self.config.chunk_size {
                final_chunks.push(chunk);
            } else {
                // Split on whitespace
                final_chunks.extend(self.split_on_whitespace(&chunk));
            }
        }

        final_chunks
    }

    /// Split a large chunk on whitespace boundaries
    fn split_on_whitespace(&self, text: &str) -> Vec<String> {
        let words: Vec<&str> = text.split_whitespace().collect();
        let mut chunks = Vec::new();
        let mut current = String::new();

        for word in words {
            let word_len = word.len();
            let space_len = if current.is_empty() { 0 } else { 1 };

            if current.len() + space_len + word_len > self.config.chunk_size {
                if !current.is_empty() {
                    chunks.push(current.trim().to_string());
                    current = word.to_string();
                } else {
                    // Single word exceeds chunk_size - truncate
                    chunks.push(word[..self.config.chunk_size].to_string());
                }
            } else {
                if !current.is_empty() {
                    current.push(' ');
                }
                current.push_str(word);
            }
        }

        if !current.is_empty() {
            chunks.push(current.trim().to_string());
        }

        chunks
    }

    /// Count words in text
    fn count_words(&self, text: &str) -> i32 {
        text.split_whitespace().count() as i32
    }

    /// Generate SHA-256 hash of content for deduplication
    pub fn content_hash(content: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        format!("{:x}", hasher.finalize())
    }
}

/// A text chunk ready for embedding
#[derive(Debug, Clone)]
pub struct Chunk {
    pub document_id: Uuid,
    pub chunk_index: i32,
    pub content: String,
    pub word_count: i32,
}