use std::fmt;

use ring::digest::{digest, SHA256};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Scenario;

const CORPUS_FORMAT: &[u8] = b"willitcall-corpus-v1";
const WIC_50_V1_CATALOG: &[u8] = include_bytes!("../catalogs/wic-50-v1.json");

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusCatalog {
    pub id: String,
    pub revision: String,
    pub scenario_count: u32,
    pub sha256: String,
    pub scenarios: Vec<Scenario>,
}

#[derive(Debug)]
pub struct CorpusCatalogError(String);

impl fmt::Display for CorpusCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for CorpusCatalogError {}

pub fn load_catalog(bytes: &[u8]) -> Result<CorpusCatalog, CorpusCatalogError> {
    let catalog: CorpusCatalog = serde_json::from_slice(bytes)
        .map_err(|error| CorpusCatalogError(format!("invalid corpus catalog: {error}")))?;
    catalog.verify()?;
    Ok(catalog)
}

pub fn load_frozen_v1_catalog() -> Result<CorpusCatalog, CorpusCatalogError> {
    load_catalog(WIC_50_V1_CATALOG)
}

impl CorpusCatalog {
    pub fn verify(&self) -> Result<(), CorpusCatalogError> {
        let scenario_count = u32::try_from(self.scenarios.len()).map_err(|_| {
            CorpusCatalogError("catalog scenario count does not fit in u32".to_owned())
        })?;
        if self.scenario_count != scenario_count {
            return Err(CorpusCatalogError(format!(
                "catalog declares {} scenarios but contains {scenario_count}",
                self.scenario_count
            )));
        }

        let actual = corpus_identity(&self.scenarios);
        if self.sha256 != actual {
            return Err(CorpusCatalogError(format!(
                "catalog hash mismatch: declared {}, computed {actual}",
                self.sha256
            )));
        }
        Ok(())
    }
}

pub fn corpus_identity(scenarios: &[Scenario]) -> String {
    let serialization = canonical_serialization(scenarios);
    let hash = digest(&SHA256, &serialization);
    format!("sha256:{}", hex(hash.as_ref()))
}

fn canonical_serialization(scenarios: &[Scenario]) -> Vec<u8> {
    let mut ordered = scenarios.iter().collect::<Vec<_>>();
    ordered.sort_by(|left, right| left.id.cmp(&right.id));

    let mut output = Vec::new();
    write_bytes(&mut output, CORPUS_FORMAT);
    write_len(&mut output, ordered.len());
    for scenario in ordered {
        let value = serde_json::to_value(scenario).expect("scenario should serialize to JSON");
        let mut encoded = Vec::new();
        write_value(&mut encoded, &value);
        write_bytes(&mut output, &encoded);
    }
    output
}

fn write_value(output: &mut Vec<u8>, value: &Value) {
    match value {
        Value::Null => output.push(0),
        Value::Bool(false) => output.push(1),
        Value::Bool(true) => output.push(2),
        Value::Number(number) => {
            output.push(3);
            write_bytes(output, number.to_string().as_bytes());
        }
        Value::String(string) => {
            output.push(4);
            write_bytes(output, string.as_bytes());
        }
        Value::Array(values) => {
            output.push(5);
            write_len(output, values.len());
            for value in values {
                let mut encoded = Vec::new();
                write_value(&mut encoded, value);
                write_bytes(output, &encoded);
            }
        }
        Value::Object(object) => {
            output.push(6);
            write_len(output, object.len());
            let mut fields = object.iter().collect::<Vec<_>>();
            fields.sort_by(|left, right| left.0.cmp(right.0));
            for (name, value) in fields {
                write_bytes(output, name.as_bytes());
                let mut encoded = Vec::new();
                write_value(&mut encoded, value);
                write_bytes(output, &encoded);
            }
        }
    }
}

fn write_bytes(output: &mut Vec<u8>, bytes: &[u8]) {
    write_len(output, bytes.len());
    output.extend_from_slice(bytes);
}

fn write_len(output: &mut Vec<u8>, len: usize) {
    let len = u64::try_from(len).expect("corpus serialization length should fit in u64");
    output.extend_from_slice(&len.to_be_bytes());
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(DIGITS[(byte >> 4) as usize] as char);
        encoded.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    encoded
}
