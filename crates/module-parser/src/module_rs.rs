use super::config::{ConfigModule, ConfigModuleMetadata};
use anyhow::Context;
use cargo_metadata::{Package, Target};
use std::fs;
use std::path::PathBuf;
use syn::{Attribute, Item, Lit};

pub(crate) fn retrieve_module_rs(
    package: &Package,
    target: Target,
) -> anyhow::Result<(String, ConfigModule)> {
    let lib_rs = PathBuf::from(&target.src_path);
    let src = lib_rs
        .parent()
        .with_context(|| format!("no source parent for {}", target.src_path))?;
    let module_rs = src.join("module.rs");
    let content = fs::read_to_string(&module_rs)
        .with_context(|| format!("can't read module from {}", module_rs.display()))?;
    let ast = syn::parse_file(&content)?;
    let crate_root = PathBuf::from(&package.manifest_path)
        .parent()
        .map(|p| p.display().to_string());

    for item in ast.items {
        if let Item::Struct(struct_item) = item
            && let Some(module_info) = parse_modkit_module_attribute(&struct_item.attrs)?
        {
            let config_module = ConfigModule {
                metadata: ConfigModuleMetadata {
                    package: Some(package.name.to_string()),
                    version: Some(package.version.to_string()),
                    features: vec![],
                    path: crate_root,
                    deps: module_info.deps,
                },
            };
            return Ok((module_info.name, config_module));
        }
    }
    Err(anyhow::anyhow!("no module found"))
}

struct ModuleInfo {
    name: String,
    deps: Vec<String>,
}

fn parse_modkit_module_attribute(attrs: &[Attribute]) -> anyhow::Result<Option<ModuleInfo>> {
    for attr in attrs {
        if is_modkit_module_path(attr) {
            return parse_module_args(attr).map(Some);
        }
    }
    Ok(None)
}

fn is_modkit_module_path(attr: &Attribute) -> bool {
    let path = attr.path();
    let segments: Vec<_> = path.segments.iter().map(|s| s.ident.to_string()).collect();

    (segments.len() == 1 && segments[0] == "module")
        || (segments.len() == 2 && segments[0] == "modkit" && segments[1] == "module")
}

fn parse_module_args(attr: &Attribute) -> anyhow::Result<ModuleInfo> {
    let mut name = None;
    let mut deps = Vec::new();

    attr.parse_nested_meta(|meta| {
        if meta.path.is_ident("name") {
            let value = meta.value()?;
            let lit: Lit = value.parse()?;
            if let Lit::Str(lit_str) = lit {
                name = Some(lit_str.value());
            }
        } else if meta.path.is_ident("deps") {
            let value = meta.value()?;
            let content;
            syn::bracketed!(content in value);
            while !content.is_empty() {
                let lit: Lit = content.parse()?;
                if let Lit::Str(lit_str) = lit {
                    deps.push(lit_str.value());
                }
                if !content.is_empty() {
                    let _: syn::token::Comma = content.parse()?;
                }
            }
        } else if meta.path.is_ident("capabilities") {
            let value = meta.value()?;
            let content;
            syn::bracketed!(content in value);
            while !content.is_empty() {
                let _ident: syn::Ident = content.parse()?;
                if !content.is_empty() {
                    let _: syn::token::Comma = content.parse()?;
                }
            }
        }
        Ok(())
    })?;

    let name = name.context("module attribute must have a name")?;
    Ok(ModuleInfo { name, deps })
}
