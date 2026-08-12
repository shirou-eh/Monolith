//! Release artifact signature verification.
//!
//! `mnctl update --self` and `mnpkg update --self` both download a
//! binary tarball straight from a GitHub release and run it with root
//! consequences on the caller's machine. Before this crate existed,
//! neither verified anything beyond "did the HTTPS request succeed" —
//! TLS protects the wire, but not against a compromised release
//! artifact (bad CI run, compromised maintainer account, tampered
//! upload). This crate closes that gap the same way `pacman` closes it
//! for regular packages: a detached GPG signature, checked against a
//! public key that ships baked into the binary rather than fetched
//! over the network (fetching the verification key from the same place
//! as the artifact it verifies would make the whole check circular).
//!
//! [`RELEASE_PUBKEY`] is the actual Monolith release signing public
//! key (Ed25519). The matching private key never leaves the
//! maintainer's control — it's used in CI to sign release tarballs
//! (see `.github/workflows/release.yml`), never checked in.
//!
//! Verification shells out to the system `gpg` rather than pulling in
//! a Rust OpenPGP implementation, consistent with the rest of this
//! project (kernel/build.sh already does the same for kernel tarball
//! signatures) and because gpg's own signature-parsing has had far
//! more scrutiny than a hand-rolled or freshly-vendored crate would.

use anyhow::{bail, Context, Result};
use std::process::Command;

/// The Monolith OS release signing public key, embedded at compile
/// time. Generated once; the private half is held by the maintainer
/// and used only in release CI.
pub const RELEASE_PUBKEY: &str = include_str!("../monolith-release-signing.asc");

/// Outcome of a signature check that intentionally isn't a hard
/// failure — an asset with no `.sig` published at all (every release
/// before this feature existed) is a different situation from an
/// asset whose `.sig` exists but doesn't verify. Callers should warn
/// loudly on `Unsigned` and hard-abort on anything from
/// [`verify_detached`] returning `Err`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureStatus {
    /// Verified against [`RELEASE_PUBKEY`].
    Verified,
    /// No `.sig` asset was published alongside this artifact.
    Unsigned,
}

/// Verify `data`'s detached signature `sig` against [`RELEASE_PUBKEY`].
///
/// Imports the embedded public key into a throwaway `GNUPGHOME` (never
/// the caller's real keyring — this has no business touching a user's
/// existing GPG trust store) and runs `gpg --verify`. Returns `Err` on
/// anything other than a clean, matching signature: a bad/missing gpg
/// binary, a signature from the wrong key, or a signature over
/// different bytes than `data` (tamper detection) all fail closed.
pub fn verify_detached(data: &[u8], sig: &[u8]) -> Result<()> {
    let gnupghome = tempdir()?;
    let gnupghome_path = gnupghome.path();

    // gpg refuses to run against a GNUPGHOME with overly permissive
    // perms.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(gnupghome_path, std::fs::Permissions::from_mode(0o700))
            .context("failed to set GNUPGHOME permissions")?;
    }

    let import = Command::new("gpg")
        .env("GNUPGHOME", gnupghome_path)
        .args(["--batch", "--quiet", "--import"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("failed to spawn gpg --import — is gnupg installed?")?;
    {
        use std::io::Write;
        import
            .stdin
            .as_ref()
            .context("gpg --import stdin unavailable")?
            .write_all(RELEASE_PUBKEY.as_bytes())
            .context("failed to write public key to gpg --import")?;
    }
    let import_out = import
        .wait_with_output()
        .context("failed to wait on gpg --import")?;
    if !import_out.status.success() {
        bail!(
            "failed to import Monolith release signing key into a temporary keyring: {}",
            String::from_utf8_lossy(&import_out.stderr)
        );
    }

    let data_path = gnupghome_path.join("artifact");
    let sig_path = gnupghome_path.join("artifact.sig");
    std::fs::write(&data_path, data).context("failed to stage artifact for verification")?;
    std::fs::write(&sig_path, sig).context("failed to stage signature for verification")?;

    // --trust-model always: this isn't web-of-trust, it's pinning one
    // specific key baked into the binary. There's exactly one key that
    // can ever verify here, so "is this key trusted" is a question
    // that was already answered at compile time.
    let verify = Command::new("gpg")
        .env("GNUPGHOME", gnupghome_path)
        .args([
            "--batch",
            "--trust-model",
            "always",
            "--verify",
            sig_path.to_str().context("non-utf8 temp path")?,
            data_path.to_str().context("non-utf8 temp path")?,
        ])
        .output()
        .context("failed to run gpg --verify")?;

    if !verify.status.success() {
        bail!(
            "signature verification FAILED — this artifact does not match the Monolith release signing key:\n{}",
            String::from_utf8_lossy(&verify.stderr)
        );
    }

    Ok(())
}

/// Minimal throwaway-directory helper — this crate has exactly one
/// caller of this shape, so pulling in the `tempfile` crate for it
/// isn't worth the extra dependency.
struct TempDir(std::path::PathBuf);

impl TempDir {
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn tempdir() -> Result<TempDir> {
    let base = std::env::temp_dir();
    let unique = format!(
        "monolith-sign-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    );
    let dir = base.join(unique);
    std::fs::create_dir_all(&dir).context("failed to create temporary GNUPGHOME")?;
    Ok(TempDir(dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Round-trips against the REAL embedded RELEASE_PUBKEY are not
    // possible in a unit test — that would require the private key,
    // which by design never touches this repo. What's testable here
    // without it: a signature from a *different* key must not verify
    // against RELEASE_PUBKEY, and garbage input must fail closed
    // rather than panicking.
    #[test]
    fn garbage_signature_fails_closed() {
        let err = verify_detached(b"some artifact bytes", b"not a real signature");
        assert!(err.is_err(), "garbage signature must not verify");
    }

    #[test]
    fn empty_signature_fails_closed() {
        let err = verify_detached(b"some artifact bytes", b"");
        assert!(err.is_err(), "empty signature must not verify");
    }

    // Real round-trip against RELEASE_PUBKEY, using the actual private
    // key. `#[ignore]`d because the private key is deliberately not
    // checked into this repo — CI can't run this. Run manually with:
    //   MONOLITH_SIGN_TEST_KEY=/path/to/private.asc cargo test -p monolith-sign -- --ignored
    #[test]
    #[ignore]
    fn real_key_round_trip() {
        let key_path = std::env::var("MONOLITH_SIGN_TEST_KEY")
            .expect("set MONOLITH_SIGN_TEST_KEY to the private key path");
        let data = b"monolith-sign real-key-round-trip test artifact";

        let gnupghome = tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(gnupghome.path(), std::fs::Permissions::from_mode(0o700))
                .unwrap();
        }
        let status = Command::new("gpg")
            .env("GNUPGHOME", gnupghome.path())
            .args(["--batch", "--quiet", "--import", &key_path])
            .status()
            .unwrap();
        assert!(status.success(), "failed to import test private key");

        let artifact_path = gnupghome.path().join("artifact");
        std::fs::write(&artifact_path, data).unwrap();
        let status = Command::new("gpg")
            .env("GNUPGHOME", gnupghome.path())
            .args([
                "--batch",
                "--yes",
                "--detach-sign",
                artifact_path.to_str().unwrap(),
            ])
            .status()
            .unwrap();
        assert!(status.success(), "failed to sign test artifact");
        let sig = std::fs::read(artifact_path.with_extension("sig")).unwrap();

        // The actual function under test, against the actual embedded
        // RELEASE_PUBKEY — not a throwaway key generated by the test.
        verify_detached(data, &sig).expect("real key signature must verify against RELEASE_PUBKEY");

        // And tamper detection, one more time, through the real path.
        let tampered = b"monolith-sign real-key-round-trip test artifact TAMPERED";
        let err = verify_detached(tampered, &sig);
        assert!(err.is_err(), "tampered data must not verify");
    }
}
