use super::*;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Execution {
    fp_contract: FpContract,
    compiler: String,
    target_features: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
enum FpContract {
    #[serde(rename = "x86_64-mxcsr-rne-gradual-v1")]
    Native,
    #[serde(rename = "wasm32-core-rne-gradual-v1")]
    Wasm,
}

fn features(value: &str) -> Result<Vec<&str>> {
    ensure!(
        value.len() <= 1024,
        "artifact pair feature list exceeds limit"
    );
    let features: Vec<_> = if value.is_empty() {
        Vec::new()
    } else {
        value.split(',').collect()
    };
    ensure!(
        features.iter().all(|feature| !feature.is_empty()
            && feature.bytes().all(|byte| byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'-' | b'.' | b'_')))
            && features.windows(2).all(|pair| pair[0] < pair[1]),
        "noncanonical artifact pair features"
    );
    Ok(features)
}

pub(super) fn validate_members(format: u32, members: &[CertifiedMember; 2]) -> Result<()> {
    if format != 3 {
        ensure!(
            members.iter().all(|member| member.execution.is_none()),
            "legacy artifact pair has mixed execution facts"
        );
        ensure!(
            (members[0].target == WINDOWS && members[1].target == LINUX)
                || (members[0].target == LINUX && members[1].target == WINDOWS),
            "unsupported artifact pair targets"
        );
        return Ok(());
    }
    ensure!(
        (members[0].target == WASM && matches!(members[1].target.as_str(), WINDOWS | LINUX))
            || (members[1].target == WASM && matches!(members[0].target.as_str(), WINDOWS | LINUX)),
        "unsupported mixed artifact pair targets"
    );
    for member in members {
        let execution = member
            .execution
            .as_ref()
            .context("missing artifact pair execution facts")?;
        digest(&execution.compiler)?;
        let features = features(&execution.target_features)?;
        let wasm = member.target == WASM;
        ensure!(
            execution.fp_contract
                == if wasm {
                    FpContract::Wasm
                } else {
                    FpContract::Native
                },
            "artifact pair floating point contract mismatch"
        );
        if wasm {
            ensure!(
                features.iter().all(|feature| matches!(
                    *feature,
                    "multivalue"
                        | "mutable-globals"
                        | "nontrapping-fptoint"
                        | "reference-types"
                        | "sign-ext"
                )),
                "unsupported WebAssembly artifact features"
            );
        }
    }
    Ok(())
}

pub(super) fn validate_local(
    format: u32,
    local: &CertifiedMember,
    facts: &LocalFacts<'_>,
) -> Result<()> {
    if format == 3 {
        let execution = local
            .execution
            .as_ref()
            .context("missing local execution facts")?;
        ensure!(
            digest(&execution.compiler)? == digest(facts.compiler)?
                && execution.target_features == facts.target_features,
            "artifact pair compiled execution facts mismatch"
        );
    }
    Ok(())
}
