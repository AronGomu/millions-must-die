//! T28 validation contract: the documented merge gate carries no perf threshold.
//!
//! Phase-0 acceptance is behavioural, not timed. Performance gating is retired
//! to the later optimization phase (single source of truth:
//! `docs/05-testing.md`). This file is the executable half of that contract —
//! it fails the moment a required-path command starts consuming a frame-time
//! or noise threshold again, or a doc resumes claiming a performance pass.
//!
//! It deliberately does *not* police historical numbers: measured results are
//! retained as history in `docs/technical-prototype-results.md`. Only the
//! documented required-command sections are scanned.

use std::path::PathBuf;
use std::process::Command;

/// Doc that owns the merge gate definition.
const CONTRACT_DOC: &str = "docs/05-testing.md";
/// Doc that mirrors the gate for contributors.
const README_DOC: &str = "README.md";
/// Doc retaining the retired measurements as history.
const RESULTS_DOC: &str = "docs/technical-prototype-results.md";

/// Heading text (any level) introducing the required-command list. Matched
/// case-insensitively and ignoring trailing punctuation.
const GATE_HEADING: &str = "Required merge gate";

/// Commands documented as required must never mention these. Any hit means a
/// timing/noise number is back on the required path.
const PERF_THRESHOLD_TOKENS: &[&str] = &[
    "p95",
    "p99",
    "nmad",
    "16.67",
    "25 ms",
    "frame-time",
    "frame time",
    "percentile",
    "median",
    "throughput",
    "latency",
    "fps",
];

/// Tools whose output is measurement evidence, never a merge verdict. Frozen
/// in place and still built + unit-tested, but not runnable as a gate.
const NON_GATING_COMMANDS: &[&str] = &[
    "bench",
    "mmd-lab",
    "release-freeze",
    "release-check",
    "calibrate",
    "pilot",
    "merge-gate",
];

/// Commands that must stay required — guards against satisfying this contract
/// by gutting the gate instead of retiring the perf part of it. Compared after
/// whitespace normalization, so reflowing the docs is not a failure.
const REQUIRED_COMMANDS: &[&str] = &[
    "cargo fmt --all -- --check",
    "cargo test --workspace --locked",
    "cargo clippy --workspace --all-targets --all-features -- -D warnings",
    "nix flake check",
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_doc(rel: &str) -> String {
    let path = repo_root().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// A markdown line tagged with whether it sits inside a fenced code block.
///
/// Fence state matters: a shell comment (`# note`) inside a ```` ``` ```` block
/// must never be mistaken for a markdown heading, or section extraction would
/// silently truncate the command list and let a perf gate slip back in unseen.
struct Line<'a> {
    text: &'a str,
    fenced: bool,
    /// The ```` ``` ```` delimiter itself, which is neither heading nor command.
    delimiter: bool,
}

fn scan(doc: &str) -> Vec<Line<'_>> {
    let mut fenced = false;
    doc.lines()
        .map(|text| {
            let delimiter = text.trim_start().starts_with("```");
            if delimiter {
                fenced = !fenced;
                Line {
                    text,
                    fenced: true,
                    delimiter,
                }
            } else {
                Line {
                    text,
                    fenced,
                    delimiter,
                }
            }
        })
        .collect()
}

/// Heading level of a markdown line, or `None` when it is not a heading.
/// Callers must only apply this to non-fenced lines.
fn heading_level(line: &str) -> Option<usize> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    (hashes > 0 && line[hashes..].starts_with(' ')).then_some(hashes)
}

/// Compare heading text tolerantly: case- and trailing-punctuation-insensitive.
fn heading_matches(line: &str, heading: &str) -> bool {
    let Some(level) = heading_level(line) else {
        return false;
    };
    let text = line[level..].trim().trim_end_matches([':', '.']);
    text.eq_ignore_ascii_case(heading)
}

/// Lines of the section whose heading text is `heading`, up to the next
/// non-fenced heading of the same or higher level. `None` when absent.
fn section<'a>(lines: &'a [Line<'a>], heading: &str) -> Option<&'a [Line<'a>]> {
    let start = lines
        .iter()
        .position(|l| !l.fenced && heading_matches(l.text, heading))?;
    let level = heading_level(lines[start].text).expect("matched line is a heading");
    let rest = &lines[start + 1..];
    let end = rest
        .iter()
        .position(|l| !l.fenced && heading_level(l.text).is_some_and(|found| found <= level))
        .unwrap_or(rest.len());
    Some(&rest[..end])
}

/// Every non-blank line inside fenced blocks of `body`, comments included —
/// an annotation reintroducing a threshold must be scanned too.
fn fenced_lines<'a>(body: &'a [Line<'a>]) -> Vec<&'a str> {
    body.iter()
        .filter(|l| l.fenced && !l.delimiter && !l.text.trim().is_empty())
        .map(|l| l.text.trim())
        .collect()
}

/// Collapse whitespace and drop a trailing `#` comment, so that reflowing or
/// annotating a documented command is not mistaken for dropping it.
fn normalize_command(raw: &str) -> String {
    let code = match raw.find(" #") {
        Some(idx) => &raw[..idx],
        None => raw,
    };
    code.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The parsed required-merge-gate section of one doc.
struct GateSection {
    /// Every non-blank fenced line, shell comments included.
    fenced: Vec<String>,
    /// Executable commands only, whitespace-normalized.
    commands: Vec<String>,
    /// Everything outside the fences, lowercased.
    prose: String,
}

fn gate_section(rel: &str) -> GateSection {
    let doc = read_doc(rel);
    let lines = scan(&doc);
    let body = section(&lines, GATE_HEADING)
        .unwrap_or_else(|| panic!("{rel} must document a '{GATE_HEADING}' section"));

    let fenced: Vec<String> = fenced_lines(body).iter().map(|l| l.to_string()).collect();
    let commands: Vec<String> = fenced
        .iter()
        .filter(|l| !l.starts_with('#'))
        .map(|l| normalize_command(l))
        .filter(|c| !c.is_empty())
        .collect();
    let prose = body
        .iter()
        .filter(|l| !l.fenced)
        .map(|l| l.text)
        .collect::<Vec<_>>()
        .join("\n")
        .to_ascii_lowercase();

    assert!(
        commands.len() >= REQUIRED_COMMANDS.len(),
        "{rel} '{GATE_HEADING}' lists {} commands, expected at least {}. \
         (If a command is present but unlisted here, check it is inside the \
         fenced block and not commented out.)",
        commands.len(),
        REQUIRED_COMMANDS.len()
    );
    for required in REQUIRED_COMMANDS {
        assert!(
            commands.iter().any(|c| c == required),
            "{rel} '{GATE_HEADING}' no longer lists required command `{required}` \
             (compared after whitespace normalization); retiring perf gating \
             must not gut the behavioural gate"
        );
    }
    GateSection {
        fenced,
        commands,
        prose,
    }
}

#[test]
fn gate_list_has_no_perf_thresholds() {
    for rel in [CONTRACT_DOC, README_DOC] {
        let gate = gate_section(rel);

        // Scan raw fenced lines (comments included) for threshold numbers...
        for line in &gate.fenced {
            let lower = line.to_ascii_lowercase();
            for token in PERF_THRESHOLD_TOKENS {
                assert!(
                    !lower.contains(token),
                    "{rel}: required-gate line `{line}` states perf threshold `{token}`; \
                     performance gating is retired to the optimization phase"
                );
            }
        }

        // ...and the executable commands for frozen, non-gating tooling.
        for command in &gate.commands {
            let lower = command.to_ascii_lowercase();
            for tool in NON_GATING_COMMANDS {
                assert!(
                    !lower.contains(tool),
                    "{rel}: required command `{command}` invokes non-gating perf tool `{tool}`; \
                     it is frozen developer tooling, not a merge gate"
                );
            }
        }

        // Prose in the gate section must not reintroduce a threshold either.
        for token in PERF_THRESHOLD_TOKENS {
            assert!(
                !gate.prose.contains(token),
                "{rel}: '{GATE_HEADING}' prose states perf threshold `{token}`"
            );
        }
    }
}

#[test]
fn gate_docs_state_perf_gating_is_retired() {
    // Negative assertions alone pass vacuously on an empty doc; require the
    // positive statement too.
    let contract = read_doc(CONTRACT_DOC).to_ascii_lowercase();
    assert!(
        contract.contains("optimization phase"),
        "{CONTRACT_DOC} must point perf work at the optimization phase"
    );
    assert!(
        contract.contains("not a gate"),
        "{CONTRACT_DOC} must label the frozen perf tooling 'not a gate'"
    );

    // Every doc that states what gates a merge points at the one source of
    // truth instead of restating a deferral note — or its own gate list — of
    // its own. CONTRIBUTING.md is included because it tells contributors what
    // the maintainer runs before merging.
    for rel in [
        README_DOC,
        "docs/02-prototype-roadmap.md",
        RESULTS_DOC,
        "CONTRIBUTING.md",
    ] {
        let doc = read_doc(rel);
        assert!(
            doc.contains("05-testing.md"),
            "{rel} must link the single validation-contract source of truth ({CONTRACT_DOC})"
        );
    }
}

#[test]
fn results_doc_is_superseded_history_not_a_claim() {
    let doc = read_doc(RESULTS_DOC);
    assert!(
        doc.to_ascii_uppercase().contains("SUPERSEDED"),
        "{RESULTS_DOC} must be marked superseded"
    );
    // History is retained, not deleted: the measured curve stays readable.
    assert!(
        doc.contains("1.670"),
        "{RESULTS_DOC} must retain the measured numbers as history"
    );
    for banned in [
        "gate open",
        "rerun required",
        "one quiet-host rerun remains",
    ] {
        assert!(
            !doc.to_ascii_lowercase().contains(banned),
            "{RESULTS_DOC} still presents `{banned}` as live phase-0 work; \
             the perf gate is retired, not pending"
        );
    }
}

#[test]
fn bench_binary_still_builds() {
    // Freeze-in-place proof: the bench module is not deleted, so the app binary
    // still links it and exposes the subcommand.
    let output = Command::new(env!("CARGO_BIN_EXE_millions_must_die"))
        .args(["bench", "--help"])
        .output()
        .expect("run app bench --help");
    assert!(
        output.status.success(),
        "bench --help exit nonzero: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.to_ascii_lowercase().contains("not a gate"),
        "bench must be labelled non-gating developer tooling:\n{stdout}"
    );
}
