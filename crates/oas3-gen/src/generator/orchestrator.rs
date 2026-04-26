use std::{collections::HashSet, rc::Rc, sync::Arc};

use oas3::Spec;

use crate::generator::{
  ast::{ClientRootNode, OperationInfo, RustType, constants::HttpHeaderRef},
  codegen::{GeneratedResult, SchemaCodeGenerator, Visibility},
  converter::{
    CodegenConfig, ConverterContext, GenerationTarget, OperationsProcessor, SchemaConverter, SerdeUsageRecorder,
    build_server_trait, cache::SharedSchemaCache,
  },
  metrics::GenerationStats,
  mode::GenerationMode,
  operation_registry::OperationRegistry,
  postprocess::PostprocessOutput,
  schema_registry::SchemaRegistry,
};

const OAS3_GEN_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug)]
pub struct Orchestrator {
  spec: Spec,
  visibility: Visibility,
  config: CodegenConfig,
  operation_registry: OperationRegistry,
}

struct GenerationArtifacts {
  rust_types: Vec<RustType>,
  operations_info: Vec<OperationInfo>,
  serde_recorder: SerdeUsageRecorder,
  unique_headers: Vec<HttpHeaderRef>,
  stats: GenerationStats,
  config: CodegenConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedFinalOutput {
  pub code: GeneratedResult,
  pub stats: GenerationStats,
}

impl GeneratedFinalOutput {
  pub fn new(code: GeneratedResult, stats: GenerationStats) -> Self {
    Self { code, stats }
  }
}

impl Orchestrator {
  #[must_use]
  pub fn new(
    spec: Spec,
    visibility: Visibility,
    config: CodegenConfig,
    only_operations: Option<&HashSet<String>>,
    excluded_operations: Option<&HashSet<String>>,
  ) -> Self {
    let operation_registry = OperationRegistry::with_filters(&spec, only_operations, excluded_operations);
    Self {
      spec,
      visibility,
      config,
      operation_registry,
    }
  }

  pub fn generate(&self, mode: &dyn GenerationMode, source_path: &str) -> anyhow::Result<GeneratedFinalOutput> {
    let artifacts = self.collect_generation_artifacts();
    let serde_usage = artifacts.serde_recorder.into_usage_map();
    let postprocessed = PostprocessOutput::new(
      artifacts.rust_types,
      artifacts.operations_info,
      serde_usage,
      artifacts.config.target,
      artifacts.unique_headers,
    );

    let server_trait_def = if artifacts.config.target == GenerationTarget::Server {
      build_server_trait(&postprocessed.operations)
    } else {
      None
    };

    let codegen = SchemaCodeGenerator::builder()
      .config(artifacts.config)
      .rust_types(postprocessed.types)
      .operations(postprocessed.operations)
      .header_refs(postprocessed.header_refs)
      .uses(postprocessed.uses)
      .client(ClientRootNode::from(&self.spec))
      .maybe_server_trait(server_trait_def)
      .visibility(self.visibility)
      .source_path(source_path.to_string())
      .gen_version(OAS3_GEN_VERSION.to_string())
      .build();

    let code = mode.generate(&codegen)?;
    Ok(GeneratedFinalOutput::new(code, artifacts.stats))
  }

  fn collect_generation_artifacts(&self) -> GenerationArtifacts {
    let mut stats = GenerationStats::default();
    let mut schema_graph = SchemaRegistry::new(&self.spec, &mut stats);

    let mut cache = SharedSchemaCache::new();
    cache.initialize_from_schemas(schema_graph.schemas());
    let union_fingerprints = cache.union_fingerprints().clone();

    let (cycle_info, filtered_schemas) = schema_graph.initialize(
      &self.operation_registry,
      self.config.include_all_schemas(),
      &union_fingerprints,
    );

    let schema_graph = Arc::new(schema_graph);
    let schema_names = schema_graph.scan_and_compute_names().unwrap_or_default();
    let filtered_schemas = filtered_schemas.map(Arc::new);

    cache.set_precomputed_names(
      schema_names.names,
      schema_names.enum_names,
      schema_names.schema_metadata,
    );

    let context = Rc::new(ConverterContext::new(
      schema_graph.clone(),
      self.config.clone(),
      cache,
      filtered_schemas.clone(),
    ));

    let converter = SchemaConverter::new(&context);
    let mut rust_types = converter.convert_all_schemas(filtered_schemas.as_deref(), &mut stats);

    let processor = OperationsProcessor::new(context.clone(), &converter);
    let operation_results = processor.process_all(self.operation_registry.operations());

    rust_types.extend(operation_results.types);
    rust_types.extend(context.cache.borrow_mut().take_types());

    if context.config.zero_copy_enabled() {
      Self::propagate_lifetimes(&mut rust_types);
    }

    stats.record_orphaned_schemas(if let Some(ref schemas) = filtered_schemas {
      let total = schema_graph.keys().len();
      total.saturating_sub(schemas.len())
    } else {
      0
    });

    stats.record_warnings(operation_results.warnings);
    stats.record_rust_types(&rust_types);
    stats.record_operations(&operation_results.operations);
    stats.record_cycles(cycle_info);
    stats.record_client_methods(operation_results.operations.len());
    stats.record_client_headers(operation_results.unique_headers.len());

    GenerationArtifacts {
      rust_types,
      operations_info: operation_results.operations,
      serde_recorder: operation_results.usage_recorder,
      unique_headers: operation_results.unique_headers.into_iter().collect::<Vec<_>>(),
      stats,
      config: context.config.clone(),
    }
  }

  fn propagate_lifetimes(rust_types: &mut [RustType]) {
    use super::ast::types::RustPrimitive;

    for ty in rust_types.iter_mut() {
      ty.set_requires_lifetime_if_needed();
    }

    let refs_lifetime = |base: &RustPrimitive, set: &HashSet<String>| -> bool {
      if let RustPrimitive::Custom(name) = base {
        set.contains(name.as_ref())
      } else {
        false
      }
    };

    loop {
      let lifetime_set = rust_types
        .iter()
        .filter(|t| match t {
          RustType::Struct(d) => d.requires_lifetime,
          RustType::Enum(d) => d.requires_lifetime,
          RustType::DiscriminatedEnum(d) => d.requires_lifetime,
          RustType::ResponseEnum(d) => d.requires_lifetime,
          RustType::TypeAlias(d) => d.requires_lifetime,
        })
        .map(|t| t.type_name().to_string())
        .collect::<HashSet<_>>();

      let mut changed = false;
      for ty in rust_types.iter_mut() {
        let (needs, already) = match ty {
          RustType::Struct(d) => (
            d.fields.iter().any(|f| refs_lifetime(&f.rust_type.base_type, &lifetime_set)),
            d.requires_lifetime,
          ),
          RustType::Enum(d) => (
            d.variants.iter().any(|v| {
              v.content
                .tuple_types()
                .is_some_and(|types| types.iter().any(|t| refs_lifetime(&t.base_type, &lifetime_set)))
            }),
            d.requires_lifetime,
          ),
          RustType::DiscriminatedEnum(d) => {
            let needs = d.variants.iter().any(|v| refs_lifetime(&v.type_name.base_type, &lifetime_set))
              || d.fallback.as_ref().is_some_and(|v| refs_lifetime(&v.type_name.base_type, &lifetime_set));
            (needs, d.requires_lifetime)
          }
          RustType::ResponseEnum(d) => (
            d.variants
              .iter()
              .any(|v| v.schema_type.as_ref().is_some_and(|t| refs_lifetime(&t.base_type, &lifetime_set))),
            d.requires_lifetime,
          ),
          RustType::TypeAlias(d) => (refs_lifetime(&d.target.base_type, &lifetime_set), d.requires_lifetime),
        };
        if needs && !already {
          match ty {
            RustType::Struct(d) => d.requires_lifetime = true,
            RustType::Enum(d) => d.requires_lifetime = true,
            RustType::DiscriminatedEnum(d) => d.requires_lifetime = true,
            RustType::ResponseEnum(d) => d.requires_lifetime = true,
            RustType::TypeAlias(d) => d.requires_lifetime = true,
          }
          changed = true;
        }
      }

      if !changed {
        break;
      }
    }
  }
}
