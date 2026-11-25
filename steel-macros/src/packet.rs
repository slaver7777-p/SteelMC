use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{DeriveInput, Ident, Meta, parse_macro_input};

pub fn client_packet_derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;

    // Find the packet_id attributes
    let attrs: Vec<_> = input
        .attrs
        .iter()
        .filter(|a| a.path().is_ident("packet_id"))
        .collect();

    assert!(
        !attrs.is_empty(),
        "ClientPacket derive macro requires at least one #[packet_id(...)] attribute"
    );

    // `#[legacy_payload]` opts this packet into a version-specific body, written by the
    // inherent `write_payload_legacy` method instead of the native `WriteTo` impl.
    let legacy_payload = input
        .attrs
        .iter()
        .any(|a| a.path().is_ident("legacy_payload"));

    let mut match_arms = Vec::new();

    for attr in attrs {
        if let Meta::List(meta) = attr.meta.clone() {
            meta.parse_nested_meta(|meta| {
                let state = meta
                    .path
                    .get_ident()
                    .expect("Expected an identifier for the protocol state")
                    .to_string();
                let value: syn::Expr = meta.value()?.parse()?;
                let state_ident = Ident::new(&state, Span::call_site());

                let arm = quote! {
                    crate::utils::ConnectionProtocol::#state_ident => Some(#value),
                };
                match_arms.push(arm);

                Ok(())
            })
            .unwrap_or_else(|e| panic!("Failed to parse `packet_id` attribute: {e}"));
        } else {
            panic!("`packet_id` attribute must be a list: `#[packet_id(STATE = \"path\", ...)]`");
        }
    }

    let (impl_generics, ty_generics, _) = input.generics.split_for_impl();

    let write_payload = if legacy_payload {
        quote! {
            fn write_payload(
                &self,
                writer: &mut impl ::std::io::Write,
                version: crate::version::MinecraftVersion,
            ) -> ::std::result::Result<(), crate::utils::PacketError> {
                Self::write_payload_legacy(self, writer, version)
                    .map_err(crate::utils::PacketError::from)
            }
        }
    } else {
        quote! {}
    };

    let expanded = quote! {
        #[automatically_derived]
        impl #impl_generics crate::packet_traits::ClientPacket for #name #ty_generics {
            fn get_id(&self, protocol: crate::utils::ConnectionProtocol) -> Option<i32> {
                match protocol {
                    #(#match_arms)*
                    _ => None,
                }
            }

            #write_payload
        }
    };

    TokenStream::from(expanded)
}

/// Derives the `ServerPacket` trait for a struct.
pub fn server_packet_derive(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;

    // `#[legacy_payload]` opts this packet into reading a version-specific body through the
    // inherent `read_payload_legacy` method instead of its native `ReadFrom` impl.
    let legacy_payload = input
        .attrs
        .iter()
        .any(|a| a.path().is_ident("legacy_payload"));

    let read_packet_for = if legacy_payload {
        quote! {
            fn read_packet_for(
                data: &mut ::std::io::Cursor<&[u8]>,
                version: crate::version::MinecraftVersion,
            ) -> ::std::result::Result<Self, crate::utils::PacketError> {
                Self::read_payload_legacy(data, version).map_err(crate::utils::PacketError::from)
            }
        }
    } else {
        quote! {}
    };

    let expanded = quote! {
        #[automatically_derived]
        impl crate::packet_traits::ServerPacket for #name {
            #read_packet_for
        }
    };

    TokenStream::from(expanded)
}
