# Shipwright

Shipwright is a Rust-based Arch Linux installer focused on clear architecture, predictable execution, and maintainability.

## Architecture

The codebase is split into three layers:

1. UI layer: src/ui
- Handles all interactive prompts and user-facing output.
- Collects installer inputs and returns structured data.
- Does not run OS commands or mutate the system.

2. Logic layer: src/logic
- Defines deterministic installer data and flow.
- Owns InstallState, InstallPlan, InstallStage, and validation.
- Builds the installation plan before execution.
- Does not perform direct side effects.

3. System layer: src/system
- Owns all side-effecting OS interactions.
- Runs commands, parses system state, formats and mounts disks, and executes arch-chroot operations.
- Contains no UI prompting logic.

## Core flow

The installer flow is:

UI input -> InstallState -> InstallPlan -> pipeline execution via System layer

Main entrypoint orchestration is in src/main.rs.

## Extensible pipeline model

Shipwright includes extension points in src/logic/mod.rs:

- StageExecutor
  - Abstracts execution of a single InstallStage.
  - Lets any compatible executor run stages.

- InstallExtension
  - Hook interface with:
    - before_stage(stage, state)
    - after_stage(stage, state)
  - Used for logging, metrics, tracing, policy checks, or dry-run reporting.

- PipelineEngine
  - Coordinates stage execution.
  - Runs before/after hooks around each stage.
  - Keeps core execution deterministic while allowing optional integrations.

- run_pipeline
  - Compatibility helper that executes a plan using PipelineEngine.

## Installation stages

The plan is executed in this order:

1. DetectHardware
2. SelectDiskLayout
3. PartitionDisks
4. FormatPartitions
5. MountFilesystem
6. InstallBaseSystem
7. ConfigureSystem
8. InstallBootloader
9. CreateUser

## Data model

Important logic types include:

- InstallState
  - Central state object for selected devices, options, credentials, and hardware context.

- Device, Partition, DiskLayout
  - Structured device model that avoids ad-hoc string device handling.

- InstallOptions and UserConfig
  - Structured installer choices and user data.

## Project goals

- Keep behavior stable while improving structure.
- Make side effects explicit and isolated.
- Keep planning deterministic and testable.
- Make future features safer to add.

## Build

From project root:

cargo check
