//! Multi-host source resolver + remote archive download — consolidates kasetto v3.2.0
//! `src/source/{hosts,parse,remote,auth}.rs`.
//!
//! Host families: GitHub (+ GitHub Enterprise), GitLab (+ subgroups, + self-hosted),
//! Bitbucket Cloud, and Gitea-style (Codeberg / Gitea / Forgejo). Includes the
//! browser-URL → raw rewrite, the `ref > branch > default(main→master)` archive-URL
//! precedence, the **tar-slip path-traversal guard** (fail-closed on `..`), and
//! **env-only credentials** ([`UrlRequestAuth`] never reads creds from config or lock).

use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use reqwest::blocking::{Client, RequestBuilder};

use crate::config::{CommandEntry, GitPin, McpEntry, SourceSpec};
use crate::fsops::resolve_path;
use crate::{err, Result};

// ---------------------------------------------------------------------------
// HTTP client (kasetto src/fsops/http.rs)
// ---------------------------------------------------------------------------

static HTTP_CLIENT: OnceLock<std::result::Result<Client, String>> = OnceLock::new();

/// Shared blocking client: avoids TLS/session setup on every asset or config fetch.
/// Uses the workspace `reqwest` pin (rustls → ring); links no C TLS.
pub fn http_client() -> Result<Client> {
    let built = HTTP_CLIENT.get_or_init(|| {
        Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .user_agent(concat!("envctl-agent-env/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| e.to_string())
    });
    match built {
        Ok(c) => Ok(c.clone()),
        Err(e) => Err(err(format!("failed to build HTTP client: {e}"))),
    }
}

// ---------------------------------------------------------------------------
// Host classification (kasetto src/source/hosts.rs)
// ---------------------------------------------------------------------------

pub(crate) fn extract_host(url: &str) -> Option<String> {
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    Some(without_scheme.split('/').next()?.to_string())
}

pub(crate) fn is_gitlab_host(host: &str) -> bool {
    host == "gitlab.com" || host.ends_with(".gitlab.com") || host.starts_with("gitlab.")
}

pub(crate) fn is_bitbucket_host(host: &str) -> bool {
    host == "bitbucket.org" || host == "www.bitbucket.org"
}

/// Hosts that serve Gitea-style `/{owner}/{repo}/archive/{ref}.tar.gz` (Codeberg, Gitea, Forgejo).
pub(crate) fn is_gitea_style_host(host: &str) -> bool {
    matches!(
        host,
        "codeberg.org"
            | "www.codeberg.org"
            | "gitea.com"
            | "www.gitea.com"
            | "forgejo.org"
            | "www.forgejo.org"
    )
}

// ---------------------------------------------------------------------------
// Repo URL parsing (kasetto src/source/parse.rs)
// ---------------------------------------------------------------------------

/// A parsed repository URL, classified by host family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoUrl {
    GitHub {
        host: String,
        owner: String,
        repo: String,
    },
    GitLab {
        host: String,
        project_path: String,
    },
    /// Bitbucket Cloud (`bitbucket.org`).
    Bitbucket {
        workspace: String,
        repo_slug: String,
    },
    /// Gitea / Forgejo — including Codeberg (`codeberg.org`).
    Gitea {
        host: String,
        owner: String,
        repo: String,
    },
}

/// Parse a repository URL into a structured [`RepoUrl`].
pub fn parse_repo_url(url: &str) -> Result<RepoUrl> {
    let cleaned = url.trim_end_matches('/').trim_end_matches(".git");
    let without_scheme = cleaned
        .strip_prefix("https://")
        .or_else(|| cleaned.strip_prefix("http://"))
        .ok_or_else(|| err("unsupported URL scheme"))?;

    let parts: Vec<_> = without_scheme.splitn(2, '/').collect();
    if parts.len() < 2 || parts[1].is_empty() {
        return Err(err("unsupported repository URL"));
    }

    let host = parts[0];
    let path = parts[1];

    if is_gitlab_host(host) {
        return Ok(RepoUrl::GitLab {
            host: host.to_string(),
            project_path: path.to_string(),
        });
    }

    if is_bitbucket_host(host) {
        let segments = path_segments(path);
        if segments.len() != 2 {
            return Err(err(
                "invalid Bitbucket URL: expected https://bitbucket.org/workspace/repo",
            ));
        }
        return Ok(RepoUrl::Bitbucket {
            workspace: segments[0].to_string(),
            repo_slug: segments[1].to_string(),
        });
    }

    let segments = path_segments(path);
    if segments.len() < 2 {
        return Err(err(
            "unsupported repository URL: expected at least owner/repo",
        ));
    }

    if host == "github.com" {
        if segments.len() != 2 {
            return Err(err(
                "invalid GitHub URL: expected https://github.com/owner/repo",
            ));
        }
        return Ok(RepoUrl::GitHub {
            host: host.to_string(),
            owner: segments[0].to_string(),
            repo: segments[1].to_string(),
        });
    }

    if is_gitea_style_host(host) {
        if segments.len() != 2 {
            return Err(err(
                "invalid URL: expected https://host/owner/repo (Gitea / Codeberg style)",
            ));
        }
        return Ok(RepoUrl::Gitea {
            host: host.to_string(),
            owner: segments[0].to_string(),
            repo: segments[1].to_string(),
        });
    }

    if segments.len() >= 3 {
        return Ok(RepoUrl::GitLab {
            host: host.to_string(),
            project_path: path.to_string(),
        });
    }

    Ok(RepoUrl::GitHub {
        host: host.to_string(),
        owner: segments[0].to_string(),
        repo: segments[1].to_string(),
    })
}

fn path_segments(path: &str) -> Vec<&str> {
    path.split('/').filter(|s| !s.is_empty()).collect()
}

/// A repo-browse URL decomposed into the pieces `add` needs: the repo root, the pinned
/// ref, the sub-directory, and (when the URL points at a `SKILL.md`) the skill name.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BrowseDerived {
    pub source: String,
    pub branch: Option<String>,
    pub git_ref: Option<String>,
    pub sub_dir: Option<String>,
    pub skill_name: Option<String>,
}

/// Decompose a GitHub/Gitea/GitLab `blob`/`tree` browse URL. Returns `None` for plain repo
/// URLs and local paths (the caller uses the source verbatim).
pub fn derive_browse_url(url: &str) -> Option<BrowseDerived> {
    let scheme = if url.starts_with("https://") {
        "https://"
    } else if url.starts_with("http://") {
        "http://"
    } else {
        return None;
    };
    let without = url.trim_end_matches('/').strip_prefix(scheme)?;
    let segs: Vec<&str> = without.split('/').filter(|s| !s.is_empty()).collect();

    // Need at least host/owner/repo/<marker>/<ref>.
    let marker = segs.iter().position(|s| *s == "blob" || *s == "tree")?;
    if marker < 3 || marker + 1 >= segs.len() {
        return None;
    }

    // Repo path is everything before the marker, dropping a trailing GitLab `-`.
    let mut repo_end = marker;
    if segs[repo_end - 1] == "-" {
        repo_end -= 1;
    }
    if repo_end < 3 {
        return None;
    }
    let host_and_repo = segs[..repo_end].join("/");
    let source = format!("{scheme}{host_and_repo}");

    let git_ref_seg = segs[marker + 1];
    let rest: Vec<&str> = segs[marker + 2..].to_vec();

    let (sub_dir, skill_name) = if rest.last() == Some(&"SKILL.md") {
        let skill_segs = &rest[..rest.len() - 1];
        match skill_segs.split_last() {
            Some((name, parent)) => {
                let sub = (!parent.is_empty()).then(|| parent.join("/"));
                (sub, Some((*name).to_string()))
            }
            None => (None, None),
        }
    } else {
        let sub = (!rest.is_empty()).then(|| rest.join("/"));
        (sub, None)
    };

    let is_sha = git_ref_seg.len() == 40 && git_ref_seg.bytes().all(|b| b.is_ascii_hexdigit());
    let (branch, git_ref) = if is_sha {
        (None, Some(git_ref_seg.to_string()))
    } else {
        (Some(git_ref_seg.to_string()), None)
    };

    Some(BrowseDerived {
        source,
        branch,
        git_ref,
        sub_dir,
        skill_name,
    })
}

// ---------------------------------------------------------------------------
// Env-only credentials (kasetto src/source/auth.rs)
// ---------------------------------------------------------------------------

/// Optional custom headers plus HTTP Basic credentials (Bitbucket Cloud).
///
/// Credentials are read **from the environment only** — never from config or the lock.
pub struct UrlRequestAuth {
    pub headers: Vec<(String, String)>,
    pub basic: Option<(String, String)>,
}

impl UrlRequestAuth {
    pub fn apply(&self, mut request: RequestBuilder) -> RequestBuilder {
        if let Some((user, pass)) = &self.basic {
            request = request.basic_auth(user, Some(pass));
        }
        for (key, value) in &self.headers {
            request = request.header(key, value);
        }
        request
    }

    fn headers_only(headers: Vec<(String, String)>) -> Self {
        Self {
            headers,
            basic: None,
        }
    }

    fn basic_only(basic: Option<(String, String)>) -> Self {
        Self {
            headers: Vec::new(),
            basic,
        }
    }

    fn for_github_archive() -> Self {
        Self::headers_only(github_auth_headers())
    }

    fn for_gitlab_archive() -> Self {
        Self::headers_only(gitlab_auth_headers())
    }

    fn for_bitbucket_archive() -> Self {
        Self::basic_only(bitbucket_basic_credentials())
    }

    fn for_gitea_archive() -> Self {
        Self::headers_only(gitea_auth_headers())
    }
}

pub(crate) fn auth_env_inline_help(url: &str) -> String {
    match extract_host(url) {
        Some(h) if is_gitlab_host(&h) => {
            "set GITLAB_TOKEN (or CI_JOB_TOKEN in GitLab CI) for private GitLab.".into()
        }
        Some(h) if is_bitbucket_host(&h) => {
            "set BITBUCKET_EMAIL and BITBUCKET_TOKEN (Atlassian API token with repository read), \
             or BITBUCKET_USERNAME and BITBUCKET_APP_PASSWORD for Bitbucket Cloud."
                .into()
        }
        Some(h) if is_gitea_style_host(&h) => {
            "set CODEBERG_TOKEN, GITEA_TOKEN, or FORGEJO_TOKEN for private Codeberg (or other Gitea/Forgejo) repositories."
                .into()
        }
        Some(_) => "set GITHUB_TOKEN or GH_TOKEN for private GitHub or GitHub Enterprise.".into(),
        None => "set GITHUB_TOKEN, GH_TOKEN, GITLAB_TOKEN, Bitbucket credentials (see docs), or CODEBERG_TOKEN / GITEA_TOKEN for private repositories.".into(),
    }
}

/// Extra context for HTTP failures when fetching remote config or repo archives.
pub(crate) fn http_fetch_auth_hint(url: &str, status: u16) -> String {
    match status {
        401 | 403 => format!(" - {}", auth_env_inline_help(url)),
        404 => format!(
            " - if the repo or file is private, {}",
            auth_env_inline_help(url)
        ),
        _ => String::new(),
    }
}

/// Auth for fetching a remote resource over HTTPS (config file or archive).
pub(crate) fn auth_for_request_url(url: &str) -> UrlRequestAuth {
    let Some(host) = extract_host(url) else {
        return UrlRequestAuth {
            headers: Vec::new(),
            basic: None,
        };
    };
    if is_gitlab_host(&host) {
        return UrlRequestAuth::headers_only(gitlab_auth_headers());
    }
    if is_bitbucket_host(&host) {
        return UrlRequestAuth::basic_only(bitbucket_basic_credentials());
    }
    if is_gitea_style_host(&host) {
        return UrlRequestAuth::headers_only(gitea_auth_headers());
    }
    UrlRequestAuth::headers_only(github_auth_headers())
}

fn bitbucket_basic_credentials() -> Option<(String, String)> {
    if let (Ok(email), Ok(token)) = (
        std::env::var("BITBUCKET_EMAIL"),
        std::env::var("BITBUCKET_TOKEN"),
    ) {
        return Some((email, token));
    }
    if let (Ok(user), Ok(pass)) = (
        std::env::var("BITBUCKET_USERNAME"),
        std::env::var("BITBUCKET_APP_PASSWORD"),
    ) {
        return Some((user, pass));
    }
    None
}

fn first_env_var(keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| std::env::var(k).ok())
}

fn gitea_auth_headers() -> Vec<(String, String)> {
    match first_env_var(&["GITEA_TOKEN", "CODEBERG_TOKEN", "FORGEJO_TOKEN"]) {
        Some(token) => vec![("Authorization".to_string(), format!("token {token}"))],
        None => Vec::new(),
    }
}

fn gitlab_auth_headers() -> Vec<(String, String)> {
    if let Ok(token) = std::env::var("GITLAB_TOKEN") {
        vec![("PRIVATE-TOKEN".to_string(), token)]
    } else if let Ok(token) = std::env::var("CI_JOB_TOKEN") {
        vec![("JOB-TOKEN".to_string(), token)]
    } else {
        Vec::new()
    }
}

fn github_auth_headers() -> Vec<(String, String)> {
    match first_env_var(&["GITHUB_TOKEN", "GH_TOKEN"]) {
        Some(token) => vec![("Authorization".to_string(), format!("Bearer {token}"))],
        None => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Archive URL builders (kasetto src/source/remote.rs)
// ---------------------------------------------------------------------------

/// Resolve the archive URL + auth for a [`RepoUrl`] under a [`GitPin`], honoring the
/// `ref > branch > default` precedence.
///
/// For [`GitPin::Default`] this returns the `main` branch URL; the `main → master`
/// fallback (retrying `master` on a failed `main` fetch) is the orchestration concern of
/// the higher-level materializer (TASK-0013), since it requires a second HTTP attempt.
pub fn archive_url(parsed: &RepoUrl, pin: &GitPin) -> (String, UrlRequestAuth) {
    match pin {
        GitPin::Ref(r) => remote_repo_archive_ref(parsed, r),
        GitPin::Branch(b) => remote_repo_archive_branch(parsed, b),
        GitPin::Default => remote_repo_archive_branch(parsed, "main"),
    }
}

/// Build archive URL for a branch name (uses `refs/heads/` prefix for GitHub web archive).
pub(crate) fn remote_repo_archive_branch(
    parsed: &RepoUrl,
    branch: &str,
) -> (String, UrlRequestAuth) {
    match parsed {
        RepoUrl::GitHub { host, owner, repo } => {
            let auth = UrlRequestAuth::for_github_archive();
            // GitHub's web archive endpoint doesn't support token auth for private repos.
            // The API endpoint (api.github.com) does and works for public repos too.
            let url = if host == "github.com" && !auth.headers.is_empty() {
                format!(
                    "https://api.{host}/repos/{owner}/{repo}/tarball/{}",
                    encode_github_ref(branch)
                )
            } else {
                format!("https://{host}/{owner}/{repo}/archive/refs/heads/{branch}.tar.gz")
            };
            (url, auth)
        }
        _ => remote_repo_archive_ref(parsed, branch),
    }
}

/// Build archive URL for a generic git ref (tag, SHA, branch). Uses the short form that
/// works for any ref type on all hosts.
pub(crate) fn remote_repo_archive_ref(parsed: &RepoUrl, git_ref: &str) -> (String, UrlRequestAuth) {
    match parsed {
        RepoUrl::GitHub { host, owner, repo } => {
            let auth = UrlRequestAuth::for_github_archive();
            let url = if host == "github.com" && !auth.headers.is_empty() {
                format!(
                    "https://api.{host}/repos/{owner}/{repo}/tarball/{}",
                    encode_github_ref(git_ref)
                )
            } else {
                format!("https://{host}/{owner}/{repo}/archive/{git_ref}.tar.gz")
            };
            (url, auth)
        }
        RepoUrl::GitLab { host, project_path } => (
            gitlab_project_archive_url(host, project_path, git_ref),
            UrlRequestAuth::for_gitlab_archive(),
        ),
        RepoUrl::Bitbucket {
            workspace,
            repo_slug,
        } => (
            bitbucket_archive_tarball_url(workspace, repo_slug, git_ref),
            UrlRequestAuth::for_bitbucket_archive(),
        ),
        RepoUrl::Gitea { host, owner, repo } => (
            gitea_archive_tarball_url(host, owner, repo, git_ref),
            UrlRequestAuth::for_gitea_archive(),
        ),
    }
}

/// GitLab API path encoding: `/` → `%2F`.
fn encode_gitlab_path(path: &str) -> String {
    path.replace('/', "%2F")
}

/// GitHub API ref encoding: `/` → `%2F` so refs like `feature/foo` stay one path segment.
fn encode_github_ref(git_ref: &str) -> String {
    git_ref.replace('/', "%2F")
}

fn gitlab_project_archive_url(host: &str, project_path: &str, branch: &str) -> String {
    let encoded = encode_gitlab_path(project_path);
    format!("https://{host}/api/v4/projects/{encoded}/repository/archive.tar.gz?sha={branch}")
}

fn gitlab_repository_file_raw_url(
    host: &str,
    project: &str,
    file_path: &str,
    git_ref: &str,
) -> String {
    format!(
        "https://{host}/api/v4/projects/{}/repository/files/{}/raw?ref={git_ref}",
        encode_gitlab_path(project),
        encode_gitlab_path(file_path),
    )
}

/// Bitbucket Cloud source archive (`.../get/{branch}.tar.gz`).
fn bitbucket_archive_tarball_url(workspace: &str, repo_slug: &str, branch: &str) -> String {
    format!("https://bitbucket.org/{workspace}/{repo_slug}/get/{branch}.tar.gz")
}

fn gitea_archive_tarball_url(host: &str, owner: &str, repo: &str, branch: &str) -> String {
    format!("https://{host}/{owner}/{repo}/archive/{branch}.tar.gz")
}

/// Rewrite browser-style URLs (`/blob/`, `/src/branch/`, GitLab `/-/blob/`) to the
/// raw-content equivalent so a pasted browser URL can be used as a source / config ref.
pub fn rewrite_browse_to_raw_url(url: &str) -> Option<String> {
    let (cleaned, query) = match url.split_once('?') {
        Some((c, q)) => (c, Some(q)),
        None => (url, None),
    };
    let scheme_len = if cleaned.starts_with("https://") {
        "https://".len()
    } else if cleaned.starts_with("http://") {
        "http://".len()
    } else {
        return None;
    };
    let scheme = &cleaned[..scheme_len];
    let without_scheme = &cleaned[scheme_len..];
    let (host, rest) = without_scheme.split_once('/')?;

    if host == "github.com" {
        if let Some(rewritten) = rewrite_github_blob(rest) {
            return Some(rewritten);
        }
        return None;
    }

    if is_gitea_style_host(host) {
        if let Some(rewritten) = rewrite_gitea_src(scheme, host, rest, query) {
            return Some(rewritten);
        }
        return None;
    }

    rewrite_gitlab_raw_url(host, rest)
}

fn rewrite_github_blob(rest: &str) -> Option<String> {
    let parts: Vec<&str> = rest.splitn(5, '/').collect();
    if parts.len() < 5 {
        return None;
    }
    let (owner, repo, marker, git_ref, file_path) =
        (parts[0], parts[1], parts[2], parts[3], parts[4]);
    if !matches!(marker, "blob" | "raw") {
        return None;
    }
    if owner.is_empty() || repo.is_empty() || git_ref.is_empty() || file_path.is_empty() {
        return None;
    }
    Some(format!(
        "https://raw.githubusercontent.com/{owner}/{repo}/{git_ref}/{file_path}"
    ))
}

fn rewrite_gitea_src(scheme: &str, host: &str, rest: &str, query: Option<&str>) -> Option<String> {
    let parts: Vec<&str> = rest.splitn(6, '/').collect();
    if parts.len() < 6 {
        return None;
    }
    let (owner, repo, src, kind, git_ref, file_path) =
        (parts[0], parts[1], parts[2], parts[3], parts[4], parts[5]);
    if src != "src" {
        return None;
    }
    if !matches!(kind, "branch" | "commit" | "tag") {
        return None;
    }
    if owner.is_empty() || repo.is_empty() || git_ref.is_empty() || file_path.is_empty() {
        return None;
    }
    let mut out = format!("{scheme}{host}/{owner}/{repo}/raw/{kind}/{git_ref}/{file_path}");
    if let Some(q) = query {
        out.push('?');
        out.push_str(q);
    }
    Some(out)
}

fn rewrite_gitlab_raw_url(host: &str, rest: &str) -> Option<String> {
    for marker in ["/-/raw/", "/-/blob/"] {
        if let Some(idx) = rest.find(marker) {
            let project = &rest[..idx];
            let after = &rest[idx + marker.len()..];
            let (ref_name, file_path) = after.split_once('/')?;
            return Some(gitlab_repository_file_raw_url(
                host, project, file_path, ref_name,
            ));
        }
    }

    let parts: Vec<&str> = rest.split('/').collect();
    if parts.len() < 3 {
        return None;
    }
    let file_start = parts.iter().position(|p| p.contains('.'))?;
    if file_start < 2 {
        return None;
    }
    let project = parts[..file_start].join("/");
    let file_path = parts[file_start..].join("/");
    Some(gitlab_repository_file_raw_url(
        host, &project, &file_path, "main",
    ))
}

// ---------------------------------------------------------------------------
// Download + extract with tar-slip guard (kasetto src/source/remote.rs)
// ---------------------------------------------------------------------------

/// Fetch `fetch_url`, gunzip + untar it into `dst`, stripping the archive's top-level
/// directory. **Fail-closed tar-slip guard:** any entry whose post-strip relative path
/// contains a `..` ([`Component::ParentDir`]) is rejected with `"unsafe archive path"`.
pub fn download_extract(
    fetch_url: &str,
    auth: &UrlRequestAuth,
    dst: &Path,
    user_source: &str,
) -> Result<()> {
    if dst.exists() {
        fs::remove_dir_all(dst)?;
    }
    fs::create_dir_all(dst)?;
    let request = http_client()?.get(fetch_url);
    let request = auth.apply(request);
    let response = request
        .send()
        .map_err(|e| err(format!("failed to reach {user_source}: {e}")))?;
    let status = response.status();
    let status_u16 = status.as_u16();
    let body = response
        .bytes()
        .map_err(|e| err(format!("failed to read archive for {user_source}: {e}")))?;
    if !status.is_success() {
        return Err(err(format!(
            "failed to download {user_source} (HTTP {status_u16}){}",
            http_fetch_auth_hint(user_source, status_u16)
        )));
    }
    if body.starts_with(b"<") || body.starts_with(b"<!") {
        return Err(err(format!(
            "failed to download {user_source}: server returned HTML instead of a .tar.gz - {}",
            auth_env_inline_help(user_source)
        )));
    }
    extract_tar_gz(body.as_ref(), dst)
}

/// Gunzip + untar `body` into `dst`, stripping the leading archive directory and enforcing
/// the tar-slip guard. Split out so the guard is unit-testable without a network fetch.
pub(crate) fn extract_tar_gz(body: &[u8], dst: &Path) -> Result<()> {
    let gz = flate2::read::GzDecoder::new(body);
    let mut archive = tar::Archive::new(gz);
    for entry in archive.entries()? {
        let mut entry = entry?;
        let p = entry.path()?;
        let parts: Vec<_> = p.components().collect();
        if parts.len() < 2 {
            continue;
        }
        let rel = parts
            .iter()
            .skip(1)
            .map(|c| c.as_os_str())
            .collect::<PathBuf>();
        if rel.components().any(|c| c == Component::ParentDir) {
            return Err(err("unsafe archive path"));
        }
        let target = dst.join(rel);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        entry.unpack(target)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Source materialization + asset discovery (kasetto src/source/mod.rs)
// ---------------------------------------------------------------------------

/// A materialized source: a local on-disk root plus the skills discovered inside it.
///
/// `source_revision` records what was resolved (`local`, `ref:<r>`, `branch:<b>`, or
/// `branch:main`); `cleanup_dir`, when present, is the staging directory the caller
/// should remove once the assets have been copied out (`None` for in-place local
/// sources, which must never be deleted).
pub struct MaterializedSource {
    pub source_revision: String,
    pub available: HashMap<String, PathBuf>,
    pub source_root: PathBuf,
    pub cleanup_dir: Option<PathBuf>,
}

/// Derive a repo-name hint from a parsed URL (used as the root-level skill name when a
/// repo's `SKILL.md` sits at its top level).
fn repo_name_hint(parsed: &RepoUrl) -> String {
    match parsed {
        RepoUrl::GitHub { repo, .. } => repo.clone(),
        RepoUrl::GitLab { project_path, .. } => project_path
            .split('/')
            .next_back()
            .unwrap_or(project_path)
            .to_string(),
        RepoUrl::Bitbucket { repo_slug, .. } => repo_slug.clone(),
        RepoUrl::Gitea { repo, .. } => repo.clone(),
    }
}

/// Resolve the effective source root: `base_root` itself, or a validated `sub_dir` beneath
/// it. The sub-dir must be relative and must not escape the root (fail-closed on `..`).
fn resolve_source_root(base_root: &Path, sub_dir: Option<&str>) -> Result<PathBuf> {
    let Some(sub_dir) = sub_dir else {
        return Ok(base_root.to_path_buf());
    };

    let trimmed = sub_dir.trim();
    if trimmed.is_empty() {
        return Err(err("skills source `sub-dir` cannot be empty"));
    }

    let rel = Path::new(trimmed);
    if rel.is_absolute() {
        return Err(err("skills source `sub-dir` must be relative"));
    }
    if rel
        .components()
        .any(|c| matches!(c, Component::ParentDir | Component::RootDir))
    {
        return Err(err(
            "skills source `sub-dir` must not escape the source root",
        ));
    }

    let resolved = base_root.join(rel);
    if !resolved.exists() {
        return Err(err(format!(
            "skills source sub-dir not found: {}",
            resolved.display()
        )));
    }
    if !resolved.is_dir() {
        return Err(err(format!(
            "skills source sub-dir is not a directory: {}",
            resolved.display()
        )));
    }
    Ok(resolved)
}

/// Materialize a [`SourceSpec`] into a local on-disk root and discover its skills.
///
/// - **Local** sources (no `://`) are resolved against `cfg_dir` and used **in place**:
///   `source_revision = "local"`, `cleanup_dir = None`.
/// - **Remote** sources are downloaded + extracted into the caller-provided `stage`
///   directory (reusing [`download_extract`] + [`remote_repo_archive_branch`] /
///   [`remote_repo_archive_ref`]). The resolved revision label is carried back, and
///   `cleanup_dir = Some(stage)` so the caller can remove the staging tree afterward.
///
/// For [`GitPin::Default`] the `main` archive is tried first; a failed fetch retries
/// `master` before giving up (the deferred `main → master` fallback).
pub fn materialize_source(
    src: &SourceSpec,
    cfg_dir: &Path,
    stage: &Path,
) -> Result<MaterializedSource> {
    if src.source.contains("://") {
        let parsed = parse_repo_url(&src.source)?;
        let pin = src.git_pin();

        let source_revision = match &pin {
            GitPin::Ref(r) => {
                let (url, auth) = remote_repo_archive_ref(&parsed, r);
                download_extract(&url, &auth, stage, &src.source)?;
                format!("ref:{r}")
            }
            GitPin::Branch(b) => {
                let (url, auth) = remote_repo_archive_branch(&parsed, b);
                download_extract(&url, &auth, stage, &src.source)?;
                format!("branch:{b}")
            }
            GitPin::Default => {
                let (url, auth) = remote_repo_archive_branch(&parsed, "main");
                download_extract(&url, &auth, stage, &src.source).or_else(|_| {
                    let (url, auth) = remote_repo_archive_branch(&parsed, "master");
                    download_extract(&url, &auth, stage, &src.source).map_err(|e2| {
                        err(format!("{e2} (also tried branch `master` after `main`)"))
                    })
                })?;
                "branch:main".into()
            }
        };

        let source_root = resolve_source_root(stage, src.sub_dir.as_deref())?;
        let hint = src
            .sub_dir
            .as_deref()
            .and_then(|sub_dir| Path::new(sub_dir).file_name())
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .map(|name| name.to_string())
            .unwrap_or_else(|| repo_name_hint(&parsed));
        let available = discover_with_root_name(&source_root, Some(hint.as_str()))?;
        Ok(MaterializedSource {
            source_revision,
            available,
            source_root,
            cleanup_dir: Some(stage.to_path_buf()),
        })
    } else {
        let root = resolve_path(cfg_dir, &src.source);
        let source_root = resolve_source_root(&root, src.sub_dir.as_deref())?;
        let available = discover(&source_root)?;
        Ok(MaterializedSource {
            source_revision: "local".into(),
            available,
            source_root,
            cleanup_dir: None,
        })
    }
}

/// Discover SKILL.md-bearing skill directories under `root`, naming a root-level skill by
/// the directory's own name.
pub fn discover(root: &Path) -> Result<HashMap<String, PathBuf>> {
    let root_name = root.file_name().and_then(|name| name.to_str());
    discover_with_root_name(root, root_name)
}

/// Like [`discover`], but uses `root_name` for a root-level `SKILL.md` (so a remote repo's
/// top-level skill is named by its repo / sub-dir hint rather than a temp-dir name).
pub fn discover_with_root_name(
    root: &Path,
    root_name: Option<&str>,
) -> Result<HashMap<String, PathBuf>> {
    let mut out = HashMap::new();
    let root_skill_name = if root.join("SKILL.md").is_file() {
        if let Some(name) = root_name.filter(|name| !name.is_empty()) {
            out.insert(name.to_string(), root.to_path_buf());
            Some(name.to_string())
        } else {
            None
        }
    } else {
        None
    };
    discover_skills_in_subdir(root, &mut out)?;
    discover_skills_in_subdir(&root.join("skills"), &mut out)?;
    if let Some(ref name) = root_skill_name {
        if out.get(name).is_some_and(|p| p != root) {
            eprintln!("warning: subdirectory skill `{name}` shadows root-level SKILL.md");
        }
    }
    Ok(out)
}

fn discover_skills_in_subdir(base: &Path, out: &mut HashMap<String, PathBuf>) -> Result<()> {
    if !base.exists() {
        return Ok(());
    }
    for e in fs::read_dir(base)? {
        let e = e?;
        if !e.path().is_dir() {
            continue;
        }
        let d = e.path();
        if d.join("SKILL.md").is_file() {
            out.insert(e.file_name().to_string_lossy().into_owned(), d);
        }
    }
    Ok(())
}

/// Locate MCP pack JSON(s) in a source: well-known root files (`.mcp.json`, `mcp.json`)
/// plus every `*.json` under `mcps/` (warning if the legacy `mcp/` layout is present).
pub fn discover_mcps(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();

    // Check well-known root-level MCP files (.mcp.json is the Claude Code convention).
    for name in [".mcp.json", "mcp.json"] {
        let p = root.join(name);
        if p.is_file() {
            out.push(p);
        }
    }

    // Warn if the old mcp/ layout is present but mcps/ is not.
    if root.join("mcp").exists() && !root.join("mcps").exists() {
        eprintln!(
            "warning: found a `mcp/` directory but the scanner now uses `mcps/` — \
             rename it to suppress this warning"
        );
    }

    // Check mcps/ subdirectory for additional pack JSON files.
    let mcp_dir = root.join("mcps");
    if mcp_dir.exists() {
        for e in fs::read_dir(mcp_dir)? {
            let e = e?;
            let path = e.path();
            if e.file_type()?.is_file() && path.extension().is_some_and(|ext| ext == "json") {
                out.push(path);
            }
        }
    }

    Ok(out)
}

/// Resolve one [`McpEntry`] to a file path — mirrors skill discovery convention.
///
/// - `Name("github")` → `<root>/mcps/github.json`
/// - `Obj { name: "github", path: Some("tools") }` → `<root>/tools/github.json`
/// - `Obj { name: "github", path: None }` → `<root>/mcps/github.json`
///
/// `.json` is appended automatically when the name has no extension.
pub fn resolve_mcp_entry(root: &Path, entry: &McpEntry) -> Result<PathBuf> {
    let (name, dir) = match entry {
        McpEntry::Name(n) => (n.as_str(), "mcps"),
        McpEntry::Obj { name, path } => (name.as_str(), path.as_deref().unwrap_or("mcps")),
    };
    let filename = if name.ends_with(".json") {
        name.to_string()
    } else {
        format!("{name}.json")
    };
    let target = root.join(dir).join(&filename);
    if target.is_file() {
        Ok(target)
    } else {
        Err(err(format!(
            "MCP entry not found: {filename} in {dir}/ (resolved to {})",
            target.display()
        )))
    }
}

/// Walk `<root>/commands/**/*.md` and return a map of namespaced name → file path.
///
/// Subdirectory nesting becomes `:` separated namespaces:
/// - `commands/commit.md` → `commit`
/// - `commands/git/commit.md` → `git:commit`
/// - `commands/git/work/status.md` → `git:work:status`
pub fn discover_commands(root: &Path) -> Result<HashMap<String, PathBuf>> {
    let mut out = HashMap::new();
    let base = root.join("commands");
    if !base.exists() {
        return Ok(out);
    }
    walk_commands(&base, &base, &mut out)?;
    Ok(out)
}

fn walk_commands(base: &Path, cur: &Path, out: &mut HashMap<String, PathBuf>) -> Result<()> {
    for e in fs::read_dir(cur)? {
        let e = e?;
        let path = e.path();
        let ft = e.file_type()?;
        if ft.is_dir() {
            walk_commands(base, &path, out)?;
            continue;
        }
        if !ft.is_file() {
            continue;
        }
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let rel = match path.strip_prefix(base) {
            Ok(p) => p,
            Err(_) => continue,
        };
        let mut parts: Vec<String> = rel
            .components()
            .filter_map(|c| match c {
                Component::Normal(s) => s.to_str().map(str::to_string),
                _ => None,
            })
            .collect();
        let Some(last) = parts.last_mut() else {
            continue;
        };
        if let Some(stem) = Path::new(last.as_str())
            .file_stem()
            .and_then(|s| s.to_str())
        {
            *last = stem.to_string();
        }
        let name = parts.join(":");
        out.insert(name, path);
    }
    Ok(())
}

/// Resolve one [`CommandEntry`] to a `(namespaced-name, file-path)` pair.
///
/// - `Name("review-pr")` → look up by namespaced name in [`discover_commands`].
/// - `Obj { name: "deploy", path: Some("ops") }` → `<root>/ops/deploy.md`.
/// - `Obj { name: "deploy", path: None }` → look up by namespaced name.
pub fn resolve_command_entry(root: &Path, entry: &CommandEntry) -> Result<(String, PathBuf)> {
    match entry {
        CommandEntry::Name(n) => resolve_named_command(root, n),
        CommandEntry::Obj { name, path } => {
            if let Some(dir) = path {
                let filename = if name.ends_with(".md") {
                    name.clone()
                } else {
                    format!("{name}.md")
                };
                let target = root.join(dir).join(&filename);
                if target.is_file() {
                    let derived = name.trim_end_matches(".md").to_string();
                    Ok((derived, target))
                } else {
                    Err(err(format!(
                        "command entry not found: {filename} in {dir}/ (resolved to {})",
                        target.display()
                    )))
                }
            } else {
                resolve_named_command(root, name)
            }
        }
    }
}

fn resolve_named_command(root: &Path, name: &str) -> Result<(String, PathBuf)> {
    let available = discover_commands(root)?;
    if let Some(path) = available.get(name) {
        return Ok((name.to_string(), path.clone()));
    }
    Err(err(format!(
        "command entry not found: {name} (looked in commands/ with subdir namespaces)"
    )))
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::sync::Mutex;

    use super::*;
    use crate::config::SkillsField;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    // --- parse_repo_url ---

    #[test]
    fn parse_repo_url_github() {
        let url = parse_repo_url("https://github.com/openai/skills").expect("parse");
        assert!(matches!(url, RepoUrl::GitHub { host, owner, repo }
            if host == "github.com" && owner == "openai" && repo == "skills"));
    }

    #[test]
    fn parse_repo_url_github_enterprise_two_segment_path() {
        let url = parse_repo_url("https://ghe.example.com/acme/skill-pack").expect("parse");
        assert!(matches!(url, RepoUrl::GitHub { host, owner, repo }
            if host == "ghe.example.com" && owner == "acme" && repo == "skill-pack"));
    }

    #[test]
    fn parse_repo_url_github_trims_git_and_trailing_slash() {
        let url = parse_repo_url("https://github.com/pivoshenko/kasetto.git/").expect("parse");
        assert!(matches!(url, RepoUrl::GitHub { host, owner, repo }
            if host == "github.com" && owner == "pivoshenko" && repo == "kasetto"));
    }

    #[test]
    fn parse_repo_url_gitlab_subgroup() {
        let url = parse_repo_url("https://gitlab.example.com/group/subgroup/repo").expect("parse");
        assert!(matches!(url, RepoUrl::GitLab { host, project_path }
            if host == "gitlab.example.com" && project_path == "group/subgroup/repo"));
    }

    #[test]
    fn parse_repo_url_gitlab_com_two_segments() {
        let url = parse_repo_url("https://gitlab.com/group/project").expect("parse");
        assert!(matches!(url, RepoUrl::GitLab { host, project_path }
            if host == "gitlab.com" && project_path == "group/project"));
    }

    #[test]
    fn parse_repo_url_bitbucket_cloud() {
        let url = parse_repo_url("https://bitbucket.org/workspace/skill-repo").expect("parse");
        assert!(matches!(url, RepoUrl::Bitbucket { workspace, repo_slug }
            if workspace == "workspace" && repo_slug == "skill-repo"));
    }

    #[test]
    fn parse_repo_url_codeberg() {
        let url = parse_repo_url("https://codeberg.org/someone/skills").expect("parse");
        assert!(matches!(url, RepoUrl::Gitea { host, owner, repo }
            if host == "codeberg.org" && owner == "someone" && repo == "skills"));
    }

    #[test]
    fn parse_repo_url_rejects_non_http_scheme() {
        assert!(parse_repo_url("git@github.com:o/r.git").is_err());
    }

    // --- derive_browse_url ---

    #[test]
    fn derive_blob_skill_md_splits_subdir_and_name() {
        let d = derive_browse_url(
            "https://github.com/mattpocock/skills/blob/main/skills/personal/edit-article/SKILL.md",
        )
        .expect("derive");
        assert_eq!(d.source, "https://github.com/mattpocock/skills");
        assert_eq!(d.branch.as_deref(), Some("main"));
        assert_eq!(d.git_ref, None);
        assert_eq!(d.sub_dir.as_deref(), Some("skills/personal"));
        assert_eq!(d.skill_name.as_deref(), Some("edit-article"));
    }

    #[test]
    fn derive_tree_uses_path_as_subdir_no_name() {
        let d = derive_browse_url("https://github.com/mattpocock/skills/tree/main/skills/personal")
            .expect("derive");
        assert_eq!(d.source, "https://github.com/mattpocock/skills");
        assert_eq!(d.branch.as_deref(), Some("main"));
        assert_eq!(d.sub_dir.as_deref(), Some("skills/personal"));
        assert_eq!(d.skill_name, None);
    }

    #[test]
    fn derive_sha_ref_is_pinned_not_branch() {
        let sha = "a".repeat(40);
        let d =
            derive_browse_url(&format!("https://github.com/o/r/tree/{sha}/pack")).expect("derive");
        assert_eq!(d.git_ref.as_deref(), Some(sha.as_str()));
        assert_eq!(d.branch, None);
    }

    #[test]
    fn derive_gitlab_dash_separator() {
        let d = derive_browse_url("https://gitlab.com/group/proj/-/tree/main/skills/a")
            .expect("derive");
        assert_eq!(d.source, "https://gitlab.com/group/proj");
        assert_eq!(d.branch.as_deref(), Some("main"));
        assert_eq!(d.sub_dir.as_deref(), Some("skills/a"));
    }

    #[test]
    fn derive_plain_repo_url_is_none() {
        assert_eq!(derive_browse_url("https://github.com/owner/repo"), None);
        assert_eq!(derive_browse_url("./local/pack"), None);
    }

    // --- archive URL builders + GitPin precedence ---

    #[test]
    fn archive_url_honors_ref_branch_default_precedence() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("GITHUB_TOKEN");
        std::env::remove_var("GH_TOKEN");
        let parsed = RepoUrl::GitHub {
            host: "github.com".into(),
            owner: "o".into(),
            repo: "r".into(),
        };
        let (url_ref, _) = archive_url(&parsed, &GitPin::Ref("v2.0".into()));
        assert_eq!(url_ref, "https://github.com/o/r/archive/v2.0.tar.gz");
        let (url_branch, _) = archive_url(&parsed, &GitPin::Branch("dev".into()));
        assert_eq!(
            url_branch,
            "https://github.com/o/r/archive/refs/heads/dev.tar.gz"
        );
        let (url_default, _) = archive_url(&parsed, &GitPin::Default);
        assert_eq!(
            url_default,
            "https://github.com/o/r/archive/refs/heads/main.tar.gz"
        );
    }

    #[test]
    fn github_branch_archive_uses_refs_heads_prefix_without_token() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::remove_var("GITHUB_TOKEN");
        std::env::remove_var("GH_TOKEN");
        let parsed = RepoUrl::GitHub {
            host: "github.com".into(),
            owner: "o".into(),
            repo: "r".into(),
        };
        let (url, _) = remote_repo_archive_branch(&parsed, "main");
        assert_eq!(url, "https://github.com/o/r/archive/refs/heads/main.tar.gz");
    }

    #[test]
    fn github_branch_archive_uses_api_endpoint_with_token() {
        let _guard = ENV_LOCK.lock().unwrap();
        std::env::set_var("GITHUB_TOKEN", "test-token");
        let parsed = RepoUrl::GitHub {
            host: "github.com".into(),
            owner: "o".into(),
            repo: "r".into(),
        };
        let (url, _) = remote_repo_archive_branch(&parsed, "feature/foo");
        std::env::remove_var("GITHUB_TOKEN");
        assert_eq!(
            url,
            "https://api.github.com/repos/o/r/tarball/feature%2Ffoo"
        );
    }

    #[test]
    fn bitbucket_and_gitea_archive_urls() {
        assert_eq!(
            bitbucket_archive_tarball_url("ws", "myrepo", "main"),
            "https://bitbucket.org/ws/myrepo/get/main.tar.gz"
        );
        assert_eq!(
            gitea_archive_tarball_url("codeberg.org", "a", "b", "main"),
            "https://codeberg.org/a/b/archive/main.tar.gz"
        );
    }

    #[test]
    fn gitlab_archive_url_encodes_subgroup_path() {
        let parsed = RepoUrl::GitLab {
            host: "gitlab.com".into(),
            project_path: "group/sub/repo".into(),
        };
        let (url, _) = remote_repo_archive_ref(&parsed, "main");
        assert_eq!(
            url,
            "https://gitlab.com/api/v4/projects/group%2Fsub%2Frepo/repository/archive.tar.gz?sha=main"
        );
    }

    // --- rewrite_browse_to_raw_url ---

    #[test]
    fn rewrite_github_blob_url_to_raw() {
        let out = rewrite_browse_to_raw_url(
            "https://github.com/pivoshenko/kasetto/blob/main/kasetto.yml",
        )
        .expect("rewritten");
        assert_eq!(
            out,
            "https://raw.githubusercontent.com/pivoshenko/kasetto/main/kasetto.yml"
        );
    }

    #[test]
    fn rewrite_gitea_src_to_raw() {
        let out = rewrite_browse_to_raw_url(
            "https://codeberg.org/owner/repo/src/branch/main/kasetto.yml",
        )
        .expect("rewritten");
        assert_eq!(
            out,
            "https://codeberg.org/owner/repo/raw/branch/main/kasetto.yml"
        );
    }

    #[test]
    fn rewrite_gitlab_blob_url_uses_api_raw_endpoint() {
        let out =
            rewrite_browse_to_raw_url("https://gitlab.com/group/sub/repo/-/blob/main/kasetto.yml")
                .expect("rewritten");
        assert_eq!(
            out,
            "https://gitlab.com/api/v4/projects/group%2Fsub%2Frepo/repository/files/kasetto.yml/raw?ref=main"
        );
    }

    #[test]
    fn rewrite_skips_unrecognized_and_non_http() {
        assert!(rewrite_browse_to_raw_url("https://example.com/some/path").is_none());
        assert!(rewrite_browse_to_raw_url("git@github.com:owner/repo.git").is_none());
        assert!(rewrite_browse_to_raw_url("https://github.com/owner/repo").is_none());
    }

    // --- tar-slip guard ---

    /// Build a one-entry .tar.gz with the given member path, top-level-dir stripped on
    /// extract. Writes a raw ustar header so a malicious `..` member path can be emitted
    /// (`tar::Builder::append_data` refuses `..`, which is exactly what a real attacker's
    /// archive bypasses — so we hand-craft the header to exercise our own guard).
    fn make_tar_gz(member_path: &str, contents: &[u8]) -> Vec<u8> {
        let mut header = [0u8; 512];
        // name (offset 0, len 100)
        let name = member_path.as_bytes();
        header[..name.len()].copy_from_slice(name);
        // mode (offset 100, len 8) — "0000644\0"
        header[100..108].copy_from_slice(b"0000644\0");
        // uid/gid (offset 108/116, len 8 each)
        header[108..116].copy_from_slice(b"0000000\0");
        header[116..124].copy_from_slice(b"0000000\0");
        // size (offset 124, len 12) — octal, NUL-terminated
        let size_oct = format!("{:011o}\0", contents.len());
        header[124..136].copy_from_slice(size_oct.as_bytes());
        // mtime (offset 136, len 12)
        header[136..148].copy_from_slice(b"00000000000\0");
        // typeflag (offset 156) — '0' regular file
        header[156] = b'0';
        // ustar magic (offset 257, len 6) + version (263, len 2)
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        // checksum (offset 148, len 8): spaces during computation, then octal + NUL + space
        header[148..156].copy_from_slice(b"        ");
        let sum: u32 = header.iter().map(|&b| b as u32).sum();
        let chk = format!("{sum:06o}\0 ");
        header[148..156].copy_from_slice(chk.as_bytes());

        let mut tar_buf = Vec::new();
        tar_buf.extend_from_slice(&header);
        tar_buf.extend_from_slice(contents);
        // pad content to a 512-byte boundary
        let pad = (512 - (contents.len() % 512)) % 512;
        tar_buf.extend(std::iter::repeat(0u8).take(pad));
        // two 512-byte zero blocks mark end of archive
        tar_buf.extend(std::iter::repeat(0u8).take(1024));

        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(&tar_buf).expect("gz write");
        gz.finish().expect("gz finish")
    }

    fn temp_dir(prefix: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{nanos}"))
    }

    #[test]
    fn extract_rejects_parent_dir_traversal() {
        // After stripping the top-level `repo/`, the entry resolves to `../evil.txt`.
        let gz = make_tar_gz("repo/../evil.txt", b"pwn");
        let dst = temp_dir("agent-env-tarslip");
        let result = extract_tar_gz(&gz, &dst);
        assert!(result.is_err(), "tar-slip must be rejected");
        let msg = format!("{}", result.err().unwrap());
        assert!(msg.contains("unsafe archive path"), "got: {msg}");
        let _ = fs::remove_dir_all(&dst);
    }

    #[test]
    fn extract_accepts_safe_entry_and_strips_top_dir() {
        let gz = make_tar_gz("repo/skills/a/SKILL.md", b"# ok\n");
        let dst = temp_dir("agent-env-tarok");
        extract_tar_gz(&gz, &dst).expect("safe extract");
        let written = dst.join("skills/a/SKILL.md");
        assert!(written.is_file(), "expected stripped path to be written");
        assert_eq!(fs::read_to_string(&written).unwrap(), "# ok\n");
        let _ = fs::remove_dir_all(&dst);
    }

    // --- auth env inline help + http-fetch auth hint (S-12 / S-13) ---
    //
    // `auth_env_inline_help`/`http_fetch_auth_hint` are `pub(crate)`, so they cannot be
    // reached from the cross-crate `tests/parity_vs_kasetto.rs` harness. These in-crate tests
    // mirror kasetto's OWN certified vectors in `src/source/auth.rs::tests`.

    // S-13 — Oracle: kasetto src/source/auth.rs::tests::http_fetch_auth_hint_mentions_github_token_for_github_host
    #[test]
    fn http_fetch_auth_hint_mentions_github_token_for_github_host() {
        let h = http_fetch_auth_hint("https://github.com/org/private", 403);
        assert!(h.contains("GITHUB_TOKEN") || h.contains("GH_TOKEN"), "{h}");
    }

    // S-13 — Oracle: kasetto src/source/auth.rs::tests::http_fetch_auth_hint_mentions_gitlab_token_for_gitlab_host
    #[test]
    fn http_fetch_auth_hint_mentions_gitlab_token_for_gitlab_host() {
        let h = http_fetch_auth_hint("https://gitlab.com/group/proj", 401);
        assert!(h.contains("GITLAB_TOKEN"), "{h}");
    }

    // S-13 — Oracle: kasetto src/source/auth.rs::tests::http_fetch_auth_hint_mentions_bitbucket_env_for_bitbucket_host
    #[test]
    fn http_fetch_auth_hint_mentions_bitbucket_env_for_bitbucket_host() {
        let h = http_fetch_auth_hint("https://bitbucket.org/ws/r", 403);
        assert!(
            h.contains("BITBUCKET_EMAIL") || h.contains("BITBUCKET_USERNAME"),
            "{h}"
        );
    }

    // S-13 — Oracle: kasetto src/source/auth.rs::tests::http_fetch_auth_hint_mentions_gitea_token_for_codeberg_host
    #[test]
    fn http_fetch_auth_hint_mentions_gitea_token_for_codeberg_host() {
        let h = http_fetch_auth_hint("https://codeberg.org/u/r", 401);
        assert!(
            h.contains("CODEBERG_TOKEN")
                || h.contains("GITEA_TOKEN")
                || h.contains("FORGEJO_TOKEN"),
            "{h}"
        );
    }

    // S-13 — the else-empty arm: any non-auth status yields no hint. (kasetto auth.rs:75-83
    // `_ => String::new()`; the oracle exercises this implicitly via the 401/403/404 arms —
    // assert the empty arm directly so the full match is covered.)
    #[test]
    fn http_fetch_auth_hint_is_empty_for_non_auth_status() {
        assert_eq!(http_fetch_auth_hint("https://github.com/org/r", 200), "");
        assert_eq!(http_fetch_auth_hint("https://github.com/org/r", 500), "");
    }

    // S-13 — the 404 arm prepends the "if the repo or file is private" framing before the help.
    // (kasetto auth.rs:78-82.)
    #[test]
    fn http_fetch_auth_hint_404_mentions_private_and_help() {
        let h = http_fetch_auth_hint("https://github.com/org/r", 404);
        assert!(h.contains("private"), "{h}");
        assert!(h.contains("GITHUB_TOKEN") || h.contains("GH_TOKEN"), "{h}");
    }

    // S-12 — per-host-family inline-help vectors. Oracle: the host-family routing asserted by
    // kasetto's `http_fetch_auth_hint_mentions_*` tests (auth.rs:155-184), which delegate to
    // `auth_env_inline_help`; here we assert the help string directly per family.
    #[test]
    fn auth_env_inline_help_github_mentions_github_token() {
        let h = auth_env_inline_help("https://github.com/org/private");
        assert!(h.contains("GITHUB_TOKEN") || h.contains("GH_TOKEN"), "{h}");
    }

    #[test]
    fn auth_env_inline_help_gitlab_mentions_gitlab_token() {
        let h = auth_env_inline_help("https://gitlab.com/group/proj");
        assert!(h.contains("GITLAB_TOKEN"), "{h}");
    }

    #[test]
    fn auth_env_inline_help_bitbucket_mentions_bitbucket_env() {
        let h = auth_env_inline_help("https://bitbucket.org/ws/r");
        assert!(
            h.contains("BITBUCKET_EMAIL") || h.contains("BITBUCKET_USERNAME"),
            "{h}"
        );
    }

    #[test]
    fn auth_env_inline_help_gitea_mentions_gitea_token() {
        let h = auth_env_inline_help("https://codeberg.org/u/r");
        assert!(
            h.contains("CODEBERG_TOKEN")
                || h.contains("GITEA_TOKEN")
                || h.contains("FORGEJO_TOKEN"),
            "{h}"
        );
    }

    // S-12 — the host-less (unparseable URL) arm lists every provider's env var.
    // (kasetto auth.rs:55-71 `None =>` arm.)
    #[test]
    fn auth_env_inline_help_no_host_lists_all_providers() {
        let h = auth_env_inline_help("not-a-url");
        assert!(h.contains("GITHUB_TOKEN"), "{h}");
        assert!(h.contains("GITLAB_TOKEN"), "{h}");
        assert!(
            h.contains("CODEBERG_TOKEN") || h.contains("GITEA_TOKEN"),
            "{h}"
        );
    }

    // --- materialize_source (local, offline) ---

    #[test]
    fn local_materialize_does_not_set_cleanup_dir() {
        let root = temp_dir("agent-env-local-src");
        let skill_dir = root.join("demo-skill");
        fs::create_dir_all(&skill_dir).expect("create dirs");
        fs::write(skill_dir.join("SKILL.md"), "# Demo\n\nDesc\n").expect("write skill");

        let src = SourceSpec {
            source: root.to_string_lossy().to_string(),
            branch: None,
            git_ref: None,
            sub_dir: None,
            skills: SkillsField::Wildcard("*".to_string()),
        };
        let stage = temp_dir("agent-env-stage");
        let materialized =
            materialize_source(&src, Path::new("/"), &stage).expect("materialize local");

        assert_eq!(materialized.source_revision, "local");
        assert!(materialized.cleanup_dir.is_none());
        assert!(materialized.available.contains_key("demo-skill"));
        assert!(root.exists());

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&stage);
    }

    #[test]
    fn local_materialize_supports_sub_dir() {
        let root = temp_dir("agent-env-local-subdir-src");
        let nested = root.join("plugins/swift-apple-expert");
        fs::create_dir_all(&nested).expect("create dirs");
        fs::write(nested.join("SKILL.md"), "# Nested\n\nDesc\n").expect("write skill");

        let src = SourceSpec {
            source: root.to_string_lossy().to_string(),
            branch: None,
            git_ref: None,
            sub_dir: Some("plugins/swift-apple-expert".to_string()),
            skills: SkillsField::Wildcard("*".to_string()),
        };

        let stage = temp_dir("agent-env-stage-subdir");
        let materialized =
            materialize_source(&src, Path::new("/"), &stage).expect("materialize local subdir");

        assert!(materialized.available.contains_key("swift-apple-expert"));
        assert_eq!(
            materialized.available.get("swift-apple-expert").unwrap(),
            &nested
        );

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&stage);
    }

    /// The remote `main → master` fallback and the un-nest of a host tarball's
    /// `<repo>-<ref>/` wrapper both run inside `materialize_source` against a live host.
    /// Networked materialization is `#[ignore]`d to keep the suite offline + deterministic;
    /// the un-nest logic itself is exercised by `resolve_source_root_*` below (which feed it
    /// the already-stripped stage tree that `download_extract` produces).
    #[test]
    #[ignore = "performs a real network fetch (main→master archive download)"]
    fn remote_materialize_main_to_master_fallback() {
        let src = SourceSpec {
            // A repo whose default branch is `master` exercises the fallback arm.
            source: "https://github.com/git/git".to_string(),
            branch: None,
            git_ref: None,
            sub_dir: None,
            skills: SkillsField::Wildcard("*".to_string()),
        };
        let stage = temp_dir("agent-env-remote-stage");
        let materialized =
            materialize_source(&src, Path::new("/"), &stage).expect("materialize remote");
        assert_eq!(materialized.source_revision, "branch:main");
        assert!(materialized.cleanup_dir.is_some());
        let _ = fs::remove_dir_all(&stage);
    }

    // --- resolve_source_root (un-nest + validation) ---

    #[test]
    fn resolve_source_root_returns_base_when_no_subdir() {
        let root = temp_dir("agent-env-srcroot-base");
        fs::create_dir_all(&root).unwrap();
        let resolved = resolve_source_root(&root, None).expect("resolve");
        assert_eq!(resolved, root);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn resolve_source_root_descends_into_subdir() {
        // download_extract strips the host tarball's `<repo>-<ref>/` wrapper, leaving the
        // repo contents at the stage root; a sub-dir then selects a path beneath it.
        let stage = temp_dir("agent-env-srcroot-sub");
        let nested = stage.join("skills/personal");
        fs::create_dir_all(&nested).unwrap();
        let resolved = resolve_source_root(&stage, Some("skills/personal")).expect("resolve");
        assert_eq!(resolved, nested);
        let _ = fs::remove_dir_all(&stage);
    }

    #[test]
    fn resolve_source_root_rejects_escape_and_missing() {
        let stage = temp_dir("agent-env-srcroot-bad");
        fs::create_dir_all(&stage).unwrap();
        assert!(resolve_source_root(&stage, Some("   ")).is_err());
        assert!(resolve_source_root(&stage, Some("/abs")).is_err());
        assert!(resolve_source_root(&stage, Some("../escape")).is_err());
        assert!(resolve_source_root(&stage, Some("does-not-exist")).is_err());
        let _ = fs::remove_dir_all(&stage);
    }

    // --- discover (root-level SKILL.md + sub-dir, wildcard vs named) ---

    #[test]
    fn discover_supports_root_level_skill_with_hint() {
        let root = temp_dir("agent-env-root-skill");
        fs::create_dir_all(&root).expect("create dirs");
        fs::write(root.join("SKILL.md"), "# Root\n\nDesc\n").expect("write skill");

        let available =
            discover_with_root_name(&root, Some("raycast-script-creator")).expect("discover");
        assert!(available.contains_key("raycast-script-creator"));
        assert_eq!(available.get("raycast-script-creator").unwrap(), &root);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn discover_uses_local_directory_name_for_root_level_skill() {
        let root = temp_dir("agent-env-root-skill-local");
        fs::create_dir_all(&root).expect("create dirs");
        fs::write(root.join("SKILL.md"), "# Root\n\nDesc\n").expect("write skill");

        let available = discover(&root).expect("discover");
        let root_name = root.file_name().unwrap().to_string_lossy().to_string();
        assert!(available.contains_key(&root_name));
        assert_eq!(available.get(&root_name).unwrap(), &root);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn discover_finds_skills_in_skills_subdir() {
        let root = temp_dir("agent-env-skills-subdir");
        let a = root.join("skills/alpha");
        let b = root.join("beta");
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        fs::write(a.join("SKILL.md"), "x").unwrap();
        fs::write(b.join("SKILL.md"), "x").unwrap();
        // A directory with no SKILL.md is not a skill.
        fs::create_dir_all(root.join("skills/not-a-skill")).unwrap();

        let available = discover(&root).expect("discover");
        assert!(available.contains_key("alpha"));
        assert!(available.contains_key("beta"));
        assert!(!available.contains_key("not-a-skill"));

        let _ = fs::remove_dir_all(&root);
    }

    // --- discover_mcps ---

    #[test]
    fn discover_mcps_finds_root_dot_mcp_json() {
        let root = temp_dir("agent-env-mcp-root");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join(".mcp.json"),
            r#"{"mcpServers":{"tool":{"command":"x"}}}"#,
        )
        .unwrap();

        let mcps = discover_mcps(&root).unwrap();
        assert_eq!(mcps.len(), 1);
        assert!(mcps[0].ends_with(".mcp.json"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn discover_mcps_finds_root_mcp_json() {
        let root = temp_dir("agent-env-mcp-root2");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("mcp.json"),
            r#"{"mcpServers":{"tool":{"command":"x"}}}"#,
        )
        .unwrap();

        let mcps = discover_mcps(&root).unwrap();
        assert_eq!(mcps.len(), 1);
        assert!(mcps[0].ends_with("mcp.json"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn discover_mcps_finds_mcps_subdir_and_root() {
        let root = temp_dir("agent-env-mcp-both");
        let mcp_dir = root.join("mcps");
        fs::create_dir_all(&mcp_dir).unwrap();
        fs::write(
            root.join(".mcp.json"),
            r#"{"mcpServers":{"a":{"command":"x"}}}"#,
        )
        .unwrap();
        fs::write(
            mcp_dir.join("extra.json"),
            r#"{"mcpServers":{"b":{"command":"y"}}}"#,
        )
        .unwrap();

        let mcps = discover_mcps(&root).unwrap();
        assert_eq!(mcps.len(), 2);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn discover_mcps_returns_empty_when_nothing() {
        let root = temp_dir("agent-env-mcp-empty");
        fs::create_dir_all(&root).unwrap();

        let mcps = discover_mcps(&root).unwrap();
        assert!(mcps.is_empty());

        let _ = fs::remove_dir_all(&root);
    }

    // --- resolve_mcp_entry ---

    #[test]
    fn resolve_mcp_entry_name_looks_in_mcps_dir() {
        let root = temp_dir("agent-env-entry-name");
        let mcps_dir = root.join("mcps");
        fs::create_dir_all(&mcps_dir).unwrap();
        fs::write(
            mcps_dir.join("github.json"),
            r#"{"mcpServers":{"github":{"command":"x"}}}"#,
        )
        .unwrap();

        let entry = McpEntry::Name("github".into());
        let path = resolve_mcp_entry(&root, &entry).unwrap();
        assert!(path.ends_with("mcps/github.json"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn resolve_mcp_entry_name_auto_appends_json() {
        let root = temp_dir("agent-env-entry-ext");
        let mcps_dir = root.join("mcps");
        fs::create_dir_all(&mcps_dir).unwrap();
        fs::write(
            mcps_dir.join("linear.json"),
            r#"{"mcpServers":{"linear":{"command":"x"}}}"#,
        )
        .unwrap();

        // "linear" (no extension) should find "linear.json"
        let entry = McpEntry::Name("linear".into());
        let path = resolve_mcp_entry(&root, &entry).unwrap();
        assert!(path.ends_with("linear.json"));

        // "linear.json" (explicit extension) should also work
        let entry_ext = McpEntry::Name("linear.json".into());
        let path_ext = resolve_mcp_entry(&root, &entry_ext).unwrap();
        assert!(path_ext.ends_with("linear.json"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn resolve_mcp_entry_obj_uses_custom_path() {
        let root = temp_dir("agent-env-entry-obj");
        let tools_dir = root.join("tools");
        fs::create_dir_all(&tools_dir).unwrap();
        fs::write(
            tools_dir.join("my-server.json"),
            r#"{"mcpServers":{"my-server":{"command":"x"}}}"#,
        )
        .unwrap();

        let entry = McpEntry::Obj {
            name: "my-server".into(),
            path: Some("tools".into()),
        };
        let path = resolve_mcp_entry(&root, &entry).unwrap();
        assert!(path.ends_with("tools/my-server.json"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn resolve_mcp_entry_obj_defaults_to_mcps_dir() {
        let root = temp_dir("agent-env-entry-obj-default");
        let mcps_dir = root.join("mcps");
        fs::create_dir_all(&mcps_dir).unwrap();
        fs::write(
            mcps_dir.join("server.json"),
            r#"{"mcpServers":{"server":{"command":"x"}}}"#,
        )
        .unwrap();

        let entry = McpEntry::Obj {
            name: "server".into(),
            path: None,
        };
        let path = resolve_mcp_entry(&root, &entry).unwrap();
        assert!(path.ends_with("mcps/server.json"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn resolve_mcp_entry_missing_is_err() {
        let root = temp_dir("agent-env-entry-missing");
        fs::create_dir_all(&root).unwrap();
        let entry = McpEntry::Name("nope".into());
        assert!(resolve_mcp_entry(&root, &entry).is_err());
        let _ = fs::remove_dir_all(&root);
    }

    // --- discover_commands + resolve_command_entry ---

    #[test]
    fn discover_commands_walks_nested_subdirs() {
        let root = temp_dir("agent-env-cmd-disc");
        let nested = root.join("commands/git/work");
        fs::create_dir_all(&nested).unwrap();
        fs::write(root.join("commands/commit.md"), "---\n---\nbody\n").unwrap();
        fs::write(root.join("commands/git/commit.md"), "x").unwrap();
        fs::write(nested.join("status.md"), "x").unwrap();
        fs::write(root.join("commands/not-md.txt"), "ignored").unwrap();

        let map = discover_commands(&root).unwrap();
        assert_eq!(map.len(), 3);
        assert!(map.contains_key("commit"));
        assert!(map.contains_key("git:commit"));
        assert!(map.contains_key("git:work:status"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn resolve_command_entry_name_uses_discovery() {
        let root = temp_dir("agent-env-cmd-resolve");
        fs::create_dir_all(root.join("commands/git")).unwrap();
        fs::write(root.join("commands/git/commit.md"), "x").unwrap();

        let entry = CommandEntry::Name("git:commit".to_string());
        let (name, path) = resolve_command_entry(&root, &entry).unwrap();
        assert_eq!(name, "git:commit");
        assert!(path.ends_with("commands/git/commit.md"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn resolve_command_entry_obj_with_path() {
        let root = temp_dir("agent-env-cmd-obj");
        fs::create_dir_all(root.join("ops")).unwrap();
        fs::write(root.join("ops/deploy.md"), "x").unwrap();

        let entry = CommandEntry::Obj {
            name: "deploy".to_string(),
            path: Some("ops".to_string()),
        };
        let (name, path) = resolve_command_entry(&root, &entry).unwrap();
        assert_eq!(name, "deploy");
        assert!(path.ends_with("ops/deploy.md"));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn resolve_command_entry_missing_is_err() {
        let root = temp_dir("agent-env-cmd-missing");
        fs::create_dir_all(&root).unwrap();
        let entry = CommandEntry::Name("nope".to_string());
        assert!(resolve_command_entry(&root, &entry).is_err());
        let _ = fs::remove_dir_all(&root);
    }

    // --- XC-02: shared HTTP client (kasetto src/fsops/http.rs::http_client) ---
    //
    // Oracle: kasetto builds a process-wide `OnceLock<Result<Client, String>>`,
    // `connect_timeout(10s)` + `timeout(30s)`, UA `kasetto/{VERSION}` (envctl renames the
    // UA to `envctl-agent-env/{VERSION}`), pure-Rust rustls+ring. The decisive observable
    // contract is **memoization**: the builder closure runs at most once and every call
    // returns a clone of the same underlying client — no per-call TLS/session setup.
    //
    // No network is performed. The timeout/UA values configured on the builder are not
    // introspectable through reqwest's public `Client` API (no getters), so this test
    // asserts what IS observable — construction success + single-build memoization — and
    // NOTES the non-introspectable config here rather than faking a probe of it. (The
    // literal timeout/UA values themselves are pinned by direct source review against the
    // kasetto oracle; reqwest exposes no accessor to assert them at runtime.)

    #[test]
    fn http_client_builds_ok_and_memoizes_via_oncelock() {
        // First call builds (or returns the already-built) client.
        let first = http_client();
        assert!(first.is_ok(), "http_client() must construct successfully");

        // OnceLock identity: after the first call the static is populated, and it can only
        // have been built once — `get_or_init`'s closure never re-runs. This is the
        // observable memoization signal (in-crate access to the private static), with no
        // network round-trip.
        assert!(
            HTTP_CLIENT.get().is_some(),
            "the process-wide OnceLock must be initialized after the first http_client() call"
        );

        // A second call returns Ok without re-building — same memoized client.
        let second = http_client();
        assert!(
            second.is_ok(),
            "the second http_client() call must reuse the memoized client"
        );
    }
}
