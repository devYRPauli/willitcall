use wic_core::result::{RunResult, Status};
use wic_core::ScenarioCategory;

const CATEGORIES: [ScenarioCategory; 6] = [
    ScenarioCategory::SingleCall,
    ScenarioCategory::ParallelCalls,
    ScenarioCategory::Streaming,
    ScenarioCategory::ToolChoiceModes,
    ScenarioCategory::MultiTurn,
    ScenarioCategory::NegativeTrap,
];

pub fn render_report(result: &RunResult, color: bool) -> String {
    let mut rendered = String::new();
    if color {
        rendered.push_str("\x1b[1mwillitcall report\x1b[0m\n");
    } else {
        rendered.push_str("willitcall report\n");
    }

    for category in CATEGORIES {
        let outcomes = result
            .scenarios
            .iter()
            .filter(|outcome| outcome.category == category)
            .collect::<Vec<_>>();
        let passed = outcomes
            .iter()
            .filter(|outcome| outcome.status == Status::Pass)
            .count();
        let failed = outcomes
            .iter()
            .filter(|outcome| outcome.status == Status::Fail)
            .count();
        let errors = outcomes
            .iter()
            .filter(|outcome| outcome.status == Status::Error)
            .count();
        rendered.push_str(&format!(
            "{:<18}{} passed  {} failed  {} errors\n",
            category.to_string(),
            passed,
            failed,
            errors
        ));
    }

    let failures = result
        .scenarios
        .iter()
        .filter(|outcome| matches!(outcome.status, Status::Fail | Status::Error))
        .collect::<Vec<_>>();
    if !failures.is_empty() {
        rendered.push('\n');
    }
    for outcome in failures {
        let label = match outcome.status {
            Status::Fail => "FAIL ",
            Status::Error => "ERROR",
            _ => unreachable!("failures contains only failed or error outcomes"),
        };
        let reason = outcome
            .failure_reason
            .as_deref()
            .unwrap_or("no failure reason recorded");
        if color {
            rendered.push_str(&format!(
                "\x1b[31m{label}\x1b[0m {}: {reason}\n",
                outcome.id
            ));
        } else {
            rendered.push_str(&format!("{label} {}: {reason}\n", outcome.id));
        }
    }

    rendered.push_str(&format!(
        "TOTAL {} passed  {} failed  {} errors  {} skipped  {} total\n",
        result.totals.passed,
        result.totals.failed,
        result.totals.errors,
        result.totals.skipped,
        result.totals.total
    ));
    rendered
}
