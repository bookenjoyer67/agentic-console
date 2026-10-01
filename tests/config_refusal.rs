//! The config refusal: an unread config and an unset required key are refused at the point of use,
//! never replaced by a default that names another project's runtime.
//!
//! Three properties, one per test:
//!
//! * a config path that does not exist is refused, and the refusal names the failed read;
//! * a config that was read but omits `console.container` (or `console.role_container_prefix`) is
//!   refused, and the refusal names the key;
//! * no default names another project's container or role-box prefix -- both are empty, so a config
//!   that cannot be read can never silently aim a run at the origin project's tree.
//!
//! These are the states the recorded incident hid: a fallback that looked like it worked. Each test
//! drives the real action builder, because "refused at the point of use" means the argv is never
//! built.

use std::fs;
use std::path::PathBuf;

use agentic_console::actions::{self, ActionKind, Guards};
use agentic_console::config::{Config, Console};

/// A scratch directory for one test, cleared first.
fn scratch(name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("scratch dir");
    path
}

/// A valid config whose `console` block is exactly `console`.
fn config_with_console_block(console: &str) -> String {
    format!(
        r#"{{
  "schema_version": 1,
  "project": {{"name": "fixture", "language": "rust", "repo_marker": "Cargo.toml"}},
  "console": {console}
}}"#
    )
}

#[test]
fn a_config_path_that_does_not_exist_is_refused_and_names_the_failed_read() {
    let repo = scratch("refusal-no-config");
    let missing = repo.join("agentic.config.json"); // never written
    let config = Config::load(&repo, Some(&missing));

    assert!(
        config.config_read.is_none(),
        "no config file was found, and the config records that rather than a fallback's path"
    );

    let refused = actions::build(&config, ActionKind::GateSelftest, "", Guards::default())
        .expect_err("an action that needs the container is refused");
    let expected = format!("agentic.config.json was not found at {}", missing.display());
    assert!(
        refused.contains("agentic.config.json was not found at"),
        "the refusal names the read that failed: {refused}"
    );
    assert!(
        refused.contains(&missing.display().to_string()),
        "the refusal names the path: {refused}"
    );
    assert_eq!(
        refused, expected,
        "the sentence is the single one this console owns"
    );

    // The config itself, asked for the key, gives the same sentence the action refused with.
    assert_eq!(config.config_refusal("console.container"), expected);
}

#[test]
fn a_config_that_omits_console_container_is_refused_and_names_the_key() {
    let repo = scratch("refusal-no-container");
    let path = repo.join("agentic.config.json");
    // Real JSON, a real console block -- just without `container`.
    fs::write(
        &path,
        config_with_console_block(r#"{"claude_command": "claude", "role_container_prefix": "x-"}"#),
    )
    .expect("the config file");
    let config = Config::load(&repo, None);

    assert!(
        config.from_file && config.config_read.is_some(),
        "the config was read: this is the empty-key state, not the missing-file one"
    );
    assert!(config.console.container.is_empty());

    let refused = actions::build(&config, ActionKind::GateSelftest, "", Guards::default())
        .expect_err("an action that needs the container is refused");
    assert_eq!(
        refused,
        format!("console.container is empty in {}", path.display()),
        "the refusal names the key and the config that lacks it"
    );
}

#[test]
fn a_config_that_omits_role_container_prefix_refuses_the_role_box_and_names_the_key() {
    let repo = scratch("refusal-no-prefix");
    let path = repo.join("agentic.config.json");
    // `container` is present and non-empty; only the prefix is missing, so the role box is the action
    // that must name it.
    fs::write(
        &path,
        config_with_console_block(r#"{"container": "a-container", "claude_command": "claude"}"#),
    )
    .expect("the config file");
    let config = Config::load(&repo, None);

    let refused = actions::build(
        &config,
        ActionKind::RoleBox,
        "tester cargo test",
        Guards::default(),
    )
    .expect_err("a role box with no prefix is refused");
    assert_eq!(
        refused,
        format!(
            "console.role_container_prefix is empty in {}",
            path.display()
        ),
        "the refusal names the key the role box needs"
    );
}

#[test]
fn no_default_names_another_projects_container_or_role_prefix() {
    let console = Console::default();
    assert!(
        console.container.is_empty(),
        "the default container is empty, not another project's"
    );
    assert!(
        console.role_container_prefix.is_empty(),
        "the default role-box prefix is empty"
    );
    assert!(console.evidence_dir_raw.is_empty());
    assert!(console.briefs_dir_raw.is_empty());
    for value in [&console.container, &console.role_container_prefix] {
        assert!(
            !value.contains("komun") && !value.contains("agent-rev"),
            "no default may name another project's runtime, found {value:?}"
        );
    }

    // The same holds for a repository whose config could not be read at all: the fallback names no
    // container and no prefix, so the point of use refuses instead of aiming a run somewhere.
    let repo = scratch("refusal-defaults");
    let config = Config::load(&repo, None);
    assert!(config.console.container.is_empty());
    assert!(config.console.role_container_prefix.is_empty());
}
