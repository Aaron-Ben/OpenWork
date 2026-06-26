use proc_macro::TokenStream;
use quote::{ToTokens, format_ident, quote};
use syn::{Data, DeriveInput, Expr, Fields, Lit, Meta, Type, parse_macro_input};

#[proc_macro_derive(PgEntity, attributes(table_name, primary_key, indexed))]
pub fn derive_pg_entity(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    expand_pg_entity(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn expand_pg_entity(input: DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let ident = input.ident;
    let ident_for_error = ident.clone();
    let row_ident = format_ident!("{ident}Row");
    let table_name = table_name(&input.attrs).unwrap_or_else(|| ident.to_string().to_lowercase());
    let fields = match input.data {
        Data::Struct(data) => match data.fields {
            Fields::Named(fields) => fields.named,
            fields => {
                return Err(syn::Error::new_spanned(
                    fields,
                    "PgEntity only supports structs with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                ident_for_error,
                "PgEntity only supports structs",
            ));
        }
    };

    let mut columns = Vec::new();
    let mut indexes = Vec::new();
    let mut field_defs = Vec::new();
    let mut insert_binds = Vec::new();
    let mut update_binds = Vec::new();
    let mut from_row_assignments = Vec::new();
    let mut primary_key_field = None;

    for field in fields {
        let Some(field_ident) = field.ident else {
            continue;
        };
        let field_ty = field.ty;
        let name = field_ident.to_string();
        let rust_type = field_ty.to_token_stream().to_string();
        let is_primary_key = field
            .attrs
            .iter()
            .any(|attr| attr.path().is_ident("primary_key"));
        let indexed = field
            .attrs
            .iter()
            .any(|attr| attr.path().is_ident("indexed"));
        let nullable = is_option(&field_ty);

        if is_primary_key {
            primary_key_field = Some((field_ident.clone(), field_ty.clone(), name.clone()));
        }

        columns.push(quote! {
            ::openwork_database::PgColumn {
                name: #name,
                rust_type: #rust_type,
                primary_key: #is_primary_key,
                indexed: #indexed,
                nullable: #nullable,
            }
        });

        if indexed {
            let index_name = format!("{table_name}_{name}_idx");
            indexes.push(quote! {
                ::openwork_database::PgIndex {
                    name: #index_name,
                    columns: &[#name],
                }
            });
        }

        field_defs.push(quote! {
            pub #field_ident: #field_ty
        });
        insert_binds.push(quote! {
            .bind(&self.#field_ident)
        });
        if !is_primary_key {
            update_binds.push(quote! {
                .bind(&self.#field_ident)
            });
        }
        from_row_assignments.push(quote! {
            #field_ident: row.#field_ident
        });
    }

    let Some((primary_key_ident, primary_key_ty, primary_key_name)) = primary_key_field else {
        return Err(syn::Error::new_spanned(
            ident,
            "PgEntity requires exactly one field marked #[primary_key]",
        ));
    };

    Ok(quote! {
        #[derive(::sqlx::FromRow, Debug, Clone)]
        #[automatically_derived]
        pub struct #row_ident {
            #(#field_defs),*
        }

        impl ::openwork_database::PgSchema for #ident {
            type Id = #primary_key_ty;
            type Row = #row_ident;

            const TABLE: &'static str = #table_name;
            const ID_COLUMN: &'static str = #primary_key_name;
            const COLUMNS: &'static [::openwork_database::PgColumn] = &[#(#columns),*];
            const INDEXES: &'static [::openwork_database::PgIndex] = &[#(#indexes),*];

            fn get_id_value(&self) -> Self::Id {
                self.#primary_key_ident.clone()
            }

            fn from_row(row: Self::Row) -> Self {
                Self {
                    #(#from_row_assignments),*
                }
            }
        }

        #[::async_trait::async_trait]
        impl ::openwork_database::PgCrud for #ident {
            fn bind_insert<'q>(
                &'q self,
                query: ::sqlx::query::QueryAs<
                    'q,
                    ::sqlx::Postgres,
                    <Self as ::openwork_database::PgSchema>::Row,
                    ::sqlx::postgres::PgArguments,
                >,
            ) -> ::sqlx::query::QueryAs<
                'q,
                ::sqlx::Postgres,
                <Self as ::openwork_database::PgSchema>::Row,
                ::sqlx::postgres::PgArguments,
            > {
                query #(#insert_binds)*
            }

            fn bind_update<'q>(
                &'q self,
                query: ::sqlx::query::QueryAs<
                    'q,
                    ::sqlx::Postgres,
                    <Self as ::openwork_database::PgSchema>::Row,
                    ::sqlx::postgres::PgArguments,
                >,
            ) -> ::sqlx::query::QueryAs<
                'q,
                ::sqlx::Postgres,
                <Self as ::openwork_database::PgSchema>::Row,
                ::sqlx::postgres::PgArguments,
            > {
                query #(#update_binds)* .bind(&self.#primary_key_ident)
            }
        }
    })
}

fn table_name(attrs: &[syn::Attribute]) -> Option<String> {
    attrs.iter().find_map(|attr| {
        if !attr.path().is_ident("table_name") {
            return None;
        }
        match &attr.meta {
            Meta::NameValue(name_value) => match &name_value.value {
                Expr::Lit(expr_lit) => match &expr_lit.lit {
                    Lit::Str(value) => Some(value.value()),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        }
    })
}

fn is_option(ty: &Type) -> bool {
    let Type::Path(path) = ty else {
        return false;
    };
    path.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "Option")
}
