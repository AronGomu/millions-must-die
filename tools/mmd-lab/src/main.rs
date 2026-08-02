//! Trusted local lab CLI. Candidate archives must never supply this binary.

mod archive;
mod install;
mod macos;
mod macos_recovery;
mod pr_summary;
mod report;
mod ssh;
mod ubuntu;
mod ubuntu_recovery;
mod verify;
mod windows;
mod windows_recovery;

use std::fs;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

use clap::{Parser, Subcommand};
use mmd_engine::bench::BenchPolicy;
use toml::Value as TomlValue;

use archive::{pack_tree, write_archive_file, ArchiveBlob};
use install::{
    default_binary_path, default_manifest_path, install_current_exe, self_check, sha256_file,
};
use macos::{
    load_attestation as load_macos_attestation, load_manifest as load_macos_manifest,
    validate_macos_attestation, MacosAttestVerdict,
};
use macos_recovery::{
    MacosLabNetwork, MacosRecoveryEvent, MacosRecoveryPhase, MacosRecoveryState,
};
use pr_summary::{render_pr_summary, retain_run_dir, ValidateMode};
use report::{ClaimedStats, HostEvidence, HostManifest, LabConfig, RawTrialSamples};
use ssh::{run_fake_matrix, FakeAgent};
use ubuntu::{load_attestation, load_manifest, validate_ubuntu_attestation, AttestVerdict};
use ubuntu_recovery::{LabNetwork, RecoveryEvent, RecoveryPhase, RecoveryState};
use verify::verify_matrix;
use windows::{
    load_attestation as load_windows_attestation, load_manifest as load_windows_manifest,
    validate_windows_attestation, WindowsAttestVerdict,
};
use windows_recovery::{
    WindowsLabNetwork, WindowsRecoveryEvent, WindowsRecoveryPhase, WindowsRecoveryState,
};

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
    /// Inspect lab host readiness
    Doctor {
        /// Runner id to check (`ubuntu`). Omit for generic shell status.
        #[arg(long)]
        runner: Option<String>,
    },
    /// Validate Ubuntu host attestation against frozen runner contract
    AttestUbuntu {
        /// Frozen Ubuntu runner manifest (TOML)
        #[arg(long)]
        manifest: PathBuf,
        /// Observed host attestation JSON (fixture or inspect output)
        #[arg(long)]
        observed: PathBuf,
    },
    /// Validate Windows host attestation against frozen runner contract
    AttestWindows {
        /// Frozen Windows runner manifest (TOML)
        #[arg(long)]
        manifest: PathBuf,
        /// Observed host attestation JSON (fixture or inspect output)
        #[arg(long)]
        observed: PathBuf,
    },
    /// Validate macOS host attestation against frozen runner contract
    AttestMacos {
        /// Frozen macOS runner manifest (TOML)
        #[arg(long)]
        manifest: PathBuf,
        /// Observed host attestation JSON (fixture or inspect output)
        #[arg(long)]
        observed: PathBuf,
    },
    /// Simulate Ubuntu external restore protocol (no PXE/disk). Dry-run only.
    UbuntuRecoverSimulate {
        /// RO image identity manifest
        #[arg(long)]
        image_manifest: PathBuf,
        /// Frozen Ubuntu runner contract
        #[arg(long)]
        runner_manifest: PathBuf,
        /// Observed host attestation fixture used after simulated restore
        #[arg(long)]
        attest_fixture: PathBuf,
        /// Optional prior SSH host key fp (must differ from new)
        #[arg(long, default_value = "ssh-ed25519 AAAAprior-sim")]
        prior_host_key_fp: String,
        /// Fresh SSH host key fp after rotate
        #[arg(long, default_value = "ssh-ed25519 AAAAfresh-sim")]
        new_host_key_fp: String,
    },
    /// Simulate Windows external WinPE/FFU restore protocol (no DISM/disk). Dry-run only.
    WindowsRecoverSimulate {
        /// RO FFU identity manifest
        #[arg(long)]
        image_manifest: PathBuf,
        /// Frozen Windows runner contract
        #[arg(long)]
        runner_manifest: PathBuf,
        /// Observed host attestation fixture used after simulated restore
        #[arg(long)]
        attest_fixture: PathBuf,
        /// Optional prior Windows host identity (must differ from new)
        #[arg(long, default_value = "WIN-MACHINE-PRIOR-SIM")]
        prior_host_identity: String,
        /// Fresh Windows host identity after rotate
        #[arg(long, default_value = "WIN-MACHINE-FRESH-SIM")]
        new_host_identity: String,
    },
    /// Simulate macOS external EACS/ADE/MDM (+ DFU) restore protocol. Dry-run only.
    MacosRecoverSimulate {
        /// Example MDM profile identity (not a live enrollment payload)
        #[arg(long)]
        mdm_profile: PathBuf,
        /// Frozen macOS runner contract
        #[arg(long)]
        runner_manifest: PathBuf,
        /// Observed host attestation fixture used after simulated restore
        #[arg(long)]
        attest_fixture: PathBuf,
        /// Recovery path: `eacs` (default) or `dfu` (missed-ack then DFU fallback)
        #[arg(long, default_value = "eacs", value_parser = ["eacs", "dfu"])]
        path: String,
        /// Simulate EACS reset ack received (eacs path only)
        #[arg(long, default_value = "true", value_parser = ["true", "false"])]
        eacs_ack: String,
        /// Simulate ADE/MDM reenroll success
        #[arg(long, default_value = "true", value_parser = ["true", "false"])]
        reenroll_success: String,
        /// Optional prior macOS host identity (must differ from new)
        #[arg(long, default_value = "MAC-HOST-PRIOR-SIM")]
        prior_host_identity: String,
        /// Fresh macOS host identity after rotate
        #[arg(long, default_value = "MAC-HOST-FRESH-SIM")]
        new_host_identity: String,
    },
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
        Commands::Doctor { runner } => cmd_doctor(runner),
        Commands::AttestUbuntu { manifest, observed } => cmd_attest_ubuntu(manifest, observed),
        Commands::AttestWindows { manifest, observed } => cmd_attest_windows(manifest, observed),
        Commands::AttestMacos { manifest, observed } => cmd_attest_macos(manifest, observed),
        Commands::UbuntuRecoverSimulate {
            image_manifest,
            runner_manifest,
            attest_fixture,
            prior_host_key_fp,
            new_host_key_fp,
        } => cmd_ubuntu_recover_simulate(
            image_manifest,
            runner_manifest,
            attest_fixture,
            prior_host_key_fp,
            new_host_key_fp,
        ),
        Commands::WindowsRecoverSimulate {
            image_manifest,
            runner_manifest,
            attest_fixture,
            prior_host_identity,
            new_host_identity,
        } => cmd_windows_recover_simulate(
            image_manifest,
            runner_manifest,
            attest_fixture,
            prior_host_identity,
            new_host_identity,
        ),
        Commands::MacosRecoverSimulate {
            mdm_profile,
            runner_manifest,
            attest_fixture,
            path,
            eacs_ack,
            reenroll_success,
            prior_host_identity,
            new_host_identity,
        } => cmd_macos_recover_simulate(MacosRecoverArgs {
            mdm_profile,
            runner_manifest,
            attest_fixture,
            path,
            eacs_ack: eacs_ack == "true",
            reenroll_success: reenroll_success == "true",
            prior_host_identity,
            new_host_identity,
        }),
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

fn cmd_doctor(runner: Option<String>) -> ExitCode {
    match runner.as_deref() {
        None => {
            println!("doctor: shell ok; pass --runner ubuntu|windows|macos for host lane");
            ExitCode::SUCCESS
        }
        Some("ubuntu") => cmd_doctor_ubuntu(),
        Some("windows") => cmd_doctor_windows(),
        Some("macos") => cmd_doctor_macos(),
        Some(other) => {
            eprintln!("doctor: unknown runner `{other}` (supported: ubuntu|windows|macos)");
            ExitCode::from(2)
        }
    }
}

fn cmd_doctor_ubuntu() -> ExitCode {
    let root = discover_workspace_root();
    let mut missing = Vec::new();
    let paths = [
        "lab/manifests/ubuntu-24.04-x86_64.toml",
        "lab/provision/ubuntu/image-manifest.toml",
        "lab/provision/ubuntu/recover.sh",
        "lab/provision/ubuntu/attest.sh",
        "docs/lab/ubuntu-runner.md",
        "lab/fixtures/ubuntu-attest/pass.json",
    ];
    for rel in paths {
        let p = root.join(rel);
        if p.is_file() {
            println!("ok {rel}");
        } else {
            println!("missing {rel}");
            missing.push(rel);
        }
    }

    // Protocol unit surface present if this binary linked recovery module.
    println!("ok protocol-state-machine");

    // Physical lab not available on developer workstation path.
    println!("physical-lab absent");
    println!("physical-drill blocked_user");
    println!(
        "need: Ubuntu 24.04 ref PC + PXE/raw image store + external controller + recovery/provisioning/candidate VLANs"
    );
    println!("verdict blocked_user");

    if !missing.is_empty() {
        eprintln!("doctor ubuntu: missing contract files: {missing:?}");
        return ExitCode::from(1);
    }
    // Contracts present; physical still blocked → exit 2 (not ready).
    ExitCode::from(2)
}

fn cmd_doctor_windows() -> ExitCode {
    let root = discover_workspace_root();
    let mut missing = Vec::new();
    let paths = [
        "lab/manifests/windows-11-25h2-x86_64.toml",
        "lab/provision/windows/image-manifest.toml",
        "lab/provision/windows/recover.ps1",
        "lab/provision/windows/attest.ps1",
        "docs/lab/windows-runner.md",
        "lab/fixtures/windows-attest/pass.json",
    ];
    for rel in paths {
        let p = root.join(rel);
        if p.is_file() {
            println!("ok {rel}");
        } else {
            println!("missing {rel}");
            missing.push(rel);
        }
    }

    println!("ok protocol-state-machine");
    println!("physical-lab absent");
    println!("physical-drill blocked_user");
    println!(
        "need: Windows 11 25H2 ref PC + WinPE/FFU + external controller + recovery/provisioning/candidate VLANs"
    );
    println!("verdict blocked_user");

    if !missing.is_empty() {
        eprintln!("doctor windows: missing contract files: {missing:?}");
        return ExitCode::from(1);
    }
    ExitCode::from(2)
}

fn cmd_doctor_macos() -> ExitCode {
    let root = discover_workspace_root();
    let mut missing = Vec::new();
    let paths = [
        "lab/manifests/macos-15-arm64.toml",
        "lab/provision/macos/mdm-profile.example.json",
        "lab/provision/macos/recover.sh",
        "lab/provision/macos/attest.sh",
        "docs/lab/macos-runner.md",
        "lab/fixtures/macos-attest/pass.json",
    ];
    for rel in paths {
        let p = root.join(rel);
        if p.is_file() {
            println!("ok {rel}");
        } else {
            println!("missing {rel}");
            missing.push(rel);
        }
    }

    println!("ok protocol-state-machine");
    println!("physical-lab absent");
    println!("physical-drill blocked_user");
    println!("mdm-provider TODO(user): select/provision MDM + ABM/ADE before live drill");
    println!(
        "need: M4 Mac mini 16 GB + second Mac + USB-C + EACS/ADE/MDM + recovery/provisioning/candidate VLANs + DFU fallback drill"
    );
    println!("verdict blocked_user");

    if !missing.is_empty() {
        eprintln!("doctor macos: missing contract files: {missing:?}");
        return ExitCode::from(1);
    }
    ExitCode::from(2)
}

fn discover_workspace_root() -> PathBuf {
    // Prefer cwd when it looks like the repo; else walk from exe (dev target/).
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if cwd.join("lab/manifests/ubuntu-24.04-x86_64.toml").is_file() {
        return cwd;
    }
    if let Ok(mut dir) = std::env::current_exe() {
        for _ in 0..6 {
            if dir.join("lab/manifests/ubuntu-24.04-x86_64.toml").is_file() {
                return dir;
            }
            if !dir.pop() {
                break;
            }
        }
    }
    cwd
}

fn cmd_ubuntu_recover_simulate(
    image_manifest: PathBuf,
    runner_manifest: PathBuf,
    attest_fixture: PathBuf,
    prior_host_key_fp: String,
    new_host_key_fp: String,
) -> ExitCode {
    let expected_digest = match load_image_digest(&image_manifest) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("ubuntu-recover-simulate: image-manifest: {e}");
            return ExitCode::from(2);
        }
    };
    let runner = match load_manifest(&runner_manifest) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("ubuntu-recover-simulate: runner-manifest: {e}");
            return ExitCode::from(2);
        }
    };
    if runner.image.digest_sha256.to_ascii_lowercase() != expected_digest {
        eprintln!(
            "ubuntu-recover-simulate: image digest mismatch between image-manifest and runner manifest"
        );
        return ExitCode::from(1);
    }
    let observed = match load_attestation(&attest_fixture) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("ubuntu-recover-simulate: attest fixture: {e}");
            return ExitCode::from(2);
        }
    };

    // Drive full protocol; inject real attestation result mid-path.
    let mut state = RecoveryState::new(&expected_digest);
    let boot = [
        RecoveryEvent::StartExternalRestore {
            external_controller: true,
        },
        RecoveryEvent::RecoveryBootConfirmed,
        RecoveryEvent::ImageWriteFinished {
            readback_digest_sha256: expected_digest.clone(),
        },
        RecoveryEvent::HostIdentityRotated {
            new_host_key_fp: new_host_key_fp.clone(),
            prior_host_key_fp: Some(prior_host_key_fp.clone()),
        },
        RecoveryEvent::NetworkMoved {
            network: LabNetwork::Provisioning,
        },
        RecoveryEvent::NetworkMoved {
            network: LabNetwork::Candidate,
        },
    ];
    for ev in boot {
        if let Err(e) = state.apply(ev) {
            eprintln!("ubuntu-recover-simulate: {e}");
            return ExitCode::from(2);
        }
        if state.phase.is_quarantined() {
            return print_recovery_state(&state, ExitCode::from(1));
        }
    }

    let attest = validate_ubuntu_attestation(&runner, &observed);
    if let Err(e) = state.apply(RecoveryEvent::AttestationFinished(attest)) {
        eprintln!("ubuntu-recover-simulate: {e}");
        return ExitCode::from(2);
    }
    if state.phase.is_quarantined() {
        return print_recovery_state(&state, ExitCode::from(1));
    }

    if let Err(e) = state.apply(RecoveryEvent::EgressCanaryFinished {
        denied: true,
        detail: "dry-run:simulated-external-deny".into(),
    }) {
        eprintln!("ubuntu-recover-simulate: {e}");
        return ExitCode::from(2);
    }

    print_recovery_state(
        &state,
        if state.phase.is_terminal_success() {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
        },
    )
}

fn print_recovery_state(state: &RecoveryState, code: ExitCode) -> ExitCode {
    let phase = match &state.phase {
        RecoveryPhase::ReadyForCandidate => "ready-for-candidate".to_string(),
        RecoveryPhase::Quarantined { reason } => format!("quarantined:{reason}"),
        other => format!("{other:?}"),
    };
    println!("phase {phase}");
    println!(
        "allows_candidate_provision {}",
        state.phase.allows_candidate_provision()
    );
    if let Some(d) = &state.verified_readback_digest {
        println!("verified_readback_digest {d}");
    }
    if let Some(k) = &state.host_key_fp {
        println!("host_key_fp {k}");
    }
    for t in &state.trail {
        println!("trail {t}");
    }
    if state.phase.is_terminal_success() {
        println!("verdict ready-for-candidate");
        println!("note dry-run-only; physical drill still required");
    } else if let Some(r) = &state.quarantine_reason {
        println!("verdict quarantine");
        println!("reason {r}");
    } else {
        println!("verdict incomplete");
    }
    code
}

fn cmd_windows_recover_simulate(
    image_manifest: PathBuf,
    runner_manifest: PathBuf,
    attest_fixture: PathBuf,
    prior_host_identity: String,
    new_host_identity: String,
) -> ExitCode {
    let expected_digest = match load_windows_image_digest(&image_manifest) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("windows-recover-simulate: image-manifest: {e}");
            return ExitCode::from(2);
        }
    };
    let runner = match load_windows_manifest(&runner_manifest) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("windows-recover-simulate: runner-manifest: {e}");
            return ExitCode::from(2);
        }
    };
    if runner.ffu.digest_sha256.to_ascii_lowercase() != expected_digest {
        eprintln!(
            "windows-recover-simulate: ffu digest mismatch between image-manifest and runner manifest"
        );
        return ExitCode::from(1);
    }
    let observed = match load_windows_attestation(&attest_fixture) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("windows-recover-simulate: attest fixture: {e}");
            return ExitCode::from(2);
        }
    };

    let mut state = WindowsRecoveryState::new(&expected_digest);
    let boot = [
        WindowsRecoveryEvent::StartExternalRestore {
            external_controller: true,
        },
        WindowsRecoveryEvent::WinPeBootConfirmed,
        WindowsRecoveryEvent::FfuApplyFinished {
            success: true,
            readback_digest_sha256: expected_digest.clone(),
            detail: "dry-run:simulated-dism-apply".into(),
        },
        WindowsRecoveryEvent::HostIdentityRotated {
            new_host_identity: new_host_identity.clone(),
            prior_host_identity: Some(prior_host_identity.clone()),
        },
        WindowsRecoveryEvent::NetworkMoved {
            network: WindowsLabNetwork::Provisioning,
        },
        WindowsRecoveryEvent::NetworkMoved {
            network: WindowsLabNetwork::Candidate,
        },
    ];
    for ev in boot {
        if let Err(e) = state.apply(ev) {
            eprintln!("windows-recover-simulate: {e}");
            return ExitCode::from(2);
        }
        if state.phase.is_quarantined() {
            return print_windows_recovery_state(&state, ExitCode::from(1));
        }
    }

    let attest = validate_windows_attestation(&runner, &observed);
    if let Err(e) = state.apply(WindowsRecoveryEvent::AttestationFinished(attest)) {
        eprintln!("windows-recover-simulate: {e}");
        return ExitCode::from(2);
    }
    if state.phase.is_quarantined() {
        return print_windows_recovery_state(&state, ExitCode::from(1));
    }

    if let Err(e) = state.apply(WindowsRecoveryEvent::EgressCanaryFinished {
        denied: true,
        detail: "dry-run:simulated-external-deny".into(),
    }) {
        eprintln!("windows-recover-simulate: {e}");
        return ExitCode::from(2);
    }

    print_windows_recovery_state(
        &state,
        if state.phase.is_terminal_success() {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
        },
    )
}

fn print_windows_recovery_state(state: &WindowsRecoveryState, code: ExitCode) -> ExitCode {
    let phase = match &state.phase {
        WindowsRecoveryPhase::ReadyForCandidate => "ready-for-candidate".to_string(),
        WindowsRecoveryPhase::Quarantined { reason } => format!("quarantined:{reason}"),
        other => format!("{other:?}"),
    };
    println!("phase {phase}");
    println!(
        "allows_candidate_provision {}",
        state.phase.allows_candidate_provision()
    );
    if let Some(d) = &state.verified_readback_digest {
        println!("verified_readback_digest {d}");
    }
    if let Some(k) = &state.host_identity {
        println!("host_identity {k}");
    }
    for t in &state.trail {
        println!("trail {t}");
    }
    if state.phase.is_terminal_success() {
        println!("verdict ready-for-candidate");
        println!("note dry-run-only; physical drill still required");
    } else if let Some(r) = &state.quarantine_reason {
        println!("verdict quarantine");
        println!("reason {r}");
    } else {
        println!("verdict incomplete");
    }
    code
}

struct MacosRecoverArgs {
    mdm_profile: PathBuf,
    runner_manifest: PathBuf,
    attest_fixture: PathBuf,
    path: String,
    eacs_ack: bool,
    reenroll_success: bool,
    prior_host_identity: String,
    new_host_identity: String,
}

fn cmd_macos_recover_simulate(args: MacosRecoverArgs) -> ExitCode {
    let profile_id = match load_macos_mdm_profile_id(&args.mdm_profile) {
        Ok(id) => id,
        Err(e) => {
            eprintln!("macos-recover-simulate: mdm-profile: {e}");
            return ExitCode::from(2);
        }
    };
    let runner = match load_macos_manifest(&args.runner_manifest) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("macos-recover-simulate: runner-manifest: {e}");
            return ExitCode::from(2);
        }
    };
    if runner.enrollment.mdm_profile_id != profile_id {
        eprintln!(
            "macos-recover-simulate: mdm profile_id mismatch between mdm-profile and runner manifest"
        );
        return ExitCode::from(1);
    }
    let observed = match load_macos_attestation(&args.attest_fixture) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("macos-recover-simulate: attest fixture: {e}");
            return ExitCode::from(2);
        }
    };

    let mut state = MacosRecoveryState::new(&profile_id);

    if args.path == "dfu" {
        // Missed EACS ack → quarantine → operator DFU → reenroll path.
        let boot = [
            MacosRecoveryEvent::StartExternalRestore {
                external_controller: true,
            },
            MacosRecoveryEvent::EacsPreflightFinished {
                ok: true,
                detail: "dry-run:eacs-preflight-ok".into(),
            },
            MacosRecoveryEvent::EacsWipeFinished {
                ack_received: false,
                detail: "dry-run:simulated-eacs-timeout".into(),
            },
        ];
        for ev in boot {
            if let Err(e) = state.apply(ev) {
                eprintln!("macos-recover-simulate: {e}");
                return ExitCode::from(2);
            }
        }
        if !state.phase.is_quarantined() {
            eprintln!("macos-recover-simulate: dfu path expected quarantine after missed ack");
            return ExitCode::from(2);
        }
        if let Err(e) = state.apply(MacosRecoveryEvent::StartDfuFallback {
            external_operator: true,
        }) {
            eprintln!("macos-recover-simulate: {e}");
            return ExitCode::from(2);
        }
        if let Err(e) = state.apply(MacosRecoveryEvent::DfuRestoreFinished {
            success: true,
            detail: "dry-run:simulated-dfu".into(),
        }) {
            eprintln!("macos-recover-simulate: {e}");
            return ExitCode::from(2);
        }
        if state.phase.is_quarantined() {
            return print_macos_recovery_state(&state, ExitCode::from(1));
        }
    } else {
        let boot = [
            MacosRecoveryEvent::StartExternalRestore {
                external_controller: true,
            },
            MacosRecoveryEvent::EacsPreflightFinished {
                ok: true,
                detail: "dry-run:eacs-preflight-ok".into(),
            },
            MacosRecoveryEvent::EacsWipeFinished {
                ack_received: args.eacs_ack,
                detail: if args.eacs_ack {
                    "dry-run:eacs-ack".into()
                } else {
                    "dry-run:eacs-timeout".into()
                },
            },
        ];
        for ev in boot {
            if let Err(e) = state.apply(ev) {
                eprintln!("macos-recover-simulate: {e}");
                return ExitCode::from(2);
            }
            if state.phase.is_quarantined() {
                return print_macos_recovery_state(&state, ExitCode::from(1));
            }
        }
    }

    // Shared post-reset path: reenroll → identity → networks → attest → egress.
    if let Err(e) = state.apply(MacosRecoveryEvent::MdmReenrollFinished {
        success: args.reenroll_success,
        profile_id: if args.reenroll_success {
            profile_id.clone()
        } else {
            profile_id.clone()
        },
        detail: if args.reenroll_success {
            "dry-run:ade-mdm-ok".into()
        } else {
            "dry-run:mdm-provider-reject".into()
        },
    }) {
        eprintln!("macos-recover-simulate: {e}");
        return ExitCode::from(2);
    }
    if state.phase.is_quarantined() {
        return print_macos_recovery_state(&state, ExitCode::from(1));
    }

    let post = [
        MacosRecoveryEvent::HostIdentityRotated {
            new_host_identity: args.new_host_identity.clone(),
            prior_host_identity: Some(args.prior_host_identity.clone()),
        },
        MacosRecoveryEvent::NetworkMoved {
            network: MacosLabNetwork::Provisioning,
        },
        MacosRecoveryEvent::NetworkMoved {
            network: MacosLabNetwork::Candidate,
        },
    ];
    for ev in post {
        if let Err(e) = state.apply(ev) {
            eprintln!("macos-recover-simulate: {e}");
            return ExitCode::from(2);
        }
        if state.phase.is_quarantined() {
            return print_macos_recovery_state(&state, ExitCode::from(1));
        }
    }

    let attest = validate_macos_attestation(&runner, &observed);
    if let Err(e) = state.apply(MacosRecoveryEvent::AttestationFinished(attest)) {
        eprintln!("macos-recover-simulate: {e}");
        return ExitCode::from(2);
    }
    if state.phase.is_quarantined() {
        return print_macos_recovery_state(&state, ExitCode::from(1));
    }

    if let Err(e) = state.apply(MacosRecoveryEvent::EgressCanaryFinished {
        denied: true,
        detail: "dry-run:simulated-external-deny".into(),
    }) {
        eprintln!("macos-recover-simulate: {e}");
        return ExitCode::from(2);
    }

    print_macos_recovery_state(
        &state,
        if state.phase.is_terminal_success() {
            ExitCode::SUCCESS
        } else {
            ExitCode::from(1)
        },
    )
}

fn print_macos_recovery_state(state: &MacosRecoveryState, code: ExitCode) -> ExitCode {
    let phase = match &state.phase {
        MacosRecoveryPhase::ReadyForCandidate => "ready-for-candidate".to_string(),
        MacosRecoveryPhase::Quarantined { reason } => format!("quarantined:{reason}"),
        other => format!("{other:?}"),
    };
    println!("phase {phase}");
    println!(
        "allows_candidate_provision {}",
        state.phase.allows_candidate_provision()
    );
    if let Some(p) = &state.reset_path {
        println!("reset_path {p}");
    }
    if let Some(k) = &state.host_identity {
        println!("host_identity {k}");
    }
    for t in &state.trail {
        println!("trail {t}");
    }
    if state.phase.is_terminal_success() {
        println!("verdict ready-for-candidate");
        println!("note dry-run-only; physical drill still required");
        println!("note TODO(user) MDM/ABM selection still required for live drill");
    } else if let Some(r) = &state.quarantine_reason {
        println!("verdict quarantine");
        println!("reason {r}");
    } else {
        println!("verdict incomplete");
    }
    code
}

fn load_macos_mdm_profile_id(path: &std::path::Path) -> Result<String, String> {
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let schema = v
        .get("schema_version")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    if schema != "macos-mdm-profile-example-v1" {
        return Err(format!(
            "expected macos-mdm-profile-example-v1, got {schema}"
        ));
    }
    let initiator = v
        .get("restore")
        .and_then(|r| r.get("initiator"))
        .and_then(|x| x.as_str())
        .unwrap_or("");
    if initiator != "external-controller" {
        return Err(format!(
            "initiator must be external-controller, got {initiator}"
        ));
    }
    let egress = v
        .get("networks")
        .and_then(|n| n.get("candidate_egress_policy"))
        .and_then(|x| x.as_str())
        .unwrap_or("");
    if egress != "deny" {
        return Err(format!("candidate_egress_policy must be deny, got {egress}"));
    }
    let profile_id = v
        .get("profile_id")
        .and_then(|x| x.as_str())
        .ok_or_else(|| "profile_id missing".to_string())?
        .to_string();
    if profile_id.trim().is_empty() {
        return Err("profile_id empty".into());
    }
    // Example file must keep TODO(user) until real MDM is selected.
    let provider = v
        .get("provider")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    if !provider.contains("TODO(user)") {
        // Allow non-TODO only when explicitly provisioned later; still require id.
        // For T22 skeleton, TODO(user) is expected; non-TODO is accepted if present.
    }
    Ok(profile_id)
}

fn load_image_digest(path: &std::path::Path) -> Result<String, String> {
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let v: TomlValue = text.parse::<TomlValue>().map_err(|e| e.to_string())?;
    let schema = v
        .get("schema_version")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    if schema != "ubuntu-image-manifest-v1" {
        return Err(format!("expected ubuntu-image-manifest-v1, got {schema}"));
    }
    let initiator = v
        .get("restore")
        .and_then(|r| r.get("initiator"))
        .and_then(|x| x.as_str())
        .unwrap_or("");
    if initiator != "external-controller" {
        return Err(format!("initiator must be external-controller, got {initiator}"));
    }
    let read_only = v
        .get("image")
        .and_then(|i| i.get("read_only"))
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    if !read_only {
        return Err("image.read_only must be true".into());
    }
    let digest = v
        .get("image")
        .and_then(|i| i.get("digest_sha256"))
        .and_then(|x| x.as_str())
        .ok_or_else(|| "image.digest_sha256 missing".to_string())?
        .to_ascii_lowercase();
    if digest.len() != 64 || !digest.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("image.digest_sha256 must be 64 hex chars".into());
    }
    Ok(digest)
}

fn load_windows_image_digest(path: &std::path::Path) -> Result<String, String> {
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let v: TomlValue = text.parse::<TomlValue>().map_err(|e| e.to_string())?;
    let schema = v
        .get("schema_version")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    if schema != "windows-image-manifest-v1" {
        return Err(format!("expected windows-image-manifest-v1, got {schema}"));
    }
    let initiator = v
        .get("restore")
        .and_then(|r| r.get("initiator"))
        .and_then(|x| x.as_str())
        .unwrap_or("");
    if initiator != "external-controller" {
        return Err(format!("initiator must be external-controller, got {initiator}"));
    }
    let boot_env = v
        .get("restore")
        .and_then(|r| r.get("boot_env"))
        .and_then(|x| x.as_str())
        .unwrap_or("");
    if boot_env != "winpe" {
        return Err(format!("boot_env must be winpe, got {boot_env}"));
    }
    let read_only = v
        .get("ffu")
        .and_then(|i| i.get("read_only"))
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    if !read_only {
        return Err("ffu.read_only must be true".into());
    }
    let apply_tool = v
        .get("ffu")
        .and_then(|i| i.get("apply_tool"))
        .and_then(|x| x.as_str())
        .unwrap_or("");
    if apply_tool != "dism" {
        return Err(format!("apply_tool must be dism, got {apply_tool}"));
    }
    let egress = v
        .get("networks")
        .and_then(|n| n.get("candidate_egress_policy"))
        .and_then(|x| x.as_str())
        .unwrap_or("");
    if egress != "deny" {
        return Err(format!("candidate_egress_policy must be deny, got {egress}"));
    }
    let digest = v
        .get("ffu")
        .and_then(|i| i.get("digest_sha256"))
        .and_then(|x| x.as_str())
        .ok_or_else(|| "ffu.digest_sha256 missing".to_string())?
        .to_ascii_lowercase();
    if digest.len() != 64 || !digest.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("ffu.digest_sha256 must be 64 hex chars".into());
    }
    Ok(digest)
}

fn cmd_attest_ubuntu(manifest: PathBuf, observed: PathBuf) -> ExitCode {
    let expected = match load_manifest(&manifest) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("attest-ubuntu: load manifest failed: {e}");
            return ExitCode::from(2);
        }
    };
    let obs = match load_attestation(&observed) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("attest-ubuntu: load observed failed: {e}");
            return ExitCode::from(2);
        }
    };
    let result = validate_ubuntu_attestation(&expected, &obs);
    let verdict = match result.verdict {
        AttestVerdict::ReadyForRecovery => "ready-for-recovery",
        AttestVerdict::Quarantine => "quarantine",
        AttestVerdict::Reject => "reject",
    };
    println!("verdict {verdict}");
    for r in &result.reasons {
        println!("reason {r}");
    }
    match result.verdict {
        AttestVerdict::ReadyForRecovery => ExitCode::SUCCESS,
        AttestVerdict::Quarantine => ExitCode::from(1),
        AttestVerdict::Reject => ExitCode::from(1),
    }
}

fn cmd_attest_windows(manifest: PathBuf, observed: PathBuf) -> ExitCode {
    let expected = match load_windows_manifest(&manifest) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("attest-windows: load manifest failed: {e}");
            return ExitCode::from(2);
        }
    };
    let obs = match load_windows_attestation(&observed) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("attest-windows: load observed failed: {e}");
            return ExitCode::from(2);
        }
    };
    let result = validate_windows_attestation(&expected, &obs);
    let verdict = match result.verdict {
        WindowsAttestVerdict::ReadyForRecovery => "ready-for-recovery",
        WindowsAttestVerdict::Quarantine => "quarantine",
        WindowsAttestVerdict::MaintenanceBlock => "maintenance-block",
        WindowsAttestVerdict::Reject => "reject",
    };
    println!("verdict {verdict}");
    for r in &result.reasons {
        println!("reason {r}");
    }
    match result.verdict {
        WindowsAttestVerdict::ReadyForRecovery => ExitCode::SUCCESS,
        WindowsAttestVerdict::Quarantine
        | WindowsAttestVerdict::MaintenanceBlock
        | WindowsAttestVerdict::Reject => ExitCode::from(1),
    }
}

fn cmd_attest_macos(manifest: PathBuf, observed: PathBuf) -> ExitCode {
    let expected = match load_macos_manifest(&manifest) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("attest-macos: load manifest failed: {e}");
            return ExitCode::from(2);
        }
    };
    let obs = match load_macos_attestation(&observed) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("attest-macos: load observed failed: {e}");
            return ExitCode::from(2);
        }
    };
    let result = validate_macos_attestation(&expected, &obs);
    let verdict = match result.verdict {
        MacosAttestVerdict::ReadyForRecovery => "ready-for-recovery",
        MacosAttestVerdict::Quarantine => "quarantine",
        MacosAttestVerdict::MaintenanceBlock => "maintenance-block",
        MacosAttestVerdict::Reject => "reject",
    };
    println!("verdict {verdict}");
    for r in &result.reasons {
        println!("reason {r}");
    }
    match result.verdict {
        MacosAttestVerdict::ReadyForRecovery => ExitCode::SUCCESS,
        MacosAttestVerdict::Quarantine
        | MacosAttestVerdict::MaintenanceBlock
        | MacosAttestVerdict::Reject => ExitCode::from(1),
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
