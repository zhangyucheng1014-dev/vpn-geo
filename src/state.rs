use std::{
    env, fs, io,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

pub struct Lock {
    file: fs::File,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct State {
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub last_country: String,
}

pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn default() -> Self {
        let dir = if let Some(base) = env::var_os("XDG_STATE_HOME") {
            PathBuf::from(base).join(crate::config::APP_NAME)
        } else if let Some(home) = env::var_os("HOME") {
            PathBuf::from(home)
                .join(".local")
                .join("state")
                .join(crate::config::APP_NAME)
        } else {
            PathBuf::from(".state").join(crate::config::APP_NAME)
        };
        Self { dir }
    }

    pub fn try_lock(&self) -> Result<Option<Lock>, String> {
        fs::create_dir_all(&self.dir)
            .map_err(|error| format!("create state directory: {error}"))?;
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(self.dir.join("run.lock"))
            .map_err(|error| format!("open lock: {error}"))?;
        let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if result != 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::EWOULDBLOCK) {
                return Ok(None);
            }
            return Err(format!("lock state: {error}"));
        }
        Ok(Some(Lock { file }))
    }

    pub fn load(&self) -> Result<State, String> {
        let path = self.dir.join("state.json");
        let data = match fs::read(&path) {
            Ok(data) => data,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(State::default()),
            Err(error) => return Err(format!("read state: {error}")),
        };
        let state: State =
            serde_json::from_slice(&data).map_err(|error| format!("decode state: {error}"))?;
        if state.schema_version > 1 {
            return Err(format!(
                "unsupported state schema version {}",
                state.schema_version
            ));
        }
        let country = state.last_country.trim().to_ascii_uppercase();
        if !country.is_empty() && !crate::config::valid_country(&country) {
            return Err(format!("invalid last_country {country:?}"));
        }
        Ok(State {
            last_country: country,
            ..state
        })
    }

    pub fn save(&self, country: &str) -> Result<(), String> {
        let country = country.trim().to_ascii_uppercase();
        if !crate::config::valid_country(&country) {
            return Err(format!("invalid country {country:?}"));
        }
        fs::create_dir_all(&self.dir)
            .map_err(|error| format!("create state directory: {error}"))?;
        let state = State {
            schema_version: 1,
            last_country: country,
        };
        let data =
            serde_json::to_vec_pretty(&state).map_err(|error| format!("encode state: {error}"))?;
        let temp = self
            .dir
            .join(format!("state.json.{}.tmp", std::process::id()));
        write_private(&temp, &data)?;
        fs::rename(&temp, self.dir.join("state.json"))
            .map_err(|error| format!("replace state atomically: {error}"))?;
        if let Ok(dir) = fs::File::open(&self.dir) {
            let _ = dir.sync_all();
        }
        Ok(())
    }
}

fn write_private(path: &Path, data: &[u8]) -> Result<(), String> {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| format!("create temporary state: {error}"))?;
    file.write_all(data)
        .and_then(|_| file.sync_all())
        .map_err(|error| {
            let _ = fs::remove_file(path);
            format!("write state: {error}")
        })
}

impl Drop for Lock {
    fn drop(&mut self) {
        unsafe {
            libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

trait AsRawFd {
    fn as_raw_fd(&self) -> std::os::fd::RawFd;
}

impl AsRawFd for fs::File {
    fn as_raw_fd(&self) -> std::os::fd::RawFd {
        std::os::fd::AsRawFd::as_raw_fd(self)
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    #[test]
    fn lock_is_non_blocking() {
        let dir = std::env::temp_dir().join(format!(
            "vpn-geo-lock-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = Store { dir };
        let first = store.try_lock().unwrap().expect("first lock");
        assert!(store.try_lock().unwrap().is_none());
        drop(first);
        assert!(store.try_lock().unwrap().is_some());
        let _ = fs::remove_dir_all(&store.dir);
    }

    #[test]
    fn saves_and_loads_country() {
        let dir = std::env::temp_dir().join(format!(
            "vpn-geo-state-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = Store { dir };
        store.save("jp").unwrap();
        assert_eq!(store.load().unwrap().last_country, "JP");
        let metadata = fs::metadata(store.dir.join("state.json")).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        let _ = fs::remove_dir_all(&store.dir);
    }
}
