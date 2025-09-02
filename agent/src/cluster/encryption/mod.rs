mod aes256gcm;
mod cleartext;

use std::sync::Arc;
pub use aes256gcm::Aes256Gcm;

/// A provider of encryption keys for message encryption and decryption.
///
/// This trait is meant to be implemented by your configuration management
/// system to enable the clustering transport to dynamically reload encryption
/// keys without requiring a restart.
pub trait EncryptionKeyProvider {
    /// The type of encryption key which is provided by your configuration system.
    ///
    /// This must match the type expected by your chosen `GossipTransport` implementation.
    type Key: Sized + Clone;

    /// Returns the key which should be used to encrypt outgoing messages.
    fn get_encryption_key(&self) -> Result<Self::Key, Box<dyn std::error::Error>>;

    /// Returns a list of keys which can be used to decrypt incoming messages.
    fn get_decryption_keys(&self) -> Result<Vec<Self::Key>, Box<dyn std::error::Error>> {
        Ok(vec![self.get_encryption_key()?.clone()])
    }
}


/// A provider of encryption and decryption functionality for messages.
pub trait EncryptionProvider {
    /// The type of encryption key which is used by this provider.
    type Key: Sized + Clone;

    /// Encrypts the given plaintext using a key provided by the given key provider, returning the ciphertext.
    fn encrypt(&self, key_provider: &dyn EncryptionKeyProvider<Key=Self::Key>, plaintext: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let key = key_provider.get_encryption_key()?;
        self.encrypt_with(key, plaintext)
    }

    /// Decrypts the given ciphertext using keys provided by the given key provider, returning the plaintext.
    fn decrypt(&self, key_provider: &dyn EncryptionKeyProvider<Key=Self::Key>, ciphertext: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let keys = key_provider.get_decryption_keys()?;
        let mut last_err = None;

        for key in keys {
            match self.decrypt_with(key.clone(), ciphertext) {
                Ok(plaintext) => return Ok(plaintext),
                Err(err) => last_err = Some(err),
            }
        }

        Err(last_err.unwrap_or_else(|| "No decryption keys available".into()))
    }

    /// Encrypts the given plaintext using the provided key, returning the ciphertext.
    ///
    /// You should usually use `encrypt` instead, which gets the key from the key provider.
    fn encrypt_with(&self, key: Self::Key, plaintext: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>>;

    /// Decrypts the given ciphertext using the provided key, returning the plaintext.
    ///
    /// You should usually use `decrypt` instead, which tries multiple keys from the key provider.
    fn decrypt_with(&self, key: Self::Key, ciphertext: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>>;
}

/// A simple `EncryptionKeyProvider` which always returns the same static key.
#[derive(Clone)]
pub struct StaticKeyProvider<K: Sized + Clone> {
    key: Arc<K>,
}

impl<K: Sized + Clone> StaticKeyProvider<K> {
    pub fn new(key: K) -> Self {
        Self {
            key: Arc::new(key),
        }
    }
}

impl<K: Sized + Clone> EncryptionKeyProvider for StaticKeyProvider<K> {
    type Key = K;

    fn get_encryption_key(&self) -> Result<Self::Key, Box<dyn std::error::Error>> {
        Ok((*self.key).clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster::encryption::aes256gcm::Aes256Gcm;

    struct TestKeyProvider {
        encryption: [u8; 32],
        decryption: Vec<[u8; 32]>,
    }

    impl EncryptionKeyProvider for TestKeyProvider {
        type Key = [u8; 32];

        fn get_encryption_key(&self) -> Result<Self::Key, Box<dyn std::error::Error>> {
            Ok(self.encryption)
        }

        fn get_decryption_keys(&self) -> Result<Vec<Self::Key>, Box<dyn std::error::Error>> {
            Ok(self.decryption.clone())
        }
    }

    #[test]
    fn test_static_key_provider() -> Result<(), Box<dyn std::error::Error>> {
        let key = [0u8; 32];
        let provider = StaticKeyProvider::new(key);
        let retrieved_key = provider.get_encryption_key()?;
        assert_eq!(retrieved_key, key);
        Ok(())
    }

    #[test]
    fn test_fallback_encryption() -> Result<(), Box<dyn std::error::Error>> {
        let provider = Aes256Gcm;
        let keys = TestKeyProvider {
            encryption: [1u8; 32],
            decryption: vec![[0u8; 32], [1u8; 32], [2u8; 32]],
        };

        let plaintext = b"Hello, world!";
        let ciphertext = provider.encrypt(&keys, plaintext)?;
        assert_ne!(ciphertext, plaintext);
        let decrypted = provider.decrypt(&keys, &ciphertext)?;
        assert_eq!(decrypted, plaintext);

        let keys = TestKeyProvider {
            encryption: [3u8; 32],
            decryption: vec![[0u8; 32], [1u8; 32], [2u8; 32]],
        };
        let ciphertext = provider.encrypt(&keys, plaintext)?;
        assert_ne!(ciphertext, plaintext);
        assert!(provider.decrypt(&keys, &ciphertext).is_err());

        Ok(())
    }
}