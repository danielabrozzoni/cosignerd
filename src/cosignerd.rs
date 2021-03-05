use crate::config::{datadir_path, Config, ConfigError, ManagerConfig};

use revault_net::{noise::SecretKey as NoisePrivKey, sodiumoxide};

use std::{
    fs,
    io::{self, Read, Write},
    net::SocketAddr,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::PathBuf,
};

/// An error occuring initializing our global state
#[derive(Debug)]
pub enum CosignerDError {
    NoiseKeyError(io::Error),
    ConfigError(ConfigError),
    DatadirCreation(io::Error),
}

impl std::fmt::Display for CosignerDError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::NoiseKeyError(e) => write!(f, "Noise key initialization error: '{}'", e),
            Self::ConfigError(e) => write!(f, "Configuration error: '{}'", e),
            Self::DatadirCreation(e) => write!(f, "Creating data directory: '{}'", e),
        }
    }
}

/// Our global state
#[derive(Debug)]
pub struct CosignerD {
    pub managers: Vec<ManagerConfig>,

    pub noise_privkey: NoisePrivKey,

    pub listen: SocketAddr,
    // We store all our data in one place, that's here.
    pub data_dir: PathBuf,
}

// The communication keys are (for now) hot, so we just create it ourselves on first run.
fn read_or_create_noise_key(secret_file: &PathBuf) -> Result<NoisePrivKey, CosignerDError> {
    let mut noise_secret = NoisePrivKey([0; 32]);

    if !secret_file.as_path().exists() {
        log::info!(
            "No Noise private key at '{:?}', generating a new one",
            secret_file
        );
        noise_secret = sodiumoxide::crypto::box_::gen_keypair().1;

        // We create it in read-only but open it in write only.
        let mut options = fs::OpenOptions::new();
        options = options.write(true).create_new(true).mode(0o400).clone();

        let mut fd = options
            .open(secret_file)
            .map_err(CosignerDError::NoiseKeyError)?;
        fd.write_all(&noise_secret.as_ref())
            .map_err(CosignerDError::NoiseKeyError)?;
    } else {
        let mut noise_secret_fd =
            fs::File::open(secret_file).map_err(CosignerDError::NoiseKeyError)?;
        noise_secret_fd
            .read_exact(&mut noise_secret.0)
            .map_err(CosignerDError::NoiseKeyError)?;
    }

    // TODO: have a decent memory management and mlock() the key

    assert!(noise_secret.0 != [0; 32]);
    Ok(noise_secret)
}

pub fn create_datadir(datadir_path: &PathBuf) -> Result<(), std::io::Error> {
    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700).recursive(true).create(datadir_path)
}

impl CosignerD {
    pub fn from_config(config: Config) -> Result<Self, CosignerDError> {
        let managers = config.managers;
        let listen = config.listen;

        let mut data_dir = config
            .data_dir
            .unwrap_or(datadir_path().map_err(CosignerDError::ConfigError)?);
        if !data_dir.as_path().exists() {
            create_datadir(&data_dir).map_err(CosignerDError::DatadirCreation)?;
        }
        data_dir = fs::canonicalize(data_dir).map_err(CosignerDError::DatadirCreation)?;

        let mut noise_key_path = data_dir.clone();
        noise_key_path.push("noise_secret");
        let noise_privkey = read_or_create_noise_key(&noise_key_path)?;

        Ok(CosignerD {
            managers,
            noise_privkey,
            listen,
            data_dir,
        })
    }

    fn file_from_datadir(&self, file_name: &str) -> PathBuf {
        let data_dir_str = self
            .data_dir
            .to_str()
            .expect("Impossible: the datadir path is valid unicode");

        [data_dir_str, file_name].iter().collect()
    }

    pub fn log_file(&self) -> PathBuf {
        self.file_from_datadir("log")
    }

    pub fn pid_file(&self) -> PathBuf {
        self.file_from_datadir("cosignerd.pid")
    }

    pub fn db_file(&self) -> PathBuf {
        self.file_from_datadir("cosignerd.sqlite3")
    }
}
