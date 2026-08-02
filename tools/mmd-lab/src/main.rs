//! Trusted local lab CLI. Candidate archives must never supply this binary.

mod archive;
mod install;
mod pr_summary;
mod report;
mod ssh;
mod verify;

use std::fs;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

use clap::{Parser, Subcommand};
use mmd_engine::bench::BenchPolicy;

use archive::{pack_tree, write_archive_file, ArchiveBlob};
use install::{
    default_binary_path, default_manifest_path, install_current_exe, self_check, sha256_file,
};
use pr_summary::{render_pr_summary, retain_run_dir, ValidateMode};
use report::{ClaimedStats, HostEvidence, HostManifest, LabConfig, RawTrialSamples};
use ssh::{run_fake_matrix, FakeAgent};
use verify::verify_matrix;

#[derive(Debug, Parser)]
#[command(
    name = "mmd-lab",
    version,
    about = "Trusted local validation lab coordinator"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Inspect lab host readiness (shell; full checks in later tickets)
    Doctor,
    /// Install this binary as trusted coordinator + write out-of-tree digest
    Install {
        /// Destination binary path (default: $HOME/.local/bin/mmd-lab)
        #[arg(long)]
        bin: Option<PathBuf>,
        /// Trusted manifest path (default: $HOME/.config/mmd-lab/trusted-tools.toml)
        #[arg(long)]
        manifest: Option<PathBuf>,
    },
    /// Verify running binary path + digest against trusted manifest
    SelfCheck {
        #[arg(long)]
        manifest: Option<PathBuf>,
    },
    /// Build content-addressed source archive
    Archive {
        /// Source tree root (default: cwd)
        #[arg(long)]
        root: Option<PathBuf>,
        /// Output directory for `.mmdarc` (default: cwd)
        #[arg(long)]
        out_dir: Option<PathBuf>,
    },
    /// Validate candidate via agents; coordinator recomputes verdict
    Validate {
        /// `pr` rejects dirty worktree; `local-dev` must be explicit
        #[arg(long, value_parser = ["pr", "local-dev"])]
        mode: String,
        /// Workspace / source root to archive
        #[arg(long)]
        root: Option<PathBuf>,
        /// Optional git commit label bound into summary
        #[arg(long)]
        commit: Option<String>,
        /// Lab config TOML (optional; fake defaults used when absent)
        #[arg(long)]
        config: Option<PathBuf>,
        /// Directory of fake agent JSON fixtures (enables fake transport)
        #[arg(long)]
        fake_fixtures: Option<PathBuf>,
        /// Skip install self-check (dev tests only; never for ops)
        #[arg(long, default_value_t = false)]
        skip_self_check: bool,
        /// Trusted manifest for self-check
        #[arg(long)]
        manifest: Option<PathBuf>,
        /// Local retention root for full evidence JSON
        #[arg(long)]
        retain_dir: Option<PathBuf>,
        /// Write PR summary markdown here
        #[arg(long)]
        summary_out: Option<PathBuf>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Commands::Doctor => {
            println!("doctor: shell only; host checks land in later tickets");
            ExitCode::SUCCESS
        }
        Commands::Install { bin, manifest } => cmd_install(bin, manifest),
        Commands::SelfCheck { manifest } => cmd_self_check(manifest),
        Commands::Archive { root, out_dir } => cmd_archive(root, out_dir),
        Commands::Validate {
            mode,
            root,
            commit,
            config,
            fake_fixtures,
            skip_self_check,
            manifest,
            retain_dir,
            summary_out,
        } => cmd_validate(ValidateArgs {
            mode,
            root,
            commit,
            config,
            fake_fixtures,
            skip_self_check,
            manifest,
            retain_dir,
            summary_out,
        }),
    }
}

fn cmd_install(bin: Option<PathBuf>, manifest: Option<PathBuf>) -> ExitCode {
    let dest = match bin.or_else(|| default_binary_path().ok()) {
        Some(p) => p,
        None => {
            eprintln!("install: cannot resolve default binary path (HOME unset?)");
            return ExitCode::from(2);
        }
    };
    let man = match manifest.or_else(|| default_manifest_path().ok()) {
        Some(p) => p,
        None => {
            eprintln!("install: cannot resolve default manifest path (HOME unset?)");
            return ExitCode::from(2);
        }
    };
    match install_current_exe(&dest, &man) {
        Ok(m) => {
            println!("installed {}", m.binary_path);
            println!("sha256 {}", m.sha256);
            println!("manifest {}", man.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("install failed: {e}");
            ExitCode::from(1)
        }
    }
}

fn cmd_self_check(manifest: Option<PathBuf>) -> ExitCode {
    let man = match manifest.or_else(|| default_manifest_path().ok()) {
        Some(p) => p,
        None => {
            eprintln!("self-check: cannot resolve default manifest path (HOME unset?)");
            return ExitCode::from(2);
        }
    };
    match self_check(&man) {
        Ok(ok) => {
            println!("self-check ok");
            println!("binary {}", ok.binary_path.display());
            println!("sha256 {}", ok.sha256);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("self-check failed: {e}");
            ExitCode::from(1)
        }
    }
}

fn cmd_archive(root: Option<PathBuf>, out_dir: Option<PathBuf>) -> ExitCode {
    let root = root.unwrap_or_else(|| PathBuf::from("."));
    let out_dir = out_dir.unwrap_or_else(|| PathBuf::from("."));
    match pack_tree(&root).and_then(|blob| {
        let path = write_archive_file(&out_dir, &blob)?;
        Ok((blob, path))
    }) {
        Ok((blob, path)) => {
            println!("archive_sha256 {}", blob.sha256);
            println!("content_id {}", blob.content_id());
            println!("path {}", path.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("archive failed: {e}");
            ExitCode::from(1)
        }
    }
}

struct ValidateArgs {
    mode: String,
    root: Option<PathBuf>,
    commit: Option<String>,
    config: Option<PathBuf>,
    fake_fixtures: Option<PathBuf>,
    skip_self_check: bool,
    manifest: Option<PathBuf>,
    retain_dir: Option<PathBuf>,
    summary_out: Option<PathBuf>,
}

fn cmd_validate(args: ValidateArgs) -> ExitCode {
    let mode = match ValidateMode::parse(&args.mode) {
        Some(m) => m,
        None => {
            eprintln!("validate: mode must be pr|local-dev");
            return ExitCode::from(2);
        }
    };

    if !args.skip_self_check {
        let man = match args.manifest.clone().or_else(|| default_manifest_path().ok()) {
            Some(p) => p,
            None => {
                eprintln!("validate: manifest path unresolved");
                return ExitCode::from(2);
            }
        };
        if let Err(e) = self_check(&man) {
            eprintln!("validate: self-check failed (no dispatch): {e}");
            return ExitCode::from(1);
        }
    }

    let root = args.root.unwrap_or_else(|| PathBuf::from("."));
    let dirty = match git_dirty(&root) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("validate: worktree check failed: {e}");
            return ExitCode::from(3);
        }
    };
    if mode == ValidateMode::Pr && dirty {
        eprintln!("validate: PR mode rejects dirty worktree");
        return ExitCode::from(1);
    }
    if mode == ValidateMode::LocalDev {
        eprintln!("validate: local-dev mode (explicit); dirty_worktree={dirty}");
    }

    let blob = match pack_tree(&root) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("validate: archive failed: {e}");
            return ExitCode::from(3);
        }
    };
    println!("archive_sha256 {}", blob.sha256);

    let (evidences, required) = match load_agents(
        &blob,
        args.fake_fixtures.as_deref(),
        args.config.as_deref(),
    )
    {
        Ok(v) => v,
        Err(e) => {
            eprintln!("validate: agent matrix failed: {e}");
            return ExitCode::from(1);
        }
    };

    let required_refs: Vec<&str> = required.iter().map(String::as_str).collect();
    let policy = BenchPolicy::production();
    let matrix = verify_matrix(&blob.sha256, &evidences, &required_refs, &policy);

    let commit = args.commit.or_else(|| git_head(&root).ok());
    let summary = render_pr_summary(mode, commit.as_deref(), dirty, &matrix);
    println!("{summary}");

    if let Some(out) = args.summary_out {
        if let Some(parent) = out.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Err(e) = fs::write(&out, &summary) {
            eprintln!("validate: write summary failed: {e}");
            return ExitCode::from(3);
        }
    }

    if let Some(retain) = args.retain_dir {
        if let Err(e) = retain_evidence(&retain, &blob, &evidences, &summary) {
            eprintln!("validate: retain failed: {e}");
            return ExitCode::from(3);
        }
        println!(
            "retained {}",
            retain_run_dir(&retain, &blob.sha256).display()
        );
    }

    // Optional: print running binary digest for audit.
    if let Ok(exe) = std::env::current_exe()
        && let Ok(d) = sha256_file(&exe)
    {
        println!("coordinator_sha256 {d}");
    }

    match matrix.overall {
        mmd_engine::bench::VerdictStatus::Pass => ExitCode::SUCCESS,
        mmd_engine::bench::VerdictStatus::Inconclusive => ExitCode::from(2),
        mmd_engine::bench::VerdictStatus::Fail => ExitCode::from(1),
        mmd_engine::bench::VerdictStatus::Error => ExitCode::from(3),
        mmd_engine::bench::VerdictStatus::Recorded => ExitCode::SUCCESS,
    }
}

fn load_agents(
    blob: &ArchiveBlob,
    fake_fixtures: Option<&std::path::Path>,
    config: Option<&std::path::Path>,
) -> Result<(Vec<HostEvidence>, Vec<String>), String> {
    if let Some(dir) = fake_fixtures {
        let mut agents = load_fake_agents(dir)?;
        let required: Vec<String> = agents.iter().map(|a| a.id.clone()).collect();
        let evidences = run_fake_matrix(blob, &mut agents).map_err(|e| e.to_string())?;
        return Ok((evidences, required));
    }

    if let Some(cfg_path) = config {
        let text = fs::read_to_string(cfg_path).map_err(|e| e.to_string())?;
        let cfg: LabConfig = toml::from_str(&text).map_err(|e| e.to_string())?;
        let mut agents = Vec::new();
        let mut required = Vec::new();
        for a in &cfg.agents {
            required.push(a.id.clone());
            match a.transport.as_str() {
                "fake" => {
                    let fix = a
                        .fixture
                        .as_ref()
                        .ok_or_else(|| format!("agent {} fake transport needs fixture", a.id))?;
                    let path = cfg_path
                        .parent()
                        .unwrap_or_else(|| std::path::Path::new("."))
                        .join(fix);
                    agents.push(FakeAgent::load_fixture(&path).map_err(|e| e.to_string())?);
                }
                other => {
                    return Err(format!(
                        "agent {} transport `{other}` unsupported in this ticket (use fake)",
                        a.id
                    ));
                }
            }
        }
        let evidences = run_fake_matrix(blob, &mut agents).map_err(|e| e.to_string())?;
        return Ok((evidences, required));
    }

    // Built-in 3-agent pass fixtures (no files).
    let mut agents = default_fake_matrix_agents();
    let required: Vec<String> = agents.iter().map(|a| a.id.clone()).collect();
    let evidences = run_fake_matrix(blob, &mut agents).map_err(|e| e.to_string())?;
    Ok((evidences, required))
}

fn load_fake_agents(dir: &std::path::Path) -> Result<Vec<FakeAgent>, String> {
    // Top-level *.json only (skip negative/ subdirs).
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().and_then(|x| x.to_str()) == Some("json"))
        .collect();
    paths.sort();
    if paths.is_empty() {
        return Err(format!("no .json fixtures in {}", dir.display()));
    }
    let mut agents = Vec::new();
    for p in paths {
        agents.push(FakeAgent::load_fixture(&p).map_err(|e| e.to_string())?);
    }
    Ok(agents)
}

fn default_fake_matrix_agents() -> Vec<FakeAgent> {
    vec![
        FakeAgent::from_evidence(synthetic_pass_evidence(
            "ubuntu-ref",
            "linux-x86_64",
            "vulkan",
            "Ubuntu 24.04",
            10.0,
        )),
        FakeAgent::from_evidence(synthetic_pass_evidence(
            "windows-ref",
            "windows-x86_64",
            "d3d12",
            "Windows 11 25H2",
            11.0,
        )),
        FakeAgent::from_evidence(synthetic_pass_evidence(
            "macos-ref",
            "macos-arm64",
            "metal",
            "macOS 15",
            9.5,
        )),
    ]
}

fn synthetic_pass_evidence(
    id: &str,
    platform: &str,
    backend: &str,
    os_build: &str,
    base_ms: f64,
) -> HostEvidence {
    let mut e = HostEvidence::new(HostManifest::new(id, platform, backend, os_build), "pending");
    e.submitted_frames = 420;
    e.completed_frames = 420;
    e.max_in_flight = 2;
    e.project_rust_alloc_count = 0;
    for i in 0..7u32 {
        let ms = base_ms + f64::from(i) * 0.02;
        e.raw_trials.push(RawTrialSamples {
            agent_count: mmd_engine::bench::GATE_AGENT_COUNT,
            trial_index: i,
            frame_service_ms: vec![ms; 128],
        });
    }
    // Honest claimed stats = recompute.
    if let Ok(agg) = verify::recompute_gate_aggregate(&e) {
        e.claimed = Some(ClaimedStats {
            median_p95_ms: agg.median_p95_ms,
            median_p99_ms: agg.median_p99_ms,
            verdict: "pass".into(),
        });
    }
    e
}

fn retain_evidence(
    retain_root: &std::path::Path,
    blob: &ArchiveBlob,
    evidences: &[HostEvidence],
    summary: &str,
) -> Result<(), String> {
    let dir = retain_run_dir(retain_root, &blob.sha256);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    write_archive_file(&dir, blob).map_err(|e| e.to_string())?;
    for (i, e) in evidences.iter().enumerate() {
        let name = format!("{:02}-{}.json", i, e.host_manifest.host_id);
        let path = dir.join(name);
        let json = e.to_json_pretty().map_err(|e| e.to_string())?;
        fs::write(path, json).map_err(|e| e.to_string())?;
    }
    fs::write(dir.join("pr_summary.md"), summary).map_err(|e| e.to_string())?;
    Ok(())
}

fn git_dirty(root: &std::path::Path) -> Result<bool, String> {
    let out = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("git status: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        // Non-git trees: PR mode cannot prove clean; local-dev treats as dirty.
        if stderr.contains("not a git repository") {
            return Ok(true);
        }
        return Err(format!("git status failed: {stderr}"));
    }
    Ok(!out.stdout.is_empty())
}

fn git_head(root: &std::path::Path) -> Result<String, String> {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("git rev-parse: {e}"))?;
    if !out.status.success() {
        return Err("git rev-parse failed".into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}
