//! ONNX models for the `hybrid` and `nli` tiers (`semantic` feature).
//!
//! Files come from pinned Hugging Face revisions, are verified against
//! pinned SHA-256s on every load, and live in `<models_dir>/<name>/`. A
//! missing model is downloaded once, with one log line first, unless
//! `REASONING_ENGINE_OFFLINE=1`, which refuses rather than downloads.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use ort::environment::Environment;
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Tensor;
use sha2::{Digest, Sha256};
use tokenizers::{EncodeInput, PaddingParams, Tokenizer, TruncationParams};

use crate::hybrid::Embedder;
use crate::nli::{NliModel, NliScores};

/// Longest input either model sees, in tokens.
pub const MAX_TOKENS: usize = 512;

/// One file of a model.
#[derive(Debug, Clone, Copy)]
pub struct ModelFile {
    /// Path inside the repository at the pinned revision.
    pub remote: &'static str,
    /// File name under the model directory.
    pub local: &'static str,
    /// Expected SHA-256, lowercase hex.
    pub sha256: &'static str,
}

/// A pinned model.
#[derive(Debug, Clone, Copy)]
pub struct ModelSpec {
    /// Directory name and identity.
    pub name: &'static str,
    /// Hugging Face repository.
    pub repo: &'static str,
    /// Pinned commit.
    pub revision: &'static str,
    /// Files: the ONNX graph first, then the tokenizer.
    pub files: [ModelFile; 2],
}

impl ModelSpec {
    /// `name@<short revision>`, recorded in run-pack methodology.
    #[must_use]
    pub fn id(&self) -> String {
        format!("{}@{}", self.name, &self.revision[..12])
    }
}

/// `BAAI/bge-small-en-v1.5`, int8-quantised ONNX (as `fastembed` ships it).
pub const BGE_SMALL: ModelSpec = ModelSpec {
    name: "bge-small-en-v1.5-q",
    repo: "Qdrant/bge-small-en-v1.5-onnx-Q",
    revision: "aa8f8b060edb00e03bfdd08813a2949946c8ba55",
    files: [
        ModelFile {
            remote: "model_optimized.onnx",
            local: "model.onnx",
            sha256: "51f1bd0addd6e859e42c2c8021a5e5461385bb676a649f4b269aa445449f2431",
        },
        ModelFile {
            remote: "tokenizer.json",
            local: "tokenizer.json",
            sha256: "d241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66",
        },
    ],
};

/// `cross-encoder/nli-deberta-v3-xsmall`; labels contradiction, entailment,
/// neutral (from its `config.json`).
pub const NLI_DEBERTA_XSMALL: ModelSpec = ModelSpec {
    name: "nli-deberta-v3-xsmall",
    repo: "cross-encoder/nli-deberta-v3-xsmall",
    revision: "a150876415327c80daeff35ca6f68f5ed8cf5c24",
    files: [
        ModelFile {
            remote: "onnx/model.onnx",
            local: "model.onnx",
            sha256: "7105da41f625c42eca24e9465ec99150d02a80e644659d7a1daa93a6357155d4",
        },
        ModelFile {
            remote: "tokenizer.json",
            local: "tokenizer.json",
            sha256: "5124ef2ead1a10a717703bc436de7f353da76d6340e4587719b42b1693707964",
        },
    ],
};

/// Why a model is unavailable.
#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    /// Missing and downloads are off.
    #[error(
        "model {0} is not installed and REASONING_ENGINE_OFFLINE=1 forbids downloading it; \
         run once online or use the lexical verifier"
    )]
    Offline(&'static str),
    /// A file's hash does not match the pin.
    #[error("{path}: SHA-256 {actual} does not match the pinned {expected}")]
    Hash {
        /// File checked.
        path: PathBuf,
        /// Pinned hash.
        expected: &'static str,
        /// Hash found.
        actual: String,
    },
    /// Download failed.
    #[error("downloading {0}: {1}")]
    Download(String, String),
    /// Filesystem failure.
    #[error("model io error: {0}")]
    Io(#[from] std::io::Error),
    /// ONNX Runtime or tokenizer failure.
    #[error("loading model {0}: {1}")]
    Load(&'static str, String),
}

pub use crate::service::default_models_dir;

fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    let mut file = std::fs::File::open(path)?;
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let read = std::io::Read::read(&mut file, &mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let mut hex = String::with_capacity(64);
    for byte in hasher.finalize() {
        let _ = std::fmt::Write::write_fmt(&mut hex, format_args!("{byte:02x}"));
    }
    Ok(hex)
}

fn check(path: &Path, expected: &'static str) -> Result<(), ModelError> {
    let actual = sha256_file(path)?;
    if actual == expected {
        Ok(())
    } else {
        Err(ModelError::Hash {
            path: path.to_path_buf(),
            expected,
            actual,
        })
    }
}

async fn download(spec: &ModelSpec, file: &ModelFile, target: &Path) -> Result<(), ModelError> {
    let url = format!(
        "https://huggingface.co/{}/resolve/{}/{}",
        spec.repo, spec.revision, file.remote
    );
    let fail = |e: &dyn std::fmt::Display| ModelError::Download(url.clone(), e.to_string());
    let client = reqwest::Client::builder()
        .user_agent(re_retrieval::base::user_agent())
        .build()
        .map_err(|e| fail(&e))?;
    let mut response = client.get(&url).send().await.map_err(|e| fail(&e))?;
    if !response.status().is_success() {
        return Err(fail(&response.status()));
    }
    // Write beside the target and rename only after the hash matches, so a
    // partial download never looks installed.
    let partial = target.with_extension("partial");
    let mut out = std::fs::File::create(&partial)?;
    while let Some(chunk) = response.chunk().await.map_err(|e| fail(&e))? {
        std::io::Write::write_all(&mut out, &chunk)?;
    }
    drop(out);
    if let Err(error) = check(&partial, file.sha256) {
        let _ = std::fs::remove_file(&partial);
        return Err(error);
    }
    std::fs::rename(&partial, target)?;
    Ok(())
}

/// Make sure every file of `spec` is present and verified; returns the
/// model directory.
///
/// # Errors
/// Offline with files missing, a hash mismatch, or a failed download.
pub async fn ensure(
    spec: &ModelSpec,
    models_dir: &Path,
    offline: bool,
) -> Result<PathBuf, ModelError> {
    let dir = models_dir.join(spec.name);
    let missing: Vec<&ModelFile> = spec
        .files
        .iter()
        .filter(|f| !dir.join(f.local).exists())
        .collect();
    if !missing.is_empty() {
        if offline {
            return Err(ModelError::Offline(spec.name));
        }
        std::fs::create_dir_all(&dir)?;
        eprintln!(
            "downloading model {} from huggingface.co/{} to {}",
            spec.id(),
            spec.repo,
            dir.display()
        );
        for file in missing {
            download(spec, file, &dir.join(file.local)).await?;
        }
    }
    for file in &spec.files {
        check(&dir.join(file.local), file.sha256)?;
    }
    Ok(dir)
}

/// One ONNX Runtime environment per process, shared by both models.
static ENVIRONMENT: OnceLock<Result<Environment, String>> = OnceLock::new();

fn load(spec: &'static ModelSpec, dir: &Path) -> Result<(Mutex<Session>, Tokenizer), ModelError> {
    let fail = |e: &dyn std::fmt::Display| ModelError::Load(spec.name, e.to_string());
    let environment = ENVIRONMENT
        .get_or_init(|| ort::init().build().map_err(|e| e.to_string()))
        .as_ref()
        .map_err(|e| fail(e))?;
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get().min(4));
    let session = Session::builder(environment)
        .map_err(|e| fail(&e))?
        .with_optimization_level(GraphOptimizationLevel::Level3)
        .map_err(|e| fail(&e))?
        .with_intra_threads(threads)
        .map_err(|e| fail(&e))?
        .commit_from_file(dir.join(spec.files[0].local))
        .map_err(|e| fail(&e))?;
    let mut tokenizer =
        Tokenizer::from_file(dir.join(spec.files[1].local)).map_err(|e| fail(&e))?;
    tokenizer
        .with_truncation(Some(TruncationParams {
            max_length: MAX_TOKENS,
            ..TruncationParams::default()
        }))
        .map_err(|e| fail(&e))?;
    tokenizer.with_padding(Some(PaddingParams::default()));
    Ok((Mutex::new(session), tokenizer))
}

/// Token IDs, attention mask and (if the graph wants them) type IDs for a
/// padded batch, as `[batch, len]` tensors in the graph's input order.
fn inputs(
    session: &Session,
    encodings: &[tokenizers::Encoding],
) -> Result<Vec<(String, Tensor<i64>)>, String> {
    let batch = encodings.len();
    let len = encodings.first().map_or(0, tokenizers::Encoding::len);
    let column = |pick: fn(&tokenizers::Encoding) -> &[u32]| -> Vec<i64> {
        encodings
            .iter()
            .flat_map(|e| pick(e).iter().map(|&x| i64::from(x)))
            .collect()
    };
    session
        .inputs()
        .iter()
        .map(|input| {
            let data = match input.name() {
                "input_ids" => column(tokenizers::Encoding::get_ids),
                "attention_mask" => column(tokenizers::Encoding::get_attention_mask),
                "token_type_ids" => column(tokenizers::Encoding::get_type_ids),
                other => return Err(format!("unexpected model input {other}")),
            };
            Tensor::from_array(([batch, len], data))
                .map(|t| (input.name().to_string(), t))
                .map_err(|e| e.to_string())
        })
        .collect()
}

/// `bge-small-en-v1.5` sentence embeddings: CLS pooling, L2-normalised.
pub struct OnnxEmbedder {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
}

impl OnnxEmbedder {
    /// Load (downloading if allowed) the pinned BGE model.
    ///
    /// # Errors
    /// See [`ensure`] and [`ModelError::Load`].
    pub async fn load(models_dir: &Path, offline: bool) -> Result<Self, ModelError> {
        let dir = ensure(&BGE_SMALL, models_dir, offline).await?;
        let (session, tokenizer) = load(&BGE_SMALL, &dir)?;
        Ok(Self { session, tokenizer })
    }
}

/// Encodings are padded to one length per batch; keep batches modest.
const BATCH: usize = 16;

impl Embedder for OnnxEmbedder {
    fn id(&self) -> String {
        BGE_SMALL.id()
    }

    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, String> {
        let mut out = Vec::with_capacity(texts.len());
        for chunk in texts.chunks(BATCH) {
            let encodings = self
                .tokenizer
                .encode_batch(chunk.to_vec(), true)
                .map_err(|e| e.to_string())?;
            let mut session = self
                .session
                .lock()
                .map_err(|_| "embedding session poisoned".to_string())?;
            let feed = inputs(&session, &encodings)?;
            let outputs = session.run(feed).map_err(|e| e.to_string())?;
            let (shape, data) = outputs[0]
                .try_extract_tensor::<f32>()
                .map_err(|e| e.to_string())?;
            let (seq, hidden) = (
                usize::try_from(shape[1]).unwrap_or(0),
                usize::try_from(shape[2]).unwrap_or(0),
            );
            for row in 0..chunk.len() {
                let cls = &data[row * seq * hidden..row * seq * hidden + hidden];
                let norm = cls
                    .iter()
                    .map(|x| x * x)
                    .sum::<f32>()
                    .sqrt()
                    .max(f32::EPSILON);
                out.push(cls.iter().map(|x| x / norm).collect());
            }
        }
        Ok(out)
    }
}

/// `nli-deberta-v3-xsmall` cross-encoder: softmax over three logits.
pub struct OnnxNli {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
}

impl OnnxNli {
    /// Load (downloading if allowed) the pinned NLI model.
    ///
    /// # Errors
    /// See [`ensure`] and [`ModelError::Load`].
    pub async fn load(models_dir: &Path, offline: bool) -> Result<Self, ModelError> {
        let dir = ensure(&NLI_DEBERTA_XSMALL, models_dir, offline).await?;
        let (session, tokenizer) = load(&NLI_DEBERTA_XSMALL, &dir)?;
        Ok(Self { session, tokenizer })
    }
}

fn softmax(logits: &[f32]) -> [f32; 3] {
    let max = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let exps: Vec<f32> = logits.iter().map(|l| (l - max).exp()).collect();
    let sum: f32 = exps.iter().sum();
    [exps[0] / sum, exps[1] / sum, exps[2] / sum]
}

impl NliModel for OnnxNli {
    fn id(&self) -> String {
        NLI_DEBERTA_XSMALL.id()
    }

    fn classify(&self, pairs: &[(&str, &str)]) -> Result<Vec<NliScores>, String> {
        let mut out = Vec::with_capacity(pairs.len());
        for chunk in pairs.chunks(BATCH) {
            let batch: Vec<EncodeInput> = chunk
                .iter()
                .map(|(p, h)| EncodeInput::Dual((*p).into(), (*h).into()))
                .collect();
            let encodings = self
                .tokenizer
                .encode_batch(batch, true)
                .map_err(|e| e.to_string())?;
            let mut session = self
                .session
                .lock()
                .map_err(|_| "NLI session poisoned".to_string())?;
            let feed = inputs(&session, &encodings)?;
            let outputs = session.run(feed).map_err(|e| e.to_string())?;
            let (_, logits) = outputs[0]
                .try_extract_tensor::<f32>()
                .map_err(|e| e.to_string())?;
            for row in logits.chunks(3) {
                // Label order from the model's config.json.
                let [contradiction, entailment, neutral] = softmax(row);
                out.push(NliScores {
                    contradiction,
                    entailment,
                    neutral,
                });
            }
        }
        Ok(out)
    }
}
