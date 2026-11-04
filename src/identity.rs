use ring::digest::{SHA256, digest};
use ring::rand::SystemRandom;
use ring::signature::{Ed25519KeyPair, KeyPair};

use std::env;
use std::error::Error;
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
// use std::convert::identity;

type IdentityResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

pub struct DeviceIdentity {
    key_pair: Ed25519KeyPair,
}

fn identity_error(message: &str) -> io::Error {
    io::Error::other(message)
}

impl DeviceIdentity {
    pub fn load_or_create(path: &Path) -> IdentityResult<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
        }

        match fs::symlink_metadata(path) {
            Ok(metadata) => {
                if !metadata.file_type().is_file() || metadata.permissions().mode() & 0o077 != 0 {
                    return Err(identity_error(
                        "Identity file must be regular and accessible only by its owner",
                    )
                    .into());
                }

                let bytes = fs::read(path)?;
                let key_pair = Ed25519KeyPair::from_pkcs8(&bytes)
                    .map_err(|_| identity_error("Invalid device private key"))?;

                Ok(Self { key_pair })
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Self::generate_and_save(path),
            Err(error) => Err(error.into()),
        }
    }

    fn generate_and_save(path: &Path) -> IdentityResult<Self> {
        let rng = SystemRandom::new();

        let document = Ed25519KeyPair::generate_pkcs8(&rng)
            .map_err(|_| identity_error("Failed to generate device identity"))?;

        let key_pair = Ed25519KeyPair::from_pkcs8(document.as_ref())
            .map_err(|_| identity_error("Generated private key is invalid"))?;

        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)?;

        if let Err(error) = file
            .write_all(document.as_ref())
            .and_then(|_| file.sync_all())
        {
            // Keep a failed first write form being mistake for a valid key.
            drop(file);
            let _ = fs::remove_file(path);
            return Err(error.into());
        }

        Ok(Self { key_pair })
    }

    pub fn fingerprint(&self) -> String {
        let hash = digest(&SHA256, self.key_pair.public_key().as_ref());

        let mut output = String::with_capacity(64);
        for byte in hash.as_ref() {
            write!(&mut output, "{byte:02x}").expect("Writing to a String cannot fail");
        }

        output
    }

    pub fn public_key(&self) -> &[u8] {
        self.key_pair.public_key().as_ref()
    }

    pub fn sign(&self, message: &[u8]) -> Vec<u8> {
        self.key_pair.sign(message).as_ref().to_vec()
    }
}

pub fn default_identity_path() -> IdentityResult<PathBuf> {
    let state_dir = match env::var_os("XDG_STATE_HOME") {
        Some(value) if Path::new(&value).is_absolute() => PathBuf::from(value),
        _ => {
            let home =
                env::var_os("HOME").ok_or_else(|| identity_error("HOME is not configured"))?;
            PathBuf::from(home).join(".local/state")
        }
    };

    Ok(state_dir.join("teralow/device-key.pk8"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::{ED25519, UnparsedPublicKey};

    #[test]
    fn identity_persists_across_loads() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("device-key.pk8");

        let first = DeviceIdentity::load_or_create(&path).unwrap();
        let first_fingerprint = first.fingerprint();

        let second = DeviceIdentity::load_or_create(&path).unwrap();

        assert_eq!(first_fingerprint, second.fingerprint());
        assert_eq!(first.public_key(), second.public_key());
    }

    #[test]
    fn identity_can_sign_and_verify() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("device-key.pk8");
        let identity = DeviceIdentity::load_or_create(&path).unwrap();

        let message = b"teralow-test-message";
        let signature = identity.sign(message);

        let verifier = UnparsedPublicKey::new(&ED25519, identity.public_key());

        assert!(verifier.verify(message, &signature).is_ok());
        assert!(verifier.verify(b"modified-message", &signature).is_err());
    }

    #[test]
    fn corrupt_identity_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("device-key.pk8");

        fs::write(&path, b"invalid-private-key").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();

        assert!(DeviceIdentity::load_or_create(&path).is_err());
    }
}
