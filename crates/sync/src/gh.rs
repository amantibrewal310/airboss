//! `gh`: finding it, its accounts, the batched GraphQL read, and what its answers mean.
//!
//! Owner, repo and number only ever go in as variables, never into the query text. A token
//! only ever reaches a child's environment (`GH_TOKEN`); it's never in argv, a log, an error
//! or a table.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

use airboss_core::PrState;
use airboss_services::time::{from_ts, to_ts};
use airboss_services::{Checks, PrRef, Review, Timestamp};
use serde_json::Value;

use crate::secret::Secret;

/// How long one `gh` process may run.
pub const GH_TIMEOUT: Duration = Duration::from_secs(60);
/// At most this many PRs per GraphQL call (aliases `p0`…`p49`).
pub const BATCH: usize = 50;
/// Below this many points left, an account waits for its reset.
pub const LOW_BUDGET: u64 = 200;
/// How long a secondary rate limit holds an account.
pub const SECONDARY_WAIT: Duration = Duration::from_secs(15 * 60);

/// One variable of a GraphQL call: `-f name=value` or `-F name=number`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GhVar {
    Str(String),
    Int(u64),
}

/// What `sync` asks `gh` to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GhCall {
    /// `gh auth status --json hosts`: always exits 0, and holds no tokens.
    AuthStatus,
    /// `gh api graphql -f query=… -f o0=… -F n0=…`.
    Graphql {
        query: String,
        vars: Vec<(String, GhVar)>,
    },
}

/// A finished `gh` process.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CmdOutput {
    /// `None` when it was killed (a timeout).
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

/// Runs `gh`. `NotFound` from either call means there's no `gh` binary.
pub trait GhRunner: Send {
    fn run(
        &mut self,
        call: &GhCall,
        token: Option<&Secret>,
        timeout: Duration,
    ) -> io::Result<CmdOutput>;
    /// `gh auth token --user <login>`.
    fn token(&mut self, login: &str) -> io::Result<Secret>;
}

/// `gh`'s arguments for a call. No token is ever among them.
pub fn args(call: &GhCall) -> Vec<String> {
    match call {
        GhCall::AuthStatus => ["auth", "status", "--json", "hosts"]
            .map(String::from)
            .to_vec(),
        GhCall::Graphql { query, vars } => {
            let mut out = vec![
                "api".into(),
                "graphql".into(),
                "-f".into(),
                format!("query={query}"),
            ];
            for (name, v) in vars {
                match v {
                    GhVar::Str(s) => out.extend(["-f".into(), format!("{name}={s}")]),
                    GhVar::Int(n) => out.extend(["-F".into(), format!("{name}={n}")]),
                }
            }
            out
        }
    }
}

/// `gh auth token --user <login>`'s arguments.
pub fn token_args(login: &str) -> Vec<String> {
    vec!["auth".into(), "token".into(), "--user".into(), login.into()]
}

/// Variables removed from every child's environment.
pub const ENV_REMOVED: &[&str] = &["GH_TOKEN", "GITHUB_TOKEN"];

/// Variables set in every child's environment, plus `GH_TOKEN` for a non-active account.
pub fn child_env(token: Option<&Secret>) -> Vec<(&'static str, String)> {
    let mut env = vec![
        ("GH_PROMPT_DISABLED", "1".to_string()),
        ("GH_NO_UPDATE_NOTIFIER", "1".to_string()),
        ("GH_SPINNER_DISABLED", "1".to_string()),
        ("NO_COLOR", "1".to_string()),
    ];
    if let Some(t) = token {
        env.push(("GH_TOKEN", t.expose().to_string()));
    }
    env
}

/// `explicit`, else the first of `fixed` that exists, else the first `gh` on `path_env`.
/// A Mac app started from Finder has a short `PATH` without Homebrew, hence `fixed`.
pub fn find_gh(
    explicit: Option<&Path>,
    fixed: &[&Path],
    path_env: Option<&OsStr>,
) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return p.is_file().then(|| p.to_path_buf());
    }
    fixed
        .iter()
        .map(|p| p.to_path_buf())
        .chain(
            path_env
                .into_iter()
                .flat_map(std::env::split_paths)
                .map(|d| d.join("gh")),
        )
        .find(|p| p.is_file())
}

/// Where `gh` usually is on a Mac.
pub const GH_PLACES: &[&str] = &["/opt/homebrew/bin/gh", "/usr/local/bin/gh"];

/// One github.com account `gh` knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub login: String,
    pub active: bool,
    /// Its `state` is `success`.
    pub ok: bool,
}

/// `gh auth status --json hosts`: the github.com accounts, active first.
pub fn parse_accounts(json: &str) -> Result<Vec<Account>, String> {
    let v: Value = serde_json::from_str(json).map_err(|e| format!("gh auth status: {e}"))?;
    let entries = v
        .get("hosts")
        .ok_or("gh auth status has no hosts")?
        .get("github.com")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut out: Vec<Account> = entries
        .iter()
        .filter_map(|e| {
            Some(Account {
                login: e.get("login")?.as_str()?.to_string(),
                active: e.get("active").and_then(Value::as_bool).unwrap_or(false),
                ok: e.get("state").and_then(Value::as_str) == Some("success"),
            })
        })
        .collect();
    out.sort_by_key(|a| !a.active);
    Ok(out)
}

const FRAGMENT: &str = "fragment pr on PullRequest {
  number url title state isDraft merged mergedAt closedAt updatedAt reviewDecision
  additions deletions headRefName repository { nameWithOwner } author { login }
  commits(last: 1) { nodes { commit { statusCheckRollup { state } } } }
}";

/// One batched read: up to [`BATCH`] PRs, and the account's own open PRs when `viewer`.
pub fn query(prs: &[PrRef], viewer: bool) -> GhCall {
    let mut decls = Vec::new();
    let mut body = vec!["  rateLimit { cost remaining resetAt }".to_string()];
    if viewer {
        body.push(
            "  viewer { login pullRequests(states: OPEN, first: 50, orderBy: {field: UPDATED_AT, direction: DESC}) { nodes { ...pr } } }"
                .into(),
        );
    }
    let mut vars = Vec::new();
    for (i, pr) in prs.iter().enumerate() {
        let (owner, repo, n) = pr.parts();
        decls.push(format!("$o{i}: String!, $r{i}: String!, $n{i}: Int!"));
        body.push(format!(
            "  p{i}: repository(owner: $o{i}, name: $r{i}) {{ pullRequest(number: $n{i}) {{ ...pr }} }}"
        ));
        vars.push((format!("o{i}"), GhVar::Str(owner.into())));
        vars.push((format!("r{i}"), GhVar::Str(repo.into())));
        vars.push((format!("n{i}"), GhVar::Int(n)));
    }
    let head = if decls.is_empty() {
        "query {".to_string()
    } else {
        format!("query({}) {{", decls.join(", "))
    };
    let uses_fragment = viewer || !prs.is_empty();
    let query = format!(
        "{head}\n{}\n}}{}",
        body.join("\n"),
        if uses_fragment {
            format!("\n{FRAGMENT}")
        } else {
            String::new()
        }
    );
    GhCall::Graphql { query, vars }
}

/// One PR as GitHub reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrData {
    pub pr: PrRef,
    pub url: Option<String>,
    pub title: Option<String>,
    pub state: PrState,
    pub draft: bool,
    pub review: Review,
    pub checks: Checks,
    pub additions: Option<u32>,
    pub deletions: Option<u32>,
    pub head_ref: Option<String>,
    pub author: Option<String>,
    pub updated_at: Option<Timestamp>,
    pub merged_at: Option<Timestamp>,
}

/// What GitHub said about one requested PR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrResult {
    Found(PrData),
    /// `NOT_FOUND` at `pN`: this account can't see the repo. Try the next account.
    NoAccess,
    /// `NOT_FOUND` at `pN.pullRequest`: the repo has no such PR.
    NotFound,
    Failed(String),
}

/// `rateLimit` from an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Budget {
    pub cost: u64,
    pub remaining: u64,
    pub reset_at: Option<SystemTime>,
}

/// What one `gh api graphql` call came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GhOutcome {
    Data {
        budget: Option<Budget>,
        /// The account's own open PRs, when asked for.
        viewer: Option<Vec<PrData>>,
        /// One per requested PR, in order.
        results: Vec<(PrRef, PrResult)>,
    },
    /// Exit 4: this account is signed out.
    SignedOut,
    /// GitHub's secondary rate limit.
    SecondaryLimit,
    /// Anything else, with stderr's first line.
    Failed(String),
}

fn text(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_string)
}

fn gh_time(v: &Value, k: &str) -> Option<Timestamp> {
    v.get(k)
        .and_then(Value::as_str)
        .and_then(from_ts)
        .map(to_ts)
}

/// One `...pr` fragment.
pub fn parse_pr(v: &Value) -> Option<PrData> {
    let number = v.get("number")?.as_u64()?;
    let repo = v.get("repository")?.get("nameWithOwner")?.as_str()?;
    let merged = v.get("merged").and_then(Value::as_bool).unwrap_or(false);
    let state = match (v.get("state")?.as_str()?, merged) {
        (_, true) | ("MERGED", _) => PrState::Merged,
        ("OPEN", _) => PrState::Open,
        ("CLOSED", _) => PrState::Closed,
        _ => return None,
    };
    let review = match v.get("reviewDecision").and_then(Value::as_str) {
        Some("APPROVED") => Review::Approved,
        Some("CHANGES_REQUESTED") => Review::ChangesRequested,
        Some("REVIEW_REQUIRED") => Review::Required,
        _ => Review::None,
    };
    let rollup = v
        .pointer("/commits/nodes/0/commit/statusCheckRollup/state")
        .and_then(Value::as_str);
    let checks = match rollup {
        Some("SUCCESS") => Checks::Passing,
        Some("FAILURE" | "ERROR") => Checks::Failing,
        Some("PENDING" | "EXPECTED") => Checks::Pending,
        _ => Checks::None,
    };
    let count = |k: &str| {
        v.get(k)
            .and_then(Value::as_u64)
            .and_then(|n| u32::try_from(n).ok())
    };
    Some(PrData {
        pr: PrRef::new(repo, number)?,
        url: text(v, "url"),
        title: text(v, "title"),
        state,
        draft: v.get("isDraft").and_then(Value::as_bool).unwrap_or(false),
        review,
        checks,
        additions: count("additions"),
        deletions: count("deletions"),
        head_ref: text(v, "headRefName"),
        author: v.get("author").and_then(|a| text(a, "login")),
        updated_at: gh_time(v, "updatedAt"),
        merged_at: gh_time(v, "mergedAt"),
    })
}

fn first_line(s: &str) -> String {
    s.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or_default()
        .to_string()
}

/// What a call's output means. `prs` are the PRs asked for, in alias order.
pub fn parse_answer(out: &CmdOutput, prs: &[PrRef]) -> GhOutcome {
    if out.status == Some(4) {
        return GhOutcome::SignedOut;
    }
    if out
        .stderr
        .to_ascii_lowercase()
        .contains("secondary rate limit")
    {
        return GhOutcome::SecondaryLimit;
    }
    let json: Option<Value> = serde_json::from_str(&out.stdout).ok();
    let data = json
        .as_ref()
        .and_then(|j| j.get("data"))
        .filter(|d| d.is_object());
    let Some(data) = data.filter(|_| matches!(out.status, Some(0 | 1))) else {
        let why = match (out.status, first_line(&out.stderr)) {
            (None, _) => "gh took longer than 60 s".to_string(),
            (_, line) if !line.is_empty() => line,
            (Some(code), _) => format!("gh exited with {code}"),
        };
        return GhOutcome::Failed(why);
    };
    // Errors by alias: where on the path they point says what's missing.
    let mut errors: HashMap<String, (usize, String, String)> = HashMap::new();
    for e in json
        .as_ref()
        .and_then(|j| j.get("errors"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let path: Vec<&str> = e
            .get("path")
            .and_then(Value::as_array)
            .map(|p| p.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        if let Some(alias) = path.first() {
            errors.insert(
                alias.to_string(),
                (
                    path.len(),
                    text(e, "type").unwrap_or_default(),
                    text(e, "message").unwrap_or_default(),
                ),
            );
        }
    }
    let results = prs
        .iter()
        .enumerate()
        .map(|(i, pr)| {
            let alias = format!("p{i}");
            let node = data
                .get(&alias)
                .and_then(|r| r.get("pullRequest"))
                .filter(|n| n.is_object());
            let result = match (node.and_then(parse_pr), errors.get(&alias)) {
                (Some(found), _) => PrResult::Found(found),
                (None, Some((1, kind, _))) if kind == "NOT_FOUND" || kind == "FORBIDDEN" => {
                    PrResult::NoAccess
                }
                (None, Some((_, kind, _))) if kind == "NOT_FOUND" => PrResult::NotFound,
                (None, Some((_, _, message))) => PrResult::Failed(message.clone()),
                (None, None) => match data.get(&alias) {
                    Some(r) if r.get("pullRequest").is_some_and(Value::is_null) => {
                        PrResult::NotFound
                    }
                    _ => PrResult::Failed("GitHub sent no answer for this PR".into()),
                },
            };
            (pr.clone(), result)
        })
        .collect();
    let budget = data.get("rateLimit").map(|r| Budget {
        cost: r.get("cost").and_then(Value::as_u64).unwrap_or(0),
        remaining: r
            .get("remaining")
            .and_then(Value::as_u64)
            .unwrap_or(u64::MAX),
        reset_at: r.get("resetAt").and_then(Value::as_str).and_then(from_ts),
    });
    let viewer = data.get("viewer").filter(|v| v.is_object()).map(|v| {
        v.pointer("/pullRequests/nodes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(parse_pr)
            .collect()
    });
    GhOutcome::Data {
        budget,
        viewer,
        results,
    }
}

/// Why the `gh` client can't read anything at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GhOff {
    /// No `gh` binary.
    Missing,
    /// No github.com account signed in.
    SignedOut,
    /// `gh auth status` itself failed.
    Failed(String),
}

/// The runner, the accounts and their tokens. Shared by the GitHub worker and `lookup_pr`
/// behind one lock, so only one `gh` process runs at a time.
pub struct GhClient {
    runner: Box<dyn GhRunner>,
    accounts: Vec<Account>,
    tokens: HashMap<String, Secret>,
    read_at: Option<SystemTime>,
}

impl GhClient {
    pub fn new(runner: Box<dyn GhRunner>) -> GhClient {
        GhClient {
            runner,
            accounts: Vec::new(),
            tokens: HashMap::new(),
            read_at: None,
        }
    }

    /// The accounts last read, active first.
    pub fn accounts(&self) -> &[Account] {
        &self.accounts
    }

    /// Whether any account is signed in, as of the last read.
    pub fn signed_in(&self) -> bool {
        self.accounts.iter().any(|a| a.ok)
    }

    /// When the accounts were last read.
    pub fn read_at(&self) -> Option<SystemTime> {
        self.read_at
    }

    /// Reads the accounts again. Tokens of accounts that are gone are dropped.
    pub fn read_accounts(&mut self, now: SystemTime) -> Result<(), GhOff> {
        self.read_at = Some(now);
        let out = match self.runner.run(&GhCall::AuthStatus, None, GH_TIMEOUT) {
            Ok(out) => out,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                self.accounts.clear();
                return Err(GhOff::Missing);
            }
            Err(e) => return Err(GhOff::Failed(format!("gh couldn't run: {e}"))),
        };
        let accounts = parse_accounts(&out.stdout).map_err(GhOff::Failed)?;
        self.tokens
            .retain(|login, _| accounts.iter().any(|a| &a.login == login));
        self.accounts = accounts;
        // No account in `success` state is the same as none at all.
        if !self.accounts.iter().any(|a| a.ok) {
            return Err(GhOff::SignedOut);
        }
        Ok(())
    }

    /// Forgets an account's token, so it's fetched again (after an auth error).
    pub fn forget_token(&mut self, login: &str) {
        self.tokens.remove(login);
    }

    /// One GraphQL read as `login`: the active account uses `gh`'s own setup, any other its
    /// token from `gh auth token --user`.
    pub fn read(&mut self, login: &str, prs: &[PrRef], viewer: bool) -> GhOutcome {
        let active = self.accounts.iter().any(|a| a.login == login && a.active);
        let token = if active {
            None
        } else {
            if !self.tokens.contains_key(login) {
                match self.runner.token(login) {
                    Ok(t) if !t.expose().is_empty() => {
                        self.tokens.insert(login.to_string(), t);
                    }
                    Ok(_) => return GhOutcome::SignedOut,
                    Err(e) => {
                        return GhOutcome::Failed(format!("gh auth token failed: {}", e.kind()));
                    }
                }
            }
            self.tokens.get(login)
        };
        let call = query(prs, viewer);
        match self.runner.run(&call, token, GH_TIMEOUT) {
            Ok(out) => parse_answer(&out, prs),
            Err(e) => GhOutcome::Failed(format!("gh couldn't run: {}", e.kind())),
        }
    }
}

/// The real runner: a `gh` child process with a timeout. Never used by tests.
pub struct ProcessGh {
    explicit: Option<PathBuf>,
}

impl ProcessGh {
    /// `explicit` is `SyncConfig::gh_path`; `None` searches. The search runs on every call,
    /// so installing `gh` while the app runs is noticed on the next tick.
    pub fn new(explicit: Option<PathBuf>) -> ProcessGh {
        ProcessGh { explicit }
    }

    fn binary(&self) -> io::Result<PathBuf> {
        let fixed: Vec<&Path> = GH_PLACES.iter().map(Path::new).collect();
        let path = std::env::var_os("PATH");
        find_gh(self.explicit.as_deref(), &fixed, path.as_deref())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "gh isn't installed"))
    }

    fn spawn(
        &self,
        args: &[String],
        token: Option<&Secret>,
        timeout: Duration,
    ) -> io::Result<CmdOutput> {
        let mut cmd = Command::new(self.binary()?);
        cmd.args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for name in ENV_REMOVED {
            cmd.env_remove(name);
        }
        cmd.envs(child_env(token));
        finish(cmd.spawn()?, timeout, READ_GRACE)
    }
}

/// How long to wait for `gh`'s output once it has exited, or been killed. A process `gh`
/// started could still hold the pipes, so this stays bounded; but it's generous, because on a
/// loaded Mac a reader thread can take over a second to be scheduled. `launch` waits the same.
const READ_GRACE: Duration = Duration::from_secs(5);

/// Waits for `child`, killing it at `timeout`, then gives its readers `grace` to finish.
/// `status` is `None` when it was killed. Like `launch`'s runner, it never waits on the
/// pipes alone: a grandchild holding them would hold this thread too.
fn finish(mut child: Child, timeout: Duration, grace: Duration) -> io::Result<CmdOutput> {
    let deadline = Instant::now() + timeout;
    let stdout = read_in_background(child.stdout.take());
    let stderr = read_in_background(child.stderr.take());
    let status = wait_until(&mut child, deadline)?;
    Ok(CmdOutput {
        status: status.and_then(|s| s.code()),
        stdout: stdout.recv_timeout(grace).unwrap_or_default(),
        stderr: stderr.recv_timeout(grace).unwrap_or_default(),
    })
}

/// Waits for the child; `None` when it ran past `deadline` and was killed.
fn wait_until(child: &mut Child, deadline: Instant) -> io::Result<Option<ExitStatus>> {
    let mut pause = Duration::from_millis(1);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(Some(status)),
            Ok(None) => {}
            Err(e) => {
                // Don't leave a child we can no longer watch.
                let _ = child.kill();
                let _ = child.wait();
                return Err(e);
            }
        }
        let now = Instant::now();
        if now >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        std::thread::sleep(pause.min(deadline - now));
        pause = (pause * 2).min(Duration::from_millis(20));
    }
}

/// Reads a pipe to its end on its own thread; the text arrives on the channel.
fn read_in_background(pipe: Option<impl Read + Send + 'static>) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    match pipe {
        Some(mut pipe) => {
            std::thread::spawn(move || {
                let mut text = String::new();
                let _ = pipe.read_to_string(&mut text);
                let _ = tx.send(text);
            });
        }
        None => {
            let _ = tx.send(String::new());
        }
    }
    rx
}

impl GhRunner for ProcessGh {
    fn run(
        &mut self,
        call: &GhCall,
        token: Option<&Secret>,
        timeout: Duration,
    ) -> io::Result<CmdOutput> {
        self.spawn(&args(call), token, timeout)
    }

    fn token(&mut self, login: &str) -> io::Result<Secret> {
        let out = self.spawn(&token_args(login), None, Duration::from_secs(10))?;
        if out.status != Some(0) {
            return Err(io::Error::other("gh auth token failed"));
        }
        Ok(Secret::new(out.stdout))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    #[test]
    fn the_query_puts_values_only_in_variables() {
        let prs = vec![
            PrRef::parse("Acme/Web#7").unwrap(),
            PrRef::parse("a/b#12").unwrap(),
        ];
        let GhCall::Graphql { query: doc, vars } = query(&prs, true) else {
            panic!()
        };
        assert!(doc.starts_with("query($o0: String!, $r0: String!, $n0: Int!, $o1: String!"));
        assert!(doc.contains(
            "p1: repository(owner: $o1, name: $r1) { pullRequest(number: $n1) { ...pr } }"
        ));
        assert!(doc.contains("viewer { login pullRequests(states: OPEN, first: 50"));
        assert!(doc.contains("rateLimit { cost remaining resetAt }"));
        assert!(!doc.contains("acme") && !doc.contains("12"));
        assert_eq!(vars[0], ("o0".into(), GhVar::Str("acme".into())));
        assert_eq!(vars[2], ("n0".into(), GhVar::Int(7)));
        let a = args(&query(&prs[..1], false));
        assert_eq!(&a[..3], &["api", "graphql", "-f"]);
        assert_eq!(&a[4..], &["-f", "o0=acme", "-f", "r0=web", "-F", "n0=7"]);
    }

    #[test]
    fn a_viewer_only_query_has_no_variables() {
        let GhCall::Graphql { query: doc, vars } = query(&[], true) else {
            panic!()
        };
        assert!(doc.starts_with("query {"));
        assert!(vars.is_empty());
    }

    #[test]
    fn accounts_active_first_and_only_github_com() {
        let json = r#"{"hosts":{"github.com":[
            {"state":"success","active":false,"host":"github.com","login":"work"},
            {"state":"success","active":true,"host":"github.com","login":"me"},
            {"state":"error","active":false,"host":"github.com","login":"old"}],
          "ghe.example.com":[{"state":"success","active":true,"login":"corp"}]}}"#;
        let a = parse_accounts(json).unwrap();
        let logins: Vec<(&str, bool, bool)> = a
            .iter()
            .map(|a| (a.login.as_str(), a.active, a.ok))
            .collect();
        assert_eq!(
            logins,
            vec![
                ("me", true, true),
                ("work", false, true),
                ("old", false, false)
            ]
        );
        assert!(parse_accounts(r#"{"hosts":{}}"#).unwrap().is_empty());
        assert!(parse_accounts("nope").is_err());
    }

    /// `sh -c script`, piped as `ProcessGh` pipes `gh`. Only tests run a shell.
    fn sh(script: &str) -> std::process::Child {
        use std::process::{Command, Stdio};
        Command::new("/bin/sh")
            .args(["-c", script])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    }

    #[test]
    fn a_finished_child_gives_its_code_and_both_outputs() {
        let out = finish(sh("echo hi; echo oops >&2; exit 3"), GH_TIMEOUT, READ_GRACE).unwrap();
        assert_eq!(out.status, Some(3));
        assert_eq!(
            (out.stdout.as_str(), out.stderr.as_str()),
            ("hi\n", "oops\n")
        );
    }

    #[test]
    fn a_grandchild_holding_the_pipes_holds_us_only_for_the_grace_period() {
        let began = std::time::Instant::now();
        let out = finish(
            sh("sleep 4 & exit 0"),
            GH_TIMEOUT,
            Duration::from_millis(200),
        )
        .unwrap();
        assert_eq!(out.status, Some(0), "gh itself finished");
        assert!(
            began.elapsed() < Duration::from_secs(2),
            "{:?}",
            began.elapsed()
        );
    }

    #[test]
    fn a_child_past_its_deadline_is_killed_and_has_no_code() {
        let began = std::time::Instant::now();
        let out = finish(
            sh("sleep 4"),
            Duration::from_millis(200),
            Duration::from_millis(200),
        )
        .unwrap();
        assert_eq!(out.status, None);
        assert!(
            began.elapsed() < Duration::from_secs(2),
            "{:?}",
            began.elapsed()
        );
    }

    #[test]
    fn the_child_env_carries_the_token_only_for_other_accounts() {
        assert!(child_env(None).iter().all(|(k, _)| *k != "GH_TOKEN"));
        let env = child_env(Some(&Secret::new("t0k")));
        assert!(env.contains(&("GH_TOKEN", "t0k".to_string())));
        assert!(env.contains(&("GH_PROMPT_DISABLED", "1".to_string())));
        assert_eq!(ENV_REMOVED, &["GH_TOKEN", "GITHUB_TOKEN"]);
    }
}
