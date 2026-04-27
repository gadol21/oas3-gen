use proc_macro2::TokenStream;
use quote::{ToTokens, quote};

use super::Visibility;
use crate::generator::ast::{RustPrimitive, TypeAliasDef, TypeRef};

#[derive(Clone, Debug)]
pub(crate) struct TypeAliasFragment {
  def: TypeAliasDef,
  visibility: Visibility,
}

impl TypeAliasFragment {
  pub(crate) fn new(def: TypeAliasDef, visibility: Visibility) -> Self {
    Self { def, visibility }
  }
}

impl ToTokens for TypeAliasFragment {
  fn to_tokens(&self, tokens: &mut TokenStream) {
    let name = &self.def.name;
    let docs = &self.def.docs;
    let vis = &self.visibility;

    if self.def.requires_lifetime {
      let target: TokenStream = if let RustPrimitive::Custom(ref custom_name) = self.def.target.base_type {
        let lifetimed = TypeRef {
          base_type: RustPrimitive::Custom(format!("{custom_name}<'a>").into()),
          ..self.def.target.clone()
        };
        lifetimed.to_token_stream()
      } else {
        self.def.target.to_token_stream()
      };

      tokens.extend(quote! {
        #docs
        #vis type #name<'a> = #target;
      });
    } else {
      let target = &self.def.target;
      tokens.extend(quote! {
        #docs
        #vis type #name = #target;
      });
    }
  }
}
