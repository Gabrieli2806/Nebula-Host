//! Remembered Microsoft sign-ins: one refresh token per Minecraft profile, so
//! a restart does not ask for the device code again. Access tokens are never
//! stored; they are minted again from the refresh token when needed.
//!
//! The file lives in the app's config folder. On Windows its contents are
//! sealed with DPAPI (only this Windows user can open them); on Android the
//! folder is app-private; elsewhere the file is created owner-only.
use crate::{error::AppResult, store::paths};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone, Serialize, Deserialize)]
pub struct Remembered {
    pub name: String,
    pub refresh_token: String,
}

fn path() -> AppResult<PathBuf> {
    Ok(paths::config_dir()?.join("launcher-accounts.bin"))
}

/// Missing, unreadable or foreign files read as "nobody remembered": the
/// worst case is one more sign-in, never a crash.
pub fn load() -> BTreeMap<String, Remembered> {
    let Ok(path) = path() else { return BTreeMap::new() };
    let Ok(sealed) = std::fs::read(path) else { return BTreeMap::new() };
    unseal(&sealed)
        .and_then(|plain| serde_json::from_slice(&plain).ok())
        .unwrap_or_default()
}

pub fn save(accounts: &BTreeMap<String, Remembered>) -> AppResult<()> {
    let path = path()?;
    if accounts.is_empty() {
        let _ = std::fs::remove_file(&path);
        return Ok(());
    }
    let sealed = seal(&serde_json::to_vec(accounts)?)?;
    let temporary = path.with_extension("bin.tmp");
    write_private(&temporary, &sealed)?;
    std::fs::rename(&temporary, &path)?;
    Ok(())
}

#[cfg(unix)]
fn write_private(path: &std::path::Path, bytes: &[u8]) -> AppResult<()> {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
#[cfg(not(unix))]
fn write_private(path: &std::path::Path, bytes: &[u8]) -> AppResult<()> {
    std::fs::write(path, bytes)?;
    Ok(())
}

#[cfg(windows)]
fn seal(plain: &[u8]) -> AppResult<Vec<u8>> {
    dpapi(plain, true).ok_or_else(|| std::io::Error::other("DPAPI could not protect the sign-in").into())
}
#[cfg(windows)]
fn unseal(sealed: &[u8]) -> Option<Vec<u8>> {
    dpapi(sealed, false)
}
#[cfg(windows)]
fn dpapi(input: &[u8], protect: bool) -> Option<Vec<u8>> {
    use windows_sys::Win32::{
        Foundation::LocalFree,
        Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB},
    };
    let mut source = CRYPT_INTEGER_BLOB { cbData: input.len() as u32, pbData: input.as_ptr() as *mut u8 };
    let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
    // SAFETY: both blobs outlive the call; the output buffer is LocalAlloc'd
    // by the OS and freed below after copying.
    let ok = unsafe {
        if protect {
            CryptProtectData(&mut source, std::ptr::null(), std::ptr::null(), std::ptr::null(), std::ptr::null(), 0, &mut out)
        } else {
            CryptUnprotectData(&mut source, std::ptr::null_mut(), std::ptr::null(), std::ptr::null(), std::ptr::null(), 0, &mut out)
        }
    };
    if ok == 0 || out.pbData.is_null() {
        return None;
    }
    let bytes = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
    unsafe { LocalFree(out.pbData as _) };
    Some(bytes)
}

#[cfg(not(windows))]
fn seal(plain: &[u8]) -> AppResult<Vec<u8>> {
    Ok(plain.to_vec())
}
#[cfg(not(windows))]
fn unseal(sealed: &[u8]) -> Option<Vec<u8>> {
    Some(sealed.to_vec())
}
