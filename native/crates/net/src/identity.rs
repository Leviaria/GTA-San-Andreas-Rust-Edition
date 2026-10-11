//! Signed player identity, checked once per connection.
//!
//! A dedicated server has a long-term Ed25519 key (`server.key`). A player
//! has one secret (`identity.key`) and derives a separate Ed25519 key for
//! each server from that secret and the server's public key, so the same
//! player has a different, unlinkable license on every server.
//!
//! 1. Client `Hello` carries a fresh client nonce.
//! 2. Server `Welcome` carries its public key, a fresh server nonce and a
//!    signature over both nonces and the assigned player ID. The client
//!    checks it, so the server proves it holds the key it announced.
//! 3. Client `Auth` carries its per-server public key and a signature over
//!    the server key, both nonces, the player ID and its own key. The server
//!    checks it and the public key becomes the player's `license:`.
//!
//! Both nonces are new for every connection, so a recorded `Auth` is useless
//! later or on another connection, and a server cannot replay it elsewhere:
//! the signature names the server key, and a player's key for another server
//! is derived from that server's key, which only the player can do.
//! Signatures use `ed25519-dalek`; nothing here is home-made cryptography.
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};

const SERVER_DOMAIN: &str = "sare-server-auth-v1";
const PLAYER_DOMAIN: &str = "sare-player-auth-v1";
const KEY_DOMAIN: &str = "sare-player-key-v1";

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
pub fn unhex<const N: usize>(text: &str) -> Option<[u8; N]> {
    if text.len() != N * 2 || !text.is_ascii() {
        return None;
    }
    let mut out = [0u8; N];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}
/// 32 random bytes as 64 lowercase hex digits.
pub fn nonce() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("system randomness");
    hex(&bytes)
}
pub fn valid_hex32(text: &str) -> bool {
    unhex::<32>(text).is_some()
}

/// A dedicated server's long-term signing key.
pub struct ServerKey(SigningKey);
impl ServerKey {
    pub fn generate() -> Self {
        let mut seed = [0u8; 32];
        getrandom::fill(&mut seed).expect("system randomness");
        Self(SigningKey::from_bytes(&seed))
    }
    /// From the 64 hex digits stored in `server.key`.
    pub fn from_hex(text: &str) -> Option<Self> {
        unhex::<32>(text.trim()).map(|seed| Self(SigningKey::from_bytes(&seed)))
    }
    pub fn secret_hex(&self) -> String {
        hex(&self.0.to_bytes())
    }
    /// The server's identity as players see it (64 hex digits).
    pub fn public_hex(&self) -> String {
        hex(self.0.verifying_key().as_bytes())
    }
}

fn server_message(server_key: &str, client_nonce: &str, server_nonce: &str, id: u32) -> Vec<u8> {
    format!("{SERVER_DOMAIN}\n{server_key}\n{client_nonce}\n{server_nonce}\n{id}").into_bytes()
}
fn player_message(
    server_key: &str,
    client_nonce: &str,
    server_nonce: &str,
    id: u32,
    player_key: &str,
    legacy: Option<&str>,
) -> Vec<u8> {
    format!(
        "{PLAYER_DOMAIN}\n{server_key}\n{client_nonce}\n{server_nonce}\n{id}\n{player_key}\n{}",
        legacy.unwrap_or("-")
    )
    .into_bytes()
}
fn verify(key: &str, message: &[u8], signature: &str) -> bool {
    let (Some(key), Some(signature)) = (unhex::<32>(key), unhex::<64>(signature)) else {
        return false;
    };
    VerifyingKey::from_bytes(&key).is_ok_and(|key| {
        key.verify(message, &Signature::from_bytes(&signature))
            .is_ok()
    })
}

/// Server side of step 2.
pub fn sign_welcome(key: &ServerKey, client_nonce: &str, server_nonce: &str, id: u32) -> String {
    let message = server_message(&key.public_hex(), client_nonce, server_nonce, id);
    hex(&key.0.sign(&message).to_bytes())
}
/// Client side of step 2.
pub fn verify_welcome(
    server_key: &str,
    client_nonce: &str,
    server_nonce: &str,
    id: u32,
    signature: &str,
) -> bool {
    valid_hex32(server_nonce)
        && verify(
            server_key,
            &server_message(server_key, client_nonce, server_nonce, id),
            signature,
        )
}

/// The player's key for one server.
fn player_key(secret: &str, server_key: &str) -> SigningKey {
    let seed: [u8; 32] = Sha256::digest(format!("{KEY_DOMAIN}\n{secret}\n{server_key}")).into();
    SigningKey::from_bytes(&seed)
}
/// The license a player has on the server with this public key.
pub fn license(secret: &str, server_key: &str) -> String {
    hex(player_key(secret, server_key).verifying_key().as_bytes())
}
/// Client side of step 3: (player public key, signature).
pub fn sign_auth(
    secret: &str,
    server_key: &str,
    client_nonce: &str,
    server_nonce: &str,
    id: u32,
    legacy: Option<&str>,
) -> (String, String) {
    let key = player_key(secret, server_key);
    let public = hex(key.verifying_key().as_bytes());
    let message = player_message(server_key, client_nonce, server_nonce, id, &public, legacy);
    (public, hex(&key.sign(&message).to_bytes()))
}
/// Server side of step 3.
pub fn verify_auth(
    server_key: &str,
    client_nonce: &str,
    server_nonce: &str,
    id: u32,
    player_key: &str,
    legacy: Option<&str>,
    signature: &str,
) -> bool {
    let message = player_message(
        server_key,
        client_nonce,
        server_nonce,
        id,
        player_key,
        legacy,
    );
    verify(player_key, &message, signature)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_bind_server_nonces_player_id_and_legacy_claim() {
        let server = ServerKey::generate();
        let other = ServerKey::generate();
        let public = server.public_hex();
        let (client, fresh) = (nonce(), nonce());
        let proof = sign_welcome(&server, &client, &fresh, 3);
        assert!(verify_welcome(&public, &client, &fresh, 3, &proof));
        // A server cannot present another server's key without its secret.
        assert!(!verify_welcome(
            &other.public_hex(),
            &client,
            &fresh,
            3,
            &proof
        ));
        assert!(!verify_welcome(&public, &nonce(), &fresh, 3, &proof));
        assert!(!verify_welcome(&public, &client, &fresh, 4, &proof));

        let (key, signature) = sign_auth("secret", &public, &client, &fresh, 3, Some("ab"));
        assert_eq!(key, license("secret", &public));
        assert!(verify_auth(
            &public,
            &client,
            &fresh,
            3,
            &key,
            Some("ab"),
            &signature
        ));
        // Replays: another connection has other nonces or another player ID.
        assert!(!verify_auth(
            &public,
            &client,
            &nonce(),
            3,
            &key,
            Some("ab"),
            &signature
        ));
        assert!(!verify_auth(
            &public,
            &nonce(),
            &fresh,
            3,
            &key,
            Some("ab"),
            &signature
        ));
        assert!(!verify_auth(
            &public,
            &client,
            &fresh,
            5,
            &key,
            Some("ab"),
            &signature
        ));
        // The legacy claim cannot be swapped and the key cannot be swapped.
        assert!(!verify_auth(
            &public,
            &client,
            &fresh,
            3,
            &key,
            Some("cd"),
            &signature
        ));
        let stranger = license("someone-else", &public);
        assert!(!verify_auth(
            &public,
            &client,
            &fresh,
            3,
            &stranger,
            Some("ab"),
            &signature
        ));
        // Licenses differ per server and per player, and keys round-trip.
        assert_ne!(
            license("secret", &public),
            license("secret", &other.public_hex())
        );
        let restored = ServerKey::from_hex(&server.secret_hex()).unwrap();
        assert_eq!(restored.public_hex(), public);
        assert!(ServerKey::from_hex("zz").is_none());
    }
}
