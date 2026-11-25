//! Repos, and which repos each task is linked to (rule 24).

use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use crate::events;
use crate::model::{Actor, EventKind, Repo};
use crate::permissions::{Call, allowed};
use crate::sql::query_all;
use crate::tasks::ensure_exists;
use crate::tx::Tx;
use crate::{Error, RepoId, Result, TaskId, fault};

impl crate::Store {
    /// Registers a repo tasks can point at. Only `me` does this.
    pub fn add_repo(
        &mut self,
        name: &str,
        path: &Path,
        remote: Option<&str>,
        actor: &Actor,
    ) -> Result<Repo> {
        self.write(|tx| add_repo(tx, name, path, remote, actor))
    }

    /// Renames a repo, points it at another folder, or changes its remote. `None` leaves a
    /// field as it is; `remote: Some(None)` clears the remote. Tasks keep the repo, so they
    /// follow it. Writes no event, like `add_repo`. Only `me` does this (rule 33).
    pub fn edit_repo(
        &mut self,
        id: RepoId,
        name: Option<&str>,
        path: Option<&Path>,
        remote: Option<Option<&str>>,
        actor: &Actor,
    ) -> Result<Repo> {
        self.write(|tx| edit_repo(tx, id, name, path, remote, actor))
    }

    /// Unregisters a repo. First it's unlinked from every task that has it, done ones too,
    /// with a `repo_unlinked` event on each; returns those tasks, by id. Only `me` does this
    /// (rule 33).
    pub fn remove_repo(&mut self, id: RepoId, actor: &Actor) -> Result<Vec<TaskId>> {
        self.write(|tx| remove_repo(tx, id, actor))
    }

    /// Every registered repo, by name.
    pub fn repos(&self) -> Result<Vec<Repo>> {
        repos(&self.conn)
    }

    /// Links a task to one more repo, so `launch` starts a thread there too. Linking a repo
    /// that's already linked changes nothing. `me` and `plan` only (rule 8).
    pub fn link_repo(&mut self, id: TaskId, repo: RepoId, actor: &Actor) -> Result<()> {
        self.write(|tx| link_repo(tx, id, repo, actor))
    }

    /// Unlinks a repo from a task; `Error::NotFound` if it wasn't linked. `me` and `plan`
    /// only (rule 8).
    pub fn unlink_repo(&mut self, id: TaskId, repo: RepoId, actor: &Actor) -> Result<()> {
        self.write(|tx| unlink_repo(tx, id, repo, actor))
    }
}

fn add_repo(
    tx: &mut Tx<'_>,
    name: &str,
    path: &Path,
    remote: Option<&str>,
    actor: &Actor,
) -> Result<Repo> {
    allowed(Call::AddRepo, actor)?;
    let path = path.to_string_lossy();
    tx.execute(
        "INSERT INTO repos (name, path, remote_url) VALUES (?1, ?2, ?3)",
        params![name, path, remote],
    )
    .map_err(taken)?;
    Ok(Repo {
        id: RepoId(tx.last_insert_rowid()),
        name: name.to_string(),
        path: path.into_owned(),
        remote_url: remote.map(str::to_string),
    })
}

/// `UNIQUE` on a name or path becomes `Error::AlreadyExists`.
fn taken(e: rusqlite::Error) -> Error {
    match e {
        rusqlite::Error::SqliteFailure(f, _)
            if f.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE =>
        {
            Error::AlreadyExists
        }
        other => Error::Db(other),
    }
}

fn edit_repo(
    tx: &mut Tx<'_>,
    id: RepoId,
    name: Option<&str>,
    path: Option<&Path>,
    remote: Option<Option<&str>>,
    actor: &Actor,
) -> Result<Repo> {
    allowed(Call::EditRepo, actor)?;
    let mut repo = repos(tx)?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or(Error::NotFound)?;
    if let Some(name) = name {
        repo.name = name.trim().to_string();
    }
    if let Some(path) = path {
        repo.path = path.to_string_lossy().into_owned();
    }
    if let Some(remote) = remote {
        repo.remote_url = remote.map(str::to_string);
    }
    // A blank name can't be told apart in a list, and a blank path points nowhere.
    if repo.name.is_empty() || repo.path.trim().is_empty() {
        return Err(Error::InvalidTarget);
    }
    tx.execute(
        "UPDATE repos SET name = ?2, path = ?3, remote_url = ?4 WHERE id = ?1",
        params![id.0, repo.name, repo.path, repo.remote_url],
    )
    .map_err(taken)?;
    fault::hit(fault::REPO_EDITED)?;
    Ok(repo)
}

fn remove_repo(tx: &mut Tx<'_>, id: RepoId, actor: &Actor) -> Result<Vec<TaskId>> {
    allowed(Call::RemoveRepo, actor)?;
    let name = name(tx, id)?;
    let tasks: Vec<TaskId> = query_all(
        tx,
        "SELECT task_id FROM task_repos WHERE repo_id = ?1 ORDER BY task_id",
        params![id.0],
        |r| Ok(TaskId(r.get(0)?)),
    )?;
    tx.execute("DELETE FROM task_repos WHERE repo_id = ?1", params![id.0])?;
    for task in &tasks {
        events::record(tx, *task, EventKind::RepoUnlinked, None, Some(&name), actor)?;
    }
    fault::hit(fault::REPO_REMOVED)?;
    tx.execute("DELETE FROM repos WHERE id = ?1", params![id.0])?;
    Ok(tasks)
}

fn repos(conn: &Connection) -> Result<Vec<Repo>> {
    query_all(
        conn,
        "SELECT id, name, path, remote_url FROM repos ORDER BY name",
        [],
        |r| {
            Ok(Repo {
                id: RepoId(r.get(0)?),
                name: r.get(1)?,
                path: r.get(2)?,
                remote_url: r.get(3)?,
            })
        },
    )
}

/// A repo's name, for event bodies; `Error::NotFound` if it doesn't exist.
fn name(conn: &Connection, repo: RepoId) -> Result<String> {
    conn.query_row(
        "SELECT name FROM repos WHERE id = ?1",
        params![repo.0],
        |r| r.get(0),
    )
    .optional()?
    .ok_or(Error::NotFound)
}

/// Links `repo` to `task` with no event; `Ok(false)` if it was already linked.
pub(crate) fn insert_link(
    conn: &Connection,
    task: TaskId,
    repo: RepoId,
    actor: &Actor,
) -> Result<bool> {
    crate::tasks::ensure_repo_exists(conn, repo)?;
    let added = conn.execute(
        "INSERT INTO task_repos (task_id, repo_id, linked_by) VALUES (?1, ?2, ?3)
         ON CONFLICT DO NOTHING",
        params![task.0, repo.0, actor.to_string()],
    )?;
    Ok(added > 0)
}

fn link_repo(tx: &mut Tx<'_>, task: TaskId, repo: RepoId, actor: &Actor) -> Result<()> {
    allowed(Call::ChangeRepos, actor)?;
    ensure_exists(tx, task)?;
    let name = name(tx, repo)?;
    if !insert_link(tx, task, repo, actor)? {
        return Ok(());
    }
    fault::hit(fault::REPO_LINKED)?;
    events::record(tx, task, EventKind::RepoLinked, None, Some(&name), actor)
}

fn unlink_repo(tx: &mut Tx<'_>, task: TaskId, repo: RepoId, actor: &Actor) -> Result<()> {
    allowed(Call::ChangeRepos, actor)?;
    ensure_exists(tx, task)?;
    let name = name(tx, repo)?;
    let removed = tx.execute(
        "DELETE FROM task_repos WHERE task_id = ?1 AND repo_id = ?2",
        params![task.0, repo.0],
    )?;
    if removed == 0 {
        return Err(Error::NotFound);
    }
    fault::hit(fault::REPO_UNLINKED)?;
    events::record(tx, task, EventKind::RepoUnlinked, None, Some(&name), actor)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::{NewTask, Store};

    fn counts(s: &Store) -> (i64, i64) {
        let n = |sql: &str| s.conn.query_row(sql, [], |r| r.get(0)).unwrap();
        (
            n("SELECT count(*) FROM task_repos"),
            n("SELECT count(*) FROM task_events"),
        )
    }

    fn fixture(s: &mut Store) -> (TaskId, RepoId) {
        let repo = s
            .add_repo("web", Path::new("/w/web"), None, &Actor::Me)
            .unwrap();
        let task = s.add_task(NewTask::new("x"), &Actor::Me).unwrap();
        (task.id, repo.id)
    }

    #[test]
    fn a_failed_link_repo_leaves_no_link_and_no_event() {
        let mut s = Store::open_in_memory().unwrap();
        let (task, repo) = fixture(&mut s);
        let before = counts(&s);
        let committed = s.last_committed.clone();
        fault::arm(fault::REPO_LINKED);
        assert!(s.link_repo(task, repo, &Actor::Me).is_err());
        assert_eq!(counts(&s), before);
        assert_eq!(s.last_committed, committed);

        s.link_repo(task, repo, &Actor::Me).unwrap();
        assert_eq!(counts(&s), (before.0 + 1, before.1 + 1));
    }

    #[test]
    fn a_failed_unlink_repo_keeps_the_link_and_writes_no_event() {
        let mut s = Store::open_in_memory().unwrap();
        let (task, repo) = fixture(&mut s);
        s.link_repo(task, repo, &Actor::Me).unwrap();
        let before = counts(&s);
        fault::arm(fault::REPO_UNLINKED);
        assert!(s.unlink_repo(task, repo, &Actor::Me).is_err());
        assert_eq!(counts(&s), before);
        assert_eq!(s.task(task).unwrap().repos, vec![repo]);
    }

    #[test]
    fn a_failed_remove_repo_keeps_the_repo_its_links_and_writes_no_event() {
        let mut s = Store::open_in_memory().unwrap();
        let (task, repo) = fixture(&mut s);
        s.link_repo(task, repo, &Actor::Me).unwrap();
        let before = counts(&s);
        let committed = s.last_committed.clone();
        fault::arm(fault::REPO_REMOVED);
        assert!(s.remove_repo(repo, &Actor::Me).is_err());
        assert_eq!(counts(&s), before);
        assert_eq!(s.repos().unwrap().len(), 1);
        assert_eq!(s.last_committed, committed);

        s.remove_repo(repo, &Actor::Me).unwrap();
        assert_eq!(counts(&s), (0, before.1 + 1));
    }

    #[test]
    fn a_failed_edit_repo_changes_nothing() {
        let mut s = Store::open_in_memory().unwrap();
        let (_, repo) = fixture(&mut s);
        let before = s.repos().unwrap();
        fault::arm(fault::REPO_EDITED);
        assert!(
            s.edit_repo(repo, Some("site"), None, None, &Actor::Me)
                .is_err()
        );
        assert_eq!(s.repos().unwrap(), before);
    }

    #[test]
    fn a_failed_add_task_leaves_no_repo_link_behind() {
        let mut s = Store::open_in_memory().unwrap();
        let (_, repo) = fixture(&mut s);
        let before = counts(&s);
        fault::arm(fault::TASK_INSERTED);
        assert!(
            s.add_task(NewTask::new("y").in_repo(repo), &Actor::Me)
                .is_err()
        );
        assert_eq!(counts(&s), before);
    }
}
