use super::*;

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| value.to_string()).collect()
}

#[test]
fn task_commands_require_explicit_retry_and_edit_fences() {
    assert!(parse(&args(&["add", "--title", "Todo"])).is_err());
    assert!(parse(&args(&[
        "add",
        "--title",
        "Todo",
        "--idempotency-key",
        "request-1",
        "--json"
    ]))
    .is_ok());
    assert!(parse(&args(&["update", "--task-id", "task", "--status", "done"])).is_err());
    assert!(parse(&args(&[
        "update",
        "--task-id",
        "task",
        "--expected-revision",
        "1",
        "--status",
        "done"
    ]))
    .is_ok());
    assert!(parse(&args(&["list", "--title", "ignored"])).is_err());
    assert!(parse(&args(&["list", "--status", "running"])).is_err());
    assert!(parse(&args(&[
        "show",
        "--task-id",
        "task",
        "--project",
        "agent:r:p",
        "--workspace",
        "."
    ]))
    .is_err());
    assert!(parse(&args(&["list", "--limit", "-1"])).is_err());
    assert!(parse(&args(&[
        "complete",
        "--task-id",
        "task",
        "--expected-revision",
        "1"
    ]))
    .is_err());
    assert!(parse(&args(&[
        "complete",
        "--task-id",
        "task",
        "--expected-revision",
        "1",
        "--summary",
        "Implemented and validated"
    ]))
    .is_ok());
    assert!(parse(&args(&[
        "add",
        "--title",
        "Todo",
        "--idempotency-key",
        "key",
        "--description",
        "text",
        "--description-file",
        "file"
    ]))
    .is_err());
}

#[test]
fn task_commands_are_reachable_through_the_public_cli() {
    assert!(matches!(
        crate::cli_action(args(&["task", "list", "--project", "agent:r:p"])),
        crate::CliAction::Task(_)
    ));
    match crate::cli_action(args(&["task", "--help"])) {
        crate::CliAction::Exit { code, stdout, .. } => {
            assert_eq!(code, 0);
            assert!(stdout.contains("--idempotency-key"));
        }
        _ => panic!("task help must not perform network I/O"),
    }
}
