use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Parser, Subcommand};
use common::api_bindings::HostOperationsStatus;
use serde::{Deserialize, Serialize};

#[derive(Parser)]
#[command(version, about = "Moonlight Web release promotion coordinator")]
struct Cli {
    #[arg(long, default_value = "ops/promotion_inventory.json")]
    inventory: PathBuf,

    #[command(subcommand)]
    command: CoordinatorCommand,
}

#[derive(Subcommand)]
enum CoordinatorCommand {
    Status,
    PreparePromotion {
        #[arg(long)]
        target_environment: String,
        #[arg(long)]
        promotion_group: Option<String>,
    },
    ApplyPromotion {
        #[arg(long)]
        target_environment: String,
        #[arg(long)]
        promotion_group: Option<String>,
        #[arg(long)]
        max_hosts: Option<usize>,
    },
}

#[derive(Debug, Clone, Deserialize)]
struct PromotionInventoryDocument {
    schema_version: u32,
    inventory_name: String,
    policy_name: String,
    workspace_root: Option<String>,
    hosts: Vec<PromotionInventoryHost>,
}

#[derive(Debug, Clone, Deserialize)]
struct PromotionInventoryHost {
    bundle_name: String,
    promotion_group: String,
    expected_environment: Option<String>,
    bundle_root: String,
}

#[derive(Debug, Serialize)]
struct PromotionInventoryStatus {
    schema_version: u32,
    inventory_name: String,
    policy_name: String,
    source_path: String,
    workspace_root: String,
    host_count: usize,
    group_count: usize,
    hosts: Vec<PromotionInventoryHostStatus>,
    groups: Vec<PromotionInventoryGroupSummary>,
}

#[derive(Debug, Serialize)]
struct PromotionInventoryHostStatus {
    bundle_name: String,
    promotion_group: String,
    bundle_root: String,
    expected_environment: Option<String>,
    expected_environment_match: Option<bool>,
    status_ok: bool,
    status_error: Option<String>,
    health_grade: Option<String>,
    current_environment: Option<String>,
    current_release_id: Option<String>,
    next_promotion_target_environment: Option<String>,
    next_promotion_readiness: Option<String>,
    next_promotion_reason: Option<String>,
}

#[derive(Debug, Serialize)]
struct PromotionInventoryGroupSummary {
    promotion_group: String,
    host_count: usize,
    healthy_host_count: usize,
    promotion_ready_host_count: usize,
}

#[derive(Debug, Serialize)]
struct FleetPromotionPreparation {
    schema_version: u32,
    inventory_name: String,
    policy_name: String,
    target_environment: String,
    promotion_group: Option<String>,
    selected_host_count: usize,
    blocked_host_count: usize,
    selected_hosts: Vec<FleetPromotionHostDecision>,
    blocked_hosts: Vec<FleetPromotionHostDecision>,
}

#[derive(Debug, Serialize)]
struct FleetPromotionHostDecision {
    bundle_name: String,
    promotion_group: String,
    bundle_root: String,
    current_environment: Option<String>,
    target_environment: String,
    decision: String,
    reason: String,
}

#[derive(Debug, Serialize)]
struct FleetPromotionApplyResult {
    schema_version: u32,
    inventory_name: String,
    policy_name: String,
    target_environment: String,
    promotion_group: Option<String>,
    attempted_host_count: usize,
    applied_host_count: usize,
    skipped_host_count: usize,
    failed_host_count: usize,
    applied_hosts: Vec<FleetPromotionApplyHostResult>,
    skipped_hosts: Vec<FleetPromotionHostDecision>,
    failed_hosts: Vec<FleetPromotionApplyHostResult>,
}

#[derive(Debug, Serialize)]
struct FleetPromotionApplyHostResult {
    bundle_name: String,
    promotion_group: String,
    bundle_root: String,
    target_environment: String,
    status: String,
    reason: String,
    stdout: Option<serde_json::Value>,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("{err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let inventory = read_inventory(&cli.inventory)?;

    match cli.command {
        CoordinatorCommand::Status => {
            let result = collect_inventory_status(&inventory)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        CoordinatorCommand::PreparePromotion {
            target_environment,
            promotion_group,
        } => {
            let result = prepare_fleet_promotion(&inventory, &target_environment, promotion_group)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        CoordinatorCommand::ApplyPromotion {
            target_environment,
            promotion_group,
            max_hosts,
        } => {
            let result =
                apply_fleet_promotion(&inventory, &target_environment, promotion_group, max_hosts)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
    }

    Ok(())
}

fn read_inventory(path: &Path) -> Result<ResolvedInventory> {
    let absolute_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .context("failed to resolve current directory")?
            .join(path)
    };
    let absolute_path = fs::canonicalize(&absolute_path).unwrap_or(absolute_path);
    let raw = fs::read_to_string(&absolute_path)
        .with_context(|| format!("failed to read inventory {}", absolute_path.display()))?;
    let document = serde_json::from_str::<PromotionInventoryDocument>(&raw)
        .with_context(|| format!("failed to parse inventory {}", absolute_path.display()))?;
    if document.hosts.is_empty() {
        bail!(
            "inventory {} does not contain any hosts",
            absolute_path.display()
        );
    }

    let base_dir = absolute_path
        .parent()
        .ok_or_else(|| anyhow!("inventory path {} has no parent", absolute_path.display()))?
        .to_path_buf();
    let workspace_root = resolve_inventory_workspace_root(
        &base_dir,
        document.workspace_root.as_deref().unwrap_or("."),
    );

    let hosts = document
        .hosts
        .into_iter()
        .map(|host| {
            let resolved_bundle_root =
                resolve_inventory_bundle_root(&workspace_root, &host.bundle_root);
            ResolvedInventoryHost {
                bundle_name: host.bundle_name,
                promotion_group: host.promotion_group,
                expected_environment: host.expected_environment.map(|value| normalize_env(&value)),
                bundle_root: host.bundle_root,
                resolved_bundle_root,
            }
        })
        .collect();

    Ok(ResolvedInventory {
        schema_version: document.schema_version,
        inventory_name: document.inventory_name,
        policy_name: document.policy_name,
        source_path: absolute_path,
        workspace_root,
        hosts,
    })
}

#[derive(Debug, Clone)]
struct ResolvedInventory {
    schema_version: u32,
    inventory_name: String,
    policy_name: String,
    source_path: PathBuf,
    workspace_root: PathBuf,
    hosts: Vec<ResolvedInventoryHost>,
}

#[derive(Debug, Clone)]
struct ResolvedInventoryHost {
    bundle_name: String,
    promotion_group: String,
    expected_environment: Option<String>,
    bundle_root: String,
    resolved_bundle_root: PathBuf,
}

fn resolve_inventory_workspace_root(base_dir: &Path, configured: &str) -> PathBuf {
    let candidate = PathBuf::from(configured);
    let resolved = if candidate.is_absolute() {
        candidate
    } else {
        base_dir.join(candidate)
    };
    fs::canonicalize(&resolved).unwrap_or(resolved)
}

fn resolve_inventory_bundle_root(workspace_root: &Path, configured: &str) -> PathBuf {
    let candidate = PathBuf::from(configured);
    if candidate.is_absolute() {
        return candidate;
    }
    workspace_root.join(candidate)
}

fn normalize_env(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "dev" => "development".to_string(),
        "prod" => "production".to_string(),
        other => other.to_string(),
    }
}

fn query_host_status(bundle_root: &Path) -> Result<HostOperationsStatus> {
    let installer_path = bundle_root.join("host-installer.exe");
    let output = Command::new(&installer_path)
        .arg("--bundle-root")
        .arg(bundle_root)
        .arg("status")
        .output()
        .with_context(|| format!("failed to run {}", installer_path.display()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "host-installer status failed for {}: {}",
            bundle_root.display(),
            stderr.trim()
        );
    }

    serde_json::from_slice::<HostOperationsStatus>(&output.stdout).with_context(|| {
        format!(
            "failed to parse host status JSON for {}",
            bundle_root.display()
        )
    })
}

fn collect_inventory_status(inventory: &ResolvedInventory) -> Result<PromotionInventoryStatus> {
    let mut hosts = Vec::new();
    for host in &inventory.hosts {
        match query_host_status(&host.resolved_bundle_root) {
            Ok(status) => hosts.push(PromotionInventoryHostStatus {
                bundle_name: host.bundle_name.clone(),
                promotion_group: host.promotion_group.clone(),
                bundle_root: host.bundle_root.clone(),
                expected_environment: host.expected_environment.clone(),
                expected_environment_match: Some(
                    host.expected_environment.as_deref()
                        == status
                            .release_info
                            .as_ref()
                            .and_then(|value| value.deployment_environment.as_deref()),
                ),
                status_ok: true,
                status_error: None,
                health_grade: Some(status.health_grade),
                current_environment: status
                    .release_info
                    .as_ref()
                    .and_then(|value| value.deployment_environment.clone()),
                current_release_id: status.current_release_id,
                next_promotion_target_environment: status.next_promotion_target_environment,
                next_promotion_readiness: Some(status.next_promotion_readiness),
                next_promotion_reason: Some(status.next_promotion_reason),
            }),
            Err(err) => hosts.push(PromotionInventoryHostStatus {
                bundle_name: host.bundle_name.clone(),
                promotion_group: host.promotion_group.clone(),
                bundle_root: host.bundle_root.clone(),
                expected_environment: host.expected_environment.clone(),
                expected_environment_match: None,
                status_ok: false,
                status_error: Some(format!("{err:#}")),
                health_grade: None,
                current_environment: None,
                current_release_id: None,
                next_promotion_target_environment: None,
                next_promotion_readiness: None,
                next_promotion_reason: None,
            }),
        }
    }

    let mut group_map = std::collections::BTreeMap::<String, PromotionInventoryGroupSummary>::new();
    for host in &hosts {
        let entry = group_map.entry(host.promotion_group.clone()).or_insert(
            PromotionInventoryGroupSummary {
                promotion_group: host.promotion_group.clone(),
                host_count: 0,
                healthy_host_count: 0,
                promotion_ready_host_count: 0,
            },
        );
        entry.host_count += 1;
        if host.health_grade.as_deref() == Some("healthy") {
            entry.healthy_host_count += 1;
        }
        if host.next_promotion_readiness.as_deref() == Some("ready") {
            entry.promotion_ready_host_count += 1;
        }
    }

    Ok(PromotionInventoryStatus {
        schema_version: inventory.schema_version,
        inventory_name: inventory.inventory_name.clone(),
        policy_name: inventory.policy_name.clone(),
        source_path: inventory.source_path.display().to_string(),
        workspace_root: inventory.workspace_root.display().to_string(),
        host_count: hosts.len(),
        group_count: group_map.len(),
        hosts,
        groups: group_map.into_values().collect(),
    })
}

fn prepare_fleet_promotion(
    inventory: &ResolvedInventory,
    target_environment: &str,
    promotion_group: Option<String>,
) -> Result<FleetPromotionPreparation> {
    let target_environment = normalize_env(target_environment);
    let filter_group = promotion_group.map(|value| value.trim().to_string());
    let mut selected_hosts = Vec::new();
    let mut blocked_hosts = Vec::new();

    for host in &inventory.hosts {
        if let Some(group) = filter_group.as_deref() {
            if !host.promotion_group.eq_ignore_ascii_case(group) {
                continue;
            }
        }

        match query_host_status(&host.resolved_bundle_root) {
            Ok(status) => {
                let decision = decide_fleet_promotion(
                    &host.bundle_name,
                    &host.promotion_group,
                    &host.bundle_root,
                    host.expected_environment.as_deref(),
                    &target_environment,
                    &status,
                );
                if decision.decision == "selected" {
                    selected_hosts.push(decision);
                } else {
                    blocked_hosts.push(decision);
                }
            }
            Err(err) => {
                blocked_hosts.push(FleetPromotionHostDecision {
                    bundle_name: host.bundle_name.clone(),
                    promotion_group: host.promotion_group.clone(),
                    bundle_root: host.bundle_root.clone(),
                    current_environment: None,
                    target_environment: target_environment.clone(),
                    decision: "blocked".to_string(),
                    reason: format!("status query failed: {err:#}"),
                });
            }
        }
    }

    Ok(FleetPromotionPreparation {
        schema_version: inventory.schema_version,
        inventory_name: inventory.inventory_name.clone(),
        policy_name: inventory.policy_name.clone(),
        target_environment,
        promotion_group: filter_group,
        selected_host_count: selected_hosts.len(),
        blocked_host_count: blocked_hosts.len(),
        selected_hosts,
        blocked_hosts,
    })
}

fn decide_fleet_promotion(
    bundle_name: &str,
    promotion_group: &str,
    bundle_root: &str,
    expected_environment: Option<&str>,
    target_environment: &str,
    status: &HostOperationsStatus,
) -> FleetPromotionHostDecision {
    let current_environment = status
        .release_info
        .as_ref()
        .and_then(|value| value.deployment_environment.clone());

    if let Some(expected_environment) = expected_environment {
        if !expected_environment.eq_ignore_ascii_case(target_environment) {
            return FleetPromotionHostDecision {
                bundle_name: bundle_name.to_string(),
                promotion_group: promotion_group.to_string(),
                bundle_root: bundle_root.to_string(),
                current_environment,
                target_environment: target_environment.to_string(),
                decision: "blocked".to_string(),
                reason: format!(
                    "inventory expects environment {expected_environment}, not {target_environment}"
                ),
            };
        }
    }

    if status.next_promotion_target_environment.as_deref() != Some(target_environment) {
        return FleetPromotionHostDecision {
            bundle_name: bundle_name.to_string(),
            promotion_group: promotion_group.to_string(),
            bundle_root: bundle_root.to_string(),
            current_environment,
            target_environment: target_environment.to_string(),
            decision: "blocked".to_string(),
            reason: if status.next_promotion_reason.trim().is_empty() {
                format!(
                    "next target is {:?}, not {target_environment}",
                    status.next_promotion_target_environment
                )
            } else {
                status.next_promotion_reason.clone()
            },
        };
    }

    if !status
        .next_promotion_readiness
        .eq_ignore_ascii_case("ready")
    {
        return FleetPromotionHostDecision {
            bundle_name: bundle_name.to_string(),
            promotion_group: promotion_group.to_string(),
            bundle_root: bundle_root.to_string(),
            current_environment,
            target_environment: target_environment.to_string(),
            decision: "blocked".to_string(),
            reason: status.next_promotion_reason.clone(),
        };
    }

    FleetPromotionHostDecision {
        bundle_name: bundle_name.to_string(),
        promotion_group: promotion_group.to_string(),
        bundle_root: bundle_root.to_string(),
        current_environment,
        target_environment: target_environment.to_string(),
        decision: "selected".to_string(),
        reason: "host is ready for promotion".to_string(),
    }
}

fn apply_fleet_promotion(
    inventory: &ResolvedInventory,
    target_environment: &str,
    promotion_group: Option<String>,
    max_hosts: Option<usize>,
) -> Result<FleetPromotionApplyResult> {
    let preparation =
        prepare_fleet_promotion(inventory, target_environment, promotion_group.clone())?;
    let mut applied_hosts = Vec::new();
    let mut failed_hosts = Vec::new();
    let mut skipped_hosts = Vec::new();
    let mut attempted = 0usize;

    for (index, selected) in preparation.selected_hosts.iter().enumerate() {
        if let Some(limit) = max_hosts {
            if index >= limit {
                skipped_hosts.push(FleetPromotionHostDecision {
                    bundle_name: selected.bundle_name.clone(),
                    promotion_group: selected.promotion_group.clone(),
                    bundle_root: selected.bundle_root.clone(),
                    current_environment: selected.current_environment.clone(),
                    target_environment: selected.target_environment.clone(),
                    decision: "skipped".to_string(),
                    reason: format!("max_hosts limit {limit} reached"),
                });
                continue;
            }
        }

        attempted += 1;
        let bundle_root =
            resolve_inventory_bundle_root(&inventory.workspace_root, &selected.bundle_root);
        let installer_path = bundle_root.join("host-installer.exe");
        let output = Command::new(&installer_path)
            .arg("--bundle-root")
            .arg(&bundle_root)
            .arg("apply-release-promotion")
            .arg("--target-environment")
            .arg(&selected.target_environment)
            .output()
            .with_context(|| format!("failed to run {}", installer_path.display()))?;

        if output.status.success() {
            let stdout = serde_json::from_slice::<serde_json::Value>(&output.stdout).ok();
            applied_hosts.push(FleetPromotionApplyHostResult {
                bundle_name: selected.bundle_name.clone(),
                promotion_group: selected.promotion_group.clone(),
                bundle_root: selected.bundle_root.clone(),
                target_environment: selected.target_environment.clone(),
                status: "succeeded".to_string(),
                reason: "promotion command completed successfully".to_string(),
                stdout,
            });
        } else {
            failed_hosts.push(FleetPromotionApplyHostResult {
                bundle_name: selected.bundle_name.clone(),
                promotion_group: selected.promotion_group.clone(),
                bundle_root: selected.bundle_root.clone(),
                target_environment: selected.target_environment.clone(),
                status: "failed".to_string(),
                reason: String::from_utf8_lossy(&output.stderr).trim().to_string(),
                stdout: None,
            });
        }
    }

    skipped_hosts.extend(preparation.blocked_hosts);

    Ok(FleetPromotionApplyResult {
        schema_version: inventory.schema_version,
        inventory_name: inventory.inventory_name.clone(),
        policy_name: inventory.policy_name.clone(),
        target_environment: preparation.target_environment,
        promotion_group,
        attempted_host_count: attempted,
        applied_host_count: applied_hosts.len(),
        skipped_host_count: skipped_hosts.len(),
        failed_host_count: failed_hosts.len(),
        applied_hosts,
        skipped_hosts,
        failed_hosts,
    })
}
