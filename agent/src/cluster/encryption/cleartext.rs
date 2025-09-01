use std::error::Error;
use super::EncryptionProvider;

/// An `EncryptionProvider` which performs no encryption or decryption, returning the plaintext or ciphertext as-is.
pub struct Cleartext;

impl EncryptionProvider for Cleartext {
    type Key = ();

    fn encrypt_with(&self, _key: Self::Key, plaintext: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
        Ok(plaintext.to_vec())
    }

    fn decrypt_with(&self, _key: Self::Key, ciphertext: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
        Ok(ciphertext.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cleartext_encryption() -> Result<(), Box<dyn Error>> {
        let provider = Cleartext;
        let key = ();
        let plaintext = b"Hello, world!";
        let ciphertext = provider.encrypt_with(key, plaintext)?;
        assert_eq!(ciphertext, plaintext);
        let decrypted = provider.decrypt_with(key, &ciphertext)?;
        assert_eq!(decrypted, plaintext);
        Ok(())
    }
}