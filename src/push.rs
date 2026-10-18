//! Web push: sends notifications to paired phones through their browser's push service
//! (Apple's or Google's). Messages are encrypted for the phone (RFC 8291, aes128gcm), so the
//! push service only sees that something was sent; the request is signed with this PC's own
//! VAPID key (RFC 8292). No account or server is needed beyond what the phone already uses.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes128Gcm, Nonce};
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine;
use hkdf::Hkdf;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::{PublicKey, SecretKey};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

/// Where a phone receives pushes (from the browser's PushSubscription).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Subscription {
    pub endpoint: String,
    /// The phone's public key (base64url, 65 bytes uncompressed P-256).
    pub p256dh: String,
    /// The phone's auth secret (base64url, 16 bytes).
    pub auth: String,
}

/// A new VAPID private key (base64url), made once and kept in settings.
pub fn new_key() -> String {
    B64.encode(SecretKey::random(&mut OsRng).to_bytes())
}

fn secret(key: &str) -> Result<SecretKey, String> {
    let bytes = B64.decode(key.trim()).map_err(|e| e.to_string())?;
    SecretKey::from_slice(&bytes).map_err(|e| e.to_string())
}

/// The public half of the VAPID key, as the browser wants it (base64url, uncompressed point).
pub fn public_key(key: &str) -> Result<String, String> {
    Ok(B64.encode(secret(key)?.public_key().to_encoded_point(false).as_bytes()))
}

/// RFC 8291 encryption. `as_secret` and `salt` are random in real use (fixed in tests).
fn encrypt_with(payload: &[u8], ua_public: &[u8], auth: &[u8], as_secret: &SecretKey, salt: &[u8; 16]) -> Result<Vec<u8>, String> {
    let ua = PublicKey::from_sec1_bytes(ua_public).map_err(|_| "the phone's key isn't valid".to_string())?;
    let as_public = as_secret.public_key().to_encoded_point(false);
    let shared = p256::ecdh::diffie_hellman(as_secret.to_nonzero_scalar(), ua.as_affine());

    // IKM from the shared secret, bound to both public keys and the auth secret
    let mut key_info = b"WebPush: info\0".to_vec();
    key_info.extend_from_slice(ua_public);
    key_info.extend_from_slice(as_public.as_bytes());
    let mut ikm = [0u8; 32];
    Hkdf::<Sha256>::new(Some(auth), shared.raw_secret_bytes()).expand(&key_info, &mut ikm).map_err(|e| e.to_string())?;

    let hk = Hkdf::<Sha256>::new(Some(salt), &ikm);
    let mut cek = [0u8; 16];
    let mut nonce = [0u8; 12];
    hk.expand(b"Content-Encoding: aes128gcm\0", &mut cek).map_err(|e| e.to_string())?;
    hk.expand(b"Content-Encoding: nonce\0", &mut nonce).map_err(|e| e.to_string())?;

    // one record: payload, then the 0x02 "last record" delimiter
    let mut plain = payload.to_vec();
    plain.push(2);
    let cipher = Aes128Gcm::new_from_slice(&cek).map_err(|e| e.to_string())?;
    let sealed = cipher.encrypt(Nonce::from_slice(&nonce), plain.as_ref()).map_err(|e| e.to_string())?;

    // header: salt, record size, key id (our public key), then the record
    let mut out = Vec::with_capacity(86 + sealed.len());
    out.extend_from_slice(salt);
    out.extend_from_slice(&4096u32.to_be_bytes());
    out.push(65);
    out.extend_from_slice(as_public.as_bytes());
    out.extend_from_slice(&sealed);
    Ok(out)
}

fn encrypt(payload: &[u8], sub: &Subscription) -> Result<Vec<u8>, String> {
    let ua_public = B64.decode(sub.p256dh.trim_end_matches('=')).map_err(|e| e.to_string())?;
    let auth = B64.decode(sub.auth.trim_end_matches('=')).map_err(|e| e.to_string())?;
    let mut salt = [0u8; 16];
    OsRng.fill_bytes(&mut salt);
    encrypt_with(payload, &ua_public, &auth, &SecretKey::random(&mut OsRng), &salt)
}

/// The VAPID authorization header for an endpoint's push service.
fn vapid_header(key: &str, endpoint: &str) -> Result<String, String> {
    let sk = secret(key)?;
    let origin = endpoint.splitn(4, '/').take(3).collect::<Vec<_>>().join("/");
    let exp = chrono::Utc::now().timestamp() + 12 * 3600;
    let header = B64.encode(br#"{"typ":"JWT","alg":"ES256"}"#);
    let claims = B64.encode(serde_json::json!({"aud": origin, "exp": exp, "sub": "https://github.com/SyberKraken/morrowkeep"}).to_string());
    let signing_input = format!("{header}.{claims}");
    let sig: Signature = SigningKey::from(&sk).sign(signing_input.as_bytes());
    let jwt = format!("{signing_input}.{}", B64.encode(sig.to_bytes()));
    Ok(format!("vapid t={jwt}, k={}", public_key(key)?))
}

/// Why a send failed: the subscription is gone (unsubscribed / uninstalled), or something else.
#[derive(Debug, PartialEq)]
pub enum SendError {
    Gone,
    Other(String),
}

/// Sends one encrypted notification. `payload` is the JSON the phone's service worker reads.
pub fn send(key: &str, sub: &Subscription, payload: &serde_json::Value) -> Result<(), SendError> {
    let body = encrypt(payload.to_string().as_bytes(), sub).map_err(SendError::Other)?;
    let auth = vapid_header(key, &sub.endpoint).map_err(SendError::Other)?;
    let resp = ureq::post(&sub.endpoint)
        .set("Authorization", &auth)
        .set("Content-Encoding", "aes128gcm")
        .set("Content-Type", "application/octet-stream")
        .set("TTL", "3600")
        .set("Urgency", "normal")
        .timeout(std::time::Duration::from_secs(15))
        .send_bytes(&body);
    match resp {
        Ok(_) => Ok(()),
        Err(ureq::Error::Status(404 | 410, _)) => Err(SendError::Gone),
        Err(ureq::Error::Status(code, r)) => Err(SendError::Other(format!("push service answered {code}: {}", r.into_string().unwrap_or_default().chars().take(200).collect::<String>()))),
        Err(e) => Err(SendError::Other(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The worked example from RFC 8291, appendix A.
    #[test]
    fn rfc8291_example() {
        let d = |s: &str| B64.decode(s).unwrap();
        let as_secret = SecretKey::from_slice(&d("yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw")).unwrap();
        let ua_public = d("BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4");
        let auth = d("BTBZMqHH6r4Tts7J_aSIgg");
        let salt: [u8; 16] = d("DGv6ra1nlYgDCS1FRnbzlw").try_into().unwrap();
        let out = encrypt_with(b"When I grow up, I want to be a watermelon", &ua_public, &auth, &as_secret, &salt).unwrap();
        // the header (salt, record size, our public key) and the encrypted record from the RFC
        assert_eq!(&out[..16], &salt);
        assert_eq!(&out[21..86], d("BP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8").as_slice());
        assert_eq!(
            B64.encode(out),
            "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPTpK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN"
        );
    }

    #[test]
    fn vapid_jwt_verifies() {
        use p256::ecdsa::signature::Verifier;
        let key = new_key();
        let h = vapid_header(&key, "https://web.push.apple.com/QGuQyavXutnMOqVHz9gXnEgk").unwrap();
        let jwt = h.strip_prefix("vapid t=").unwrap().split(", k=").next().unwrap();
        let (input, sig) = jwt.rsplit_once('.').unwrap();
        let claims: serde_json::Value = serde_json::from_slice(&B64.decode(input.split('.').nth(1).unwrap()).unwrap()).unwrap();
        assert_eq!(claims["aud"], "https://web.push.apple.com");
        let vk = p256::ecdsa::VerifyingKey::from(&SigningKey::from(&secret(&key).unwrap()));
        vk.verify(input.as_bytes(), &Signature::from_slice(&B64.decode(sig).unwrap()).unwrap()).unwrap();
        assert_eq!(B64.decode(public_key(&key).unwrap()).unwrap().len(), 65);
    }

    /// The phone's side of RFC 8291, to check what `send` really puts on the wire.
    fn decrypt(body: &[u8], ua_secret: &SecretKey, auth: &[u8]) -> Vec<u8> {
        let (salt, rest) = body.split_at(16);
        let idlen = rest[4] as usize;
        let as_public = &rest[5..5 + idlen];
        let sealed = &rest[5 + idlen..];
        let ua_public = ua_secret.public_key().to_encoded_point(false);
        let shared = p256::ecdh::diffie_hellman(ua_secret.to_nonzero_scalar(), PublicKey::from_sec1_bytes(as_public).unwrap().as_affine());
        let mut key_info = b"WebPush: info ".to_vec();
        key_info.extend_from_slice(ua_public.as_bytes());
        key_info.extend_from_slice(as_public);
        let mut ikm = [0u8; 32];
        Hkdf::<Sha256>::new(Some(auth), shared.raw_secret_bytes()).expand(&key_info, &mut ikm).unwrap();
        let hk = Hkdf::<Sha256>::new(Some(salt), &ikm);
        let (mut cek, mut nonce) = ([0u8; 16], [0u8; 12]);
        hk.expand(b"Content-Encoding: aes128gcm ", &mut cek).unwrap();
        hk.expand(b"Content-Encoding: nonce ", &mut nonce).unwrap();
        let mut plain = Aes128Gcm::new_from_slice(&cek).unwrap().decrypt(Nonce::from_slice(&nonce), sealed).unwrap();
        assert_eq!(plain.pop(), Some(2)); // last-record delimiter
        plain
    }

    #[test]
    fn send_reaches_a_push_service_intact() {
        // a fake push service on a free local port
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let ua_secret = SecretKey::random(&mut OsRng);
        let mut auth = [0u8; 16];
        OsRng.fill_bytes(&mut auth);
        let sub = Subscription {
            endpoint: format!("http://127.0.0.1:{port}/push/abc"),
            p256dh: B64.encode(ua_secret.public_key().to_encoded_point(false).as_bytes()),
            auth: B64.encode(auth),
        };
        let key = new_key();
        let payload = serde_json::json!({"title": "Morrowkeep", "body": "No calories logged today.", "go": "food"});
        let (k2, s2, p2) = (key.clone(), sub.clone(), payload.clone());
        let sender = std::thread::spawn(move || send(&k2, &s2, &p2));
        let mut rq = server.recv().unwrap();
        let headers: Vec<(String, String)> = rq.headers().iter().map(|h| (h.field.as_str().as_str().to_ascii_lowercase(), h.value.to_string())).collect();
        let header = |n: &str| headers.iter().find(|(k, _)| *k == n.to_ascii_lowercase()).map(|(_, v)| v.clone()).unwrap_or_default();
        assert_eq!(header("Content-Encoding"), "aes128gcm");
        assert_eq!(header("TTL"), "3600");
        let authz = header("Authorization");
        assert!(authz.starts_with("vapid t=") && authz.ends_with(&format!("k={}", public_key(&key).unwrap())), "{authz}");
        let mut body = Vec::new();
        rq.as_reader().read_to_end(&mut body).unwrap();
        rq.respond(tiny_http::Response::empty(201)).unwrap();
        assert_eq!(sender.join().unwrap(), Ok(()));
        let plain = decrypt(&body, &ua_secret, &auth);
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&plain).unwrap(), payload);
        // a subscription the service no longer knows is reported as gone
        let (k3, s3) = (key.clone(), sub.clone());
        let sender = std::thread::spawn(move || send(&k3, &s3, &serde_json::json!({})));
        server.recv().unwrap().respond(tiny_http::Response::empty(410)).unwrap();
        assert_eq!(sender.join().unwrap(), Err(SendError::Gone));
    }
}
