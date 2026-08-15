use anyhow::{Result, anyhow};
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use rand_core::OsRng;

pub fn hash_password(raw_password: &str) -> Result<String> {
    let argon2 = Argon2::default();
    let salt = SaltString::generate(&mut OsRng);
    let hashed = argon2
        .hash_password(raw_password.as_bytes(), &salt)
        .map_err(|e| anyhow!("Hash generating error occured: {e}"))?
        .to_string();
    Ok(hashed)
}

pub fn verify_password(raw_password: &str, hashed_password: &str) -> Result<bool> {
    let argon2 = Argon2::default();
    let hash = PasswordHash::new(hashed_password)
        .map_err(|e| anyhow!("Hash processing error occured: {e}"))?;
    match argon2.verify_password(raw_password.as_bytes(), &hash) {
        Ok(_) => Ok(true),

        Err(e) => Err(anyhow!("Password verification failed: {}", e)),
    }
}
