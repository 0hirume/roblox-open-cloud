use proc_macro2::TokenStream;

use crate::{CodegenError, Result};

pub(super) fn render(tokens: TokenStream) -> Result<String> {
    let file = syn::parse2(tokens).map_err(|source| Box::new(CodegenError::Rust(source)))?;
    Ok(prettyplease::unparse(&file))
}
