use anyhow::bail;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::fmt;

#[derive(Deserialize)]
pub struct Config {
    pub modules: HashMap<String, ConfigModule>,
}

impl Config {
    pub fn create_dependencies(self) -> anyhow::Result<CargoTomlDependencies> {
        let mut dependencies = HashMap::with_capacity(self.modules.len());
        for (name, module) in self.modules {
            let Some(package) = module.metadata.package.clone() else {
                bail!("module '{name}' doesn't have package associated, please review");
            };
            let package = package.replace('-', "_");
            if dependencies.contains_key(&package) {
                bail!("module '{name}' has duplicate package name '{package}'");
            }

            dependencies.insert(
                package,
                CargoTomlDependency {
                    package: module.metadata.package,
                    version: module.metadata.version,
                    features: module.metadata.features.into_iter().collect(),
                    default_features: module.metadata.default_features,
                    path: module.metadata.path,
                },
            );
        }

        Ok(dependencies)
    }
}

#[derive(Deserialize)]
pub struct ConfigModule {
    pub metadata: ConfigModuleMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Db,
    Rest,
    RestHost,
    Stateful,
    System,
    GrpcHub,
    Grpc,
}

impl fmt::Display for Capability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::Db => "db",
            Self::Rest => "rest",
            Self::RestHost => "rest_host",
            Self::Stateful => "stateful",
            Self::System => "system",
            Self::GrpcHub => "grpc_hub",
            Self::Grpc => "grpc",
        };
        f.write_str(name)
    }
}

#[derive(Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct ConfigModuleMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(
        default,
        serialize_with = "opt_string_none_as_star::serialize",
        deserialize_with = "opt_string_none_as_star::deserialize"
    )]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_features: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deps: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<Capability>,
}

#[derive(Default, Serialize)]
pub struct CargoToml {
    #[serde(default)]
    pub package: Package,
    pub dependencies: CargoTomlDependencies,
    pub features: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub workspace: HashMap<String, Vec<String>>,
}

pub type CargoTomlDependencies = HashMap<String, CargoTomlDependency>;

#[derive(Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct CargoTomlDependency {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    #[serde(
        default,
        serialize_with = "opt_string_none_as_star::serialize",
        deserialize_with = "opt_string_none_as_star::deserialize"
    )]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub features: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_features: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Serialize)]
pub struct Package {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub edition: String,
}

impl Default for Package {
    fn default() -> Self {
        Self {
            name: "server".to_owned(),
            version: "0.0.1".to_owned(),
            edition: "2024".to_owned(),
        }
    }
}

mod opt_string_none_as_star {
    use serde::{Deserialize, Deserializer, Serializer};

    #[allow(clippy::ref_option)]
    pub fn serialize<S>(v: &Option<String>, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match v.as_deref() {
            None => s.serialize_str("*"),
            Some(x) => s.serialize_str(x),
        }
    }

    pub fn deserialize<'de, D>(d: D) -> Result<Option<String>, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Accept missing/null as None; accept "*" as None; otherwise Some(value).
        let opt = Option::<String>::deserialize(d)?;
        Ok(match opt.as_deref() {
            None | Some("*") => None,
            Some(x) => Some(x.to_string()),
        })
    }
}
