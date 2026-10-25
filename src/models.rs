//! First-run model downloads from Hugging Face: pinned revisions, resumable, SHA-256 verified.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

use crate::config;

pub struct ModelFile {
    pub repo: &'static str,
    pub revision: &'static str,
    pub name: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

pub struct Model {
    pub dir: &'static str,
    pub files: &'static [ModelFile],
}

const PARAKEET_REPO: &str = "istupakov/parakeet-tdt-0.6b-v3-onnx";
const PARAKEET_REV: &str = "8f23f0c03c8761650bdb5b40aaf3e40d2c15f1ce";

/// Parakeet TDT 0.6B v3, int8 (CC-BY-4.0, NVIDIA; ONNX export by Ilya Stupakov).
pub const SPEECH: Model = Model {
    dir: "parakeet-tdt-0.6b-v3-int8",
    files: &[
        ModelFile {
            repo: PARAKEET_REPO,
            revision: PARAKEET_REV,
            name: "config.json",
            size: 97,
            sha256: "666903c76b9798caf2c210afd4f6cd60b08a8dbf9800ec8d7a3bc0d2148ac466",
        },
        ModelFile {
            repo: PARAKEET_REPO,
            revision: PARAKEET_REV,
            name: "vocab.txt",
            size: 93_939,
            sha256: "d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d",
        },
        ModelFile {
            repo: PARAKEET_REPO,
            revision: PARAKEET_REV,
            name: "decoder_joint-model.int8.onnx",
            size: 18_202_004,
            sha256: "eea7483ee3d1a30375daedc8ed83e3960c91b098812127a0d99d1c8977667a70",
        },
        ModelFile {
            repo: PARAKEET_REPO,
            revision: PARAKEET_REV,
            name: "encoder-model.int8.onnx",
            size: 652_183_999,
            sha256: "6139d2fa7e1b086097b277c7149725edbab89cc7c7ae64b23c741be4055aff09",
        },
    ],
};

/// Qwen3.5 2B Instruct, Q4_K_M GGUF (Apache-2.0, Qwen team; quantized by Unsloth).
pub const CLEANUP: Model = Model {
    dir: "qwen3.5-2b",
    files: &[ModelFile {
        repo: "unsloth/Qwen3.5-2B-GGUF",
        revision: "f6d5376be1edb4d416d56da11e5397a961aca8ae",
        name: "Qwen3.5-2B-Q4_K_M.gguf",
        size: 1_280_835_840,
        sha256: "aaf42c8b7c3cab2bf3d69c355048d4a0ee9973d48f16c731c0520ee914699223",
    }],
};

impl Model {
    pub fn path(&self) -> PathBuf {
        config::models_dir().join(self.dir)
    }

    pub fn total_size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }

    /// All files present at their final size (checksums are verified when downloading).
    pub fn is_ready(&self) -> bool {
        self.files.iter().all(|f| std::fs::metadata(self.path().join(f.name)).is_ok_and(|m| m.len() == f.size))
    }

    /// Downloads missing files, retrying with backoff. `progress` gets bytes done out of `total_size()`.
    pub fn download(&self, progress: &dyn Fn(u64)) -> Result<()> {
        std::fs::create_dir_all(self.path())?;
        let mut done = 0;
        for file in self.files {
            let mut attempt = 0;
            loop {
                match self.fetch(file, &|n| progress(done + n)) {
                    Ok(()) => break,
                    Err(e) if attempt < 5 => {
                        attempt += 1;
                        log::warn!("download of {} failed (attempt {attempt}): {e:#}", file.name);
                        std::thread::sleep(Duration::from_secs(2u64.pow(attempt)));
                    }
                    Err(e) => return Err(e),
                }
            }
            done += file.size;
            progress(done);
        }
        Ok(())
    }

    fn fetch(&self, file: &ModelFile, progress: &dyn Fn(u64)) -> Result<()> {
        let target = self.path().join(file.name);
        if std::fs::metadata(&target).is_ok_and(|m| m.len() == file.size) {
            return Ok(());
        }
        let partial = self.path().join(format!("{}.part", file.name));
        let mut have = std::fs::metadata(&partial).map_or(0, |m| m.len());
        if have > file.size {
            std::fs::remove_file(&partial)?;
            have = 0;
        }
        if have < file.size {
            let url = format!("https://huggingface.co/{}/resolve/{}/{}", file.repo, file.revision, file.name);
            let response = ureq::get(&url).header("Range", format!("bytes={have}-")).call()?;
            if response.status() != 206 {
                have = 0; // server ignored the range; start over
            }
            let mut out =
                OpenOptions::new().create(true).write(true).append(have > 0).truncate(have == 0).open(&partial)?;
            let mut body = response.into_body().into_reader();
            let mut buf = vec![0u8; 1 << 20];
            let mut last_report = 0;
            loop {
                let n = body.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                out.write_all(&buf[..n])?;
                have += n as u64;
                if have - last_report > 4 << 20 {
                    progress(have);
                    last_report = have;
                }
            }
            out.flush()?;
        }
        if have != file.size {
            bail!("{} is {have} bytes, expected {}", file.name, file.size);
        }
        let digest = sha256_file(&partial)?;
        if digest != file.sha256 {
            std::fs::remove_file(&partial)?;
            bail!("{} failed its checksum", file.name);
        }
        std::fs::rename(&partial, &target).context("moving model into place")?;
        log::info!("downloaded {}", file.name);
        Ok(())
    }
}

fn sha256_file(path: &PathBuf) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}
