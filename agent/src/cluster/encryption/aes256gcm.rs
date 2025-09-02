use std::error::Error;
use super::EncryptionProvider;

/// An `EncryptionProvider` which uses AES-256-GCM for authenticated encryption.
pub struct Aes256Gcm;

impl EncryptionProvider for Aes256Gcm {
    type Key = [u8; 32];

    fn encrypt_with(&self, key: Self::Key, plaintext: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
        use aes_gcm::aead::{Aead, AeadCore, OsRng};
        use aes_gcm::{Aes256Gcm, Key, KeyInit};

        let key: &Key<Aes256Gcm> = &key.into();
        let cipher = Aes256Gcm::new(&key);

        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let ciphertext = cipher.encrypt(&nonce, plaintext)
            .map_err(|e| format!("Failed to encrypt gossip packet, ensure that you have provided a valid shared secret: {e:?}"))?;

        let mut result = nonce.to_vec();
        result.reserve_exact(ciphertext.len());
        result.extend(ciphertext);
        Ok(result)
    }

    fn decrypt_with(&self, key: Self::Key, ciphertext: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
        use aes_gcm::aead::{Aead, Nonce};
        use aes_gcm::{Aes256Gcm, Key, KeyInit};

        if ciphertext.len() < 12 {
            return Err("Ciphertext too short to contain nonce".into());
        }

        let key: &Key<Aes256Gcm> = &key.into();
        let cipher = Aes256Gcm::new(&key);

        let (nonce_bytes, ciphertext) = ciphertext.split_at(12);
        let nonce = Nonce::<Aes256Gcm>::from_slice(nonce_bytes);

        let plaintext = cipher.decrypt(&nonce, ciphertext)
            .map_err(|e| format!("Failed to decrypt gossip packet, ensure that you have provided the correct shared secret: {e:?}"))?;
        Ok(plaintext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_aes256gcm_encryption() -> Result<(), Box<dyn Error>> {
        let provider = Aes256Gcm;
        let key = [0u8; 32];
        let plaintext = b"Hello, world!";
        let ciphertext = provider.encrypt_with(key, plaintext)?;
        assert_ne!(ciphertext, plaintext);
        let decrypted = provider.decrypt_with(key, &ciphertext)?;
        assert_eq!(decrypted, plaintext);
        Ok(())
    }
}