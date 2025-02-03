use eyre::{bail, eyre, Result, WrapErr};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::env;
use tracing::instrument;

use crate::database::CommitSummary;

#[derive(Debug, Deserialize)]
struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
}

#[derive(Debug, Deserialize)]
struct GeminiCandidate {
    content: GeminiContent2,
}

#[derive(Debug, Deserialize)]
struct GeminiContent2 {
    parts: Vec<GeminiPart2>,
}

#[derive(Debug, Deserialize)]
struct GeminiPart2 {
    text: String,
}

#[derive(Debug, Serialize)]
struct OpenAIEmbeddingRequest<'a> {
    model: &'a str,
    input: &'a str,
    encoding_format: &'a str,
}

#[derive(Debug, Deserialize)]
struct OpenAIEmbeddingResponse {
    data: Vec<OpenAIEmbedding>,
}

#[derive(Debug, Deserialize)]
struct OpenAIEmbedding {
    embedding: Vec<f32>,
}

pub struct MachineLearning {
    client: Client,
    gemini_api_key: String,
    openai_api_key: String,
}

impl MachineLearning {
    #[instrument]
    pub fn new() -> Result<Self> {
        let gemini_api_key =
            env::var("GEMINI_API_KEY").wrap_err("GEMINI_API_KEY environment variable not set")?;
        let openai_api_key =
            env::var("OPENAI_API_KEY").wrap_err("OPENAI_API_KEY environment variable not set")?;

        Ok(Self {
            client: Client::new(),
            gemini_api_key,
            openai_api_key,
        })
    }

    #[instrument(skip(self, text))]
    pub async fn get_embedding(&self, text: &str) -> Result<Vec<f32>> {
        let request = OpenAIEmbeddingRequest {
            model: "text-embedding-3-small",
            input: text,
            encoding_format: "float",
        };

        let response_text = self
            .client
            .post("https://api.openai.com/v1/embeddings")
            .header("Authorization", format!("Bearer {}", self.openai_api_key))
            .json(&request)
            .send()
            .await
            .wrap_err("Failed to send embedding request to OpenAI")?
            .text()
            .await
            .wrap_err("Failed to get response text from OpenAI")?;

        let response = serde_json::from_str::<OpenAIEmbeddingResponse>(&response_text)
            .wrap_err_with(|| {
                format!(
                    "Failed to parse OpenAI embedding response.\nInput text: {}\nResponse text: {}",
                    text, response_text
                )
            })?;

        Ok(response.data[0].embedding.clone())
    }

    #[instrument(skip(self, text))]
    pub async fn summarize_text(&self, text: &str) -> Result<CommitSummary> {
        let request = json!({
            "contents": [{
                "role": "",
                "parts": [{
                    "text": text
                }]
            }],
            "systemInstruction": {
                "role": "user",
                "parts": [{
                    "text": "Analyze the code changes and extract technical details into the specified structure. Focus on technical aspects that would indicate developer expertise and skills required. Be concise and specific."
                }]
            },
            "generationConfig": {
                "temperature": 0.2,
                "topK": 40,
                "topP": 0.95,
                "maxOutputTokens": 8192,
                "responseMimeType": "application/json",
                "responseSchema": {
                    "type": "object",
                    "required": ["languages", "frameworks_libraries", "patterns", "specialized_knowledge"],
                    "properties": {
                        "languages": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Programming languages involved in the changes"
                        },
                        "frameworks_libraries": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Frameworks and libraries used or modified"
                        },
                        "patterns": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Design patterns, architectural patterns, or coding patterns used"
                        },
                        "specialized_knowledge": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Areas of specialized knowledge required"
                        }
                    }
                }
            }
        });

        let response = self
            .client
            .post(format!(
                "https://generativelanguage.googleapis.com/v1beta/models/gemini-1.5-flash-8b:generateContent?key={}",
                self.gemini_api_key
            ))
            .json(&request)
            .send()
            .await
            .wrap_err("Failed to send request to Gemini API")?
            .text()
            .await
            .wrap_err("Failed to get response text from Gemini API")?;

        // First parse the Gemini response structure
        let gemini_response: GeminiResponse = serde_json::from_str(&response)
            .wrap_err_with(|| format!("Failed to parse Gemini response: {}", response))?;

        // Get the first candidate's text
        let summary_text = gemini_response
            .candidates
            .first()
            .ok_or_else(|| eyre!("No candidates in Gemini response"))?
            .content
            .parts
            .first()
            .ok_or_else(|| eyre!("No parts in Gemini response"))?
            .text
            .as_str();

        // Now parse the actual summary content
        match serde_json::from_str(summary_text) {
            Ok(summary) => Ok(summary),
            Err(e) => {
                bail!(eyre!(
                    "Failed to parse Gemini response as CommitSummary: {}\nResponse: {}",
                    e,
                    response
                ))
            }
        }
    }

    #[instrument(skip(self, text))]
    pub async fn summarize_readme(&self, text: &str) -> Result<String> {
        let request = json!({
            "contents": [{
                "role": "",
                "parts": [{
                    "text": text
                }]
            }],
            "systemInstruction": {
                "role": "user",
                "parts": [{
                    "text": "Provide a concise summary of this repository's README, focusing on the project's purpose, key features, and technical aspects."
                }]
            },
            "generationConfig": {
                "temperature": 0.2,
                "topK": 40,
                "topP": 0.95,
                "maxOutputTokens": 8192,
                "responseMimeType": "application/json",
                "responseSchema": {
                    "type": "object",
                    "required": ["summary"],
                    "properties": {
                        "summary": {
                            "type": "string",
                            "description": "A concise summary of the README content"
                        }
                    }
                }
            }
        });

        let response = self
            .client
            .post(format!(
                "https://generativelanguage.googleapis.com/v1beta/models/gemini-1.5-flash-8b:generateContent?key={}",
                self.gemini_api_key
            ))
            .json(&request)
            .send()
            .await
            .wrap_err("Failed to send request to Gemini API")?
            .text()
            .await
            .wrap_err("Failed to get response text from Gemini API")?;

        // Parse the Gemini response structure
        let gemini_response: GeminiResponse = serde_json::from_str(&response)
            .wrap_err_with(|| format!("Failed to parse Gemini response: {}", response))?;

        // Get the first candidate's text
        let summary = gemini_response
            .candidates
            .first()
            .ok_or_else(|| eyre!("No candidates in Gemini response"))?
            .content
            .parts
            .first()
            .ok_or_else(|| eyre!("No parts in Gemini response"))?
            .text
            .clone();

        // Parse the JSON response to extract the summary
        let summary_obj: serde_json::Value = serde_json::from_str(&summary)
            .wrap_err_with(|| format!("Failed to parse summary JSON: {}", summary))?;

        let summary_text = summary_obj["summary"]
            .as_str()
            .ok_or_else(|| eyre!("Missing 'summary' field in response"))?
            .to_string();

        Ok(summary_text)
    }

    pub fn cosine_similarity(vec1: &[f32], vec2: &[f32]) -> f32 {
        let dot_product: f32 = vec1.iter().zip(vec2.iter()).map(|(a, b)| a * b).sum();
        let norm1: f32 = vec1.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm2: f32 = vec2.iter().map(|x| x * x).sum::<f32>().sqrt();

        if norm1 == 0.0 || norm2 == 0.0 {
            0.0
        } else {
            dot_product / (norm1 * norm2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dotenv::dotenv;

    #[tokio::test]
    async fn test_gemini_summarization() -> Result<()> {
        dotenv().ok();

        let generator = MachineLearning::new()?;
        let sample_code = r#"
        fn add(a: i32, b: i32) -> i32 {
            a + b
        }
        "#;

        let summary = generator.summarize_text(sample_code).await?;

        // Check that at least one of the vectors contains data
        assert!(
            !summary.languages.is_empty()
                || !summary.frameworks_libraries.is_empty()
                || !summary.patterns.is_empty()
                || !summary.specialized_knowledge.is_empty(),
            "Expected at least one non-empty field in the summary"
        );

        // Check that Rust is identified as a language
        assert!(
            summary
                .languages
                .iter()
                .any(|lang| lang.to_lowercase().contains("rust")),
            "Expected Rust to be identified as a language"
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_embedding_generation() -> Result<()> {
        dotenv().ok();

        let generator = MachineLearning::new()?;
        let text = "Hello, world!";

        let embedding = generator.get_embedding(text).await?;

        // Embeddings should be non-empty
        assert!(!embedding.is_empty());

        // Test cosine similarity with itself (should be 1.0)
        let similarity = MachineLearning::cosine_similarity(&embedding, &embedding);
        assert!((similarity - 1.0).abs() < 1e-6);

        Ok(())
    }

    #[tokio::test]
    async fn test_cosine_similarity() {
        let vec1 = vec![1.0, 0.0, 0.0];
        let vec2 = vec![0.0, 1.0, 0.0];
        let vec3 = vec![1.0, 0.0, 0.0];

        // Orthogonal vectors should have similarity 0
        assert_eq!(MachineLearning::cosine_similarity(&vec1, &vec2), 0.0);

        // Same vectors should have similarity 1
        assert_eq!(MachineLearning::cosine_similarity(&vec1, &vec3), 1.0);

        // Test with zero vector
        let zero_vec = vec![0.0, 0.0, 0.0];
        assert_eq!(MachineLearning::cosine_similarity(&vec1, &zero_vec), 0.0);
    }

    #[tokio::test]
    async fn test_readme_summarization() -> Result<()> {
        dotenv().ok();

        let generator = MachineLearning::new()?;
        let sample_readme = r#"
        # Sample Project

        This is a Rust project that demonstrates async/await patterns and error handling.
        
        ## Features
        - Async operations
        - Error handling with eyre
        - Unit testing
        "#;

        let summary = generator.summarize_readme(sample_readme).await?;

        // Check that we got a non-empty summary
        assert!(!summary.is_empty());
        assert!(summary.len() > 10); // Arbitrary minimum length for a meaningful summary

        Ok(())
    }
}
