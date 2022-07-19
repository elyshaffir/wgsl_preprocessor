use proc_macro::TokenStream;
use quote::{quote, quote_spanned};
use syn;

#[proc_macro_derive(WGSLType)]
pub fn wgsl_type_derive(input: TokenStream) -> TokenStream {
	// Construct a representation of Rust code as a syntax tree
	// that we can manipulate
	let syn::DeriveInput { ident, data, .. } = syn::parse_macro_input!(input);

	let description = match data {
		syn::Data::Struct(s) => match s.fields {
			syn::Fields::Named(syn::FieldsNamed { named, .. }) => {
				let idents = named.iter().map(|f| (&f.ty)).last();
				format!("{}", quote! {#idents.type_name()})
			}
			_ => quote_spanned! {
				ident.span() =>
				compile_error!("Couldn't properly iterate the struct's fields in order to generate them in WGSL code."); // todo
			}
			.to_string(),
		},
		_ => quote_spanned! {
			ident.span() =>
			compile_error!("Cannot derive for non-structs.");
		}
		.to_string(),
	};

	let output = quote! {
		impl WGSLType for #ident {
			fn type_name() -> String {
				stringify!(#ident).to_string()
			}

			fn declaration() -> String {
				format!("struct {} {{{}}};", Self::type_name(), #description)
			}

			fn definition(&self) -> String {
				format!("{}({})", Self::type_name(), #description)
			}
		}
	};

	output.into()
}
