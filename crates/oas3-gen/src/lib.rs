#![allow(clippy::doc_markdown)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::too_many_lines)]

pub mod generator;
#[doc(hidden)]
pub mod ui;
pub mod utils;

pub use generator::{
  CodegenConfig, EnumCasePolicy, EnumDeserializePolicy, EnumHelperPolicy, GenerationTarget, HeaderScope, ODataPolicy,
  SchemaScope, ZeroCopyPolicy,
  codegen::{GeneratedFileType, GeneratedResult, Visibility},
  metrics::GenerationStats,
  mode::{ClientModMode, ClientMode, GenerationMode, ServerModMode, TypesMode},
  orchestrator::{GeneratedFinalOutput, Orchestrator},
};
pub use utils::spec::{SpecFormat, SpecLoader};

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "../fixtures"]
mod fixtures {
  pub mod intersection_union;
  pub mod petstore;
  pub mod petstore_server;
  pub mod union_serde;
}
