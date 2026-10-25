//! Parakeet TDT 0.6B v3 (int8 ONNX) on the CPU. Port of onnx-asr's NeMo TDT greedy decoding.

use std::path::Path;

use anyhow::{Context, Result, anyhow};
use ort::session::Session;
use ort::value::Tensor;

/// NeMo 128-bin log-mel feature extractor from onnx-asr (MIT, (c) 2025 Ilya Stupakov).
const PREPROCESSOR: &[u8] = include_bytes!("../assets/nemo128.onnx");
const MAX_TOKENS_PER_STEP: usize = 10;
const STATE_LAYERS: usize = 2;
const STATE_SIZE: usize = 640;

pub struct Parakeet {
    pre: Session,
    encoder: Session,
    decoder: Session,
    vocab: Vec<String>,
    blank: usize,
}

impl Parakeet {
    pub fn load(dir: &Path) -> Result<Self> {
        let env = ort::init().with_name("mew").build()?;
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(8);
        let session = |model: Option<&Path>| -> Result<Session> {
            let mut builder = Session::builder(&env)?.with_intra_threads(threads).map_err(|e| anyhow!("{e}"))?;
            Ok(match model {
                Some(path) => builder.commit_from_file(path)?,
                None => builder.commit_from_memory(PREPROCESSOR)?,
            })
        };
        let mut vocab = Vec::new();
        for line in std::fs::read_to_string(dir.join("vocab.txt"))?.lines() {
            let (token, id) = line.rsplit_once(' ').context("bad vocab line")?;
            let id: usize = id.parse()?;
            vocab.resize(vocab.len().max(id + 1), String::new());
            vocab[id] = token.replace('\u{2581}', " ");
        }
        let blank = vocab.iter().position(|t| t == "<blk>").context("vocab has no <blk>")?;
        let mut asr = Self {
            pre: session(None)?,
            encoder: session(Some(&dir.join("encoder-model.int8.onnx")))?,
            decoder: session(Some(&dir.join("decoder_joint-model.int8.onnx")))?,
            vocab,
            blank,
        };
        asr.transcribe(&vec![0.0; 16_000])?; // warm up
        Ok(asr)
    }

    /// 16 kHz mono samples in, text out.
    pub fn transcribe(&mut self, audio: &[f32]) -> Result<String> {
        if audio.len() < 1600 {
            return Ok(String::new());
        }
        let n = audio.len();
        let out = self.pre.run(ort::inputs![
            "waveforms" => Tensor::from_array(([1usize, n], audio.to_vec()))?,
            "waveforms_lens" => Tensor::from_array(([1usize], vec![n as i64]))?,
        ])?;
        let (shape, features) = out["features"].try_extract_tensor::<f32>()?;
        let (shape, features) = (shape.to_vec(), features.to_vec());
        let feature_len = out["features_lens"].try_extract_tensor::<i64>()?.1[0];
        drop(out);

        let out = self.encoder.run(ort::inputs![
            "audio_signal" => Tensor::from_array((shape, features))?,
            "length" => Tensor::from_array(([1usize], vec![feature_len]))?,
        ])?;
        let (shape, encoded) = out["outputs"].try_extract_tensor::<f32>()?;
        let (dim, frames) = (shape[1] as usize, shape[2] as usize); // [1, D, T]
        let encoded = encoded.to_vec();
        let encoded_len = (out["encoded_lengths"].try_extract_tensor::<i64>()?.1[0] as usize).min(frames);
        drop(out);

        let tokens = self.decode(&encoded, dim, frames, encoded_len)?;
        Ok(detokenize(tokens.iter().map(|&t| self.vocab[t].as_str())))
    }

    fn decode(&mut self, encoded: &[f32], dim: usize, frames: usize, len: usize) -> Result<Vec<usize>> {
        let vocab_size = self.vocab.len();
        let mut state = (vec![0f32; STATE_LAYERS * STATE_SIZE], vec![0f32; STATE_LAYERS * STATE_SIZE]);
        let mut tokens: Vec<usize> = Vec::new();
        let (mut t, mut emitted) = (0, 0);
        while t < len {
            // Encoder output is [D, T]; take column t as a [1, D, 1] frame.
            let frame: Vec<f32> = (0..dim).map(|d| encoded[d * frames + t]).collect();
            let prev = *tokens.last().unwrap_or(&self.blank) as i32;
            let out = self.decoder.run(ort::inputs![
                "encoder_outputs" => Tensor::from_array(([1usize, dim, 1], frame))?,
                "targets" => Tensor::from_array(([1usize, 1], vec![prev]))?,
                "target_length" => Tensor::from_array(([1usize], vec![1i32]))?,
                "input_states_1" => Tensor::from_array(([STATE_LAYERS, 1, STATE_SIZE], state.0.clone()))?,
                "input_states_2" => Tensor::from_array(([STATE_LAYERS, 1, STATE_SIZE], state.1.clone()))?,
            ])?;
            let logits = out["outputs"].try_extract_tensor::<f32>()?.1;
            let token = argmax(&logits[..vocab_size]);
            let step = argmax(&logits[vocab_size..]);
            if token != self.blank {
                state = (
                    out["output_states_1"].try_extract_tensor::<f32>()?.1.to_vec(),
                    out["output_states_2"].try_extract_tensor::<f32>()?.1.to_vec(),
                );
                tokens.push(token);
                emitted += 1;
            }
            if step > 0 {
                t += step;
                emitted = 0;
            } else if token == self.blank || emitted == MAX_TOKENS_PER_STEP {
                t += 1;
                emitted = 0;
            }
        }
        Ok(tokens)
    }
}

fn argmax(xs: &[f32]) -> usize {
    xs.iter().enumerate().fold(0, |best, (i, &x)| if x > xs[best] { i } else { best })
}

/// Joins SentencePiece pieces like onnx-asr: drop a leading space and spaces not followed by a word character.
fn detokenize<'a>(pieces: impl Iterator<Item = &'a str>) -> String {
    let joined: String = pieces.collect();
    let chars: Vec<char> = joined.chars().collect();
    let mut out = String::with_capacity(joined.len());
    for (i, &c) in chars.iter().enumerate() {
        if c.is_whitespace() {
            let next_is_word = chars.get(i + 1).is_some_and(|n| n.is_alphanumeric() || *n == '_');
            if i > 0 && next_is_word {
                out.push(' ');
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::detokenize;

    #[test]
    fn detokenize_matches_onnx_asr_spacing() {
        assert_eq!(detokenize([" Hello", ",", " world", " ."].into_iter()), "Hello, world.");
        assert_eq!(detokenize([" a", "  b"].into_iter()), "a b");
    }
}
