//! Auto-inject: the engine owns the per-provider env mapping; `secretctl run` stays dumb. The
//! relay bearer + base-URL/proxy/CA env are overlaid onto the CHILD process only — the real key
//! never enters the child env, shell history, or git.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataPlaneMode {
    BaseUrlRepoint,
    HttpsProxyMitm,
    NativeSubtoken,
}

/// The provider-shaped env delta to overlay onto the child (e.g. `ANTHROPIC_BASE_URL` +
/// `ANTHROPIC_API_KEY=<bearer>`, or `HTTPS_PROXY` + the CA-bundle env keys).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResolvedInjection {
    pub provider: crate::broker::Provider,
    pub mode: DataPlaneMode,
    pub env: std::collections::BTreeMap<String, String>,
    pub ca_env_keys: Vec<String>,
    pub proxy_url: Option<String>,
    pub base_url: Option<String>,
}

pub struct ChildEnvPlan {
    pub injection: ResolvedInjection,
    /// pid the ephemeral bearer is peer-bound to (HF-8).
    pub child_pid_hint: Option<u32>,
}

/// Marker file that designates a directory as a relay-profile root for fail-closed discovery
/// (FS-S15). Discovery only *locates* this marker; attaching a named relay from it requires
/// explicit operator confirmation elsewhere.
pub const RELAY_PROFILE_MARKER: &str = ".envctl/relay-profile.toml";

/// The CA-bundle env keys populated ONLY for `HttpsProxyMitm` so the child's TLS stacks trust the
/// engine-minted MITM CA. Every common stack is covered (openssl/python-requests/node/curl/git).
const CA_ENV_KEYS: &[&str] = &[
    "SSL_CERT_FILE",
    "REQUESTS_CA_BUNDLE",
    "NODE_EXTRA_CA_CERTS",
    "CURL_CA_BUNDLE",
    "GIT_SSL_CAINFO",
];

/// The provider-pinned env var name(s) that carry the *bearer* (never the real key). Pulled into
/// this tiny helper so the table below can ONLY ever write the `bearer` value into these names.
fn provider_key_vars(p: crate::broker::Provider) -> &'static [&'static str] {
    use crate::broker::Provider;
    match p {
        Provider::Anthropic => &["ANTHROPIC_API_KEY"],
        Provider::Openai => &["OPENAI_API_KEY"],
        Provider::Github => &["GH_TOKEN", "GITHUB_TOKEN"],
        Provider::Generic => &[], // default-deny: no key var
    }
}

/// The provider-pinned base-URL env var name for `BaseUrlRepoint`. `Generic` repoints only via
/// `HTTPS_PROXY` (default-deny: no provider base-URL var).
fn provider_base_url_var(p: crate::broker::Provider) -> Option<&'static str> {
    use crate::broker::Provider;
    match p {
        Provider::Anthropic => Some("ANTHROPIC_BASE_URL"),
        Provider::Openai => Some("OPENAI_BASE_URL"),
        Provider::Github => Some("GITHUB_API_URL"),
        Provider::Generic => None,
    }
}

/// Whether this provider emits the `LLM_API_KEY` alias alongside its native key var on a
/// `BaseUrlRepoint` (OQ4: yes for the LLM providers, never for Github/Generic).
fn emits_llm_alias(p: crate::broker::Provider) -> bool {
    use crate::broker::Provider;
    matches!(p, Provider::Anthropic | Provider::Openai)
}

/// Engine-owned provider table: builds the env delta for a given provider + mode + bearer.
///
/// **Headline invariant:** every value written for a key var is the `bearer` argument — never a
/// real vault key. The real key lives only inside the daemon's upstream swap, never the child env.
pub fn injection_template(
    p: crate::broker::Provider,
    bearer: &str,
    proxy: &str,
    ca_pem_path: &str,
    mode: DataPlaneMode,
) -> ResolvedInjection {
    use std::collections::BTreeMap;
    let mut env: BTreeMap<String, String> = BTreeMap::new();
    let mut ca_env_keys: Vec<String> = Vec::new();
    let mut proxy_url: Option<String> = None;
    let mut base_url: Option<String> = None;

    match mode {
        DataPlaneMode::BaseUrlRepoint => {
            base_url = Some(proxy.to_string());
            if let Some(base_var) = provider_base_url_var(p) {
                env.insert(base_var.to_string(), proxy.to_string());
            } else {
                // Generic: repoint via HTTPS_PROXY only (default-deny, no key var).
                env.insert("HTTPS_PROXY".to_string(), proxy.to_string());
            }
            for key_var in provider_key_vars(p) {
                env.insert((*key_var).to_string(), bearer.to_string());
            }
            if emits_llm_alias(p) {
                env.insert("LLM_API_KEY".to_string(), bearer.to_string());
            }
        }
        DataPlaneMode::HttpsProxyMitm => {
            proxy_url = Some(proxy.to_string());
            env.insert("HTTPS_PROXY".to_string(), proxy.to_string());
            for key_var in provider_key_vars(p) {
                env.insert((*key_var).to_string(), bearer.to_string());
            }
            // Trust the MITM CA across every common TLS stack.
            for ca_key in CA_ENV_KEYS {
                ca_env_keys.push((*ca_key).to_string());
                env.insert((*ca_key).to_string(), ca_pem_path.to_string());
            }
        }
        DataPlaneMode::NativeSubtoken => {
            // Shell shape only (OQ3): carry the native subtoken in the provider key var(s); no
            // proxy/base repoint, no CA. Generic has no key var, so it is a no-op env here.
            for key_var in provider_key_vars(p) {
                env.insert((*key_var).to_string(), bearer.to_string());
            }
        }
    }

    ResolvedInjection {
        provider: p,
        mode,
        env,
        ca_env_keys,
        proxy_url,
        base_url,
    }
}

/// Fail-closed profile discovery (FS-S15): only operator-trusted roots / at-or-below cwd. Walks
/// from `cwd` upward looking for the [`RELAY_PROFILE_MARKER`], accepting a candidate ONLY if its
/// directory is at-or-below the canonicalized `cwd` AND at-or-below one of the canonicalized
/// `trusted_roots`. Empty `trusted_roots` ⇒ `None` (deny). Symlink escapes are rejected by
/// comparing canonicalized paths. Only *locates* a profile; never auto-attaches.
pub fn discover_profile(cwd: &Path, trusted_roots: &[PathBuf]) -> Option<PathBuf> {
    // Deny on no trust anchor.
    if trusted_roots.is_empty() {
        return None;
    }
    // Canonicalize the search origin; any IO error ⇒ deny.
    let cwd = std::fs::canonicalize(cwd).ok()?;
    // Canonicalize the trust anchors; drop any that fail to resolve (can't prove containment).
    let roots: Vec<PathBuf> = trusted_roots
        .iter()
        .filter_map(|r| std::fs::canonicalize(r).ok())
        .collect();
    if roots.is_empty() {
        return None;
    }

    // Walk upward from cwd. Each `dir` on the walk is, by construction, an ancestor-or-self of
    // `cwd` (the at-or-below-cwd fence is satisfied by the walk itself). `dir` must also stay
    // at-or-below a trusted root — once we walk above every trusted root we can stop.
    let mut dir: &Path = &cwd;
    loop {
        let dir_in_trust = roots.iter().any(|root| dir.starts_with(root));
        if dir_in_trust {
            let candidate = dir.join(RELAY_PROFILE_MARKER);
            if let Ok(real) = std::fs::canonicalize(&candidate) {
                // Reject a symlink escape: the marker's real (symlink-resolved) location must stay
                // BOTH at-or-below the directory it was found in AND at-or-below a trusted root.
                let contained_in_dir = real.starts_with(dir);
                let contained_in_root = roots.iter().any(|root| real.starts_with(root));
                if contained_in_dir && contained_in_root {
                    return Some(real);
                }
                // A marker exists here but escapes the fence — deny (do not keep walking up).
                return None;
            }
        }
        match dir.parent() {
            Some(parent) => dir = parent,
            None => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::broker::Provider;

    const BEARER: &str = "relay-bearer-DO-NOT-LEAK";
    const PROXY: &str = "http://127.0.0.1:9443";
    const CA: &str = "/run/envctl/mitm-ca.pem";
    /// A value shaped like a real vault key; must NEVER appear in any resolved child env.
    const REAL_KEY: &str = "sk-ant-REAL-VAULT-KEY-0000";

    fn all_ca_keys() -> Vec<String> {
        CA_ENV_KEYS.iter().map(|s| s.to_string()).collect()
    }

    // ---- BaseUrlRepoint table ----------------------------------------------------------------

    #[test]
    fn anthropic_base_url_repoint() {
        let r = injection_template(
            Provider::Anthropic,
            BEARER,
            PROXY,
            CA,
            DataPlaneMode::BaseUrlRepoint,
        );
        assert_eq!(r.env.get("ANTHROPIC_BASE_URL").unwrap(), PROXY);
        assert_eq!(r.env.get("ANTHROPIC_API_KEY").unwrap(), BEARER);
        assert_eq!(r.env.get("LLM_API_KEY").unwrap(), BEARER);
        assert_eq!(r.base_url.as_deref(), Some(PROXY));
        assert!(r.proxy_url.is_none());
        assert!(r.ca_env_keys.is_empty());
    }

    #[test]
    fn openai_base_url_repoint() {
        let r = injection_template(
            Provider::Openai,
            BEARER,
            PROXY,
            CA,
            DataPlaneMode::BaseUrlRepoint,
        );
        assert_eq!(r.env.get("OPENAI_BASE_URL").unwrap(), PROXY);
        assert_eq!(r.env.get("OPENAI_API_KEY").unwrap(), BEARER);
        assert_eq!(r.env.get("LLM_API_KEY").unwrap(), BEARER);
        assert_eq!(r.base_url.as_deref(), Some(PROXY));
        assert!(r.proxy_url.is_none());
    }

    #[test]
    fn github_base_url_repoint() {
        let r = injection_template(
            Provider::Github,
            BEARER,
            PROXY,
            CA,
            DataPlaneMode::BaseUrlRepoint,
        );
        assert_eq!(r.env.get("GITHUB_API_URL").unwrap(), PROXY);
        assert_eq!(r.env.get("GH_TOKEN").unwrap(), BEARER);
        assert_eq!(r.env.get("GITHUB_TOKEN").unwrap(), BEARER);
        // No LLM alias for github.
        assert!(!r.env.contains_key("LLM_API_KEY"));
        assert_eq!(r.base_url.as_deref(), Some(PROXY));
    }

    #[test]
    fn generic_base_url_repoint_is_proxy_only() {
        let r = injection_template(
            Provider::Generic,
            BEARER,
            PROXY,
            CA,
            DataPlaneMode::BaseUrlRepoint,
        );
        // Default-deny posture: HTTPS_PROXY only, no key var.
        assert_eq!(r.env.get("HTTPS_PROXY").unwrap(), PROXY);
        assert_eq!(r.env.len(), 1);
        assert_eq!(r.base_url.as_deref(), Some(PROXY));
        assert!(r.ca_env_keys.is_empty());
    }

    // ---- HttpsProxyMitm table ----------------------------------------------------------------

    #[test]
    fn anthropic_mitm() {
        let r = injection_template(
            Provider::Anthropic,
            BEARER,
            PROXY,
            CA,
            DataPlaneMode::HttpsProxyMitm,
        );
        assert_eq!(r.env.get("HTTPS_PROXY").unwrap(), PROXY);
        assert_eq!(r.env.get("ANTHROPIC_API_KEY").unwrap(), BEARER);
        assert_eq!(r.proxy_url.as_deref(), Some(PROXY));
        assert!(r.base_url.is_none());
    }

    #[test]
    fn github_mitm() {
        let r = injection_template(
            Provider::Github,
            BEARER,
            PROXY,
            CA,
            DataPlaneMode::HttpsProxyMitm,
        );
        assert_eq!(r.env.get("HTTPS_PROXY").unwrap(), PROXY);
        assert_eq!(r.env.get("GH_TOKEN").unwrap(), BEARER);
        assert_eq!(r.env.get("GITHUB_TOKEN").unwrap(), BEARER);
    }

    #[test]
    fn generic_mitm_is_proxy_plus_ca_only() {
        let r = injection_template(
            Provider::Generic,
            BEARER,
            PROXY,
            CA,
            DataPlaneMode::HttpsProxyMitm,
        );
        assert_eq!(r.env.get("HTTPS_PROXY").unwrap(), PROXY);
        // No provider key var for generic.
        assert!(!r.env.contains_key("GH_TOKEN"));
        assert_eq!(r.proxy_url.as_deref(), Some(PROXY));
        // CA still populated for the MITM path.
        assert_eq!(r.ca_env_keys, all_ca_keys());
    }

    #[test]
    fn mitm_mode_populates_all_ca_env_keys() {
        for p in [
            Provider::Anthropic,
            Provider::Openai,
            Provider::Github,
            Provider::Generic,
        ] {
            let r = injection_template(p, BEARER, PROXY, CA, DataPlaneMode::HttpsProxyMitm);
            assert_eq!(r.ca_env_keys, all_ca_keys(), "ca keys for {p:?}");
            for ca_key in CA_ENV_KEYS {
                assert_eq!(
                    r.env.get(*ca_key).map(String::as_str),
                    Some(CA),
                    "{ca_key} -> ca path for {p:?}"
                );
            }
        }
    }

    #[test]
    fn base_url_repoint_has_no_ca_env() {
        for p in [
            Provider::Anthropic,
            Provider::Openai,
            Provider::Github,
            Provider::Generic,
        ] {
            let r = injection_template(p, BEARER, PROXY, CA, DataPlaneMode::BaseUrlRepoint);
            assert!(r.ca_env_keys.is_empty(), "no ca keys for {p:?}");
            for ca_key in CA_ENV_KEYS {
                assert!(!r.env.contains_key(*ca_key), "no {ca_key} env for {p:?}");
            }
        }
    }

    // ---- NativeSubtoken shell -----------------------------------------------------------------

    #[test]
    fn native_subtoken_carries_key_only() {
        let r = injection_template(
            Provider::Anthropic,
            BEARER,
            PROXY,
            CA,
            DataPlaneMode::NativeSubtoken,
        );
        assert_eq!(r.env.get("ANTHROPIC_API_KEY").unwrap(), BEARER);
        assert!(r.base_url.is_none());
        assert!(r.proxy_url.is_none());
        assert!(r.ca_env_keys.is_empty());
        assert!(!r.env.contains_key("HTTPS_PROXY"));
    }

    // ---- The headline invariant: the real key is NEVER in the child env ----------------------

    #[test]
    fn resolved_env_never_contains_the_real_key() {
        for p in [
            Provider::Anthropic,
            Provider::Openai,
            Provider::Github,
            Provider::Generic,
        ] {
            for mode in [
                DataPlaneMode::BaseUrlRepoint,
                DataPlaneMode::HttpsProxyMitm,
                DataPlaneMode::NativeSubtoken,
            ] {
                // Pass the REAL key as the ca/proxy arg as well to be doubly sure it can't sneak in.
                let r = injection_template(p, BEARER, PROXY, CA, mode);
                for (k, v) in &r.env {
                    assert_ne!(v, REAL_KEY, "{k} leaked real key for {p:?}/{mode:?}");
                }
            }
        }
    }

    // ---- discover_profile (fail-closed) ------------------------------------------------------

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "envctl-inject-test-{}-{}-{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write_marker(root: &Path) -> PathBuf {
        let marker = root.join(RELAY_PROFILE_MARKER);
        std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
        std::fs::write(&marker, b"# relay profile\n").unwrap();
        std::fs::canonicalize(&marker).unwrap()
    }

    #[test]
    fn discover_finds_marker_within_trusted_root() {
        let root = tmpdir("found");
        let expected = write_marker(&root);
        let cwd = root.join("a").join("b");
        std::fs::create_dir_all(&cwd).unwrap();
        let trusted = vec![root.clone()];
        let got = discover_profile(&cwd, &trusted);
        assert_eq!(got, Some(expected));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn discover_denies_when_marker_outside_trusted_roots() {
        // Marker lives under `project`, but the trusted root is an unrelated sibling.
        let base = tmpdir("outside");
        let project = base.join("project");
        std::fs::create_dir_all(&project).unwrap();
        write_marker(&project);
        let other = base.join("other-root");
        std::fs::create_dir_all(&other).unwrap();
        let got = discover_profile(&project, &[other]);
        assert_eq!(got, None);
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn discover_empty_roots_denies() {
        let root = tmpdir("emptyroots");
        write_marker(&root);
        let got = discover_profile(&root, &[]);
        assert_eq!(got, None);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn discover_rejects_symlink_escape() {
        // The marker's real target lives OUTSIDE the trusted root via a symlink. cwd/root are the
        // "inside" tree; the marker is a symlink pointing into an out-of-fence file.
        let base = tmpdir("symlink");
        let inside = base.join("inside");
        std::fs::create_dir_all(inside.join(".envctl")).unwrap();
        let outside = base.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let real_target = outside.join("relay-profile.toml");
        std::fs::write(&real_target, b"# escaped\n").unwrap();

        let link = inside.join(RELAY_PROFILE_MARKER);
        std::os::unix::fs::symlink(&real_target, &link).unwrap();

        // Trust only `inside`; the symlink's canonical target is under `outside` ⇒ deny.
        let got = discover_profile(&inside, std::slice::from_ref(&inside));
        assert_eq!(got, None);
        std::fs::remove_dir_all(&base).ok();
    }
}
