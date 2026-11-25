//! Who may make which call: SPEC-core rule 8, as one table.
//!
//! Status changes have their own from/to table (rule 6) in `status.rs`.

use crate::model::Actor;
use crate::{Error, Result};

/// Every call that changes something, apart from status changes.
// Variants are used as T4–T12 land; the rule 8 test already covers all of them.
#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Call {
    AddRepo,
    AddTask,
    AddPlan,
    EditTask,
    AddNote,
    MoveTask,
    DeleteTask,
    Link,
    /// Agents may only remove links they added; the caller checks that part.
    Unlink,
    SetPrState,
    ChangeDependency,
}

/// `Ok` if `actor` may make `call` at all; `Error::NotAllowed` otherwise.
pub(crate) fn allowed(call: Call, actor: &Actor) -> Result<()> {
    use Actor::*;
    use Call::*;
    if matches!(actor, Agent(r) if r.trim().is_empty()) {
        return Err(Error::NotAllowed);
    }
    let ok = match call {
        AddRepo | EditTask | MoveTask | DeleteTask => *actor == Me,
        AddTask | Unlink => matches!(actor, Me | Agent(_)),
        AddPlan | ChangeDependency => matches!(actor, Me | Plan),
        // A plan arrives as a tree of tasks; it has nothing to say about one afterwards.
        AddNote => *actor != Plan,
        Link => true,
        SetPrState => matches!(actor, Me | SyncGh),
    };
    if ok { Ok(()) } else { Err(Error::NotAllowed) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every_actor() -> Vec<Actor> {
        vec![
            Actor::Me,
            Actor::Agent("t3/a1".into()),
            Actor::Plan,
            Actor::SyncGh,
            Actor::SyncT3,
            Actor::Launch,
        ]
    }

    /// The rule 8 table, row by row: (call, actors allowed). Every other actor must be refused.
    #[test]
    fn rule_8_table() {
        let me = Actor::Me;
        let agent = Actor::Agent("t3/a1".into());
        let table: Vec<(Call, Vec<Actor>)> = vec![
            (Call::AddRepo, vec![me.clone()]),
            (Call::AddTask, vec![me.clone(), agent.clone()]),
            (Call::AddPlan, vec![me.clone(), Actor::Plan]),
            (Call::EditTask, vec![me.clone()]),
            (
                Call::AddNote,
                every_actor()
                    .into_iter()
                    .filter(|a| *a != Actor::Plan)
                    .collect(),
            ),
            (Call::MoveTask, vec![me.clone()]),
            (Call::DeleteTask, vec![me.clone()]),
            (Call::Link, every_actor()),
            (Call::Unlink, vec![me.clone(), agent.clone()]),
            (Call::SetPrState, vec![me.clone(), Actor::SyncGh]),
            (Call::ChangeDependency, vec![me.clone(), Actor::Plan]),
        ];
        for (call, allowed_actors) in table {
            for actor in every_actor() {
                let expected = allowed_actors.contains(&actor);
                assert_eq!(
                    allowed(call, &actor).is_ok(),
                    expected,
                    "{call:?} by {actor}: expected allowed = {expected}"
                );
            }
        }
    }

    #[test]
    fn an_agent_with_an_empty_ref_may_do_nothing() {
        for call in [Call::AddNote, Call::Link, Call::AddTask] {
            assert!(allowed(call, &Actor::Agent(String::new())).is_err());
            assert!(allowed(call, &Actor::Agent("  ".into())).is_err());
        }
    }
}
