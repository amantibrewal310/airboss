//! The `gh` layer against `FakeGh`: accounts, the batched read, every mapping row, and
//! what each kind of answer means. Nothing here runs the real `gh`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::time::SystemTime;

use airboss_core::PrState;
use airboss_services::{Checks, PrRef, Review};
use airboss_sync::fake::{FakeFailure, FakeGh, FakePr, TempDir, fake_token};
use airboss_sync::gh::{self, CmdOutput, GhClient, GhOff, GhOutcome, PrResult};

fn pr(s: &str) -> PrRef {
    PrRef::parse(s).unwrap()
}

fn now() -> SystemTime {
    airboss_services::time::from_ts("2026-11-26T12:00:00.000Z").unwrap()
}

fn client(gh: &FakeGh) -> GhClient {
    let mut c = GhClient::new(Box::new(gh.clone()));
    let _ = c.read_accounts(now());
    c
}

fn found(outcome: &GhOutcome, i: usize) -> gh::PrData {
    match outcome {
        GhOutcome::Data { results, .. } => match &results[i].1 {
            PrResult::Found(d) => d.clone(),
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
}

#[test]
fn finding_gh() {
    let dir = TempDir::new();
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join("gh"), "").unwrap();
    let nowhere = dir.path().join("nowhere/gh");
    let path = std::env::join_paths([dir.path().join("empty"), bin.clone()]).unwrap();
    assert_eq!(
        gh::find_gh(None, &[&nowhere], Some(&path)),
        Some(bin.join("gh"))
    );
    let fixed = bin.join("gh");
    assert_eq!(gh::find_gh(None, &[&fixed], None), Some(fixed.clone()));
    assert_eq!(gh::find_gh(Some(&fixed), &[], None), Some(fixed));
    assert_eq!(
        gh::find_gh(Some(Path::new("/no/gh")), &[], Some(&path)),
        None,
        "an explicit path is used alone"
    );
    assert_eq!(gh::find_gh(None, &[&nowhere], None), None);
}

#[test]
fn missing_gh_and_signed_out() {
    let mut c = GhClient::new(Box::new(FakeGh::new().missing()));
    assert_eq!(c.read_accounts(now()), Err(GhOff::Missing));
    let mut c = GhClient::new(Box::new(FakeGh::new()));
    assert_eq!(c.read_accounts(now()), Err(GhOff::SignedOut));
    let mut c = GhClient::new(Box::new(FakeGh::new().broken_account("old")));
    assert_eq!(
        c.read_accounts(now()),
        Err(GhOff::SignedOut),
        "no success account"
    );
}

#[test]
fn the_active_account_uses_gh_itself_and_others_get_a_token_in_the_env_only() {
    let gh = FakeGh::new().account("me", true).account("work", false);
    let mut c = client(&gh);
    let logins: Vec<&str> = c.accounts().iter().map(|a| a.login.as_str()).collect();
    assert_eq!(logins, vec!["me", "work"]);
    c.read("me", &[], true);
    c.read("work", &[], true);
    c.read("work", &[], true);
    let calls = gh.calls();
    let tokens: Vec<_> = calls.iter().filter(|c| c.kind == "token").collect();
    assert_eq!(tokens.len(), 1, "fetched once and held in memory");
    assert_eq!(tokens[0].argv, vec!["auth", "token", "--user", "work"]);
    let reads = gh.reads();
    assert_eq!(
        reads
            .iter()
            .map(|c| (c.account.clone().unwrap(), c.token_set))
            .collect::<Vec<_>>(),
        vec![
            ("me".into(), false),
            ("work".into(), true),
            ("work".into(), true)
        ]
    );
    for call in &calls {
        assert!(
            call.argv.iter().all(|a| !a.contains(&fake_token("work"))),
            "never in argv"
        );
    }
    c.forget_token("work");
    c.read("work", &[], false);
    assert_eq!(
        gh.calls().iter().filter(|c| c.kind == "token").count(),
        2,
        "fetched again"
    );
}

#[test]
fn each_mapping_row() {
    let mut merged = FakePr::open("me");
    merged.state = "MERGED";
    let mut closed = FakePr::open("me");
    closed.state = "CLOSED";
    let mut draft = FakePr::open("me");
    draft.draft = true;
    draft.review = Some("CHANGES_REQUESTED");
    draft.rollup = Some("FAILURE");
    let mut approved = FakePr::open("me");
    approved.review = Some("APPROVED");
    approved.rollup = Some("SUCCESS");
    let mut required = FakePr::open("me");
    required.review = Some("REVIEW_REQUIRED");
    required.rollup = Some("PENDING");
    let mut errored = FakePr::open("me");
    errored.rollup = Some("ERROR");
    let mut expected = FakePr::open("me");
    expected.rollup = Some("EXPECTED");
    let gh = FakeGh::new()
        .account("me", true)
        .sees("me", "acme/web")
        .pr("acme/web#1", merged)
        .pr("acme/web#2", closed)
        .pr("acme/web#3", draft)
        .pr("acme/web#4", approved)
        .pr("acme/web#5", required)
        .pr("acme/web#6", errored)
        .pr("acme/web#7", expected)
        .pr("acme/web#8", FakePr::open("me"));
    let mut c = client(&gh);
    let prs: Vec<PrRef> = (1..=8).map(|n| pr(&format!("acme/web#{n}"))).collect();
    let out = c.read("me", &prs, false);
    let d: Vec<gh::PrData> = (0..8).map(|i| found(&out, i)).collect();
    assert_eq!(d[0].state, PrState::Merged);
    assert!(d[0].merged_at.is_some());
    assert_eq!(d[1].state, PrState::Closed);
    assert_eq!(d[7].state, PrState::Open);
    assert_eq!(
        (d[2].draft, d[2].review, d[2].checks),
        (true, Review::ChangesRequested, Checks::Failing)
    );
    assert_eq!(
        (d[3].review, d[3].checks),
        (Review::Approved, Checks::Passing)
    );
    assert_eq!(
        (d[4].review, d[4].checks),
        (Review::Required, Checks::Pending)
    );
    assert_eq!(d[5].checks, Checks::Failing, "ERROR");
    assert_eq!(d[6].checks, Checks::Pending, "EXPECTED");
    assert_eq!(
        (d[7].review, d[7].checks),
        (Review::None, Checks::None),
        "null and no rollup"
    );
    let x = &d[7];
    assert_eq!(x.pr.as_str(), "acme/web#8");
    assert_eq!(x.url.as_deref(), Some("https://github.com/acme/web/pull/8"));
    assert_eq!((x.additions, x.deletions), (Some(10), Some(2)));
    assert_eq!(
        (x.head_ref.as_deref(), x.author.as_deref()),
        (Some("feat/x"), Some("me"))
    );
    assert_eq!(x.title.as_deref(), Some("A change"));
    assert_eq!(x.updated_at.as_deref(), Some("2026-11-26T11:00:00.000Z"));
}

#[test]
fn not_found_at_the_repo_is_no_access_and_at_the_pr_is_not_found() {
    let gh = FakeGh::new()
        .account("me", true)
        .sees("me", "acme/web")
        .pr("acme/web#1", FakePr::open("x"));
    let mut c = client(&gh);
    let out = c.read(
        "me",
        &[pr("acme/web#1"), pr("acme/web#99"), pr("secret/repo#1")],
        true,
    );
    let GhOutcome::Data {
        results,
        viewer,
        budget,
    } = out
    else {
        panic!()
    };
    assert!(matches!(results[0].1, PrResult::Found(_)));
    assert_eq!(results[1].1, PrResult::NotFound);
    assert_eq!(results[2].1, PrResult::NoAccess);
    assert_eq!(viewer, Some(Vec::new()), "x's PR isn't mine");
    assert_eq!(budget.unwrap().remaining, 4999);
}

#[test]
fn viewer_prs_are_mine_and_open() {
    let mut mine_merged = FakePr::open("me");
    mine_merged.state = "MERGED";
    let gh = FakeGh::new()
        .account("me", true)
        .sees("me", "acme/web")
        .pr("acme/web#1", FakePr::open("me"))
        .pr("acme/web#2", mine_merged)
        .pr("acme/web#3", FakePr::open("someone"));
    let mut c = client(&gh);
    let GhOutcome::Data { viewer, .. } = c.read("me", &[], true) else {
        panic!()
    };
    let refs: Vec<String> = viewer
        .unwrap()
        .into_iter()
        .map(|p| p.pr.to_string())
        .collect();
    assert_eq!(refs, vec!["acme/web#1"]);
}

#[test]
fn failures_signed_out_secondary_limit_and_timeouts() {
    let gh = FakeGh::new().account("me", true);
    let mut c = client(&gh);
    gh.fail_next("me", FakeFailure::SignedOut);
    assert_eq!(c.read("me", &[], true), GhOutcome::SignedOut);
    gh.fail_next("me", FakeFailure::Secondary);
    assert_eq!(c.read("me", &[], true), GhOutcome::SecondaryLimit);
    gh.fail_next(
        "me",
        FakeFailure::Exit("error connecting to api.github.com\nmore".into()),
    );
    assert_eq!(
        c.read("me", &[], true),
        GhOutcome::Failed("error connecting to api.github.com".into())
    );
    gh.fail_next("me", FakeFailure::Timeout);
    assert_eq!(
        c.read("me", &[], true),
        GhOutcome::Failed("gh took longer than 60 s".into())
    );
}

#[test]
fn exit_one_with_data_is_used() {
    let out = CmdOutput {
        status: Some(1),
        stdout: r#"{"data":{"rateLimit":{"cost":1,"remaining":150,"resetAt":"2026-11-26T13:00:00Z"},"p0":null},
                    "errors":[{"type":"NOT_FOUND","path":["p0"],"message":"nope"}]}"#.into(),
        stderr: "gh: Could not resolve to a Repository".into(),
    };
    let GhOutcome::Data {
        results, budget, ..
    } = gh::parse_answer(&out, &[pr("a/b#1")])
    else {
        panic!()
    };
    assert_eq!(results[0].1, PrResult::NoAccess);
    let b = budget.unwrap();
    assert_eq!((b.cost, b.remaining), (1, 150));
    assert!(b.reset_at.is_some());
}

#[test]
fn no_token_in_debug_output() {
    let gh = FakeGh::new().account("me", true).account("work", false);
    let mut c = client(&gh);
    c.read("work", &[], true);
    let secret = airboss_sync::Secret::new(fake_token("work"));
    assert!(!format!("{secret:?}").contains("gho_"));
    assert!(!format!("{:?}", gh.calls()).contains(&fake_token("work")));
}
