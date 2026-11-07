use std::{
    env, fs,
    io::{self, Write},
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Mutex,
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use vpn_geo_app::{AppError, AppResult, ports::StateStore};
use vpn_geo_domain::{APP_NAME, config::valid_country};

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct State {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    last_country: String,
}

pub struct FileStateStore {
    dir: PathBuf,
    lock: Mutex<Option<fs::File>>,
}

impl Default for FileStateStore {
    fn default() -> Self {
        let dir = if let Some(base) = env::var_os("XDG_STATE_HOME") {
            PathBuf::from(base).join(APP_NAME)
        } else if let Some(home) = env::var_os("HOME") {
            PathBuf::from(home).join(".local/state").join(APP_NAME)
        } else {
            PathBuf::from(".state").join(APP_NAME)
        };
        Self {
            dir,
            lock: Mutex::new(None),
        }
    }
}

#[async_trait]
impl StateStore for FileStateStore {
    async fn lock(&self) -> AppResult<bool> {
        fs::create_dir_all(&self.dir)
            .map_err(|error| AppError::state(format!("create state directory: {error}")))?;
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(self.dir.join("run.lock"))
            .map_err(|error| AppError::state(format!("open lock: {error}")))?;
        let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if result != 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::EWOULDBLOCK) {
                return Ok(false);
            }
            return Err(AppError::state(format!("lock state: {error}")));
        }
        *self.lock.lock().unwrap() = Some(file);
        Ok(true)
    }

    async fn load_country(&self) -> AppResult<Option<String>> {
        let data = match fs::read(self.dir.join("state.json")) {
            Ok(data) => data,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(AppError::state(format!("read state: {error}"))),
        };
        let state: State = serde_json::from_slice(&data)
            .map_err(|error| AppError::state(format!("decode state: {error}")))?;
        if state.schema_version > 1 {
            return Err(AppError::state(format!(
                "unsupported state schema version {}",
                state.schema_version
            )));
        }
        let country = state.last_country.trim().to_ascii_uppercase();
        if !country.is_empty() && !valid_country(&country) {
            return Err(AppError::state(format!("invalid last_country {country:?}")));
        }
        Ok(if country.is_empty() {
            None
        } else {
            Some(country)
        })
    }

    async fn save_country(&self, country: &str) -> AppResult<()> {
        let country = country.trim().to_ascii_uppercase();
        if !valid_country(&country) {
            return Err(AppError::state(format!("invalid country {country:?}")));
        }
        fs::create_dir_all(&self.dir)
            .map_err(|error| AppError::state(format!("create state directory: {error}")))?;
        let data = serde_json::to_vec_pretty(&State {
            schema_version: 1,
            last_country: country,
        })
        .map_err(|error| AppError::state(format!("encode state: {error}")))?;
        let temp = self
            .dir
            .join(format!("state.json.{}.tmp", std::process::id()));
        write_private(&temp, &data)?;
        fs::rename(&temp, self.dir.join("state.json"))
            .map_err(|error| AppError::state(format!("replace state atomically: {error}")))?;
        Ok(())
    }

    async fn unlock(&self) {
        if let Some(file) = self.lock.lock().unwrap().take() {
            unsafe {
                libc::flock(file.as_raw_fd(), libc::LOCK_UN);
            }
        }
    }
}

fn write_private(path: &Path, data: &[u8]) -> AppResult<()> {
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| AppError::state(format!("create temporary state: {error}")))?;
    file.write_all(data)
        .and_then(|_| file.sync_all())
        .map_err(|error| {
            let _ = fs::remove_file(path);
            AppError::state(format!("write state: {error}"))
        })
}
