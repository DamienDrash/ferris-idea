//! Containers for IDECraft.
//!
//! Implements `SPEC-0119`: Docker through the Engine API over its Unix socket or TCP (no Docker
//! CLI needed), and Kubernetes through `kubectl … -o json`, which brings the user's kubeconfig,
//! credentials and plugins along; `SPEC-0240` shells and commands inside containers, `SPEC-0241`
//! published ports and port forwards, `SPEC-0242` Docker Compose and `SPEC-0243` engines (local
//! VMs, Docker CLI contexts, remote hosts over SSH).

pub mod compose;
mod docker;
mod engines;
pub mod exec;
#[doc(hidden)]
pub mod fake;
mod kube;
pub mod ports;

pub use docker::{
    Container, ContainerAction, DockerError, DockerHost, DockerInfo, EXEC_LIMIT, ExecOptions,
    ExecOutput, ExecSession, Image, SHELL_CHOOSER, demux_logs, find_host,
};
pub use engines::{Engine, choose, engines, engines_here};
pub use kube::{KubeContext, KubeError, KubeResource, Kubectl, age, exec_object, is_kubectl};
