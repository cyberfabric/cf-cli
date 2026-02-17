use anyhow::bail;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Deserialize)]
pub struct Config {
    pub modules: HashMap<String, ConfigModule>,
}

impl Config {
    pub fn create_dependencies(self) -> anyhow::Result<HashMap<String, ConfigModuleMetadata>> {
        let mut dependencies = HashMap::with_capacity(self.modules.len());
        for (name, module) in self.modules.into_iter() {
            let Some(package) = module.metadata.package.clone() else {
                bail!("module '{name}' doesn't have package associated, please review");
            };
            dependencies.insert(package.replace("-", "_"), module.metadata);
        }

        Ok(dependencies)
    }
}

#[derive(Deserialize)]
pub struct ConfigModule {
    pub metadata: ConfigModuleMetadata,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deps: Vec<String>,
}

#[derive(Default, Serialize)]
pub struct CargoToml {
    #[serde(default)]
    pub package: Package,
    pub dependencies: HashMap<String, ConfigModuleMetadata>,
    pub features: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub workspace: HashMap<String, Vec<String>>,
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
            None => None,
            Some("*") => None,
            Some(x) => Some(x.to_string()),
        })
    }
}
