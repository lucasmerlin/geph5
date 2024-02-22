use std::{collections::VecDeque, io::Write};

use arrayref::array_ref;
use blake3::derive_key;
use chacha20poly1305::{AeadInPlace, ChaCha20Poly1305, Key, KeyInit};
use smallvec::{SmallVec, ToSmallVec};

pub struct State {
    send_aead: ChaCha20Poly1305,
    send_nonce: u64,
    send_buf: Vec<u8>,
    recv_aead: ChaCha20Poly1305,
    recv_nonce: u64,
}

impl State {
    /// Derives a state from a given shared secret.
    pub fn new(ss: &[u8], is_server: bool) -> Self {
        let (send_key_label, recv_key_label) = if is_server {
            ("dn", "up")
        } else {
            ("up", "dn")
        };
        let send_key = derive_key(send_key_label, ss);
        let send_key = Key::from_slice(&send_key);
        let recv_key = derive_key(recv_key_label, ss);
        let recv_key = Key::from_slice(&recv_key);

        let send_aead = ChaCha20Poly1305::new(send_key);
        let recv_aead = ChaCha20Poly1305::new(recv_key);

        State {
            send_aead,
            send_nonce: 0,
            send_buf: vec![],
            recv_aead,
            recv_nonce: 0,
        }
    }

    fn send_nonce(&mut self) -> [u8; 12] {
        let mut nonce = [0u8; 12];
        nonce[..8].copy_from_slice(&self.send_nonce.to_le_bytes());
        self.send_nonce += 1;
        nonce
    }

    /// Encrypts a hunk of data.
    pub fn encrypt(&mut self, bts: &[u8], output: &mut Vec<u8>) {
        let mut length = (bts.len() as u32).to_le_bytes();

        // Pad the nonce to 96 bits (12 bytes)
        let nonce = self.send_nonce();

        // Encrypt the length with send_aead
        let tag_length = self
            .send_aead
            .encrypt_in_place_detached(&nonce.into(), &[], &mut length)
            .expect("encryption failure!");

        // Append the encrypted length and its tag to the output
        output.extend_from_slice(&length);
        output.extend_from_slice(tag_length.as_slice());

        // Prepare the next nonce for the body encryption
        let nonce = self.send_nonce();

        // Encrypt the body with send_aead
        self.send_buf.clear();
        self.send_buf.extend_from_slice(bts);
        let tag_body = self
            .send_aead
            .encrypt_in_place_detached(&nonce.into(), &[], &mut self.send_buf)
            .expect("encryption failure!");

        // Append the encrypted body and its tag to the output
        output.extend_from_slice(&self.send_buf);
        output.extend_from_slice(tag_body.as_slice());
    }

    fn recv_nonce(&self, offset: u64) -> [u8; 12] {
        let mut nonce = [0u8; 12];
        nonce[..8].copy_from_slice(&(self.recv_nonce + offset).to_le_bytes());
        nonce
    }

    /// Decrypts a hunk of data. If InvalidData is returned, this function may return correctly if more bytes are given.
    pub fn decrypt(&mut self, bts: &[u8], mut output: impl Write) -> Result<usize, std::io::Error> {
        if bts.len() < 24 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Encrypted data is too short",
            ));
        }

        // Split the input into the encrypted length, its tag, the encrypted body, and its tag
        let mut enc_length = *array_ref![bts, 0, 4];
        let tag_length = *array_ref![bts, 4, 16];
        let (_, rest) = bts.split_at(4);
        let (_, rest) = rest.split_at(16);
        let (enc_body, tag_body) = rest.split_at(rest.len() - 16);

        // Prepare the nonce for the length decryption
        let nonce = self.recv_nonce(0);

        // Decrypt the length with recv_aead
        self.recv_aead
            .decrypt_in_place_detached(
                &nonce.into(),
                &[],
                &mut enc_length,
                array_ref![tag_length, 0, 16].into(),
            )
            .map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "Decryption failed")
            })?;
        let length = u32::from_le_bytes(enc_length) as usize;

        // Check the length for sanity
        if length > enc_body.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Decrypted length does not match body length",
            ));
        }

        let mut enc_body: SmallVec<[u8; 32768]> = enc_body[..length].to_smallvec();

        // Prepare the next nonce for the body decryption
        let nonce = self.recv_nonce(1);

        // Decrypt the body with recv_aead

        self.recv_aead
            .decrypt_in_place_detached(
                &nonce.into(),
                &[],
                &mut enc_body,
                array_ref![tag_body, 0, 16].into(),
            )
            .map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::BrokenPipe, "Decryption failed")
            })?;

        // Append the decrypted body to the output
        output.write_all(&enc_body).unwrap();
        self.recv_nonce += 2;
        Ok(enc_length.len() + tag_length.len() + enc_body.len() + enc_body.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::OsRng;
    use x25519_dalek::EphemeralSecret;

    #[test]
    fn test_state_encryption_decryption() {
        let secret = EphemeralSecret::random_from_rng(OsRng);
        let shared_secret = x25519_dalek::PublicKey::from(&secret).as_bytes().to_vec();

        let mut state = State::new(&shared_secret, false);

        let data = b"Hello, world!";
        let mut encrypted_data = vec![];
        state.encrypt(data, &mut encrypted_data);

        let mut state = State::new(&shared_secret, true);
        let mut decrypted_data = vec![];
        state.decrypt(&encrypted_data, &mut decrypted_data).unwrap();

        assert_eq!(data, decrypted_data.as_slice());
    }

    #[test]
    fn test_state_encrypt_decrypt_multiple() {
        let secret = EphemeralSecret::random_from_rng(OsRng);
        let shared_secret = x25519_dalek::PublicKey::from(&secret).as_bytes().to_vec();

        let mut state = State::new(&shared_secret, false);

        let data1 = b"Hello, world!";
        let mut encrypted_data1 = vec![];
        state.encrypt(data1, &mut encrypted_data1);

        let data2 = b"Goodbye, world!";
        let mut encrypted_data2 = vec![];
        state.encrypt(data2, &mut encrypted_data2);

        let mut state = State::new(&shared_secret, true);
        let mut decrypted_data1 = vec![];
        state
            .decrypt(&encrypted_data1, &mut decrypted_data1)
            .unwrap();

        let mut decrypted_data2 = vec![];
        state
            .decrypt(&encrypted_data2, &mut decrypted_data2)
            .unwrap();

        assert_eq!(data1, decrypted_data1.as_slice());
        assert_eq!(data2, decrypted_data2.as_slice());
    }
}