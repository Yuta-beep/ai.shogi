use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
use thiserror::Error;

const BUILTIN_SKILL_REGISTRY_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/docs/skill-registry-v2-draft.json"
));

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillRegistryDocument {
    pub version: String,
    pub updated_at: String,
    pub implementation_kinds: Vec<ImplementationKindSpec>,
    pub registries: RegistryCollection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImplementationKindSpec {
    pub code: String,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryCollection {
    pub trigger: RegistrySpec,
    pub target: RegistrySpec,
    pub effect: RegistrySpec,
    pub condition: RegistrySpec,
    pub param: RegistrySpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistrySpec {
    pub groups: Vec<RegistryGroupSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryGroupSpec {
    pub group_code: String,
    pub group_name: String,
    pub description: String,
    pub options: Vec<RegistryOptionSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryOptionSpec {
    pub option_code: String,
    pub option_name: String,
    pub description: String,
    #[serde(default)]
    pub value_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDefinitionDocument {
    pub version: String,
    pub updated_at: String,
    #[serde(default)]
    pub source_of_truth: Vec<SkillSourceRef>,
    pub definitions: Vec<SkillDefinition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSourceRef {
    pub piece_char: String,
    pub skill_text: String,
    pub source_file: String,
    pub source_function: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDefinition {
    pub skill_id: u64,
    pub piece_chars: Vec<String>,
    pub source: SkillSource,
    pub classification: SkillClassification,
    pub trigger: RegistryRef,
    #[serde(default)]
    pub conditions: Vec<SkillCondition>,
    #[serde(default)]
    pub effects: Vec<SkillEffect>,
    #[serde(default)]
    pub script_hook: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSource {
    pub skill_text: String,
    pub source_kind: String,
    pub source_file: String,
    pub source_function: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillClassification {
    pub implementation_kind: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryRef {
    pub group: String,
    #[serde(rename = "type")]
    pub type_code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCondition {
    pub order: u16,
    pub group: String,
    #[serde(rename = "type")]
    pub type_code: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillEffect {
    pub order: u16,
    pub group: String,
    #[serde(rename = "type")]
    pub type_code: String,
    pub target: TargetRef,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetRef {
    pub group: String,
    pub selector: String,
}

#[derive(Debug, Clone, Default)]
pub struct SkillRuntimeRules {
    pub registry: Option<SkillRegistryDocument>,
    pub definitions: Vec<SkillDefinition>,
    pub legacy_skill_effects: Vec<Value>,
}

#[derive(Debug, Error)]
pub enum SkillSchemaError {
    #[error("failed to parse skill registry: {0}")]
    RegistryParse(String),
    #[error("failed to parse skill definitions: {0}")]
    DefinitionParse(String),
    #[error("invalid skill registry: {0}")]
    InvalidRegistry(String),
    #[error("invalid skill definition: {0}")]
    InvalidDefinition(String),
}

#[derive(Debug, Clone, Copy)]
enum RegistryKind {
    Trigger,
    Target,
    Effect,
    Condition,
    Param,
}

impl RegistryKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Trigger => "trigger",
            Self::Target => "target",
            Self::Effect => "effect",
            Self::Condition => "condition",
            Self::Param => "param",
        }
    }
}

#[derive(Debug)]
struct RegistryIndex {
    implementation_kinds: HashSet<String>,
    options_by_kind: HashMap<&'static str, HashMap<String, HashSet<String>>>,
}

static BUILTIN_SKILL_REGISTRY: OnceLock<SkillRegistryDocument> = OnceLock::new();

pub fn builtin_skill_registry() -> &'static SkillRegistryDocument {
    BUILTIN_SKILL_REGISTRY.get_or_init(|| {
        let registry: SkillRegistryDocument = serde_json::from_str(BUILTIN_SKILL_REGISTRY_JSON)
            .expect("bundled skill registry json must parse");
        validate_skill_registry(&registry).expect("bundled skill registry must be valid");
        registry
    })
}

pub fn parse_skill_registry_value(value: Value) -> Result<SkillRegistryDocument, SkillSchemaError> {
    let registry: SkillRegistryDocument = serde_json::from_value(value)
        .map_err(|e| SkillSchemaError::RegistryParse(e.to_string()))?;
    validate_skill_registry(&registry)?;
    Ok(registry)
}

pub fn parse_skill_definition_document_value(
    value: Value,
) -> Result<SkillDefinitionDocument, SkillSchemaError> {
    if value.is_array() {
        let definitions = serde_json::from_value::<Vec<SkillDefinition>>(value)
            .map_err(|e| SkillSchemaError::DefinitionParse(e.to_string()))?;
        return Ok(SkillDefinitionDocument {
            version: "inline-array".to_string(),
            updated_at: "".to_string(),
            source_of_truth: Vec::new(),
            definitions,
        });
    }

    if value.get("definitions").is_some() {
        return serde_json::from_value::<SkillDefinitionDocument>(value)
            .map_err(|e| SkillSchemaError::DefinitionParse(e.to_string()));
    }

    let definition = serde_json::from_value::<SkillDefinition>(value)
        .map_err(|e| SkillSchemaError::DefinitionParse(e.to_string()))?;
    Ok(SkillDefinitionDocument {
        version: "single-definition".to_string(),
        updated_at: "".to_string(),
        source_of_truth: Vec::new(),
        definitions: vec![definition],
    })
}

pub fn validate_skill_registry(registry: &SkillRegistryDocument) -> Result<(), SkillSchemaError> {
    if registry.implementation_kinds.is_empty() {
        return Err(SkillSchemaError::InvalidRegistry(
            "implementationKinds must not be empty".to_string(),
        ));
    }

    let mut implementation_kind_set = HashSet::new();
    for kind in &registry.implementation_kinds {
        if kind.code.trim().is_empty() {
            return Err(SkillSchemaError::InvalidRegistry(
                "implementation kind code must not be empty".to_string(),
            ));
        }
        if !implementation_kind_set.insert(kind.code.clone()) {
            return Err(SkillSchemaError::InvalidRegistry(format!(
                "duplicate implementation kind: {}",
                kind.code
            )));
        }
    }

    let specs = [
        (RegistryKind::Trigger, &registry.registries.trigger),
        (RegistryKind::Target, &registry.registries.target),
        (RegistryKind::Effect, &registry.registries.effect),
        (RegistryKind::Condition, &registry.registries.condition),
        (RegistryKind::Param, &registry.registries.param),
    ];

    for (kind, spec) in specs {
        validate_registry_spec(kind, spec)?;
    }

    Ok(())
}

pub fn validate_skill_definitions(
    registry: &SkillRegistryDocument,
    definitions: &[SkillDefinition],
) -> Result<(), SkillSchemaError> {
    let index = build_registry_index(registry)?;

    let mut seen_skill_ids = HashSet::new();
    for definition in definitions {
        if !seen_skill_ids.insert(definition.skill_id) {
            return Err(SkillSchemaError::InvalidDefinition(format!(
                "duplicate skillId: {}",
                definition.skill_id
            )));
        }

        if definition.piece_chars.is_empty() {
            return Err(SkillSchemaError::InvalidDefinition(format!(
                "skillId {} must have at least one piece char",
                definition.skill_id
            )));
        }

        let implementation_kind = definition.classification.implementation_kind.as_str();
        if !index.implementation_kinds.contains(implementation_kind) {
            return Err(SkillSchemaError::InvalidDefinition(format!(
                "skillId {} uses unknown implementation kind: {}",
                definition.skill_id, implementation_kind
            )));
        }

        validate_registry_ref(
            &index,
            RegistryKind::Trigger,
            &definition.trigger,
            definition.skill_id,
        )?;

        let conditions = ensure_unique_orders(
            definition
                .conditions
                .iter()
                .map(|condition| condition.order),
            definition.skill_id,
            "condition",
        )?;
        let _ = conditions;
        for condition in &definition.conditions {
            validate_registry_ref(
                &index,
                RegistryKind::Condition,
                &RegistryRef {
                    group: condition.group.clone(),
                    type_code: condition.type_code.clone(),
                },
                definition.skill_id,
            )?;
        }

        let effects = ensure_unique_orders(
            definition.effects.iter().map(|effect| effect.order),
            definition.skill_id,
            "effect",
        )?;
        let _ = effects;
        for effect in &definition.effects {
            validate_registry_ref(
                &index,
                RegistryKind::Effect,
                &RegistryRef {
                    group: effect.group.clone(),
                    type_code: effect.type_code.clone(),
                },
                definition.skill_id,
            )?;
            validate_registry_ref(
                &index,
                RegistryKind::Target,
                &RegistryRef {
                    group: effect.target.group.clone(),
                    type_code: effect.target.selector.clone(),
                },
                definition.skill_id,
            )?;
        }

        match implementation_kind {
            "primitive" => {
                if definition.script_hook.is_some() {
                    return Err(SkillSchemaError::InvalidDefinition(format!(
                        "skillId {} primitive must not define scriptHook",
                        definition.skill_id
                    )));
                }
                if definition.effects.len() != 1 {
                    return Err(SkillSchemaError::InvalidDefinition(format!(
                        "skillId {} primitive must have exactly one effect",
                        definition.skill_id
                    )));
                }
            }
            "composite" => {
                if definition.script_hook.is_some() {
                    return Err(SkillSchemaError::InvalidDefinition(format!(
                        "skillId {} composite must not define scriptHook",
                        definition.skill_id
                    )));
                }
                if definition.effects.len() < 2 {
                    return Err(SkillSchemaError::InvalidDefinition(format!(
                        "skillId {} composite must have at least two effects",
                        definition.skill_id
                    )));
                }
            }
            "script_hook" => {
                if definition
                    .script_hook
                    .as_deref()
                    .unwrap_or("")
                    .trim()
                    .is_empty()
                {
                    return Err(SkillSchemaError::InvalidDefinition(format!(
                        "skillId {} script_hook must define scriptHook",
                        definition.skill_id
                    )));
                }
                if !definition.effects.is_empty() {
                    return Err(SkillSchemaError::InvalidDefinition(format!(
                        "skillId {} script_hook must not define common effects",
                        definition.skill_id
                    )));
                }
            }
            _ => {}
        }
    }

    Ok(())
}

fn validate_registry_spec(kind: RegistryKind, spec: &RegistrySpec) -> Result<(), SkillSchemaError> {
    if spec.groups.is_empty() {
        return Err(SkillSchemaError::InvalidRegistry(format!(
            "{} groups must not be empty",
            kind.as_str()
        )));
    }

    let mut group_codes = HashSet::new();
    let mut option_codes = HashSet::new();

    for group in &spec.groups {
        if group.group_code.trim().is_empty() {
            return Err(SkillSchemaError::InvalidRegistry(format!(
                "{} groupCode must not be empty",
                kind.as_str()
            )));
        }
        if !group_codes.insert(group.group_code.clone()) {
            return Err(SkillSchemaError::InvalidRegistry(format!(
                "duplicate {} groupCode: {}",
                kind.as_str(),
                group.group_code
            )));
        }
        if group.options.is_empty() {
            return Err(SkillSchemaError::InvalidRegistry(format!(
                "{} group {} must define at least one option",
                kind.as_str(),
                group.group_code
            )));
        }
        for option in &group.options {
            if option.option_code.trim().is_empty() {
                return Err(SkillSchemaError::InvalidRegistry(format!(
                    "{} optionCode must not be empty in group {}",
                    kind.as_str(),
                    group.group_code
                )));
            }
            if !option_codes.insert(option.option_code.clone()) {
                return Err(SkillSchemaError::InvalidRegistry(format!(
                    "duplicate {} optionCode: {}",
                    kind.as_str(),
                    option.option_code
                )));
            }
        }
    }

    Ok(())
}

fn build_registry_index(
    registry: &SkillRegistryDocument,
) -> Result<RegistryIndex, SkillSchemaError> {
    validate_skill_registry(registry)?;

    let mut options_by_kind: HashMap<&'static str, HashMap<String, HashSet<String>>> =
        HashMap::new();

    for (kind, spec) in [
        (RegistryKind::Trigger, &registry.registries.trigger),
        (RegistryKind::Target, &registry.registries.target),
        (RegistryKind::Effect, &registry.registries.effect),
        (RegistryKind::Condition, &registry.registries.condition),
        (RegistryKind::Param, &registry.registries.param),
    ] {
        let mut by_group = HashMap::new();
        for group in &spec.groups {
            by_group.insert(
                group.group_code.clone(),
                group
                    .options
                    .iter()
                    .map(|option| option.option_code.clone())
                    .collect(),
            );
        }
        options_by_kind.insert(kind.as_str(), by_group);
    }

    Ok(RegistryIndex {
        implementation_kinds: registry
            .implementation_kinds
            .iter()
            .map(|kind| kind.code.clone())
            .collect(),
        options_by_kind,
    })
}

fn validate_registry_ref(
    index: &RegistryIndex,
    kind: RegistryKind,
    reference: &RegistryRef,
    skill_id: u64,
) -> Result<(), SkillSchemaError> {
    let groups = index.options_by_kind.get(kind.as_str()).ok_or_else(|| {
        SkillSchemaError::InvalidRegistry(format!("missing {} registry", kind.as_str()))
    })?;

    let options = groups.get(&reference.group).ok_or_else(|| {
        SkillSchemaError::InvalidDefinition(format!(
            "skillId {} uses unknown {} group: {}",
            skill_id,
            kind.as_str(),
            reference.group
        ))
    })?;

    if !options.contains(&reference.type_code) {
        return Err(SkillSchemaError::InvalidDefinition(format!(
            "skillId {} uses unknown {} option {} in group {}",
            skill_id,
            kind.as_str(),
            reference.type_code,
            reference.group
        )));
    }

    Ok(())
}

fn ensure_unique_orders<I>(
    orders: I,
    skill_id: u64,
    label: &str,
) -> Result<HashSet<u16>, SkillSchemaError>
where
    I: IntoIterator<Item = u16>,
{
    let mut seen = HashSet::new();
    for order in orders {
        if order == 0 {
            return Err(SkillSchemaError::InvalidDefinition(format!(
                "skillId {} {} order must start at 1",
                skill_id, label
            )));
        }
        if !seen.insert(order) {
            return Err(SkillSchemaError::InvalidDefinition(format!(
                "skillId {} has duplicate {} order {}",
                skill_id, label, order
            )));
        }
    }
    Ok(seen)
}
