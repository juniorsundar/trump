use aes_gcm::{
    Aes256Gcm, Key, Nonce,
    aead::{Aead, KeyInit},
};
use base64::{Engine as _, engine::general_purpose};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

const KEY_FILE: &str = ".trump-key";
const INFO_FILE: &str = ".trump-info";

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum AuthType {
    Password,
    KeyPath,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AuthData {
    pub auth_type: AuthType,
    pub secret: String, // Encrypted password or path to key
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Config {
    pub targets: HashMap<String, AuthData>,
}

/// Helper to get the path to a file in the user's home directory
fn get_home_path(filename: &str) -> Option<PathBuf> {
    dirs::home_dir().map(|p| p.join(filename))
}

/// Retrieves or creates the encryption key (32 bytes)
fn get_or_create_key() -> Result<Key<Aes256Gcm>, Box<dyn std::error::Error>> {
    let key_path = get_home_path(KEY_FILE).ok_or("Could not determine home directory")?;

    if key_path.exists() {
        let key_str = fs::read_to_string(&key_path)?;
        let key_bytes = general_purpose::STANDARD.decode(key_str.trim())?;
        if key_bytes.len() != 32 {
            return Err(format!("Invalid key length in {:?}!", key_path).into());
        }
        Ok(*Key::<Aes256Gcm>::from_slice(&key_bytes))
    } else {
        // Generate new key
        let mut key_bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut key_bytes);

        // Save base64 representation
        let key_str = general_purpose::STANDARD.encode(key_bytes);
        fs::write(key_path, key_str)?;

        Ok(*Key::<Aes256Gcm>::from_slice(&key_bytes))
    }
}

pub fn encrypt(data: &str) -> Result<String, Box<dyn std::error::Error>> {
    let key = get_or_create_key()?;
    let cipher = Aes256Gcm::new(&key);

    // Generate unique nonce (96-bits)
    let mut nonce_bytes = [0u8; 12];
    rand::rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, data.as_bytes())
        .map_err(|e| format!("Encryption failure: {}!", e))?;

    // Combine nonce + ciphertext
    let mut combined = nonce_bytes.to_vec();
    combined.extend(ciphertext);

    Ok(general_purpose::STANDARD.encode(combined))
}

pub fn decrypt(encrypted_data: &str) -> Result<String, Box<dyn std::error::Error>> {
    let key = get_or_create_key()?;
    let cipher = Aes256Gcm::new(&key);

    let combined = general_purpose::STANDARD.decode(encrypted_data)?;
    if combined.len() < 12 {
        return Err("Invalid encrypted data length".into());
    }

    let (nonce_bytes, ciphertext_bytes) = combined.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext_bytes)
        .map_err(|e| format!("Decryption failure: {}!", e))?;

    Ok(String::from_utf8(plaintext)?)
}

pub fn load_config() -> Result<Config, Box<dyn std::error::Error>> {
    let config_path = get_home_path(INFO_FILE).ok_or("Could not determine home directory")?;

    if config_path.exists() {
        let content = fs::read_to_string(config_path)?;
        let config: Config = serde_json::from_str(&content)?;
        Ok(config)
    } else {
        Ok(Config::default())
    }
}

pub fn save_config(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    let config_path = get_home_path(INFO_FILE).ok_or("Could not determine home directory")?;
    let content = serde_json::to_string_pretty(config)?;
    fs::write(config_path, content)?;
    Ok(())
}
