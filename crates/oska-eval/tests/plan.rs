//! `oska-eval plan`: a batch config read against the task set, the runs
//! the batch will make, and their estimated cost, printed before anything
//! runs. The task set is the `validate` fixture's (`greeting` and `token`
//! active, `fetch` deferred, five attempts each). Refresh the golden plan
//! with `UPDATE_GOLDEN=1 cargo test -p oska-eval --test plan` and review
//! the diff. No test calls a model.

mod common;

use common::{assert_golden, harness, Scratch};

const TASKS: &str = "tests/fixtures/validate/valid";
const CONFIG: &str = "tests/fixtures/plan/2026-11-20-hook.json";

/// Two tasks, two variants, five attempts: 20 runs at an estimated 0.50
/// US dollars each is 10.00, within the budget of 25. The batch is named
/// by the config file.
#[test]
fn a_plan_lists_every_run_and_estimates_its_cost() {
    let outcome = harness(&["plan", TASKS, CONFIG]);
    assert_eq!(outcome.code, 0, "{}{}", outcome.stdout, outcome.stderr);
    assert_golden("plan.json", &outcome.stdout);
    let plan = &outcome.stdout;
    assert!(plan.contains("\"batch\": \"2026-11-20-hook\""), "{plan}");
    for expected in [
        "\"runs\": 20,",
        "\"turns_at_most\": 800,",
        "\"seconds_at_most\": 18000,",
        "\"cost_usd\": 10.0000,",
        "\"within_budget\": true",
        "\"record\": \"token-no-web-5.json\"",
    ] {
        assert!(plan.contains(expected), "no {expected} in {plan}");
    }
}

/// Writes `config` as `<name>.json` in `scratch` and plans it.
fn plan_config(scratch: &Scratch, name: &str, config: &str) -> common::Outcome {
    let path = scratch.path().join(format!("{name}.json"));
    std::fs::write(&path, config).expect("should write the config");
    harness(&["plan", TASKS, path.to_str().expect("UTF-8 path")])
}

/// A batch whose estimate passes its budget is a failed check: the driver
/// will not start it.
#[test]
fn an_estimate_over_the_budget_fails_the_plan() {
    let scratch = Scratch::new();
    let outcome = plan_config(
        &scratch,
        "over",
        r#"{"schema": 1, "model": "example-model", "tasks": ["greeting"],
            "variants": [{"name": "base", "web": true}],
            "budget_usd": 2, "estimate_usd_per_run": 0.5}"#,
    );
    assert_eq!(outcome.code, 1, "{}", outcome.stdout);
    assert!(
        outcome.stdout.contains("\"cost_usd\": 2.5000")
            && outcome.stdout.contains("\"within_budget\": false"),
        "{}",
        outcome.stdout
    );
}

/// Each config that is wrong is bad input, with a message that says why.
#[test]
fn a_malformed_config_or_an_unknown_or_deferred_task_is_bad_input() {
    let base = |tasks: &str, variants: &str, extra: &str| {
        format!(
            r#"{{"schema": 1, "model": "example-model", "tasks": {tasks},
                "variants": {variants}, "budget_usd": 5,
                "estimate_usd_per_run": 0.5{extra}}}"#
        )
    };
    let one = r#"[{"name": "base", "web": true}]"#;
    let cases = [
        ("unknown", base(r#"["nope"]"#, one, ""), "no task `nope`"),
        ("deferred", base(r#"["fetch"]"#, one, ""), "deferred"),
        (
            "traversal",
            base(r#"["../valid/greeting"]"#, one, ""),
            "is not a task id",
        ),
        ("no-tasks", base("[]", one, ""), "`tasks` must list"),
        (
            "twice",
            base(r#"["greeting", "greeting"]"#, one, ""),
            "`greeting` is listed twice",
        ),
        (
            "variant-name",
            base(
                r#"["greeting"]"#,
                r#"[{"name": "a;touch x", "web": true}]"#,
                "",
            ),
            "variant `a;touch x`",
        ),
        (
            "variant-twice",
            base(
                r#"["greeting"]"#,
                r#"[{"name": "a", "web": true}, {"name": "a", "web": false}]"#,
                "",
            ),
            "the variant `a` is listed twice",
        ),
        (
            "variant-field",
            base(r#"["greeting"]"#, r#"[{"name": "a", "hook": true}]"#, ""),
            "unknown field `hook`",
        ),
        (
            "unknown-field",
            base(r#"["greeting"]"#, one, r#", "yes": true"#),
            "unknown field `yes`",
        ),
        (
            "duplicate-key",
            base(r#"["greeting"]"#, one, r#", "budget_usd": 500"#),
            "the key `budget_usd` appears twice",
        ),
        (
            "budget",
            base(r#"["greeting"]"#, one, "").replace("\"budget_usd\": 5", "\"budget_usd\": 0"),
            "`budget_usd` must be a number above 0",
        ),
        (
            "model",
            base(r#"["greeting"]"#, one, "").replace("example-model", "a model"),
            "`model` must be text with no space",
        ),
        ("not-json", "{".to_string(), "at line 1"),
    ];
    for (name, config, message) in cases {
        let scratch = Scratch::new();
        let outcome = plan_config(&scratch, name, &config);
        assert_eq!(outcome.code, 2, "{name}: {}", outcome.stdout);
        assert!(
            outcome.stdout.contains(message),
            "{name}: expected `{message}` in {}",
            outcome.stdout
        );
    }
}

/// The batch is named by the config file, so the name must be one a
/// batch directory can have: never a path that climbs out of the results.
#[test]
fn attack_a_config_file_whose_name_is_not_a_batch_id_is_bad_input() {
    let scratch = Scratch::new();
    let config =
        std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(CONFIG))
            .expect("the config");
    for name in ["Upper", "..", "a b", "x.y"] {
        let outcome = plan_config(&scratch, name, &config);
        assert_eq!(outcome.code, 2, "{name}: {}", outcome.stdout);
        assert!(
            outcome.stdout.contains("is not a batch id"),
            "{name}: {}",
            outcome.stdout
        );
    }
    let path = scratch.path().join("config.txt");
    std::fs::write(&path, &config).expect("should write");
    let outcome = harness(&["plan", TASKS, path.to_str().expect("UTF-8")]);
    assert_eq!(outcome.code, 2, "{}", outcome.stdout);
}
