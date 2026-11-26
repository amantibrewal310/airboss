//! Who is calling: worked out once, at start-up, from the thread's environment.
//!
//! Over MCP a caller is always an agent. Tool arguments never name the actor.

use airboss_core::{Actor, LinkKind};

/// The variables identity is read from, in the order they're tried.
pub const T3_THREAD: &str = "T3CODE_THREAD_ID";
/// Set by Herdr's core in every terminal it runs: the terminal's address.
pub const HERDR_TERMINAL: &str = "HC_TERMINAL_ID";
pub const CLAUDE_SESSION: &str = "CLAUDE_CODE_SESSION_ID";
pub const AIRBOSS_THREAD: &str = "AIRBOSS_THREAD";

/// The agent this process acts as, and the thread links that are its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub actor: Actor,
    /// This thread's T3 id, the `ref` of its `t3_thread` link.
    pub t3_thread: Option<String>,
    /// This thread's Claude Code session id, the `ref` of its `claude_session` link.
    pub claude_session: Option<String>,
    /// This thread's Herdr terminal address, the `ref` of its `herdr_terminal` link.
    pub herdr_terminal: Option<String>,
    /// Set when the thread couldn't be identified; the server instructions repeat it.
    pub warning: Option<String>,
}

impl Identity {
    /// Reads identity through `var`, which returns a variable's value if it's set:
    /// T3 Code's thread id, then Herdr's terminal address, then Claude Code's session id, then
    /// `AIRBOSS_THREAD`. A thread host (T3 Code, Herdr) names the thread before the agent it
    /// runs, so the agent's own link is the one Do it made. `parent_pid` names the caller when
    /// nothing else does. Blank values count as unset.
    pub fn from_env(var: impl Fn(&str) -> Option<String>, parent_pid: u32) -> Identity {
        let get = |name: &str| {
            var(name)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        let t3 = get(T3_THREAD);
        let herdr = get(HERDR_TERMINAL);
        let claude = get(CLAUDE_SESSION);
        let named = |thread: String, t3_thread, claude_session, herdr_terminal| Identity {
            actor: Actor::Agent(thread),
            t3_thread,
            claude_session,
            herdr_terminal,
            warning: None,
        };
        if let Some(id) = t3 {
            return named(format!("t3/{id}"), Some(id), claude, herdr);
        }
        if let Some(address) = herdr {
            return named(format!("herdr/{address}"), None, claude, Some(address));
        }
        if let Some(id) = claude {
            return named(format!("claude/{id}"), None, Some(id), None);
        }
        if let Some(value) = get(AIRBOSS_THREAD) {
            let thread = value.strip_prefix("agent:").unwrap_or(&value).to_string();
            let under = |prefix: &str| {
                thread
                    .strip_prefix(prefix)
                    .filter(|rest| !rest.is_empty())
                    .map(str::to_string)
            };
            let (t3_thread, claude_session, herdr_terminal) =
                (under("t3/"), under("claude/"), under("herdr/"));
            return named(thread, t3_thread, claude_session, herdr_terminal);
        }
        let actor = Actor::Agent(format!("unknown/{parent_pid}"));
        let warning = format!(
            "This server couldn't tell which thread it serves: none of {T3_THREAD}, \
             {HERDR_TERMINAL}, {CLAUDE_SESSION} or {AIRBOSS_THREAD} is set. It acts as {actor}, so `mine` and \
             linking this thread without a `ref` won't work. Set {AIRBOSS_THREAD} in this \
             client's MCP config to fix it."
        );
        Identity {
            actor,
            t3_thread: None,
            claude_session: None,
            herdr_terminal: None,
            warning: Some(warning),
        }
    }

    /// This process's identity, from its real environment.
    pub fn current() -> Identity {
        Identity::from_env(
            |name| std::env::var(name).ok(),
            std::os::unix::process::parent_id(),
        )
    }

    /// The `ref` of this thread's own link of `kind`, if it has one.
    pub fn thread_ref(&self, kind: LinkKind) -> Option<&str> {
        match kind {
            LinkKind::T3Thread => self.t3_thread.as_deref(),
            LinkKind::ClaudeSession => self.claude_session.as_deref(),
            LinkKind::HerdrTerminal => self.herdr_terminal.as_deref(),
            _ => None,
        }
    }

    /// The kind of this thread's own link, the one "link this thread" should use: the host
    /// that named it (T3 Code, Herdr), else Claude Code. `None` when the thread is unknown.
    pub fn own_kind(&self) -> Option<LinkKind> {
        [
            LinkKind::T3Thread,
            LinkKind::HerdrTerminal,
            LinkKind::ClaudeSession,
        ]
        .into_iter()
        .find(|&kind| {
            self.thread_ref(kind)
                .is_some_and(|r| self.actor == Actor::Agent(format!("{}/{r}", prefix(kind))))
        })
    }

    /// Every link that means "this thread", for `mine`.
    pub fn thread_links(&self) -> Vec<(LinkKind, &str)> {
        [
            LinkKind::T3Thread,
            LinkKind::HerdrTerminal,
            LinkKind::ClaudeSession,
        ]
        .into_iter()
        .filter_map(|kind| self.thread_ref(kind).map(|r| (kind, r)))
        .collect()
    }
}

/// The actor prefix a thread kind names its agents with.
fn prefix(kind: LinkKind) -> &'static str {
    match kind {
        LinkKind::T3Thread => "t3",
        LinkKind::HerdrTerminal => "herdr",
        _ => "claude",
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use std::collections::HashMap;

    use super::*;

    fn with(vars: &[(&str, &str)]) -> Identity {
        let map: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Identity::from_env(|name| map.get(name).cloned(), 4242)
    }

    fn agent(r: &str) -> Actor {
        Actor::Agent(r.into())
    }

    #[test]
    fn a_t3_thread_is_named_by_its_t3_id() {
        let id = with(&[(T3_THREAD, "97b5c8b5")]);
        assert_eq!(id.actor, agent("t3/97b5c8b5"));
        assert_eq!(id.thread_ref(LinkKind::T3Thread), Some("97b5c8b5"));
        assert_eq!(id.thread_ref(LinkKind::ClaudeSession), None);
        assert_eq!(id.warning, None);
    }

    #[test]
    fn a_herdr_terminal_names_the_agent_before_claude_code_does() {
        // Claude Code in a Herdr pane: the terminal is the thread, as a T3 thread would be.
        let id = with(&[(HERDR_TERMINAL, "t4p"), (CLAUDE_SESSION, "5e551011")]);
        assert_eq!(id.actor, agent("herdr/t4p"));
        assert_eq!(id.own_kind(), Some(LinkKind::HerdrTerminal));
        assert_eq!(id.thread_ref(LinkKind::HerdrTerminal), Some("t4p"));
        assert_eq!(
            id.thread_links(),
            vec![
                (LinkKind::HerdrTerminal, "t4p"),
                (LinkKind::ClaudeSession, "5e551011")
            ]
        );
        // Codex in a Herdr pane passes only the terminal.
        let id = with(&[(HERDR_TERMINAL, "t7")]);
        assert_eq!(id.actor, agent("herdr/t7"));
        assert_eq!(id.own_kind(), Some(LinkKind::HerdrTerminal));
        // T3 Code still comes first.
        let id = with(&[(T3_THREAD, "97b5"), (HERDR_TERMINAL, "t4p")]);
        assert_eq!(id.actor, agent("t3/97b5"));
        assert_eq!(id.own_kind(), Some(LinkKind::T3Thread));
        // Claude Code on its own is unchanged.
        let id = with(&[(CLAUDE_SESSION, "5e551011")]);
        assert_eq!(id.actor, agent("claude/5e551011"));
        assert_eq!(id.own_kind(), Some(LinkKind::ClaudeSession));
        // Named by hand.
        let id = with(&[(AIRBOSS_THREAD, "herdr/vps/t7")]);
        assert_eq!(id.actor, agent("herdr/vps/t7"));
        assert_eq!(id.thread_ref(LinkKind::HerdrTerminal), Some("vps/t7"));
        assert_eq!(id.own_kind(), Some(LinkKind::HerdrTerminal));
        // Unknown: no kind to name.
        let id = with(&[]);
        assert_eq!(id.own_kind(), None);
        assert!(id.warning.unwrap().contains(HERDR_TERMINAL));
    }

    #[test]
    fn a_t3_thread_running_claude_code_knows_both_ids_but_is_named_by_t3() {
        let id = with(&[(T3_THREAD, "97b5"), (CLAUDE_SESSION, "5e551011")]);
        assert_eq!(id.actor, agent("t3/97b5"));
        assert_eq!(
            id.thread_links(),
            vec![
                (LinkKind::T3Thread, "97b5"),
                (LinkKind::ClaudeSession, "5e551011")
            ]
        );
    }

    #[test]
    fn a_claude_code_session_outside_t3_is_named_by_its_session() {
        let id = with(&[(CLAUDE_SESSION, "5e551011")]);
        assert_eq!(id.actor, agent("claude/5e551011"));
        assert_eq!(
            id.thread_links(),
            vec![(LinkKind::ClaudeSession, "5e551011")]
        );
        assert_eq!(id.warning, None);
    }

    #[test]
    fn airboss_thread_names_the_agent_when_nothing_else_does() {
        let id = with(&[(AIRBOSS_THREAD, "codex/airboss-main")]);
        assert_eq!(id.actor, agent("codex/airboss-main"));
        assert!(id.thread_links().is_empty());
        assert_eq!(id.warning, None);
    }

    #[test]
    fn airboss_thread_comes_after_the_client_ids() {
        let id = with(&[(AIRBOSS_THREAD, "codex/x"), (CLAUDE_SESSION, "s1")]);
        assert_eq!(id.actor, agent("claude/s1"));
    }

    #[test]
    fn airboss_thread_in_t3_or_claude_form_knows_its_thread_link() {
        let id = with(&[(AIRBOSS_THREAD, "t3/abc")]);
        assert_eq!(id.actor, agent("t3/abc"));
        assert_eq!(id.thread_ref(LinkKind::T3Thread), Some("abc"));
        let id = with(&[(AIRBOSS_THREAD, "agent:claude/s9")]);
        assert_eq!(id.actor, agent("claude/s9"));
        assert_eq!(id.thread_ref(LinkKind::ClaudeSession), Some("s9"));
    }

    #[test]
    fn nothing_set_is_an_unknown_agent_named_by_the_parent_pid_with_a_warning() {
        let id = with(&[]);
        assert_eq!(id.actor, agent("unknown/4242"));
        assert!(id.thread_links().is_empty());
        let warning = id.warning.unwrap();
        assert!(warning.contains("agent:unknown/4242"), "{warning}");
        assert!(warning.contains(AIRBOSS_THREAD), "{warning}");
    }

    #[test]
    fn blank_values_count_as_unset() {
        let id = with(&[
            (T3_THREAD, "  "),
            (CLAUDE_SESSION, ""),
            (AIRBOSS_THREAD, " "),
        ]);
        assert_eq!(id.actor, agent("unknown/4242"));
    }

    #[test]
    fn values_are_trimmed() {
        let id = with(&[(T3_THREAD, " 97b5\n")]);
        assert_eq!(id.actor, agent("t3/97b5"));
    }

    #[test]
    fn the_actor_is_never_me() {
        for vars in [
            vec![(AIRBOSS_THREAD, "me")],
            vec![(AIRBOSS_THREAD, "agent:me")],
            vec![],
        ] {
            assert!(matches!(with(&vars).actor, Actor::Agent(_)));
        }
    }
}
