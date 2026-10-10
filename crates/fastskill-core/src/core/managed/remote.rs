//! Fetching over `https://`: the credential command, where its token may go, redirects and size
//! limits (ADR-0016 decisions 1, 5, 6 and 15).
//!
//! The token is held only for one command run. It is never stored, logged, printed or looked
//! at, and it is sent only to the managed source's origin. A redirect to another origin drops it,
//! and a redirect to anything but `https://` is refused.
//!
//! Requests are blocking: call this from a plain thread, not from inside an async task.

use super::config::ManagedSource;
use reqwest::blocking::{Client, RequestBuilder, Response};
use reqwest::header::{HeaderValue, AUTHORIZATION};
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long the credential command may run.
pub const CREDENTIAL_TIMEOUT: Duration = Duration::from_secs(30);
/// The most the credential command may print.
pub const MAX_CREDENTIAL_OUTPUT: usize = 16 * 1024;
/// The largest managed state envelope accepted.
pub const MAX_STATE_BYTES: u64 = 8 * 1024 * 1024;
/// The largest skill artifact accepted.
pub const MAX_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;
/// The environment variable that tells the credential command no one can answer a prompt.
pub const INTERACTIVE_ENV: &str = "FASTSKILL_INTERACTIVE";

const MAX_REDIRECTS: usize = 10;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(600);

/// Why a fetch failed. `sign_in` says the user must sign in again: the credential command
/// failed, or the source answered 401.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct RemoteError {
    pub message: String,
    pub sign_in: bool,
}

impl RemoteError {
    fn failed(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            sign_in: false,
        }
    }

    fn sign_in(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            sign_in: true,
        }
    }
}

impl From<RemoteError> for crate::core::service::ServiceError {
    fn from(error: RemoteError) -> Self {
        Self::InvalidOperation(error.message)
    }
}

/// A token from the credential command. Its `Debug` form never shows it.
#[derive(Clone)]
pub struct Token(HeaderValue);

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(..)")
    }
}

/// Run the credential command and take the first line it prints as the token.
///
/// The command runs without a shell, from `working_dir`, for at most [`CREDENTIAL_TIMEOUT`]. Its
/// standard error reaches the terminal when `interactive`, so it can prompt; otherwise it's
/// discarded and [`INTERACTIVE_ENV`] is set to `0`. Errors never include what it printed.
pub fn run_credential_command(
    command: &[String],
    working_dir: &Path,
    interactive: bool,
) -> Result<Token, RemoteError> {
    let Some((program, args)) = command.split_first() else {
        return Err(RemoteError::sign_in("the credential command is empty"));
    };
    let mut process = Command::new(program);
    process.args(args).stdout(Stdio::piped());
    if working_dir.is_dir() {
        process.current_dir(working_dir);
    } else {
        process.current_dir(std::env::temp_dir());
    }
    if interactive {
        process.stdin(Stdio::inherit()).stderr(Stdio::inherit());
    } else {
        process
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .env(INTERACTIVE_ENV, "0");
    }
    let mut child = process.spawn().map_err(|error| {
        RemoteError::sign_in(format!(
            "the credential command {program:?} couldn't start: {error}"
        ))
    })?;
    let stdout = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut output = Vec::new();
        if let Some(stdout) = stdout {
            // One byte over the limit is enough to know it's too much.
            let _ = stdout
                .take(MAX_CREDENTIAL_OUTPUT as u64 + 1)
                .read_to_end(&mut output);
        }
        output
    });

    let deadline = Instant::now() + CREDENTIAL_TIMEOUT;
    let mut reader = Some(reader);
    let mut output = None;
    let status = loop {
        if reader.as_ref().is_some_and(|reader| reader.is_finished()) {
            let read = reader.take().and_then(|reader| reader.join().ok());
            if read
                .as_ref()
                .is_some_and(|read| read.len() > MAX_CREDENTIAL_OUTPUT)
            {
                let _ = child.kill();
                let _ = child.wait();
                return Err(too_much_output());
            }
            output = read;
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(RemoteError::sign_in(format!(
                    "the credential command didn't finish within {} seconds",
                    CREDENTIAL_TIMEOUT.as_secs()
                )));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => {
                let _ = child.kill();
                return Err(RemoteError::sign_in(format!(
                    "the credential command couldn't be waited for: {error}"
                )));
            }
        }
    };
    let output = match reader {
        Some(reader) => reader.join().unwrap_or_default(),
        None => output.unwrap_or_default(),
    };
    if !status.success() {
        return Err(RemoteError::sign_in(format!(
            "the credential command exited with {status}"
        )));
    }
    if output.len() > MAX_CREDENTIAL_OUTPUT {
        return Err(too_much_output());
    }
    let first = output.split(|byte| *byte == b'\n').next().unwrap_or(&[]);
    let first = first.strip_suffix(b"\r").unwrap_or(first);
    if first.is_empty() {
        return Err(RemoteError::sign_in(
            "the credential command printed no token",
        ));
    }
    let mut value = std::str::from_utf8(first)
        .ok()
        .and_then(|token| HeaderValue::from_str(&format!("Bearer {token}")).ok())
        .ok_or_else(|| {
            RemoteError::sign_in("the credential command printed a line that can't be a token")
        })?;
    value.set_sensitive(true);
    Ok(Token(value))
}

fn too_much_output() -> RemoteError {
    RemoteError::sign_in(format!(
        "the credential command printed more than {} KiB",
        MAX_CREDENTIAL_OUTPUT / 1024
    ))
}

/// An https client for one command run, holding the token for the source's origin.
#[derive(Debug)]
pub struct Remote {
    client: Client,
    source: ManagedSource,
    token: Option<Token>,
}

impl Remote {
    /// A client for `source`. `token` goes only to the source's origin.
    pub fn new(source: &ManagedSource, token: Option<Token>) -> Result<Self, RemoteError> {
        let policy = reqwest::redirect::Policy::custom(|attempt| {
            if attempt.url().scheme() != "https" {
                let message = format!(
                    "refused a redirect to {}: only https:// is followed",
                    attempt.url()
                );
                attempt.error(message)
            } else if attempt.previous().len() >= MAX_REDIRECTS {
                attempt.error("too many redirects")
            } else {
                // reqwest drops Authorization when the scheme, host or port changes.
                attempt.follow()
            }
        });
        let builder = Client::builder()
            .redirect(policy)
            .https_only(true)
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .user_agent(concat!("fastskill/", env!("CARGO_PKG_VERSION")));
        let client = test_roots::add(builder)
            .build()
            .map_err(|error| RemoteError::failed(format!("can't set up https: {error}")))?;
        Ok(Self {
            client,
            source: source.clone(),
            token,
        })
    }

    /// Whether a token is held.
    pub fn has_token(&self) -> bool {
        self.token.is_some()
    }

    /// The source's envelope.
    pub fn get_state(&self) -> Result<Vec<u8>, RemoteError> {
        let ManagedSource::Https(url) = &self.source else {
            return Err(RemoteError::failed("the managed source isn't https://"));
        };
        let response = self.send(
            self.request(reqwest::Method::GET, url.as_str()),
            "the managed state",
        )?;
        let mut bytes = Vec::new();
        read_limited(response, MAX_STATE_BYTES, &mut bytes, "the managed state")?;
        Ok(bytes)
    }

    /// Download `url` into the file at `into`.
    pub fn download(&self, url: &str, into: &Path) -> Result<(), RemoteError> {
        let what = format!("the artifact {url}");
        let response = self.send(self.request(reqwest::Method::GET, url), &what)?;
        let mut file = std::fs::File::create(into)
            .map_err(|error| RemoteError::failed(format!("can't write {what}: {error}")))?;
        read_limited(response, MAX_ARTIFACT_BYTES, &mut file, &what)
    }

    /// POST a JSON body to `url`; `Ok` when the source accepted it.
    pub fn post_json(&self, url: &str, body: Vec<u8>) -> Result<(), RemoteError> {
        let request = self
            .request(reqwest::Method::POST, url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body);
        self.send(request, "the report").map(drop)
    }

    fn request(&self, method: reqwest::Method, url: &str) -> RequestBuilder {
        let request = self.client.request(method, url);
        match &self.token {
            Some(Token(value)) if self.source.same_origin(url) => {
                request.header(AUTHORIZATION, value.clone())
            }
            _ => request,
        }
    }

    fn send(&self, request: RequestBuilder, what: &str) -> Result<Response, RemoteError> {
        let response = request.send().map_err(|error| {
            RemoteError::failed(format!("can't fetch {what}: {}", chain(&error)))
        })?;
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let message = format!("fetching {what} failed with HTTP {status}");
        if status == reqwest::StatusCode::UNAUTHORIZED {
            Err(RemoteError::sign_in(message))
        } else {
            Err(RemoteError::failed(message))
        }
    }
}

/// Copy at most `limit` bytes of `response` into `into`, refusing anything longer.
fn read_limited(
    response: Response,
    limit: u64,
    into: &mut impl std::io::Write,
    what: &str,
) -> Result<(), RemoteError> {
    if response
        .content_length()
        .is_some_and(|length| length > limit)
    {
        return Err(too_large(what, limit));
    }
    let copied = std::io::copy(&mut response.take(limit + 1), into)
        .map_err(|error| RemoteError::failed(format!("can't read {what}: {error}")))?;
    if copied > limit {
        return Err(too_large(what, limit));
    }
    Ok(())
}

fn too_large(what: &str, limit: u64) -> RemoteError {
    RemoteError::failed(format!(
        "{what} is larger than {} MiB",
        limit / (1024 * 1024)
    ))
}

/// An error and its causes, which is where reqwest puts the useful part.
fn chain(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut cause = error.source();
    while let Some(next) = cause {
        text.push_str(": ");
        text.push_str(&next.to_string());
        cause = next.source();
    }
    text
}

/// Extra trusted roots, which only tests set, for their local https servers.
mod test_roots {
    use reqwest::blocking::ClientBuilder;

    #[cfg(not(test))]
    pub fn add(builder: ClientBuilder) -> ClientBuilder {
        builder
    }

    #[cfg(test)]
    thread_local! {
        static ROOT: std::cell::RefCell<Option<reqwest::Certificate>> =
            const { std::cell::RefCell::new(None) };
    }

    #[cfg(test)]
    pub fn add(builder: ClientBuilder) -> ClientBuilder {
        match ROOT.with(|root| root.borrow().clone()) {
            Some(root) => builder.add_root_certificate(root),
            None => builder,
        }
    }

    /// Trust `root` for clients built on this thread.
    #[cfg(test)]
    pub fn trust(root: reqwest::Certificate) {
        ROOT.with(|slot| *slot.borrow_mut() = Some(root));
    }
}

#[cfg(test)]
pub(crate) use test_roots::trust as trust_test_root;

#[cfg(test)]
#[path = "remote_tests.rs"]
mod tests;
