//! The capability engine as this machine has it (§20.3): Jev with the stored key, when the person
//! consented here; else off, with no request and no note.

use std::collections::BTreeMap;

use forge_home::Home;

use crate::digest::Digest;
use crate::engine::{Engine, EngineError};
use crate::jev::{BASE_URL, Jev};
use crate::setup::{self, State};

/// `FORGE_CAPABILITIES_URL`: another base URL for the API, for Forge's own tests.
pub const URL_VAR: &str = "FORGE_CAPABILITIES_URL";

/// An engine that is always off (a machine without consent; the tests that do not use it).
pub struct Off;

impl Engine for Off {
    fn values(
        &self,
        _: &Digest,
        _: &[(String, &str)],
    ) -> Result<BTreeMap<String, f64>, EngineError> {
        Err(EngineError::Off)
    }
}

pub struct Machine {
    home: Home,
    base: String,
}

impl Machine {
    /// The engine of the user whose state is under `home`; `base`: the API's URL, else TypeSafe's.
    pub fn new(home: Home, base: Option<String>) -> Machine {
        Machine {
            home,
            base: base.unwrap_or_else(|| BASE_URL.to_string()),
        }
    }
}

impl Engine for Machine {
    fn values(
        &self,
        digest: &Digest,
        questions: &[(String, &str)],
    ) -> Result<BTreeMap<String, f64>, EngineError> {
        if setup::state(&self.home) != State::On {
            return Err(EngineError::Off);
        }
        let key = setup::key(&self.home).ok_or(EngineError::Off)?;
        Jev::new(key, forge_tools::catalog::CAPABILITY_MODEL, &self.base).values(digest, questions)
    }
}
