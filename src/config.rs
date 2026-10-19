//! Startup policy. Inputs are explicit: no environment or network access here.
use bootleg_core::{BuildCommit, ContainerId, DisplayName, MetadataText, ProjectId, SecretString};
use std::collections::BTreeMap;
use url::Url;

pub type Environment = BTreeMap<String, String>;
#[derive(Default)]
pub struct Overrides {
    pub name: Option<String>,
    pub name_env: Option<String>,
}
#[derive(Debug)]
pub enum Startup {
    ExecThrough,
    Participate(Box<Configuration>),
}
#[derive(Debug)]
pub struct BuildBaseline {
    pub commit: Option<BuildCommit>,
    pub dirty: Option<bool>,
}
#[derive(Debug, Default)]
pub struct RuntimeClaims {
    pub pod_uid: Option<MetadataText>,
    pub pod_name: Option<MetadataText>,
    pub namespace: Option<MetadataText>,
    pub container_name: Option<MetadataText>,
    pub cluster_name: Option<MetadataText>,
}
#[derive(Debug)]
pub struct Configuration {
    pub endpoint: Url,
    pub project: ProjectId,
    pub credential: SecretString,
    pub container: ContainerId,
    pub name: DisplayName,
    pub baseline: BuildBaseline,
    pub runtime: RuntimeClaims,
}
// Errors identify settings, never their potentially secret values.
pub type ConfigError = &'static str;
fn boolean(env: &Environment, key: &'static str) -> Result<Option<bool>, ConfigError> {
    env.get(key)
        .map(|v| match v.as_str() {
            "true" => Ok(true),
            "false" => Ok(false),
            _ => Err(key),
        })
        .transpose()
}
fn metadata(env: &Environment, key: &'static str) -> Result<Option<MetadataText>, ConfigError> {
    env.get(key).map(|v| v.parse().map_err(|_| key)).transpose()
}
pub fn resolve(
    env: &Environment,
    flags: &Overrides,
    os_hostname: Option<&str>,
    container: ContainerId,
) -> Result<Startup, ConfigError> {
    if env.get("BOOTLEG_DISABLED").is_some_and(|v| v == "true") {
        return Ok(Startup::ExecThrough);
    }
    boolean(env, "BOOTLEG_DISABLED")?;
    let (project, credential) = match (
        env.get("BOOTLEG_PROJECT_ID"),
        env.get("BOOTLEG_PROJECT_TOKEN"),
    ) {
        (None, None) => return Ok(Startup::ExecThrough),
        (Some(p), Some(c)) => (p, c),
        _ => return Err("both BOOTLEG_PROJECT_ID and BOOTLEG_PROJECT_TOKEN are required"),
    };
    let project = project.parse().map_err(|_| "BOOTLEG_PROJECT_ID")?;
    if !credential.starts_with("project_")
        || credential.len() <= 8
        || credential.len() > 512
        || !credential.bytes().all(|b| b.is_ascii_graphic())
    {
        return Err("BOOTLEG_PROJECT_TOKEN");
    }
    let mut endpoint = Url::parse(env.get("BOOTLEG_API_URL").ok_or(
        "deployment connection settings are missing; restore your deployment configuration. For standalone/local testing, set BOOTLEG_API_URL (see README.md)",
    )?)
    .map_err(|_| "invalid connection override: BOOTLEG_API_URL must be a valid HTTPS origin")?;
    if !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
        || endpoint.host_str().is_none()
        || endpoint.path() != "/"
    {
        return Err(
            "invalid connection override: BOOTLEG_API_URL must be a credential-free origin without a path, query, or fragment",
        );
    }
    let scheme = match endpoint.scheme() {
        "https" => "wss",
        "http" if boolean(env, "BOOTLEG_ALLOW_INSECURE")? == Some(true) => "ws",
        _ => {
            return Err(
                "invalid connection override: BOOTLEG_API_URL requires HTTPS (local testing only: BOOTLEG_ALLOW_INSECURE=true)",
            );
        }
    };
    endpoint.set_scheme(scheme).map_err(|_| "BOOTLEG_API_URL")?;
    endpoint.set_path("/api/v1/agent/connect");
    let name = resolve_name(env, flags, os_hostname, &container)?;
    let commit = env
        .get("BOOTLEG_BUILD_COMMIT")
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().map_err(|_| "BOOTLEG_BUILD_COMMIT"))
        .transpose()?;
    Ok(Startup::Participate(Box::new(Configuration {
        endpoint,
        project,
        credential: SecretString::new(credential.clone()),
        container,
        name,
        baseline: BuildBaseline {
            commit,
            dirty: boolean(env, "BOOTLEG_BUILD_DIRTY")?,
        },
        runtime: RuntimeClaims {
            pod_uid: metadata(env, "POD_UID")?,
            pod_name: metadata(env, "POD_NAME")?.or(metadata(env, "KUBERNETES_POD_NAME")?),
            namespace: metadata(env, "POD_NAMESPACE")?,
            container_name: metadata(env, "BOOTLEG_CONTAINER_NAME")?,
            cluster_name: metadata(env, "BOOTLEG_CLUSTER_NAME")?,
        },
    })))
}
pub fn resolve_name(
    env: &Environment,
    flags: &Overrides,
    os_hostname: Option<&str>,
    id: &ContainerId,
) -> Result<DisplayName, ConfigError> {
    if let Some(value) = flags.name.as_ref().or(env.get("BOOTLEG_INSTANCE_NAME")) {
        return value.parse().map_err(|_| "instance name");
    }
    if let Some(key) = flags.name_env.as_ref().or(env.get("BOOTLEG_NAME_ENV")) {
        if key.is_empty() {
            return Err("name-env");
        }
        return env
            .get(key)
            .ok_or("selected name-env is missing")?
            .parse()
            .map_err(|_| "selected name-env is invalid");
    }
    for key in ["POD_NAME", "KUBERNETES_POD_NAME", "HOSTNAME"] {
        if let Some(value) = env.get(key).filter(|s| !s.is_empty()) {
            let value = if key != "HOSTNAME" {
                match env.get("BOOTLEG_CONTAINER_NAME") {
                    Some(container) => format!("{value}/{container}"),
                    None => value.clone(),
                }
            } else {
                value.clone()
            };
            return value.parse().map_err(|_| "derived instance name");
        }
    }
    if let Some(value) = os_hostname.filter(|s| !s.is_empty()) {
        return value.parse().map_err(|_| "OS hostname");
    }
    format!("container-{}", &id.to_string()[10..18])
        .parse()
        .map_err(|_| "instance name")
}
