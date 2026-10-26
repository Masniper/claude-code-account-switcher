// Read a Claude Desktop profile's OAuth token.
//
// The desktop app stores credentials the way every Chromium app does on
// macOS: a random password in the Keychain under service "Claude Safe
// Storage", stretched with PBKDF2-HMAC-SHA1 (salt "saltysalt", 1003
// iterations, 16-byte key) into an AES-128-CBC key with a fixed IV of 16
// spaces. The ciphertext sits base64-encoded in the profile's config.json
// under `oauth:tokenCacheV2`, prefixed with the version marker `v10`.
//
// This is Chromium's scheme, not something Anthropic designed, but the
// contents are theirs and undocumented: a map from
// `<accountUuid>:<orgUuid>:<audience>:<scopes>` to a token record. It can
// change; every failure here is soft, and the caller falls back to the
// account uuid sitting in plaintext in the same config.json.
//
// Reading the Keychain entry prompts for the login keychain password the
// first time — the entry's ACL only lists the app itself. That is the user's
// call to make, so callers say what is about to happen before asking.

use std::path::Path;
use std::process::Command;

use aes::cipher::{BlockModeDecrypt, KeyIvInit, block_padding::Pkcs7};
use hmac::Hmac;
use sha1::Sha1;

type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;

const KEYCHAIN_SERVICE: &str = "Claude Safe Storage";
const KEYCHAIN_ACCOUNT: &str = "Claude Key";
const SALT: &[u8] = b"saltysalt";
const ITERATIONS: u32 = 1003;
const PREFIX: &[u8] = b"v10";
/// Chromium's macOS IV: sixteen spaces, not a random one.
const IV: [u8; 16] = [b' '; 16];

/// One entry of the profile's token cache.
#[derive(Debug, PartialEq)]
pub struct TokenEntry {
    pub token: String,
    /// First field of the cache key — the account this token belongs to.
    pub account_uuid: Option<String>,
    /// Whether the key's scope list mentions `user:profile`, which is what
    /// the identity endpoint wants.
    pub has_profile_scope: bool,
    /// Unix seconds, from `expiresAt` (milliseconds in the file).
    pub expires_at: Option<i64>,
}

/// The account uuid the app last signed in as, straight out of `config.json`
/// with no decryption at all. Weaker than the API answer — it is a uuid, not
/// an email, and it survives a sign-out — but it costs nothing and it is what
/// remains when Keychain access is refused.
pub fn last_known_account_uuid(profile: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(profile.join("config.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&raw).ok()?;
    json.get("lastKnownAccountUuid")
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
        .map(String::from)
}

/// The Keychain password behind `Claude Safe Storage`. **Prompts** the first
/// time — see the module comment.
pub fn safe_storage_password() -> Option<String> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let out = Command::new("security")
        .args(["find-generic-password", "-s", KEYCHAIN_SERVICE])
        .args(["-a", KEYCHAIN_ACCOUNT, "-w"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if raw.is_empty() { None } else { Some(raw) }
}

pub fn derive_key(password: &str) -> [u8; 16] {
    let mut key = [0u8; 16];
    pbkdf2::pbkdf2::<Hmac<Sha1>>(password.as_bytes(), SALT, ITERATIONS, &mut key)
        .expect("pbkdf2 into a fixed 16-byte buffer cannot fail");
    key
}

/// Decrypt a `v10`-prefixed blob. `None` for anything that isn't one, or that
/// doesn't decrypt to valid UTF-8 — a wrong key produces bytes, not an error,
/// so the UTF-8 check is what actually catches it.
pub fn decrypt(blob: &[u8], key: &[u8; 16]) -> Option<String> {
    let body = blob.strip_prefix(PREFIX)?;
    if body.is_empty() || body.len() % 16 != 0 {
        return None;
    }
    let mut buf = body.to_vec();
    let plain = Aes128CbcDec::new(key.into(), &IV.into())
        .decrypt_padded::<Pkcs7>(&mut buf)
        .ok()?;
    String::from_utf8(plain.to_vec()).ok()
}

/// The `oauth:tokenCacheV2` blob, base64-decoded. Falls back to the pre-V2
/// key, which survives the migration as an empty placeholder and so is only
/// used when it actually holds something.
pub fn token_blob(profile: &Path) -> Option<Vec<u8>> {
    let raw = std::fs::read_to_string(profile.join("config.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&raw).ok()?;
    ["oauth:tokenCacheV2", "oauth:tokenCache"]
        .iter()
        .find_map(|key| {
            json.get(*key)
                .and_then(|v| v.as_str())
                .filter(|v| !v.is_empty())
        })
        .and_then(base64_decode)
}

/// Every usable entry of a decrypted token cache, newest-usable-first is left
/// to `pick`. Entries without a token string are dropped.
pub fn parse_cache(plain: &str) -> Vec<TokenEntry> {
    let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(plain)
    else {
        return Vec::new();
    };
    map.iter()
        .filter_map(|(key, value)| {
            let token = value.get("token")?.as_str()?;
            if token.is_empty() {
                return None;
            }
            Some(TokenEntry {
                token: token.to_string(),
                account_uuid: key
                    .split(':')
                    .next()
                    .filter(|s| !s.is_empty())
                    .map(String::from),
                has_profile_scope: key.contains("user:profile"),
                // Stored in milliseconds; seconds is what the rest of the
                // codebase speaks.
                expires_at: value
                    .get("expiresAt")
                    .and_then(|v| v.as_i64())
                    .map(|ms| ms / 1000),
            })
        })
        .collect()
}

/// The entry to authenticate with: unexpired first, and among those the one
/// whose scopes include `user:profile`, since that is the endpoint we ask
/// first. An entry with no `expiresAt` is treated as usable rather than
/// discarded — the field is theirs to remove.
pub fn pick(entries: &[TokenEntry], now: i64) -> Option<&TokenEntry> {
    let fresh = |e: &&TokenEntry| e.expires_at.is_none_or(|exp| exp > now);
    entries
        .iter()
        .filter(fresh)
        .find(|e| e.has_profile_scope)
        .or_else(|| entries.iter().find(fresh))
}

pub enum TokenResult {
    Ok(String),
    /// No credential in the profile at all.
    NotSignedIn,
    /// The Keychain entry could not be read — most often because the user
    /// declined the prompt.
    NoKeychain,
    /// Decryption or parsing failed: the app changed the format, or the
    /// Keychain password is not the one this blob was encrypted with.
    Unreadable,
    /// Credentials are there but past their expiry, and refreshing them is
    /// the app's job, not ours.
    Expired,
}

/// The profile's live access token. Every step can fail softly, and each
/// failure is a different thing to tell the user, hence the enum.
pub fn profile_token(profile: &Path) -> TokenResult {
    let Some(blob) = token_blob(profile) else {
        return TokenResult::NotSignedIn;
    };
    let plain = if cfg!(windows) {
        let Some(key) = windows_key(profile) else {
            return TokenResult::NoKeychain;
        };
        decrypt_gcm(&blob, &key)
    } else {
        let Some(password) = safe_storage_password() else {
            return TokenResult::NoKeychain;
        };
        decrypt(&blob, &derive_key(&password))
    };
    let Some(plain) = plain else {
        return TokenResult::Unreadable;
    };
    let entries = parse_cache(&plain);
    if entries.is_empty() {
        return TokenResult::Unreadable;
    }
    match pick(&entries, now_secs()) {
        Some(entry) => TokenResult::Ok(entry.token.clone()),
        None => TokenResult::Expired,
    }
}

// Windows: Chromium keeps a random 32-byte key in the profile's `Local
// State`, under `os_crypt.encrypted_key`, base64 of `DPAPI` followed by the
// key wrapped with DPAPI for the current user. A `v10` blob is AES-256-GCM
// under that key: a 12-byte nonce, the ciphertext, a 16-byte tag. This is
// Chromium's documented scheme; that Claude Desktop's Electron uses it for
// the token cache is expected, not yet checked against a Windows install.

/// The DPAPI-wrapped key out of a profile's `Local State`, without its
/// `DPAPI` marker.
fn wrapped_windows_key(profile: &Path) -> Option<Vec<u8>> {
    let raw = std::fs::read_to_string(profile.join("Local State")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let encoded = json.get("os_crypt")?.get("encrypted_key")?.as_str()?;
    base64_decode(encoded)?
        .strip_prefix(b"DPAPI")
        .map(<[u8]>::to_vec)
}

/// The profile's AES key, unwrapped by DPAPI through PowerShell. No prompt:
/// DPAPI answers for the signed-in user. The wrapped key reaches PowerShell
/// as an environment variable, never in the command text.
fn windows_key(profile: &Path) -> Option<[u8; 32]> {
    let wrapped = wrapped_windows_key(profile)?;
    let out = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Add-Type -AssemblyName System.Security; \
             [Convert]::ToBase64String([Security.Cryptography.ProtectedData]::Unprotect(\
             [Convert]::FromBase64String($env:CLAUDE_ACC_DPAPI), $null, 'CurrentUser'))",
        ])
        .env("CLAUDE_ACC_DPAPI", base64_encode(&wrapped))
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    base64_decode(String::from_utf8_lossy(&out.stdout).trim())?
        .try_into()
        .ok()
}

/// Decrypt a Windows `v10` blob: AES-256-GCM, 12-byte nonce, 16-byte tag, no
/// associated data. `None` if the tag does not match, which is what a wrong
/// key gives, or if the text is not UTF-8. Written on the `aes` block cipher
/// this crate already has, rather than adding a GCM crate for one call.
pub fn decrypt_gcm(blob: &[u8], key: &[u8; 32]) -> Option<String> {
    use aes::cipher::{BlockCipherEncrypt, KeyInit};
    let body = blob.strip_prefix(PREFIX)?;
    if body.len() < 12 + 16 {
        return None;
    }
    let (nonce, rest) = body.split_at(12);
    let (ciphertext, tag) = rest.split_at(rest.len() - 16);

    let cipher = aes::Aes256::new(key.into());
    let encrypt = |input: [u8; 16]| -> [u8; 16] {
        let mut block = input.into();
        cipher.encrypt_block(&mut block);
        block.into()
    };
    let counter = |n: u32| -> [u8; 16] {
        let mut block = [0u8; 16];
        block[..12].copy_from_slice(nonce);
        block[12..].copy_from_slice(&n.to_be_bytes());
        block
    };

    // GHASH over the ciphertext and the length block, then the tag check.
    let h = u128::from_be_bytes(encrypt([0u8; 16]));
    let mut y = 0u128;
    for chunk in ciphertext.chunks(16) {
        let mut block = [0u8; 16];
        block[..chunk.len()].copy_from_slice(chunk);
        y = gf_mul(y ^ u128::from_be_bytes(block), h);
    }
    y = gf_mul(y ^ (ciphertext.len() as u128 * 8), h);
    let expected = (u128::from_be_bytes(encrypt(counter(1))) ^ y).to_be_bytes();
    if expected != tag {
        return None;
    }

    let mut plain = Vec::with_capacity(ciphertext.len());
    for (i, chunk) in ciphertext.chunks(16).enumerate() {
        let stream = encrypt(counter(2 + i as u32));
        plain.extend(chunk.iter().zip(stream).map(|(c, k)| c ^ k));
    }
    String::from_utf8(plain).ok()
}

/// Multiplication in GCM's GF(2^128), bit by bit as the spec writes it.
fn gf_mul(x: u128, y: u128) -> u128 {
    let mut z = 0u128;
    let mut v = y;
    for i in 0..128 {
        if (x >> (127 - i)) & 1 == 1 {
            z ^= v;
        }
        let carry = v & 1;
        v >>= 1;
        if carry == 1 {
            v ^= 0xE1 << 120;
        }
    }
    z
}

/// Standard base64 with padding, encode only; `base64_decode`'s other half.
fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n =
            chunk.iter().fold(0u32, |acc, &b| acc << 8 | u32::from(b)) << (8 * (3 - chunk.len()));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Standard base64 with padding, decode only. Hand-rolled rather than adding
/// a dependency for thirty lines; whitespace is ignored, anything else
/// invalid is `None`.
fn base64_decode(input: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() / 4 * 3);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    for c in input.bytes() {
        let value = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            b'\n' | b'\r' | b' ' | b'\t' => continue,
            _ => return None,
        };
        acc = (acc << 6) | value as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    // Expected values come from Node's own AES-256-GCM, not from this code.
    const GCM_KEY: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
    const GCM_CASES: [(&str, &str); 3] = [
        (
            "",
            "a0a1a2a3a4a5a6a7a8a9aaab5c699625af4b93a0f8220a2a6119c5d0",
        ),
        (
            r#"{"x":1}"#,
            "a0a1a2a3a4a5a6a7a8a9aaab9d3a040f7ffa7f62ef0584f3230570ee3fcb6ae34ce54f",
        ),
        (
            r#"{"acc:org:aud:user:profile user:inference":{"token":"sk-ant-oat01-abc","expiresAt":1900000000000}}"#,
            "a0a1a2a3a4a5a6a7a8a9aaab9d3a1d4e26f16dcd055fe6a66340b5ad15de6360e0d82405f06b06f30cce073bbb18219add473d5e3abe3eb32b0eec922275647240a3715320307f73cb11f1808591e40d5387c8c2919a8f71fdabbffbb9299195fcf1a94ade450eaa81aa7da4f2f9dfc8ff23651d8fd562399e394cc80a99",
        ),
    ];

    fn v10(body_hex: &str) -> Vec<u8> {
        [b"v10".as_slice(), &unhex(body_hex)].concat()
    }

    #[test]
    fn a_windows_blob_decrypts_to_what_node_encrypted() {
        let key: [u8; 32] = unhex(GCM_KEY).try_into().unwrap();
        for (plain, body) in GCM_CASES {
            assert_eq!(decrypt_gcm(&v10(body), &key).as_deref(), Some(plain));
        }
        let parsed = parse_cache(&decrypt_gcm(&v10(GCM_CASES[2].1), &key).unwrap());
        assert_eq!(parsed[0].token, "sk-ant-oat01-abc");
    }

    #[test]
    fn a_wrong_key_or_a_changed_byte_is_refused_by_the_tag() {
        let key: [u8; 32] = unhex(GCM_KEY).try_into().unwrap();
        let mut wrong = key;
        wrong[0] ^= 1;
        let blob = v10(GCM_CASES[1].1);
        assert_eq!(decrypt_gcm(&blob, &wrong), None);
        for at in [3, 3 + 12, blob.len() - 1] {
            let mut changed = blob.clone();
            changed[at] ^= 1;
            assert_eq!(decrypt_gcm(&changed, &key), None, "byte {at}");
        }
        assert_eq!(decrypt_gcm(b"v10short", &key), None);
        assert_eq!(decrypt_gcm(&blob[3..], &key), None);
    }

    #[test]
    fn base64_round_trips_every_padding_length() {
        for n in 0..8u8 {
            let bytes: Vec<u8> = (0..n).map(|i| i.wrapping_mul(97)).collect();
            assert_eq!(
                base64_decode(&base64_encode(&bytes)).as_deref(),
                Some(bytes.as_slice())
            );
        }
        assert_eq!(base64_encode(b"DPAPI"), "RFBBUEk=");
    }

    #[test]
    fn the_wrapped_key_is_read_from_local_state_without_its_marker() {
        let dir = std::env::temp_dir().join(format!("cc-localstate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let encoded = base64_encode(b"DPAPIwrapped-bytes");
        std::fs::write(
            dir.join("Local State"),
            format!(r#"{{"os_crypt":{{"encrypted_key":"{encoded}"}}}}"#),
        )
        .unwrap();
        assert_eq!(
            wrapped_windows_key(&dir).as_deref(),
            Some(b"wrapped-bytes".as_slice())
        );
        std::fs::write(
            dir.join("Local State"),
            r#"{"os_crypt":{"encrypted_key":"bm9wZQ=="}}"#,
        )
        .unwrap();
        assert_eq!(wrapped_windows_key(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Real DPAPI: wrap a key for this user, write it as Chromium would, read
    // it back through `windows_key`.
    #[cfg(windows)]
    #[test]
    fn a_dpapi_wrapped_key_is_unwrapped_for_the_signed_in_user() {
        let key: Vec<u8> = (0..32u8).collect();
        let out = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "Add-Type -AssemblyName System.Security; \
                 [Convert]::ToBase64String([Security.Cryptography.ProtectedData]::Protect(\
                 [Convert]::FromBase64String($env:K), $null, 'CurrentUser'))",
            ])
            .env("K", base64_encode(&key))
            .output()
            .unwrap();
        assert!(out.status.success());
        let wrapped = base64_decode(String::from_utf8_lossy(&out.stdout).trim()).unwrap();
        let dir = std::env::temp_dir().join(format!("cc-dpapi-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let encoded = base64_encode(&[b"DPAPI".as_slice(), &wrapped].concat());
        std::fs::write(
            dir.join("Local State"),
            format!(r#"{{"os_crypt":{{"encrypted_key":"{encoded}"}}}}"#),
        )
        .unwrap();
        assert_eq!(windows_key(&dir).map(|k| k.to_vec()), Some(key));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn hex(key: [u8; 16]) -> String {
        key.iter().map(|b| format!("{:02x}", b)).collect()
    }

    // Pinned against an independent implementation, so a wrong digest, salt
    // or iteration count fails here rather than as an unexplained 401:
    //   python3 -c "import hashlib,binascii; print(binascii.hexlify(
    //       hashlib.pbkdf2_hmac('sha1', b'peanuts', b'saltysalt', 1003, 16)))"
    #[test]
    fn key_derivation_matches_an_independent_pbkdf2() {
        assert_eq!(
            derive_key("peanuts"),
            derive_key("peanuts"),
            "deterministic"
        );
        assert_eq!(
            hex(derive_key("peanuts")),
            "d9a09d499b4e1b7461f28e67972c6dbd"
        );
        assert_eq!(hex(derive_key("")), "1cbee826d6938327ae9043f63bfc26d7");
    }

    #[test]
    fn decrypt_round_trips_what_the_same_key_encrypted() {
        use aes::cipher::{BlockModeEncrypt, KeyIvInit};
        let key = derive_key("peanuts");
        let plain = br#"{"a:b:c:user:profile":{"token":"t"}}"#;
        let mut buf = vec![0u8; plain.len() + 16];
        buf[..plain.len()].copy_from_slice(plain);
        let ct = cbc::Encryptor::<aes::Aes128>::new(&key.into(), &IV.into())
            .encrypt_padded::<Pkcs7>(&mut buf, plain.len())
            .unwrap()
            .to_vec();

        let mut blob = PREFIX.to_vec();
        blob.extend_from_slice(&ct);
        assert_eq!(
            decrypt(&blob, &key).as_deref(),
            Some(r#"{"a:b:c:user:profile":{"token":"t"}}"#)
        );
    }

    #[test]
    fn decrypt_rejects_what_it_cannot_read() {
        let key = derive_key("peanuts");
        assert_eq!(decrypt(b"", &key), None, "empty");
        assert_eq!(decrypt(b"v11abcdefghijklmnop", &key), None, "wrong version");
        assert_eq!(decrypt(b"v10short", &key), None, "not a whole block");
        // A wrong key decrypts to bytes, not an error — the UTF-8 check is
        // the only thing standing between that and a garbage "token".
        let mut blob = PREFIX.to_vec();
        blob.extend_from_slice(&[0u8; 32]);
        assert_eq!(decrypt(&blob, &key), None, "garbage ciphertext");
    }

    const CACHE: &str = r#"{
        "acc-1:org:https://api.anthropic.com:user:inference user:sessions": {
            "token": "inference-token", "expiresAt": 4000000
        },
        "acc-1:org:https://api.anthropic.com:user:profile": {
            "token": "profile-token", "expiresAt": 4000000
        },
        "acc-1:org:https://api.anthropic.com:user:nothing": {
            "token": "", "expiresAt": 4000000
        }
    }"#;

    #[test]
    fn parsing_reads_the_uuid_out_of_the_key_and_drops_empty_tokens() {
        let entries = parse_cache(CACHE);
        assert_eq!(entries.len(), 2, "the empty-token entry is dropped");
        assert!(
            entries
                .iter()
                .all(|e| e.account_uuid.as_deref() == Some("acc-1"))
        );
        // Milliseconds in the file, seconds everywhere else.
        assert!(entries.iter().all(|e| e.expires_at == Some(4000)));
    }

    #[test]
    fn parsing_survives_anything_that_is_not_the_expected_shape() {
        assert!(parse_cache("not json").is_empty());
        assert!(parse_cache("[]").is_empty());
        assert!(parse_cache(r#"{"k":{"no-token":1}}"#).is_empty());
    }

    #[test]
    fn the_profile_scoped_token_is_preferred() {
        let entries = parse_cache(CACHE);
        assert_eq!(pick(&entries, 0).unwrap().token, "profile-token");
    }

    #[test]
    fn expired_entries_are_skipped() {
        let entries = parse_cache(CACHE);
        // Past every expiry: nothing is usable, rather than a stale token
        // that would only produce a confusing 401.
        assert!(pick(&entries, 5000).is_none());
    }

    #[test]
    fn a_non_profile_token_is_used_when_it_is_all_there_is() {
        let entries = parse_cache(
            r#"{"acc:org:aud:user:inference": {"token": "only-one", "expiresAt": 4000000}}"#,
        );
        assert_eq!(pick(&entries, 0).unwrap().token, "only-one");
    }

    #[test]
    fn an_entry_without_an_expiry_is_still_usable() {
        let entries = parse_cache(r#"{"acc:org:aud:user:profile": {"token": "t"}}"#);
        assert_eq!(entries[0].expires_at, None);
        assert_eq!(pick(&entries, i64::MAX).unwrap().token, "t");
    }

    #[test]
    fn picking_from_nothing_is_nothing() {
        assert!(pick(&[], 0).is_none());
    }

    #[test]
    fn base64_decodes_padded_and_unpadded_input() {
        assert_eq!(base64_decode("djEw").unwrap(), b"v10");
        assert_eq!(base64_decode("YQ==").unwrap(), b"a");
        assert_eq!(base64_decode("YWI=").unwrap(), b"ab");
        assert_eq!(base64_decode("YWJj").unwrap(), b"abc");
        assert_eq!(base64_decode("").unwrap(), b"");
        assert_eq!(base64_decode("dj\nEw").unwrap(), b"v10", "newlines ignored");
    }

    #[test]
    fn base64_rejects_characters_it_does_not_know() {
        assert_eq!(base64_decode("dj!w"), None);
    }
}
