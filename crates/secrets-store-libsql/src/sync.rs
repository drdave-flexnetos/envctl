//! Async-to-sync bridge. The engine's [`envctl_secrets::vault::Store`] trait is fully sync
//! (`&self` methods returning `anyhow::Result`); the libSQL remote client is async. This module
//! owns a PRIVATE current-thread tokio runtime and adapts every async libSQL call to a blocking
//! one via `Runtime::block_on`.
//!
//! ## Why a private current-thread runtime
//!
//! The runtime is `Arc`-wrapped and reused for the life of one store instance (one connection).
//! A `new_current_thread` runtime is intentional: the Store is called from the daemon's blocking
//! worker context (never from inside an outer tokio worker — calling `block_on` from a tokio worker
//! thread panics), and a single-threaded reactor is sufficient to drive one HTTP/Hrana connection.
//! Keeping the runtime private to this crate is what lets the engine stay completely async-free.
//!
//! ## Reconnect-on-stream-expiry (HF: Hrana idle timeout)
//!
//! A libSQL `remote` connection holds a Hrana stream baton that sqld EXPIRES after a short idle
//! window. The engine interleaves slow CPU work between store ops — most notably argon2id during
//! `init_vault` (seconds to tens of seconds), which sits between a store read and the first
//! `save_keyslot` write. If the baton expires in that gap the next statement fails — sqld surfaces
//! this as either `STREAM_EXPIRED` ("the stream has expired due to inactivity") OR, for a baton
//! gone stale across a long idle window or an advanced DB generation, a 400 `Received an invalid
//! baton`. So every primitive runs through [`SyncConnection::run_retry`]: on EITHER shape (see
//! [`is_stream_expired`]) it reconnects ONCE (a fresh `db.connect()`) and retries. The retried
//! statements are idempotent (`INSERT OR REPLACE` / reads) and a stream fault means the prior
//! attempt never committed, so the retry is safe.

use std::sync::{Arc, Mutex};

use tokio::runtime::Runtime;

use crate::error::{Error, Result};

/// A sync wrapper over one libSQL [`libsql::Connection`] plus the private runtime that drives it and
/// the [`libsql::Database`] handle needed to re-`connect()` after a Hrana stream-expiry. Cloneable:
/// the `Arc`s are shared, so the engine can hold the store behind an `Arc` and every method reuses
/// the one reactor + (reconnectable) connection.
#[derive(Clone)]
pub struct SyncConnection {
    rt: Arc<Runtime>,
    db: Arc<libsql::Database>,
    conn: Arc<Mutex<libsql::Connection>>,
}

/// True if `e` is a recoverable Hrana stream/baton fault — one where the fix is to reconnect (a
/// fresh `db.connect()`) and retry, because the prior statement never committed.
///
/// sqld surfaces TWO distinct shapes of "your Hrana session is gone", BOTH idle/staleness-driven
/// and BOTH recoverable by reconnecting:
///   * `STREAM_EXPIRED` — "The stream has expired due to inactivity" (the idle-baton timeout).
///   * `Received an invalid baton` (HTTP 400) — sqld rejects a stale/advanced/unknown baton when a
///     long-idle connection makes its first request, or after the DB generation advanced under a
///     concurrent writer. Observed on `secretctl unlock` after the daemon sat locked (idle) for a
///     long window (2026-06-13). This shape was previously UNMATCHED, so `run_retry` did not
///     reconnect and the 400 surfaced to the caller as an unlock failure.
///
/// We match case-insensitively on the codes/prose so a libSQL formatting tweak is less likely to
/// silently disable reconnect; see the unit test below. Genuine, non-session errors (UNIQUE
/// violations, connection-refused) must NOT match — they are not retryable.
fn is_stream_expired(e: &Error) -> bool {
    let s = e.to_string().to_ascii_lowercase();
    s.contains("stream_expired")
        || s.contains("stream has expired")
        || s.contains("stream expired")
        || s.contains("invalid baton")
        || s.contains("baton not found")
        || s.contains("stream not found")
}

impl SyncConnection {
    /// Build the private current-thread runtime, open the remote database, and connect. All async
    /// work happens on the runtime we own here.
    ///
    /// ## Why a custom (plaintext) connector
    ///
    /// libSQL's `remote` feature ships NO HTTP connector unless its `tls` feature is also enabled —
    /// and `tls` pulls `hyper-rustls 0.25 -> rustls 0.22`, a SECOND rustls major alongside the
    /// workspace's single ring-only `rustls 0.23` (breaking the no-C / single-rustls gate, and there
    /// is no hyper-0.14 hyper-rustls on rustls 0.23). So we supply our own connector. `Database::
    /// open_remote_with_connector` with a bare `hyper::client::HttpConnector` is exactly what libSQL
    /// uses for its own no-TLS path; it is **plaintext**, so the URL MUST be loopback (the daemon's
    /// `config` enforces this). For a REMOTE sqld, front it with a loopback TLS terminator (stunnel /
    /// spiped / cloudflared) and point at `http://127.0.0.1:<local-port>` — that keeps the daemon's
    /// dependency graph gate-clean. (`HttpConnector` also `enforce_http`s, rejecting `https://` URIs
    /// as defense-in-depth.)
    pub fn open_remote(url: &str, auth_token: &str) -> Result<Self> {
        // A PRIVATE current-thread runtime (no `rt-multi-thread` feature, no worker pool). One
        // reactor drives one HTTP/Hrana connection; `enable_all` turns on the I/O + time drivers
        // the libSQL client needs.
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| Error::RuntimeCreation(e.to_string()))?;
        let (db, conn) = rt.block_on(async {
            // Plaintext loopback connector (see the doc comment above for why not libSQL's `tls`).
            #[allow(deprecated)]
            // open_remote_with_connector is the documented custom-connector entry
            let db = libsql::Database::open_remote_with_connector(
                url.to_string(),
                auth_token.to_string(),
                hyper::client::HttpConnector::new(),
            )
            .map_err(|e| Error::Connect(e.to_string()))?;
            let conn = db.connect().map_err(|e| Error::Connect(e.to_string()))?;
            Ok::<_, Error>((db, conn))
        })?;
        Ok(Self {
            rt: Arc::new(rt),
            db: Arc::new(db),
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Borrow the underlying runtime handle (used by `store` for multi-statement `block_on`s).
    pub fn runtime(&self) -> &Arc<Runtime> {
        &self.rt
    }

    /// A fresh handle to the current connection (a cheap clone of the libSQL connection handle).
    /// Used inside runtime-driven async blocks in `store` (e.g. the transaction methods).
    pub fn conn(&self) -> libsql::Connection {
        self.conn.lock().expect("conn lock poisoned").clone()
    }

    /// Re-establish the connection after a Hrana stream-expiry (a fresh `db.connect()`).
    fn reconnect(&self) -> Result<()> {
        let fresh = self
            .db
            .connect()
            .map_err(|e| Error::Connect(e.to_string()))?;
        *self.conn.lock().expect("conn lock poisoned") = fresh;
        Ok(())
    }

    /// Run `f` with a fresh connection handle; on a Hrana stream-expiry, reconnect ONCE and retry.
    /// `f` MUST be re-runnable (it receives a fresh `Connection` + the runtime each attempt and must
    /// not consume captured state). All store I/O routes through here so a long argon2 gap before a
    /// write (init_vault) can never surface a `STREAM_EXPIRED` to the engine.
    pub fn run_retry<T>(&self, f: impl Fn(libsql::Connection, &Runtime) -> Result<T>) -> Result<T> {
        match f(self.conn(), &self.rt) {
            Err(e) if is_stream_expired(&e) => {
                self.reconnect()?;
                f(self.conn(), &self.rt)
            }
            other => other,
        }
    }

    /// Run a `SELECT` and return the FIRST row (or `None`). Parameterized only — `params` is a
    /// `Vec<libsql::Value>` bound positionally to the `?` placeholders in `sql`.
    pub fn query_one(&self, sql: &str, params: Vec<libsql::Value>) -> Result<Option<libsql::Row>> {
        self.run_retry(|conn, rt| {
            rt.block_on(async {
                let mut rows = conn
                    .query(sql, params.clone())
                    .await
                    .map_err(|e| Error::QueryFailed(e.to_string()))?;
                rows.next()
                    .await
                    .map_err(|e| Error::QueryFailed(e.to_string()))
            })
        })
    }

    /// Run a `SELECT` and collect ALL rows.
    pub fn query_all(&self, sql: &str, params: Vec<libsql::Value>) -> Result<Vec<libsql::Row>> {
        self.run_retry(|conn, rt| {
            rt.block_on(async {
                let mut rows = conn
                    .query(sql, params.clone())
                    .await
                    .map_err(|e| Error::QueryFailed(e.to_string()))?;
                let mut out = Vec::new();
                while let Some(r) = rows
                    .next()
                    .await
                    .map_err(|e| Error::QueryFailed(e.to_string()))?
                {
                    out.push(r);
                }
                Ok(out)
            })
        })
    }

    /// Run an `INSERT`/`UPDATE`/`DELETE` and return the affected-row count.
    pub fn execute(&self, sql: &str, params: Vec<libsql::Value>) -> Result<u64> {
        self.run_retry(|conn, rt| {
            rt.block_on(async {
                conn.execute(sql, params.clone())
                    .await
                    .map_err(|e| Error::ExecuteFailed(e.to_string()))
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::is_stream_expired;
    use crate::error::Error;

    #[test]
    fn detects_hrana_stream_expiry() {
        // The real shape from libsql 0.9.30: a Hrana error carrying the server `code` STREAM_EXPIRED.
        let real = Error::ExecuteFailed(
            "Hrana: `api error: `status=400, body={\"message\":\"The stream has expired due to \
             inactivity\",\"code\":\"STREAM_EXPIRED\"}``"
                .into(),
        );
        assert!(
            is_stream_expired(&real),
            "must match the real STREAM_EXPIRED error"
        );
        // Case-insensitive + the prose-only variant (defends against a libSQL Display tweak).
        assert!(is_stream_expired(&Error::QueryFailed(
            "The Stream Has Expired".into()
        )));
        assert!(is_stream_expired(&Error::ExecuteFailed(
            "stream_expired".into()
        )));
        // The "invalid baton" shape — the exact error seen on `secretctl unlock` after a long idle
        // window (2026-06-13). Must reconnect+retry, not surface a 400.
        assert!(
            is_stream_expired(&Error::QueryFailed(
                "Hrana: `api error: `status=400 Bad Request, body=Received an invalid baton``"
                    .into()
            )),
            "the `invalid baton` 400 must trigger a reconnect"
        );
        // Sibling baton/stream-gone shapes are also recoverable by reconnecting.
        assert!(is_stream_expired(&Error::QueryFailed(
            "baton not found".into()
        )));
        assert!(is_stream_expired(&Error::QueryFailed(
            "stream not found".into()
        )));
        // Unrelated errors must NOT trigger a reconnect (no spurious retry of a genuine failure).
        assert!(!is_stream_expired(&Error::ExecuteFailed(
            "UNIQUE constraint failed: secrets.row_id".into()
        )));
        assert!(!is_stream_expired(&Error::Connect(
            "connection refused".into()
        )));
    }
}
