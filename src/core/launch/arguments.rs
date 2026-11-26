use crate::core::minecraft::libraries::{rules_allow, TargetPlatform};
use crate::core::minecraft::metadata::{ArgumentEntry, ArgumentValue, VersionMetadata};
use crate::error::{LauncherError, Result};
use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

/// Substitui placeholders `${chave}` no argumento.
/// Placeholders desconhecidos NÃO são substituídos silenciosamente por string vazia.
pub fn substitute_placeholders(template: &str, vars: &HashMap<String, String>) -> Result<String> {
    substitute_placeholders_with_context(template, vars, None, None)
}

pub fn substitute_placeholders_with_context(
    template: &str,
    vars: &HashMap<String, String>,
    version_id: Option<&str>,
    field: Option<&str>,
) -> Result<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"\$\{([A-Za-z0-9_]+)\}").expect("valid regex"));

    let mut unknown: Option<String> = None;
    let replaced = re.replace_all(template, |caps: &regex::Captures| {
        let key = &caps[1];
        if let Some(val) = vars.get(key) {
            val.clone()
        } else {
            unknown = Some(key.to_string());
            caps[0].to_string()
        }
    });

    if let Some(missing_key) = unknown {
        let ver = version_id.unwrap_or("unknown");
        let fld = field.unwrap_or("launch arguments");
        return Err(LauncherError::Launch(format!(
            "Unresolved placeholder '${{{missing_key}}}' in {fld} (Minecraft {ver})"
        )));
    }

    Ok(replaced.into_owned())
}

/// Resolve uma lista de entradas de argumento (JVM ou Game) respeitando regras de plataforma e features.
pub fn resolve_argument_entries(
    entries: &[ArgumentEntry],
    platform: &TargetPlatform,
    features: &HashMap<String, bool>,
    vars: &HashMap<String, String>,
    version_id: Option<&str>,
    field: Option<&str>,
) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for entry in entries {
        match entry {
            ArgumentEntry::Simple(raw) => {
                out.push(substitute_placeholders_with_context(
                    raw, vars, version_id, field,
                )?);
            }
            ArgumentEntry::Conditional { rules, value } => {
                if !rules_allow(rules, platform, features) {
                    continue;
                }
                match value {
                    ArgumentValue::Single(s) => out.push(substitute_placeholders_with_context(
                        s, vars, version_id, field,
                    )?),
                    ArgumentValue::Multiple(list) => {
                        for s in list {
                            out.push(substitute_placeholders_with_context(
                                s, vars, version_id, field,
                            )?);
                        }
                    }
                }
            }
        }
    }
    Ok(out)
}

/// Resolve os argumentos JVM e Game a partir dos metadados oficiais (formato moderno ou legado).
pub fn resolve_version_arguments(
    metadata: &VersionMetadata,
    platform: &TargetPlatform,
    features: &HashMap<String, bool>,
    vars: &HashMap<String, String>,
) -> Result<(Vec<String>, Vec<String>)> {
    if let Some(arguments) = &metadata.arguments {
        let jvm = resolve_argument_entries(
            &arguments.jvm,
            platform,
            features,
            vars,
            Some(&metadata.id),
            Some("arguments.jvm"),
        )?;
        let game = resolve_argument_entries(
            &arguments.game,
            platform,
            features,
            vars,
            Some(&metadata.id),
            Some("arguments.game"),
        )?;
        Ok((jvm, game))
    } else if let Some(legacy) = &metadata.minecraft_arguments {
        // Formato legado (anterior à 1.13): `minecraftArguments` + flags JVM padrão.
        let mut game = Vec::new();
        for token in legacy.split_whitespace() {
            game.push(substitute_placeholders_with_context(
                token,
                vars,
                Some(&metadata.id),
                Some("minecraftArguments"),
            )?);
        }
        let jvm = vec![
            substitute_placeholders_with_context(
                "-Djava.library.path=${natives_directory}",
                vars,
                Some(&metadata.id),
                Some("legacy.jvm"),
            )?,
            "-cp".to_string(),
            substitute_placeholders_with_context(
                "${classpath}",
                vars,
                Some(&metadata.id),
                Some("legacy.jvm"),
            )?,
        ];
        Ok((jvm, game))
    } else {
        Err(LauncherError::Launch(format!(
            "Version '{}' does not specify launch arguments (missing arguments and minecraftArguments).",
            metadata.id
        )))
    }
}
